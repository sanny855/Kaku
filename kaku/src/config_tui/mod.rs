mod app;
mod ui;
mod state;

use anyhow::Context;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::path::PathBuf;


use config;
use crate::tui_core::components::{
    select_box::SelectBox, text_input::TextInput, toggle::Toggle,
};
use crate::tui_core::form::{FormApp, FormField, FormFieldWidget};
use crate::tui_core::EventResult;
use state::ConfigField;
pub use app::App;

pub fn run() -> anyhow::Result<()> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    stdout
        .execute(EnterAlternateScreen)
        .context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let mut app = App::new();
    app.load_config();
    app.capture_initial_theme();

    let (result, should_signal) = run_app(&mut terminal, &mut app);
    let theme_changed = app.theme_changed();

    disable_raw_mode().context("disable raw mode")?;
    terminal
        .backend_mut()
        .execute(LeaveAlternateScreen)
        .context("leave alternate screen")?;

    if should_signal {
        signal_config_changed();

        if theme_changed {
            config::reload();
            crate::tui_core::theme::clear_theme_cache();
            update_opencode_theme();
        }
    }

    result
}

use std::time::{Duration, Instant};

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> (anyhow::Result<()>, bool) {
    let mut last_modified: Option<Instant> = None;
    let debounce_duration = Duration::from_millis(500);

    loop {
        if let Err(e) = terminal.draw(|f| ui::ui(f, app)) {
            return (Err(e.into()), false);
        }

        let timeout = if let Some(last) = last_modified {
            debounce_duration.saturating_sub(last.elapsed())
        } else {
            // Block indefinitely if no pending changes, or poll slowly
            // We use a long duration to effectively block until an event occurs
            Duration::from_secs(60)
        };

        if crossterm::event::poll(timeout).unwrap_or(false) {
            let event = match event::read() {
                Ok(e) => e,
                Err(e) => return (Err(e.into()), false),
            };

            let Event::Key(key) = event else { continue };
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Err(e) = app.save_if_dirty() {
                        return (Err(e), app.has_saved);
                    }
                    return (Ok(()), app.has_saved);
                }
                KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                    if let Err(e) = app.save_if_dirty() {
                        return (Err(e), app.has_saved);
                    }
                    return (Ok(()), app.has_saved);
                }
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Err(e) = app.save_config() {
                        return (Err(e), app.has_saved);
                    }
                    last_modified = None;
                    continue;
                }
                KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Err(e) = open_config_in_editor() {
                        return (Err(e), app.has_saved);
                    }
                    return (Ok(()), app.has_saved);
                }
                _ => {}
            }

            let result = app.form.handle_event(&event);
            if matches!(result, EventResult::Changed) {
                app.dirty = true;
                last_modified = Some(Instant::now());
            }
        } else {
            // Timeout reached, which means debounce duration has passed since last modification
            if app.dirty && last_modified.map_or(false, |l| l.elapsed() >= debounce_duration) {
                if let Err(_e) = app.save_config() {
                    // Silently ignore save errors in background
                }
                last_modified = None;
            }
        }
    }
}


fn signal_config_changed() {
    use std::io::Write;
    let seq = if std::env::var("TMUX").is_ok() {
        b"\x1bPtmux;\x1b\x1b]1337;SetUserVar=KAKU_CONFIG_CHANGED=MQ==\x07\x1b\\" as &[u8]
    } else {
        b"\x1b]1337;SetUserVar=KAKU_CONFIG_CHANGED=MQ==\x07" as &[u8]
    };
    let _ = std::io::stdout().write_all(seq);
    let _ = std::io::stdout().flush();
}
fn update_opencode_theme() {
    let opencode_dir = config::HOME_DIR.join(".config").join("opencode");
    let themes_dir = opencode_dir.join("themes");
    let theme_file = themes_dir.join("kaku-match.json");
    let legacy_file = themes_dir.join("wezterm-match.json");
    let config_files = [
        opencode_dir.join("opencode.jsonc"),
        opencode_dir.join("opencode.json"),
        opencode_dir.join("tui.json"),
    ];

    // Migrate old users: remove legacy theme file.
    if legacy_file.exists() {
        let _ = std::fs::remove_file(&legacy_file);
    }

    // Update any known OpenCode config files to use the new theme name.
    for config_file in &config_files {
        if !config_file.exists() {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(config_file) {
            let updated = content.replace("\"wezterm-match\"", "\"kaku-match\"");
            if updated != content {
                let _ = std::fs::write(config_file, updated);
            }
        }
    }

    // Ensure themes directory exists
    if let Err(e) = std::fs::create_dir_all(&themes_dir) {
        eprintln!(
            "\x1b[33mWarning: Failed to create OpenCode themes directory: {}\x1b[0m",
            e
        );
        return;
    }

    let theme_content = crate::ai_config::opencode_theme_json();
    if let Err(e) = std::fs::write(&theme_file, theme_content.as_bytes()) {
        eprintln!(
            "\x1b[33mWarning: Failed to update OpenCode theme: {}\x1b[0m",
            e
        );
    }
}
fn open_config_in_editor() -> anyhow::Result<()> {
    let config_path = config::user_config_path();
    crate::utils::open_in_editor(&config_path)
}
