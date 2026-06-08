# Config Agent Guide

The `config` crate owns config loading, Lua integration, schema behavior, proxy settings, versioned defaults, and AI configuration.

## Scope

`config` controls:
- loading user and bundled configs
- Lua execution and binding lifecycle
- schema mapping between Lua and Rust
- config subscriptions and reload behavior
- AI model, fast-model, and proxy settings consumed by GUI and CLI flows
- versioned default config and release readiness checks

## Where to Look

- `config/src/config.rs`: load/parse flow
- `config/src/lib.rs`: config API and subscriptions
- `config/src/proxy.rs`: proxy configuration used by AI and network-facing flows
- `config/src/version.rs`: config version and migration-related constants
- `assets/macos/Kaku.app/Contents/Resources/kaku.lua`: bundled fallback config

## Config TUI (`kaku/src/config_tui/` and `kaku/src/tui_core/`)

The config TUI is the interactive terminal UI for editing Kaku config. It lives in the `kaku` CLI crate, not the `config` crate.

- `kaku/src/config_tui/` - TUI app: `app.rs` (main loop), `state.rs` (form state), `ui.rs` (rendering)
- `kaku/src/tui_core/` - Reusable TUI primitives: `form.rs`, `theme.rs`, `components/` (text_input, select_box, toggle, list_editor)
- The split between `config_tui` (domain-specific) and `tui_core` (generic components) is intentional; do not merge them back.
- `tui_core` is designed to be reused by `ai_config/tui/` and any future TUI flows.
- Debounce logic lives in `tui_core` and should not be duplicated in `config_tui`.

## AI Config TUI (`kaku/src/ai_config/`)

The AI config TUI lives in the `kaku` CLI crate and shares terminal UI primitives with `config_tui`.

- Keep `fast_model`, primary model, and proxy settings aligned between CLI config, GUI AI chat, and documentation.
- Do not duplicate form widgets or debounce behavior outside `kaku/src/tui_core/`.
- Verify AI config changes against both config parsing and the visible TUI flow.

## Practical Rules

- Loading priority: user config first, bundled config second.
- Keep reload-safe behavior for startup hooks and subscriptions.
- Avoid introducing config paths that bypass existing precedence rules.
- Do not reintroduce `KAKU_CONFIG_FILE`; config path override was intentionally removed.
- Keep bundled fallback config authoritative at `assets/macos/Kaku.app/Contents/Resources/kaku.lua`.
- Preserve compatibility with runtime reload callers that trigger `config::reload()` from GUI-side signals.
- Treat `config_version` 23 as the current release baseline. Any version bump must update bundled config, release checks, docs, and migration expectations together.
- New config fields are user-facing behavior. Keep them out of pure cleanup/refactor patches unless the maintainer explicitly approved the product change, and update bundled defaults plus documentation in the same change when they do land.
- Keep alternate-screen wheel scroll behavior configurable; terminal and GUI defaults must not diverge.

## Cross-References

- [`kaku-gui/AGENTS.md`](../kaku-gui/AGENTS.md) - GUI config consumers and reload signals.
- [`lua-api-crates/AGENTS.md`](../lua-api-crates/AGENTS.md) - Lua APIs that expose config values.
- [`mux/AGENTS.md`](../mux/AGENTS.md) - Alert propagation for config changes.
