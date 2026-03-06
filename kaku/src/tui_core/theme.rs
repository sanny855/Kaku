use config::{configuration, ConfigHandle};
use ratatui::style::Color;
use std::sync::Mutex;
use wezterm_term::color::{ColorPalette, SrgbaTuple};

#[derive(Clone, Copy)]
pub struct ThemePalette {
    pub primary: SrgbaTuple,
    pub secondary: SrgbaTuple,
    pub accent: SrgbaTuple,
    pub error: SrgbaTuple,
    pub info: SrgbaTuple,
    pub text: SrgbaTuple,
    pub muted: SrgbaTuple,
    pub bg: SrgbaTuple,
    pub panel: SrgbaTuple,
    pub is_light: bool,
}

#[derive(Clone, Copy)]
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

static THEME_CACHE: Mutex<Option<(usize, Theme)>> = Mutex::new(None);

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

fn luminance(color: SrgbaTuple) -> f32 {
    0.299 * color.0 + 0.587 * color.1 + 0.114 * color.2
}

fn is_light_color(color: SrgbaTuple) -> bool {
    luminance(color) > 0.5
}

fn color_distance(a: SrgbaTuple, b: SrgbaTuple) -> f32 {
    let dr = a.0 - b.0;
    let dg = a.1 - b.1;
    let db = a.2 - b.2;
    (dr * dr + dg * dg + db * db).sqrt()
}

fn has_enough_separation(bg: SrgbaTuple, color: SrgbaTuple) -> bool {
    color_distance(bg, color) >= 0.18 || (luminance(bg) - luminance(color)).abs() >= 0.12
}

fn pick_visible(bg: SrgbaTuple, candidates: &[SrgbaTuple]) -> SrgbaTuple {
    for candidate in candidates {
        let candidate = opaque(*candidate);
        if has_enough_separation(bg, candidate) {
            return candidate;
        }
    }

    let fallback = opaque(candidates[0]);
    blend(bg, fallback, if is_light_color(bg) { 0.55 } else { 0.45 })
}

fn pick_muted(bg: SrgbaTuple, text: SrgbaTuple, candidate: SrgbaTuple) -> SrgbaTuple {
    let candidate = opaque(candidate);
    if has_enough_separation(bg, candidate) {
        candidate
    } else {
        blend(bg, text, if is_light_color(bg) { 0.42 } else { 0.5 })
    }
}

fn to_color(color: SrgbaTuple) -> Color {
    let (r, g, b, _) = color.to_srgb_u8();
    Color::Rgb(r, g, b)
}

fn palette_from_config(config: &ConfigHandle) -> ThemePalette {
    let palette: ColorPalette = config.resolved_palette.clone().into();
    let bg = opaque(palette.background);
    let text = opaque(palette.foreground);
    let is_light = is_light_color(bg);

    let primary = pick_visible(
        bg,
        &[palette.colors.0[13], palette.colors.0[5], palette.cursor_bg],
    );
    let secondary = pick_visible(
        bg,
        &[
            palette.colors.0[10],
            palette.colors.0[6],
            palette.colors.0[2],
        ],
    );
    let accent = pick_visible(
        bg,
        &[
            palette.colors.0[11],
            palette.colors.0[3],
            palette.cursor_border,
        ],
    );
    let error = pick_visible(
        bg,
        &[
            palette.colors.0[9],
            palette.colors.0[1],
            palette.cursor_border,
        ],
    );
    let info = pick_visible(
        bg,
        &[palette.colors.0[12], palette.colors.0[4], palette.cursor_bg],
    );
    let muted = pick_muted(bg, text, palette.colors.0[8]);
    let panel = blend(bg, text, if is_light { 0.05 } else { 0.08 });

    ThemePalette {
        primary,
        secondary,
        accent,
        error,
        info,
        text,
        muted,
        bg,
        panel,
        is_light,
    }
}

fn theme_from_config(config: &ConfigHandle) -> Theme {
    let palette = palette_from_config(config);

    Theme {
        primary: to_color(palette.primary),
        secondary: to_color(palette.secondary),
        accent: to_color(palette.accent),
        error: to_color(palette.error),
        text: to_color(palette.text),
        muted: to_color(palette.muted),
        bg: to_color(palette.bg),
        panel: to_color(palette.panel),
    }
}

fn current_theme() -> Theme {
    let config = configuration();
    let generation = config.generation();

    let mut cached = THEME_CACHE.lock().unwrap();
    if let Some((cached_generation, theme)) = *cached {
        if cached_generation == generation {
            return theme;
        }
    }

    let theme = theme_from_config(&config);
    *cached = Some((generation, theme));
    theme
}

pub fn current_theme_palette() -> ThemePalette {
    let config = configuration();
    palette_from_config(&config)
}

/// Clear the cached theme detection result so that the next call re-reads
/// the resolved palette from the current configuration.
pub fn clear_theme_cache() {
    *THEME_CACHE.lock().unwrap() = None;
}

pub fn purple() -> Color {
    current_theme().primary
}

pub fn green() -> Color {
    current_theme().secondary
}

pub fn yellow() -> Color {
    current_theme().accent
}

pub fn red() -> Color {
    current_theme().error
}

pub fn text_fg() -> Color {
    current_theme().text
}

pub fn muted() -> Color {
    current_theme().muted
}

pub fn bg() -> Color {
    current_theme().bg
}

pub fn panel() -> Color {
    current_theme().panel
}
