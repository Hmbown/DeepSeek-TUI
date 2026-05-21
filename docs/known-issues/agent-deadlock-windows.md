# Bug Report: Agent Deadlock / TUI Freeze During Multi-Turn Sessions

## Environment

- **OS**: Windows 11 (PowerShell)
- **DeepSeek TUI version**: v0.8.9 (commit 4511ea7)
- **Rust version**: (please fill)
- **Terminal**: Windows Terminal / (please fill)
- **Model**: deepseek-v4-pro
- **Workspace**: C:\project\write (Python/FastAPI project with SQLite)

## Symptoms

During extended multi-turn agent sessions, the TUI experiences complete freezes:

1. **Dynamic ripple animation stops** — the thinking indicator in the TUI freezes
2. **No tool calls complete** — the agent appears to hang mid-operation
3. **Terminal becomes unresponsive** — requires force-restart of the TUI process
4. **On restart**: the agent resumes but has lost context about what happened during the freeze
5. **Between freezes**: partial work is sometimes executed (temp scripts created, DB modifications made), suggesting the agent was mid-task when the freeze occurred

This happened **3+ times** in a single ~4-hour session working on a Python/FastAPI project.

## Reproduction Pattern

The freezes consistently occur during:
1. Multi-turn sessions (5+ turns) with long context
2. When the agent is in the middle of parallel tool calls (file reads, shell commands)
3. After `exec_shell` commands that produce garbled output (Windows console encoding issues with Chinese characters)
4. Specifically correlated with the `python -c` command failing with `SyntaxError: unterminated string literal` on Windows PowerShell

### Steps to Reproduce

1. Start a session with `deepseek-tui` on Windows 11
2. Load a medium-sized workspace (~50 files, Python/FastAPI project with Chinese content)
3. Execute multiple turns involving:
   - `git_status`, `git_diff`, `read_file` calls
   - `exec_shell` with `python -c` commands (which fail with SyntaxError on PowerShell)
   - Database queries via Python scripts
4. Continue for 5+ turns
5. The freeze becomes more likely as context grows

## Observed Logs / Error Patterns

Before each freeze, the following patterns were observed:

1. **`python -c` SyntaxError**: PowerShell mangles single-quote strings containing escaped characters:
   ```
   File "<string>", line 1
       "import
       ^
   SyntaxError: unterminated string literal (detected at line 1)
   ```

2. **Tool call timeouts**: `exec_shell` with 120s timeout for `dir /s /b C:\*.toml` timed out

3. **Console encoding**: All `exec_shell` output with Chinese characters shows garbled text (expected on Windows, but may interact poorly with the TUI's output buffer)

## Potential Root Causes (Hypotheses)

### Hypothesis 1: Async Runtime Deadlock in tui-core
The TUI's event-driven state machine may deadlock when:
- A long-running shell command blocks the task queue
- The agent tries to write to the TUI display buffer while it's in a frozen state
- The `tokio` runtime's worker threads are exhausted

**Evidence**: The ripple animation stops (rendering thread blocked), but the session resumes on restart (state not corrupted).

### Hypothesis 2: Context Window / Compaction Race Condition
- As context approaches 80%, compaction may trigger
- If the agent is mid-tool-call during compaction, a race condition could freeze the event loop

**Evidence**: Freezes consistently occur after 5+ turns (context accumulation).

### Hypothesis 3: Windows-Specific Terminal Buffer Overflow
- Windows console encoding issues with Chinese characters produce malformed output
- The TUI's output parser may not handle these edge cases, causing a buffer deadlock
- The `python -c` SyntaxError is a known Windows/PowerShell issue

**Evidence**: Every freeze was preceded by `exec_shell` commands with garbled Chinese output.

### Hypothesis 4: SQLite Concurrency in State Crate
- The session state is persisted in SQLite
- If multiple turns or sub-agents attempt concurrent writes, a SQLITE_BUSY lock may cascade into a deadlock

**Evidence**: The project being worked on uses SQLite extensively, and the TUI's own state crate also uses SQLite.

## Suggested Debugging Actions

1. **Add heartbeat logging**: Log a heartbeat every 5s from the main event loop to identify when/where it freezes
2. **Add timeout to exec_shell**: Hard-kill shell commands after N seconds regardless of state
3. **Add deadlock detection**: Use `tokio-console` or `parking_lot::deadlock` detection in debug builds
4. **Improve Windows encoding handling**: Sanitize shell output before passing to the TUI display buffer
5. **Add compaction-safe fencing**: Ensure no tool calls are in-flight when compaction starts
6. **Log agent turn boundaries**: Write turn start/end to a debug log file for post-mortem analysis

## Workarounds (For Users)

1. Restart the TUI when the ripple animation freezes
2. Split long sessions into shorter ones (3-4 turns max)
3. Avoid `python -c` on PowerShell; use temporary `.py` script files instead
4. Monitor context usage and manually `/compact` before hitting 80%

## Related Issues

- #549: CPU hang (already fixed in v0.8.8-hotfixes)
- This may be a regression or a different manifestation of the same class of bug
