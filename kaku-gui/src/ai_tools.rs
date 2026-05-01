//! Built-in tools for the Kaku AI chat overlay.
//!
//! Implements the OpenAI function-calling schema so the model can read/write files,
//! list directories, run shell commands, and more, all without leaving the terminal.

use crate::ai_client::AssistantConfig;
use anyhow::{Context, Result};
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{BufRead, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// SIGKILL the entire process group led by this child. Required because
/// `Child::kill()` only signals the direct child (the login shell), which
/// leaves any grandchild like `sleep 30` still running.
fn kill_process_group(child: &std::process::Child) {
    unsafe {
        libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
    }
}

// ─── Background process registry ─────────────────────────────────────────────

struct BgProcess {
    child: std::process::Child,
    /// Stdout and stderr are piped to reader threads at spawn time. Both streams
    /// write into this shared buffer so shell_poll never blocks on read().
    output: Arc<Mutex<String>>,
}

impl Drop for BgProcess {
    fn drop(&mut self) {
        // Kill the process group and wait so the child doesn't linger as a
        // zombie. try_wait() alone is a no-op for still-running children.
        kill_process_group(&self.child);
        let _ = self.child.wait();
    }
}

static BG_PROCS: OnceLock<Mutex<HashMap<u32, BgProcess>>> = OnceLock::new();

fn bg_registry() -> &'static Mutex<HashMap<u32, BgProcess>> {
    BG_PROCS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Spawn a reader thread that drains `reader` into `buf`, up to `cap` bytes.
///
/// The shared `bytes_total` counter is incremented for every byte read (across
/// all sibling reader threads). Once the cumulative total reaches `cap`, the
/// thread continues reading from the pipe (to prevent the child from blocking
/// on a full pipe buffer) but stops writing to `buf`.
///
/// Returns a `JoinHandle` so callers can wait for the thread to finish.
fn pump_reader_capped<R: Read + Send + 'static>(
    reader: R,
    buf: Arc<Mutex<String>>,
    bytes_total: Arc<AtomicUsize>,
    cap: usize,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut r = reader;
        let mut chunk = [0u8; 4096];
        loop {
            match r.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let prev = bytes_total.fetch_add(n, Ordering::Relaxed);
                    if prev < cap {
                        let writable = (cap - prev).min(n);
                        let text = String::from_utf8_lossy(&chunk[..writable]).into_owned();
                        if let Ok(mut g) = buf.lock() {
                            g.push_str(&text);
                        }
                    }
                    // After cap is reached: keep reading so the child is not
                    // stalled waiting for us to consume its pipe buffer.
                }
            }
        }
    })
}

/// JSON-schema description of one tool, ready to pass to the API.
pub struct ToolDef {
    pub name: &'static str,
    pub description: Cow<'static, str>,
    /// JSON Schema for the function's parameters.
    pub parameters: serde_json::Value,
}

/// Returns the path to the memory file used by memory_read / curator writes.
pub(crate) fn memory_file_path() -> std::path::PathBuf {
    crate::soul::memory_path()
}

/// Presence of this file means the user has already seen the onboarding greeting.
#[allow(dead_code)]
pub(crate) fn onboarding_flag_path() -> std::path::PathBuf {
    // Keep the flag in the config root (not inside soul/) so the soul dir can
    // be wiped cleanly without losing the onboarding state.
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        std::path::PathBuf::from(xdg)
            .join("kaku")
            .join("ai_chat_onboarded")
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        std::path::PathBuf::from(home)
            .join(".config")
            .join("kaku")
            .join("ai_chat_onboarded")
    }
}

/// All tools exposed to the model, filtered by the active configuration.
pub fn all_tools(config: &AssistantConfig) -> Vec<ToolDef> {
    let mut tools = vec![
        ToolDef {
            name: "fs_read",
            description: Cow::Borrowed(
                "Read a file and return its content. By default returns the whole file up to the \
                 output cap. Use start_line / end_line to read a specific range (1-indexed, \
                 inclusive). Efficient for large files when you only need a section.",
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute or ~/relative path" },
                    "start_line": {
                        "type": "integer",
                        "description": "First line to return (1 = first line of file). Optional."
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "Last line to return (inclusive). Optional."
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "fs_list",
            description: Cow::Borrowed("List files and sub-directories inside a directory. \
                          Directories are shown with a trailing /."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path" }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "fs_write",
            description: Cow::Borrowed("Write (create or overwrite) a file with the given content. \
                          Parent directories are created automatically."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path":    { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDef {
            name: "fs_patch",
            description: Cow::Borrowed("Replace the first occurrence of `old_text` with `new_text` in a file. \
                          Fails if old_text is not found."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path":     { "type": "string" },
                    "old_text": { "type": "string", "description": "Exact text to find" },
                    "new_text": { "type": "string", "description": "Replacement text" }
                },
                "required": ["path", "old_text", "new_text"]
            }),
        },
        ToolDef {
            name: "fs_mkdir",
            description: Cow::Borrowed("Create a directory and all missing parent directories."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "fs_delete",
            description: Cow::Borrowed("Delete a file or directory (recursive for directories)."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        },
        ToolDef {
            name: "shell_exec",
            description: Cow::Borrowed(
                "Run an arbitrary shell command via bash and return stdout + stderr. \
                 Use for building, testing, grepping, git, npm, cargo, etc. \
                 Output is capped; for commands that produce large output or run \
                 indefinitely, use shell_bg + shell_poll instead.",
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute (passed to bash -c)"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory override (optional, defaults to pane cwd)"
                    },
                    "detail": {
                        "type": "string",
                        "enum": ["brief", "default", "full"],
                        "description": "Output size: 'brief' for summaries, 'default' (standard cap), \
                                        'full' for deep inspection. Default: 'default'."
                    }
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "shell_bg",
            description: Cow::Borrowed("Start a long-running shell command in the background and return its process id immediately. \
                          Use for commands that take minutes (builds, dev servers, watchers). \
                          Call shell_poll to check status and collect output."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to run in background"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory (optional)"
                    }
                },
                "required": ["command"]
            }),
        },
        ToolDef {
            name: "shell_poll",
            description: Cow::Borrowed("Check the status of a background process started with shell_bg. \
                          Returns accumulated stdout/stderr and whether the process has exited. \
                          Pass timeout_secs > 0 to wait up to that many seconds for it to finish."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pid": {
                        "type": "integer",
                        "description": "Process id returned by shell_bg"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Seconds to wait for process exit (0 = non-blocking check)"
                    }
                },
                "required": ["pid"]
            }),
        },
        ToolDef {
            name: "pwd",
            description: Cow::Borrowed("Return the current working directory of the terminal pane."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
    ];

    // web_fetch is always available when tools are enabled.
    tools.push(ToolDef {
        name: "web_fetch",
        description: Cow::Borrowed(
            "Fetch a URL and return its content as Markdown. \
             Uses defuddle.md then r.jina.ai as free anonymous backends. \
             Use for reading documentation, articles, or any public web page.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Full URL to fetch (must start with http:// or https://)"
                },
                "detail": {
                    "type": "string",
                    "enum": ["brief", "default", "full"],
                    "description": "Output size. Default: 'default'."
                }
            },
            "required": ["url"]
        }),
    });

    // web_search is opt-in: only registered when provider + key are configured.
    if config.web_search_ready() {
        let provider = config.web_search_provider.as_deref().unwrap_or("search");
        tools.push(ToolDef {
            name: "web_search",
            description: Cow::Owned(format!(
                "Search the web using {} and return results with title, URL, snippet, and (where supported) \
                 a direct AI answer. Use for finding current information, documentation, or answering questions. \
                 Use kind='news' for recent events; kind='deep' (pipellm) for richer RAG results; \
                 freshness to limit by recency.",
                provider
            )),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["web", "news", "deep"],
                        "description": "'web' (default), 'news' (recent news), or 'deep' (pipellm: full RAG pipeline)"
                    },
                    "freshness": {
                        "type": "string",
                        "description": "Recency filter: 'pd' (24h), 'pw' (7d), 'pm' (31d), 'py' (1y). \
                                        Brave also accepts custom ranges like '2024-01-01to2024-06-30'."
                    },
                    "search_depth": {
                        "type": "string",
                        "enum": ["basic", "advanced"],
                        "description": "Tavily only. 'advanced' performs deeper crawling for richer results."
                    }
                },
                "required": ["query"]
            }),
        });

        // read_url: read a specific URL and return its clean text content.
        // Uses provider-native readers where available, falls back to generic fetchers.
        tools.push(ToolDef {
            name: "read_url",
            description: Cow::Borrowed(
                "Fetch a web page and return its clean text content, optimized for AI reading. \
                 Use after web_search to read the full content of a promising result. \
                 Handles JS-heavy pages better than web_fetch for supported providers.",
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Full URL to read (must start with http:// or https://)"
                    }
                },
                "required": ["url"]
            }),
        });
    }

    // project_summary: zero-cost project context from marker files.
    tools.push(ToolDef {
        name: "project_summary",
        description: Cow::Borrowed(
            "Scan a directory for project markers (Cargo.toml, package.json, go.mod, \
             Makefile, .git, etc.) and return a brief summary: language, build system, \
             key directories, entry points. Call this first on unfamiliar codebases.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to scan (defaults to cwd)"
                }
            },
            "required": []
        }),
    });

    // file_tree: directory tree with depth limit, respecting .gitignore.
    tools.push(ToolDef {
        name: "file_tree",
        description: Cow::Borrowed(
            "List the directory tree up to a given depth, skipping .git, node_modules, \
             target, and other common noise directories. Useful for understanding project \
             structure before searching.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Root directory (defaults to cwd)"
                },
                "depth": {
                    "type": "integer",
                    "description": "Maximum depth to recurse (default 3, max 6)"
                }
            },
            "required": []
        }),
    });

    // symbol_search: language-aware definition search using rg.
    tools.push(ToolDef {
        name: "symbol_search",
        description: Cow::Borrowed(
            "Find symbol definitions (functions, types, traits, classes, methods) by name. \
             More precise than grep_search for code navigation because it uses language-aware \
             patterns to match definitions, not arbitrary occurrences.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Symbol name or partial name to search for"
                },
                "kind": {
                    "type": "string",
                    "enum": ["function", "type", "class", "method", "all"],
                    "description": "Kind of symbol to find (default: all)"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (defaults to cwd)"
                },
                "glob": {
                    "type": "string",
                    "description": "File glob filter, e.g. '*.rs' or '*.{ts,tsx}'"
                }
            },
            "required": ["query"]
        }),
    });

    // grep_search: fast recursive text/regex search across files.
    tools.push(ToolDef {
        name: "grep_search",
        description: Cow::Borrowed(
            "Recursively search for a regex pattern in files and return matching lines with context. \
             Use for finding symbol definitions, usages, TODO comments, or any text pattern across \
             the codebase. Faster and more precise than reading individual files.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regular expression pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search in (defaults to cwd)"
                },
                "glob": {
                    "type": "string",
                    "description": "File glob filter, e.g. '*.rs' or '*.{ts,tsx}' (optional)"
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Lines of context before and after each match (default 2)"
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Case-insensitive matching (default false)"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matching lines to return (default 100)"
                },
                "detail": {
                    "type": "string",
                    "enum": ["brief", "default", "full"],
                    "description": "Output size. Default: 'default'."
                }
            },
            "required": ["pattern"]
        }),
    });

    // memory_read: read-only access to the rolling MEMORY.md file. Writes are
    // handled exclusively by the background curator so there is a single writer.
    tools.push(ToolDef {
        name: "memory_read",
        description: Cow::Borrowed(
            "Read the rolling memory file that stores persistent facts, \
             preferences, and project context across AI chat sessions. \
             Kaku updates this file automatically after each conversation; \
             you do not need to write to it yourself.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
    });

    // soul_read: read-only access to the user-authored soul identity files.
    // The user edits these directly; the curator never writes to them.
    tools.push(ToolDef {
        name: "soul_read",
        description: Cow::Borrowed(
            "Read one of the user's soul identity files. These are stable, \
             user-authored documents that describe who the user is (soul), \
             their preferred style (style), and how they work (skill). \
             Call this when you need to recall a specific identity detail \
             not already present in the system prompt.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "file": {
                    "type": "string",
                    "enum": ["soul", "style", "skill", "memory"],
                    "description": "Which soul file to read. Omit to read all four."
                }
            },
            "required": []
        }),
    });

    // http_request: generic HTTP client for API calls.
    tools.push(ToolDef {
        name: "http_request",
        description: Cow::Borrowed(
            "Make an HTTP request (GET, POST, PUT, PATCH, DELETE) and return the response status, \
             headers, and body. Use for testing APIs, fetching JSON endpoints, or any HTTP call \
             that requires a specific method or request body. For web pages, prefer web_fetch instead.",
        ),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "method": {
                    "type": "string",
                    "enum": ["GET", "POST", "PUT", "PATCH", "DELETE"],
                    "description": "HTTP method"
                },
                "url": {
                    "type": "string",
                    "description": "Full URL (must start with http:// or https://)"
                },
                "headers": {
                    "type": "object",
                    "description": "Optional extra request headers as key-value pairs",
                    "additionalProperties": { "type": "string" }
                },
                "body": {
                    "type": "string",
                    "description": "Request body (for POST/PUT/PATCH). If it is valid JSON, \
                                   Content-Type is set to application/json automatically."
                },
                "query": {
                    "type": "object",
                    "description": "Optional URL query parameters as key-value pairs",
                    "additionalProperties": { "type": "string" }
                }
            },
            "required": ["method", "url"]
        }),
    });

    tools
}

/// Serialize a ToolDef into the JSON object expected by the OpenAI API.
pub fn to_api_schema(tool: &ToolDef) -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.parameters,
        }
    })
}

/// Fallback output cap for tools not matched in `budget_for`.
const DEFAULT_RESULT_BYTES: usize = 8_000;

/// Hard wall-clock ceiling for a single `shell_exec` invocation. Anything
/// slower should be launched via `shell_bg` + `shell_poll` instead; 60s is
/// long enough for `cargo check` on a warm cache but stops a hung `grep -r`
/// from blocking the whole agent loop indefinitely.
const SHELL_EXEC_TIMEOUT_SECS: u64 = 60;

/// Wall-clock ceiling for grep_search / symbol_search. These tools spawn rg or
/// grep, which can stall on very large trees or network-mounted filesystems.
const SEARCH_TIMEOUT_SECS: u64 = 30;

/// Per-tool byte budgets for tool-call results.
///
/// `detail` maps to the budget tier:
///   "brief"   -> half the default (faster, shorter answers)
///   "default" -> normal cap (the zero-arg / unspecified case)
///   "full"    -> expanded cap for deep inspection
fn budget_for(tool: &str, detail: &str) -> usize {
    let (default_bytes, max_bytes): (usize, usize) = match tool {
        "fs_list" | "pwd" | "memory_read" | "soul_read" | "project_summary" => (2_000, 4_000),
        "fs_read" | "grep_search" | "symbol_search" => (8_000, 16_000),
        "file_tree" => (4_000, 8_000),
        "shell_exec" | "shell_poll" => (12_000, 24_000),
        "web_fetch" | "read_url" => (10_000, 20_000),
        "shell_bg" => (8_000, 8_000),
        _ => (DEFAULT_RESULT_BYTES, DEFAULT_RESULT_BYTES),
    };
    match detail {
        "brief" => default_bytes / 2,
        "full" => max_bytes,
        _ => default_bytes,
    }
}

/// Execute a tool by name. `args` is the parsed JSON from the model.
/// `cwd` is the agent's current working directory; shell_exec updates it in-place
/// when the command changes directory (e.g. via `cd`).
///
/// `cancel` is polled by long-running tools (currently shell_exec) so Esc /
/// session shutdown can interrupt a hung child process.
pub fn execute(
    name: &str,
    args: &serde_json::Value,
    cwd: &mut String,
    config: &AssistantConfig,
    cancel: &Arc<AtomicBool>,
) -> Result<String> {
    // Per-tool byte cap, honoring any optional `detail` argument.
    let detail = args["detail"].as_str().unwrap_or("default");
    let cap = budget_for(name, detail);

    let result = match name {
        "fs_read" => {
            let raw_path = args["path"].as_str().context("missing path")?;
            let path = resolve(raw_path, cwd)?;
            reject_if_sensitive(&path)?;
            // For relative paths, ensure they don't escape the working directory
            // (e.g. ../../.ssh/id_rsa). Absolute paths and ~/... are always allowed.
            if !raw_path.starts_with('/') && !raw_path.starts_with("~/") {
                let canon_path = std::fs::canonicalize(&path)
                    .with_context(|| format!("resolve '{}' inside working directory", raw_path))?;
                let canon_cwd = std::fs::canonicalize(&cwd)
                    .with_context(|| format!("resolve working directory '{}'", cwd))?;
                if !canon_path.starts_with(&canon_cwd) {
                    anyhow::bail!(
                        "path '{}' resolves outside the working directory; \
                         use an absolute path to access it",
                        raw_path
                    );
                }
            }
            let file =
                std::fs::File::open(&path).with_context(|| format!("read {}", path.display()))?;

            let start_line = args["start_line"].as_u64().map(|n| n as usize);
            let end_line = args["end_line"].as_u64().map(|n| n as usize);

            if start_line.is_some() || end_line.is_some() {
                // Line-range mode: stream with BufReader so we never load the
                // whole file into memory.
                let reader = std::io::BufReader::new(file);
                let start = start_line.unwrap_or(1);
                let end = end_line.unwrap_or(usize::MAX);
                let mut out = String::new();
                let mut line_num = 1usize;
                for line_result in reader.lines() {
                    let line = line_result.with_context(|| format!("read {}", path.display()))?;
                    if line_num < start {
                        line_num += 1;
                        continue;
                    }
                    if line_num > end {
                        break;
                    }
                    out.push_str(&line);
                    out.push('\n');
                    if out.len() >= cap {
                        out.push_str(&format!(
                            "[truncated: output exceeded {} bytes at line {}]",
                            cap, line_num
                        ));
                        break;
                    }
                    line_num += 1;
                }
                if out.is_empty() {
                    format!(
                        "(no content in lines {}..={})",
                        start,
                        end_line
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "EOF".into())
                    )
                } else {
                    out
                }
            } else {
                // Full-file mode: read at most cap + 512 bytes from disk.
                // The +512 gives slack to find a valid UTF-8 char boundary.
                let mut buf = Vec::with_capacity(cap + 512);
                file.take((cap + 512) as u64)
                    .read_to_end(&mut buf)
                    .with_context(|| format!("read {}", path.display()))?;
                String::from_utf8_lossy(&buf).into_owned()
            }
        }
        "fs_list" => {
            let path = resolve(args["path"].as_str().context("missing path")?, cwd)?;
            reject_if_sensitive(&path)?;
            let mut entries: Vec<String> = std::fs::read_dir(&path)
                .with_context(|| format!("list {}", path.display()))?
                .filter_map(|e| e.ok())
                .map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        format!("{}/", name)
                    } else {
                        name
                    }
                })
                .collect();
            entries.sort();
            entries.join("\n")
        }
        "fs_write" => {
            let path = resolve(args["path"].as_str().context("missing path")?, cwd)?;
            reject_if_sensitive(&path)?;
            let content = args["content"].as_str().context("missing content")?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, content).with_context(|| format!("write {}", path.display()))?;
            format!("Written {} bytes to {}", content.len(), path.display())
        }
        "fs_patch" => {
            let path = resolve(args["path"].as_str().context("missing path")?, cwd)?;
            reject_if_sensitive(&path)?;
            let old_text = args["old_text"].as_str().context("missing old_text")?;
            let new_text = args["new_text"].as_str().context("missing new_text")?;
            let original = std::fs::read_to_string(&path)
                .with_context(|| format!("read {}", path.display()))?;
            if !original.contains(old_text) {
                anyhow::bail!("old_text not found in {}", path.display());
            }
            let patched = original.replacen(old_text, new_text, 1);
            std::fs::write(&path, &patched).with_context(|| format!("write {}", path.display()))?;
            format!("Patched {} (replaced 1 occurrence)", path.display())
        }
        "fs_mkdir" => {
            let path = resolve(args["path"].as_str().context("missing path")?, cwd)?;
            reject_if_sensitive(&path)?;
            std::fs::create_dir_all(&path).with_context(|| format!("mkdir {}", path.display()))?;
            format!("Created {}", path.display())
        }
        "fs_delete" => {
            let path = resolve(args["path"].as_str().context("missing path")?, cwd)?;
            reject_if_sensitive(&path)?;
            if path.is_dir() {
                std::fs::remove_dir_all(&path)
                    .with_context(|| format!("rmdir {}", path.display()))?;
            } else {
                std::fs::remove_file(&path).with_context(|| format!("rm {}", path.display()))?;
            }
            format!("Deleted {}", path.display())
        }
        "shell_exec" => {
            let command = args["command"].as_str().context("missing command")?;
            let exec_cwd = args["cwd"]
                .as_str()
                .map(|p| resolve(p, cwd))
                .transpose()?
                .unwrap_or_else(|| PathBuf::from(cwd.as_str()));
            // Write CWD to a temp file so it is never lost to output capping.
            // The stdout stream may be truncated for high-output commands, but
            // the temp file is written unconditionally after the command exits.
            // PID + nanosecond timestamp avoids collisions between rapid calls.
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            let cwd_tmp_path =
                std::env::temp_dir().join(format!("kaku_cwd_{}_{}.txt", std::process::id(), ts));
            let wrapped = format!(
                "{}; __kaku_rc=$?; printf '%s' \"$(pwd)\" > {}; exit $__kaku_rc",
                command,
                cwd_tmp_path.display()
            );
            // Use the user's login shell so nvm/conda/pyenv etc. are available.
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());

            // Reserve 512 bytes for tags/exit-code appended below, so the final
            // result stays at or under `cap` and the bottom truncation code won't fire.
            let streaming_cap = cap.saturating_sub(512);

            let mut child = std::process::Command::new(&shell)
                .arg("-l")
                .arg("-c")
                .arg(&wrapped)
                .current_dir(&exec_cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                // Put the child in its own process group so cancel/timeout can
                // signal every descendant, not just the login shell.
                .process_group(0)
                .spawn()
                .with_context(|| format!("shell exec failed ({})", shell))?;

            // Shared byte counter across both reader threads. When the cumulative
            // total reaches `streaming_cap`, readers drain the pipe but stop writing.
            let bytes_total = Arc::new(AtomicUsize::new(0));
            let stdout_buf = Arc::new(Mutex::new(String::new()));
            let stderr_buf = Arc::new(Mutex::new(String::new()));

            let h1 = child.stdout.take().map(|s| {
                pump_reader_capped(s, stdout_buf.clone(), bytes_total.clone(), streaming_cap)
            });
            let h2 = child.stderr.take().map(|s| {
                pump_reader_capped(s, stderr_buf.clone(), bytes_total.clone(), streaming_cap)
            });

            // Poll until the child exits, the user cancels, or the hard timeout
            // fires. When output exceeds the cap we stop buffering additional
            // bytes, but keep polling so cancel/timeout still work.
            let start = Instant::now();
            let timeout = Duration::from_secs(SHELL_EXEC_TIMEOUT_SECS);
            let mut canceled = false;
            let mut timed_out = false;
            let mut overflowed = false;
            loop {
                if cancel.load(Ordering::Relaxed) {
                    kill_process_group(&child);
                    canceled = true;
                    break;
                }
                if start.elapsed() >= timeout {
                    kill_process_group(&child);
                    timed_out = true;
                    break;
                }
                if !overflowed && bytes_total.load(Ordering::Relaxed) >= streaming_cap {
                    overflowed = true;
                }
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            // Wait for the child to finish, then join reader threads.
            let status = child.wait().ok();
            if let Some(h) = h1 {
                let _ = h.join();
            }
            if let Some(h) = h2 {
                let _ = h.join();
            }

            let stdout_raw = stdout_buf.lock().map(|g| g.clone()).unwrap_or_default();
            // Update CWD from the temp file (written regardless of output cap).
            if let Ok(new_dir) = std::fs::read_to_string(&cwd_tmp_path) {
                let new_dir = new_dir.trim().to_string();
                if !new_dir.is_empty() {
                    *cwd = new_dir;
                }
            }
            let _ = std::fs::remove_file(&cwd_tmp_path);
            // Strip any leftover inline __KAKU_CWD__ marker from stdout.
            let mut stdout_lines: Vec<&str> = stdout_raw.lines().collect();
            stdout_lines.retain(|l| !l.starts_with("__KAKU_CWD__:"));
            let mut out = stdout_lines.join("\n");
            if stdout_raw.ends_with('\n') && !out.ends_with('\n') {
                out.push('\n');
            }
            let stderr_str = stderr_buf.lock().map(|g| g.clone()).unwrap_or_default();
            if !stderr_str.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str("[stderr] ");
                out.push_str(&stderr_str);
            }
            if overflowed {
                let total = bytes_total.load(Ordering::Relaxed);
                out.push_str(&format!(
                    "\n[truncated: first ~{} bytes shown ({} total). \
                     For large output, use shell_bg + shell_poll to avoid waiting.]",
                    streaming_cap, total
                ));
            }
            if canceled {
                out.push_str("\n[canceled by user before completion]");
            }
            if timed_out {
                out.push_str(&format!(
                    "\n[killed: exceeded {}s timeout. For long-running commands \
                     use shell_bg + shell_poll; for searching code use grep_search.]",
                    SHELL_EXEC_TIMEOUT_SECS
                ));
            }
            // Always report non-zero exit code so the model knows when a command failed.
            // Skip when we killed the child ourselves: the signal-derived status
            // is noise compared to the canceled/timeout message already appended.
            if let Some(s) = status {
                if !s.success() && !canceled && !timed_out {
                    let code = s.code().unwrap_or(-1);
                    out.push_str(&format!("\n[exit {}]", code));
                }
            }
            if out.trim().is_empty() {
                "(no output)".into()
            } else {
                out
            }
        }
        "shell_bg" => {
            let command = args["command"].as_str().context("missing command")?;
            let exec_cwd = args["cwd"]
                .as_str()
                .map(|p| resolve(p, cwd))
                .transpose()?
                .unwrap_or_else(|| PathBuf::from(cwd.as_str()));
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
            let mut child = std::process::Command::new(&shell)
                .arg("-l")
                .arg("-c")
                .arg(command)
                .current_dir(&exec_cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                // Own process group so kill_process_group() reaches all descendants.
                .process_group(0)
                .spawn()
                .with_context(|| format!("failed to spawn background command: {}", command))?;
            let pid = child.id();
            let output = Arc::new(Mutex::new(String::new()));
            // Take stdout/stderr before inserting into the registry so the reader
            // threads own the pipes; shell_poll reads the shared buffer instead.
            // Cap the combined output to avoid unbounded memory growth for long-running
            // processes (e.g. `tail -f`, dev servers, `yes`).
            let bg_cap = budget_for("shell_bg", "default");
            let bg_bytes = Arc::new(AtomicUsize::new(0));
            if let Some(stdout) = child.stdout.take() {
                let _ = pump_reader_capped(stdout, output.clone(), bg_bytes.clone(), bg_cap);
            }
            if let Some(stderr) = child.stderr.take() {
                let _ = pump_reader_capped(stderr, output.clone(), bg_bytes.clone(), bg_cap);
            }
            bg_registry()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(pid, BgProcess { child, output });
            format!(
                "Background process started (pid {}). Use shell_poll to check status.",
                pid
            )
        }
        "shell_poll" => {
            let pid = args["pid"].as_u64().context("missing pid")? as u32;
            let timeout_secs = args["timeout_secs"].as_u64().unwrap_or(0);

            // Take a snapshot of the output buffer and do a non-blocking try_wait
            // while holding the registry lock for as short a time as possible.
            let (snapshot, status_opt) = {
                let mut registry = bg_registry().lock().unwrap_or_else(|e| e.into_inner());
                let proc = registry
                    .get_mut(&pid)
                    .ok_or_else(|| anyhow::anyhow!("no background process with pid {}", pid))?;
                let snap = proc
                    .output
                    .lock()
                    .ok()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                let status = proc.child.try_wait().ok().flatten();
                (snap, status)
            }; // registry lock released here

            // For timeout > 0: poll try_wait outside the registry lock so we do
            // not block other shell_bg / shell_poll calls during the wait.
            let final_status = if timeout_secs == 0 || status_opt.is_some() {
                status_opt
            } else {
                let deadline = Instant::now() + Duration::from_secs(timeout_secs);
                loop {
                    std::thread::sleep(Duration::from_millis(200));
                    if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
                        break None;
                    }
                    let mut registry = bg_registry().lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(proc) = registry.get_mut(&pid) {
                        if let Ok(Some(s)) = proc.child.try_wait() {
                            break Some(s);
                        }
                    } else {
                        break None;
                    }
                }
            };

            // If the process finished, grab the final output and remove it from the registry.
            let final_snapshot = if final_status.is_some() {
                let mut registry = bg_registry().lock().unwrap_or_else(|e| e.into_inner());
                let snap = registry
                    .get(&pid)
                    .and_then(|p| p.output.lock().ok().map(|g| g.clone()))
                    .unwrap_or(snapshot);
                registry.remove(&pid);
                snap
            } else {
                snapshot
            };

            let (done_str, exit_str): (String, String) = match final_status {
                Some(s) => {
                    let code = s.code().unwrap_or(-1);
                    ("done".into(), format!("[exit {}]", code))
                }
                None => ("running".into(), String::new()),
            };

            if final_snapshot.is_empty() {
                format!("pid {}: {} {}", pid, done_str, exit_str)
            } else {
                format!("pid {}: {} {}\n{}", pid, done_str, exit_str, final_snapshot)
            }
        }
        "pwd" => cwd.clone(),
        "web_fetch" => {
            let url = args["url"].as_str().context("missing url")?;
            if !url.starts_with("http://") && !url.starts_with("https://") {
                anyhow::bail!("url must start with http:// or https://");
            }
            if let Some(script) = &config.web_fetch_script {
                // Hidden escape hatch: run user's custom fetch script.
                let output = std::process::Command::new("bash")
                    .arg(script)
                    .arg("--")
                    .arg(url)
                    .output()
                    .context("web_fetch_script exec failed")?;
                if !output.status.success() {
                    anyhow::bail!(
                        "fetch script failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                String::from_utf8_lossy(&output.stdout).into_owned()
            } else {
                fetch_markdown_default(url)?
            }
        }
        "web_search" => {
            let query = args["query"].as_str().context("missing query")?;
            let provider = config
                .web_search_provider
                .as_deref()
                .context("web_search provider not configured")?;
            let api_key = config
                .web_search_api_key
                .as_deref()
                .context("web_search api key missing")?;
            let kind = args["kind"].as_str();
            let freshness = args["freshness"].as_str();
            let search_depth = args["search_depth"].as_str();
            match provider {
                "brave" => search_brave(query, api_key, kind, freshness)?,
                "pipellm" => search_pipellm(query, api_key, kind)?,
                "tavily" => search_tavily(query, api_key, kind, freshness, search_depth)?,
                _ => anyhow::bail!("unknown web_search provider: {}", provider),
            }
        }
        "read_url" => {
            let url = args["url"].as_str().context("missing url")?;
            if !url.starts_with("http://") && !url.starts_with("https://") {
                anyhow::bail!("url must start with http:// or https://");
            }
            let provider = config.web_search_provider.as_deref().unwrap_or("");
            let api_key = config.web_search_api_key.as_deref().unwrap_or("");
            exec_read_url(url, provider, api_key)?
        }
        "project_summary" => {
            let scan_path = args["path"]
                .as_str()
                .map(|p| resolve(p, cwd))
                .transpose()?
                .unwrap_or_else(|| PathBuf::from(cwd.as_str()));
            exec_project_summary(&scan_path)?
        }
        "file_tree" => {
            let tree_path = args["path"]
                .as_str()
                .map(|p| resolve(p, cwd))
                .transpose()?
                .unwrap_or_else(|| PathBuf::from(cwd.as_str()));
            let depth = args["depth"].as_u64().unwrap_or(3).min(6) as usize;
            exec_file_tree(&tree_path, depth)?
        }
        "symbol_search" => {
            let query = args["query"].as_str().context("missing query")?;
            let search_path = args["path"].as_str().unwrap_or(cwd);
            let kind = args["kind"].as_str().unwrap_or("all");
            let glob_filter = args["glob"].as_str();
            exec_symbol_search(query, kind, search_path, glob_filter, cwd, cancel)?
        }
        "grep_search" => {
            let pattern = args["pattern"].as_str().context("missing pattern")?;
            let search_path = args["path"].as_str().unwrap_or(cwd);
            let context_lines = args["context_lines"].as_u64().unwrap_or(2) as usize;
            let case_insensitive = args["case_insensitive"].as_bool().unwrap_or(false);
            let max_results = args["max_results"].as_u64().unwrap_or(100) as usize;
            let glob_filter = args["glob"].as_str();
            exec_grep_search(
                pattern,
                search_path,
                glob_filter,
                context_lines,
                case_insensitive,
                max_results,
                cwd,
                cancel,
            )?
        }
        "memory_read" => {
            let path = memory_file_path();
            match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(_) => "(no memories yet)".into(),
            }
        }
        "soul_read" => {
            let file = args["file"].as_str().unwrap_or("all");
            match file {
                "soul" => read_soul_file(&crate::soul::soul_path(), "SOUL"),
                "style" => read_soul_file(&crate::soul::style_path(), "STYLE"),
                "skill" => read_soul_file(&crate::soul::skill_path(), "SKILL"),
                "memory" => read_soul_file(&crate::soul::memory_path(), "MEMORY"),
                _ => {
                    let soul = read_soul_file(&crate::soul::soul_path(), "SOUL");
                    let style = read_soul_file(&crate::soul::style_path(), "STYLE");
                    let skill = read_soul_file(&crate::soul::skill_path(), "SKILL");
                    let memory = read_soul_file(&crate::soul::memory_path(), "MEMORY");
                    vec![soul, style, skill, memory]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<String>>()
                        .join("\n\n---\n\n")
                }
            }
        }
        "http_request" => {
            let method = args["method"].as_str().context("missing method")?;
            let url = args["url"].as_str().context("missing url")?;
            if !url.starts_with("http://") && !url.starts_with("https://") {
                anyhow::bail!("url must start with http:// or https://");
            }
            let body = args["body"].as_str();
            let headers = args["headers"].as_object();
            let query_params = args["query"].as_object();
            exec_http_request(method, url, headers, body, query_params)?
        }
        _ => anyhow::bail!("unknown tool: {}", name),
    };

    // Truncate oversized results so they don't exhaust the context window.
    // Spill the full content to a temp file so the model can fs_read it if needed.
    if result.len() > cap {
        let boundary = (0..=cap)
            .rev()
            .find(|&i| result.is_char_boundary(i))
            .unwrap_or(0);
        let truncated = &result[..boundary];
        // Write full content to a temp file for follow-up reads.
        let tmp_path = std::env::temp_dir().join(format!(
            "kaku_tool_{}_{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let note = if std::fs::write(&tmp_path, result.as_bytes()).is_ok() {
            if let Ok(mut registry) = SPILL_FILES.lock() {
                registry.push(tmp_path.clone());
            }
            format!(
                "\n[truncated: {} of {} bytes shown]\
                 \n[spill: {}]\
                 \n[hint: use fs_read(\"{}\") to read the rest]",
                cap,
                result.len(),
                tmp_path.display(),
                tmp_path.display()
            )
        } else {
            format!(
                "\n[truncated: {} bytes shown of {} total]",
                cap,
                result.len()
            )
        };
        Ok(format!("{}{}", truncated, note))
    } else {
        Ok(result)
    }
}

// ─── Soul helpers ─────────────────────────────────────────────────────────────

fn read_soul_file(path: &std::path::Path, label: &str) -> String {
    match std::fs::read_to_string(path) {
        Ok(content) if !content.trim().is_empty() => {
            format!("## {}\n\n{}", label, content.trim_end())
        }
        _ => String::new(),
    }
}

// ─── Spill-file cleanup ───────────────────────────────────────────────────────

static SPILL_FILES: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Remove all temp spill files created during this session.
pub fn cleanup_spill_files() {
    if let Ok(mut files) = SPILL_FILES.lock() {
        for path in files.drain(..) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

// ─── Web fetch helpers ────────────────────────────────────────────────────────

/// Shared HTTP client for all web tool calls (connection pool, keep-alive).
fn web_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|e| {
                log::warn!("web client build failed ({e}), falling back to default");
                reqwest::blocking::Client::new()
            })
    })
}

/// Maximum bytes we will buffer from any single HTTP fetch response.
/// Upstream Markdown converters (defuddle, jina) return article text that is
/// usually well under 100 KB, so this guard is mainly a safety net.
const MAX_FETCH_BYTES: usize = 512 * 1024; // 512 KB

/// Read at most `MAX_FETCH_BYTES` from a reqwest blocking Response.
/// reqwest::blocking::Response implements std::io::Read, so we can cap at the
/// source without buffering the full body.
fn read_response_capped(resp: reqwest::blocking::Response) -> Result<String> {
    let mut buf = Vec::with_capacity(MAX_FETCH_BYTES.min(64 * 1024));
    resp.take(MAX_FETCH_BYTES as u64)
        .read_to_end(&mut buf)
        .context("read HTTP response body")?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Read at most 4 KiB from an error response for logging / error messages.
/// Prevents a malicious or misbehaving server from forcing large allocations
/// on non-2xx paths where we only need a short diagnostic snippet.
fn read_error_body(resp: reqwest::blocking::Response) -> String {
    let mut buf = Vec::with_capacity(4096);
    let _ = resp.take(4096).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Fetch a URL as Markdown. Primary: defuddle.md. Fallback: r.jina.ai.
fn fetch_markdown_default(url: &str) -> Result<String> {
    let client = web_client();
    // Primary: defuddle.md, cleaner article extraction.
    if let Ok(resp) = client.get(format!("https://defuddle.md/{}", url)).send() {
        if resp.status().is_success() {
            if let Ok(body) = read_response_capped(resp) {
                if !body.trim().is_empty() {
                    return Ok(body);
                }
            }
        }
    }
    // Fallback: r.jina.ai, free anonymous Markdown converter.
    let resp = client
        .get(format!("https://r.jina.ai/{}", url))
        .send()
        .context("both defuddle.md and r.jina.ai unreachable")?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "fetch failed: defuddle.md and r.jina.ai both returned non-2xx (last: {})",
            resp.status()
        );
    }
    read_response_capped(resp).context("read fetch response body")
}

// ─── Web search providers ─────────────────────────────────────────────────────

fn search_brave(
    query: &str,
    api_key: &str,
    kind: Option<&str>,
    freshness: Option<&str>,
) -> Result<String> {
    // Use dedicated news endpoint when kind="news"; otherwise standard web search.
    let endpoint = if kind == Some("news") {
        "https://api.search.brave.com/res/v1/news/search"
    } else {
        "https://api.search.brave.com/res/v1/web/search"
    };
    let mut req = web_client()
        .get(endpoint)
        .query(&[("q", query), ("count", "10"), ("extra_snippets", "true")])
        .header("X-Subscription-Token", api_key)
        .header("Accept", "application/json");
    if let Some(f) = freshness {
        req = req.query(&[("freshness", f)]);
    }
    let resp = req.send().context("brave search request failed")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = read_error_body(resp);
        anyhow::bail!(
            "brave search returned {}: {}",
            status,
            body.chars().take(300).collect::<String>()
        );
    }
    let json: serde_json::Value = resp.json().context("parse brave response")?;
    // News endpoint returns json["results"]; web endpoint returns json["web"]["results"].
    let results = if kind == Some("news") {
        json["results"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or(&[])
    } else {
        json["web"]["results"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or(&[])
    };
    if results.is_empty() {
        return Ok("No results found.".into());
    }
    let mut out = String::new();
    for r in results.iter().take(10) {
        let title = r["title"].as_str().unwrap_or("(no title)");
        let url = r["url"].as_str().unwrap_or("");
        let desc = r["description"].as_str().unwrap_or("");
        out.push_str(&format!("- **{}** <{}>\n  {}\n", title, url, desc));
        // Surface extra_snippets if present (up to 3 to keep output concise).
        if let Some(extras) = r["extra_snippets"].as_array() {
            for snippet in extras.iter().take(3) {
                if let Some(s) = snippet.as_str() {
                    out.push_str(&format!("  > {}\n", s));
                }
            }
        }
    }
    Ok(out)
}

fn search_pipellm(query: &str, api_key: &str, kind: Option<&str>) -> Result<String> {
    // API uses GET with ?q= param (not POST+JSON).
    // Primary domain: api.pipellm.ai (console-facing gateway).
    // Fallback: api.pipellm.com (legacy, may be geo-filtered).
    // kind="deep" → /search (full RAG: content extraction + rerank); else simple-search.
    // kind="news" → /search-news (news-specific retrieval).
    let path = match kind {
        Some("news") => "v1/websearch/search-news",
        Some("deep") => "v1/websearch/search",
        _ => "v1/websearch/simple-search",
    };
    let domains = ["https://api.pipellm.ai", "https://api.pipellm.com"];
    let mut last_err = String::new();
    for base in &domains {
        let url = format!("{}/{}", base, path);
        let resp = match web_client()
            .get(&url)
            .query(&[("q", query)])
            .bearer_auth(api_key)
            .send()
        {
            Ok(r) => r,
            Err(e) => {
                last_err = e.to_string();
                continue;
            }
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let body = read_error_body(resp);
            last_err = format!(
                "{} from {}: {}",
                status,
                base,
                body.chars().take(300).collect::<String>()
            );
            continue;
        }
        let json: serde_json::Value = resp.json().context("parse pipellm response")?;
        // Serper-format: "organic" at root; some endpoints wrap it under "data".
        let results = json["organic"]
            .as_array()
            .or_else(|| json["data"]["organic"].as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        if results.is_empty() {
            return Ok("No results found.".into());
        }
        let mut out = String::new();
        for r in results.iter().take(10) {
            let title = r["title"].as_str().unwrap_or("(no title)");
            let url = r["link"]
                .as_str()
                .or_else(|| r["url"].as_str())
                .unwrap_or("");
            let snippet = r["snippet"]
                .as_str()
                .or_else(|| r["content"].as_str())
                .unwrap_or("");
            out.push_str(&format!("- **{}** <{}>\n  {}\n", title, url, snippet));
        }
        return Ok(out);
    }
    anyhow::bail!("pipellm search failed: {}", last_err)
}

fn search_tavily(
    query: &str,
    api_key: &str,
    kind: Option<&str>,
    freshness: Option<&str>,
    search_depth: Option<&str>,
) -> Result<String> {
    // Auth: Authorization: Bearer header (not api_key in body).
    // include_answer: true always. Tavily returns a direct AI-synthesized answer alongside results.
    let mut body = serde_json::json!({
        "query": query,
        "max_results": 10,
        "include_answer": true
    });
    // kind="news" → topic: "news"; kind="finance" → topic: "finance".
    if let Some(k) = kind {
        if k == "news" || k == "finance" {
            body["topic"] = serde_json::json!(k);
        }
    }
    // search_depth: "advanced" for deeper crawling.
    if let Some(d) = search_depth {
        body["search_depth"] = serde_json::json!(d);
    }
    // freshness → days param (pd=1, pw=7, pm=31, py=365).
    if let Some(f) = freshness {
        let days: u32 = match f {
            "pd" => 1,
            "pw" => 7,
            "pm" => 31,
            "py" => 365,
            other => other.parse().unwrap_or(7),
        };
        body["days"] = serde_json::json!(days);
    }
    let resp = web_client()
        .post("https://api.tavily.com/search")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .context("tavily search request failed")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = read_error_body(resp);
        anyhow::bail!(
            "tavily search returned {}: {}",
            status,
            body.chars().take(300).collect::<String>()
        );
    }
    let json: serde_json::Value = resp.json().context("parse tavily response")?;
    let results = json["results"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let mut out = String::new();
    // Surface the direct AI answer first if present.
    if let Some(answer) = json["answer"].as_str() {
        if !answer.is_empty() {
            out.push_str(&format!("**Answer:** {}\n\n", answer));
        }
    }
    if results.is_empty() && out.is_empty() {
        return Ok("No results found.".into());
    }
    for r in results.iter().take(10) {
        let title = r["title"].as_str().unwrap_or("(no title)");
        let url = r["url"].as_str().unwrap_or("");
        let content = r["content"].as_str().unwrap_or("");
        out.push_str(&format!("- **{}** <{}>\n  {}\n", title, url, content));
    }
    Ok(out)
}

fn exec_project_summary(path: &Path) -> Result<String> {
    let mut out = String::new();
    let dir_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    out.push_str(&format!("Project: {}\n", dir_name));

    let is_git = path.join(".git").exists();
    if is_git {
        out.push_str("VCS: git\n");
    }

    struct Marker {
        file: &'static str,
        lang: &'static str,
        build: &'static str,
    }
    let markers = [
        Marker {
            file: "Cargo.toml",
            lang: "Rust",
            build: "cargo",
        },
        Marker {
            file: "package.json",
            lang: "JavaScript/TypeScript",
            build: "npm/pnpm",
        },
        Marker {
            file: "go.mod",
            lang: "Go",
            build: "go",
        },
        Marker {
            file: "pyproject.toml",
            lang: "Python",
            build: "pip/poetry",
        },
        Marker {
            file: "setup.py",
            lang: "Python",
            build: "setuptools",
        },
        Marker {
            file: "Gemfile",
            lang: "Ruby",
            build: "bundler",
        },
        Marker {
            file: "pom.xml",
            lang: "Java",
            build: "Maven",
        },
        Marker {
            file: "build.gradle",
            lang: "Java/Kotlin",
            build: "Gradle",
        },
        Marker {
            file: "CMakeLists.txt",
            lang: "C/C++",
            build: "CMake",
        },
        Marker {
            file: "Makefile",
            lang: "",
            build: "make",
        },
        Marker {
            file: "Package.swift",
            lang: "Swift",
            build: "SwiftPM",
        },
    ];

    let mut langs: Vec<&str> = Vec::new();
    let mut builds: Vec<&str> = Vec::new();
    for m in &markers {
        if path.join(m.file).exists() {
            if !m.lang.is_empty() && !langs.contains(&m.lang) {
                langs.push(m.lang);
            }
            if !builds.contains(&m.build) {
                builds.push(m.build);
            }
        }
    }

    if !langs.is_empty() {
        out.push_str(&format!("Languages: {}\n", langs.join(", ")));
    }
    if !builds.is_empty() {
        out.push_str(&format!("Build system: {}\n", builds.join(", ")));
    }

    // Extract name/version from Cargo.toml or package.json if present.
    if let Ok(cargo) = std::fs::read_to_string(path.join("Cargo.toml")) {
        for line in cargo.lines().take(20) {
            let trimmed = line.trim();
            if trimmed.starts_with("name")
                || trimmed.starts_with("version")
                || trimmed.starts_with("description")
            {
                out.push_str(&format!("  {}\n", trimmed));
            }
        }
        // List workspace members if present.
        if let Some(idx) = cargo.find("[workspace]") {
            for line in cargo[idx..].lines().skip(1).take(20) {
                let t = line.trim();
                if t.starts_with('[') && t != "[workspace]" {
                    break;
                }
                if t.starts_with('"') || t.starts_with("members") {
                    out.push_str(&format!("  {}\n", t));
                }
            }
        }
    }
    if let Ok(pkg) = std::fs::read_to_string(path.join("package.json")) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&pkg) {
            if let Some(name) = json["name"].as_str() {
                out.push_str(&format!("  name: {}\n", name));
            }
            if let Some(ver) = json["version"].as_str() {
                out.push_str(&format!("  version: {}\n", ver));
            }
        }
    }

    // Key directories (1 level).
    let key_dirs: Vec<String> = std::fs::read_dir(path)
        .ok()
        .into_iter()
        .flat_map(|rd| rd.filter_map(|e| e.ok()))
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            !n.starts_with('.') && n != "node_modules" && n != "target" && n != "__pycache__"
        })
        .collect();
    if !key_dirs.is_empty() {
        let mut sorted = key_dirs;
        sorted.sort();
        out.push_str(&format!("Directories: {}\n", sorted.join(", ")));
    }

    // Entry points.
    let entry_candidates = [
        "src/main.rs",
        "src/lib.rs",
        "src/index.ts",
        "src/index.js",
        "main.go",
        "main.py",
        "app.py",
        "index.js",
        "index.ts",
    ];
    let mut entries: Vec<&str> = Vec::new();
    for e in &entry_candidates {
        if path.join(e).exists() {
            entries.push(e);
        }
    }
    if !entries.is_empty() {
        out.push_str(&format!("Entry points: {}\n", entries.join(", ")));
    }

    Ok(out)
}

fn exec_file_tree(root: &Path, max_depth: usize) -> Result<String> {
    const SKIP_DIRS: &[&str] = &[
        ".git",
        "node_modules",
        "target",
        "__pycache__",
        ".next",
        "dist",
        "build",
        ".build",
        ".cache",
        "vendor",
        ".bundle",
        "venv",
        ".venv",
        "Pods",
        "DerivedData",
    ];
    const MAX_ENTRIES: usize = 500;

    let mut out = String::new();
    let mut count = 0usize;

    fn walk(
        dir: &Path,
        prefix: &str,
        depth: usize,
        max_depth: usize,
        out: &mut String,
        count: &mut usize,
    ) {
        if depth > max_depth || *count >= MAX_ENTRIES {
            return;
        }
        let mut entries: Vec<(String, bool)> = match std::fs::read_dir(dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    (name, is_dir)
                })
                .filter(|(name, _)| !name.starts_with('.') || name == ".github")
                .collect(),
            Err(_) => return,
        };
        entries.sort_by(|a, b| match (a.1, b.1) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.0.cmp(&b.0),
        });

        for (name, is_dir) in &entries {
            if *count >= MAX_ENTRIES {
                out.push_str(&format!("{}... (truncated)\n", prefix));
                return;
            }
            *count += 1;
            if *is_dir {
                out.push_str(&format!("{}{}/\n", prefix, name));
                if !SKIP_DIRS.contains(&name.as_str()) {
                    walk(
                        &dir.join(name),
                        &format!("{}  ", prefix),
                        depth + 1,
                        max_depth,
                        out,
                        count,
                    );
                }
            } else {
                out.push_str(&format!("{}{}\n", prefix, name));
            }
        }
    }

    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    out.push_str(&format!("{}/\n", root_name));
    walk(root, "  ", 1, max_depth, &mut out, &mut count);
    if count >= MAX_ENTRIES {
        out.push_str(&format!("[truncated at {} entries]\n", MAX_ENTRIES));
    }
    Ok(out)
}

fn escape_regex(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn exec_symbol_search(
    query: &str,
    kind: &str,
    search_path: &str,
    glob_filter: Option<&str>,
    cwd: &str,
    cancel: &Arc<AtomicBool>,
) -> Result<String> {
    let abs_path = resolve(search_path, cwd)?.to_string_lossy().into_owned();

    // Build language-aware regex patterns for definitions.
    let patterns: Vec<String> = match kind {
        "function" => vec![
            format!(r"(fn|function|def|func)\s+{}", escape_regex(query)),
            format!(
                r"(const|let|var)\s+{}\s*=\s*(async\s+)?\(",
                escape_regex(query)
            ),
        ],
        "type" => vec![format!(
            r"(type|struct|enum|interface|typedef)\s+{}",
            escape_regex(query)
        )],
        "class" => vec![format!(r"(class|struct)\s+{}", escape_regex(query))],
        "method" => vec![
            format!(r"(fn|def|func|function)\s+{}", escape_regex(query)),
            format!(r"\.{}\s*=\s*function", escape_regex(query)),
        ],
        _ => vec![
            format!(r"(fn|function|def|func)\s+{}", escape_regex(query)),
            format!(
                r"(const|let|var)\s+{}\s*=\s*(async\s+)?\(",
                escape_regex(query)
            ),
            format!(
                r"(type|struct|enum|interface|class|trait|typedef)\s+{}",
                escape_regex(query)
            ),
            format!(r"(pub\s+)?(mod|module)\s+{}", escape_regex(query)),
        ],
    };

    let combined = patterns.join("|");

    // Use ripgrep if available, fall back to grep.
    static HAS_RG: OnceLock<bool> = OnceLock::new();
    let rg = *HAS_RG.get_or_init(|| {
        std::process::Command::new("rg")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    });

    let mut cmd = if rg {
        let mut c = std::process::Command::new("rg");
        c.arg("--line-number")
            .arg("--no-heading")
            .arg("--color=never")
            .arg("--max-count=50");
        if let Some(g) = glob_filter {
            c.arg("--glob").arg(g);
        }
        c.arg(&combined).arg(&abs_path);
        c
    } else {
        let mut c = std::process::Command::new("grep");
        c.arg("-rn").arg("--color=never").arg("-E");
        if let Some(g) = glob_filter {
            c.arg("--include").arg(g);
        }
        c.arg(&combined).arg(&abs_path);
        c
    };

    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    let mut child = cmd.spawn().context("symbol_search exec failed")?;

    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("symbol_search stdout missing"))?;
    let collected = Arc::new(Mutex::new(Vec::<u8>::new()));
    let collected_clone = collected.clone();
    let reader_thread = std::thread::spawn(move || {
        let mut r = stdout_pipe;
        let mut buf = [0u8; 8192];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Ok(mut g) = collected_clone.lock() {
                        g.extend_from_slice(&buf[..n]);
                    }
                }
            }
        }
    });

    let start = Instant::now();
    let timeout = Duration::from_secs(SEARCH_TIMEOUT_SECS);
    let mut timed_out = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            kill_process_group(&child);
            child.wait().ok();
            let _ = reader_thread.join();
            anyhow::bail!("symbol_search canceled");
        }
        if start.elapsed() >= timeout {
            kill_process_group(&child);
            timed_out = true;
            break;
        }
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.wait().ok();
    let _ = reader_thread.join();

    let raw = collected.lock().map(|g| g.clone()).unwrap_or_default();
    let text = String::from_utf8_lossy(&raw);

    if text.trim().is_empty() {
        if timed_out {
            return Ok(format!(
                "symbol_search timed out after {}s with no results for '{}'.",
                SEARCH_TIMEOUT_SECS, query
            ));
        }
        return Ok(format!("No symbol definitions found for '{}'.", query));
    }

    // Deduplicate and limit results.
    let mut lines: Vec<&str> = text.lines().take(100).collect();
    lines.sort_by(|a, b| {
        let a_has_kw = a.contains("fn ")
            || a.contains("function ")
            || a.contains("def ")
            || a.contains("struct ")
            || a.contains("class ")
            || a.contains("type ")
            || a.contains("enum ")
            || a.contains("trait ")
            || a.contains("interface ");
        let b_has_kw = b.contains("fn ")
            || b.contains("function ")
            || b.contains("def ")
            || b.contains("struct ")
            || b.contains("class ")
            || b.contains("type ")
            || b.contains("enum ")
            || b.contains("trait ")
            || b.contains("interface ");
        b_has_kw.cmp(&a_has_kw)
    });

    let mut out = lines.join("\n");
    if timed_out {
        out.push_str(&format!(
            "\n[... timed out after {}s, results may be partial]",
            SEARCH_TIMEOUT_SECS
        ));
    }
    Ok(out)
}

fn exec_grep_search(
    pattern: &str,
    search_path: &str,
    glob_filter: Option<&str>,
    context_lines: usize,
    case_insensitive: bool,
    max_results: usize,
    cwd: &str,
    cancel: &Arc<AtomicBool>,
) -> Result<String> {
    // Use ripgrep if available, fall back to grep. Cached after first probe.
    static HAS_RG: OnceLock<bool> = OnceLock::new();
    let rg = *HAS_RG.get_or_init(|| {
        std::process::Command::new("rg")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    });
    let abs_path = resolve(search_path, cwd)?.to_string_lossy().into_owned();

    let mut cmd = if rg {
        let mut c = std::process::Command::new("rg");
        c.arg("--line-number")
            .arg("--no-heading")
            .arg("--color=never")
            .arg(format!("--context={}", context_lines))
            // Stop scanning each file early; post-filter caps the total.
            .arg(format!("--max-count={}", max_results));
        if case_insensitive {
            c.arg("--ignore-case");
        }
        if let Some(g) = glob_filter {
            c.arg("--glob").arg(g);
        }
        c.arg(pattern).arg(&abs_path);
        c
    } else {
        let mut c = std::process::Command::new("grep");
        c.arg("-rn")
            .arg(format!("-C{}", context_lines))
            .arg("--color=never");
        if case_insensitive {
            c.arg("-i");
        }
        if let Some(g) = glob_filter {
            c.arg("--include").arg(g);
        }
        c.arg(pattern).arg(&abs_path);
        c
    };

    // Stream stdout line by line to avoid buffering the full output in memory.
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0);
    let mut child = cmd.spawn().context("grep_search exec failed")?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("grep stdout missing"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("grep stderr missing"))?;

    // Drain stderr in a background thread to prevent the child from blocking
    // on a full pipe buffer when it writes many errors (bad regex, permission
    // denied while walking directories, etc.). Keep only the first 512 bytes.
    let stderr_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::with_capacity(512)));
    let stderr_buf_clone = stderr_buf.clone();
    let stderr_handle = std::thread::spawn(move || {
        let mut err = stderr;
        let mut chunk = [0u8; 512];
        loop {
            match err.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Ok(mut g) = stderr_buf_clone.lock() {
                        let remaining = 512usize.saturating_sub(g.len());
                        if remaining > 0 {
                            g.extend_from_slice(&chunk[..remaining.min(n)]);
                        }
                        // Keep draining past the cap so the child is not stalled.
                    }
                }
            }
        }
    });

    let result_lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let match_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let truncated_flag = Arc::new(AtomicBool::new(false));

    let rl = result_lines.clone();
    let mc = match_count.clone();
    let tf = truncated_flag.clone();
    let max = max_results;
    let reader_handle = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        for line_result in reader.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };
            if !line.starts_with("--") {
                if mc.load(Ordering::Relaxed) >= max {
                    tf.store(true, Ordering::Relaxed);
                    break;
                }
                mc.fetch_add(1, Ordering::Relaxed);
            }
            if let Ok(mut g) = rl.lock() {
                g.push(line);
            }
        }
    });

    let start = Instant::now();
    let timeout = Duration::from_secs(SEARCH_TIMEOUT_SECS);
    let mut timed_out = false;
    let mut canceled = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            kill_process_group(&child);
            canceled = true;
            break;
        }
        if start.elapsed() >= timeout {
            kill_process_group(&child);
            timed_out = true;
            break;
        }
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.wait().ok();
    let _ = reader_handle.join();
    let _ = stderr_handle.join();

    let truncated = truncated_flag.load(Ordering::Relaxed);
    let lines = result_lines.lock().map(|g| g.clone()).unwrap_or_default();

    if lines.is_empty() {
        if canceled {
            anyhow::bail!("grep_search canceled");
        }
        if timed_out {
            return Ok(format!(
                "grep_search timed out after {}s with no results.",
                SEARCH_TIMEOUT_SECS
            ));
        }
        let hint = stderr_buf
            .lock()
            .ok()
            .map(|g| {
                String::from_utf8_lossy(&g)
                    .trim()
                    .chars()
                    .take(200)
                    .collect::<String>()
            })
            .unwrap_or_default();
        if !hint.is_empty() {
            return Ok(format!("No matches. ({})", hint));
        }
        return Ok("No matches found.".into());
    }

    let mut out = lines.join("\n");
    if truncated {
        out.push_str(&format!("\n[... truncated at {} results]", max_results));
    }
    if timed_out {
        out.push_str(&format!(
            "\n[... timed out after {}s, results may be partial]",
            SEARCH_TIMEOUT_SECS
        ));
    }
    if canceled {
        out.push_str("\n[... canceled by user]");
    }
    Ok(out)
}

fn exec_http_request(
    method: &str,
    url: &str,
    headers: Option<&serde_json::Map<String, serde_json::Value>>,
    body: Option<&str>,
    query_params: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Result<String> {
    let client = web_client();
    let mut req = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "PATCH" => client.patch(url),
        "DELETE" => client.delete(url),
        _ => anyhow::bail!("unsupported HTTP method: {}", method),
    };

    if let Some(params) = query_params {
        let pairs: Vec<(&str, &str)> = params
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.as_str(), s)))
            .collect();
        req = req.query(&pairs);
    }

    if let Some(hdrs) = headers {
        for (k, v) in hdrs {
            if let Some(val) = v.as_str() {
                req = req.header(k.as_str(), val);
            }
        }
    }

    if let Some(b) = body {
        // Detect JSON body and set Content-Type automatically.
        if serde_json::from_str::<serde_json::Value>(b).is_ok() {
            req = req
                .header("Content-Type", "application/json")
                .body(b.to_string());
        } else {
            req = req.body(b.to_string());
        }
    }

    let resp = req
        .send()
        .with_context(|| format!("http_request {} {} failed", method, url))?;

    let status = resp.status();
    let resp_headers: Vec<String> = resp
        .headers()
        .iter()
        .filter(|(k, _)| {
            let name = k.as_str().to_ascii_lowercase();
            matches!(
                name.as_str(),
                "content-type" | "content-length" | "x-request-id" | "x-ratelimit-remaining"
            )
        })
        .map(|(k, v)| format!("{}: {}", k, v.to_str().unwrap_or("?")))
        .collect();
    let body_text = read_response_capped(resp).context("read http_request response body")?;

    let mut out = format!("HTTP {}\n", status.as_u16());
    if !resp_headers.is_empty() {
        out.push_str(&resp_headers.join("\n"));
        out.push('\n');
    }
    out.push('\n');
    // Pretty-print JSON if the body looks like JSON.
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body_text) {
        out.push_str(&serde_json::to_string_pretty(&json).unwrap_or(body_text));
    } else {
        out.push_str(&body_text);
    }
    Ok(out)
}

/// Refuse reads of well-known credential / system-secret locations, even when
/// the caller passes an absolute or `~/`-prefixed path (both of which bypass
/// the cwd sandbox). Best-effort canonicalization: on ENOENT we compare the
/// raw path so a file about to be created in a blocked directory is still
/// caught.
fn reject_if_sensitive(path: &Path) -> Result<()> {
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let home = std::env::var("HOME").unwrap_or_default();
    let mut blocked: Vec<PathBuf> = vec![
        PathBuf::from("/etc/shadow"),
        PathBuf::from("/etc/sudoers"),
        PathBuf::from("/etc/sudoers.d"),
        PathBuf::from("/private/etc/shadow"),
        PathBuf::from("/private/etc/sudoers"),
        PathBuf::from("/private/etc/sudoers.d"),
    ];
    if !home.is_empty() {
        for rel in [".ssh", ".aws/credentials", ".gnupg", ".config/kaku/secrets"] {
            blocked.push(PathBuf::from(&home).join(rel));
        }
    }
    for b in &blocked {
        let b_canon = std::fs::canonicalize(b).unwrap_or_else(|_| b.clone());
        if canon == b_canon || canon.starts_with(&b_canon) {
            anyhow::bail!(
                "refused: '{}' is a protected secret location",
                path.display()
            );
        }
    }
    Ok(())
}

/// Handles `~/…` expansion and relative paths (resolved against `cwd`).
fn resolve(path: &str, cwd: &str) -> Result<PathBuf> {
    let p = if path.starts_with("~/") || path == "~" {
        let home = std::env::var("HOME").context("HOME not set")?;
        if path == "~" {
            PathBuf::from(home)
        } else {
            PathBuf::from(home).join(&path[2..])
        }
    } else if path.starts_with('/') {
        PathBuf::from(path)
    } else {
        PathBuf::from(cwd).join(path)
    };
    Ok(p)
}

/// Read a URL and return clean extracted text.
/// Uses provider-native readers where available, falls back to generic fetchers.
fn exec_read_url(url: &str, provider: &str, api_key: &str) -> Result<String> {
    match provider {
        "pipellm" => {
            // PipeLLM reader: clean server-side extraction, handles JS pages.
            let domains = ["https://api.pipellm.ai", "https://api.pipellm.com"];
            let mut last_err = String::new();
            for base in &domains {
                let resp = match web_client()
                    .get(format!("{}/v1/websearch/reader", base))
                    .query(&[("url", url)])
                    .bearer_auth(api_key)
                    .send()
                {
                    Ok(r) => r,
                    Err(e) => {
                        last_err = e.to_string();
                        continue;
                    }
                };
                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = read_error_body(resp);
                    last_err = format!(
                        "{} from {}: {}",
                        status,
                        base,
                        body.chars().take(300).collect::<String>()
                    );
                    continue;
                }
                let json: serde_json::Value =
                    resp.json().context("parse pipellm reader response")?;
                // Response may be plain text or JSON with "content"/"text" field.
                let text = json["content"]
                    .as_str()
                    .or_else(|| json["text"].as_str())
                    .or_else(|| json.as_str())
                    .unwrap_or("")
                    .to_string();
                if !text.trim().is_empty() {
                    return Ok(text);
                }
                return Ok("Page returned empty content.".into());
            }
            // Both domains failed; fall back to generic reader.
            log::warn!(
                "pipellm reader failed ({}), falling back to generic fetch",
                last_err
            );
            fetch_markdown_default(url)
        }
        "tavily" => {
            // Tavily extract: purpose-built for AI content extraction from URLs.
            let resp = web_client()
                .post("https://api.tavily.com/extract")
                .bearer_auth(api_key)
                .json(&serde_json::json!({ "urls": [url] }))
                .send()
                .context("tavily extract request failed")?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = read_error_body(resp);
                // Fall back on failure rather than hard-erroring.
                log::warn!(
                    "tavily extract returned {} ({}), falling back to generic fetch",
                    status,
                    body.trim().chars().take(200).collect::<String>()
                );
                return fetch_markdown_default(url);
            }
            let json: serde_json::Value = resp.json().context("parse tavily extract response")?;
            // Response: {"results": [{"url": ..., "raw_content": ...}]}
            let content = json["results"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|r| r["raw_content"].as_str().or_else(|| r["content"].as_str()))
                .unwrap_or("")
                .to_string();
            if content.trim().is_empty() {
                return fetch_markdown_default(url);
            }
            Ok(content)
        }
        // Brave and unknown providers: fall back to generic markdown fetchers.
        _ => fetch_markdown_default(url),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn no_cancel() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    fn dummy_config() -> AssistantConfig {
        AssistantConfig {
            api_key: "test".to_string(),
            chat_model: "test".to_string(),
            chat_model_choices: vec![],
            base_url: "https://example.com".to_string(),
            provider: "Custom".to_string(),
            auth_type: "api_key".to_string(),
            chat_tools_enabled: false,
            web_search_provider: None,
            web_search_api_key: None,
            web_fetch_script: None,
            memory_curator_model: None,
        }
    }

    #[test]
    fn resolve_expands_tilde() {
        let home = std::env::var("HOME").expect("HOME not set");
        assert_eq!(
            resolve("~/foo", "/tmp").unwrap(),
            PathBuf::from(&home).join("foo")
        );
        assert_eq!(resolve("~", "/tmp").unwrap(), PathBuf::from(&home));
    }

    #[test]
    fn resolve_absolute_unchanged() {
        assert_eq!(
            resolve("/etc/passwd", "/tmp").unwrap(),
            PathBuf::from("/etc/passwd")
        );
    }

    #[test]
    fn resolve_relative_to_cwd() {
        assert_eq!(
            resolve("src/main.rs", "/project").unwrap(),
            PathBuf::from("/project/src/main.rs")
        );
    }

    #[test]
    fn fs_read_refuses_ssh_directory() {
        let home = std::env::var("HOME").expect("HOME not set");
        let ssh_probe = format!("{}/.ssh/id_rsa", home);
        let args = serde_json::json!({"path": ssh_probe});
        let mut cwd = "/tmp".to_string();
        let err = execute("fs_read", &args, &mut cwd, &dummy_config(), &no_cancel())
            .expect_err("fs_read should refuse ~/.ssh paths");
        assert!(
            err.to_string().contains("protected secret location"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn fs_list_refuses_ssh_directory() {
        let home = std::env::var("HOME").expect("HOME not set");
        let args = serde_json::json!({"path": format!("{}/.ssh", home)});
        let mut cwd = "/tmp".to_string();
        let err = execute("fs_list", &args, &mut cwd, &dummy_config(), &no_cancel())
            .expect_err("fs_list should refuse ~/.ssh");
        assert!(
            err.to_string().contains("protected secret location"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn fs_mkdir_refuses_ssh_directory() {
        let home = std::env::var("HOME").expect("HOME not set");
        let args = serde_json::json!({"path": format!("{}/.ssh/evil", home)});
        let mut cwd = "/tmp".to_string();
        let err = execute("fs_mkdir", &args, &mut cwd, &dummy_config(), &no_cancel())
            .expect_err("fs_mkdir should refuse ~/.ssh/*");
        assert!(
            err.to_string().contains("protected secret location"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn fs_delete_refuses_ssh_file() {
        let home = std::env::var("HOME").expect("HOME not set");
        let args = serde_json::json!({"path": format!("{}/.ssh/id_rsa", home)});
        let mut cwd = "/tmp".to_string();
        let err = execute("fs_delete", &args, &mut cwd, &dummy_config(), &no_cancel())
            .expect_err("fs_delete should refuse ~/.ssh/*");
        assert!(
            err.to_string().contains("protected secret location"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn fs_read_caps_large_files() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        // fs_read default cap is DEFAULT_RESULT_BYTES (8 000). Write more than that.
        let huge = "x".repeat(DEFAULT_RESULT_BYTES + 2000);
        tmp.write_all(huge.as_bytes()).unwrap();
        let path = tmp.path().to_string_lossy();
        let args = serde_json::json!({"path": path.to_string()});
        let mut cwd = "/tmp".to_string();
        let result = execute("fs_read", &args, &mut cwd, &dummy_config(), &no_cancel()).unwrap();
        assert!(
            result.contains("[truncated:"),
            "expected truncation note in result, got: {}",
            &result[..result.len().min(200)]
        );
        // Truncated content + structured note should be a bit above DEFAULT_RESULT_BYTES.
        assert!(result.len() > DEFAULT_RESULT_BYTES && result.len() < DEFAULT_RESULT_BYTES + 500);
    }

    #[test]
    fn fs_list_basic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let args = serde_json::json!({"path": dir.path().to_string_lossy().to_string()});
        let mut cwd = "/tmp".to_string();
        let result = execute("fs_list", &args, &mut cwd, &dummy_config(), &no_cancel()).unwrap();
        assert!(
            result.contains("a.txt"),
            "expected a.txt in listing: {}",
            result
        );
        assert!(
            result.contains("sub/"),
            "expected sub/ in listing: {}",
            result
        );
    }

    #[test]
    fn shell_exec_respects_cancel_flag() {
        // Spawn a sleep that would otherwise run for 30s; flip cancel after
        // 200ms and confirm execute() returns promptly with a canceled marker.
        let args = serde_json::json!({"command": "sleep 30"});
        let mut cwd = "/tmp".to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        let trigger = Arc::clone(&cancel);
        let flipper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            trigger.store(true, Ordering::Relaxed);
        });
        let start = Instant::now();
        let result = execute("shell_exec", &args, &mut cwd, &dummy_config(), &cancel).unwrap();
        flipper.join().unwrap();
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "cancel should return within a few seconds, took {:?}",
            elapsed
        );
        assert!(
            result.contains("[canceled by user"),
            "expected canceled marker, got: {}",
            result
        );
    }

    #[test]
    fn shell_exec_overflow_still_honors_cancel() {
        // Emit far more than the output cap, then sleep. execute() must remain
        // cancelable after overflow instead of blocking in child.wait().
        let args = serde_json::json!({
            "command": "yes x | head -c 5000000; sleep 2"
        });
        let mut cwd = "/tmp".to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        let trigger = Arc::clone(&cancel);
        let flipper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            trigger.store(true, Ordering::Relaxed);
        });
        let start = Instant::now();
        let result = execute("shell_exec", &args, &mut cwd, &dummy_config(), &cancel).unwrap();
        flipper.join().unwrap();
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "cancel after overflow should return quickly, took {:?}",
            elapsed
        );
        assert!(
            result.contains("[canceled by user"),
            "expected canceled marker, got: {}",
            result
        );
    }

    #[test]
    fn grep_search_finds_pattern() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.txt"), "hello world\nfoo bar\n").unwrap();
        let args = serde_json::json!({
            "pattern": "hello",
            "path": dir.path().to_string_lossy().to_string()
        });
        let mut cwd = "/tmp".to_string();
        let result = execute(
            "grep_search",
            &args,
            &mut cwd,
            &dummy_config(),
            &no_cancel(),
        )
        .unwrap();
        assert!(
            result.contains("hello world"),
            "expected match in result: {}",
            result
        );
    }

    // ---------------------------------------------------------------
    // Eval test suite: end-to-end tool-chain scenarios on a fixture
    // ---------------------------------------------------------------

    fn create_project_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "demo-app"
version = "0.1.0"
edition = "2021"
"#,
        )
        .unwrap();

        std::fs::write(root.join("README.md"), "# Demo App\nA small CLI tool for greeting users.\n").unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(
            root.join("Makefile"),
            "test:\n\tcargo test\nbuild:\n\tcargo build\n",
        )
        .unwrap();

        std::fs::create_dir_all(root.join("src")).unwrap();

        std::fs::write(
            root.join("src/main.rs"),
            r#"fn main() {
    let nmae = "World";
    println!("Hello, {}!", nmae);
}
"#,
        )
        .unwrap();

        std::fs::write(
            root.join("src/lib.rs"),
            r#"pub fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#,
        )
        .unwrap();

        std::fs::write(
            root.join("src/utils.rs"),
            r#"// TODO: add input validation
pub fn sanitize_input(input: &str) -> String {
    input.trim().to_string()
}
"#,
        )
        .unwrap();

        std::fs::write(
            root.join("src/config.rs"),
            r#"pub struct AppConfig {
    pub verbose: bool,
    pub name: String,
}
"#,
        )
        .unwrap();

        std::fs::create_dir_all(root.join("tests")).unwrap();
        std::fs::write(
            root.join("tests/integration.rs"),
            r#"#[test]
fn test_greet() {
    assert_eq!(demo_app::greet("Rust"), "Hello, Rust!");
}
"#,
        )
        .unwrap();

        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/API.md"), "# API\n\n## greet(name)\nReturns a greeting string.\n").unwrap();

        dir
    }

    #[test]
    fn eval_01_project_summary_and_readme() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let summary = execute(
            "project_summary",
            &serde_json::json!({}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(summary.contains("Rust"), "summary should detect Rust: {}", summary);
        assert!(summary.contains("cargo"), "summary should mention cargo: {}", summary);

        let readme = execute(
            "fs_read",
            &serde_json::json!({"path": "README.md"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(readme.contains("Demo App"), "README should contain project name: {}", readme);
    }

    #[test]
    fn eval_02_file_tree() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let tree = execute(
            "file_tree",
            &serde_json::json!({"depth": 2}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(tree.contains("src/"), "tree should list src/: {}", tree);
        assert!(tree.contains("main.rs"), "tree should list main.rs: {}", tree);
        assert!(tree.contains("lib.rs"), "tree should list lib.rs: {}", tree);
        assert!(tree.contains("Cargo.toml"), "tree should list Cargo.toml: {}", tree);
        assert!(!tree.contains(".git/"), "tree should not list .git/: {}", tree);
        assert!(!tree.contains("target/"), "tree should not list target/: {}", tree);
    }

    #[test]
    fn eval_03_symbol_search_greet() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let result = execute(
            "symbol_search",
            &serde_json::json!({"query": "greet", "kind": "function", "glob": "*.rs"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(result.contains("fn greet"), "should find fn greet: {}", result);
        assert!(result.contains("lib.rs"), "should locate in lib.rs: {}", result);
    }

    #[test]
    fn eval_04_grep_search_todo() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let result = execute(
            "grep_search",
            &serde_json::json!({"pattern": "TODO", "glob": "*.rs"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(result.contains("utils.rs"), "should find TODO in utils.rs: {}", result);
        assert!(result.contains("TODO"), "should contain the TODO text: {}", result);
    }

    #[test]
    fn eval_05_fs_read_line_range() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let result = execute(
            "fs_read",
            &serde_json::json!({"path": "src/main.rs", "start_line": 1, "end_line": 5}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        let lines: Vec<&str> = result.lines().collect();
        assert!(lines.len() <= 5, "should return at most 5 lines, got {}", lines.len());
        assert!(result.contains("fn main"), "should contain fn main: {}", result);
    }

    #[test]
    fn eval_06_read_then_patch() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let before = execute(
            "fs_read",
            &serde_json::json!({"path": "src/main.rs"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(before.contains("nmae"), "should contain typo before patch: {}", before);

        let patch_result = execute(
            "fs_patch",
            &serde_json::json!({
                "path": "src/main.rs",
                "old_text": "let nmae = \"World\";\n    println!(\"Hello, {}!\", nmae);",
                "new_text": "let name = \"World\";\n    println!(\"Hello, {}!\", name);"
            }),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            patch_result.contains("replaced 1 occurrence"),
            "patch should report replacement: {}",
            patch_result
        );

        let after = execute(
            "fs_read",
            &serde_json::json!({"path": "src/main.rs"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(!after.contains("nmae"), "typo should be gone after patch: {}", after);
        assert!(after.contains("let name"), "corrected text should be present: {}", after);
    }

    #[test]
    fn eval_07_fs_write_and_verify() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let content = "pub fn helper() -> &'static str {\n    \"help\"\n}\n";
        let write_result = execute(
            "fs_write",
            &serde_json::json!({"path": "src/helpers.rs", "content": content}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(write_result.contains("Written"), "should confirm write: {}", write_result);
        assert!(
            write_result.contains(&format!("{}", content.len())),
            "should report byte count: {}",
            write_result
        );

        let read_back = execute(
            "fs_read",
            &serde_json::json!({"path": "src/helpers.rs"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(read_back.contains("pub fn helper"), "should read back written content: {}", read_back);
    }

    #[test]
    fn eval_08_shell_exec() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let result = execute(
            "shell_exec",
            &serde_json::json!({"command": "echo 'all 3 tests passed'"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            result.contains("all 3 tests passed"),
            "should contain echo output: {}",
            result
        );
    }

    #[test]
    fn eval_09_list_then_delete() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let before = execute(
            "fs_list",
            &serde_json::json!({"path": "docs"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(before.contains("API.md"), "docs should contain API.md before delete: {}", before);

        let del = execute(
            "fs_delete",
            &serde_json::json!({"path": "docs/API.md"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(del.contains("Deleted"), "should confirm deletion: {}", del);

        let after = execute(
            "fs_list",
            &serde_json::json!({"path": "docs"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(!after.contains("API.md"), "API.md should be gone after delete: {}", after);
    }

    #[test]
    fn eval_10_error_paths() {
        let fixture = create_project_fixture();
        let mut cwd = fixture.path().to_string_lossy().into_owned();
        let cfg = dummy_config();
        let cancel = no_cancel();

        let read_err = execute(
            "fs_read",
            &serde_json::json!({"path": "src/nonexistent.rs"}),
            &mut cwd,
            &cfg,
            &cancel,
        );
        assert!(read_err.is_err(), "fs_read of nonexistent file should return Err");

        let grep_result = execute(
            "grep_search",
            &serde_json::json!({"pattern": "ZZZZNOTEXIST"}),
            &mut cwd,
            &cfg,
            &cancel,
        )
        .unwrap();
        assert!(
            grep_result.contains("No matches"),
            "grep for nonexistent pattern should report no matches: {}",
            grep_result
        );
    }
}
