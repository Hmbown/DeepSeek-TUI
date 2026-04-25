You are working in /Volumes/VIXinSSD/deepseek-tui.

Goal:
Continue improving deepseek-tui as a first-class DeepSeek V4 coding harness, building directly on the previous overnight slice (commit f190ab74 + merge aa356e8b + import fix f9ede57c, plus release commit d7944421 / tag v0.4.6). This time, ACTUALLY read /Volumes/VIXinSSD/mathcode — the previous run claimed it wasn't mounted and adapted patterns from the work-order description instead. mathcode IS mounted; the directory exists and is readable.

What already shipped (do not redo):
- Slice A: setup --status, setup --clean (with --force gate), setup --tools, setup --plugins, doctor --json, init_tools_dir / init_plugins_dir scaffolding with frontmatter examples (# name: / # description: / # usage: in tools/example.sh; ----delimited YAML in plugins/example/PLUGIN.md). Code in crates/tui/src/main.rs.
- Slice B: fake-wrapper hostility — TOOL_CALL_START_MARKERS/END_MARKERS + filter_tool_call_delta promoted to pub(crate); FAKE_WRAPPER_NOTICE + contains_fake_tool_wrapper(); per-turn fake_wrapper_notice_emitted flag in handle_deepseek_turn. Code in crates/tui/src/core/engine.rs. Locked by crates/tui/tests/protocol_recovery.rs.
- Slice C: docs in docs/CONFIGURATION.md (Setup status, clean, and extension dirs section + Why the engine strips XML/[TOOL_CALL] text section) and README.md quick-checks block.
- 37 new tests landed (23 unit + 14 integration). All green.

Read first (in this order):
1. AGENTS.md
2. git status --short --branch  (expect clean tree on main, 0 ahead of origin/main; a stash from a prior session may exist — leave it alone)
3. README.md
4. docs/ARCHITECTURE.md
5. docs/CONFIGURATION.md  (note the new Setup status / extension dirs / Why the engine strips ... sections)
6. docs/RUNTIME_API.md
7. crates/tui/src/main.rs  (large file; pay attention to the new run_setup_status, run_setup_clean, init_tools_dir, init_plugins_dir, run_doctor, run_doctor_json helpers — do not duplicate them)
8. crates/tui/src/core/engine.rs  (new: TOOL_CALL_START_MARKERS, TOOL_CALL_END_MARKERS, filter_tool_call_delta, FAKE_WRAPPER_NOTICE, contains_fake_tool_wrapper)
9. crates/tui/src/core/engine/tests.rs
10. crates/tui/tests/protocol_recovery.rs
11. crates/tui/src/client.rs
12. crates/tui/src/tui/widgets/mod.rs
13. crates/tui/src/core/capacity.rs  (existing compaction decisions — relevant for issue #6)

Then ACTUALLY read mathcode (it IS mounted):
14. /Volumes/VIXinSSD/mathcode/README.md
15. /Volumes/VIXinSSD/mathcode/setup.sh   (~10KB; full status/clean/help flag matrix and error messages worth borrowing)
16. /Volumes/VIXinSSD/mathcode/run        (763B; thin wrapper that loads .env and dispatches)
17. /Volumes/VIXinSSD/mathcode/.env.example  (~6KB; clear provider/backend comments — note our codebase only loads .env via dotenvy::dotenv() in crates/tui/src/main.rs:475 and has no .env.example)
18. /Volumes/VIXinSSD/mathcode/skills/README.md
19. /Volumes/VIXinSSD/mathcode/plugins/README.md
20. /Volumes/VIXinSSD/mathcode/tools/  (list contents; look for self-describing tool scripts)
21. /Volumes/VIXinSSD/mathcode/bin/  (list contents)

Open GitHub issue spine (use as roadmap):
- #6 Coherence as plain-language session health  ← previous slice's recommended next step
- #7 Long-session evals for coherence/context handling
- #8 Workspace extraction/runtime seams
- #9 Thread/turn/item protocol stability
- #10 Sandboxing, approvals, server safety
- #11 Skills/plugins/MCP installability and management  (partly done; doctor + status now report tools/plugins; install/manage UX still missing)
- #12 DeepSeek API key setup/provider drift  (partly done; setup --status surfaces source — but no .env.example, no provider-aware setup wizard)
- #13 Observability for agent quality/coherence events  (partly done; fake-wrapper strip emits a status event — coherence ladder still missing)
- #14 Whale/DeepSeek TUI design system
- #15 Thin IDE companions over runtime API
- #16 Public roadmap/docs rewrite

Pick 1-3 high-leverage slices that can be finished and tested tonight. Strongly preferred (in priority order):

Slice 1 (recommended — issue #6 + #13): Plain-language coherence ladder.
  - The runtime already emits compaction events (CompactionStarted/Completed/Failed) and capacity decisions live in crates/tui/src/core/capacity.rs.
  - Add an Event::CoherenceState variant with a small fixed ladder: Healthy / GettingCrowded / RefreshingContext / VerifyingRecentWork / ResettingPlan.
  - Emit it from one place (engine), driven by existing capacity decisions and compaction events. Do NOT add a new background task.
  - Surface it in the TUI footer as a single chip (right-aligned, terminal-native, no emoji) and on the runtime API thread shape.
  - Snapshot-test the footer chip for each state and unit-test the state transitions from a synthetic capacity event log.
  - Land docs in docs/RUNTIME_API.md (CoherenceState shape) and docs/CONFIGURATION.md (footer chip).

Slice 2 (issue #12 + mathcode reference): .env.example + setup wizard messages.
  - Adapt /Volumes/VIXinSSD/mathcode/.env.example structure (provider/backend grouping, comment style) to a deepseek-tui .env.example at repo root.
  - Cover: DEEPSEEK_API_KEY, DEEPSEEK_BASE_URL (global vs china), DEEPSEEK_MODEL, NVIDIA_API_KEY (NIM), NIM_BASE_URL, RUST_LOG, sandbox toggles.
  - Update setup --status to point at .env.example when no .env is found ("Run `cp .env.example .env` and edit").
  - Do NOT add a `run` wrapper — dotenvy handles .env loading inside the binary, the wrapper is redundant and introduces drift.
  - Lock the .env.example shape with a test that asserts every documented variable is referenced from at least one source file.

Slice 3 (issue #14 / TUI render polish): Whale/DeepSeek design tokens.
  - Extract the current TUI color/border/padding choices into a single deepseek_theme module (light + dark variants), without changing any visible output yet.
  - Add a snapshot test or two that lock the existing rendering of one plan cell and one tool cell.
  - Out of scope for tonight: actually changing colors. Goal is to make a future skin-swap a 5-line change.

DO NOT pick:
- Anything that re-touches NIM provider, V4 reasoning context accounting, or tool-call routing — that work is finished and protected by tests.
- Anything that re-introduces multi_tool_use.parallel.
- Anything that introduces an alternate fake-wrapper code path (XML, [TOOL_CALL], <function_calls>, ```tool_code, ```python, etc.). The engine's contract is: API tool channel only, all five wrapper shapes get stripped + emit a notice.
- Broad rewrites of main.rs, the engine, or the TUI app loop.
- A `run` shell wrapper. (.env loading is already in-binary via dotenvy.)

Hard rules / hostility budget:
- Prompts must never instruct or imply that the assistant should emit XML/markdown/Replit-style fake tool calls. If you touch crates/tui/src/prompts/*.txt, the rule is: API tool channel only, never wrapped tool calls in assistant text.
- Protocol recovery must remain visible. If you change how filter_tool_call_delta or FAKE_WRAPPER_NOTICE works, update crates/tui/tests/protocol_recovery.rs in the same commit and keep at least one assertion that all 5 marker pairs are present.
- UI must stay dense and terminal-native. No emoji, no decorative box drawing for data, no gratuitous color. DeepSeek-specific copy (Whale, V4) is welcome.
- Show, don't hide, runtime problems. New events should be compact (single-line where possible) and machine-readable as structured fields.

Stop rules:
- Do not revert any user/unrelated work; the previous run already merged and pushed (current main HEAD: d7944421, tag v0.4.6).
- Do not push tags or trigger releases. Local commits + push to a feature branch is fine; the maintainer handles release decisions.
- Do not delete or modify the existing stash (if any).
- If a live provider is down/unpaid, use fixtures/mocks and say so explicitly in the report.
- If a gate fails from pre-existing unrelated work, isolate it with evidence — don't paper over.
- Pre-existing test cluster (8 tests in tools::git*, tools::diagnostics, tui::ui::tests::workspace_context_refresh*) fails because of a sandbox commit-signing issue with `git commit ... -S`. Document but do not "fix" by globally setting commit.gpgsign=false — fix narrowly inside init_git_repo at crates/tui/src/tui/ui/tests.rs:158 if you touch it.

Required verification before final report:
- cargo fmt --all -- --check
- cargo check --workspace --all-targets --locked
- cargo test --workspace --all-features --locked
- cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
- cargo build --release --locked -p deepseek-tui-cli -p deepseek-tui

Live smoke (optional, time-boxed at 60s):
- If DEEPSEEK_API_KEY or NVIDIA_API_KEY is set in the env, run a single bounded turn against the relevant provider with a hard token cap and report the first 30 lines of output. Otherwise skip and say so.

Final report headings (use exactly these):
1. Summary
2. Implemented Changes
3. GitHub Issues Advanced
4. MathCode Reference Points Used  (be honest: list which mathcode files you actually read and which patterns you adapted vs deliberately skipped, with one-sentence rationale per skip)
5. Tests/Gates
6. Live Smoke Result
7. Remaining Risks
8. Next Best Slice
