# DeepSeek TUI — Cache Orchestration (Phase 8)

## V4 Prefix Cache Economics

DeepSeek V4 caches shared prefixes at 128-token granularity with ~90% cost
discount. Cache hits are free (or near-free); cache misses incur the full
input cost. Effective cache orchestration is the difference between a
$0.50 session and a $0.10 session for the same work.

## Configuration

```toml
# ~/.deepseek/config.toml
[cache_orchestration]
# Enable automatic cache-preserving rewrites (default: true)
enabled = true
# Minimum common prefix ratio before we optimize (default: 0.8)
min_prefix_ratio = 0.8
# Maximum turns before suggesting /compact (default: 40)
max_turns_before_compact_hint = 40
# Cost budget per session in USD (0 = unlimited)
cost_budget_usd = 1.00
# Cost budget period: "session" | "daily" | "weekly"
cost_budget_period = "daily"
# Model to use when over budget (empty = keep current model)
budget_fallback_model = "deepseek-v4-flash"
```

## Automatic Optimizations

1. **Prefix stability detection** — Before sending a request, the engine
   computes the common prefix ratio with the previous request. If >90%,
   the request is assembled to maximize byte-stability (preserving cache).

2. **Compaction alignment** — `/compact` is automatically aligned to
   128-token boundaries so the compacted prefix hits the cache from the
   first post-compaction turn.

3. **Cost budget enforcement** — When a session's cumulative cost exceeds
   the configured budget, the engine automatically falls back to Flash
   for non-critical turns. Pro is reserved for: architecture decisions,
   security reviews, release work, and debugging.

4. **Cache telemetry** — `/tokens` and `/cost` show per-turn cache hit
   rates. A visual indicator (🟢 >80% / 🟡 >40% / 🔴 <40%) helps users
   understand when their prompt patterns are cache-friendly.

## RLM 2.0 — Interactive REPL (planned)

Current RLM is one-shot batch: sub-LLM writes Python, runs it once,
returns `FINAL()`. RLM 2.0 will support:

- **Multi-step execution**: sub-LLM can run Python in steps, inspect
  intermediate output, and decide next actions.
- **Conditional logic**: "If classification confidence < 0.8, retry with
  a more specific prompt."
- **`max_steps` parameter**: prevents infinite loops (default: 10).

```json
// RLM 2.0 interactive mode (planned API shape)
{
  "task": "Classify and re-classify until confident",
  "file_path": "data.csv",
  "interactive": true,
  "max_steps": 10
}
```
