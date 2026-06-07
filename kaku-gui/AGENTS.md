# Kaku GUI Agent Guide

`kaku-gui` owns rendering, window lifecycle, user interaction, AI chat, and the `k` helper binary.

## Scope

`kaku-gui` is responsible for:
- terminal window rendering
- input and mouse handling
- tab/pane UI behavior
- startup lifecycle and app event flow
- AI chat overlays, approval UI, inline status, syntax highlighting, and assistant transport
- the `k` helper binary under `src/bin/k.rs`

## Where to Look

- `src/termwindow/mod.rs`: core window state and action dispatch
- `src/termwindow/render/`: rendering pipeline
- `src/termwindow/mouseevent.rs`: mouse and drag behavior
- `src/termwindow/webgpu.rs`: surface resize and present mode handling
- `src/frontend.rs`: app lifecycle and macOS integration events
- `src/overlay/`: launcher and overlays
- `src/overlay/ai_chat/`: AI chat overlay, markdown rendering, syntax highlighting, and Waza integration
- `src/ai_chat_engine/`: AI chat engine, approval, and compaction flow
- `src/ai_client.rs`, `src/ai_remote.rs`, `src/ai_tools/`: provider and tool integration. Keep tool-name strings stable so persisted conversations replay correctly.
- `src/ai_tools/paths.rs`: sandbox and sensitive-path guards.
- `src/ai_tools/fs.rs`: read, list, write, patch, mkdir, and delete tools.
- `src/ai_tools/shell.rs`: exec, background execution, and polling.
- `src/ai_tools/web.rs`: search, fetch, and URL reading.
- `src/ai_tools/search.rs`: grep and symbol search.
- `src/ai_tools/project.rs`: project summaries and file tree helpers.
- `src/ai_tools/soul.rs`: memory and soul reads.
- `src/ai_tools/registry.rs`: `ToolDef`, `all_tools`, and `to_api_schema`.
- `src/cli_chat/`: command-line chat flow
- `src/thread_util.rs`: macOS worker thread lifetime helpers
- `src/tabbar.rs`: tab bar behavior

## Practical Rules

- Prefer existing helper methods over re-implementing logic inline.
- Keep rendering decisions in rendering modules, not scattered across event code.
- Be careful with drag and wheel interactions; regressions are user-visible quickly.
- Keep initial window dimension math in `TermWindow::new_window()` consistent with `get_os_border()` behavior.
- Route menu-driven update flows through `run_kaku_update_from_menu()` in `src/frontend.rs`.
- Keep `KAKU_CONFIG_CHANGED` fast path in `emit_user_var_event()` before pane ownership checks.
- Pass fullscreen state into `TabBarState::new()` and preserve fullscreen-specific title button spacing behavior.
- Keep AI overlay state changes cheap enough for live terminal interaction; avoid blocking render/input loops on provider calls.
- Preserve approval prompts and inline AI status placement when touching AI chat or shell flows.
- Wrap macOS worker thread spawns in the existing autorelease-pool helper so shutdown does not reintroduce use-after-free crashes.

## Known Pitfalls

- During window drag, terminal move/wheel events may need suppression.
- WebGPU surface reconfigure should happen only on meaningful state changes.
- Finder "open with" behavior is handled in `frontend.rs` event flow.
- Option+Click cursor movement fires only when: no mouse grab, no alt screen, click is on the same row as the cursor. It sends Left/Right arrow sequences proportional to the column delta; see `mouseevent.rs`.
- Block cursor height uses `natural_cell_height` (ignoring `line_height`) so Nvim visual selections match WezTerm proportions.
- Top border clearance for macOS integrated buttons is state-sensitive:
  - top tab bar visible -> add small gap
  - top tab bar hidden or bottom tab bar -> add larger clearance for traffic lights
- Overlay geometry must update when panes are split, resized, closed, or moved.
- Alternate-screen wheel behavior is config-controlled; do not hard-code one terminal behavior across normal and alternate screens.
- Inline `#` AI query status belongs below the buffer, not as a toast, so terminal content remains stable.
- `fast_model` and proxy config are part of the AI UX contract; verify both when changing provider selection or transport.
- Per-pane overlays (AI chat) render in place of their underlying pane and live in `pane_state`, not the tab's pane tree. They have no window handle, so their output only repaints the window through `PaneOutput` -> `mux_pane_output_event` -> `is_pane_visible` -> `window.invalidate()`. `is_pane_visible` (`termwindow/mod.rs`) MUST treat an active per-pane overlay pane as visible, and the overlay's `render()` MUST `term.flush()`, or streamed output and the loading spinner freeze until an input event forces a repaint. Symptom-to-cause: if `top` refreshes on its own but the AI chat needs a click, the break is one of these two.
- Scrollback viewport pruning: when the scrolled-back position falls out of scrollback during output, `normalize_viewport` (`termwindow/mod.rs`) snaps to the bottom (`None`). Do NOT re-clamp to `scrollback_top` -- that pins the view to the advancing top edge and reads as a jarring jump-to-top during streaming output (#448). This policy has flip-flopped before; keep it snap-to-bottom.
- Tab bar sizing and hit-testing have pure seams in `src/tabbar.rs`: `tab_width_budget` (per-tab column budget that drives truncation) and `is_tab_hover` (clickable span). The truncation and position regressions (#439/#443/#445) came from layout logic with no test seam, so when you change tab sizing, truncation, or click regions, extend their unit tests instead of only eyeballing the running app. `tab_width_budget` locks the invariant that a tab is never squeezed below 1 column.

## Cross-References

- [`mux/AGENTS.md`](../mux/AGENTS.md) - Tab and pane abstractions consumed by GUI.
- [`termwiz/AGENTS.md`](../termwiz/AGENTS.md) - TUI primitives used in overlays.
- [`config/AGENTS.md`](../config/AGENTS.md) - Config loading and reload signals.
