# DeepSeek Palette

DeepSeek CLI uses a shared palette so the TUI and CLI output stay on-brand.
The source of truth is `src/palette.rs`.

## Brand Colors

- DeepSeek Blue `#3578E5` (primary accent, headers, key labels)
- DeepSeek Sky `#6AAEF2` (secondary accent, hints, focus)
- DeepSeek Aqua `#36BBD4` (success/active state)
- DeepSeek Navy `#183F8A` (mode badges, deep accent)
- DeepSeek Ink `#0B1526` (dark background surfaces)
- DeepSeek Slate `#121C2E` (composer background)
- DeepSeek Red `#E25060` (errors)

## Semantic Tokens

- `TEXT_PRIMARY`, `TEXT_MUTED`, `TEXT_DIM`
- `STATUS_SUCCESS`, `STATUS_WARNING`, `STATUS_ERROR`, `STATUS_INFO`
- `SELECTION_BG`, `COMPOSER_BG`

## Usage

- Prefer `crate::palette::*` constants instead of hardcoded colors.
- For CLI (non-TUI) output, use the `*_RGB` constants with `colored::Colorize::truecolor`.
