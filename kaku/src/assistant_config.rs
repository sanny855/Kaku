//! Kaku Assistant configuration management.
//!
//! This module handles the configuration file for Kaku's built-in AI assistant,
//! including default values, file paths, and ensuring required configuration keys exist.
//!
//! The configuration is stored in `assistant.toml` in the user's Kaku config directory.

use anyhow::{Context, anyhow};
use std::path::{Path, PathBuf};

/// Default AI model to use when none is specified.
/// This is a lightweight, fast model suitable for command analysis.
pub const DEFAULT_MODEL: &str = "gpt-5-mini";

/// Default API base URL for the AI service.
pub const DEFAULT_BASE_URL: &str = "https://api.vivgrid.com/v1";

/// Returns the path to the assistant.toml configuration file.
///
/// The file is located in the same directory as the user's Kaku config,
/// typically `~/.config/kaku/assistant.toml` on macOS/Linux.
///
/// # Errors
/// Returns an error if the user config path cannot be determined or has no parent directory.
pub fn assistant_toml_path() -> anyhow::Result<PathBuf> {
    let user_config_path = config::user_config_path();
    let config_dir = user_config_path
        .parent()
        .ok_or_else(|| anyhow!("invalid user config path: {}", user_config_path.display()))?;
    Ok(config_dir.join("assistant.toml"))
}

/// Ensures the assistant.toml configuration file exists, creating it with defaults if necessary.
///
/// This function:
/// 1. Creates the config directory if it doesn't exist
/// 2. Writes a default configuration file if none exists
/// 3. Ensures required keys (model, base_url) are present, adding them if missing
///
/// # Returns
/// * `Ok(PathBuf)` - The path to the configuration file
///
/// # Errors
/// Returns an error if the config directory cannot be created or the file cannot be written.
pub fn ensure_assistant_toml_exists() -> anyhow::Result<PathBuf> {
    let path = assistant_toml_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("invalid assistant.toml path: {}", path.display()))?;
    config::create_user_owned_dirs(parent).context("create config directory")?;

    if !path.exists() {
        std::fs::write(&path, default_assistant_toml_template())
            .with_context(|| format!("write {}", path.display()))?;
    }

    ensure_required_keys(&path)?;
    Ok(path)
}

/// Returns the default assistant.toml configuration template.
///
/// This template includes documentation comments explaining each configuration option
/// and uses the default model and base URL constants.
///
/// The template has `enabled = true` but the API key is commented out,
/// requiring the user to explicitly configure their API key.
pub fn default_assistant_toml_template() -> String {
    format!(
        "# Kaku Assistant configuration\n\
# enabled: true enables command analysis suggestions; false disables requests.\n\
# api_key: provider API key, example: \"sk-xxxx\".\n\
# model: model id, example: \"DeepSeek-V3.2\" or \"gpt-5-mini\".\n\
# base_url: chat-completions API root URL.\n\
\n\
enabled = true\n\
# api_key = \"<your_api_key>\"\n\
model = \"{DEFAULT_MODEL}\"\n\
base_url = \"{DEFAULT_BASE_URL}\"\n"
    )
}

/// Ensures that required configuration keys exist in the assistant.toml file.
///
/// If the `model` or `base_url` keys are missing, they are added with their default values.
/// This ensures backward compatibility when new required fields are added.
///
/// # Arguments
/// * `path` - Path to the assistant.toml file
///
/// # Errors
/// Returns an error if the file cannot be read or written.
fn ensure_required_keys(path: &Path) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut updated = raw.trim_end().to_string();
    let mut changed = false;

    if !toml_has_key(&raw, "model") {
        if !updated.is_empty() {
            updated.push('\n');
        }
        updated.push_str(&format!("model = \"{DEFAULT_MODEL}\"\n"));
        changed = true;
    }

    if !toml_has_key(&raw, "base_url") {
        if !updated.is_empty() {
            updated.push('\n');
        }
        updated.push_str(&format!("base_url = \"{DEFAULT_BASE_URL}\"\n"));
        changed = true;
    }

    if changed {
        std::fs::write(path, updated.as_bytes())
            .with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

/// Checks if a TOML key exists in the given content (ignoring comments and section headers).
///
/// This is a simple parser that looks for `key = value` patterns, skipping lines
/// that are comments (starting with #) or section headers (starting with [).
///
/// # Arguments
/// * `content` - The TOML file content to search
/// * `key` - The key name to look for
///
/// # Returns
/// `true` if the key is found, `false` otherwise
fn toml_has_key(content: &str, key: &str) -> bool {
    for line in content.lines() {
        let head = line.split('#').next().unwrap_or("").trim();
        if head.is_empty() || head.starts_with('[') {
            continue;
        }
        if let Some((name, _)) = head.split_once('=') {
            if name.trim() == key {
                return true;
            }
        }
    }
    false
}
