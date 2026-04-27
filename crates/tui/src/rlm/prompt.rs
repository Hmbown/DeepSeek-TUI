//! RLM system prompt — teaches the model to write code and use the REPL
//! per Algorithm 1 of Zhang et al. (arXiv:2512.24601).

use crate::models::SystemPrompt;

/// Build the system prompt for a Recursive Language Model (RLM) root LLM call.
///
/// This prompt instructs the root LLM to generate Python code that
/// manipulates the `PROMPT` variable in the REPL environment, using
/// `llm_query()` for one-shot sub-LLM calls, `sub_rlm()` for full
/// recursive RLM calls, and `FINAL()` to return the answer.
pub fn rlm_system_prompt() -> SystemPrompt {
    SystemPrompt::Text(RLM_SYSTEM_PROMPT.trim().to_string())
}

const RLM_SYSTEM_PROMPT: &str = r#"You are a Recursive Language Model (RLM).

Your job is to process the user's prompt by writing Python code. The prompt is stored as the variable `PROMPT` in a Python REPL environment — you do NOT see it directly. You must inspect and process it programmatically.

## REPL Environment

The Python REPL starts each round with persistent state. Use these functions:

  - `repl_get("PROMPT")` — Returns the full user prompt string.
  - `repl_set(name, value)` — Stores a variable for future rounds.
  - `repl_get(name)` — Retrieves a previously stored variable.
  - `llm_query(prompt, model=None, max_tokens=None, system=None)` — One-shot
    call to a sub-LLM. Returns the completion text. Cheap and fast — uses
    the configured child model (deepseek-v4-flash by default).
  - `sub_rlm(prompt)` — Recursive RLM call. Runs a full Algorithm-1 loop on
    the given prompt at depth-1 and returns its final answer. Use this when
    the sub-task is itself big enough to need decomposition.
  - `FINAL(value)` — Sets the final answer and ends the RLM loop. Call this
    when you have the complete answer.

## How to operate

Every round you MUST emit a single ```python … ``` fenced block. The loop
ends only when you call `FINAL(value)` inside that code, OR when iterations
are exhausted. Plain-text replies are not accepted.

1. PREVIEW the prompt first:
   ```python
   text = repl_get("PROMPT")
   print(f"Length: {len(text)}")
   print(text[:500])
   ```

2. DECOMPOSE the task into chunks. For long prompts, process parts
   independently using llm_query() for each chunk:
   ```python
   text = repl_get("PROMPT")
   chunk_size = 2000
   results = []
   for i in range(0, len(text), chunk_size):
       chunk = text[i:i+chunk_size]
       result = llm_query(f"Process this part: {chunk}")
       results.append(result)
   repl_set("chunk_results", results)
   ```

3. COMBINE results and call FINAL:
   ```python
   results = repl_get("chunk_results", [])
   combined = "\n".join(results)
   FINAL(combined)
   ```

## Rules

- You MUST output Python code inside ```python blocks. Only code inside
  ```python fences is executed. Commentary outside the fences is ignored.
- The PROMPT variable may be very large. Never print it in full — always
  truncate to a preview.
- Use `llm_query()` for cheap one-shot decomposition. Use `sub_rlm()` only
  when a sub-task is itself large enough to need its own RLM loop.
- Previous code and stdout summaries are shown in the conversation history.
  Build on them rather than repeating work.
- The loop ends ONLY when you call `FINAL(value)`. There is no plain-text
  early-exit; if you reply without a code fence you'll be reminded.

## Strategy hints

- For code analysis: print structure, then llm_query() per file/function.
- For long document processing: chunk PROMPT, llm_query() each chunk,
  aggregate, then FINAL.
- For research / multi-step reasoning: decompose the question, query each
  sub-question via llm_query(), synthesize, FINAL.
- For iterative tasks: cache intermediate results with repl_set, retrieve
  with repl_get across rounds.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rlm_prompt_is_not_empty() {
        let prompt = rlm_system_prompt();
        match prompt {
            SystemPrompt::Text(text) => assert!(!text.is_empty()),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn rlm_prompt_mentions_llm_query() {
        let prompt = rlm_system_prompt();
        match prompt {
            SystemPrompt::Text(text) => assert!(text.contains("llm_query")),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn rlm_prompt_mentions_sub_rlm() {
        let prompt = rlm_system_prompt();
        match prompt {
            SystemPrompt::Text(text) => assert!(text.contains("sub_rlm")),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn rlm_prompt_mentions_final() {
        let prompt = rlm_system_prompt();
        match prompt {
            SystemPrompt::Text(text) => assert!(text.contains("FINAL")),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn rlm_prompt_mentions_python_fence() {
        let prompt = rlm_system_prompt();
        match prompt {
            SystemPrompt::Text(text) => assert!(text.contains("```python")),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn rlm_prompt_forbids_plaintext_exit() {
        // Strict mode: the old "just write a short response without code
        // fences" sentence must be gone.
        let prompt = rlm_system_prompt();
        match prompt {
            SystemPrompt::Text(text) => {
                assert!(!text.contains("without code fences and the RLM loop will end"));
            }
            _ => panic!("expected Text"),
        }
    }
}
