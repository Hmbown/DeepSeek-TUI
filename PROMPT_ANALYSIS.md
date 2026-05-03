# System Prompt Analysis — "Mismanaged Genius" Hypothesis

## Methodology

Read every prompt layer (`base.md`, mode overlays, personality, approval policies),
traced the assembly logic in `prompts.rs`, and compared against what DeepSeek V4 can
actually do vs what the prompt currently encourages.

---

## Summary: The Prompt Is Cautious, Not Strategic

The current prompt has excellent safety rails — clear "when NOT to use" guidance,
anti-hallucination instructions, and decomposition philosophy. But it treats the
model's most powerful capabilities (RLM, sub-agents, parallel tool execution) as
**specialty escape hatches** rather than **default strategic tools**. The result:
a capable model that hesitates to parallelize, underuses its fan-out abilities, and
serializes work that could be done concurrently.

The prompt was written when the model was less reliable and needed guardrails. V4
models can handle more autonomy — the prompt should reflect that.

---

## Gap-by-Gap Analysis

### Gap 1: RLM Is Framed as a Last Resort, Not a Strategic Tool

**Current text** (`base.md`, "RLM Is a Specialty Tool"):
> `rlm` is for one specific shape of work: a long input that genuinely does not fit
> in your context. Reach for it ONLY when direct reasoning over the input is impossible
> because of its size.

**Problem**: RLM is actually three tools in one:
1. Chunk-and-process for long inputs (the only case the prompt acknowledges)
2. Parallel `llm_query_batched` for multi-angle analysis (e.g., "classify these 20 items")
3. `rlm_query` for recursive decomposition of problems that benefit from sub-LLM critique

The prompt actively discourages cases 2 and 3. A model that could classify 20 files in
parallel instead reads them one at a time. A model that could get a "second opinion" on
its reasoning from a sub-LLM instead trusts its first pass.

**Suggested rewrite** — replace the restrictive framing with a capability guide:

```
## RLM — When to Use It

RLM loads input into a Python REPL where you write code that calls sub-LLM helpers
(`llm_query`, `llm_query_batched`, `rlm_query`). Three patterns, not one:

**CHUNK** — A single input that genuinely doesn't fit in your context window (a whole file
> 50K tokens, a long transcript, a multi-document corpus). Split it, process each chunk,
synthesize.

**BATCH** — Many independent items that each need LLM attention (classify 20 entries,
extract fields from 30 documents, score 15 candidates). Use `llm_query_batched` for
parallel execution — it fans out to the same DeepSeek client and finishes in one turn
what would take 15 sequential reads.

**RECURSE** — A problem that benefits from decomposition + critique. Use `rlm_query` to
have a sub-LLM review your reasoning, identify gaps, or explore alternative approaches.
The sub-LLM returns a synthesized answer you verify against live tool output.

**When NOT to use RLM**: a single short file you can read directly; a simple
classification on 3 items; interactive iterative exploration (RLM is one-shot batch).
For those, `read_file`, `grep_files`, or `agent_spawn` are faster and cheaper.
```

### Gap 2: Sub-Agents Are "Implementation, Not Exploration"

**Current text** (`base.md`, "When NOT to use `agent_spawn`"):
> You haven't first laid out a plan with `checklist_write`. Sub-agents are
> implementation, not exploration.

**Problem**: This directly contradicts the Plan mode prompt, which correctly says
"Spawn read-only sub-agents for parallel investigation." But the Agent mode prompt
gets the restrictive version. The result: in Agent mode (where most work happens),
the model treats sub-agents as a last step ("now implement the plan") rather than a
discovery tool ("investigate these 4 things in parallel to understand the problem").

**Reality**: Sub-agents are the BEST tool for parallel exploration. A single
`agent_spawn` call that fans out to 3 read-only children investigating different
modules is faster AND more thorough than reading them sequentially.

**Suggested rewrite** — move sub-agent guidance from "when NOT to use" to a positive
section:

```
## Sub-Agent Strategy

Sub-agents are cheap — DeepSeek V4 Flash costs $0.14/M input. Use them liberally for
parallel work:

- **Parallel investigation**: When you need to understand 3+ independent files or
  modules, spawn one read-only sub-agent per target. They run concurrently and return
  structured findings you synthesize.

- **Parallel implementation**: After a plan is laid out (`checklist_write` +
  `update_plan`), spawn one sub-agent per independent leaf task. Each does one
  thing well; you integrate results.

- **Solo tasks**: A single read, a single search, a focused question — do these
  yourself. Spawning has overhead; one-turn reads are faster direct.

- **Sequential work**: If step B depends on step A's output, run A yourself, then
  decide whether to spawn B based on what A found.
```

### Gap 3: No "Batch Everything" Instinct

**Current text** (`base.md`, "Your V4 Characteristics"):
> **Parallel execution.** Batch independent reads, searches, and greps into a single
> turn. Never serialize operations that can run concurrently — parallel tool calls
> share the same turn and finish faster.

**Problem**: This instruction is correct but buried in a V4 Characteristics section
the model may not internalize as a behavioral rule. The model often fires one tool,
waits for the result, then fires another — even when both are independent.

**Suggested addition** — add a concrete heuristic at the top of the toolbox section:

```
## Parallel-First Heuristic

Before you fire any tool, scan your plan: is there another tool you could run
concurrently? If two operations don't depend on each other, batch them. Examples:

- Reading 3 files → 3 `read_file` calls in one turn
- Searching for 2 patterns → 2 `grep_files` calls in one turn
- Checking git status AND reading a config → `git_status` + `read_file` in one turn

The dispatcher runs parallel tool calls simultaneously. Serializing independent
operations wastes the user's time and your context budget.
```

### Gap 4: Thinking Budget Too Conservative for V4

**Current text** (`base.md`, "Thinking Budget"):
| Task type | Thinking depth | Rationale |
|-----------|---------------|-----------|
| Simple factual lookup | Skip | Answer is immediate |
| Code generation (single function) | Light | Pattern-matching |

**Problem**: V4 models have 1M context and produce thinking tokens that improve
output quality even for "simple" tasks. Skipping thinking on a factual lookup is
correct. But "Light" for code generation understates the value of thinking — a
30-second think before writing a function catches edge cases, checks against
project conventions, and prevents rework.

**Suggested rewrite** — bump the defaults up one tier:

| Task type | Thinking depth | Rationale |
|-----------|---------------|-----------|
| Simple factual lookup (read, search) | Skip | Answer is immediate |
| Tool output interpretation | Light | Verify result matches intent |
| Code generation (single function) | Medium | Conventions, edge cases, context fit |
| Multi-file refactor | Medium | Cross-file dependencies |
| Debugging (error to root cause) | Deep | Hypothesis generation |
| Architecture design | Deep | Trade-offs, constraints |
| Security review | Deep | Adversarial reasoning |

### Gap 5: No "Verify Before Claiming" Pattern

**Current state**: The subagent output format (`subagent_output_format.md`) has an
EVIDENCE section that requires concrete artifact citations. This is excellent. But
the main prompt (`base.md`) doesn't establish this as a general habit.

**Problem**: The model sometimes reads a file, then writes a patch based on its
memory of the file rather than re-reading the specific lines it's changing. Or it
claims a shell command succeeded based on exit code 0 without checking the output.

**Suggested addition** — add to the "Decomposition Philosophy" section:

```
## Verification Principle

After every tool call that produces a result you'll act on, verify before
proceeding:
- File reads: confirm the line numbers you're about to patch are what you think
- Shell commands: check stdout, not just exit code
- Search results: confirm the match is what you expected
- Sub-agent results: cross-check one finding against a direct `read_file`

Don't claim a change worked until you've observed evidence. Don't trust memory
over live tool output.
```

### Gap 6: No Composition Heuristic for Complex Work

**Current state**: The prompt says "For complex initiatives, layer `update_plan`
above `checklist_write`." This is correct but vague. The model sometimes creates
a plan, creates a checklist, and then works through the checklist without
re-evaluating the plan.

**Suggested addition**:

```
## Composition Pattern for Multi-Step Work

For any task estimated to take 5+ steps:

1. `update_plan` — 3-6 high-level phases (status: pending)
2. `checklist_write` — concrete leaf tasks under the first phase (mark first
   `in_progress`)
3. Execute phase 1, updating checklist as you go
4. After each phase completes, re-read your plan: does phase 2 still make sense?
   Update the plan if new information changes the approach.
5. When a phase reveals sub-problems, add them to the checklist or spawn
   investigation sub-agents — don't guess.
```

### Gap 7: Approval Mode Contradiction

**Current state**: The Agent mode approval policy says "Any write, patch, shell
execution, sub-agent spawn, or CSV batch operation will ask for approval first."
But the "Key principle" says "make your work visible" and encourages
`checklist_write` to populate the sidebar.

**Problem**: In Agent mode, the model often waits for approval on EACH step
individually. A batch of 3 `edit_file` calls requires 3 separate approval rounds.
The prompt should encourage batching approvals: present the full plan, get
approval once, then execute all writes in parallel.

**Suggested addition** — add to the Agent mode overlay:

```
## Efficient Approvals

When your plan includes multiple writes, present them together:
1. Show `checklist_write` with all write steps listed
2. Request approval for the batch ("I need to make 3 edits across 2 files...")
3. Once approved, execute all writes in one turn (parallel `edit_file` /
   `apply_patch` calls)

Don't sequence approvals one at a time. The user wants context, not interruption.
```

---

## Concrete Prompt Changes

### 1. `base.md` — Replace "RLM Is a Specialty Tool" section

Remove the current restrictive "RLM Is a Specialty Tool" section entirely.
Replace with the "RLM — When to Use It" section from Gap 1 above.

### 2. `base.md` — Replace "When NOT to use `agent_spawn`"

Remove the bullet about sub-agents from the "When NOT to use" section.
Move it to a new positive "Sub-Agent Strategy" section (Gap 2 above) placed
immediately after the "Decomposition Philosophy" section.

### 3. `base.md` — Add "Parallel-First Heuristic"

Insert after the toolbox reference section, before "When NOT to use."
(Gap 3 above.)

### 4. `base.md` — Bump thinking budget defaults

Change the "Code generation (single function)" row from Light → Medium.
(Gap 4 above.) Single-line change.

### 5. `base.md` — Add "Verification Principle"

Insert as a sub-heading under "Decomposition Philosophy."
(Gap 5 above.)

### 6. `base.md` — Add "Composition Pattern"

Insert as a sub-heading under "Decomposition Philosophy," after
"Verification Principle."
(Gap 6 above.)

### 7. `modes/agent.md` — Add "Efficient Approvals"

Insert at the end of the Agent mode overlay.
(Gap 7 above.)

---

## What NOT to Change

- **"When NOT to use `exec_shell`"** — this guidance is correct and important.
  Typed tools beat shell-outs for reliability.
- **"When NOT to use `edit_file` / `apply_patch`"** — tool selection rules are
  good and prevent blind patching.
- **Preamble rhythm** — the tone guidance is well-calibrated.
- **Output formatting** — terminal constraints are real; the guidance is correct.
- **Context management** — the ~80% compaction suggestion is practical.
- **Sub-agent sentinel protocol** — the integration pattern is well-defined.

---

## Risk Assessment

**Risk: Over-parallelization**. A model told to "batch everything" might spawn
sub-agents for trivial reads. Mitigation: the "Solo tasks" bullet in the new
sub-agent strategy section explicitly says "do these yourself."

**Risk: Over-thinking**. Bumping the thinking budget might waste tokens on
simple code generation. Mitigation: "Medium" for single-function generation is
still conservative; the model can self-regulate with the existing guidance
"skip for lookups."

**Risk: RLM over-use**. Framing RLM as a strategic tool might cause inappropriate
use for tasks better served by `agent_spawn`. Mitigation: the new "When NOT to
use RLM" bullet covers the common failure modes.

**Risk: Cache busting**. Adding text to the system prompt changes its byte
representation, which busts the prefix cache for the first turn after the change.
Mitigation: this is a one-time cost; subsequent turns hit the cache at the new
prompt boundary.
