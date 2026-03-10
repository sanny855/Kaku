# Kaku Layout Handoff (Top-Tab Jitter + Non-Fullscreen Padding)

## 1) Goal
Fix two remaining UX issues in Kaku layout behavior:

1. **Top-tab + fullscreen**: switching away to another Space/app and returning still causes a visible one-frame layout jitter.
2. **Top-tab + non-fullscreen (default padding path)**: bottom spacing still feels too large / unstable in some cases.

The user feedback is: "still jitters", and "default non-fullscreen padding still has a bit of issue".

## 2) Current User-Observed Symptoms

- Current state: "no-tab" and some fullscreen cases improved, but **top-tab fullscreen return path still jitters**.
- Also, in **non-fullscreen default padding path**, spacing is still not fully matching expectation (bottom gap perception still off).

## 3) Environment / Constraints

- Platform: macOS only.
- Repo path: `/Users/tang/www/Kaku`.
- Branch/worktree is dirty with unrelated edits. **Do not reset/revert unrelated changes.**
- Keep solution simple and maintainable; avoid large architectural rewrite.

## 4) Files Most Relevant

- Rust resize/layout:
  - `kaku-gui/src/termwindow/resize.rs`
  - `kaku-gui/src/termwindow/mod.rs`
- Lua override logic:
  - `assets/macos/Kaku.app/Contents/Resources/kaku.lua`

## 5) Important Existing Logic (Already Added)

### 5.1 Lua side
- `update_window_config(...)` has `effective_full_screen` stickiness when unfocused to avoid transient downgrade.
- `window-focus-changed` path now triggers config update pipeline (already wired).
- File area: `kaku.lua` around `update_window_config` (near lines ~344+).

### 5.2 Rust side
- Deferred relayout has epoch latest-wins dedupe.
- Top-tab focus/visibility paths prefer deferred relayout (avoid immediate apply frame).
- File: `kaku-gui/src/termwindow/mod.rs` around:
  - `focus_changed` (~897)
  - `visibility_changed` (~962)
  - `schedule_deferred_layout_relayout` (~1067)

### 5.3 Resize normalization + bottom-gap handling
- Fullscreen normalization helper:
  - `should_normalize_fullscreen_state_on_resize(...)`
- Top-tab visible non-fullscreen rebalance helper:
  - `rebalance_top_padding_for_bottom_gap(...)`
- File: `kaku-gui/src/termwindow/resize.rs` around:
  - helper section (~26+)
  - `resize()` (~64+)
  - `apply_dimensions()` non-scale branch (~390+)

## 6) Why It May Still Be Failing

Likely race/order issue remains across these chains:

1. **Focus/visibility/deferred relayout sequencing**
   - Multiple events may still produce 2 layout passes with different effective state.
2. **Transient fullscreen state with geometry drift**
   - Current normalization still depends on focus + some conditions; edge transitions may slip through.
3. **Config override reload timing**
   - Lua `set_config_overrides` + silent config reload can trigger another layout pass shortly after first paint.
4. **Row quantization slack distribution**
   - Bottom gap visual result depends on row rounding remainder; current rebalance may not cover all branches/events.

## 7) Reproduction Matrix (Must Test)

Use top-tab layout (`tab_bar_at_bottom = false`) and default managed padding path (no custom `config.window_padding` assignment).

1. Non-fullscreen, single tab (tab hidden): observe bottom gap baseline.
2. Non-fullscreen, multiple tabs (tab visible): compare bottom gap against case #1.
3. Fullscreen, multiple tabs: switch to another Space/app then return.
4. Fullscreen -> non-fullscreen -> fullscreen transitions with focus changes.

Expected:
- No one-frame jump/jitter during switch-back.
- Non-fullscreen visible-tab bottom gap should not look larger than the no-tab baseline.

## 8) Required Debug Capture

Enable both Rust/Lua layout logs:

```bash
KAKU_LAYOUT_DEBUG=1 make dev
```

Capture and inspect lines containing:

- `resize:normalize_fullscreen`
- `apply_dimensions:begin`
- `apply_dimensions:end`
- `apply_dimensions:rebalance_top_tab_bottom_gap`
- `focus_changed:`
- `visibility_changed:`
- `deferred_layout_relayout:`
- `lua:update_window_config`

The key is to identify if two adjacent passes use different values for:
- fullscreen state
- show_tab_bar
- padding(top,bottom)
- rows

## 9) Acceptance Criteria

1. User cannot reproduce fullscreen switch-back jitter in top-tab mode.
2. Non-fullscreen top-tab bottom spacing is visually stable and not excessively larger when tabs are visible.
3. Custom user `window_padding` semantics remain respected.
4. Tests pass:

```bash
cargo +nightly fmt --all
make check
make test
```

## 10) Suggested Strategy (Recommended)

Implement minimal, behavior-focused stabilization instead of broad refactor:

1. Introduce a short-lived **layout-sticky fullscreen effective state** in Rust (top-tab only), driven by focus/visibility transitions, so all layout decisions in that transition frame consume one consistent state.
2. Ensure `show_tab_bar` + padding computation read that same effective state in one pass.
3. Keep row-quantization slack policy deterministic: explicitly cap bottom visual gap for top-tab non-fullscreen visible-tab path.
4. Avoid duplicating policy between Lua and Rust; Lua controls overrides, Rust computes geometry.

## 11) Ready-to-Copy Prompt for Another AI

Use this directly:

"You are fixing Kaku macOS layout behavior in `/Users/tang/www/Kaku`.
Focus only on top-tab jitter and non-fullscreen bottom padding stability.
Current symptoms: fullscreen switch-away/return still jitters; non-fullscreen default path bottom gap still feels off.
Read `TOP_TAB_LAYOUT_JITTER_HANDOFF_2026-03-10.md`, then inspect:
- `kaku-gui/src/termwindow/resize.rs`
- `kaku-gui/src/termwindow/mod.rs`
- `assets/macos/Kaku.app/Contents/Resources/kaku.lua`
Use `KAKU_LAYOUT_DEBUG=1 make dev` logs to validate event order and double-layout passes.
Implement minimal maintainable fixes (no large refactor), preserve custom user padding semantics, and run:
`cargo +nightly fmt --all && make check && make test`.
Return concrete diff summary + why jitter is eliminated."

