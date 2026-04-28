//! Standalone CLI renderer for the `k` command.
//!
//! Renders StreamMsg events to stdout/stderr and drives the rustyline REPL.

use crate::ai_chat_engine::{Engine, StreamMsg};
use crate::ai_conversations;
use std::io::Write;

// ── Entry point ───────────────────────────────────────────────────────────────

pub struct CliArgs {
    /// If Some, run a single one-shot query then exit.
    pub prompt: Option<String>,
    /// Force a new conversation even when a cwd mapping exists.
    pub new: bool,
    /// List recent conversations and optionally resume by ID.
    pub resume: Option<Option<String>>,
}

/// Main entry point for the `k` binary.
pub fn run(args: CliArgs) -> anyhow::Result<()> {
    // Load assistant config.
    let cfg = crate::ai_client::AssistantConfig::load()?;
    let model = cfg.chat_model.clone();
    let client = crate::ai_client::AiClient::new(cfg);

    // Handle --resume: list or switch.
    if let Some(resume_arg) = args.resume {
        let convs = ai_conversations::load_index();
        if convs.is_empty() {
            eprintln!("No conversations found.");
            return Ok(());
        }
        if let Some(id) = resume_arg {
            let cwd = std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut engine = Engine::with_conv_id(cwd.clone(), client, model, &id)?;
            let _ = ai_conversations::write_cwd_index(&cwd, &id);
            return run_repl(&mut engine);
        } else {
            print_recent_conversations();
            return Ok(());
        }
    }

    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Resolve or create a conversation for this cwd.
    let conv_id = if args.new {
        ai_conversations::start_new_active()?
    } else {
        ai_conversations::resolve_or_create_conv_for_cwd(&cwd)?
    };

    let mut engine = Engine::with_conv_id(cwd.clone(), client, model, &conv_id)?;
    // Keep the cwd index up to date in case this is a newly created conv.
    let _ = ai_conversations::write_cwd_index(&cwd, &engine.active_id);

    if let Some(prompt) = args.prompt {
        // One-shot mode: run one round and exit.
        run_one_shot(&mut engine, prompt)?;
    } else {
        // Interactive REPL.
        run_repl(&mut engine)?;
    }

    Ok(())
}

// ── One-shot ──────────────────────────────────────────────────────────────────

fn run_one_shot(engine: &mut Engine, prompt: String) -> anyhow::Result<()> {
    let rx = engine.submit(prompt);
    let mut assistant_buf = String::new();
    let stdout = std::io::stdout();

    for msg in rx {
        match msg {
            StreamMsg::AssistantStart => {}
            StreamMsg::Token(t) => {
                assistant_buf.push_str(&t);
                let mut out = stdout.lock();
                let _ = out.write_all(t.as_bytes());
                let _ = out.flush();
            }
            StreamMsg::ToolStart { name, args_preview } => {
                eprintln!("  -> {} {}", name, args_preview);
            }
            StreamMsg::ToolDone { result_preview } => {
                eprintln!("     <- {}", result_preview);
            }
            StreamMsg::ToolFailed { error } => {
                eprintln!("     [error] {}", error);
            }
            StreamMsg::ApprovalRequired { summary, reply_tx } => {
                eprint!("Allow: {}? [y/N] ", summary);
                let _ = std::io::stderr().flush();
                let mut line = String::new();
                let approved = std::io::stdin()
                    .read_line(&mut line)
                    .map(|_| line.trim().to_lowercase() == "y")
                    .unwrap_or(false);
                let _ = reply_tx.send(approved);
            }
            StreamMsg::Done => {
                println!();
                break;
            }
            StreamMsg::Err(e) => {
                eprintln!("\n[error] {}", e);
                break;
            }
        }
    }

    if !assistant_buf.is_empty() {
        engine.record_assistant(assistant_buf);
        engine.spawn_post_round_tasks();
    }

    Ok(())
}

// ── REPL ──────────────────────────────────────────────────────────────────────

fn run_repl(engine: &mut Engine) -> anyhow::Result<()> {
    use rustyline::error::ReadlineError;
    use rustyline::DefaultEditor;

    let history_path = {
        let p = config::user_config_path();
        p.parent().map(|d| d.join("k_history.txt"))
    };

    let mut rl = DefaultEditor::new()?;
    if let Some(ref hp) = history_path {
        let _ = rl.load_history(hp);
    }

    let conv_count = engine.messages.iter().filter(|m| m.role == "user").count();
    if conv_count > 0 {
        eprintln!("[resuming conversation: {} turns]", conv_count);
    }
    eprintln!("k chat — /new  /resume  /clear  /status  /exit  Ctrl+D to quit");

    loop {
        let prompt = format!("k> ");
        let line = match rl.readline(&prompt) {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("[readline error] {}", e);
                break;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let _ = rl.add_history_entry(trimmed);

        // Slash commands.
        if trimmed.starts_with('/') {
            let mut parts = trimmed.splitn(2, ' ');
            let cmd = parts.next().unwrap_or("");
            let rest = parts.next().unwrap_or("").trim();
            match cmd {
                "/exit" | "/quit" => break,
                "/new" | "/clear" => {
                    engine.start_new()?;
                    let cwd = engine.cwd.clone();
                    let _ = ai_conversations::write_cwd_index(&cwd, &engine.active_id);
                    eprintln!(
                        "[{}]",
                        if cmd == "/new" {
                            "new conversation started"
                        } else {
                            "conversation cleared"
                        }
                    );
                    continue;
                }
                "/resume" => {
                    if rest.is_empty() {
                        print_recent_conversations();
                    } else {
                        match engine.switch_to(rest) {
                            Ok(_) => eprintln!("[switched to {}]", rest),
                            Err(e) => eprintln!("[error] {}", e),
                        }
                    }
                    continue;
                }
                "/status" => {
                    let turns = engine.messages.iter().filter(|m| m.role == "user").count();
                    eprintln!(
                        "conv: {}  turns: {}  cwd: {}",
                        engine.active_id, turns, engine.cwd
                    );
                    continue;
                }
                "/memory" => {
                    let path = crate::soul::memory_path();
                    match std::fs::read_to_string(&path) {
                        Ok(contents) if !contents.trim().is_empty() => println!("{}", contents),
                        Ok(_) => eprintln!("[no memories yet]"),
                        Err(_) => eprintln!("[no memory file at {}]", path.display()),
                    }
                    continue;
                }
                other => {
                    eprintln!("[unknown command: {}]", other);
                    continue;
                }
            }
        }

        // Regular prompt: stream the response.
        run_one_shot(engine, trimmed.to_string())?;
    }

    if let Some(ref hp) = history_path {
        let _ = rl.save_history(hp);
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn print_recent_conversations() {
    let convs = ai_conversations::load_index();
    if convs.is_empty() {
        eprintln!("No conversations.");
        return;
    }
    eprintln!("Recent conversations:");
    for (i, c) in convs.iter().take(10).enumerate() {
        let summary = if c.summary.trim().is_empty() {
            "(no summary)"
        } else {
            c.summary.as_str()
        };
        eprintln!("  {}  {}  {}", i + 1, c.id, summary);
    }
    eprintln!("Use /resume <id> to switch.");
}
