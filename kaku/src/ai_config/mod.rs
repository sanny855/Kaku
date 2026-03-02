pub const OPENCODE_THEME_DARK_JSON: &str = r##"{
  "$schema": "https://opencode.ai/theme.json",
  "defs": {
    "bg": "#15141b",
    "panel": "#1f1d28",
    "element": "#29263c",
    "text": "#edecee",
    "muted": "#6d6d6d",
    "primary": "#a277ff",
    "secondary": "#61ffca",
    "accent": "#ffca85",
    "error": "#ff6767",
    "warning": "#ffca85",
    "success": "#61ffca",
    "info": "#5fa8ff",
    "border": "#29263c",
    "border_active": "#3d3a52",
    "border_subtle": "#1f1d28"
  },
  "theme": {
    "primary": { "dark": "primary", "light": "primary" },
    "secondary": { "dark": "secondary", "light": "secondary" },
    "accent": { "dark": "accent", "light": "accent" },
    "error": { "dark": "error", "light": "error" },
    "warning": { "dark": "warning", "light": "warning" },
    "success": { "dark": "success", "light": "success" },
    "info": { "dark": "info", "light": "info" },
    "text": { "dark": "text", "light": "text" },
    "textMuted": { "dark": "muted", "light": "muted" },
    "background": { "dark": "bg", "light": "bg" },
    "backgroundPanel": { "dark": "panel", "light": "panel" },
    "backgroundElement": { "dark": "element", "light": "element" },
    "border": { "dark": "border", "light": "border" },
    "borderActive": { "dark": "border_active", "light": "border_active" },
    "borderSubtle": { "dark": "border_subtle", "light": "border_subtle" },
    "diffAdded": { "dark": "success", "light": "success" },
    "diffRemoved": { "dark": "error", "light": "error" },
    "diffContext": { "dark": "muted", "light": "muted" },
    "diffHunkHeader": { "dark": "primary", "light": "primary" },
    "diffHighlightAdded": { "dark": "success", "light": "success" },
    "diffHighlightRemoved": { "dark": "error", "light": "error" },
    "diffAddedBg": { "dark": "#1b2a24", "light": "#1b2a24" },
    "diffRemovedBg": { "dark": "#2a1b20", "light": "#2a1b20" },
    "diffContextBg": { "dark": "bg", "light": "bg" },
    "diffLineNumber": { "dark": "muted", "light": "muted" },
    "diffAddedLineNumberBg": { "dark": "#1b2a24", "light": "#1b2a24" },
    "diffRemovedLineNumberBg": { "dark": "#2a1b20", "light": "#2a1b20" },
    "markdownText": { "dark": "text", "light": "text" },
    "markdownHeading": { "dark": "primary", "light": "primary" },
    "markdownLink": { "dark": "info", "light": "info" },
    "markdownLinkText": { "dark": "primary", "light": "primary" },
    "markdownCode": { "dark": "accent", "light": "accent" },
    "markdownBlockQuote": { "dark": "muted", "light": "muted" },
    "markdownEmph": { "dark": "accent", "light": "accent" },
    "markdownStrong": { "dark": "secondary", "light": "secondary" },
    "markdownHorizontalRule": { "dark": "muted", "light": "muted" },
    "markdownListItem": { "dark": "primary", "light": "primary" },
    "markdownListEnumeration": { "dark": "accent", "light": "accent" },
    "markdownImage": { "dark": "info", "light": "info" },
    "markdownImageText": { "dark": "primary", "light": "primary" },
    "markdownCodeBlock": { "dark": "text", "light": "text" },
    "syntaxComment": { "dark": "muted", "light": "muted" },
    "syntaxKeyword": { "dark": "primary", "light": "primary" },
    "syntaxFunction": { "dark": "secondary", "light": "secondary" },
    "syntaxVariable": { "dark": "text", "light": "text" },
    "syntaxString": { "dark": "success", "light": "success" },
    "syntaxNumber": { "dark": "accent", "light": "accent" },
    "syntaxType": { "dark": "info", "light": "info" },
    "syntaxOperator": { "dark": "primary", "light": "primary" },
    "syntaxPunctuation": { "dark": "text", "light": "text" }
  }
}
"##;

pub const OPENCODE_THEME_LIGHT_JSON: &str = r##"{
  "$schema": "https://opencode.ai/theme.json",
  "defs": {
    "bg": "#FFFCF0",
    "panel": "#F2F0E5",
    "element": "#E5DFC5",
    "text": "#403E3C",
    "muted": "#6F6E69",
    "primary": "#5E3DB3",
    "secondary": "#24837B",
    "accent": "#8C6D00",
    "error": "#AF3029",
    "warning": "#8C6D00",
    "success": "#24837B",
    "info": "#205EA6",
    "border": "#D5D3C7",
    "border_active": "#B8B7AD",
    "border_subtle": "#E8E6DB"
  },
  "theme": {
    "primary": { "dark": "#5E3DB3", "light": "#5E3DB3" },
    "secondary": { "dark": "#24837B", "light": "#24837B" },
    "accent": { "dark": "#8C6D00", "light": "#8C6D00" },
    "error": { "dark": "#AF3029", "light": "#AF3029" },
    "warning": { "dark": "#8C6D00", "light": "#8C6D00" },
    "success": { "dark": "#24837B", "light": "#24837B" },
    "info": { "dark": "#205EA6", "light": "#205EA6" },
    "text": { "dark": "#403E3C", "light": "#403E3C" },
    "textMuted": { "dark": "#6F6E69", "light": "#6F6E69" },
    "background": { "dark": "#FFFCF0", "light": "#FFFCF0" },
    "backgroundPanel": { "dark": "#F2F0E5", "light": "#F2F0E5" },
    "backgroundElement": { "dark": "#E5DFC5", "light": "#E5DFC5" },
    "border": { "dark": "#D5D3C7", "light": "#D5D3C7" },
    "borderActive": { "dark": "#B8B7AD", "light": "#B8B7AD" },
    "borderSubtle": { "dark": "#E8E6DB", "light": "#E8E6DB" },
    "diffAdded": { "dark": "#24837B", "light": "#24837B" },
    "diffRemoved": { "dark": "#AF3029", "light": "#AF3029" },
    "diffContext": { "dark": "#6F6E69", "light": "#6F6E69" },
    "diffHunkHeader": { "dark": "#5E3DB3", "light": "#5E3DB3" },
    "diffHighlightAdded": { "dark": "#24837B", "light": "#24837B" },
    "diffHighlightRemoved": { "dark": "#AF3029", "light": "#AF3029" },
    "diffAddedBg": { "dark": "#E6F2E8", "light": "#E6F2E8" },
    "diffRemovedBg": { "dark": "#F5E6E6", "light": "#F5E6E6" },
    "diffContextBg": { "dark": "#FFFCF0", "light": "#FFFCF0" },
    "diffLineNumber": { "dark": "muted", "light": "muted" },
    "diffAddedLineNumberBg": { "dark": "#E6F2E8", "light": "#E6F2E8" },
    "diffRemovedLineNumberBg": { "dark": "#F5E6E6", "light": "#F5E6E6" },
    "markdownText": { "dark": "text", "light": "text" },
    "markdownHeading": { "dark": "primary", "light": "primary" },
    "markdownLink": { "dark": "info", "light": "info" },
    "markdownLinkText": { "dark": "primary", "light": "primary" },
    "markdownCode": { "dark": "accent", "light": "accent" },
    "markdownBlockQuote": { "dark": "muted", "light": "muted" },
    "markdownEmph": { "dark": "accent", "light": "accent" },
    "markdownStrong": { "dark": "secondary", "light": "secondary" },
    "markdownHorizontalRule": { "dark": "muted", "light": "muted" },
    "markdownListItem": { "dark": "primary", "light": "primary" },
    "markdownListEnumeration": { "dark": "accent", "light": "accent" },
    "markdownImage": { "dark": "info", "light": "info" },
    "markdownImageText": { "dark": "primary", "light": "primary" },
    "markdownCodeBlock": { "dark": "text", "light": "text" },
    "syntaxComment": { "dark": "muted", "light": "muted" },
    "syntaxKeyword": { "dark": "primary", "light": "primary" },
    "syntaxFunction": { "dark": "secondary", "light": "secondary" },
    "syntaxVariable": { "dark": "text", "light": "text" },
    "syntaxString": { "dark": "success", "light": "success" },
    "syntaxNumber": { "dark": "accent", "light": "accent" },
    "syntaxType": { "dark": "info", "light": "info" },
    "syntaxOperator": { "dark": "primary", "light": "primary" },
    "syntaxPunctuation": { "dark": "text", "light": "text" }
  }
}
"##;

/// Returns the appropriate OpenCode theme JSON based on user's Kaku theme setting.
pub fn opencode_theme_json() -> &'static str {
    if theme::is_light_theme() {
        OPENCODE_THEME_LIGHT_JSON
    } else {
        OPENCODE_THEME_DARK_JSON
    }
}

pub mod theme;
mod tui;

use anyhow::Context;
use clap::Parser;

#[derive(Debug, Parser, Clone, Default)]
pub struct AiConfigCommand {}

impl AiConfigCommand {
    pub fn run(&self) -> anyhow::Result<()> {
        tui::run().context("ai config tui")
    }
}
