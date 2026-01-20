# RLM Implementation Gap Analysis

This document compares the DeepSeek CLI's current RLM-like sub-agent system against the actual Recursive Language Models (RLM) architecture described in the paper by Khattab et al. (2025).

## Overview

The RLM paper introduces a paradigm where LLMs treat long prompts as part of an external environment, allowing programmatic examination, decomposition, and recursive self-calling over prompt snippets. The DeepSeek CLI has implemented a sub-agent system that touches on some RLM concepts but lacks critical RLM-specific infrastructure.

**Current Status**: DeepSeek CLI now includes a shared RLM session with dedicated tools (`rlm_load`, `rlm_exec`, `rlm_query`, `rlm_status`) and an RLM system prompt that externalizes context. Remaining gaps are mostly around deeper recursive orchestration and semantic chunking.

## Update (v0.1.6)

The following RLM gaps have been addressed in Sprint 2/3:

- **REPL integration** via `rlm_exec` tool against a shared RLM session
- **Sub-call support** via `rlm_query` with batch and verify modes
- **Externalized context** with RLM context summaries injected into the system prompt
- **RLM-specific prompt** (`src/prompts/rlm.txt`) with FINAL / FINAL_VAR guidance
- **Chunking helpers** (`chunk_sections`, `chunk_lines`, `chunk_auto`) for semantic-ish splits
- **Auto-chunk batching** (`rlm_query` + `auto_chunks`) for whole-doc sweeps
- **Buffer variables** (`vars/get/set/append/del` + `store_as` + FINAL_VAR parsing)
- **Usage tracking** for RLM sub-calls (query count + token totals)
- **REPL toggle** (`/repl`) with RLM chat default
- **LLM-managed context loading** (`rlm_load`, plus `/load @path` workspace support)
- **RLM session status** (`rlm_status` for context + usage summaries)
- **Auto-RLM switching** for large file requests and large pastes (keeps small-context queries in base mode per paper tradeoff)
- **RLM usage guardrails** in the footer (warns on high query/token usage)

Remaining opportunities (low priority): deeper recursive sub-agent loops and more model-specific prompt tuning.

---

## Key RLM Concepts (From Paper)

### Core Architecture
1. **REPL Environment**: Python REPL where context is loaded as a variable
2. **llm_query Function**: Enables recursive sub-LM calls within the REPL
3. **Context as External Variable**: Prompt is NOT fed directly to the LLM
4. **Programmatic Context Interaction**: Model writes code to examine/decompose context
5. **Buffer Variables**: Accumulate partial results across recursive calls
6. **FINAL/FINAL_VAR Tags**: Structured answer output mechanism

### Key Behaviors
- Iterative code execution in REPL
- Dynamic context chunking based on analysis
- Recursive sub-calls for information-dense tasks
- Answer verification through sub-LM calls
- Cost-aware sub-call batching

---

## Gap Analysis

### 1. Missing REPL Integration for LLM

**RLM Paper Requirement:**
> "The REPL environment is initialized with: 1) A 'context' variable that contains extremely important information about your query. 2) A 'llm_query' function that allows you to query an LLM inside your REPL environment. 3) The ability to use 'print()' statements to view the output of your REPL code."

**Current DeepSeek Implementation (v0.1.6):**
- RLM mode exposes `rlm_exec` and `rlm_query` tools to the model
- REPL expressions operate on shared session state across turns
- LLM can execute expressions and spawn sub-calls from tool usage

**Gap Severity:** 🟢 LOW

**Status:** ✅ Addressed via RLM tools + prompt integration

---

### 2. No Recursive Sub-Call Architecture

**RLM Paper Requirement:**
> "RLMs defer essentially unbounded-length reasoning chains to sub-(R)LM calls... RLMs store the output of sub-LM calls over the input in variables and stitch them together to form a final answer."

**Current DeepSeek Implementation (v0.1.6):**
- Recursive sub-calls are now available via repeated `rlm_query` tool invocations
- Shared buffer variables allow stitching results across calls
- Sub-agent nesting is still flat (no hierarchical runtime)

**Gap Severity:** 🟡 MEDIUM

**Remaining Enhancements:**
- Optional nested sub-agent orchestration with shared buffers + depth limits

---

### 3. Missing RLM-Specific System Prompts

**RLM Paper Requirement:**
> "You are tasked with answering a query with associated context... You can access, transform, and analyze this context interactively in a REPL environment that can recursively query sub-LLs, which you are strongly encouraged to use as much as possible."

**Current DeepSeek Implementation (v0.1.6):**
- Dedicated RLM prompt (`src/prompts/rlm.txt`) with REPL/tool guidance
- RLM sub-call prompt enforces FINAL / FINAL_VAR output conventions
- Prompt guidance for batching and verification

**Gap Severity:** 🟢 LOW

**Status:** ✅ Addressed

---

### 4. No Context Offloading to External Environment

**RLM Paper Requirement:**
> "The key insight is that long prompts should not be fed into the neural network directly but should instead be treated as part of the environment that the LLM can symbolically interact with."

**Current DeepSeek Implementation (v0.1.6):**
- RLM contexts are stored externally in `RlmSession`
- Only summaries are injected into the system prompt
- LLM accesses context via `rlm_exec`, `rlm_query`, and `rlm_load`

**Gap Severity:** 🟢 LOW

**Status:** ✅ Addressed

---

### 5. Missing Context Chunking Intelligence

**RLM Paper Requirement:**
> "An example strategy is to first look at the context and figure out a chunking strategy, then break up the context into smart chunks, and query an LLM per chunk with a particular question."

**Current DeepSeek Implementation (v0.1.6):**
- Fixed-size chunking (`chunk`) plus `chunk_sections`, `chunk_lines`, and `chunk_auto`
- LLM controls chunking via `rlm_exec` before issuing sub-calls
- `rlm_query auto_chunks` enables whole-document sweeps over `chunk_auto`
- No true semantic chunking (AST/function/paragraph-aware)

**Current Code (src/rlm.rs):**
```rust
pub fn chunk(&self, chunk_size: usize, overlap: usize) -> Vec<ChunkInfo> {
    // Fixed-size character-based chunking only
}
```

**Gap Severity:** 🟡 MEDIUM

**Remaining Enhancements:**
- Deeper semantic chunking (AST/function-aware) and richer metadata

---

### 6. No Buffer Variable System

**RLM Paper Requirement:**
> "Use these variables as buffers to build up your final answer... store the output of sub-LM calls over the input in variables and stitch them together."

**Current DeepSeek Implementation (v0.1.6):**
- Buffer variables are supported via `vars/get/set/append/del`
- `rlm_query` supports `store_as` + FINAL_VAR parsing to persist results
- Variables persist per context across tool calls

**Current Code (src/rlm.rs):**
```rust
pub struct RlmContext {
    pub variables: HashMap<String, String>,
    ...
}
```

**Gap Severity:** 🟢 LOW

**Status:** ✅ Addressed

---

### 7. Missing Answer Verification Pattern

**RLM Paper Requirement:**
> "We observed several instances of answer verification made by RLMs through sub-LM calls... Some of these strategies implicitly avoid context rot by using sub-LMs to perform verification."

**Current DeepSeek Implementation (v0.1.6):**
- `rlm_query` supports `mode="verify"` for explicit verification calls
- LLM can batch verification queries to cross-check answers

**Gap Severity:** 🟢 LOW

**Remaining Enhancements:**
- Optional confidence scoring or contradiction heuristics

---

### 8. No Cost-Aware Sub-Call Batching

**RLM Paper Requirement (Appendix D.1):**
> "IMPORTANT: Be very careful about using 'llm_query' as it incurs high runtime costs. Always batch as much information as reasonably possible into each call (aim for around 200k characters per call)."

**Current DeepSeek Implementation (v0.1.6):**
- Sub-call usage tracking (query count + token totals)
- Prompt guidance to batch queries and cap payload size
- `rlm_status` exposes aggregate usage stats
- Footer guardrails warn on high query/token usage

**Gap Severity:** 🟢 LOW

**Remaining Enhancements:**
- Optional hard caps or per-model budget limits

---

### 9. No Iterative REPL Loop Integration

**RLM Paper Requirement:**
> "You will be queried iteratively until you provide a final answer... Output to the REPL environment and recursive LLMs as much as possible."

**Current DeepSeek Implementation (v0.1.6):**
- Shared RLM session persists across tool calls and turns
- LLM iteratively invokes `rlm_exec`/`rlm_query` within a single turn
- FINAL / FINAL_VAR markers enforced in prompts

**Gap Severity:** 🟢 LOW

**Status:** ✅ Addressed

---

### 10. Missing Model-Specific RLM Tuning

**RLM Paper Requirement:**
> "The only difference in the prompt is an extra line... warning against using too many sub-calls... Between GPT-5 and Qwen3-Coder, we found different behavior... models are inefficient decision makers over their context."

**Current DeepSeek Implementation:**
- Single system prompt for all sub-agent types
- No model-specific tuning
- No adaptive prompting based on model behavior
- No sub-call warning mechanisms

**Gap Severity:** 🟢 LOW

**Required Implementation:**
- Model-aware prompting strategies
- Adaptive sub-call limits per model
- Behavior monitoring and correction
- Per-model cost/performance tracking

---

## Remaining Optional Components

The core RLM workflow is now implemented via tools (`rlm_load`, `rlm_exec`, `rlm_query`, `rlm_status`)
and prompt integration. The following are optional future refactors:

- **`src/rlm_engine.rs`**: central orchestration layer if RLM logic grows
- **`src/rlm_prompts.rs`**: model-specific prompt variants and tuning
- **`src/rlm_repl.rs`**: richer syntax/REPL language (current expressions are sufficient)
- **`src/tools/subagent.rs`**: nested sub-agent orchestration with shared buffers

---

## Remaining Improvements (Post-Sprint 3)

| Priority | Gap | Files to Change | Effort |
|----------|-----|-----------------|--------|
| P2 | Semantic chunking + metadata | rlm.rs | Medium |
| P2 | Budget hard caps / per-model limits | rlm.rs, tui/ui.rs | Medium |
| P3 | Nested sub-agent orchestration | tools/subagent.rs | High |
| P3 | Model-specific tuning | prompts/rlm.txt or new module | Low |

---

## Comparison Summary

| Aspect | RLM Paper | DeepSeek CLI | Gap |
|--------|-----------|-------------|-----|
| Context Handling | External variable in REPL | Externalized RLM session + prompt summary | 🟢 LOW |
| Sub-Calls | Recursive with buffers | `rlm_query` + shared buffers (no nested runtime) | 🟡 MEDIUM |
| REPL | Python REPL with llm_query | Tool-based REPL (`rlm_exec` + `rlm_query`) | 🟢 LOW |
| Output Format | FINAL/FINAL_VAR tags | Enforced in RLM prompts | 🟢 LOW |
| System Prompts | RLM-specific with examples | RLM + sub-call prompts | 🟢 LOW |
| Context Chunking | Adaptive, semantic | Fixed + section/line/auto chunking | 🟡 MEDIUM |
| Buffer Variables | Persistent across calls | Vars + store_as + FINAL_VAR | 🟢 LOW |
| Cost Tracking | Per-sub-call budgeting | Usage totals + batch guidance + UI warnings | 🟢 LOW |
| Answer Verification | Sub-LM confirmation | Verify mode in `rlm_query` | 🟢 LOW |
| Iterative Execution | Multi-turn REPL loop | Shared session across turns | 🟢 LOW |

---

## References

- Khattab, O., Kraska, A., & Zhang, A. L. (2025). Recursive Language Models. arXiv:2512.24601
- DeepSeek CLI Implementation: src/rlm.rs, src/tools/subagent.rs
