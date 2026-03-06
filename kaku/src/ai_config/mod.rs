pub 
mod tui;

use anyhow::Context;
use clap::Parser;
use wezterm_term::color::SrgbaTuple;

fn opaque(color: SrgbaTuple) -> SrgbaTuple {
    SrgbaTuple(color.0, color.1, color.2, 1.0)
}

fn blend(base: SrgbaTuple, overlay: SrgbaTuple, amount: f32) -> SrgbaTuple {
    let amount = amount.clamp(0.0, 1.0);
    SrgbaTuple(
        base.0 + (overlay.0 - base.0) * amount,
        base.1 + (overlay.1 - base.1) * amount,
        base.2 + (overlay.2 - base.2) * amount,
        1.0,
    )
}

fn to_hex(color: SrgbaTuple) -> String {
    let (r, g, b, _) = opaque(color).to_srgb_u8();
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// Returns OpenCode theme JSON derived from the user's current Kaku palette.
pub fn opencode_theme_json() -> String {
    let palette = crate::tui_core::theme::current_theme_palette();

    let warning = palette.accent;
    let success = palette.secondary;
    let info = palette.info;
    let border = blend(
        palette.bg,
        palette.text,
        if palette.is_light { 0.14 } else { 0.18 },
    );
    let border_active = blend(
        palette.bg,
        palette.primary,
        if palette.is_light { 0.28 } else { 0.32 },
    );
    let border_subtle = blend(
        palette.bg,
        palette.text,
        if palette.is_light { 0.08 } else { 0.1 },
    );
    let element = blend(
        palette.bg,
        palette.text,
        if palette.is_light { 0.09 } else { 0.13 },
    );
    let diff_added_bg = blend(
        palette.bg,
        success,
        if palette.is_light { 0.14 } else { 0.18 },
    );
    let diff_removed_bg = blend(
        palette.bg,
        palette.error,
        if palette.is_light { 0.14 } else { 0.18 },
    );

    format!(
        r#"{{
  "$schema": "https://opencode.ai/theme.json",
  "defs": {{
    "bg": "{bg}",
    "panel": "{panel}",
    "element": "{element}",
    "text": "{text}",
    "muted": "{muted}",
    "primary": "{primary}",
    "secondary": "{secondary}",
    "accent": "{accent}",
    "error": "{error}",
    "warning": "{warning}",
    "success": "{success}",
    "info": "{info}",
    "border": "{border}",
    "border_active": "{border_active}",
    "border_subtle": "{border_subtle}",
    "diff_added_bg": "{diff_added_bg}",
    "diff_removed_bg": "{diff_removed_bg}"
  }},
  "theme": {{
    "primary": {{ "dark": "primary", "light": "primary" }},
    "secondary": {{ "dark": "secondary", "light": "secondary" }},
    "accent": {{ "dark": "accent", "light": "accent" }},
    "error": {{ "dark": "error", "light": "error" }},
    "warning": {{ "dark": "warning", "light": "warning" }},
    "success": {{ "dark": "success", "light": "success" }},
    "info": {{ "dark": "info", "light": "info" }},
    "text": {{ "dark": "text", "light": "text" }},
    "textMuted": {{ "dark": "muted", "light": "muted" }},
    "background": {{ "dark": "bg", "light": "bg" }},
    "backgroundPanel": {{ "dark": "panel", "light": "panel" }},
    "backgroundElement": {{ "dark": "element", "light": "element" }},
    "border": {{ "dark": "border", "light": "border" }},
    "borderActive": {{ "dark": "border_active", "light": "border_active" }},
    "borderSubtle": {{ "dark": "border_subtle", "light": "border_subtle" }},
    "diffAdded": {{ "dark": "success", "light": "success" }},
    "diffRemoved": {{ "dark": "error", "light": "error" }},
    "diffContext": {{ "dark": "muted", "light": "muted" }},
    "diffHunkHeader": {{ "dark": "primary", "light": "primary" }},
    "diffHighlightAdded": {{ "dark": "success", "light": "success" }},
    "diffHighlightRemoved": {{ "dark": "error", "light": "error" }},
    "diffAddedBg": {{ "dark": "diff_added_bg", "light": "diff_added_bg" }},
    "diffRemovedBg": {{ "dark": "diff_removed_bg", "light": "diff_removed_bg" }},
    "diffContextBg": {{ "dark": "bg", "light": "bg" }},
    "diffLineNumber": {{ "dark": "muted", "light": "muted" }},
    "diffAddedLineNumberBg": {{ "dark": "diff_added_bg", "light": "diff_added_bg" }},
    "diffRemovedLineNumberBg": {{ "dark": "diff_removed_bg", "light": "diff_removed_bg" }},
    "markdownText": {{ "dark": "text", "light": "text" }},
    "markdownHeading": {{ "dark": "primary", "light": "primary" }},
    "markdownLink": {{ "dark": "info", "light": "info" }},
    "markdownLinkText": {{ "dark": "primary", "light": "primary" }},
    "markdownCode": {{ "dark": "accent", "light": "accent" }},
    "markdownBlockQuote": {{ "dark": "muted", "light": "muted" }},
    "markdownEmph": {{ "dark": "accent", "light": "accent" }},
    "markdownStrong": {{ "dark": "secondary", "light": "secondary" }},
    "markdownHorizontalRule": {{ "dark": "muted", "light": "muted" }},
    "markdownListItem": {{ "dark": "primary", "light": "primary" }},
    "markdownListEnumeration": {{ "dark": "accent", "light": "accent" }},
    "markdownImage": {{ "dark": "info", "light": "info" }},
    "markdownImageText": {{ "dark": "primary", "light": "primary" }},
    "markdownCodeBlock": {{ "dark": "text", "light": "text" }},
    "syntaxComment": {{ "dark": "muted", "light": "muted" }},
    "syntaxKeyword": {{ "dark": "primary", "light": "primary" }},
    "syntaxFunction": {{ "dark": "secondary", "light": "secondary" }},
    "syntaxVariable": {{ "dark": "text", "light": "text" }},
    "syntaxString": {{ "dark": "success", "light": "success" }},
    "syntaxNumber": {{ "dark": "accent", "light": "accent" }},
    "syntaxType": {{ "dark": "info", "light": "info" }},
    "syntaxOperator": {{ "dark": "primary", "light": "primary" }},
    "syntaxPunctuation": {{ "dark": "text", "light": "text" }}
  }}
}}"#,
        bg = to_hex(palette.bg),
        panel = to_hex(palette.panel),
        element = to_hex(element),
        text = to_hex(palette.text),
        muted = to_hex(palette.muted),
        primary = to_hex(palette.primary),
        secondary = to_hex(palette.secondary),
        accent = to_hex(palette.accent),
        error = to_hex(palette.error),
        warning = to_hex(warning),
        success = to_hex(success),
        info = to_hex(info),
        border = to_hex(border),
        border_active = to_hex(border_active),
        border_subtle = to_hex(border_subtle),
        diff_added_bg = to_hex(diff_added_bg),
        diff_removed_bg = to_hex(diff_removed_bg),
    )
}

#[derive(Debug, Parser, Clone, Default)]
pub struct AiConfigCommand {}

impl AiConfigCommand {
    pub fn run(&self) -> anyhow::Result<()> {
        tui::run().context("ai config tui")
    }
}

#[cfg(test)]
mod tests {
    use super::opencode_theme_json;

    #[test]
    fn opencode_theme_json_is_valid_json() {
        let json = opencode_theme_json();
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("opencode theme json should parse");

        assert_eq!(parsed["$schema"], "https://opencode.ai/theme.json");
        assert!(parsed["defs"]["bg"].is_string());
        assert!(parsed["theme"]["background"]["dark"].is_string());
    }
}
