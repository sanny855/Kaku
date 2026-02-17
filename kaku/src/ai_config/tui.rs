use anyhow::Context;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Terminal;
use std::io;
use std::path::PathBuf;

use std::sync::LazyLock;

struct Theme {
    primary: Color,
    secondary: Color,
    accent: Color,
    error: Color,
    text: Color,
    muted: Color,
    bg: Color,
    panel: Color,
}

fn parse_hex(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    Color::Rgb(r, g, b)
}

static THEME: LazyLock<Theme> = LazyLock::new(|| {
    let json: serde_json::Value =
        serde_json::from_str(super::OPENCODE_THEME_JSON).unwrap_or_default();
    let defs = &json["defs"];
    let hex =
        |key: &str, fallback: &str| -> Color { parse_hex(defs[key].as_str().unwrap_or(fallback)) };
    Theme {
        primary: hex("primary", "#a277ff"),
        secondary: hex("secondary", "#61ffca"),
        accent: hex("accent", "#ffca85"),
        error: hex("error", "#ff6767"),
        text: hex("text", "#edecee"),
        muted: hex("muted", "#6b6b6b"),
        bg: hex("bg", "#15141b"),
        panel: hex("element", "#1f1d28"),
    }
});

macro_rules! define_colors {
    ($($name:ident => $field:ident),* $(,)?) => {
        $(
            #[allow(non_snake_case)]
            fn $name() -> Color { THEME.$field }
        )*
    }
}

define_colors! {
    PURPLE => primary,
    GREEN => secondary,
    YELLOW => accent,
    RED => error,
    TEXT => text,
    MUTED => muted,
    BG => bg,
    PANEL => panel,
}

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    ClaudeCode,
    Codex,
    Gemini,
    Copilot,
    OpenCode,
    OpenClaw,
}

impl Tool {
    fn label(&self) -> &'static str {
        match self {
            Tool::ClaudeCode => "Claude Code",
            Tool::Codex => "Codex",
            Tool::Gemini => "Gemini CLI",
            Tool::Copilot => "Copilot CLI",
            Tool::OpenCode => "OpenCode",
            Tool::OpenClaw => "OpenClaw",
        }
    }

    fn config_path(&self) -> PathBuf {
        let home = config::HOME_DIR.clone();
        match self {
            Tool::ClaudeCode => home.join(".claude").join("settings.json"),
            Tool::Codex => home.join(".codex").join("config.toml"),
            Tool::Gemini => home.join(".gemini").join("settings.json"),
            Tool::Copilot => home.join(".copilot").join("config.json"),
            Tool::OpenCode => home.join(".config").join("opencode").join("opencode.json"),
            Tool::OpenClaw => {
                let new_path = home.join(".openclaw").join("openclaw.json");
                if new_path.exists() {
                    return new_path;
                }
                let legacy = home.join(".clawdbot").join("clawdbot.json");
                if legacy.exists() {
                    return legacy;
                }
                new_path
            }
        }
    }
}

const ALL_TOOLS: [Tool; 6] = [
    Tool::ClaudeCode,
    Tool::Codex,
    Tool::Gemini,
    Tool::Copilot,
    Tool::OpenCode,
    Tool::OpenClaw,
];

struct FieldEntry {
    key: String,
    value: String,
    options: Vec<String>,
    editable: bool,
}

impl Default for FieldEntry {
    fn default() -> Self {
        Self {
            key: String::new(),
            value: String::new(),
            options: Vec::new(),
            editable: true,
        }
    }
}

struct ToolState {
    tool: Tool,
    installed: bool,
    fields: Vec<FieldEntry>,
}

impl ToolState {
    fn load(tool: Tool) -> Self {
        let path = tool.config_path();
        if !path.exists() {
            return ToolState {
                tool,
                installed: false,
                fields: Vec::new(),
            };
        }

        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                return ToolState {
                    tool,
                    installed: true,
                    fields: vec![FieldEntry {
                        key: "error".into(),
                        value: "failed to read config".into(),
                        options: vec![],
                        ..Default::default()
                    }],
                }
            }
        };

        let fields = match tool {
            Tool::ClaudeCode => {
                let parsed: serde_json::Value = match serde_json::from_str(&raw) {
                    Ok(v) => v,
                    Err(_) => {
                        let stripped = strip_jsonc_comments(&raw);
                        serde_json::from_str(&stripped).unwrap_or_default()
                    }
                };
                extract_claude_code_fields(&parsed)
            }
            Tool::Codex => extract_codex_fields(&raw),
            Tool::Gemini => {
                let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
                extract_gemini_fields(&parsed)
            }
            Tool::Copilot => {
                let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
                extract_copilot_fields(&parsed)
            }
            Tool::OpenCode => {
                let parsed: serde_json::Value = match serde_json::from_str(&raw) {
                    Ok(v) => v,
                    Err(_) => {
                        let stripped = strip_jsonc_comments(&raw);
                        serde_json::from_str(&stripped).unwrap_or_default()
                    }
                };
                extract_opencode_fields(&parsed)
            }
            Tool::OpenClaw => {
                let parsed: serde_json::Value = match serde_json::from_str(&raw) {
                    Ok(v) => v,
                    Err(_) => {
                        let stripped = strip_jsonc_comments(&raw);
                        serde_json::from_str(&stripped).unwrap_or_default()
                    }
                };
                extract_openclaw_fields(&parsed)
            }
        };

        ToolState {
            tool,
            installed: true,
            fields,
        }
    }
}

fn strip_jsonc_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(&next) = chars.peek() {
                    out.push(next);
                    chars.next();
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }

        if c == '"' {
            in_string = true;
            out.push(c);
            continue;
        }

        if c == '/' {
            if let Some(&next) = chars.peek() {
                if next == '/' {
                    for ch in chars.by_ref() {
                        if ch == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                    continue;
                }
                if next == '*' {
                    chars.next();
                    while let Some(ch) = chars.next() {
                        if ch == '*' {
                            if chars.peek() == Some(&'/') {
                                chars.next();
                                break;
                            }
                        }
                    }
                    continue;
                }
            }
        }

        out.push(c);
    }
    out
}

fn json_str(val: &serde_json::Value, key: &str) -> String {
    val.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn mask_key(val: &str) -> String {
    if val.is_empty() {
        return "—".into();
    }
    if val.len() <= 8 {
        return "****".into();
    }
    format!("{}...****", &val[..8])
}

fn extract_claude_code_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let model = json_str(val, "model");

    let mut fields = vec![FieldEntry {
        key: "Primary Model".into(),
        value: if model.is_empty() {
            "default".into()
        } else {
            model
        },
        options: vec![],
        ..Default::default()
    }];

    // Show env-based provider config if present (e.g. OpenRouter, Pipe AI, Kimi)
    if let Some(env) = val.get("env").and_then(|e| e.as_object()) {
        let base_url = env
            .get("ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let auth_token = env
            .get("ANTHROPIC_AUTH_TOKEN")
            .or_else(|| env.get("ANTHROPIC_API_KEY"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !base_url.is_empty() {
            fields.push(FieldEntry {
                key: "Base URL".into(),
                value: base_url,
                options: vec![],
                ..Default::default()
            });
        }
        if !auth_token.is_empty() {
            fields.push(FieldEntry {
                key: "API Key".into(),
                value: mask_key(&auth_token),
                options: vec![],
                ..Default::default()
            });
        }
    }

    fields
}

fn extract_codex_fields(raw: &str) -> Vec<FieldEntry> {
    let mut fields = Vec::new();

    // Parse TOML manually for the fields we care about
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim().trim_matches('"');
            let val = val.trim().trim_matches('"');
            match key {
                "model" => {
                    fields.push(FieldEntry {
                        key: "Model".into(),
                        value: val.to_string(),
                        options: vec![],
                        editable: false,
                    });
                }
                "model_reasoning_effort" => {
                    fields.push(FieldEntry {
                        key: "Reasoning Effort".into(),
                        value: val.to_string(),
                        options: vec![],
                        editable: false,
                    });
                }
                _ => {}
            }
        }
    }

    // Check auth status from auth.json
    let auth_path = config::HOME_DIR.join(".codex").join("auth.json");
    if let Ok(auth_raw) = std::fs::read_to_string(&auth_path) {
        if let Ok(auth) = serde_json::from_str::<serde_json::Value>(&auth_raw) {
            let auth_mode = auth.get("auth_mode").and_then(|v| v.as_str()).unwrap_or("");
            if !auth_mode.is_empty() {
                fields.push(FieldEntry {
                    key: "Auth".into(),
                    value: format!("✓ connected ({})", auth_mode),
                    options: vec![],
                    editable: false,
                });
            }
        }
    }

    fields
}

fn extract_gemini_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let mut fields = Vec::new();

    let auth_type = val
        .get("security")
        .and_then(|s| s.get("auth"))
        .and_then(|a| a.get("selectedType"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !auth_type.is_empty() {
        fields.push(FieldEntry {
            key: "Auth".into(),
            value: format!("✓ connected ({})", auth_type),
            options: vec![],
            editable: false,
        });
    }

    fields
}

fn extract_copilot_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let model = json_str(val, "model");

    let mut fields = vec![FieldEntry {
        key: "Model".into(),
        value: if model.is_empty() {
            "default".into()
        } else {
            model
        },
        options: vec![],
        editable: false,
    }];

    // Copilot authenticates via GitHub OAuth; session files indicate auth
    let session_dir = config::HOME_DIR.join(".copilot").join("session-state");
    if session_dir.exists() {
        fields.push(FieldEntry {
            key: "Auth".into(),
            value: "✓ connected (github)".into(),
            options: vec![],
            editable: false,
        });
    }

    fields
}

fn extract_opencode_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let primary_model = json_str(val, "model");
    let small_model = json_str(val, "small_model");
    let theme = json_str(val, "theme");

    let mut fields = vec![
        FieldEntry {
            key: "Primary Model".into(),
            value: if primary_model.is_empty() {
                "—".into()
            } else {
                primary_model
            },
            options: vec![],
            ..Default::default()
        },
        FieldEntry {
            key: "Small Model".into(),
            value: if small_model.is_empty() {
                "—".into()
            } else {
                small_model
            },
            options: vec![],
            ..Default::default()
        },
        FieldEntry {
            key: "Theme".into(),
            value: if theme.is_empty() {
                "—".into()
            } else {
                theme
            },
            options: vec![],
            editable: false,
        },
    ];

    // Read auth.json for provider authentication status
    let auth_path = config::HOME_DIR
        .join(".local")
        .join("share")
        .join("opencode")
        .join("auth.json");
    if let Ok(auth_raw) = std::fs::read_to_string(&auth_path) {
        if let Ok(auth) = serde_json::from_str::<serde_json::Value>(&auth_raw) {
            if let Some(obj) = auth.as_object() {
                // Sort: well-known providers first, then rest alphabetically
                let priority = |name: &str| -> usize {
                    match name {
                        n if n.contains("claude") || n.contains("anthropic") => 0,
                        "openai" => 1,
                        "google" => 2,
                        "github-copilot" => 3,
                        _ => 4,
                    }
                };
                let mut entries: Vec<_> = obj.iter().collect();
                entries.sort_by(|(a, _), (b, _)| {
                    let pa = priority(a);
                    let pb = priority(b);
                    pa.cmp(&pb).then_with(|| a.cmp(b))
                });

                for (name, entry) in entries {
                    let auth_type = entry
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let status = match auth_type.as_str() {
                        "api" => {
                            let key = entry.get("key").and_then(|v| v.as_str()).unwrap_or("");
                            format!("✓ {}", mask_key(key))
                        }
                        "oauth" => "✓ connected".into(),
                        _ => auth_type.clone(),
                    };

                    fields.push(FieldEntry {
                        key: name.clone(),
                        value: format!("{} ({})", status, auth_type),
                        options: vec![],
                        editable: false,
                    });
                }
            }
        }
    }

    // Dynamically enumerate providers from config
    if let Some(providers) = val.get("provider").and_then(|p| p.as_object()) {
        for (name, prov) in providers {
            let opts = prov.get("options").unwrap_or(&serde_json::Value::Null);
            let url = opts
                .get("baseURL")
                .or_else(|| opts.get("base_url"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let key = opts
                .get("apiKey")
                .or_else(|| opts.get("api_key"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let display_name = prov
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Collect model names from this provider
            let models_display = prov
                .get("models")
                .and_then(|m| m.as_object())
                .map(|obj| {
                    obj.iter()
                        .map(|(id, m)| {
                            m.get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or(id)
                                .to_string()
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();

            // Provider header
            fields.push(FieldEntry {
                key: name.clone(),
                value: if display_name.is_empty() {
                    "provider".into()
                } else {
                    display_name
                },
                options: vec![],
                editable: false,
            });

            if !url.is_empty() {
                fields.push(FieldEntry {
                    key: format!("{} ▸ Base URL", name),
                    value: url,
                    options: vec![],
                    ..Default::default()
                });
            }
            if !key.is_empty() {
                fields.push(FieldEntry {
                    key: format!("{} ▸ API Key", name),
                    value: mask_key(&key),
                    options: vec![],
                    ..Default::default()
                });
            }
            if !models_display.is_empty() {
                fields.push(FieldEntry {
                    key: format!("{} ▸ Models", name),
                    value: models_display,
                    options: vec![],
                    editable: false,
                });
            }
        }
    }

    fields
}

fn extract_openclaw_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let agents = val.get("agents").unwrap_or(&serde_json::Value::Null);
    let defaults = agents.get("defaults").unwrap_or(&serde_json::Value::Null);
    let model = defaults.get("model").unwrap_or(&serde_json::Value::Null);

    let primary = model
        .get("primary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Collect all model IDs from all providers for the Primary Model selector
    let mut all_model_ids: Vec<String> = Vec::new();

    let mut provider_fields: Vec<FieldEntry> = Vec::new();

    // Dynamically enumerate all providers
    if let Some(providers) = val
        .get("models")
        .and_then(|m| m.get("providers"))
        .and_then(|p| p.as_object())
    {
        for (name, prov) in providers {
            let url = prov
                .get("baseUrl")
                .or_else(|| prov.get("baseURL"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let key = prov
                .get("apiKey")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Collect models from this provider
            let model_ids: Vec<String> = prov
                .get("models")
                .and_then(|m| m.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|m| m.get("id").and_then(|v| v.as_str()))
                        .map(|id| format!("{}/{}", name, id))
                        .collect()
                })
                .unwrap_or_default();

            // Also check agents.defaults.models for any registered models with this provider
            if let Some(agent_models) = defaults.get("models").and_then(|m| m.as_object()) {
                for model_key in agent_models.keys() {
                    if model_key.starts_with(&format!("{}/", name))
                        && !all_model_ids.contains(model_key)
                    {
                        all_model_ids.push(model_key.clone());
                    }
                }
            }

            for mid in &model_ids {
                if !all_model_ids.contains(mid) {
                    all_model_ids.push(mid.clone());
                }
            }

            let models_display = if model_ids.is_empty() {
                // Show from agent defaults if provider models array is empty
                all_model_ids
                    .iter()
                    .filter(|m| m.starts_with(&format!("{}/", name)))
                    .map(|m| m.split('/').last().unwrap_or(m).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                model_ids
                    .iter()
                    .map(|m| m.split('/').last().unwrap_or(m).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };

            // Provider header
            let api_type = prov
                .get("api")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            provider_fields.push(FieldEntry {
                key: name.clone(),
                value: if api_type.is_empty() {
                    "provider".into()
                } else {
                    api_type
                },
                options: vec![],
                editable: false,
            });

            provider_fields.push(FieldEntry {
                key: format!("{} ▸ Base URL", name),
                value: if url.is_empty() { "—".into() } else { url },
                options: vec![],
                ..Default::default()
            });
            provider_fields.push(FieldEntry {
                key: format!("{} ▸ API Key", name),
                value: mask_key(&key),
                options: vec![],
                ..Default::default()
            });
            if !models_display.is_empty() {
                provider_fields.push(FieldEntry {
                    key: format!("{} ▸ Models", name),
                    value: models_display,
                    options: vec![],
                    editable: false,
                });
            }
        }
    }

    let mut fields = vec![FieldEntry {
        key: "Primary Model".into(),
        value: if primary.is_empty() {
            "—".into()
        } else {
            primary
        },
        options: all_model_ids,
        ..Default::default()
    }];

    fields.extend(provider_fields);

    let plugins = val
        .get("plugins")
        .and_then(|p| p.get("entries"))
        .and_then(|e| e.as_object())
        .map(|obj| {
            obj.keys()
                .filter(|k| {
                    obj.get(*k)
                        .and_then(|v| v.get("enabled"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true)
                })
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    fields.push(FieldEntry {
        key: "Plugins".into(),
        value: if plugins.is_empty() {
            "—".into()
        } else {
            plugins
        },
        options: vec![],
        editable: false,
    });

    fields
}

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    ToolList,
    Editor,
}

struct App {
    tools: Vec<ToolState>,
    tool_index: usize,
    field_index: usize,
    focus: Focus,
    editing: bool,
    edit_buf: String,
    selecting: bool,
    select_index: usize,
    select_options: Vec<String>,
    status_msg: Option<String>,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        let tools: Vec<ToolState> = ALL_TOOLS.iter().map(|t| ToolState::load(*t)).collect();
        App {
            tools,
            tool_index: 0,
            field_index: 0,
            focus: Focus::ToolList,
            editing: false,
            edit_buf: String::new(),
            selecting: false,
            select_index: 0,
            select_options: Vec::new(),
            status_msg: None,
            should_quit: false,
        }
    }

    fn total_rows(&self) -> usize {
        self.tools
            .iter()
            .map(|t| {
                if t.fields.is_empty() {
                    1
                } else {
                    t.fields.len()
                }
            })
            .sum()
    }

    fn flatten_index(&self) -> usize {
        let mut idx = 0;
        for (ti, tool) in self.tools.iter().enumerate() {
            if ti == self.tool_index {
                return idx + self.field_index;
            }
            idx += if tool.fields.is_empty() {
                1
            } else {
                tool.fields.len()
            };
        }
        idx
    }

    fn set_from_flat(&mut self, flat: usize) {
        let mut remaining = flat;
        for (ti, tool) in self.tools.iter().enumerate() {
            let count = if tool.fields.is_empty() {
                1
            } else {
                tool.fields.len()
            };
            if remaining < count {
                self.tool_index = ti;
                self.field_index = remaining;
                return;
            }
            remaining -= count;
        }
    }

    fn move_up(&mut self) {
        let flat = self.flatten_index();
        if flat > 0 {
            self.set_from_flat(flat - 1);
        }
    }

    fn move_down(&mut self) {
        let flat = self.flatten_index();
        if flat + 1 < self.total_rows() {
            self.set_from_flat(flat + 1);
        }
    }

    fn start_edit(&mut self) {
        let tool = &self.tools[self.tool_index];
        if !tool.installed || tool.fields.is_empty() {
            return;
        }
        if self.field_index >= tool.fields.len() {
            return;
        }
        if !tool.fields[self.field_index].editable {
            return;
        }
        let field = &tool.fields[self.field_index];
        if !field.options.is_empty() {
            self.selecting = true;
            self.select_options = field.options.clone();
            self.select_index = field
                .options
                .iter()
                .position(|o| *o == field.value)
                .unwrap_or(0);
            self.focus = Focus::Editor;
            return;
        }
        self.editing = true;
        // Clear buffer for placeholder "—" and masked API Key values
        self.edit_buf = if field.value == "—" || field.key.contains("API Key") {
            String::new()
        } else {
            field.value.clone()
        };
        self.focus = Focus::Editor;
    }

    fn confirm_select(&mut self) {
        if !self.selecting {
            return;
        }
        self.selecting = false;
        self.focus = Focus::ToolList;

        let tool = &mut self.tools[self.tool_index];
        if self.field_index >= tool.fields.len() {
            return;
        }
        if self.select_index >= self.select_options.len() {
            return;
        }

        let new_val = self.select_options[self.select_index].clone();
        if new_val == tool.fields[self.field_index].value {
            return;
        }

        tool.fields[self.field_index].value = new_val.clone();

        let field_key = tool.fields[self.field_index].key.clone();
        match save_field(tool.tool, &field_key, &new_val) {
            Ok(()) => self.status_msg = Some(format!("Saved {} → {}", field_key, new_val)),
            Err(e) => self.status_msg = Some(format!("Save failed: {}", e)),
        }
        self.reload_current_tool();
    }

    fn cancel_select(&mut self) {
        self.selecting = false;
        self.focus = Focus::ToolList;
    }

    fn confirm_edit(&mut self) {
        if !self.editing {
            return;
        }
        self.editing = false;
        self.focus = Focus::ToolList;

        let tool = &mut self.tools[self.tool_index];
        if self.field_index >= tool.fields.len() {
            return;
        }

        let new_val = self.edit_buf.trim().to_string();
        let field_key = tool.fields[self.field_index].key.clone();

        // Empty input on API Key fields means cancel, not clear
        if new_val.is_empty() && field_key.contains("API Key") {
            return;
        }

        let old_val = tool.fields[self.field_index].value.clone();
        if new_val == old_val || (new_val.is_empty() && old_val == "—") {
            return;
        }

        tool.fields[self.field_index].value = new_val.clone();

        match save_field(tool.tool, &field_key, &new_val) {
            Ok(()) => self.status_msg = Some(format!("Saved {} → {}", field_key, new_val)),
            Err(e) => self.status_msg = Some(format!("Save failed: {}", e)),
        }
        self.reload_current_tool();
    }

    fn reload_current_tool(&mut self) {
        let tool_type = self.tools[self.tool_index].tool;
        self.tools[self.tool_index] = ToolState::load(tool_type);
    }

    fn cancel_edit(&mut self) {
        self.editing = false;
        self.selecting = false;
        self.focus = Focus::ToolList;
    }

    fn sync_theme(&mut self) {
        match sync_kaku_theme() {
            Ok(msg) => self.status_msg = Some(msg),
            Err(e) => self.status_msg = Some(format!("Theme sync failed: {}", e)),
        }
        self.tools = ALL_TOOLS.iter().map(|t| ToolState::load(*t)).collect();
    }

    fn open_config(&self) {
        let tool = &self.tools[self.tool_index];
        let path = tool.tool.config_path();
        if !path.exists() {
            return;
        }
        let _ = std::process::Command::new("/usr/bin/open")
            .arg(&path)
            .status();
    }
}

fn save_field(tool: Tool, field_key: &str, new_val: &str) -> anyhow::Result<()> {
    let path = tool.config_path();
    if !path.exists() {
        anyhow::bail!("config file not found: {}", path.display());
    }

    let raw = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut parsed: serde_json::Value = serde_json::from_str(&raw)
        .or_else(|_| serde_json::from_str(&strip_jsonc_comments(&raw)))
        .with_context(|| format!("parse {}", path.display()))?;

    match tool {
        Tool::Codex | Tool::Gemini | Tool::Copilot => return Ok(()),
        Tool::ClaudeCode => {
            let env_key = match field_key {
                "Base URL" => Some("ANTHROPIC_BASE_URL"),
                "API Key" => Some("ANTHROPIC_AUTH_TOKEN"),
                _ => None,
            };
            let top_key = match field_key {
                "Primary Model" => Some("model"),
                _ => None,
            };

            if let Some(ek) = env_key {
                let obj = parsed.as_object_mut().context("root is not object")?;
                let env = obj.entry("env").or_insert_with(|| serde_json::json!({}));
                if let Some(env_obj) = env.as_object_mut() {
                    if new_val == "—" || new_val.is_empty() {
                        env_obj.remove(ek);
                    } else {
                        env_obj.insert(
                            ek.to_string(),
                            serde_json::Value::String(new_val.to_string()),
                        );
                    }
                }
            } else if let Some(tk) = top_key {
                if let Some(obj) = parsed.as_object_mut() {
                    if new_val == "—" || new_val.is_empty() {
                        obj.remove(tk);
                    } else {
                        obj.insert(
                            tk.to_string(),
                            serde_json::Value::String(new_val.to_string()),
                        );
                    }
                }
            } else {
                return Ok(());
            }
        }
        Tool::OpenCode => {
            // Top-level fields
            let top_key = match field_key {
                "Primary Model" => Some("model"),
                "Small Model" => Some("small_model"),
                "Theme" => Some("theme"),
                _ => None,
            };

            if let Some(tk) = top_key {
                if let Some(obj) = parsed.as_object_mut() {
                    if new_val == "—" || new_val.is_empty() {
                        obj.remove(tk);
                    } else {
                        obj.insert(
                            tk.to_string(),
                            serde_json::Value::String(new_val.to_string()),
                        );
                    }
                }
            } else if let Some(sep_pos) = field_key.find(" ▸ ") {
                // Provider sub-fields: "provider_name ▸ Base URL" / "provider_name ▸ API Key"
                let provider_name = &field_key[..sep_pos];
                let sub_field = &field_key[sep_pos + " ▸ ".len()..];
                let json_field = match sub_field {
                    "Base URL" => "baseURL",
                    "API Key" => "apiKey",
                    _ => return Ok(()),
                };

                let prov = parsed
                    .as_object_mut()
                    .context("root not object")?
                    .entry("provider")
                    .or_insert_with(|| serde_json::json!({}))
                    .as_object_mut()
                    .context("provider not object")?
                    .entry(provider_name)
                    .or_insert_with(|| serde_json::json!({}))
                    .as_object_mut()
                    .context("provider entry not object")?
                    .entry("options")
                    .or_insert_with(|| serde_json::json!({}));

                if let Some(obj) = prov.as_object_mut() {
                    if new_val == "—" || new_val.is_empty() {
                        obj.remove(json_field);
                    } else {
                        obj.insert(
                            json_field.to_string(),
                            serde_json::Value::String(new_val.to_string()),
                        );
                    }
                }
            } else {
                return Ok(());
            }
        }
        Tool::OpenClaw => {
            // Parse "provider_name ▸ Base URL" or "provider_name ▸ API Key"
            if let Some(sep_pos) = field_key.find(" ▸ ") {
                let provider_name = &field_key[..sep_pos];
                let sub_field = &field_key[sep_pos + " ▸ ".len()..];

                if sub_field == "Models" {
                    // Rename model key in agents.defaults.models
                    // old_display: "claude-opus-4-5-20251101" → full key: "provider/claude-opus-4-5-20251101"
                    // new_val:     "claude-opus-4-6"          → full key: "provider/claude-opus-4-6"
                    let defaults = parsed
                        .as_object_mut()
                        .context("root not object")?
                        .entry("agents")
                        .or_insert_with(|| serde_json::json!({}))
                        .as_object_mut()
                        .context("agents not object")?
                        .entry("defaults")
                        .or_insert_with(|| serde_json::json!({}))
                        .as_object_mut()
                        .context("defaults not object")?;

                    // Collect old→new key mappings first
                    let prefix = format!("{}/", provider_name);
                    let mut renames: Vec<(String, String)> = Vec::new();

                    if let Some(models_obj) =
                        defaults.get_mut("models").and_then(|m| m.as_object_mut())
                    {
                        let old_keys: Vec<String> = models_obj
                            .keys()
                            .filter(|k| k.starts_with(&prefix))
                            .cloned()
                            .collect();

                        for old_key in old_keys {
                            if let Some(val) = models_obj.remove(&old_key) {
                                let new_full = format!("{}/{}", provider_name, new_val);
                                models_obj.insert(new_full.clone(), val);
                                renames.push((old_key, new_full));
                            }
                        }
                    }

                    // Sync model.primary if it pointed to an old key
                    if let Some(model_obj) =
                        defaults.get_mut("model").and_then(|m| m.as_object_mut())
                    {
                        for (old_key, new_full) in &renames {
                            if model_obj.get("primary").and_then(|v| v.as_str()) == Some(old_key) {
                                model_obj.insert(
                                    "primary".to_string(),
                                    serde_json::Value::String(new_full.clone()),
                                );
                            }
                        }
                    }
                } else {
                    let json_field = match sub_field {
                        "Base URL" => "baseUrl",
                        "API Key" => "apiKey",
                        _ => return Ok(()),
                    };

                    let providers = parsed
                        .as_object_mut()
                        .context("root not object")?
                        .entry("models")
                        .or_insert_with(|| serde_json::json!({}))
                        .as_object_mut()
                        .context("models not object")?
                        .entry("providers")
                        .or_insert_with(|| serde_json::json!({}))
                        .as_object_mut()
                        .context("providers not object")?;

                    let prov = providers
                        .entry(provider_name)
                        .or_insert_with(|| serde_json::json!({}));

                    if let Some(obj) = prov.as_object_mut() {
                        if new_val == "—" || new_val.is_empty() {
                            obj.remove(json_field);
                        } else {
                            obj.insert(
                                json_field.to_string(),
                                serde_json::Value::String(new_val.to_string()),
                            );
                        }
                    }
                }
            } else if field_key == "Primary Model" {
                let model = parsed
                    .as_object_mut()
                    .context("root not object")?
                    .entry("agents")
                    .or_insert_with(|| serde_json::json!({}))
                    .as_object_mut()
                    .context("agents not object")?
                    .entry("defaults")
                    .or_insert_with(|| serde_json::json!({}))
                    .as_object_mut()
                    .context("defaults not object")?
                    .entry("model")
                    .or_insert_with(|| serde_json::json!({}));

                if let Some(obj) = model.as_object_mut() {
                    if new_val == "—" || new_val.is_empty() {
                        obj.remove("primary");
                    } else {
                        obj.insert(
                            "primary".to_string(),
                            serde_json::Value::String(new_val.to_string()),
                        );
                    }
                }
            } else {
                return Ok(());
            }
        }
    }

    let output = serde_json::to_string_pretty(&parsed).context("serialize config")?;
    std::fs::write(&path, output.as_bytes())
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn sync_kaku_theme() -> anyhow::Result<String> {
    let opencode_dir = config::HOME_DIR.join(".config").join("opencode");
    let themes_dir = opencode_dir.join("themes");
    config::create_user_owned_dirs(&opencode_dir).context("create opencode config dir")?;
    config::create_user_owned_dirs(&themes_dir).context("create opencode themes dir")?;

    let theme_content = super::OPENCODE_THEME_JSON;
    let theme_file = themes_dir.join("wezterm-match.json");
    std::fs::write(&theme_file, theme_content).context("write opencode theme")?;

    let config_path = opencode_dir.join("opencode.json");
    if !config_path.exists() {
        let config_content = "{\n  \"theme\": \"wezterm-match\"\n}\n";
        std::fs::write(&config_path, config_content).context("write opencode config")?;
    } else {
        let raw = std::fs::read_to_string(&config_path).context("read opencode config")?;
        let mut parsed: serde_json::Value = serde_json::from_str(&raw)
            .or_else(|_| serde_json::from_str(&strip_jsonc_comments(&raw)))
            .context("parse opencode config")?;
        if let Some(obj) = parsed.as_object_mut() {
            obj.insert(
                "theme".into(),
                serde_json::Value::String("wezterm-match".into()),
            );
        }
        let output = serde_json::to_string_pretty(&parsed).context("serialize")?;
        std::fs::write(&config_path, output).context("write opencode config")?;
    }

    Ok("Kaku theme synced to OpenCode".into())
}

pub fn run() -> anyhow::Result<()> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    crossterm::execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )
    .context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let mut app = App::new();
    let result = run_loop(&mut terminal, &mut app);

    disable_raw_mode().context("disable raw mode")?;
    crossterm::execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )
    .context("leave alternate screen")?;
    terminal.show_cursor().context("show cursor")?;

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    loop {
        terminal.draw(|frame| ui(frame, app))?;

        match event::read().context("read event")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if app.selecting {
                    match key.code {
                        KeyCode::Enter => app.confirm_select(),
                        KeyCode::Esc => app.cancel_select(),
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.select_index > 0 {
                                app.select_index -= 1;
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.select_index + 1 < app.select_options.len() {
                                app.select_index += 1;
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                if app.editing {
                    match key.code {
                        KeyCode::Enter => app.confirm_edit(),
                        KeyCode::Esc => app.cancel_edit(),
                        KeyCode::Backspace => {
                            app.edit_buf.pop();
                        }
                        KeyCode::Char(c) => app.edit_buf.push(c),
                        _ => {}
                    }
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.should_quit = true
                    }
                    KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                    KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                    KeyCode::Enter => app.start_edit(),
                    KeyCode::Char('t') => app.sync_theme(),
                    KeyCode::Char('o') => app.open_config(),
                    _ => {}
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => app.move_up(),
                MouseEventKind::ScrollDown => app.move_down(),
                _ => {}
            },
            _ => {}
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn ui(frame: &mut ratatui::Frame, app: &mut App) {
    let area = frame.area();

    // Fill background
    frame.render_widget(Block::default().style(Style::default().bg(BG())), area);

    let chunks = Layout::vertical([
        Constraint::Length(2), // logo header
        Constraint::Min(4),   // tool list
        Constraint::Length(1), // status bar
    ])
    .split(area);

    render_header(frame, chunks[0]);
    render_tools(frame, chunks[1], app);
    render_status_bar(frame, chunks[2], app);

    if app.selecting {
        render_selector(frame, area, app);
    } else if app.editing {
        render_editor(frame, area, app);
    }
}

fn render_header(frame: &mut ratatui::Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(
            "  Kaku",
            Style::default().fg(PURPLE()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(MUTED())),
        Span::styled("AI Config", Style::default().fg(TEXT())),
    ]);
    frame.render_widget(Paragraph::new(vec![line, Line::from("")]), area);
}

fn render_tools(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let mut items: Vec<ListItem> = Vec::new();
    let mut selected_flat: Option<usize> = None;
    let mut flat = 0usize;

    for (ti, tool) in app.tools.iter().enumerate() {
        let is_current_tool = ti == app.tool_index;
        let path_str = tool.tool.config_path().display().to_string();
        let home = config::HOME_DIR.display().to_string();
        let short_path = path_str.replace(&home, "~");

        let tool_style = if tool.installed {
            Style::default().fg(GREEN()).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED())
        };

        let header = Line::from(vec![
            Span::styled(
                if is_current_tool { "▸ " } else { "  " },
                Style::default().fg(PURPLE()),
            ),
            Span::styled(tool.tool.label(), tool_style),
            Span::styled("  ", Style::default()),
            Span::styled(short_path, Style::default().fg(MUTED())),
            if !tool.installed {
                Span::styled("  (not installed)", Style::default().fg(MUTED()))
            } else {
                Span::raw("")
            },
        ]);
        items.push(ListItem::new(header));
        if is_current_tool && app.field_index == 0 && tool.fields.is_empty() {
            selected_flat = Some(flat);
        }
        flat += 1;

        for (fi, field) in tool.fields.iter().enumerate() {
            let is_selected = is_current_tool && fi == app.field_index;
            if is_selected {
                selected_flat = Some(flat);
            }

            let marker = if is_selected { "▸" } else { "├" };
            let last = fi == tool.fields.len() - 1;
            let connector = if last && !is_selected { "└" } else { marker };

            let val_color = if field.value.starts_with('✓') {
                GREEN()
            } else if field.value.starts_with('✗') {
                RED()
            } else if field.value == "—" {
                MUTED()
            } else {
                YELLOW()
            };

            // Detect sub-fields (keys with " ▸ ") for hierarchical display
            let (display_key, extra_indent) = if let Some(pos) = field.key.find(" ▸ ") {
                (format!("▸ {}", &field.key[pos + " ▸ ".len()..]), true)
            } else {
                (field.key.clone(), false)
            };

            let indent_str = if extra_indent { "    │  " } else { "    " };
            let key_width = if extra_indent { 21 } else { 24 };

            // Prefix: ✓ already present for auth, · for read-only, none for editable
            let val_prefix = if field.value.starts_with('✓') || field.value.starts_with('✗') {
                ""
            } else if !field.editable {
                "· "
            } else {
                ""
            };

            let line = Line::from(vec![
                Span::styled(indent_str, Style::default().fg(MUTED())),
                Span::styled(
                    connector,
                    Style::default().fg(if is_selected { PURPLE() } else { MUTED() }),
                ),
                Span::styled(
                    "─ ",
                    Style::default().fg(if is_selected { PURPLE() } else { MUTED() }),
                ),
                Span::styled(
                    format!("{:<width$}", display_key, width = key_width),
                    Style::default().fg(TEXT()),
                ),
                Span::styled(val_prefix, Style::default().fg(val_color)),
                Span::styled(&field.value, Style::default().fg(val_color)),
            ]);

            items.push(ListItem::new(line));
            flat += 1;
        }

        items.push(ListItem::new(Line::raw("")));
        flat += 1;
    }

    let mut state = ListState::default();
    state.select(selected_flat);

    let list = List::new(items).highlight_style(Style::default().bg(PANEL()));

    frame.render_stateful_widget(list, area, &mut state);
}

fn render_status_bar(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let status = if let Some(msg) = &app.status_msg {
        Line::from(vec![
            Span::styled(" ℹ ", Style::default().fg(GREEN())),
            Span::styled(msg.as_str(), Style::default().fg(TEXT())),
        ])
    } else {
        Line::from(vec![
            Span::styled(" ↑↓", Style::default().fg(PURPLE())),
            Span::styled(" Navigate  ", Style::default().fg(MUTED())),
            Span::styled("Enter", Style::default().fg(PURPLE())),
            Span::styled(" Edit  ", Style::default().fg(MUTED())),
            Span::styled("t", Style::default().fg(PURPLE())),
            Span::styled(" Sync Theme  ", Style::default().fg(MUTED())),
            Span::styled("o", Style::default().fg(PURPLE())),
            Span::styled(" Open File  ", Style::default().fg(MUTED())),
            Span::styled("q", Style::default().fg(PURPLE())),
            Span::styled(" Quit", Style::default().fg(MUTED())),
        ])
    };

    frame.render_widget(Paragraph::new(status), area);
}

fn render_editor(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let tool = &app.tools[app.tool_index];
    if app.field_index >= tool.fields.len() {
        return;
    }
    let field = &tool.fields[app.field_index];

    let popup_width = 50u16.min(area.width.saturating_sub(4));
    let popup_height = 5u16;
    let popup = Rect::new(
        (area.width.saturating_sub(popup_width)) / 2,
        (area.height.saturating_sub(popup_height)) / 2,
        popup_width,
        popup_height,
    );

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" Edit: ", Style::default().fg(PURPLE())),
            Span::styled(&field.key, Style::default().fg(TEXT())),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PURPLE()))
        .style(Style::default().bg(BG()));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let input = Paragraph::new(Line::from(vec![
        Span::styled(&app.edit_buf, Style::default().fg(TEXT())),
        Span::styled("▏", Style::default().fg(PURPLE())),
    ]));
    frame.render_widget(input, inner.inner(Margin::new(1, 1)));
}

fn render_selector(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let tool = &app.tools[app.tool_index];
    if app.field_index >= tool.fields.len() {
        return;
    }
    let field = &tool.fields[app.field_index];

    let option_count = app.select_options.len() as u16;
    let popup_width = 60u16.min(area.width.saturating_sub(4));
    let popup_height = (option_count + 2).min(area.height.saturating_sub(4));
    let popup = Rect::new(
        (area.width.saturating_sub(popup_width)) / 2,
        (area.height.saturating_sub(popup_height)) / 2,
        popup_width,
        popup_height,
    );

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(Line::from(vec![
            Span::styled(" Select: ", Style::default().fg(PURPLE())),
            Span::styled(&field.key, Style::default().fg(TEXT())),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PURPLE()))
        .style(Style::default().bg(BG()));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let items: Vec<ListItem> = app
        .select_options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let is_sel = i == app.select_index;
            let marker = if is_sel { "▸ " } else { "  " };
            let style = if is_sel {
                Style::default().fg(PURPLE()).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT())
            };
            ListItem::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(PURPLE())),
                Span::styled(opt.as_str(), style),
            ]))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.select_index));

    let list = List::new(items).highlight_style(Style::default().bg(PANEL()));
    frame.render_stateful_widget(list, inner, &mut state);
}
