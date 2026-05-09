# Remove Footer Water-Spout Animation

**Date**: 2026-05-09
**Status**: done (uncommitted)

## Summary

Removed the animated Unicode block-character wave ("water-spout strip") from the
footer spacer. The footer now renders a plain whitespace gap between the left
status line and the right-hand chip cluster at all times. The low-key
"working…" dot-pulse text label on the left side is retained as the sole
in-flight status indicator.

## Motivation

- The wave animation was visually distracting during AI turns.
- It never achieved the natural feel of a MacBook Pro breathing light despite
  several tuning rounds (breathing envelope, 20 fps, per-strip colour pulse).
- Simplifying the footer to text-only status reduces visual noise and aligns
  with a calmer UI.

## Changes

### `crates/tui/src/tui/widgets/footer.rs`

- Removed `FooterProps::working_strip_frame` field.
- Removed `WAVE_GLYPHS` constant (8 Unicode block-height glyphs `▁`-`█`).
- Removed `footer_working_strip_glyph_at()` (sine-wave + breathing-envelope math).
- Removed `footer_working_strip_string()` (per-frame strip builder).
- Simplified `FooterWidget::render()` spacer logic to `Span::raw(" ".repeat(…))`.
- Removed `working_strip_frame: None` from `FooterProps::from_app()`.
- Removed 5 strip-related tests.

### `crates/tui/src/tui/ui.rs`

- Removed the `if !app.low_motion { props.working_strip_frame = … }` block from
  `render_footer()`.
- Updated `UI_STATUS_ANIMATION_MS` doc comment to remove the water-spout
  reference; the constant now only documents the per-tool spinner pulse.

### Retained

- `footer_working_label()` — ASCII dot-pulse ("working" → "working…") as the
  sole low-key in-flight signal.
- `footer_working_strip_active()` — still drives the text-label visibility.
  Renamed in a future cleanup pass to `footer_working_label_active`.

## Verification

Compilation not verified in this environment (Rust 1.85.1 < project minimum
1.88). Run after upgrading:

```
cargo test -p deepseek-tui -- footer
```
