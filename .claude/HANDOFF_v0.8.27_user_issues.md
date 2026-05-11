# v0.8.27 — User-Issue Strategy Handoff

**Audience:** the AI agent picking up post-v0.8.26 user-bug work.
**Scope:** the issues filed by users in the 24–48 hours after v0.8.26
shipped, plus older issues with concrete fix shapes that didn't make
v0.8.26.
**This is layered on top of the in-flight v0.8.27 cycle** — there are
already 16 community-PR commits on `work/v0.8.27`. Don't start over;
add to it.

---

## Where you are

- **Working tree:** `/Volumes/VIXinSSD/whalebro/deepseek-tui`
- **Active branch:** `work/v0.8.27` (off main at v0.8.26 tip)
- **Already on the branch:** 16 commits — community PRs (#1316, #1317,
  #1181, #1203, #1140, #1247, #1223, #1185, #1220, #1233, #1235,
  #1197, plus a trackpad scroll fix and a card-rail UI tweak)
- **Reference docs:** `.claude/HANDOFF_v0.8.26_security.md` for the
  release flow steps 7–11 (same shape applies for v0.8.27)
- **Issue board:** GitHub `Hmbown/DeepSeek-TUI`

The previous agent only did community-PR cherry-picks. The strategic
bug-fix work in this document is **not started**. Assume zero
overlap.

---

## Hard rules

1. **STOP and ask Hunter** before merging the v0.8.27 PR, tagging,
   or publishing to crates.io / npm / Homebrew.
2. No `--no-verify`, no `--no-gpg-sign`, no force push.
3. Don't leak `.private/` content into PRs / CHANGELOG / release notes.
4. v0.8.27 is **NOT** a security release. If a new GHSA arrives mid-
   cycle, branch v0.8.28 — don't bundle.
5. Time-box thorny items at 30 min. Defer to v0.8.28 instead of
   sinking the cycle.

---

## P0 — ship these, they fix real user pain

### 1. Cross-terminal flicker (#1119, #1352, #1356, #1363, #1366, #1260, #1295)

**The most-reported bug since v0.8.26 shipped.** Five Ghostty / VSCode-
terminal reports in 24 hours plus the existing Windows ones. **Same
root cause, single fix.**

**Diagnosis.** v0.8.22 added a viewport-reset escape sequence to fix
viewport drift after focus/resize. The sequence is:

```
\x1b[r       set scroll region to entire screen
\x1b[?6l     reset DECOM origin mode
\x1b[H       cursor home
\x1b[2J      erase entire screen
\x1b[3J      erase saved lines
```

This fires on every redraw. `\x1b[2J\x1b[3J` is destructive — full
clear. Terminals that don't optimize differential redraws (Ghostty,
VSCode terminal in some configurations, Win10 conhost) blank-then-
repaint every frame, producing visible flicker.

The smoking-gun datapoint is **#1356**: "doesn't flicker on M4 Air,
doesn't flicker for Claude Code / Codex / Gemini CLIs in the same
VSCode terminal." Other CLIs use the alt-screen buffer's natural
double-buffering and don't emit a destructive reset every frame.

**Strategy — pick #1; #2 is the fallback.**

#### 1.A — Replace destructive reset with lighter sequence (~30 min)

In `crates/tui/src/tui/ui.rs` (search for the const that holds the
reset sequence — likely named `VIEWPORT_RESET` or similar, check the
v0.8.22 / v0.8.24 commits that mention `recover_terminal_modes` or
`FocusGained`):

```rust
// before
const VIEWPORT_RESET: &str = "\x1b[r\x1b[?6l\x1b[H\x1b[2J\x1b[3J";

// after — drop the destructive 2J/3J; alt-screen buffer's existing
// double-buffering handles the redraw without screen blanking.
const VIEWPORT_RESET: &str = "\x1b[r\x1b[?6l\x1b[H";
```

Add a regression test that asserts the constant doesn't contain
`2J` or `3J` (the destructive parts). The viewport-drift fix that
the original sequence was added to address came from #1041 / similar
— verify it's still working with the lighter sequence by manual
smoke on macOS Terminal.app.

#### 1.B — Audit redraw-rate and only emit on actual drift (~1 day)

If 1.A reintroduces drift, fall back to this: track previous viewport
state and only emit the reset when drift is detected (post-resize,
post-focus-gain, post-pager-close). Search for where the constant is
emitted; if it's in the per-frame draw path, that's the bug.

#### 1.C — Per-terminal opt-out as belt-and-suspenders

Detect `TERM_PROGRAM=ghostty`, `TERM_PROGRAM=vscode` (also covers
VSCode terminal), and known-flaky `TERM` values. Skip the reset
entirely on those. Document this as a fallback in the commit message;
prefer 1.A as the primary fix.

**Action:**
1. File a tracking issue "Cross-terminal flicker survey" linking all
   seven reports (#1119, #1352, #1356, #1363, #1366, #1260, #1295).
2. Apply fix 1.A.
3. Manual smoke on macOS Terminal.app + iTerm2 + Ghostty (Hunter has
   Ghostty available; ask him).
4. Comment on each linked issue: "Fixed in v0.8.27 — please update
   and reopen if you still see flicker."

---

### 2. Long-text wrap (#1344, #1351, possibly #1359)

**Diagnosis.** v0.8.25 fixed long markdown **table cells** (`wrap_cell_text`
helper in `markdown_render.rs`). Long **paragraphs** and long **input
lines** still clip at viewport width on some terminals, instead of
wrapping. #1344 reports both directions; #1351 reports the same
symptom plus a separate "table content shows `...`" issue. #1359 is
"VSCode terminal won't wrap" — possibly the same root cause if VSCode
reports terminal size differently.

**Strategy.**

1. Reproduce at narrow width: `COLUMNS=60 deepseek` → paste a 200-
   character input line, ask for a 200-character paragraph response.
   Confirm both clip rather than wrap.
2. Trace the wrap paths:
   - `crates/tui/src/tui/markdown_render.rs::render_message` →
     `render_line_with_links` → `wrap_text` (paragraphs)
   - `crates/tui/src/tui/composer.rs` (or wherever the input box
     renders) — likely has its own wrap logic that diverged
3. Unify on `wrap_text` from `markdown_render`. The composer should
   use the same width-aware wrapper as the transcript.
4. Add snapshot tests for both surfaces at widths 40, 60, 80, 120.
5. For VSCode-terminal-specific size detection issues (#1359), verify
   `crossterm::terminal::size()` returns the right value when run
   inside VSCode terminal. If wrong, look at `--columns` override.

**Cost:** 3-4 hours including tests.

---

### 3. Pager copy-out (#1354)

**Diagnosis.** When users hit `Alt+V` (tool details) or `Ctrl+O`
(thinking content), they get a pager view. The pager intercepts mouse
capture, so terminal-native selection is disabled inside it. There's
no in-app copy keybinding. Result: users can see the content but
can't copy it. High-frustration UX gap — pager users are usually
specifically there to copy something out.

**Strategy.** Add a `c` (or `y`, vi-style) keybinding inside the
pager view that copies the entire visible content to clipboard, with
a status confirmation toast.

In `crates/tui/src/tui/views/pager.rs` (or wherever `PagerView` is
defined — search for `impl ModalView for PagerView`):

```rust
// inside handle_key, somewhere with the existing Esc/q/PgUp/PgDn handlers
KeyCode::Char('c') | KeyCode::Char('y') => {
    let text = self.body_text(); // whatever method gives the full body
    if app.clipboard.write_text(&text).is_ok() {
        app.status_message = Some("Pager content copied".to_string());
    } else {
        app.status_message = Some("Copy failed".to_string());
    }
    return Vec::new();
}
```

Also surface the keybinding in the pager footer: append `[c copy]`
to the existing affordance line.

Add a regression test that constructs a pager, sends `c`, and
asserts the clipboard mock saw the body text.

**Cost:** ~45 minutes.

---

## P1 — should ship; clear shape, real impact

### 4. Ctrl+C context-sensitive (#1337, #1367)

**Diagnosis.** Two related issues:
- **#1337:** Windows users expect `Ctrl+C` to copy (legacy Windows
  convention). Our binding is exit. They lose work copying.
- **#1367:** Users don't know how to interrupt a long-running task.
  `Esc` works but isn't discoverable.

**Strategy — context-sensitive Ctrl+C (resolves both):**

Three branches based on app state:

| State | Ctrl+C behavior |
|---|---|
| **Selection active** | Copy + clear selection. No exit. |
| **Turn in progress** | Interrupt the turn (same as Esc). No exit. |
| **Idle, no selection** | First press: status hint "Press Ctrl+C again to exit". Second press within 2s: exit. |

This pattern is well-precedented (htop, less, tmux) and addresses
both issues in one change. Mirror Vim's "are you sure" pattern for
the idle case.

In `crates/tui/src/tui/ui.rs::handle_key_event`, find the
`KeyCode::Char('c')` + `KeyModifiers::CONTROL` arm:

```rust
KeyCode::Char('c') if m.contains(KeyModifiers::CONTROL) => {
    // Branch 1: selection active → copy
    if app.viewport.transcript_selection.is_active() {
        copy_active_selection(app);
        app.viewport.transcript_selection.clear();
        return Vec::new();
    }
    // Branch 2: turn in progress → interrupt
    if app.is_loading {
        // existing interrupt logic — same code path as Esc
        return interrupt_current_turn(app);
    }
    // Branch 3: idle → first press shows hint, second press within 2s exits
    let now = Instant::now();
    let recent_ctrl_c = app.last_ctrl_c.is_some_and(|t| now.duration_since(t) < Duration::from_secs(2));
    if recent_ctrl_c {
        return vec![ViewEvent::Exit];
    }
    app.last_ctrl_c = Some(now);
    app.status_message = Some("Press Ctrl+C again to exit".to_string());
    Vec::new()
}
```

Plus the discoverability hint for #1367: status bar during streaming
shows `[Esc cancel · Ctrl+C twice exit]`.

**Cost:** ~2 hours including tests for each branch.

---

### 5. `notify` tool (#1322)

**Diagnosis.** Model-triggerable desktop notifications. Long agent
runs would benefit from an "I'm done, look at me" pop-up. Other
tools (Claude Code) have this.

**Strategy.** Add a built-in `notify` tool spec.

1. Add `notify-rust` to `crates/tui/Cargo.toml` (already cross-
   platform: macOS Notification Center, Linux libnotify, Windows toast).
2. New tool in `crates/tui/src/tools/notify.rs`:
   ```rust
   pub struct NotifyTool;
   
   #[async_trait]
   impl ToolSpec for NotifyTool {
       fn name(&self) -> &'static str { "notify" }
       fn description(&self) -> &'static str {
           "Display a desktop notification to the user. Use sparingly — only when a long-running task completes or needs the user's attention."
       }
       fn input_schema(&self) -> Value { /* {title: required, body: optional} */ }
       fn capabilities(&self) -> Vec<ToolCapability> {
           vec![ToolCapability::RequiresApproval]
       }
       fn approval_requirement(&self) -> ApprovalRequirement {
           ApprovalRequirement::Auto  // notifications are low-risk
       }
       async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, ToolError> {
           // truncate title to ~60 chars, body to ~200
           // skip if app is currently focused (don't notify about
           // the thing the user is watching) — read from
           // app.focus_state if available
           // call notify_rust::Notification::new()...
       }
   }
   ```
3. Wire up in `tool_setup.rs` (probably register conditional on a
   `Feature::DesktopNotifications` feature flag, default-on).
4. Add config opt-out: `[tools.notify] enabled = false`.

Auto-suppress when terminal is focused — the user is watching, no
notification needed.

**Cost:** ~3-4 hours.

---

### 6. `/skills --remote` diagnostic (#1329)

**Diagnosis.** "Failed to fetch" with no details. Could be TLS,
network policy, auth, rate limit. Bare error → undiagnosable.

**Strategy.** First fix is observability — surface the underlying
error chain.

In `crates/tui/src/commands/skills.rs` (or wherever `--remote` is
handled), find the `.unwrap_err()` or `.context(...)` that's
collapsing the chain:

```rust
// before
return Err(anyhow!("Failed to fetch"));

// after
return Err(err.context("Failed to fetch remote skills"));
// or, when surfacing to the user:
return CommandResult::error(format!("Failed to fetch remote skills:\n{err:#}"));
```

Mirror the v0.8.23 #1244 fix shape (alternate `{err:#}` formatting
for the full anyhow chain).

Once the underlying error is visible, the actual bug becomes
diagnosable. Likely either a TLS issue (rustls vs system trust store)
or the network policy blocking the registry endpoint.

**Cost:** ~30 minutes for the diagnostic improvement.

---

### 7. MCP lazy reload on config change (#1267 part 2)

**Diagnosis.** v0.8.26 fixed the diagnostic side (stderr capture).
The "auto-reload after config edit" piece is still missing — users
have to manually run `/mcp reload` after editing `~/.deepseek/config.toml`.

**Strategy — lazy hash check (no file watcher).** File watchers add
long-lived tasks and have edge cases on remote / network filesystems.
A lazy hash compare is bounded and cheap.

In `crates/tui/src/mcp.rs::McpPool`:

```rust
pub struct McpPool {
    // ... existing fields
    config_hash: u64,  // hash of mcp config at last (re)connection
}

impl McpPool {
    fn current_config_hash(&self, config: &McpConfig) -> u64 {
        let mut hasher = std::hash::DefaultHasher::new();
        // hash the relevant fields: servers map, timeouts, sandbox_mode
        config.hash(&mut hasher);
        hasher.finish()
    }

    pub async fn get_or_connect(&mut self, server: &str, config: &McpConfig) -> Result<&mut McpConnection> {
        let new_hash = self.current_config_hash(config);
        if new_hash != self.config_hash {
            self.reload_all(config).await?;
            self.config_hash = new_hash;
        }
        // existing get_or_connect logic
    }
}
```

`McpConfig` and adjacent types may need `Hash` derived. If hashing
the whole config tree is expensive, hash just the `[mcp_servers]`
section + `sandbox_mode`.

**Cost:** ~2 hours including tests.

---

## P2 — nice-to-have if time permits

### 8. Layout overlap (#1357)

**Diagnosis.** Input box and inline runtime hint ("Cache: 99% hit |
hit X | miss Y") render in adjacent rects but one isn't clearing its
area properly when the other expands.

**Strategy.** Inspect `crates/tui/src/tui/ui.rs::render` — find the
composer's reserved-rows calculation. It probably doesn't account for
the hint line on resize / long-content. Fix the rect math.

**Cost:** ~2 hours (1 to repro, 1 to fix).

### 9. `/skills` filter argument (#1318)

**Diagnosis.** v0.8.26 added inter-row spacing (#1328 from @reidliu41).
Reporter may want more.

**Strategy.**
1. Comment on #1318 asking if v0.8.26's spacing is enough.
2. If not, add `/skills <prefix>` arg → filter to skills whose names
   start with `<prefix>`. Mirror how `/help <topic>` works.

**Cost:** Triage ping; 30 min if filter wanted.

### 10. Status comments on partial fixes (#1112, #1267, #1318)

Three issues that are partly addressed and need the reporter to
confirm:

- **#1112** — 1.2 TB snapshots. Cap added in v0.8.24. Comment:
  "500 MB cap added in v0.8.24. Are you still seeing growth above
  that? If so, please share `du -sh ~/.deepseek/snapshots`."
- **#1267** — macOS Seatbelt blocks npx MCP. Already commented during
  v0.8.26 cycle. Don't re-comment.
- **#1318** — `/skills` crowded. Comment: "v0.8.26 added inter-row
  spacing (#1328). Does this resolve it for you?"

**Cost:** ~5 minutes total.

---

## P3 — investigate or defer

### #1338 — Enter mid-run crashes Windows TUI

**Defer unless you have Windows.** Add stack capture so the next
reporter gets actionable output:

```rust
// in main.rs — add panic hook that logs to ~/.deepseek/last-panic.log
std::panic::set_hook(Box::new(|info| {
    let _ = std::fs::write(
        dirs::home_dir().unwrap_or_default().join(".deepseek/last-panic.log"),
        format!("{info}\n{}", std::backtrace::Backtrace::capture()),
    );
}));
```

**Cost:** 30 min for the diagnostic; actual fix needs Windows VM.

### #1062 — Capacity-memory checkpoint cross-session recovery

Old, complex. Don't pull into v0.8.27. Needs scope conversation with
Hunter.

### #1067 — glibc version required (older Linux distros)

Static-link the deepseek binary or add a musl build to release.yml.
**v0.8.27 candidate if anyone has time** — purely a build-config
change.

### #1364 — Hooks mutation rights + turn-end event

**Defer to v0.9.0.** Real ask — Claude Code hooks have this. Worth
doing as part of a hooks-v2 task. Out of scope for a polish release.

### #1343 — Desktop GUI

**Defer.** Recurring request. v0.9.x territory at the earliest.
Comment with roadmap status if not already.

---

## Issues to close as fixed in v0.8.26

These need a comment + close. Already verified by the previous agent:

| # | Title | Fixed by |
|---|---|---|
| #1163 | Mouse drag-select / copy doesn't auto-scroll | PR #1239 |
| #1169 | Selection crosses sidebar | Mouse-capture default-on for WT |
| #1255 | Win10 conversation can't scroll | Mouse-capture default-on |
| #1292 | Mac trackpad text selection broken | Drag-select rewrite |
| #1298 | Wheel scrolls input history not transcript | Mouse-capture default-on |
| #1308 | base_url for ollama/vllm ignored | Config-load warning |
| #1331 | Mouse wheel changed in v0.8.24 | Mouse-capture default-on |

**Action:** Run through with this comment template (translated for
zh-CN issues #1255, #1292):

```
Fixed in [v0.8.26](https://github.com/Hmbown/DeepSeek-TUI/releases/tag/v0.8.26).
Please update with:

- npm: `npm install -g deepseek-tui@latest`
- brew: `brew upgrade deepseek-tui`
- cargo: `cargo install --force deepseek-tui-cli`

Reopen if you still hit it. Thanks for the report!
```

---

## Workflow

### Step 1 — Branch state confirmation

```bash
cd /Volumes/VIXinSSD/whalebro/deepseek-tui
git checkout work/v0.8.27
git pull origin work/v0.8.27 || true
git log --oneline main..HEAD | head -20
```

You should see ~16 commits already on the branch. Add to it; don't
restart.

### Step 2 — Tackle in priority order (P0 → P3)

For each item:

1. Read the issue thread on GitHub. Note any reporter clarifications.
2. Implement per the strategy above.
3. Add tests (TDD where the strategy specifies; verification snapshot
   otherwise).
4. After each commit:
   ```bash
   cargo fmt --all
   cargo clippy -p deepseek-tui --all-targets --all-features --locked -- -D warnings
   cargo test -p deepseek-tui --bin deepseek-tui --all-features --locked --no-fail-fast \
     2>&1 | grep "test result:" | tail -3
   ```
5. Add a CHANGELOG entry under `## [0.8.27]` `### Fixed` or `### Added`,
   crediting the issue number and original reporter.

The known-flaky test is
`mcp_connection_supports_streamable_http_event_stream_responses` —
passes in isolation, intermittent under load. Don't chase.

### Step 3 — Issue triage pass

After each P0/P1 fix lands, close the corresponding issue with a
comment template. Don't wait until the end of the cycle — closing as
you go keeps the issue list visibly responsive.

### Step 4 — Bump version when ready

```bash
sed -i '' 's|^version = "0.8.26"|version = "0.8.27"|' Cargo.toml
find crates -maxdepth 2 -name Cargo.toml -exec sed -i '' \
  's|version = "0.8.26"|version = "0.8.27"|g' {} +
sed -i '' 's|"version": "0.8.26"|"version": "0.8.27"|' \
  npm/deepseek-tui/package.json
sed -i '' 's|"deepseekBinaryVersion": "0.8.26"|"deepseekBinaryVersion": "0.8.27"|' \
  npm/deepseek-tui/package.json
cargo update --workspace --offline
./scripts/release/check-versions.sh
```

Add `## [0.8.27] - YYYY-MM-DD` heading at the top of CHANGELOG.md.

### Step 5 — Full preflight + install

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked --no-fail-fast \
  2>&1 | grep "test result:" | tail -10
./scripts/release/check-versions.sh
./scripts/release/publish-crates.sh dry-run
cargo build --release --locked -p deepseek-tui-cli -p deepseek-tui
node scripts/release/npm-wrapper-smoke.js

cargo install --path crates/cli --force --locked
cargo install --path crates/tui --force --locked
deepseek --version  # confirm: deepseek 0.8.27 (<sha>)
```

### Step 6 — STOP-FOR-MAINTAINER

Push the branch and open the release PR. Hand back to Hunter:

- PR number + link
- Bullet list of all P0/P1/P2 items completed
- Items deferred (P3 items) with reason
- Preflight summary
- "deepseek 0.8.27 installed at ~/.cargo/bin/, ready for testing"
- Issues closed with v0.8.26 fixed-in comment

WAIT for Hunter's "go" before merging, tagging, or publishing.

### Step 7 — Release flow

Same as v0.8.26 — see `.claude/HANDOFF_v0.8.26_security.md` steps 8–11.
Concretely: merge PR → auto-tag fires → release.yml builds matrix +
GitHub Release → crates.io publish → npm publish → Homebrew formula
update → verify GHCR → README post-merge bookkeeping.

**No GHSA flow this cycle.** If a new advisory comes in, branch
v0.8.28 — don't bundle.

### Step 8 — CNB mirror (new for v0.8.27)

After GitHub Release is live:

```bash
# If CNB_TOKEN is in repo secrets, the GitHub Action handles it
# automatically on tag push. Verify:
#   https://cnb.cool/deepseek-tui.com/DeepSeek-TUI/-/tags

# Otherwise (one-time bring-up was done manually) push from local:
git remote add cnb https://<token>@cnb.cool/deepseek-tui.com/DeepSeek-TUI 2>/dev/null || true
git push cnb v0.8.27 main
```

Add a banner to README.md and README.zh-CN.md if not already there:

```
> 🇨🇳 国内镜像 / Mainland China mirror:
>   https://cnb.cool/deepseek-tui.com/DeepSeek-TUI
> Issues and PRs: please use GitHub.
```

---

## Quality bar

Apply to every change:

- CI green (modulo documented flaky)
- No new `unwrap()` / `expect()` outside test code
- No new external network surfaces without `validate_network_policy`
- New env vars or config keys → `config.example.toml` entry + CHANGELOG note
- Behavior changes user-visible → CHANGELOG entry calling out the change

When in doubt, defer to v0.8.28. A clean release of 8 P0/P1 items beats
a cluttered release of 15 with one regression.

---

## Output expectation

Realistic v0.8.27 landing zone on top of the existing 16 commits:

- **All 7 closable v0.8.26 issues** closed with comments
- **P0 #1, #2, #3** fully shipped (flicker, wrap, pager copy)
- **P1 #4, #5, #6, #7** at least 2 of 4 shipped
- **P2 #8, #9, #10** at least the comment-pings
- **CNB mirror** wired in

That's a substantial v0.8.27 that respects the "post-v0.8.26 inflow"
framing. Users see real responsiveness to their reports.

If at any point something looks materially harder than this document
suggests, STOP and surface to Hunter with the specifics. Don't
freelance scope.
