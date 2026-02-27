use crate::assistant_config;
use crate::utils::{is_jsonc_path, parse_json_or_jsonc, write_atomic};
use anyhow::Context;
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseEventKind,
};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

mod ui;

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    KakuAssistant,
    ClaudeCode,
    Codex,
    Gemini,
    Copilot,
    FactoryDroid,
    OpenCode,
    OpenClaw,
}

impl Tool {
    fn label(&self) -> &'static str {
        match self {
            Tool::KakuAssistant => "Kaku Assistant",
            Tool::ClaudeCode => "Claude Code",
            Tool::Codex => "Codex",
            Tool::Gemini => "Gemini CLI",
            Tool::Copilot => "Copilot CLI",
            Tool::FactoryDroid => "Factory Droid",
            Tool::OpenCode => "OpenCode",
            Tool::OpenClaw => "OpenClaw",
        }
    }

    fn config_path(&self) -> PathBuf {
        let home = config::HOME_DIR.clone();
        match self {
            Tool::KakuAssistant => assistant_config::assistant_toml_path().unwrap_or_else(|_| {
                config::HOME_DIR
                    .join(".config")
                    .join("kaku")
                    .join("assistant.toml")
            }),
            Tool::ClaudeCode => home.join(".claude").join("settings.json"),
            Tool::Codex => home.join(".codex").join("config.toml"),
            Tool::Gemini => home.join(".gemini").join("settings.json"),
            Tool::Copilot => home.join(".copilot").join("config.json"),
            Tool::FactoryDroid => home.join(".factory").join("settings.json"),
            Tool::OpenCode => {
                let jsonc_path = home.join(".config").join("opencode").join("opencode.jsonc");
                if jsonc_path.exists() {
                    return jsonc_path;
                }
                home.join(".config").join("opencode").join("opencode.json")
            }
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

const ALL_TOOLS: [Tool; 8] = [
    Tool::KakuAssistant,
    Tool::ClaudeCode,
    Tool::Codex,
    Tool::Gemini,
    Tool::Copilot,
    Tool::FactoryDroid,
    Tool::OpenCode,
    Tool::OpenClaw,
];

const FACTORY_DROID_DEFAULT_MODEL: &str = "opus";
const FACTORY_DROID_DEFAULT_REASONING: &str = "off";
const FACTORY_DROID_DEFAULT_AUTONOMY: &str = "normal";
const FACTORY_DROID_REASONING_OPTIONS: [&str; 5] = ["off", "none", "low", "medium", "high"];
const FACTORY_DROID_AUTONOMY_OPTIONS: [&str; 5] =
    ["normal", "spec", "auto-low", "auto-medium", "auto-high"];
const UI_ERROR_TTL: Duration = Duration::from_secs(5);

static UI_ERRORS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn ui_errors() -> &'static Mutex<Vec<String>> {
    UI_ERRORS.get_or_init(|| Mutex::new(Vec::new()))
}

fn push_ui_error(message: impl Into<String>) {
    let message = message.into();
    let mut guard = match ui_errors().lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    if guard.last().is_some_and(|last| last == &message) {
        return;
    }
    if guard.len() >= 8 {
        guard.remove(0);
    }
    guard.push(message);
}

fn pop_ui_error() -> Option<String> {
    let mut guard = ui_errors().lock().ok()?;
    if guard.is_empty() {
        None
    } else {
        Some(guard.remove(0))
    }
}

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
        let path = if tool == Tool::KakuAssistant {
            match assistant_config::ensure_assistant_toml_exists() {
                Ok(path) => path,
                Err(err) => {
                    return ToolState {
                        tool,
                        installed: true,
                        fields: vec![FieldEntry {
                            key: "error".into(),
                            value: err.to_string(),
                            options: vec![],
                            editable: false,
                        }],
                    };
                }
            }
        } else {
            tool.config_path()
        };

        if tool != Tool::KakuAssistant && !path.exists() {
            return ToolState {
                tool,
                installed: false,
                fields: Vec::new(),
            };
        }

        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("failed to read config for {}: {}", tool.label(), e);
                return ToolState {
                    tool,
                    installed: false,
                    fields: vec![FieldEntry {
                        key: "error".into(),
                        value: format!("failed to read config: {}", e),
                        options: vec![],
                        ..Default::default()
                    }],
                };
            }
        };

        let fields = match tool {
            Tool::KakuAssistant => extract_kaku_assistant_fields(&raw),
            Tool::ClaudeCode => {
                let parsed = parse_json_or_jsonc_with_debug(&raw, tool.label());
                extract_claude_code_fields(&parsed)
            }
            Tool::Codex => extract_codex_fields(&raw),
            Tool::Gemini => {
                let parsed = parse_json_with_debug(&raw, tool.label());
                extract_gemini_fields(&parsed)
            }
            Tool::Copilot => {
                let parsed = parse_json_with_debug(&raw, tool.label());
                extract_copilot_fields(&parsed)
            }
            Tool::FactoryDroid => {
                let parsed = parse_json_with_debug(&raw, tool.label());
                extract_factory_droid_fields(&parsed)
            }
            Tool::OpenCode => {
                let parsed = parse_json_or_jsonc_with_debug(&raw, tool.label());
                extract_opencode_fields(&parsed)
            }
            Tool::OpenClaw => {
                let parsed = parse_json_or_jsonc_with_debug(&raw, tool.label());
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

fn parse_json_with_debug(raw: &str, tool_label: &str) -> serde_json::Value {
    serde_json::from_str(raw).unwrap_or_else(|e| {
        log::warn!("failed to parse {} config json: {}", tool_label, e);
        push_ui_error(format!("{tool_label} config is invalid JSON"));
        serde_json::Value::Null
    })
}

fn parse_json_or_jsonc_with_debug(raw: &str, tool_label: &str) -> serde_json::Value {
    parse_json_or_jsonc(raw).unwrap_or_else(|e| {
        log::warn!("failed to parse {} config json/jsonc: {}", tool_label, e);
        push_ui_error(format!("{tool_label} config is malformed"));
        serde_json::Value::Null
    })
}

fn read_text_with_debug(path: &Path, context: &str) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => Some(raw),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            log::debug!("{context}: {} not found", path.display());
            None
        }
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::InvalidData
            ) =>
        {
            log::warn!("{context}: failed to read {}: {}", path.display(), e);
            push_ui_error(format!(
                "{context}: cannot read {}. Check file permission or encoding.",
                path.display()
            ));
            None
        }
        Err(e) => {
            log::debug!("{context}: failed to read {}: {}", path.display(), e);
            None
        }
    }
}

fn parse_json_value_with_debug(raw: &str, context: &str) -> Option<serde_json::Value> {
    serde_json::from_str(raw)
        .map_err(|e| {
            log::warn!("{context}: failed to parse json: {}", e);
            push_ui_error(format!("{context}: invalid JSON format"));
        })
        .ok()
}

fn read_json_file_with_debug(path: &Path, context: &str) -> Option<serde_json::Value> {
    let raw = read_text_with_debug(path, context)?;
    parse_json_value_with_debug(&raw, context)
}

fn decode_jwt_payload_with_debug(token: &str, context: &str) -> Option<serde_json::Value> {
    // JWT format: header.payload.signature
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        log::debug!("{context}: invalid jwt format");
        push_ui_error(format!("{context}: invalid token format"));
        return None;
    }

    let mut payload = parts[1].to_string();
    while payload.len() % 4 != 0 {
        payload.push('=');
    }

    use base64::Engine;
    let decoded = base64::engine::general_purpose::URL_SAFE
        .decode(&payload)
        .map_err(|e| {
            log::debug!("{context}: failed to decode jwt payload: {}", e);
            push_ui_error(format!("{context}: token payload decode failed"));
        })
        .ok()?;
    serde_json::from_slice(&decoded)
        .map_err(|e| {
            log::debug!("{context}: failed to parse jwt payload json: {}", e);
            push_ui_error(format!("{context}: token payload JSON is invalid"));
        })
        .ok()
}

fn json_str(val: &serde_json::Value, key: &str) -> String {
    val.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn json_nested_or_top_str(
    val: &serde_json::Value,
    parent_key: &str,
    nested_key: &str,
    legacy_top_key: &str,
) -> String {
    val.get(parent_key)
        .and_then(|v| v.as_object())
        .and_then(|obj| obj.get(nested_key))
        .and_then(|v| v.as_str())
        .or_else(|| val.get(legacy_top_key).and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string()
}

fn string_options(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_string()).collect()
}

fn mask_key(val: &str) -> String {
    if val.is_empty() {
        return "—".into();
    }
    if val.len() <= 12 {
        return "****".into();
    }
    // Show first 12 chars and last 4 chars
    format!("{}...{}", &val[..12], &val[val.len() - 4..])
}

/// Configuration for the Kaku built-in AI assistant.
///
/// This struct holds the configuration for Kaku's AI-powered command analysis
/// feature. It ensures that model and base_url always have valid values
/// by falling back to defaults when empty strings are provided.
#[derive(Debug, Clone)]
struct KakuAssistantConfig {
    /// Whether the AI assistant is enabled
    enabled: bool,
    /// API key for the AI service (may be empty if not configured)
    api_key: String,
    /// Model identifier (never empty, falls back to default)
    model: String,
    /// Base URL for the API endpoint (never empty, falls back to default)
    base_url: String,
}

impl KakuAssistantConfig {
    /// Creates a new configuration with the given values.
    ///
    /// # Arguments
    /// * `enabled` - Whether the assistant is enabled
    /// * `api_key` - API key (empty string if not set)
    /// * `model` - Model identifier (empty strings will be replaced with default)
    /// * `base_url` - Base URL (empty strings will be replaced with default)
    fn new(
        enabled: bool,
        api_key: impl Into<String>,
        model: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        let model = model.into();
        let base_url = base_url.into();
        Self {
            enabled,
            api_key: api_key.into(),
            model: if model.trim().is_empty() {
                assistant_config::DEFAULT_MODEL.to_string()
            } else {
                model
            },
            base_url: if base_url.trim().is_empty() {
                assistant_config::DEFAULT_BASE_URL.to_string()
            } else {
                base_url
            },
        }
    }

    /// Returns whether the assistant is enabled.
    fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns the API key (may be empty).
    fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Returns the model identifier (never empty).
    fn model(&self) -> &str {
        &self.model
    }

    /// Returns the base URL (never empty).
    fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl Default for KakuAssistantConfig {
    fn default() -> Self {
        Self::new(true, String::new(), String::new(), String::new())
    }
}

/// Parses a KakuAssistantConfig from TOML content.
///
/// This function gracefully handles malformed TOML by using default values
/// for any missing or invalid fields.
fn parse_kaku_assistant_config(raw: &str) -> KakuAssistantConfig {
    let parsed = raw.parse::<toml::Value>().unwrap_or_else(|e| {
        log::warn!("failed to parse assistant.toml: {}", e);
        push_ui_error("Kaku Assistant config TOML is malformed");
        toml::Value::Table(Default::default())
    });

    let enabled = parsed
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let api_key = parsed.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
    let model = parsed.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let base_url = parsed
        .get("base_url")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    KakuAssistantConfig::new(enabled, api_key, model, base_url)
}

fn get_kaku_assistant_api_key() -> Option<String> {
    let path = assistant_config::ensure_assistant_toml_exists()
        .map_err(|e| log::debug!("assistant config not available: {}", e))
        .ok()?;
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| log::debug!("failed to read assistant config: {}", e))
        .ok()?;
    let cfg = parse_kaku_assistant_config(&raw);
    if cfg.api_key().trim().is_empty() {
        log::debug!("assistant config has no api_key set");
        None
    } else {
        Some(cfg.api_key().to_string())
    }
}

fn kaku_assistant_enabled_display(cfg: &KakuAssistantConfig) -> &'static str {
    if !cfg.is_enabled() {
        return "Off";
    }
    if cfg.api_key().trim().is_empty() {
        return "Not configured";
    }
    "On"
}

fn extract_kaku_assistant_fields(raw: &str) -> Vec<FieldEntry> {
    let cfg = parse_kaku_assistant_config(raw);
    vec![
        FieldEntry {
            key: "Enabled".into(),
            value: kaku_assistant_enabled_display(&cfg).into(),
            options: vec!["On".into(), "Off".into()],
            editable: true,
        },
        FieldEntry {
            key: "Model".into(),
            value: cfg.model().to_string(),
            options: vec![],
            editable: true,
        },
        FieldEntry {
            key: "Base URL".into(),
            value: cfg.base_url().to_string(),
            options: vec![],
            editable: true,
        },
        FieldEntry {
            key: "API Key".into(),
            value: mask_key(cfg.api_key()),
            options: vec![],
            editable: true,
        },
    ]
}

fn render_toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

fn write_kaku_assistant_config(path: &Path, cfg: &KakuAssistantConfig) -> anyhow::Result<()> {
    let mut out = String::new();
    out.push_str("# Kaku Assistant configuration\n");
    out.push_str(
        "# enabled: true enables command analysis suggestions; false disables requests.\n",
    );
    out.push_str("# api_key: provider API key, example: \"sk-xxxx\".\n");
    out.push_str("# model: model id, example: \"DeepSeek-V3.2\" or \"gpt-5-mini\".\n");
    out.push_str("# base_url: chat-completions API root URL.\n\n");
    out.push_str(if cfg.is_enabled() {
        "enabled = true\n"
    } else {
        "enabled = false\n"
    });
    if cfg.api_key().trim().is_empty() {
        out.push_str("# api_key = \"<your_api_key>\"\n");
    } else {
        out.push_str(&format!(
            "api_key = {}\n",
            render_toml_string(cfg.api_key().trim())
        ));
    }
    out.push_str(&format!(
        "model = {}\n",
        render_toml_string(cfg.model().trim())
    ));
    out.push_str(&format!(
        "base_url = {}\n",
        render_toml_string(cfg.base_url().trim())
    ));
    write_atomic(path, out.as_bytes()).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn save_kaku_assistant_field(field_key: &str, new_val: &str) -> anyhow::Result<()> {
    let path = assistant_config::ensure_assistant_toml_exists()?;
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            log::debug!(
                "assistant config missing when saving; recreating {}",
                path.display()
            );
            String::new()
        }
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::InvalidData
            ) =>
        {
            log::warn!("failed to read assistant config {}: {}", path.display(), e);
            push_ui_error(format!(
                "cannot read {}. Check file permission or encoding.",
                path.display()
            ));
            String::new()
        }
        Err(e) => {
            log::debug!("failed to read assistant config {}: {}", path.display(), e);
            String::new()
        }
    };
    let cfg = parse_kaku_assistant_config(&raw);

    // Build updated config based on which field changed
    let updated = match field_key {
        "Enabled" => {
            let enabled = matches!(new_val.trim(), "On" | "on" | "true" | "1");
            KakuAssistantConfig::new(enabled, cfg.api_key(), cfg.model(), cfg.base_url())
        }
        "Model" => {
            let model = if new_val.trim().is_empty() || new_val == "—" {
                assistant_config::DEFAULT_MODEL
            } else {
                new_val.trim()
            };
            KakuAssistantConfig::new(cfg.is_enabled(), cfg.api_key(), model, cfg.base_url())
        }
        "Base URL" => {
            let base_url = if new_val.trim().is_empty() || new_val == "—" {
                assistant_config::DEFAULT_BASE_URL
            } else {
                new_val.trim()
            };
            KakuAssistantConfig::new(cfg.is_enabled(), cfg.api_key(), cfg.model(), base_url)
        }
        "API Key" => KakuAssistantConfig::new(
            cfg.is_enabled(),
            new_val.trim(),
            cfg.model(),
            cfg.base_url(),
        ),
        _ => return Ok(()),
    };

    write_kaku_assistant_config(&path, &updated)
}

/// Get OpenAI account email from JWT token in auth.json
fn get_opencode_openai_account(entry: &serde_json::Value) -> Option<String> {
    let token = entry.get("access")?.as_str()?;
    let jwt_data = decode_jwt_payload_with_debug(token, "opencode openai account")?;

    // OpenAI JWT payload contains email in custom claim
    jwt_data
        .get("https://api.openai.com/profile")?
        .get("email")?
        .as_str()
        .map(|s| s.to_string())
}

/// Get Google account email by matching refresh token with antigravity-accounts.json
fn get_opencode_google_account(entry: &serde_json::Value) -> Option<String> {
    let refresh_token = entry.get("refresh")?.as_str()?;

    // Extract project ID from refresh token (format: "token|project-id")
    let project_id = if let Some(pos) = refresh_token.rfind('|') {
        &refresh_token[pos + 1..]
    } else {
        return None;
    };

    let accounts_path = config::HOME_DIR
        .join(".config")
        .join("opencode")
        .join("antigravity-accounts.json");

    let parsed = read_json_file_with_debug(&accounts_path, "opencode google account")?;

    // Find account with matching project ID
    let accounts = parsed.get("accounts")?.as_array()?;
    for account in accounts {
        if account.get("projectId")?.as_str() == Some(project_id) {
            return account.get("email")?.as_str().map(|s| s.to_string());
        }
    }

    None
}

/// Get GitHub Copilot username from gh auth status
fn get_opencode_github_copilot_account() -> Option<String> {
    get_copilot_account()
}

/// Get Gemini account email from google_accounts.json
fn get_gemini_account() -> Option<String> {
    let path = config::HOME_DIR
        .join(".gemini")
        .join("google_accounts.json");

    let parsed = read_json_file_with_debug(&path, "gemini account")?;

    // Extract "active" field
    parsed.get("active")?.as_str().map(|s| s.to_string())
}

/// Get Codex account email from JWT token in auth.json
fn get_codex_account() -> Option<String> {
    let auth_path = config::HOME_DIR.join(".codex").join("auth.json");
    let auth_json = read_json_file_with_debug(&auth_path, "codex account")?;

    // Extract access_token from tokens object
    let token = auth_json.get("tokens")?.get("access_token")?.as_str()?;

    let jwt_data = decode_jwt_payload_with_debug(token, "codex account")?;

    // OpenAI JWT payload contains email in custom claim
    jwt_data
        .get("https://api.openai.com/profile")?
        .get("email")?
        .as_str()
        .map(|s| s.to_string())
}

/// Get full API Key from auth.json for OpenCode provider
fn get_opencode_api_key(provider_name: &str) -> Option<String> {
    let auth_path = config::HOME_DIR
        .join(".local")
        .join("share")
        .join("opencode")
        .join("auth.json");

    let parsed = read_json_file_with_debug(&auth_path, "opencode api key")?;

    parsed
        .get(provider_name)?
        .get("key")?
        .as_str()
        .map(|s| s.to_string())
}

/// Get GitHub Copilot username from gh CLI
fn get_copilot_account() -> Option<String> {
    let output = match std::process::Command::new("gh")
        .args(["api", "user", "-q", ".login"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            log::debug!("gh account probe failed to launch: {}", e);
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::debug!(
            "gh account probe failed with status {}: {}",
            output.status,
            stderr.trim()
        );
        return None;
    }

    String::from_utf8(output.stdout)
        .map_err(|e| log::debug!("gh account probe returned non-utf8 stdout: {}", e))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Get Claude Code account email from claude auth status
fn get_claude_code_account() -> Option<String> {
    let output = match std::process::Command::new("claude")
        .args(["auth", "status"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            log::debug!("claude auth status probe failed to launch: {}", e);
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::debug!(
            "claude auth status probe failed with status {}: {}",
            output.status,
            stderr.trim()
        );
        return None;
    }

    let json_str = String::from_utf8(output.stdout)
        .map_err(|e| log::debug!("claude auth status probe returned non-utf8 stdout: {}", e))
        .ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| log::debug!("failed to parse claude auth status json: {}", e))
        .ok()?;

    // Extract email from auth status JSON
    parsed.get("email")?.as_str().map(|s| s.to_string())
}

/// Format auth status, with account fallback to auth method
fn format_auth_status(account: Option<String>, fallback_method: &str) -> String {
    match account {
        Some(acc) if !acc.is_empty() => format!("✓ {}", acc),
        _ => format!("✓ {}", fallback_method),
    }
}

/// Fetch models.dev data, cached to ~/.cache/kaku/models.json.
/// No TTL — use `r` key in TUI to force refresh.
fn load_models_dev_json() -> Option<serde_json::Value> {
    let cache_dir = config::HOME_DIR.join(".cache").join("kaku");
    let cache_path = cache_dir.join("models.json");

    // Use cache if exists
    if let Some(raw) = read_text_with_debug(&cache_path, "models.dev cache") {
        if let Some(v) = parse_json_value_with_debug(&raw, "models.dev cache") {
            return Some(v);
        }
    }

    // Fetch from API via curl (macOS built-in)
    fetch_models_dev_json()
}

/// Force fetch from models.dev and update cache.
fn fetch_models_dev_json() -> Option<serde_json::Value> {
    let cache_dir = config::HOME_DIR.join(".cache").join("kaku");
    let cache_path = cache_dir.join("models.json");

    let output = match std::process::Command::new("curl")
        .args(["-sS", "--max-time", "10", "https://models.dev/api.json"])
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            log::debug!("models.dev fetch failed to launch curl: {}", e);
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::debug!(
            "models.dev fetch failed with status {}: {}",
            output.status,
            stderr.trim()
        );
        return None;
    }

    let raw = String::from_utf8(output.stdout)
        .map_err(|e| log::debug!("models.dev fetch returned non-utf8 stdout: {}", e))
        .ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| log::debug!("failed to parse models.dev json: {}", e))
        .ok()?;
    let _ = config::create_user_owned_dirs(&cache_dir);
    let _ = std::fs::write(&cache_path, &raw);
    Some(v)
}

/// Read model IDs from models.dev for a given provider.
/// Returns latest models sorted by release_date (newest first), deduped to
/// only keep the short alias (e.g. "claude-sonnet-4-5") and skip dated
/// variants (e.g. "claude-sonnet-4-5-20250929").
fn read_models_dev(provider_id: &str) -> Vec<String> {
    let parsed = match load_models_dev_json() {
        Some(v) => v,
        None => return Vec::new(),
    };
    let models = match parsed
        .get(provider_id)
        .and_then(|p| p.get("models"))
        .and_then(|m| m.as_object())
    {
        Some(m) => m,
        None => return Vec::new(),
    };

    // Collect (id, release_date) pairs, skip embedding/tts/image-only models
    let mut items: Vec<(&str, &str)> = models
        .iter()
        .filter_map(|(id, m)| {
            // Skip non-chat models
            let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.contains("Embedding")
                || name.contains("TTS")
                || name.contains("Image")
                || name.contains("Live")
            {
                return None;
            }
            // Skip dated variants (e.g. "claude-opus-4-5-20251101", "gemini-2.5-flash-preview-06-17")
            if id.chars().rev().take(8).all(|c| c.is_ascii_digit()) {
                return None;
            }
            // Skip dated preview variants with "xx-xx" suffix (e.g. "09-2025", "06-17")
            // Require both segments ≥ 2 chars to avoid filtering version numbers like "4-5"
            if let Some(last_dash) = id.rfind('-') {
                let suffix = &id[last_dash + 1..];
                if let Some(second_dash) = id[..last_dash].rfind('-') {
                    let prev = &id[second_dash + 1..last_dash];
                    if prev.len() >= 2
                        && prev.len() <= 4
                        && suffix.len() >= 2
                        && suffix.len() <= 4
                        && prev.chars().all(|c| c.is_ascii_digit())
                        && suffix.chars().all(|c| c.is_ascii_digit())
                    {
                        return None;
                    }
                }
            }
            // Skip "-latest" aliases (e.g. "gemini-flash-latest")
            if id.ends_with("-latest") {
                return None;
            }
            let rd = m.get("release_date").and_then(|v| v.as_str()).unwrap_or("");
            Some((id.as_str(), rd))
        })
        .collect();

    items.sort_by(|a, b| b.1.cmp(a.1));
    items
        .into_iter()
        .take(4)
        .map(|(id, _)| id.to_string())
        .collect()
}

fn extract_claude_code_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let model = json_str(val, "model");

    let model_options = read_models_dev("anthropic");

    let display_value = if model.is_empty() {
        // Show the latest model name as default hint
        model_options
            .first()
            .map(|m| format!("{} (default)", m))
            .unwrap_or_else(|| "default".into())
    } else {
        model
    };

    let mut fields = vec![FieldEntry {
        key: "Model".into(),
        value: display_value,
        options: model_options,
        ..Default::default()
    }];

    // Auth status: Claude Code uses OAuth; statsig dir indicates active session
    let statsig_dir = config::HOME_DIR.join(".claude").join("statsig");
    if statsig_dir.exists() {
        let account = get_claude_code_account();
        fields.push(FieldEntry {
            key: "Auth".into(),
            value: format_auth_status(account, "oauth"),
            options: vec![],
            editable: false,
        });
    }

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

    // Read available models from Codex model cache
    let model_options = read_codex_model_options();

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
                        options: model_options.clone(),
                        ..Default::default()
                    });
                }
                "model_reasoning_effort" => {
                    fields.push(FieldEntry {
                        key: "Reasoning Effort".into(),
                        value: val.to_string(),
                        options: read_codex_reasoning_options(),
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }
    }

    // Check auth status from auth.json
    let auth_path = config::HOME_DIR.join(".codex").join("auth.json");
    if let Some(auth) = read_json_file_with_debug(&auth_path, "codex auth status") {
        let auth_mode = auth.get("auth_mode").and_then(|v| v.as_str()).unwrap_or("");
        if !auth_mode.is_empty() {
            let account = get_codex_account();
            fields.push(FieldEntry {
                key: "Auth".into(),
                value: format_auth_status(account, auth_mode),
                options: vec![],
                editable: false,
            });
        }
    }

    fields
}

/// Read model slugs from Codex's own cache, or from models.dev.
fn read_codex_model_options() -> Vec<String> {
    let cache_path = config::HOME_DIR.join(".codex").join("models_cache.json");
    if let Some(parsed) = read_json_file_with_debug(&cache_path, "codex model cache") {
        let mut models: Vec<(String, usize)> = parsed
            .get("models")
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|m| {
                        m.get("visibility")
                            .and_then(|v| v.as_str())
                            .map(|v| v == "list")
                            .unwrap_or(false)
                    })
                    .filter_map(|m| {
                        let slug = m.get("slug").and_then(|v| v.as_str())?;
                        let priority =
                            m.get("priority").and_then(|v| v.as_u64()).unwrap_or(999) as usize;
                        Some((slug.to_string(), priority))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !models.is_empty() {
            models.sort_by_key(|(_, p)| *p);
            return models.into_iter().map(|(s, _)| s).collect();
        }
    }

    read_models_dev("openai")
}

/// Read reasoning effort options from Codex's models cache for the current model.
fn read_codex_reasoning_options() -> Vec<String> {
    let config_path = config::HOME_DIR.join(".codex").join("config.toml");
    let current_model = read_text_with_debug(&config_path, "codex config")
        .and_then(|raw| {
            raw.lines()
                .find(|l| l.trim_start().starts_with("model"))
                .and_then(|l| l.split_once('='))
                .map(|(_, v)| v.trim().trim_matches('"').to_string())
        })
        .unwrap_or_default();

    let cache_path = config::HOME_DIR.join(".codex").join("models_cache.json");
    if let Some(parsed) = read_json_file_with_debug(&cache_path, "codex model cache") {
        if let Some(models) = parsed.get("models").and_then(|m| m.as_array()) {
            // Find the current model or first visible model
            let model = models
                .iter()
                .find(|m| m.get("slug").and_then(|v| v.as_str()) == Some(&current_model))
                .or_else(|| {
                    models
                        .iter()
                        .find(|m| m.get("visibility").and_then(|v| v.as_str()) == Some("list"))
                });

            if let Some(m) = model {
                if let Some(levels) = m
                    .get("supported_reasoning_levels")
                    .and_then(|l| l.as_array())
                {
                    let opts: Vec<String> = levels
                        .iter()
                        .filter_map(|l| {
                            l.get("effort")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        })
                        .collect();
                    if !opts.is_empty() {
                        return opts;
                    }
                }
            }
        }
    }

    vec!["low".into(), "medium".into(), "high".into()]
}

fn extract_gemini_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let mut fields = Vec::new();

    let model = val
        .get("model")
        .and_then(|m| {
            m.get("name")
                .and_then(|n| n.as_str())
                .or_else(|| m.as_str())
        })
        .unwrap_or("")
        .to_string();
    let model_options = read_models_dev("google");

    let display_value = if model.is_empty() {
        model_options
            .first()
            .map(|m| format!("{} (default)", m))
            .unwrap_or_else(|| "default".into())
    } else {
        model
    };

    fields.push(FieldEntry {
        key: "Model".into(),
        value: display_value,
        options: model_options,
        ..Default::default()
    });

    let auth_type = val
        .get("security")
        .and_then(|s| s.get("auth"))
        .and_then(|a| a.get("selectedType"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !auth_type.is_empty() {
        let account = get_gemini_account();
        fields.push(FieldEntry {
            key: "Auth".into(),
            value: format_auth_status(account, auth_type),
            options: vec![],
            editable: false,
        });
    }

    fields
}

/// Read model choices from `copilot --help` output, fallback to models.dev.
fn read_copilot_model_options() -> Vec<String> {
    let output = match std::process::Command::new("copilot").arg("--help").output() {
        Ok(output) => output,
        Err(e) => {
            log::debug!("copilot --help probe failed to launch: {}", e);
            return read_models_dev("anthropic");
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::debug!(
            "copilot --help probe failed with status {}: {}",
            output.status,
            stderr.trim()
        );
        return read_models_dev("anthropic");
    }

    let text = String::from_utf8_lossy(&output.stdout);
    // Find "--model" first, then parse the choices after it
    if let Some(model_pos) = text.find("--model") {
        let after_model = &text[model_pos..];
        if let Some(choices_pos) = after_model.find("choices:") {
            let rest = &after_model[choices_pos + "choices:".len()..];
            if let Some(end) = rest.find(')') {
                let choices_str = &rest[..end];
                let models: Vec<String> = choices_str
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !models.is_empty() {
                    return models;
                }
            }
        }
    }

    log::debug!("copilot --help output did not expose model choices");
    read_models_dev("anthropic")
}

fn extract_copilot_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let model = json_str(val, "model");

    let model_options = read_copilot_model_options();

    let mut fields = vec![FieldEntry {
        key: "Model".into(),
        value: if model.is_empty() {
            "default".into()
        } else {
            model
        },
        options: model_options,
        ..Default::default()
    }];

    // Copilot authenticates via GitHub OAuth; session files indicate auth
    let session_dir = config::HOME_DIR.join(".copilot").join("session-state");
    if session_dir.exists() {
        let account = get_copilot_account();
        fields.push(FieldEntry {
            key: "Auth".into(),
            value: format_auth_status(account, "github"),
            options: vec![],
            editable: false,
        });
    }

    fields
}

fn extract_factory_droid_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let session_defaults = val
        .get("sessionDefaultSettings")
        .and_then(|v| v.as_object());
    let model = json_nested_or_top_str(val, "sessionDefaultSettings", "model", "model");
    let reasoning = json_nested_or_top_str(
        val,
        "sessionDefaultSettings",
        "reasoningEffort",
        "reasoningEffort",
    );
    let autonomy = session_defaults
        .and_then(|s| s.get("autonomyMode").or_else(|| s.get("autonomyLevel")))
        .and_then(|v| v.as_str())
        .or_else(|| {
            val.get("autonomyMode")
                .or_else(|| val.get("autonomyLevel"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("")
        .to_string();

    vec![
        FieldEntry {
            key: "Model".into(),
            value: if model.is_empty() {
                FACTORY_DROID_DEFAULT_MODEL.into()
            } else {
                model
            },
            options: vec![],
            editable: true,
        },
        FieldEntry {
            key: "Reasoning Effort".into(),
            value: if reasoning.is_empty() {
                FACTORY_DROID_DEFAULT_REASONING.into()
            } else {
                reasoning
            },
            options: string_options(&FACTORY_DROID_REASONING_OPTIONS),
            editable: true,
        },
        FieldEntry {
            key: "Autonomy Level".into(),
            value: if autonomy.is_empty() {
                FACTORY_DROID_DEFAULT_AUTONOMY.into()
            } else {
                autonomy
            },
            options: string_options(&FACTORY_DROID_AUTONOMY_OPTIONS),
            editable: true,
        },
    ]
}

fn factory_droid_session_key(field_key: &str) -> Option<&'static str> {
    match field_key {
        "Model" => Some("model"),
        "Reasoning Effort" => Some("reasoningEffort"),
        // Normalize writes to `autonomyMode` while still accepting legacy `autonomyLevel`.
        "Autonomy Level" => Some("autonomyMode"),
        _ => None,
    }
}

fn save_factory_droid_field(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    field_key: &str,
    new_val: &str,
) -> anyhow::Result<()> {
    let Some(target_key) = factory_droid_session_key(field_key) else {
        return Ok(());
    };

    let session_defaults = obj
        .entry("sessionDefaultSettings")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .context("sessionDefaultSettings is not an object")?;

    if new_val == "—" || new_val.is_empty() {
        session_defaults.remove(target_key);
    } else {
        session_defaults.insert(
            target_key.to_string(),
            serde_json::Value::String(new_val.to_string()),
        );
    }

    Ok(())
}

fn extract_opencode_fields(val: &serde_json::Value) -> Vec<FieldEntry> {
    let primary_model = json_str(val, "model");

    // Collect model IDs from configured providers in opencode.json
    let mut model_options: Vec<String> = val
        .get("provider")
        .and_then(|p| p.as_object())
        .map(|providers| {
            let mut ids = Vec::new();
            for (name, prov) in providers {
                if let Some(models) = prov.get("models").and_then(|m| m.as_object()) {
                    for model_id in models.keys() {
                        ids.push(format!("{}/{}", name, model_id));
                    }
                }
            }
            ids
        })
        .unwrap_or_default();

    // Also discover models from authenticated providers in auth.json
    if model_options.is_empty() {
        let auth_path = config::HOME_DIR
            .join(".local")
            .join("share")
            .join("opencode")
            .join("auth.json");
        if let Some(auth) = read_json_file_with_debug(&auth_path, "opencode auth providers") {
            if let Some(obj) = auth.as_object() {
                for auth_name in obj.keys() {
                    // Map well-known aliases, otherwise use auth name directly
                    let models_dev_id = match auth_name.as_str() {
                        "github-copilot" => "anthropic",
                        other => other,
                    };
                    for model in read_models_dev(models_dev_id) {
                        let prefixed = format!("{}/{}", auth_name, model);
                        if !model_options.contains(&prefixed) {
                            model_options.push(prefixed);
                        }
                    }
                }
            }
        }
    }

    let has_options = !model_options.is_empty();
    let mut fields = vec![FieldEntry {
        key: "Model".into(),
        value: if primary_model.is_empty() {
            "—".into()
        } else {
            primary_model
        },
        options: model_options,
        editable: has_options,
        ..Default::default()
    }];

    // Read auth.json for provider authentication status
    let auth_path = config::HOME_DIR
        .join(".local")
        .join("share")
        .join("opencode")
        .join("auth.json");
    if let Some(auth) = read_json_file_with_debug(&auth_path, "opencode auth status") {
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
                    "oauth" => {
                        let account = match name.as_str() {
                            "openai" => get_opencode_openai_account(entry),
                            "google" => get_opencode_google_account(entry),
                            "github-copilot" => get_opencode_github_copilot_account(),
                            _ => None,
                        };
                        format_auth_status(account, "oauth")
                    }
                    _ => auth_type.clone(),
                };

                fields.push(FieldEntry {
                    key: name.clone(),
                    value: status,
                    options: vec![],
                    editable: auth_type == "api", // API keys are editable, OAuth is not
                });
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

    // Fallback: if no providers defined, collect models from agents.defaults.models
    if all_model_ids.is_empty() {
        if let Some(agent_models) = defaults.get("models").and_then(|m| m.as_object()) {
            for model_key in agent_models.keys() {
                all_model_ids.push(model_key.clone());
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

#[derive(Clone, PartialEq, Eq)]
enum AppMode {
    Browsing,
    Editing {
        field_idx: usize,
        buffer: String,
        cursor: usize,
    },
    Selecting {
        field_idx: usize,
        options: Vec<String>,
        selected: usize,
    },
}

struct App {
    tools: Vec<ToolState>,
    tool_index: usize,
    field_index: usize,
    focus: Focus,
    mode: AppMode,
    status_msg: Option<String>,
    last_error: Option<String>,
    error_expire: Option<Instant>,
    should_quit: bool,
}

fn status_value_for_display(field_key: &str, new_val: &str) -> String {
    if field_key.contains("API Key") {
        return if new_val.trim().is_empty() {
            "—".into()
        } else {
            mask_key(new_val.trim())
        };
    }
    new_val.to_string()
}

impl App {
    fn new() -> Self {
        let tools: Vec<ToolState> = ALL_TOOLS
            .iter()
            .map(|t| ToolState::load(*t))
            .filter(|t| t.tool == Tool::KakuAssistant || t.installed)
            .collect();
        let first = tools.iter().position(|t| !t.fields.is_empty()).unwrap_or(0);
        let mut app = App {
            tools,
            tool_index: first,
            field_index: 0,
            focus: Focus::ToolList,
            mode: AppMode::Browsing,
            status_msg: None,
            last_error: None,
            error_expire: None,
            should_quit: false,
        };
        app.sync_transient_errors();
        app
    }

    fn total_rows(&self) -> usize {
        self.tools.iter().map(|t| t.fields.len()).sum()
    }

    fn flatten_index(&self) -> usize {
        let mut idx = 0;
        for (ti, tool) in self.tools.iter().enumerate() {
            if ti == self.tool_index {
                return idx + self.field_index;
            }
            idx += tool.fields.len();
        }
        idx
    }

    fn is_editing(&self) -> bool {
        matches!(self.mode, AppMode::Editing { .. })
    }

    fn is_selecting(&self) -> bool {
        matches!(self.mode, AppMode::Selecting { .. })
    }

    fn editing_view(&self) -> Option<(usize, &str, usize)> {
        match &self.mode {
            AppMode::Editing {
                field_idx,
                buffer,
                cursor,
            } => Some((*field_idx, buffer.as_str(), *cursor)),
            _ => None,
        }
    }

    fn editing_mut(&mut self) -> Option<(&mut String, &mut usize)> {
        match &mut self.mode {
            AppMode::Editing { buffer, cursor, .. } => Some((buffer, cursor)),
            _ => None,
        }
    }

    fn selecting_view(&self) -> Option<(usize, &[String], usize)> {
        match &self.mode {
            AppMode::Selecting {
                field_idx,
                options,
                selected,
            } => Some((*field_idx, options.as_slice(), *selected)),
            _ => None,
        }
    }

    fn set_error(&mut self, message: impl Into<String>) {
        self.last_error = Some(message.into());
        self.error_expire = Some(Instant::now() + UI_ERROR_TTL);
    }

    fn sync_transient_errors(&mut self) {
        while let Some(error) = pop_ui_error() {
            self.set_error(error);
        }

        if self
            .error_expire
            .is_some_and(|expire_at| Instant::now() >= expire_at)
        {
            self.last_error = None;
            self.error_expire = None;
        }
    }

    fn set_from_flat(&mut self, flat: usize) {
        let mut remaining = flat;
        for (ti, tool) in self.tools.iter().enumerate() {
            let count = tool.fields.len();
            if count == 0 {
                continue;
            }
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

    fn move_select_up(&mut self) {
        if let AppMode::Selecting { selected, .. } = &mut self.mode {
            if *selected > 0 {
                *selected -= 1;
            }
        }
    }

    fn move_select_down(&mut self) {
        if let AppMode::Selecting {
            selected, options, ..
        } = &mut self.mode
        {
            if *selected + 1 < options.len() {
                *selected += 1;
            }
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
        let field = &tool.fields[self.field_index];

        // Show OAuth re-authentication command for non-editable auth fields
        if !field.editable {
            if field.key == "Auth" || (field.value.starts_with('✓') && !field.key.contains(" ▸ "))
            {
                let cmd = match tool.tool {
                    Tool::KakuAssistant => None,
                    Tool::OpenCode => Some("opencode auth"),
                    Tool::Gemini => Some("gemini auth login"),
                    Tool::Codex => Some("codex auth login"),
                    Tool::Copilot => Some("gh auth login"),
                    Tool::FactoryDroid => Some("droid"),
                    Tool::ClaudeCode => Some("claude auth login"),
                    Tool::OpenClaw => None,
                };

                if let Some(auth_cmd) = cmd {
                    self.open_in_terminal(auth_cmd);
                } else {
                    self.status_msg = Some("OpenClaw uses API keys, check config file".to_string());
                }
            } else if field.value.starts_with('✓') {
                // OAuth provider in OpenCode auth.json (e.g., "openai", "google", "github-copilot")
                let auth_cmd = format!("opencode auth add {}", field.key.as_str());
                self.open_in_terminal(&auth_cmd);
            }
            return;
        }

        if !field.options.is_empty() {
            self.mode = AppMode::Selecting {
                field_idx: self.field_index,
                options: field.options.clone(),
                selected: field
                    .options
                    .iter()
                    .position(|o| *o == field.value)
                    .unwrap_or(0),
            };
            self.focus = Focus::Editor;
            return;
        }

        // For API Key fields, load the full key from auth.json (OpenCode) or config
        let edit_buf = if field.value == "—" {
            // Empty placeholder
            String::new()
        } else if tool.tool == Tool::KakuAssistant && field.key == "API Key" {
            get_kaku_assistant_api_key().unwrap_or_else(String::new)
        } else if field.key.contains("API Key") && !field.key.contains(" ▸ ") {
            // OpenCode provider API Key from opencode.json - keep masked value behavior
            String::new()
        } else if tool.tool == Tool::OpenCode
            && !field.key.contains(" ▸ ")
            && field.editable
            && field.value.starts_with("✓")
        {
            // OpenCode auth.json API Key - load full key (editable API type fields)
            get_opencode_api_key(&field.key).unwrap_or_else(String::new)
        } else {
            field.value.clone()
        };
        let edit_cursor = edit_buf.len(); // Start cursor at end (always a valid byte boundary)
        self.mode = AppMode::Editing {
            field_idx: self.field_index,
            buffer: edit_buf,
            cursor: edit_cursor,
        };
        self.focus = Focus::Editor;
    }

    fn confirm_select(&mut self) {
        let AppMode::Selecting {
            field_idx,
            options,
            selected,
        } = std::mem::replace(&mut self.mode, AppMode::Browsing)
        else {
            return;
        };
        self.focus = Focus::ToolList;

        if self.tool_index >= self.tools.len() {
            return;
        }
        if field_idx >= self.tools[self.tool_index].fields.len() {
            return;
        }
        if selected >= options.len() {
            return;
        }

        self.field_index = field_idx;
        let new_val = options[selected].clone();
        let tool_kind = self.tools[self.tool_index].tool;
        let field_key = self.tools[self.tool_index].fields[field_idx].key.clone();
        let old_val = self.tools[self.tool_index].fields[field_idx].value.clone();

        if new_val == old_val {
            return;
        }

        self.tools[self.tool_index].fields[field_idx].value = new_val.clone();
        let status_val = status_value_for_display(&field_key, &new_val);
        match save_field(tool_kind, &field_key, &new_val) {
            Ok(()) => self.status_msg = Some(format!("Saved {} → {}", field_key, status_val)),
            Err(e) => {
                self.status_msg = Some(format!("Save failed: {}", e));
                self.set_error(format!("Save failed: {}", e));
            }
        }
        self.reload_current_tool();
    }

    fn cancel_select(&mut self) {
        self.mode = AppMode::Browsing;
        self.focus = Focus::ToolList;
    }

    fn confirm_edit(&mut self) {
        let AppMode::Editing {
            field_idx,
            buffer,
            cursor: _,
        } = std::mem::replace(&mut self.mode, AppMode::Browsing)
        else {
            return;
        };
        self.focus = Focus::ToolList;

        let tool = &mut self.tools[self.tool_index];
        if field_idx >= tool.fields.len() {
            return;
        }

        self.field_index = field_idx;
        let new_val = buffer.trim().to_string();
        let field_key = tool.fields[field_idx].key.clone();

        // Empty input on API Key fields means cancel, not clear
        if new_val.is_empty() && field_key.contains("API Key") {
            return;
        }

        let old_val = tool.fields[field_idx].value.clone();
        if new_val == old_val || (new_val.is_empty() && old_val == "—") {
            return;
        }

        tool.fields[field_idx].value = new_val.clone();

        let status_val = status_value_for_display(&field_key, &new_val);
        match save_field(tool.tool, &field_key, &new_val) {
            Ok(()) => self.status_msg = Some(format!("Saved {} → {}", field_key, status_val)),
            Err(e) => {
                self.status_msg = Some(format!("Save failed: {}", e));
                self.set_error(format!("Save failed: {}", e));
            }
        }
        self.reload_current_tool();
    }

    fn reload_current_tool(&mut self) {
        let tool_type = self.tools[self.tool_index].tool;
        self.tools[self.tool_index] = ToolState::load(tool_type);
    }

    fn cancel_edit(&mut self) {
        self.mode = AppMode::Browsing;
        self.focus = Focus::ToolList;
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

    fn refresh_models(&mut self) {
        // Delete cache to force re-fetch
        let cache_path = config::HOME_DIR
            .join(".cache")
            .join("kaku")
            .join("models.json");
        let _ = std::fs::remove_file(&cache_path);

        match fetch_models_dev_json() {
            Some(_) => {
                self.tools = ALL_TOOLS
                    .iter()
                    .map(|t| ToolState::load(*t))
                    .filter(|t| t.tool == Tool::KakuAssistant || t.installed)
                    .collect();
                self.status_msg = Some("Models refreshed".into());
                self.sync_transient_errors();
            }
            None => {
                self.status_msg = Some("Refresh failed (network error)".into());
                self.set_error("Models refresh failed. Check network or proxy.");
            }
        }
    }

    /// Open a shell command in a new Kaku tab (preferred) or fall back to Terminal.app.
    fn open_in_terminal(&mut self, cmd: &str) {
        // Prefer a new tab in the current Kaku window.
        // kaku cli spawn reads WEZTERM_PANE from the environment to target the right window.
        // Append `exec $SHELL` so the pane stays alive after the command finishes.
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let shell_cmd = format!("{}; exec \"{}\"", cmd, shell);
        let kaku_status = std::process::Command::new("kaku")
            .args(["cli", "spawn", "--", &shell, "-c", &shell_cmd])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if kaku_status.as_ref().is_ok_and(|status| status.success()) {
            self.status_msg = Some("Opening in new Kaku tab...".into());
            return;
        }

        // Fallback: open in macOS Terminal.app via osascript.
        let script = format!("tell application \"Terminal\" to do script \"{}\"", cmd);
        match std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .spawn()
        {
            Ok(_) => self.status_msg = Some("Opening in new terminal window...".into()),
            Err(_) => {
                self.status_msg = Some(format!("Failed to open terminal. Run '{}' manually", cmd))
            }
        }
    }
}

fn save_field(tool: Tool, field_key: &str, new_val: &str) -> anyhow::Result<()> {
    if tool == Tool::KakuAssistant {
        return save_kaku_assistant_field(field_key, new_val);
    }

    // Codex uses TOML; delegate immediately before any JSON parsing attempt.
    if tool == Tool::Codex {
        return save_codex_field(field_key, new_val);
    }

    let path = tool.config_path();
    if !path.exists() {
        anyhow::bail!("config file not found: {}", path.display());
    }

    let raw = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut parsed: serde_json::Value =
        parse_json_or_jsonc(&raw).with_context(|| format!("parse {}", path.display()))?;

    match tool {
        Tool::KakuAssistant => unreachable!("Kaku Assistant is handled before JSON parsing"),
        Tool::Gemini => {
            if field_key == "Model" {
                if let Some(obj) = parsed.as_object_mut() {
                    if new_val == "—" || new_val.is_empty() {
                        obj.remove("model");
                    } else {
                        let keep_string_shape = obj.get("model").is_some_and(|m| m.is_string());
                        if keep_string_shape {
                            obj.insert(
                                "model".to_string(),
                                serde_json::Value::String(new_val.to_string()),
                            );
                        } else {
                            obj.insert("model".to_string(), serde_json::json!({"name": new_val}));
                        }
                    }
                }
            } else {
                return Ok(());
            }
        }
        Tool::Copilot => {
            if field_key == "Model" {
                if let Some(obj) = parsed.as_object_mut() {
                    if new_val == "—" || new_val.is_empty() {
                        obj.remove("model");
                    } else {
                        obj.insert(
                            "model".to_string(),
                            serde_json::Value::String(new_val.to_string()),
                        );
                    }
                }
            } else {
                return Ok(());
            }
        }
        Tool::FactoryDroid => {
            let obj = parsed.as_object_mut().context("root is not object")?;
            save_factory_droid_field(obj, field_key, new_val)?;
        }
        Tool::ClaudeCode => {
            let env_key = match field_key {
                "Base URL" => Some("ANTHROPIC_BASE_URL"),
                "API Key" => Some("ANTHROPIC_AUTH_TOKEN"),
                _ => None,
            };
            let top_key = match field_key {
                "Model" => Some("model"),
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
            let top_key = match field_key {
                "Model" => Some("model"),
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
                // Auth.json provider API keys (field_key is provider name like "kimi-for-coding")
                let auth_path = config::HOME_DIR
                    .join(".local")
                    .join("share")
                    .join("opencode")
                    .join("auth.json");

                if !auth_path.exists() {
                    return Ok(());
                }

                let auth_raw = std::fs::read_to_string(&auth_path)
                    .with_context(|| format!("read {}", auth_path.display()))?;
                let mut auth_parsed: serde_json::Value = serde_json::from_str(&auth_raw)
                    .with_context(|| format!("parse {}", auth_path.display()))?;

                if let Some(auth_obj) = auth_parsed.as_object_mut() {
                    if let Some(provider) = auth_obj.get_mut(field_key) {
                        if let Some(provider_obj) = provider.as_object_mut() {
                            // Check if this is an API type provider
                            if provider_obj.get("type").and_then(|v| v.as_str()) == Some("api") {
                                if new_val == "—" || new_val.is_empty() {
                                    provider_obj.remove("key");
                                } else {
                                    provider_obj.insert(
                                        "key".to_string(),
                                        serde_json::Value::String(new_val.to_string()),
                                    );
                                }

                                // Save to auth.json
                                let output = serde_json::to_string_pretty(&auth_parsed)
                                    .context("serialize auth.json")?;
                                write_atomic(&auth_path, output.as_bytes())
                                    .with_context(|| format!("write {}", auth_path.display()))?;
                            }
                        }
                    }
                }
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
        Tool::Codex => unreachable!("Codex is handled before JSON parsing"),
    }

    let output = serde_json::to_string_pretty(&parsed).context("serialize config")?;
    if is_jsonc_path(&path) {
        log::info!(
            "Note: {} comments will be removed when Kaku rewrites this file.",
            path.display()
        );
    }
    write_atomic(&path, output.as_bytes()).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Save a field to Codex TOML config (~/.codex/config.toml)
fn save_codex_field(field_key: &str, new_val: &str) -> anyhow::Result<()> {
    let path = Tool::Codex.config_path();
    save_codex_field_at(&path, field_key, new_val)
}

fn save_codex_field_at(path: &Path, field_key: &str, new_val: &str) -> anyhow::Result<()> {
    let toml_key = match field_key {
        "Model" => "model",
        "Reasoning Effort" => "model_reasoning_effort",
        _ => return Ok(()),
    };

    let raw = if path.exists() {
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?
    } else {
        String::new()
    };

    let mut lines: Vec<String> = raw.lines().map(|l| l.to_string()).collect();
    let target = format!("{} = ", toml_key);
    let new_line = format!("{} = \"{}\"", toml_key, new_val);

    let mut found = false;
    for line in &mut lines {
        if line.trim_start().starts_with(&target) {
            if new_val == "—" || new_val.is_empty() {
                *line = String::new();
            } else {
                *line = new_line.clone();
            }
            found = true;
            break;
        }
    }

    if !found && !new_val.is_empty() && new_val != "—" {
        // Insert before the first [section] or at the end
        let insert_pos = lines
            .iter()
            .position(|l| l.trim_start().starts_with('['))
            .unwrap_or(lines.len());
        lines.insert(insert_pos, new_line);
    }

    // Remove empty lines that resulted from deletion
    let output: Vec<&str> = lines.iter().map(|l| l.as_str()).collect();
    let result = output.join("\n");
    write_atomic(path, result.as_bytes()).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

pub fn run() -> anyhow::Result<()> {
    enable_raw_mode().context("enable raw mode")?;
    crossterm::execute!(io::stdout(), EnableBracketedPaste).context("enable bracketed paste")?;
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let mut app = App::new();
    let result = run_loop(&mut terminal, &mut app);

    let _ = crossterm::execute!(io::stdout(), DisableBracketedPaste);
    disable_raw_mode().context("disable raw mode")?;
    terminal.show_cursor().context("show cursor")?;
    // Emit a trailing newline so zsh does not show a partial-line `%` indicator.
    println!();

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    loop {
        app.sync_transient_errors();
        terminal.draw(|frame| ui::ui(frame, app))?;

        match event::read().context("read event")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                app.status_msg = None;
                if app.is_selecting() {
                    match key.code {
                        KeyCode::Enter | KeyCode::Char('\n') | KeyCode::Char('\r') => {
                            app.confirm_select()
                        }
                        KeyCode::Esc => app.cancel_select(),
                        KeyCode::Up | KeyCode::Char('k') => app.move_select_up(),
                        KeyCode::Down | KeyCode::Char('j') => app.move_select_down(),
                        _ => {}
                    }
                    continue;
                }

                if app.is_editing() {
                    match key.code {
                        KeyCode::Enter | KeyCode::Char('\n') | KeyCode::Char('\r') => {
                            app.confirm_edit()
                        }
                        KeyCode::Esc => app.cancel_edit(),
                        KeyCode::Left => {
                            if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                if *edit_cursor > 0 {
                                    *edit_cursor = prev_char_boundary(edit_buf, *edit_cursor);
                                }
                            }
                        }
                        KeyCode::Right => {
                            if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                if *edit_cursor < edit_buf.len() {
                                    *edit_cursor = next_char_boundary(edit_buf, *edit_cursor);
                                }
                            }
                        }
                        KeyCode::Home => {
                            if let Some((_, edit_cursor)) = app.editing_mut() {
                                *edit_cursor = 0;
                            }
                        }
                        KeyCode::End => {
                            if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                *edit_cursor = edit_buf.len();
                            }
                        }
                        KeyCode::Backspace => {
                            if key.modifiers.contains(KeyModifiers::CONTROL)
                                || key.modifiers.contains(KeyModifiers::SUPER)
                            {
                                // Cmd+Backspace (macOS) or Ctrl+Backspace - clear all
                                if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                    edit_buf.clear();
                                    *edit_cursor = 0;
                                }
                            } else if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                if *edit_cursor > 0 {
                                    edit_backspace(edit_buf, edit_cursor);
                                }
                            }
                        }
                        KeyCode::Delete => {
                            if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                if *edit_cursor < edit_buf.len() {
                                    edit_delete(edit_buf, *edit_cursor);
                                }
                            }
                        }
                        KeyCode::Char(c) => {
                            // Handle Ctrl+U (clear line) - macOS Cmd+Backspace may also send this
                            if (key.modifiers.contains(KeyModifiers::CONTROL)
                                || key.modifiers.contains(KeyModifiers::SUPER))
                                && c == 'u'
                            {
                                if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                    edit_buf.clear();
                                    *edit_cursor = 0;
                                }
                            }
                            // Ignore other control characters
                            else if !key.modifiers.contains(KeyModifiers::CONTROL)
                                && !key.modifiers.contains(KeyModifiers::SUPER)
                            {
                                if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                                    edit_insert_char(edit_buf, edit_cursor, c);
                                }
                            }
                        }
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
                    KeyCode::Enter | KeyCode::Char('\n') | KeyCode::Char('\r') => app.start_edit(),
                    KeyCode::Char('o') => app.open_config(),
                    KeyCode::Char('r') => app.refresh_models(),
                    _ => {}
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => app.move_up(),
                MouseEventKind::ScrollDown => app.move_down(),
                _ => {}
            },
            Event::Paste(text) => {
                if !app.is_editing() || text.is_empty() {
                    continue;
                }

                let ends_with_newline = text.ends_with('\n') || text.ends_with('\r');

                // Clipboard paste may include a trailing newline from terminal copy.
                // Strip line breaks so paste doesn't break single-line inputs.
                let cleaned: String = text.chars().filter(|c| *c != '\r' && *c != '\n').collect();
                if !cleaned.is_empty() {
                    if let Some((edit_buf, edit_cursor)) = app.editing_mut() {
                        for c in cleaned.chars() {
                            edit_insert_char(edit_buf, edit_cursor, c);
                        }
                    }
                }

                // If the pasted text (often from voice typing tools) ends with a newline, auto-submit
                if ends_with_newline {
                    app.confirm_edit();
                }
            }
            _ => {}
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn prev_char_boundary(buf: &str, cursor: usize) -> usize {
    let cursor = cursor.min(buf.len());
    if cursor == 0 {
        return 0;
    }
    buf[..cursor]
        .char_indices()
        .next_back()
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn next_char_boundary(buf: &str, cursor: usize) -> usize {
    let cursor = cursor.min(buf.len());
    if cursor >= buf.len() {
        return buf.len();
    }
    cursor + buf[cursor..].chars().next().map_or(0, |c| c.len_utf8())
}

fn edit_backspace(buf: &mut String, cursor: &mut usize) {
    if *cursor == 0 || buf.is_empty() {
        return;
    }
    let prev = prev_char_boundary(buf, *cursor);
    buf.remove(prev);
    *cursor = prev;
}

fn edit_delete(buf: &mut String, cursor: usize) {
    if cursor >= buf.len() || buf.is_empty() {
        return;
    }
    buf.remove(cursor);
}

fn edit_insert_char(buf: &mut String, cursor: &mut usize, c: char) {
    let at = (*cursor).min(buf.len());
    buf.insert(at, c);
    *cursor = at + c.len_utf8();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn codex_save_round_trip_for_model_and_reasoning_effort() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");

        std::fs::write(
            &path,
            "model = \"old\"\nmodel_reasoning_effort = \"low\"\n\n[projects.\"/tmp\"]\ntrust_level = \"trusted\"\n",
        )
        .expect("seed config");

        save_codex_field_at(&path, "Model", "gpt-5").expect("update model");
        save_codex_field_at(&path, "Reasoning Effort", "high").expect("update effort");
        let saved = std::fs::read_to_string(&path).expect("read config");
        assert!(saved.contains("model = \"gpt-5\""));
        assert!(saved.contains("model_reasoning_effort = \"high\""));
        assert!(saved.contains("[projects.\"/tmp\"]"));

        save_codex_field_at(&path, "Model", "").expect("remove model");
        let saved = std::fs::read_to_string(&path).expect("read config");
        assert!(!saved.contains("model = \"gpt-5\""));
        assert!(saved.contains("model_reasoning_effort = \"high\""));
        assert!(saved.contains("[projects.\"/tmp\"]"));
    }

    #[test]
    fn codex_save_creates_new_top_level_entry_before_sections() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[profiles.default]\nfoo = \"bar\"\n").expect("seed config");

        save_codex_field_at(&path, "Model", "gpt-5").expect("insert model");
        let saved = std::fs::read_to_string(&path).expect("read config");
        let model_pos = saved.find("model = \"gpt-5\"").expect("model line");
        let section_pos = saved.find("[profiles.default]").expect("section");
        assert!(model_pos < section_pos);
    }

    #[test]
    fn unicode_edit_helpers_keep_cursor_on_char_boundaries() {
        let mut buf = "你a好".to_string();
        let mut cursor = buf.len();

        cursor = prev_char_boundary(&buf, cursor);
        assert_eq!(cursor, "你a".len());
        cursor = prev_char_boundary(&buf, cursor);
        assert_eq!(cursor, "你".len());
        cursor = prev_char_boundary(&buf, cursor);
        assert_eq!(cursor, 0);

        cursor = next_char_boundary(&buf, cursor);
        assert_eq!(cursor, "你".len());
        cursor = next_char_boundary(&buf, cursor);
        assert_eq!(cursor, "你a".len());

        edit_insert_char(&mut buf, &mut cursor, '界');
        assert_eq!(buf, "你a界好");
        assert_eq!(cursor, "你a界".len());

        edit_backspace(&mut buf, &mut cursor);
        assert_eq!(buf, "你a好");
        assert_eq!(cursor, "你a".len());

        cursor = prev_char_boundary(&buf, cursor);
        assert_eq!(cursor, "你".len());
        edit_delete(&mut buf, cursor);
        assert_eq!(buf, "你好");
        assert_eq!(cursor, "你".len());
    }

    #[test]
    fn factory_droid_extract_prefers_session_defaults() {
        let parsed = serde_json::json!({
            "model": "legacy-model",
            "reasoningEffort": "low",
            "autonomyMode": "spec",
            "sessionDefaultSettings": {
                "model": "fd-model",
                "reasoningEffort": "medium",
                "autonomyLevel": "auto-low"
            }
        });

        let fields = extract_factory_droid_fields(&parsed);
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].key, "Model");
        assert_eq!(fields[0].value, "fd-model");
        assert!(fields[0].options.is_empty());

        assert_eq!(fields[1].key, "Reasoning Effort");
        assert_eq!(fields[1].value, "medium");
        assert_eq!(
            fields[1].options,
            string_options(&FACTORY_DROID_REASONING_OPTIONS)
        );

        assert_eq!(fields[2].key, "Autonomy Level");
        assert_eq!(fields[2].value, "auto-low");
        assert_eq!(
            fields[2].options,
            string_options(&FACTORY_DROID_AUTONOMY_OPTIONS)
        );
    }

    #[test]
    fn factory_droid_extract_falls_back_to_top_level_and_defaults() {
        let parsed = serde_json::json!({
            "model": "legacy-model",
            "reasoningEffort": "high",
            "autonomyLevel": "spec"
        });

        let fields = extract_factory_droid_fields(&parsed);
        assert_eq!(fields[0].value, "legacy-model");
        assert_eq!(fields[1].value, "high");
        assert_eq!(fields[2].value, "spec");

        let empty_fields = extract_factory_droid_fields(&serde_json::json!({}));
        assert_eq!(empty_fields[0].value, FACTORY_DROID_DEFAULT_MODEL);
        assert_eq!(empty_fields[1].value, FACTORY_DROID_DEFAULT_REASONING);
        assert_eq!(empty_fields[2].value, FACTORY_DROID_DEFAULT_AUTONOMY);
    }

    #[test]
    fn factory_droid_save_writes_session_defaults_and_normalizes_autonomy_key() {
        let mut obj = serde_json::json!({
            "autonomyLevel": "spec"
        })
        .as_object()
        .cloned()
        .expect("object");

        save_factory_droid_field(&mut obj, "Autonomy Level", "auto-high").expect("save autonomy");
        save_factory_droid_field(&mut obj, "Model", "gpt-5.1-codex").expect("save model");
        save_factory_droid_field(&mut obj, "Reasoning Effort", "").expect("clear reasoning effort");

        let session_defaults = obj
            .get("sessionDefaultSettings")
            .and_then(|v| v.as_object())
            .expect("sessionDefaultSettings object");

        assert_eq!(
            session_defaults
                .get("autonomyMode")
                .and_then(|v| v.as_str()),
            Some("auto-high")
        );
        assert_eq!(session_defaults.get("autonomyLevel"), None);
        assert_eq!(
            session_defaults.get("model").and_then(|v| v.as_str()),
            Some("gpt-5.1-codex")
        );
        assert_eq!(session_defaults.get("reasoningEffort"), None);
    }

    #[test]
    fn kaku_assistant_enabled_shows_not_configured_without_api_key() {
        let fields = extract_kaku_assistant_fields("enabled = true\n");
        let enabled = fields
            .iter()
            .find(|f| f.key == "Enabled")
            .expect("enabled field");
        assert_eq!(enabled.value, "Not configured");
    }

    #[test]
    fn kaku_assistant_enabled_shows_off_when_disabled() {
        let fields = extract_kaku_assistant_fields("enabled = false\n");
        let enabled = fields
            .iter()
            .find(|f| f.key == "Enabled")
            .expect("enabled field");
        assert_eq!(enabled.value, "Off");
    }
}
