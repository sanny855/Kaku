//! AI client for Kaku's built-in chat overlay.
//!
//! Reads API config from `~/.config/kaku/assistant.toml` and provides
//! a synchronous streaming chat completion client (OpenAI-compatible API).
//! Supports function/tool calling for agentic workflows.
//!
//! Runs on a plain OS thread (inside overlay), so blocking I/O is fine.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use crate::ai_auth;
use reqwest::header::{HeaderName, HeaderValue};

const DEFAULT_MODEL: &str = "gpt-5.4-mini";
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// Configuration loaded from `assistant.toml`.
#[derive(Clone)]
#[allow(dead_code)]
pub struct AssistantConfig {
    pub api_key: String,
    /// Deep chat model. Falls back to the Simple Model from assistant.toml when omitted.
    pub chat_model: String,
    /// Optional user-curated model list for the chat overlay. When set, the chat
    /// overlay cycles only through these via Shift+Tab and skips the auto-fetch step.
    pub chat_model_choices: Vec<String>,
    pub base_url: String,
    /// Optional extra headers for enterprise proxies / API gateways.
    pub custom_headers: Vec<(String, String)>,
    /// Provider name derived from base_url and auth_type (e.g. "OpenAI", "Copilot").
    pub provider: String,
    /// Auth mechanism: "api_key" (default), "copilot", or "codex".
    /// Legacy "gemini_key" values are recognized only to surface a friendly
    /// error at load time; the Gemini provider was removed in V0.10.0.
    pub auth_type: String,
    /// When false, the `tools` field is omitted from chat requests.
    /// Set `chat_tools_enabled = false` in assistant.toml for providers that do not
    /// support function calling (e.g. some Kimi or local-model variants).
    pub chat_tools_enabled: bool,
    /// Web search provider: "brave", "pipellm", or "tavily". None = disabled.
    pub web_search_provider: Option<String>,
    /// API key for web_search_provider. None = search tool not registered.
    pub web_search_api_key: Option<String>,
    /// Hidden escape hatch: path to a custom fetch script (not in TUI or template).
    /// Script receives the URL as $1 and must print Markdown to stdout.
    pub web_fetch_script: Option<String>,
    /// Simple Model for quick command generation and lightweight chat. When it
    /// differs from chat_model, the overlay offers it via Shift+Tab.
    pub fast_model: Option<String>,
    /// Optional dedicated model for background memory curation. Falls back to
    /// `chat_model` when unset. Point at a cheaper/faster model to reduce cost.
    pub memory_curator_model: Option<String>,
}

impl AssistantConfig {
    pub fn load() -> Result<Self> {
        let path = assistant_toml_path()?;
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("Cannot read {}", path.display()))?;
        let parsed: toml::Value = raw.parse().context("Invalid assistant.toml")?;

        let auth_type = parsed
            .get("auth_type")
            .and_then(|v| v.as_str())
            .unwrap_or("api_key")
            .to_string();

        // The Gemini provider was removed in V0.10.0. Surface a clear migration
        // path instead of letting the OpenAI-compatible code path silently
        // mangle Gemini requests.
        if auth_type == "gemini_key" {
            anyhow::bail!(
                "Gemini provider was removed in V0.10.0. Open `kaku ai` and \
                 switch to a different provider (OpenAI, Copilot, Codex, or a \
                 custom OpenAI-compatible endpoint), then update {}.",
                path.display()
            );
        }

        // OAuth providers (Copilot, Codex) do not need an api_key in the TOML.
        let api_key_required = auth_type == "api_key";

        let api_key = parsed
            .get("api_key")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if api_key_required && api_key.trim().is_empty() {
            anyhow::bail!(
                "api_key not set in {}. Run `kaku ai` to configure.",
                path.display()
            );
        }

        let model = parsed
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_MODEL)
            .to_string();

        let legacy_fast_model = parsed
            .get("fast_model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);

        let simple_model = legacy_fast_model.clone().unwrap_or_else(|| model.clone());

        // If an old config had both model and fast_model but no chat_model,
        // preserve model as the deep slot and fold fast_model into Simple Model.
        let chat_model = parsed
            .get("chat_model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                if legacy_fast_model.is_some() {
                    model.clone()
                } else {
                    simple_model.clone()
                }
            });

        let chat_model_choices = parsed
            .get("chat_model_choices")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let base_url = parsed
            .get("base_url")
            .and_then(|v| v.as_str())
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/')
            .to_string();

        let custom_headers = parse_custom_headers(parsed.get("custom_headers"))?;

        let provider = detect_provider_with_auth(&base_url, &auth_type).to_string();

        let chat_tools_enabled = parsed
            .get("chat_tools_enabled")
            .and_then(|v| v.as_bool())
            // OpenAI-compatible tool calling is supported by all providers we
            // ship presets for; per-provider opt-out is still possible by
            // setting `chat_tools_enabled = false` in assistant.toml.
            .unwrap_or(true);

        let web_search_provider = parsed
            .get("web_search_provider")
            .and_then(|v| v.as_str())
            .filter(|s| matches!(*s, "brave" | "pipellm" | "tavily"))
            .map(String::from);

        let web_search_api_key = parsed
            .get("web_search_api_key")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);

        let web_fetch_script = parsed
            .get("web_fetch_script")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| expand_tilde(s));

        let fast_model = (simple_model != chat_model).then_some(simple_model);

        let memory_curator_model = parsed
            .get("memory_curator_model")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);

        Ok(Self {
            api_key,
            chat_model,
            chat_model_choices,
            base_url,
            custom_headers,
            provider,
            auth_type,
            chat_tools_enabled,
            web_search_provider,
            web_search_api_key,
            web_fetch_script,
            fast_model,
            memory_curator_model,
        })
    }

    /// Returns true when a web_search provider and its API key are both configured.
    pub fn web_search_ready(&self) -> bool {
        self.web_search_provider.is_some() && self.web_search_api_key.is_some()
    }
}

fn parse_custom_headers(value: Option<&toml::Value>) -> Result<Vec<(String, String)>> {
    let raw_headers: Vec<String> = match value {
        Some(toml::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::trim))
            .filter(|item| !item.is_empty())
            .map(String::from)
            .collect(),
        Some(toml::Value::String(raw)) => raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(String::from)
            .collect(),
        Some(_) | None => Vec::new(),
    };

    let mut headers = Vec::new();
    for raw in raw_headers {
        let (name, value) = raw
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid custom_headers entry `{raw}`"))?;
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || value.is_empty() {
            anyhow::bail!("invalid custom_headers entry `{raw}`");
        }
        if name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("content-type") {
            anyhow::bail!("custom_headers cannot override `{name}`");
        }
        HeaderName::from_bytes(name.as_bytes())
            .with_context(|| format!("invalid custom header name `{name}`"))?;
        HeaderValue::from_str(value)
            .with_context(|| format!("invalid custom header value for `{name}`"))?;
        headers.push((name.to_string(), value.to_string()));
    }
    Ok(headers)
}

fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return Path::new(&home).join(rest).to_string_lossy().into_owned();
        }
    }
    s.to_string()
}

fn assistant_toml_path() -> Result<PathBuf> {
    let user_config_path = config::user_config_path();
    let config_dir = user_config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid user config path"))?;
    Ok(config_dir.join("assistant.toml"))
}

// ─── Message types ────────────────────────────────────────────────────────────

/// A single message in API format. Stored as a raw JSON value so it can represent
/// any role (system, user, assistant, tool) including tool_calls and tool results.
#[derive(Clone)]
pub struct ApiMessage(pub serde_json::Value);

impl ApiMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self(serde_json::json!({ "role": "system", "content": content.into() }))
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self(serde_json::json!({ "role": "user", "content": content.into() }))
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self(serde_json::json!({ "role": "assistant", "content": content.into() }))
    }
    /// Assistant turn that requested tool calls (content is null per the OpenAI spec).
    pub fn assistant_tool_calls(tool_calls: serde_json::Value) -> Self {
        Self(serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": tool_calls
        }))
    }
    /// Tool result message returned after executing a function call.
    /// Includes the tool name so non-OpenAI providers (for example Gemini)
    /// can map responses back to the corresponding function declaration.
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self(serde_json::json!({
            "role": "tool",
            "tool_call_id": tool_call_id.into(),
            "name": name.into(),
            "content": content.into()
        }))
    }

    /// Approximate serialized byte size of this message. Used for history-budget
    /// accounting in the agent loop; does not need to be exact.
    pub fn byte_len(&self) -> usize {
        serde_json::to_vec(&self.0).map(|v| v.len()).unwrap_or(0)
    }
}

// ─── Tool calling ─────────────────────────────────────────────────────────────

/// A fully assembled tool call returned by the model after streaming is complete.
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Complete JSON-encoded arguments string, e.g. `{"path": "~/Downloads"}`.
    pub arguments: String,
}

// ─── Client ───────────────────────────────────────────────────────────────────

/// Synchronous AI client for use inside overlay threads.
/// Clone is cheap: reqwest::blocking::Client is Arc-backed internally.
#[derive(Clone)]
pub struct AiClient {
    config: AssistantConfig,
    client: reqwest::blocking::Client,
}

/// Process-level HTTP client shared across all overlay sessions.
///
/// TLS stack is initialized once; subsequent `AiClient::new` calls are free.
///
/// Proxy resolution: respects the standard proxy env vars when present
/// (reqwest does this by default), and otherwise falls back to the system
/// proxy detected via `scutil --proxy` on macOS. Without that fallback,
/// launches from the menu bar / Finder inherit launchd's empty environment
/// and silently bypass the user's configured proxy — the same hazard already
/// fixed in the curl-based update path.
fn shared_http_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let mut builder = reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(30))
            .timeout(std::time::Duration::from_secs(600));

        if let Some(proxy_url) = config::proxy::detect_system_proxy() {
            match reqwest::Proxy::all(&proxy_url) {
                Ok(proxy) => {
                    log::info!("AI HTTP client using system proxy: {}", proxy_url);
                    builder = builder.proxy(proxy);
                }
                Err(e) => log::warn!(
                    "Failed to apply detected system proxy {}: {}; continuing without proxy",
                    proxy_url,
                    e
                ),
            }
        }

        builder.build().unwrap_or_else(|e| {
            log::warn!("Failed to build HTTP client: {e}; falling back to default client");
            reqwest::blocking::Client::new()
        })
    })
}

impl AiClient {
    pub fn new(config: AssistantConfig) -> Self {
        Self {
            config,
            client: shared_http_client().clone(),
        }
    }

    /// Whether this client will include tools in chat requests.
    pub fn tools_enabled(&self) -> bool {
        self.config.chat_tools_enabled
    }

    /// Returns a reference to the loaded assistant configuration.
    pub fn config(&self) -> &AssistantConfig {
        &self.config
    }

    /// Single-shot (non-streaming) completion for short tasks like title generation.
    ///
    /// Internally uses `chat_step` with an empty tools list and accumulates all tokens
    /// into a String. The returned text is trimmed of leading/trailing whitespace.
    pub fn complete_once(&self, model: &str, messages: &[ApiMessage]) -> Result<String> {
        let cancelled = AtomicBool::new(false);
        let mut text = String::new();
        self.chat_step(model, messages, &[], &cancelled, &mut |tok| {
            text.push_str(tok);
        })?;
        Ok(text.trim().to_string())
    }

    /// Fetch available chat models from `{base_url}/models`.
    /// Filters out non-chat models (embeddings, TTS, image, etc.).
    pub fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/models", self.config.base_url);
        let req = self.client.get(&url);
        let req = self.apply_auth_headers(req)?;
        let resp = req.send().context("GET /models failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            anyhow::bail!("models API {}: {}", status, body);
        }
        let v: serde_json::Value = resp.json().context("parse /models response")?;
        let arr = v
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| anyhow::anyhow!("missing `data` array in /models response"))?;
        let mut out: Vec<String> = arr
            .iter()
            .filter_map(|m| m.get("id").and_then(|s| s.as_str()).map(String::from))
            .filter(|id| kaku_ai_utils::is_chat_model_id(id))
            .collect();
        out.sort();
        out.dedup();
        out.truncate(30);
        Ok(out)
    }

    /// Build provider-specific auth headers for the HTTP request builder.
    fn apply_auth_headers(
        &self,
        req: reqwest::blocking::RequestBuilder,
    ) -> Result<reqwest::blocking::RequestBuilder> {
        let req = match self.config.auth_type.as_str() {
            "copilot" => {
                let token = ai_auth::get_copilot_token(&self.client)?;
                req.header("Authorization", format!("Bearer {token}"))
                    .header("Copilot-Integration-Id", "vscode-chat")
                    .header("Editor-Version", "vscode/1.110.1")
                    .header("Editor-Plugin-Version", "copilot-chat/0.38.2")
                    .header("Openai-Organization", "github-copilot")
                    .header("Openai-Intent", "conversation-panel")
            }
            "codex" => {
                let token = ai_auth::read_codex_access_token().ok_or_else(|| {
                    anyhow::anyhow!("Codex: not logged in. Run `codex auth login` to authenticate.")
                })?;
                req.header("Authorization", format!("Bearer {token}"))
            }
            _ => {
                // Default: api_key as a Bearer header for all OpenAI-compatible
                // providers (OpenAI, DeepSeek, Kimi, custom proxies).
                req.header("Authorization", format!("Bearer {}", self.config.api_key))
            }
        };
        self.apply_custom_headers(req)
    }

    fn apply_custom_headers(
        &self,
        mut req: reqwest::blocking::RequestBuilder,
    ) -> Result<reqwest::blocking::RequestBuilder> {
        for (name, value) in &self.config.custom_headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("invalid custom header name `{name}`"))?;
            let header_value = HeaderValue::from_str(value)
                .with_context(|| format!("invalid custom header value for `{name}`"))?;
            req = req.header(header_name, header_value);
        }
        Ok(req)
    }

    /// Single chat step with optional tool support.
    ///
    /// Streams text tokens via `on_token`. If the model responds by requesting
    /// tool calls instead of (or before) text, returns those calls for the
    /// caller to execute and loop. Returns an empty vec when the step is text-only.
    ///
    /// The caller must set `cancelled` to `true` to abort mid-stream.
    pub fn chat_step(
        &self,
        model: &str,
        messages: &[ApiMessage],
        tools: &[serde_json::Value],
        cancelled: &AtomicBool,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<Vec<ToolCall>> {
        let url = format!("{}/chat/completions", self.config.base_url);

        let mut body = serde_json::json!({
            "model": model,
            "messages": messages.iter().map(|m| m.0.clone()).collect::<Vec<_>>(),
            "stream": true,
        });
        if !tools.is_empty() && self.config.chat_tools_enabled {
            body["tools"] = serde_json::Value::Array(tools.to_vec());
        }

        let req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Accept-Encoding", "identity")
            .json(&body);
        let req = self.apply_auth_headers(req)?;

        let response = send_with_retry(req, "API", cancelled)?;

        let reader = BufReader::new(response);
        // Accumulate tool call fragments by index; each index is one pending call.
        // BTreeMap keeps indices sorted so we process them in order.
        let mut tc_buf: BTreeMap<usize, ToolCallBuf> = BTreeMap::new();
        let mut finish_reason = String::new();

        for line in reader.lines() {
            if cancelled.load(Ordering::Relaxed) {
                break;
            }
            let line = line.context("read SSE line")?;
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            if data.trim() == "[DONE]" {
                break;
            }
            let chunk = match serde_json::from_str::<serde_json::Value>(data) {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("Failed to parse SSE chunk: {e}");
                    continue;
                }
            };

            let Some(choice) = chunk["choices"].get(0) else {
                continue;
            };

            // Capture finish_reason when present.
            if let Some(fr) = choice["finish_reason"].as_str() {
                if !fr.is_empty() && fr != "null" {
                    finish_reason = fr.to_string();
                }
            }

            let delta = &choice["delta"];

            // Text delta (standard) and reasoning delta (DeepSeek et al.).
            if let Some(reasoning) = delta["reasoning_content"]
                .as_str()
                .or_else(|| choice["reasoning"].as_str())
            {
                if !reasoning.is_empty() {
                    on_token(&format!("<think>\n{}\n</think>\n\n", reasoning));
                }
            }
            if let Some(content) = delta["content"].as_str() {
                on_token(content);
            }

            // Tool call deltas: accumulate arguments by index.
            if let Some(tc_arr) = delta["tool_calls"].as_array() {
                for tc in tc_arr {
                    let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                    let entry = tc_buf.entry(idx).or_default();
                    if let Some(id) = tc["id"].as_str() {
                        entry.id = id.to_string();
                    }
                    if let Some(name) = tc["function"]["name"].as_str() {
                        entry.name = name.to_string();
                    }
                    if let Some(args) = tc["function"]["arguments"].as_str() {
                        if entry.arguments.len() < 65_536 {
                            entry.arguments.push_str(args);
                        }
                    }
                }
            }
        }

        // Build ToolCall results. Some proxies (e.g. vivgrid) never set
        // finish_reason to "tool_calls" even when streaming tool call deltas,
        // so fall back to any accumulated tc_buf entries with a valid name.
        if finish_reason == "tool_calls" || !tc_buf.is_empty() {
            let calls = tc_buf
                .into_values()
                .filter(|b| !b.name.is_empty())
                .map(|b| ToolCall {
                    id: b.id,
                    name: b.name,
                    arguments: b.arguments,
                })
                .collect::<Vec<_>>();
            if calls.is_empty() {
                Ok(vec![])
            } else {
                Ok(calls)
            }
        } else {
            Ok(vec![])
        }
    }
}

/// Send a request up to 3 times with exponential backoff on transient
/// failures (network errors, HTTP 429, HTTP 5xx). Non-retryable HTTP errors
/// (4xx other than 429) bail immediately so misconfiguration surfaces fast.
///
/// `provider_label` is folded into log lines and the final error message so a
/// user reading logs can tell which transport failed.
fn send_with_retry(
    req: reqwest::blocking::RequestBuilder,
    provider_label: &str,
    cancelled: &AtomicBool,
) -> Result<reqwest::blocking::Response> {
    let mut last_err = String::new();
    for attempt in 0..3u32 {
        if attempt > 0 {
            let backoff = std::time::Duration::from_secs(1 << attempt);
            std::thread::sleep(backoff);
            if cancelled.load(Ordering::Relaxed) {
                anyhow::bail!("cancelled during retry backoff");
            }
        }
        let r = match req.try_clone().context("clone request")?.send() {
            Ok(r) => r,
            Err(e) => {
                last_err = e.to_string();
                log::warn!(
                    "{} HTTP attempt {}: {}",
                    provider_label,
                    attempt + 1,
                    last_err
                );
                continue;
            }
        };
        let status = r.status();
        if status.is_success() {
            return Ok(r);
        }
        let code = status.as_u16();
        let body = r.text().unwrap_or_default();
        if code == 429 || code >= 500 {
            let preview: String = body.chars().take(200).collect();
            last_err = format!("{} error {}: {}", provider_label, code, preview);
            log::warn!(
                "{} HTTP attempt {} retryable: {}",
                provider_label,
                attempt + 1,
                last_err
            );
            continue;
        }
        anyhow::bail!("{} error {}: {}", provider_label, code, body);
    }
    Err(anyhow::anyhow!(
        "{} request failed after 3 attempts: {}",
        provider_label,
        last_err
    ))
}

// ─── Private helpers ──────────────────────────────────────────────────────────

/// Buffer for accumulating streamed tool call fragments.
#[derive(Default)]
struct ToolCallBuf {
    id: String,
    name: String,
    arguments: String,
}

/// Maps (base_url, auth_type) to a display provider name.
///
/// Single source of truth for provider naming. The `kaku` binary used to
/// carry a parallel `#[allow(dead_code)]` table; that copy was removed in
/// V0.10.0 because it never matched the GUI version under maintenance.
fn detect_provider_with_auth(base_url: &str, auth_type: &str) -> &'static str {
    let normalized = base_url.trim().trim_end_matches('/').to_ascii_lowercase();
    match (normalized.as_str(), auth_type) {
        ("https://api.githubcopilot.com", _) => "Copilot",
        ("https://api.openai.com/v1", "codex") => "Codex",
        _ => "Custom",
    }
}

// Delegated to kaku-ai-utils crate to avoid cross-binary drift.

#[cfg(test)]
mod tests {
    use super::{detect_provider_with_auth, parse_custom_headers};

    #[test]
    fn detects_copilot_and_codex_and_falls_back_to_custom() {
        assert_eq!(
            detect_provider_with_auth("https://api.githubcopilot.com", "copilot"),
            "Copilot"
        );
        assert_eq!(
            detect_provider_with_auth("https://api.openai.com/v1", "codex"),
            "Codex"
        );
        // Same OpenAI URL with the default api_key auth is treated as a generic
        // OpenAI-compatible endpoint, so we surface it as Custom.
        assert_eq!(
            detect_provider_with_auth("https://api.openai.com/v1", "api_key"),
            "Custom"
        );
        // Unknown / removed providers (Gemini was dropped in V0.10.0) fall
        // through to Custom rather than crashing detection.
        assert_eq!(
            detect_provider_with_auth("https://generativelanguage.googleapis.com", "gemini_key"),
            "Custom"
        );
        assert_eq!(detect_provider_with_auth("", "api_key"), "Custom");
    }

    #[test]
    fn trailing_slash_does_not_break_match() {
        assert_eq!(
            detect_provider_with_auth("https://api.githubcopilot.com/", "copilot"),
            "Copilot"
        );
        assert_eq!(
            detect_provider_with_auth("https://api.openai.com/v1/", "codex"),
            "Codex"
        );
    }

    #[test]
    fn parses_custom_headers_from_array_and_rejects_bad_entries() {
        let value = toml::Value::Array(vec![
            toml::Value::String("X-Customer-ID: acme".to_string()),
            toml::Value::String("X-Trace: abc:123".to_string()),
        ]);
        let headers = parse_custom_headers(Some(&value)).unwrap();
        assert_eq!(
            headers,
            vec![
                ("X-Customer-ID".to_string(), "acme".to_string()),
                ("X-Trace".to_string(), "abc:123".to_string())
            ]
        );

        let bad = toml::Value::Array(vec![toml::Value::String("missing-colon".to_string())]);
        assert!(parse_custom_headers(Some(&bad)).is_err());

        let reserved =
            toml::Value::Array(vec![toml::Value::String("Authorization: nope".to_string())]);
        assert!(parse_custom_headers(Some(&reserved)).is_err());
    }
}
