---
version: alpha
name: Kaku
description: >
  Terminal emulator for macOS. Dark-first, monospace-led design system
  built around a neon-green accent on near-black surfaces. No light mode in v1.

colors:
  bg:            "#0a0a0a"
  bg-elevated:   "#050505"
  bg-card:       "#111111"
  border:        "#1f1f1f"
  border-strong: "#2a2a2a"
  primary:       "#ffffff"
  secondary:     "#888888"
  muted:         "#666666"
  accent:        "#00ff9f"
  accent-dim:    "#00b870"
  error:         "#ff5c5c"
  warning:       "#ffd56b"

typography:
  display:
    fontFamily:    JetBrains Mono
    fontSize:      3rem
    fontWeight:    700
    lineHeight:    1.1
    letterSpacing: -0.02em
  display-mobile:
    fontFamily:    JetBrains Mono
    fontSize:      2rem
    fontWeight:    700
    lineHeight:    1.1
    letterSpacing: -0.02em
  body-md:
    fontFamily: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize:   1rem
    fontWeight: 400
    lineHeight: 1.6
  body-sm:
    fontFamily: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize:   0.875rem
    fontWeight: 400
    lineHeight: 1.5
  code:
    fontFamily: JetBrains Mono, ui-monospace, "SF Mono", Menlo, Consolas, monospace
    fontSize:   0.8125rem
    fontWeight: 400
    lineHeight: 1.6
  label:
    fontFamily:    JetBrains Mono, ui-monospace, "SF Mono", Menlo, Consolas, monospace
    fontSize:      0.6875rem
    fontWeight:    400
    lineHeight:    1
    letterSpacing: 0.04em

rounded:
  sm: 3px
  md: 6px
  lg: 10px

spacing:
  1:  4px
  2:  8px
  3:  12px
  4:  16px
  6:  24px
  8:  32px
  12: 48px
  16: 64px

components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor:       "{colors.bg}"
    rounded:         "{rounded.md}"
    padding:         12px 20px
    typography:      code
  button-primary-hover:
    backgroundColor: "{colors.accent-dim}"
  button-ghost:
    backgroundColor: transparent
    textColor:       "{colors.primary}"
    rounded:         "{rounded.md}"
    padding:         11px 20px
  button-ghost-hover:
    textColor: "{colors.accent}"
  nav:
    backgroundColor: "{colors.bg-elevated}"
    textColor:       "{colors.secondary}"
    height:          52px
    padding:         0 24px
  card:
    backgroundColor: "{colors.bg-card}"
    rounded:         "{rounded.lg}"
    padding:         24px
  card-hover:
    backgroundColor: "#161616"
  divider:
    backgroundColor: "{colors.border}"
    height:          1px
  divider-strong:
    backgroundColor: "{colors.border-strong}"
    height:          1px
  hint:
    textColor: "{colors.muted}"
    typography: label
  alert-error:
    backgroundColor: "rgba(255,92,92,0.08)"
    textColor:       "{colors.error}"
    rounded:         "{rounded.md}"
    padding:         12px 16px
  alert-warning:
    backgroundColor: "rgba(255,213,107,0.08)"
    textColor:       "{colors.warning}"
    rounded:         "{rounded.md}"
    padding:         12px 16px
  terminal:
    backgroundColor: "#000000"
    textColor:       "{colors.accent}"
    rounded:         "{rounded.md}"
    padding:         16px
  badge:
    backgroundColor: "rgba(0,255,159,0.08)"
    textColor:       "{colors.accent}"
    rounded:         "{rounded.sm}"
    padding:         3px 8px
  focus-ring:
    textColor: "{colors.accent}"
    rounded:   "{rounded.sm}"
---

## Overview

Kaku is a GPU-accelerated terminal emulator for macOS with built-in AI. The visual
language is **raw terminal hacker**: pure-black surfaces, JetBrains Mono as the
primary display typeface, and a single neon-green accent (`#00ff9f`) that connects
the brand to the terminal output it renders. Every surface is dark; v1 ships no
light mode.

The tone is terse and technical. Copy is written for developers who already know
what a terminal is. Decorative elements are removed in favour of motion that
demonstrates the product (animated terminal replays, tab switching, copy feedback).

The one thing this design leaves in memory: **green text on black**, the universal
signal for "this is a terminal" — used here at brand scale.

## Colors

The palette is deliberately minimal. One background family, one accent, two
semantic states.

**Background family** — three steps of near-black create surface hierarchy without
any light:

| Token | Value | Role |
|-------|-------|------|
| `bg` | `#0a0a0a` | Page background |
| `bg-elevated` | `#050505` | Nav, elevated chrome |
| `bg-card` | `#111111` | Cards, panels, inset blocks |

`bg-elevated` is darker than `bg` — the nav sits below the perceived horizon,
reinforcing the full-bleed terminal feel. Do not reverse this; a lighter nav
breaks the depth illusion.

**Borders** — two steps, never more:

| Token | Value | Role |
|-------|-------|------|
| `border` | `#1f1f1f` | Default dividers, card outlines |
| `border-strong` | `#2a2a2a` | Focused, hovered, or active states |

**Text** — three weights of white:

| Token | Value | Role |
|-------|-------|------|
| `primary` | `#ffffff` | Headings, primary copy |
| `secondary` | `#888888` | Nav links, metadata, labels |
| `muted` | `#666666` | Hints, placeholders, de-emphasised copy |

**Accent** — the single colour commitment. Use it for interactive elements,
highlights, and the terminal cursor. Never use it decoratively; it must always
signal "actionable" or "output from the terminal".

| Token | Value | Role |
|-------|-------|------|
| `accent` | `#00ff9f` | CTAs, links, focus rings, terminal output |
| `accent-dim` | `#00b870` | Hover state of accent elements |

**Semantic** — error and warning only. Both are warm to contrast against the cool
black base.

| Token | Value | Role |
|-------|-------|------|
| `error` | `#ff5c5c` | Destructive actions, error states |
| `warning` | `#ffd56b` | Non-blocking warnings |

Do not introduce additional colours. If a new state requires a colour, use opacity
on an existing token (e.g. `rgba(0,255,159,0.08)` for the badge background).

## Typography

Two families, one job each.

**JetBrains Mono** is the display font. All headings and hero copy use it. The
choice is intentional: Kaku renders monospace text; the marketing site should
feel continuous with what the app actually produces.

**System UI** (`-apple-system`, `BlinkMacSystemFont`, `Segoe UI`) handles all
body prose. Switching to monospace for long-form copy would hurt readability.
System UI keeps the reading experience fast and native.

Scale in use:

| Token | Size | Family | Weight | Use |
|-------|------|--------|--------|-----|
| `display` | 3rem / 48px | JetBrains Mono | 700 | H1, hero headline |
| `display-mobile` | 2rem / 32px | JetBrains Mono | 700 | H1 at ≤ 720px |
| `body-md` | 1rem / 16px | System UI | 400 | Section intros, card descriptions |
| `body-sm` | 0.875rem / 14px | System UI | 400 | Feature copy, FAQ answers |
| `code` | 0.8125rem / 13px | JetBrains Mono | 400 | Inline code, CTA labels, nav links |
| `label` | 0.6875rem / 11px | JetBrains Mono | 400 | Badges, stat labels |

Letter-spacing on `display`: `-0.02em`. Tight tracking on large monospace type
prevents the letterforms from reading as too mechanical at heading scale.

## Layout

Two container widths:

| Name | Max-width | Use |
|------|-----------|-----|
| Narrow (`kk-container`) | 720px | Text-heavy sections: hero prose, FAQ, QuickStart |
| Wide (`kk-container-wide`) | 1024px | Grid sections: FeatureGrid, ScreenshotGallery, Download methods |

All containers: `margin: 0 auto`, `padding: 0 24px`.

**Section rhythm**: sections are separated by `48px` vertical padding. Within a
section, the heading-to-content gap is `24px`. Within a card grid, gap is `16px`.

**Grid defaults**: FeatureGrid and Download methods use a 3-column grid on desktop
(`min-width: 721px`), collapsing to 2-column at ≤ 720px, 1-column at ≤ 480px.
ScreenshotGallery uses 2-column, collapsing to 1-column at ≤ 720px.

The nav is sticky at `top: 0`, `height: 52px`, with a `border-bottom: 1px solid {border}`.
It does not cast a shadow — shadow would imply a light source inconsistent with
the near-black surfaces below.

## Elevation & Depth

Kaku uses **background-step depth** exclusively. No box shadows on page surfaces.
Depth is created by making nested surfaces incrementally lighter (`bg` → `bg-card`).

The terminal component (`background: #000000`) is the one surface that inverts
this logic — it drops below the page background to signal "this is the actual
terminal". This is intentional.

Cards use a `border: 1px solid {border}` to define their boundary. On hover,
`border-color` advances to `{border-strong}` and `background` steps to `#161616`.
No `box-shadow` is added on hover.

Focus rings use `outline: 2px solid {accent}` at `outline-offset: 2px`. This is
the only place where accent appears as a border treatment.

## Shapes

Radius is small and consistent with a developer tool aesthetic. Pill buttons and
large radius cards would conflict with the terminal theme.

| Token | Value | Use |
|-------|-------|-----|
| `sm` | 3px | Badges, focus rings, inline code |
| `md` | 6px | Buttons, input fields |
| `lg` | 10px | Cards, modals, terminal windows |

Do not mix radius values within a single component family. All buttons use `md`.
All cards use `lg`. Never use `border-radius: 50%` except for the avatar in
the WhyKaku section.

## Components

### Button — Primary

Background: `{colors.accent}` / Text: `{colors.bg}` / Radius: `{rounded.md}`

Padding `12px 20px`. Typography: `code` (JetBrains Mono 13px). On hover,
background transitions to `{colors.accent-dim}`. On press: `scale(0.97)`.
The text is always black-on-green — do not invert or make the background
transparent.

### Button — Ghost

Background: `transparent` / Border: `1px solid {colors.border-strong}` /
Text: `{colors.primary}` / Radius: `{rounded.md}`

Padding `11px 20px` (one pixel less vertical than primary to account for the
border). On hover, `text-color` steps to `{colors.accent}`. Used for secondary
CTAs (e.g. "Read Docs", "View Changelog").

### Nav

Height `52px`, background `{colors.bg-elevated}`, bottom border `{colors.border}`.
Brand uses `font-weight: 700`, `font-size: 16px`, system sans. Nav links use
`code` typography in `{colors.secondary}`, transitioning to `{colors.primary}`
on hover. Language switcher and GitHub link sit at the right end.

### Card

Background `{colors.bg-card}`, radius `{rounded.lg}`, padding `24px`, border
`1px solid {colors.border}`. Cards are used in FeatureGrid and WhyKaku. They
do not scroll or clip overflow — content is always fully visible.

### Terminal

Background `#000000` (not `{colors.bg}`), radius `{rounded.md}`, padding `16px`.
Text uses `code` typography in `{colors.accent}`. The "bar" (traffic-light row)
uses three dots in `#333333`. Cursor is a blinking `_` in `{colors.accent}`.

### Badge / Tag

Background `rgba(0,255,159,0.08)`, text `{colors.accent}`, radius `{rounded.sm}`,
padding `3px 8px`. Label typography. Used to label section types ("AI Feature",
version tags). Never use a solid accent background for a badge — it competes
with the primary CTA.

## Do's and Don'ts

**Do:**
- Use `{colors.accent}` exclusively for actionable and terminal-output elements.
- Use JetBrains Mono for all headings and short labels.
- Separate surface depth with background-color steps, not shadows.
- Apply `active:scale(0.97)` to all buttons and interactive cards.
- Respect `prefers-reduced-motion`: disable CSS animations, keep layout transitions.
- Maintain `color-scheme: dark` even if a user requests light mode (v1 contract).

**Don't:**
- Don't add a second accent colour. If you need a variant, use `accent-dim` or opacity.
- Don't apply `box-shadow` to cards or nav — this breaks the background-step depth model.
- Don't use Inter, Roboto, or any geometric sans for headings — the terminal identity lives in JetBrains Mono.
- Don't add light-mode rules. Doing so before v2 ships will cause inconsistency.
- Don't use `border-radius > 10px` on any surface. Larger radii feel inconsistent with a terminal tool.
- Don't use `color.accent` as a section background fill — it reads as highlighted text or a bug.
- Don't introduce a third container width. Use narrow for text, wide for grids.
