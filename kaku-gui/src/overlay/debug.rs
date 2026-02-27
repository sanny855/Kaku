use crate::scripting::guiwin::GuiWin;
use chrono::prelude::*;
use futures::FutureExt;
use log::Level;
use luahelper::ValuePrinter;
use mlua::Value;
use mux::termwiztermtab::TermWizTerminal;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use termwiz::cell::{AttributeChange, CellAttributes, Intensity};
use termwiz::color::AnsiColor;
use termwiz::input::{InputEvent, KeyCode, KeyEvent};
use termwiz::lineedit::*;
use termwiz::surface::Change;
use termwiz::terminal::Terminal;

lazy_static::lazy_static! {
    static ref LATEST_LOG_ENTRY: Mutex<Option<DateTime<Local>>> = Mutex::new(None);
}

struct LuaReplHost {
    history: BasicHistory,
    lua: mlua::Lua,
}

fn history_file_name() -> PathBuf {
    config::DATA_DIR.join("repl-history")
}

impl LuaReplHost {
    fn new(lua: mlua::Lua) -> Self {
        let mut history = BasicHistory::default();
        if let Ok(data) = std::fs::read_to_string(history_file_name()) {
            for line in data.lines() {
                history.add(line);
            }
        }
        Self { history, lua }
    }

    fn add_history(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }

        if let Some(last) = self.history.last() {
            if self.history.get(last).as_deref() == Some(line) {
                // Don't add duplicate lines
                return;
            }
        }
        self.history.add(line);
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(history_file_name())
        {
            writeln!(file, "{}", line).ok();
        }
    }
}

fn format_lua_err(err: mlua::Error) -> String {
    match err {
        mlua::Error::SyntaxError {
            incomplete_input: true,
            ..
        } => "...".to_string(),
        _ => format!("{:#}", err),
    }
}

fn fragment_to_expr_or_statement(lua: &mlua::Lua, text: &str) -> Result<String, String> {
    let expr = format!("return {};", text);

    let chunk = lua.load(&expr).set_name("=repl");
    match chunk.into_function() {
        Ok(_) => {
            // It's an expression
            Ok(text.to_string())
        }
        Err(_) => {
            // Try instead as a statement
            let chunk = lua.load(text).set_name("=repl");
            match chunk.into_function() {
                Ok(_) => Ok(text.to_string()),
                Err(err) => Err(format_lua_err(err)),
            }
        }
    }
}

impl LineEditorHost for LuaReplHost {
    fn history(&mut self) -> &mut dyn History {
        &mut self.history
    }

    fn resolve_action(
        &mut self,
        event: &InputEvent,
        editor: &mut LineEditor<'_>,
    ) -> Option<Action> {
        let (line, _cursor) = editor.get_line_and_cursor();
        if line.is_empty()
            && matches!(
                event,
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Escape,
                    ..
                })
            )
        {
            Some(Action::Cancel)
        } else {
            None
        }
    }

    fn render_preview(&self, line: &str) -> Vec<OutputElement> {
        let mut preview = vec![];

        if let Err(err) = fragment_to_expr_or_statement(&self.lua, line) {
            preview.push(OutputElement::Text(err))
        }

        preview
    }
}

pub fn show_debug_overlay(
    mut term: TermWizTerminal,
    gui_win: GuiWin,
    opengl_info: String,
    connection_info: String,
) -> anyhow::Result<()> {
    term.no_grab_mouse_in_raw_mode();

    let loaded = config::Config::load();
    if let Err(err) = &loaded.config {
        log::warn!(
            "Doctor panel failed to load user config and will try a fallback Lua context: {:#}",
            err
        );
    }
    for warning in &loaded.warnings {
        log::warn!("Doctor panel config warning: {}", warning);
    }

    let config::LoadedConfig { lua, .. } = loaded;
    // Try hard to fall back to some kind of working lua context even
    // if the user's config file is temporarily out of whack
    let lua = match lua {
        Some(lua) => lua,
        None => {
            log::warn!(
                "Doctor panel did not receive a Lua context from the loaded config; falling back"
            );
            match config::Config::try_default() {
                Ok(config::LoadedConfig { lua: Some(lua), .. }) => lua,
                _ => config::lua::make_lua_context(std::path::Path::new(""))?,
            }
        }
    };

    lua.load("wezterm = require 'wezterm'").exec()?;
    lua.globals().set("window", gui_win)?;
    let lua_version: String = lua.globals().get("_VERSION")?;

    let mut host = Some(LuaReplHost::new(lua));

    term.render(&[Change::Title("Kaku Doctor".to_string())])?;

    fn print_new_log_entries(term: &mut TermWizTerminal) -> termwiz::Result<()> {
        let entries = env_bootstrap::ringlog::get_entries();
        let mut changes = vec![];
        for entry in entries {
            if let Some(latest) = LATEST_LOG_ENTRY.lock().unwrap().as_ref() {
                if entry.then <= *latest {
                    // already seen this one
                    continue;
                }
            }
            LATEST_LOG_ENTRY.lock().unwrap().replace(entry.then);

            changes.push(Change::AllAttributes(CellAttributes::default()));
            changes.push(Change::Text(entry.then.format("%H:%M:%S%.3f ").to_string()));

            changes.push(
                AttributeChange::Foreground(match entry.level {
                    Level::Error => AnsiColor::Maroon.into(),
                    Level::Warn => AnsiColor::Red.into(),
                    Level::Info => AnsiColor::Green.into(),
                    Level::Debug => AnsiColor::Blue.into(),
                    Level::Trace => AnsiColor::Fuchsia.into(),
                })
                .into(),
            );
            changes.push(Change::Text(entry.level.as_str().to_string()));
            changes.push(Change::AllAttributes(CellAttributes::default()));
            changes.push(AttributeChange::Intensity(Intensity::Bold).into());
            changes.push(Change::Text(format!(" {}", entry.target)));
            changes.push(Change::AllAttributes(CellAttributes::default()));
            changes.push(Change::Text(format!(
                " > {}\r\n",
                entry.msg.replace("\n", "\r\n")
            )));
        }
        term.render(&changes)
    }

    let version = config::wezterm_version();
    let triple = config::wezterm_target_triple();
    let mut doctor_snapshot = PendingDoctorSnapshot::spawn();

    term.render(&[Change::Text(format!(
        "Kaku Doctor\r\n\
         Kaku version: {version} {triple}\r\n\
         Window Environment: {connection_info}\r\n\
         Lua Version: {lua_version}\r\n\
         {opengl_info}\r\n\
         {}\
         Enter lua statements or expressions and hit Enter.\r\n\
         Press ESC or CTRL-D to exit\r\n",
        doctor_snapshot.placeholder_text(),
    ))])?;
    doctor_snapshot.render_if_ready(&mut term)?;
    // `read_line` blocks until an input event arrives. Give the background
    // doctor snapshot a short window to render before entering the editor loop,
    // so the panel does not appear to "require Enter" to show results.
    doctor_snapshot.wait_for_initial_render(&mut term)?;

    loop {
        doctor_snapshot.render_if_ready(&mut term)?;
        print_new_log_entries(&mut term)?;
        let mut editor = LineEditor::new(&mut term);
        editor.set_prompt("> ");
        if let Some(line) = editor.read_line(host.as_mut().unwrap())? {
            if line.is_empty() {
                continue;
            }
            host.as_mut().unwrap().add_history(&line);

            let passed_host = host.take().unwrap();

            let (host_res, text) =
                smol::block_on(promise::spawn::spawn_into_main_thread(async move {
                    evaluate_trampoline(passed_host, line)
                        .recv()
                        .await
                        .map_err(|e| mlua::Error::external(format!("{:#}", e)))
                        .expect("returning result not to fail")
                }));

            host.replace(host_res);

            if text != "nil" {
                term.render(&[Change::Text(format!("{}\r\n", text.replace("\n", "\r\n")))])?;
            }
            doctor_snapshot.render_if_ready(&mut term)?;
        } else {
            return Ok(());
        }
    }
}

struct PendingDoctorSnapshot {
    receiver: Option<mpsc::Receiver<String>>,
    rendered: bool,
}

impl PendingDoctorSnapshot {
    fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(doctor_snapshot_text());
        });
        Self {
            receiver: Some(rx),
            rendered: false,
        }
    }

    fn placeholder_text(&self) -> &'static str {
        "Doctor snapshot is capturing in background and will print below when ready.\r\n\r\n"
    }

    fn render_if_ready(&mut self, term: &mut TermWizTerminal) -> termwiz::Result<()> {
        if self.rendered {
            return Ok(());
        }

        let Some(rx) = self.receiver.as_ref() else {
            return Ok(());
        };

        match rx.try_recv() {
            Ok(text) => {
                self.rendered = true;
                self.receiver.take();
                term.render(&[Change::Text(text)])
            }
            Err(mpsc::TryRecvError::Empty) => Ok(()),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.rendered = true;
                self.receiver.take();
                term.render(&[Change::Text(
                    "Kaku Doctor Snapshot\r\nFailed to capture doctor snapshot in background.\r\n\r\n"
                        .to_string(),
                )])
            }
        }
    }

    fn wait_for_initial_render(&mut self, term: &mut TermWizTerminal) -> termwiz::Result<()> {
        if self.rendered {
            return Ok(());
        }

        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            self.render_if_ready(term)?;
            if self.rendered {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        Ok(())
    }
}

fn doctor_snapshot_text() -> String {
    let wrapper_hint = doctor_wrapper_repair_hint_text();
    let Some(kaku_bin) = resolve_kaku_cli_for_doctor() else {
        return format!(
            "Kaku Doctor Snapshot\r\n\
             {}\
             kaku doctor unavailable because Kaku CLI binary was not found.\r\n\
             Expected sibling binary named `kaku` next to `kaku-gui` or in /Applications/Kaku.app.\r\n\
             \r\n",
            wrapper_hint
        );
    };

    // Run with a timeout so the panel does not stay stuck forever if the
    // kaku subprocess hangs (e.g. wrapper on a slow network mount).
    let mut child = match Command::new(&kaku_bin)
        .arg("doctor")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            return format!(
                "Kaku Doctor Snapshot\r\n\
                 {}\
                 Failed to execute {} doctor\r\n\
                 Error: {}\r\n\
                 \r\n",
                wrapper_hint,
                kaku_bin.display(),
                err
            );
        }
    };

    let deadline = Instant::now() + Duration::from_secs(10);
    let output_result = loop {
        match child.try_wait() {
            Ok(Some(_)) => break child.wait_with_output(),
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return format!(
                    "Kaku Doctor Snapshot\r\n\
                     {}\
                     Timed out waiting for `kaku doctor` after 10 seconds.\r\n\
                     The kaku binary may be inaccessible or hanging. Try running `kaku doctor` in a terminal.\r\n\
                     \r\n",
                    wrapper_hint
                );
            }
            Err(err) => {
                return format!(
                    "Kaku Doctor Snapshot\r\n\
                     {}\
                     Failed while waiting for {} doctor\r\n\
                     Error: {}\r\n\
                     \r\n",
                    wrapper_hint,
                    kaku_bin.display(),
                    err
                );
            }
        }
    };

    match output_result {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let normalized = stdout.replace('\n', "\r\n");
            format!("Kaku Doctor Snapshot\r\n{}{normalized}\r\n", wrapper_hint)
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).replace('\n', "\r\n");
            format!(
                "Kaku Doctor Snapshot\r\n\
                 {}\
                 Failed to run: {} doctor\r\n\
                 Exit status: {}\r\n\
                 {}\r\n",
                wrapper_hint,
                kaku_bin.display(),
                output.status,
                if stderr.is_empty() {
                    "No stderr output".to_string()
                } else {
                    format!("stderr: {stderr}")
                }
            )
        }
        Err(err) => format!(
            "Kaku Doctor Snapshot\r\n\
             {}\
             Failed to execute {} doctor\r\n\
             Error: {}\r\n\
             \r\n",
            wrapper_hint,
            kaku_bin.display(),
            err
        ),
    }
}

fn doctor_wrapper_repair_hint_text() -> String {
    let wrapper = config::HOME_DIR
        .join(".config")
        .join("kaku")
        .join("zsh")
        .join("bin")
        .join("kaku");

    if config::is_executable_file(&wrapper) {
        return String::new();
    }

    let mut lines = Vec::new();
    lines.push(format!(
        "Shell command entry is missing or not executable: {}\r\n",
        wrapper.display()
    ));
    lines.push("Fix from a terminal, then reload zsh:\r\n".to_string());

    if let Some(kaku_bin) = resolve_kaku_app_bin_for_repair() {
        lines.push(format!("  {} init --update-only\r\n", kaku_bin.display()));
    } else {
        lines.push(
            "  /Applications/Kaku.app/Contents/MacOS/kaku init --update-only\r\n".to_string(),
        );
        lines.push(
            "  ~/Applications/Kaku.app/Contents/MacOS/kaku init --update-only\r\n".to_string(),
        );
    }
    lines.push("  exec zsh -l\r\n".to_string());
    lines.push("\r\n".to_string());

    lines.concat()
}

fn resolve_kaku_cli_for_doctor() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("kaku"));
        }
    }

    candidates.push(PathBuf::from("/Applications/Kaku.app/Contents/MacOS/kaku"));
    candidates.push(
        config::HOME_DIR
            .join("Applications")
            .join("Kaku.app")
            .join("Contents")
            .join("MacOS")
            .join("kaku"),
    );

    if let Some(path_os) = std::env::var_os("PATH") {
        for entry in std::env::split_paths(&path_os) {
            candidates.push(entry.join("kaku"));
        }
    }

    candidates
        .into_iter()
        .find(|p| config::is_executable_file(p))
}

fn resolve_kaku_app_bin_for_repair() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("/Applications/Kaku.app/Contents/MacOS/kaku"),
        config::HOME_DIR
            .join("Applications")
            .join("Kaku.app")
            .join("Contents")
            .join("MacOS")
            .join("kaku"),
    ];
    candidates
        .iter()
        .find(|p| config::is_executable_file(p))
        .cloned()
}

// A bit of indirection because spawn_into_main_thread wants the
// overall future to be Send but mlua::Value, mlua::Chunk are not
// Send.  We need to split off the actual evaluation future to
// run separately, so we spawn it and use a channel to funnel
// the result back to the caller without blocking the gui thread.
fn evaluate_trampoline(
    host: LuaReplHost,
    expr: String,
) -> smol::channel::Receiver<(LuaReplHost, String)> {
    let (tx, rx) = smol::channel::bounded(1);
    promise::spawn::spawn(async move {
        let _ = tx.send(evaluate(host, expr).await).await;
    })
    .detach();
    rx
}

async fn evaluate(host: LuaReplHost, expr: String) -> (LuaReplHost, String) {
    async fn do_it(host: &LuaReplHost, expr: &str) -> String {
        let code = match fragment_to_expr_or_statement(&host.lua, expr) {
            Ok(code) => code,
            Err(err) => return err,
        };
        let chunk = host.lua.load(&code).set_name("repl");

        let result = chunk
            .eval_async::<Value>()
            .map(|result| match result {
                Ok(result) => {
                    let value = ValuePrinter(result);
                    format!("{:#?}", value)
                }
                Err(err) => format_lua_err(err),
            })
            .await;

        result
    }

    let result = do_it(&host, &expr).await;
    (host, result)
}
