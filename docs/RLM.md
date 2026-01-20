# RLM Mode

RLM mode (“Recursive Language Model” mode) is DeepSeek CLI’s long-context workflow: it stores large context externally (Aleph-style external memory) and provides REPL-like tools to explore and query it without stuffing everything into the model’s context window.

If you’re curious about the research inspiration and implementation notes, see:

- `docs/rlm-paper.txt`
- `docs/rlm_gap_analysis.md`

## When To Use It

RLM mode is best for:

- “Analyze this large file / doc”
- “Summarize the whole repository”
- “Search for every occurrence of X and explain it”
- Big pasted blocks of text

The UI may auto-switch to RLM for large file requests, “largest file”, explicit “RLM” requests, and large pastes.

## How To Use It

### Switch modes

- Press `Tab` until you reach **RLM**
- Or type `/rlm` (or `/aleph`) to jump directly into RLM mode

### Load context

In RLM mode, `/load` loads external context (in other modes, `/load` loads a saved chat JSON):

```text
/load @path/to/file.rs
```

`@path` is workspace-relative.

### Inspect and query

- `/status` shows which contexts are loaded and basic usage totals.
- `/repl` toggles expression input mode.

Typical REPL helpers include:

- `lines(1, 80)` (show a slice of the context)
- `search("pattern")`
- `chunk(2000)` (create fixed-size chunks for later querying)

Under the hood, the model uses tools like `rlm_load`, `rlm_exec`, `rlm_status`, and `rlm_query`.

## Cost and Safety Notes

- `rlm_query` can be expensive because it triggers additional model calls. Prefer batching related questions.
- RLM mode auto-approves tools; keep `--workspace` scoped to the repo you want it to access.
