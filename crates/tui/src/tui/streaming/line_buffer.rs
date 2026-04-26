//! Newline-boundary gate for streaming text.
//!
//! `LineBuffer` is an upstream-of-the-chunker safety layer that holds back any
//! text after the LAST `\n` until the next newline arrives. This prevents
//! partial multi-character markdown — most importantly partial code fences
//! (` ``` `) whose meaning flips depending on what follows on the same line —
//! from ever becoming visible state in the renderer.
//!
//! Mental model:
//! - `push(delta)`  appends raw stream text to an internal pending buffer.
//! - `take_committable()` returns only the prefix up to and including the
//!   LAST `\n` and clears that prefix. Whatever follows the last `\n` stays
//!   in the buffer for the next push.
//! - `flush()` returns whatever is left, used at end-of-stream when the model
//!   signals the turn is done. (The contract upstream of the chunker is that
//!   only complete-line text is committed; `flush()` is the explicit escape
//!   hatch when we know no more text will arrive.)
//!
//! See `cx5_chx5_newline_gate.md` in the task brief for full rationale.

/// Holds streaming text until a newline boundary is reached.
///
/// This is upstream of [`StreamChunker`](super::commit_tick::StreamChunker)
/// in the streaming pipeline:
///
/// ```text
/// raw delta -> LineBuffer.push -> take_committable -> StreamChunker.push_delta -> commit tick
/// ```
///
/// The chunker also enforces a "drain-up-to-last-newline" rule on its pending
/// buffer, but `LineBuffer` exists as a *separate* layer so that:
/// 1. The contract is explicit and locally testable.
/// 2. Future downstream consumers (e.g. live preview that renders queued lines
///    optimistically) cannot accidentally see a partial fence.
/// 3. End-of-turn flush semantics are owned by the gate, not the policy.
#[derive(Debug, Default, Clone)]
pub struct LineBuffer {
    /// Pending text not yet released because no terminating `\n` has been seen
    /// since the last commit.
    pending: String,
}

impl LineBuffer {
    /// Create an empty buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a raw delta.
    pub fn push(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        self.pending.push_str(delta);
    }

    /// Return the prefix of the pending buffer up to and including the LAST
    /// `\n`. Whatever follows that newline (if anything) stays buffered.
    ///
    /// Returns an empty string when the buffer is empty or contains no
    /// newline yet — callers can treat the empty-string case as "nothing
    /// committable on this push".
    pub fn take_committable(&mut self) -> String {
        let Some(last_nl) = self.pending.rfind('\n') else {
            return String::new();
        };
        // Drain everything up to and including the last newline. The remaining
        // tail (post-newline) stays in `pending` and is concatenated with the
        // next `push` before the next commit decision is made.
        self.pending.drain(..=last_nl).collect()
    }

    /// Return whatever is left in the buffer, even if it is not newline
    /// terminated. Used when the stream ends so we don't strand the final
    /// partial line.
    pub fn flush(&mut self) -> String {
        std::mem::take(&mut self.pending)
    }

    /// Whether the buffer holds any uncommitted text.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Length of the pending tail in bytes (testing/observability).
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Reset the buffer (e.g. on stream restart).
    pub fn reset(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_without_newline_holds_everything() {
        // Cornerstone invariant: nothing escapes the gate until a newline
        // terminates the line. This is what protects partial code fences
        // (e.g. ``` arriving in chunk N, language tag in chunk N+1).
        let mut buf = LineBuffer::new();
        buf.push("hello");
        assert_eq!(buf.take_committable(), "");
        assert_eq!(buf.pending_len(), 5);
        assert!(!buf.is_empty());
    }

    #[test]
    fn push_with_trailing_partial_returns_only_prefix() {
        let mut buf = LineBuffer::new();
        buf.push("hello\nwo");
        assert_eq!(buf.take_committable(), "hello\n");
        // Tail is held for next call.
        assert_eq!(buf.pending_len(), 2);
        assert!(!buf.is_empty());
    }

    #[test]
    fn next_push_is_concatenated_with_held_tail() {
        let mut buf = LineBuffer::new();
        buf.push("hello\nwo");
        assert_eq!(buf.take_committable(), "hello\n");
        // The held "wo" is concatenated with "rld\n", and the whole line
        // becomes committable.
        buf.push("rld\n");
        assert_eq!(buf.take_committable(), "world\n");
        assert!(buf.is_empty());
    }

    #[test]
    fn flush_returns_unterminated_tail() {
        let mut buf = LineBuffer::new();
        buf.push("trailing without newline");
        // No newline → nothing committable.
        assert_eq!(buf.take_committable(), "");
        // End-of-stream flush returns it raw.
        assert_eq!(buf.flush(), "trailing without newline");
        assert!(buf.is_empty());
    }

    #[test]
    fn flush_is_empty_when_buffer_drained() {
        let mut buf = LineBuffer::new();
        buf.push("a\n");
        assert_eq!(buf.take_committable(), "a\n");
        assert_eq!(buf.flush(), "");
    }

    #[test]
    fn multi_line_burst_returns_prefix_through_last_newline() {
        // Multiple newlines in one push: the entire prefix up through the
        // last newline is committable in one go; only the unterminated tail
        // is held.
        let mut buf = LineBuffer::new();
        buf.push("a\nb\nc\nd");
        assert_eq!(buf.take_committable(), "a\nb\nc\n");
        assert_eq!(buf.pending_len(), 1);
        // Finishing "d" with a newline releases it on the next take.
        buf.push("\n");
        assert_eq!(buf.take_committable(), "d\n");
    }

    #[test]
    fn partial_code_fence_never_escapes_the_gate() {
        // Acceptance scenario from CX#5: a fenced code block whose opener
        // arrives split across deltas must never expose "foo```rust" without
        // a terminating newline. We assert that on every intermediate
        // commit, the *committed* text either contains a newline or is empty
        // — i.e. the pre-language partial fence never leaks.
        let mut buf = LineBuffer::new();

        // Chunk 1: a paragraph fragment ending with the fence opener.
        buf.push("foo```");
        let c1 = buf.take_committable();
        assert!(
            c1.is_empty() || c1.ends_with('\n'),
            "partial fence leaked: {c1:?}"
        );
        assert!(
            !c1.contains("foo```"),
            "fence opener escaped without newline: {c1:?}"
        );

        // Chunk 2: language tag + start of body. The fence line is now
        // newline-terminated, so it can commit; the post-newline body is
        // held.
        buf.push("rust\nlet x");
        let c2 = buf.take_committable();
        assert!(
            c2.ends_with('\n'),
            "expected newline-terminated commit: {c2:?}"
        );
        assert_eq!(c2, "foo```rust\n");

        // Chunk 3: rest of body and the fence closer.
        buf.push("= 1;\n```\n");
        let c3 = buf.take_committable();
        assert_eq!(c3, "let x= 1;\n```\n");
        assert!(buf.is_empty());
    }

    #[test]
    fn empty_push_is_a_noop() {
        let mut buf = LineBuffer::new();
        buf.push("");
        assert!(buf.is_empty());
        assert_eq!(buf.take_committable(), "");
    }

    #[test]
    fn reset_clears_pending_tail() {
        let mut buf = LineBuffer::new();
        buf.push("partial");
        assert_eq!(buf.pending_len(), 7);
        buf.reset();
        assert!(buf.is_empty());
        assert_eq!(buf.flush(), "");
    }
}
