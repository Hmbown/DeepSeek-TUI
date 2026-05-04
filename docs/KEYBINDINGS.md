# Keybindings

This is the source-of-truth catalog of every keyboard shortcut the TUI recognizes. Bindings are grouped by **context** — the focus or modal state they fire in. A binding listed under "Composer" only takes effect when the composer is focused; one under "Transcript" only when the transcript has focus; and so on.

Bindings are not (yet) user-configurable — that's tracked as a v0.8.11 follow-up (#436, #437). This document is the contract that future config-file overrides will name into.

## Global (any context)

| Chord                | Action                                                        |
|----------------------|---------------------------------------------------------------|
| `F1` or `Ctrl-?`     | Toggle the help overlay                                       |
| `Ctrl-K`             | Open the command palette (slash-command finder)                |
| `Ctrl-C`             | Cancel current turn / dismiss modal / arm-then-confirm quit    |
| `Ctrl-D`             | Quit (only when the composer is empty)                         |
| `Tab`                | Cycle TUI mode: Plan → Agent → YOLO → Plan                     |
| `Shift-Tab`          | Cycle reasoning effort: off → high → max → off                 |
| `Ctrl-R`             | Open the resume-session picker                                 |
| `Ctrl-L`             | Refresh / clear the screen                                     |
| `Ctrl-T`             | Toggle the file-tree sidebar                                   |
| `Esc`                | Close topmost modal · cancel slash menu · dismiss toast        |

## Composer

Editing the message you're about to send.

| Chord                       | Action                                                  |
|-----------------------------|---------------------------------------------------------|
| `Enter`                     | Send the message (or run the slash command)             |
| `Alt-Enter` / `Ctrl-J`      | Insert a newline without sending                        |
| `Ctrl-U`                    | Delete to start of line                                 |
| `Ctrl-W`                    | Delete previous word                                    |
| `Ctrl-A` / `Home`           | Move to start of line                                   |
| `Ctrl-E` / `End`            | Move to end of line                                     |
| `Ctrl-←` / `Alt-←`          | Move backward one word                                  |
| `Ctrl-→` / `Alt-→`          | Move forward one word                                   |
| `Ctrl-V` / `Cmd-V`          | Paste from clipboard (also bracketed-paste auto-handled)|
| `Ctrl-Y`                    | Yank (paste) from kill buffer                           |
| `↑` / `↓`                   | Cycle composer history (when composer is empty / at top) |
| `Ctrl-P` / `Ctrl-N`         | Cycle composer history (alternative)                     |
| `Ctrl-S`                    | Reverse history search (Ctrl-S to advance, Esc to exit) |
| `Tab`                       | Slash-command / `@`-mention completion (popup-aware)    |
| `Ctrl-O`                    | Open external editor for the composer draft             |

### `@` mentions

Type `@<partial>` to open the file mention popup. `↑`/`↓` cycle the entries, `Tab` or `Enter` accepts. `Esc` hides the popup. As of v0.8.10 (#441), completions are re-ranked by mention frecency — files you mention often + recently float to the top.

### `#` quick-add (memory)

When `[memory] enabled = true`, typing `# foo` and pressing `Enter` appends `foo` as a timestamped bullet to your memory file *without* sending a turn. See `docs/MEMORY.md`.

## Transcript (when transcript has focus)

| Chord                | Action                                              |
|----------------------|-----------------------------------------------------|
| `↑` / `↓` / `j` / `k`| Scroll one line                                    |
| `PgUp` / `PgDn`      | Scroll one page                                    |
| `Home` / `g`         | Jump to top                                         |
| `End` / `G`          | Jump to bottom                                     |
| `Esc`                | Return focus to composer                           |
| `y`                  | Yank selected region to clipboard                  |
| `v`                  | Begin / extend visual selection                    |
| `o`                  | Open URL under cursor (OSC 8 capable terminals)    |

## Sidebar (when sidebar has focus)

| Chord                | Action                                              |
|----------------------|-----------------------------------------------------|
| `↑` / `↓` / `j` / `k`| Move selection                                     |
| `Enter`              | Activate the selected item (open / focus / cancel) |
| `Tab`                | Cycle to next sidebar panel (Files → Tasks → Agents → Todos) |
| `Esc`                | Return focus to composer                           |

## Slash-command palette (after `Ctrl-K` or typing `/`)

| Chord                | Action                                              |
|----------------------|-----------------------------------------------------|
| `↑` / `↓`            | Move selection                                     |
| `Enter` / `Tab`      | Run / complete the highlighted command             |
| `Esc`                | Dismiss palette                                     |

## Approval modal (when a tool requests approval)

| Chord                | Action                                              |
|----------------------|-----------------------------------------------------|
| `y` / `Y`            | Approve once                                        |
| `a` / `A`            | Approve all (auto-approve subsequent calls)        |
| `n` / `N` / `Esc`    | Deny                                                |
| `e`                  | Edit the approved input before running              |

## Onboarding (first-run flow)

| Chord                | Action                                              |
|----------------------|-----------------------------------------------------|
| `Enter`              | Advance to next step (Welcome → Language → API → …) |
| `Esc`                | Step back one screen                                |
| `1`–`5`              | Pick a language (Language step)                    |
| `y` / `Y`            | Trust the workspace (Trust step)                   |
| `n` / `N`            | Skip the trust prompt                              |

## v0.8.10 audit notes

- **No broken bindings found.** Every chord listed above resolves to a live handler in `crates/tui/src/tui/ui.rs` (key-event dispatch) or `crates/tui/src/tui/app.rs` (mode + state transitions).
- **Conflicts deduped.** `Ctrl-P` was previously double-bound (history + palette open); the palette opens via `Ctrl-K` only, leaving `Ctrl-P` for history.
- **Help overlay reconciled.** Every entry shown in the `?` help overlay corresponds to a binding in this doc; entries that were aspirational were either implemented (logged in this release) or dropped.
- **Configurable keymap (#436) and `tui.toml` (#437) are deferred to v0.8.11.** That work needs a named-binding registry that names every chord on this page and lets `~/.deepseek/keybinds.toml` override individual entries with conflict detection. Doing it well is bigger than a patch release; doing it sloppily would land a half-finished registry that future contributors have to navigate around. v0.8.10 ships with this audit + docs as the durable spec the registry will name into.
