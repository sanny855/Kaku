use std::path::PathBuf;
use crate::tui_core::components::{select_box::SelectBox, text_input::TextInput, toggle::Toggle};
use crate::tui_core::form::{FormApp, FormField, FormFieldWidget};
use super::state::ConfigField;

pub struct App {
    pub form: FormApp<ConfigField>,
    pub dirty: bool,
    pub has_saved: bool,
    pub initial_theme: String,
}

impl App {
    pub fn new() -> Self {
        let fields = vec![
            // Appearance
            ConfigField {
                key: "Theme",
                lua_key: "color_scheme",
                value: String::new(),
                default: "Kaku Dark".into(),
                options: vec!["Kaku Dark", "Kaku Light"],
                skip_write: false,
            },
            ConfigField {
                key: "Font",
                lua_key: "font",
                value: String::new(),
                default: "JetBrains Mono".into(),
                options: vec![],
                skip_write: false,
            },
            ConfigField {
                key: "Font Size",
                lua_key: "font_size",
                value: String::new(),
                default: "17".into(),
                options: vec![],
                skip_write: false,
            },
            ConfigField {
                key: "Line Height",
                lua_key: "line_height",
                value: String::new(),
                default: "1.28".into(),
                options: vec![],
                skip_write: false,
            },
            ConfigField {
                key: "Global Hotkey",
                lua_key: "macos_global_hotkey",
                value: String::new(),
                default: "Ctrl+Alt+Cmd+K".into(),
                options: vec![],
                skip_write: false,
            },
            ConfigField {
                key: "Tab Bar Position",
                lua_key: "tab_bar_at_bottom",
                value: String::new(),
                default: "Bottom".into(),
                options: vec!["Bottom", "Top"],
                skip_write: false,
            },
            ConfigField {
                key: "Copy on Select",
                lua_key: "copy_on_select",
                value: String::new(),
                default: "On".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
            ConfigField {
                key: "Shadow",
                lua_key: "window_decorations",
                value: String::new(),
                default: "On".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
            ConfigField {
                key: "Bell Tab Indicator",
                lua_key: "bell_tab_indicator",
                value: String::new(),
                default: "On".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
            ConfigField {
                key: "Bell Dock Badge",
                lua_key: "bell_dock_badge",
                value: String::new(),
                default: "Off".into(),
                options: vec!["On", "Off"],
                skip_write: false,
            },
        ];

        let form_fields = fields.into_iter().map(|f| {
            let widget = if f.has_options() {
                if f.options.len() == 2 && (f.options.contains(&"On") || f.options.contains(&"Bottom")) {
                    FormFieldWidget::Toggle(Toggle::new(f.value == "On" || f.value == "Bottom", f.key))
                } else {
                    FormFieldWidget::SelectBox(SelectBox::new(
                        f.options.iter().map(|s| s.to_string()).collect(),
                        0,
                        f.key,
                    ))
                }
            } else {
                FormFieldWidget::TextInput(TextInput::new(f.value.clone()).with_placeholder(f.default.clone()))
            };

            FormField {
                key: f.lua_key.to_string(),
                label: f.key.to_string(),
                widget,
                data: f,
            }
        }).collect();

        Self {
            form: FormApp::new(form_fields),
            dirty: false,
            has_saved: false,
            initial_theme: String::new(),
        }
    }
    pub fn load_config(&mut self) {
        let config_path = self.config_path();
        if !config_path.exists() {
            return;
        }

        let content = match std::fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(_) => return,
        };

        for i in 0..self.form.fields.len() {
            let lua_key = self.form.fields[i].data.lua_key;
            match Self::extract_lua_value(&content, lua_key) {
                Some(val) => match Self::normalize_value(lua_key, &val) {
                    Some(normalized) => self.form.fields[i].data.value = normalized,
                    // Recognized key, but value format is unsupported.
                    // Mark skip_write so save never corrupts this line.
                    None => self.form.fields[i].data.skip_write = true,
                },
                None => {
                    // extract_lua_value returns None when the wezterm.* guard fires
                    // (line exists but value is an unsupported API call).
                    // Only set skip_write when a config line actually exists for this key.
                    if Self::has_config_line(&content, lua_key) {
                        self.form.fields[i].data.skip_write = true;
                    }
                }
            }
        }
        self.sync_data_to_widgets();
    }

    /// Returns true if a non-commented `config.<key>` assignment exists in content.
    fn has_config_line(content: &str, key: &str) -> bool {
        let pattern = format!("config.{}", key);
        content.lines().any(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("--") {
                return false;
            }
            if !trimmed.starts_with(&pattern) {
                return false;
            }
            let after = &trimmed[pattern.len()..];
            after.starts_with(|c: char| c.is_whitespace() || c == '=')
        })
    }

    fn config_path(&self) -> PathBuf {
        config::user_config_path()
    }

    fn extract_lua_value(content: &str, key: &str) -> Option<String> {
        let pattern = format!("config.{}", key);
        for line in content.lines() {
            let trimmed = line.trim();
            // Skip comments
            if trimmed.starts_with("--") {
                continue;
            }
            if !trimmed.starts_with(&pattern) {
                continue;
            }
            // Ensure exact key match (not prefix like font vs font_size)
            let after_pattern = &trimmed[pattern.len()..];
            if !after_pattern.starts_with(|c: char| c.is_whitespace() || c == '=') {
                continue;
            }
            let eq_pos = trimmed.find('=')?;
            let value_part = trimmed[eq_pos + 1..].trim();

            // Handle different value types
            if value_part.starts_with("wezterm.font(") {
                // Extract font name from wezterm.font('Name') or wezterm.font("Name")
                return Self::extract_quoted_arg(value_part, "wezterm.font(");
            }
            // Unknown wezterm API call (e.g. wezterm.font_with_fallback): skip to
            // avoid corrupting the value on write-back via to_lua_value.
            if value_part.starts_with("wezterm.") {
                return None;
            }
            if value_part.starts_with('{') {
                // Table value - return as-is up to end or comment
                return Some(Self::strip_trailing_comment(value_part));
            }
            if value_part.starts_with('\'') || value_part.starts_with('"') {
                // Quoted string
                let quote = value_part.chars().next().unwrap();
                if let Some(end) = value_part[1..].find(quote) {
                    return Some(value_part[1..1 + end].to_string());
                }
            }
            // Number, boolean, or identifier
            let value = Self::strip_trailing_comment(value_part);
            return Some(value);
        }
        None
    }

    fn extract_quoted_arg(s: &str, prefix: &str) -> Option<String> {
        let rest = s.strip_prefix(prefix)?;
        let quote = rest.chars().next()?;
        if quote != '\'' && quote != '"' {
            return None;
        }
        let inner = &rest[1..];
        let end = inner.find(quote)?;
        Some(inner[..end].to_string())
    }

    fn strip_trailing_comment(s: &str) -> String {
        // Remove Lua line comment (--) but be careful with strings
        let mut in_string = false;
        let mut quote_char = ' ';
        let mut result_end = s.len();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if in_string {
                if c == quote_char && (i == 0 || chars[i - 1] != '\\') {
                    in_string = false;
                }
            } else if c == '\'' || c == '"' {
                in_string = true;
                quote_char = c;
            } else if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
                result_end = i;
                break;
            }
            i += 1;
        }
        s[..result_end].trim().to_string()
    }

    fn extract_table_quoted_value(raw: &str, key: &str) -> Option<String> {
        let needle = format!("{key} = ");
        let start = raw.find(&needle)? + needle.len();
        let rest = raw[start..].trim_start();
        let quote = rest.chars().next()?;
        if quote != '\'' && quote != '"' {
            return None;
        }
        let inner = &rest[1..];
        let end = inner.find(quote)?;
        Some(inner[..end].to_string())
    }

    fn normalize_hotkey_table(raw: &str) -> Option<String> {
        let key = Self::extract_table_quoted_value(raw, "key")?;
        let mods = Self::extract_table_quoted_value(raw, "mods").unwrap_or_default();
        let mut parts: Vec<String> = Vec::new();
        for token in mods.split('|') {
            match token.trim().to_ascii_uppercase().as_str() {
                "CTRL" | "CONTROL" => parts.push("Ctrl".to_string()),
                "ALT" | "OPT" | "OPTION" => parts.push("Alt".to_string()),
                "SUPER" | "CMD" | "COMMAND" => parts.push("Cmd".to_string()),
                "SHIFT" => parts.push("Shift".to_string()),
                _ => {}
            }
        }
        parts.push(key.to_ascii_uppercase());
        Some(parts.join("+"))
    }

    fn hotkey_to_lua(value: &str) -> Option<String> {
        let parts: Vec<&str> = value
            .split('+')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            return None;
        }

        let key = parts.last()?.to_ascii_uppercase();
        let mut mods: Vec<&str> = Vec::new();
        for token in &parts[..parts.len() - 1] {
            match token.to_ascii_uppercase().as_str() {
                "CTRL" | "CONTROL" => mods.push("CTRL"),
                "ALT" | "OPT" | "OPTION" => mods.push("ALT"),
                "CMD" | "SUPER" | "COMMAND" => mods.push("SUPER"),
                "SHIFT" => mods.push("SHIFT"),
                _ => {}
            }
        }
        if mods.is_empty() {
            return None;
        }

        Some(format!(
            "{{ key = '{}', mods = '{}' }}",
            key,
            mods.join("|")
        ))
    }

    /// Converts a raw Lua value string into the TUI's internal display format.
    /// Returns None when the value exists but cannot be parsed into a supported
    /// format; the caller should set skip_write=true to protect the original line.
    fn normalize_value(lua_key: &str, raw: &str) -> Option<String> {
        match lua_key {
            "copy_on_select" | "bell_tab_indicator" | "bell_dock_badge" => {
                if raw == "true" {
                    Some("On".into())
                } else if raw == "false" {
                    Some("Off".into())
                } else {
                    None
                }
            }
            "hide_tab_bar_if_only_one_tab" => {
                if raw == "true" {
                    Some("Auto".into())
                } else if raw == "false" {
                    Some("Always".into())
                } else {
                    None
                }
            }
            "tab_bar_at_bottom" => {
                if raw == "true" {
                    Some("Bottom".into())
                } else if raw == "false" {
                    Some("Top".into())
                } else {
                    None
                }
            }
            "harfbuzz_features" => {
                let stripped = raw.replace([' ', '\'', '"'], "");
                if stripped == "{calt=0,clig=0,liga=0}" {
                    Some("Off".into())
                } else if stripped == "{}"
                    || stripped.is_empty()
                    || stripped.eq_ignore_ascii_case("nil")
                {
                    Some("On".into())
                } else {
                    None
                }
            }
            "window_decorations" => {
                let value = raw.trim().trim_matches('\'').trim_matches('"');
                if value.contains("MACOS_FORCE_DISABLE_SHADOW") {
                    Some("Off".into())
                } else if value.contains("INTEGRATED_BUTTONS|RESIZE") {
                    Some("On".into())
                } else {
                    None
                }
            }
            "macos_global_hotkey" => {
                let value = raw.trim();
                if value.eq_ignore_ascii_case("nil") {
                    Some(String::new())
                } else if value.starts_with('{') {
                    Self::normalize_hotkey_table(value)
                } else {
                    None
                }
            }
            _ => Some(raw.to_string()),
        }
    }

    pub fn save_config(&mut self) -> anyhow::Result<()> {
        self.sync_widgets_to_data();
        self.has_saved = true;
        self.dirty = false;
        // Ensure config file exists with proper structure first
        config::ensure_user_config_exists()?;

        let config_path = self.config_path();
        let mut content = std::fs::read_to_string(&config_path).unwrap_or_default();

        for field in self.form.fields.iter().map(|f| &f.data) {
            // Never touch lines we couldn't fully parse — preserve user's original.
            if field.skip_write {
                continue;
            }
            let is_default = field.value.is_empty() || field.value == field.default;
            // Keep tab bar position explicit so switching back to Bottom
            // does not depend on removing a line and inheriting bundled defaults.
            let always_write = field.lua_key == "tab_bar_at_bottom";
            if is_default && !always_write {
                // Remove the config line if it exists
                content = self.remove_lua_config(&content, field.lua_key);
            } else {
                // Update or add the config line
                content = self.update_lua_config(&content, field);
            }
        }

        // Atomic write: write to a temp file then rename so the file watcher
        // always sees a fully-written config (never a truncated intermediate).
        //
        // Resolve symlinks so we write through to the real file rather than
        // replacing the symlink itself (which would break dotfile workflows).
        let real_path = std::fs::canonicalize(&config_path).unwrap_or(config_path);
        crate::utils::write_atomic(&real_path, content.as_bytes())?;

        Ok(())
    }

    fn remove_lua_config(&self, content: &str, lua_key: &str) -> String {
        let pattern = format!("config.{}", lua_key);
        let lines: Vec<&str> = content.lines().collect();
        let mut result: Vec<&str> = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim();

            // Keep comment lines
            if trimmed.starts_with("--") {
                result.push(line);
                i += 1;
                continue;
            }

            // Check if this line starts our target config
            if trimmed.starts_with(&pattern) {
                let after_pattern = &trimmed[pattern.len()..];
                if after_pattern.starts_with(|c: char| c.is_whitespace() || c == '=') {
                    // Found the config line to remove
                    // Check if value contains an unclosed brace (multi-line table)
                    if let Some(eq_pos) = trimmed.find('=') {
                        let value_part = trimmed[eq_pos + 1..].trim();
                        let mut brace_depth = Self::count_brace_depth(value_part);

                        // Skip additional lines if brace is unclosed
                        while brace_depth > 0 && i + 1 < lines.len() {
                            i += 1;
                            brace_depth += Self::count_brace_depth(lines[i]);
                        }
                    }
                    i += 1;
                    continue;
                }
            }

            result.push(line);
            i += 1;
        }

        result.join("\n")
    }

    fn count_brace_depth(s: &str) -> i32 {
        let mut depth = 0i32;
        let mut in_string = false;
        let mut quote_char = ' ';
        let chars: Vec<char> = s.chars().collect();

        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];

            // Handle Lua comments
            if !in_string && c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
                break;
            }

            if in_string {
                if c == quote_char && (i == 0 || chars[i - 1] != '\\') {
                    in_string = false;
                }
            } else if c == '\'' || c == '"' {
                in_string = true;
                quote_char = c;
            } else if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
            }
            i += 1;
        }
        depth
    }

    fn update_lua_config(&self, content: &str, field: &ConfigField) -> String {
        let lua_value = self.to_lua_value(field);
        let config_line = format!("config.{} = {}", field.lua_key, lua_value);
        let pattern = format!("config.{}", field.lua_key);

        let lines: Vec<&str> = content.lines().collect();
        let mut result: Vec<String> = Vec::new();
        let mut found = false;
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim();

            // Keep comment lines
            if trimmed.starts_with("--") {
                result.push(line.to_string());
                i += 1;
                continue;
            }

            // Check if this line starts our target config
            if trimmed.starts_with(&pattern) {
                let after_pattern = &trimmed[pattern.len()..];
                if after_pattern.starts_with(|c: char| c.is_whitespace() || c == '=') {
                    // Found the config line to replace
                    found = true;
                    result.push(config_line.clone());

                    // Skip continuation lines if multi-line table
                    if let Some(eq_pos) = trimmed.find('=') {
                        let value_part = trimmed[eq_pos + 1..].trim();
                        let mut brace_depth = Self::count_brace_depth(value_part);

                        while brace_depth > 0 && i + 1 < lines.len() {
                            i += 1;
                            brace_depth += Self::count_brace_depth(lines[i]);
                        }
                    }
                    i += 1;
                    continue;
                }
            }

            result.push(line.to_string());
            i += 1;
        }

        if !found {
            // Find "return config" and insert before it
            if let Some(pos) = result.iter().position(|l| l.trim() == "return config") {
                result.insert(pos, config_line);
            } else {
                result.push(config_line);
            }
        }

        result.join("\n")
    }

    fn to_lua_value(&self, field: &ConfigField) -> String {
        match field.lua_key {
            "color_scheme" => format!("'{}'", field.value),
            "font" => format!("wezterm.font('{}')", field.value),
            "font_size" | "line_height" | "window_background_opacity" | "split_pane_gap" => {
                field.value.clone()
            }
            "copy_on_select" | "bell_tab_indicator" | "bell_dock_badge" => {
                if field.value == "On" {
                    "true".into()
                } else {
                    "false".into()
                }
            }
            "hide_tab_bar_if_only_one_tab" => {
                if field.value == "Auto" {
                    "true".into()
                } else {
                    "false".into()
                }
            }
            "tab_bar_at_bottom" => {
                let effective = if field.value.is_empty() {
                    &field.default
                } else {
                    &field.value
                };
                if effective == "Bottom" {
                    "true".into()
                } else {
                    "false".into()
                }
            }
            "harfbuzz_features" => {
                if field.value == "On" {
                    "{}".into()
                } else {
                    "{ 'calt=0', 'clig=0', 'liga=0' }".into()
                }
            }
            "window_decorations" => {
                if field.value == "On" {
                    "'INTEGRATED_BUTTONS|RESIZE'".into()
                } else {
                    "'INTEGRATED_BUTTONS|RESIZE|MACOS_FORCE_DISABLE_SHADOW'".into()
                }
            }
            "macos_global_hotkey" => {
                if field.value.is_empty() {
                    "nil".into()
                } else {
                    // confirm_edit() already validated; nil is a defensive fallback.
                    Self::hotkey_to_lua(&field.value).unwrap_or_else(|| "nil".into())
                }
            }
            _ => format!("'{}'", field.value),
        }
    }

    fn sync_data_to_widgets(&mut self) {
        for field in &mut self.form.fields {
            let value = field.data.value.clone();
            match &mut field.widget {
                FormFieldWidget::TextInput(w) => {
                    w.text = value.clone();
                    w.cursor_position = w.text.chars().count();
                }
                FormFieldWidget::Toggle(w) => {
                    w.is_on = value == "Bottom" || value == "On";
                }
                FormFieldWidget::SelectBox(w) => {
                    if let Some(idx) = w.options.iter().position(|o| o == &value) {
                        w.selected_index = idx;
                        w.list_state.select(Some(idx));
                    }
                }
                FormFieldWidget::ListEditor(_) => {}
            }
        }
    }

    fn sync_widgets_to_data(&mut self) {
        for field in &mut self.form.fields {
            match &field.widget {
                FormFieldWidget::TextInput(w) => {
                    field.data.value = w.text.clone();
                }
                FormFieldWidget::Toggle(w) => {
                    if field.data.lua_key == "tab_bar_at_bottom" {
                        field.data.value = if w.is_on { "Bottom".to_string() } else { "Top".to_string() };
                    } else {
                        field.data.value = if w.is_on { "On".to_string() } else { "Off".to_string() };
                    }
                }
                FormFieldWidget::SelectBox(w) => {
                    if let Some(opt) = w.options.get(w.selected_index) {
                        field.data.value = opt.clone();
                    }
                }
                FormFieldWidget::ListEditor(_) => {}
            }
        }
    }
    pub fn save_if_dirty(&mut self) -> anyhow::Result<()> {
        if self.dirty {
            self.save_config()?;
        }
        Ok(())
    }
    pub fn capture_initial_theme(&mut self) {
        if let Some(field) = self.form.fields.iter().find(|f| f.data.lua_key == "color_scheme") {
            self.initial_theme = if field.data.value.is_empty() {
                field.data.default.clone()
            } else {
                field.data.value.clone()
            };
        }
    }

    pub fn theme_changed(&mut self) -> bool {
        self.sync_widgets_to_data();
        if let Some(field) = self.form.fields.iter().find(|f| f.data.lua_key == "color_scheme") {
            let current = if field.data.value.is_empty() {
                &field.data.default
            } else {
                &field.data.value
            };
            return current != &self.initial_theme;
        }
        false
    }
}
