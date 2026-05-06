//! Commit-tick scheduler that drains a stream chunker according to policy.
//!
//! Bridges [`AdaptiveChunkingPolicy`] with a concrete [`StreamChunker`] queue.
//! Callers feed raw text deltas via [`StreamChunker::push_delta`] (only
//! newline-terminated text becomes queued lines), then call
//! [`run_commit_tick`] on every commit beat to obtain the text safe to flush
//! to the transcript on this beat.
//!
//! The chunker is the unit of streaming — one per active block (assistant /
//! thinking). Tool output is unbuffered and bypasses this path.

use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

use super::chunking::AdaptiveChunkingPolicy;
use super::chunking::ChunkingDecision;
use super::chunking::DrainPlan;
use super::chunking::QueueSnapshot;

/// Buffers raw stream deltas and emits committed text in line units.
///
/// Only the substring up to the *last* `\n` is committed; trailing partial
/// content stays in the buffer. This is what protects partial code fences
/// (` ``` `) and other line-sensitive markdown from rendering mid-state.
#[derive(Debug, Default)]
pub struct StreamChunker {
    /// Bytes received but not yet split into a complete line.
    pending: String,
    /// Complete lines waiting to be flushed to the transcript.
    /// Each entry preserves its trailing `\n` so reassembly is lossless.
    queue: VecDeque<QueuedLine>,
}

#[derive(Debug, Clone)]
struct QueuedLine {
    text: String,
    enqueued_at: Instant,
}

impl StreamChunker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a raw model delta. Returns whether at least one new line was queued.
    pub fn push_delta(&mut self, delta: &str) -> bool {
        if delta.is_empty() {
            return false;
        }
        self.pending.push_str(delta);

        let Some(last_nl) = self.pending.rfind('\n') else {
            return false;
        };

        // Drain everything up to and including the last newline into queued lines.
        // Splitting by line keeps the chunker source-agnostic and lets the policy
        // count "lines waiting" without peeking at text content.
        let now = Instant::now();
        let committed: String = self.pending.drain(..=last_nl).collect();
        let mut produced = false;
        for chunk in split_lines_keep_terminator(&committed) {
            if chunk.is_empty() {
                continue;
            }
            self.queue.push_back(QueuedLine {
                text: chunk,
                enqueued_at: now,
            });
            produced = true;
        }
        produced
    }

    /// Number of complete lines currently queued for commit.
    pub fn queued_lines(&self) -> usize {
        self.queue.len()
    }

    /// Age of the oldest queued line, if any.
    pub fn oldest_queued_age(&self, now: Instant) -> Option<Duration> {
        self.queue
            .front()
            .map(|q| now.saturating_duration_since(q.enqueued_at))
    }

    /// Whether the queue is empty AND no buffered partial line remains.
    pub fn is_idle(&self) -> bool {
        self.queue.is_empty() && self.pending.is_empty()
    }

    /// Snapshot for policy decisions.
    pub fn snapshot(&self, now: Instant) -> QueueSnapshot {
        QueueSnapshot {
            queued_lines: self.queue.len(),
            oldest_age: self.oldest_queued_age(now),
        }
    }

    /// Drain `max_lines` complete lines and return them as concatenated text.
    pub fn drain_lines(&mut self, max_lines: usize) -> String {
        let n = max_lines.min(self.queue.len());
        let mut out = String::new();
        for queued in self.queue.drain(..n) {
            out.push_str(&queued.text);
        }
        out
    }

    /// Drain any remaining pending bytes (called at stream finalize).
    /// This includes both queued complete lines AND the tail partial line.
    pub fn drain_remaining(&mut self) -> String {
        let mut out = String::new();
        while let Some(q) = self.queue.pop_front() {
            out.push_str(&q.text);
        }
        if !self.pending.is_empty() {
            out.push_str(&self.pending);
            self.pending.clear();
        }
        out
    }

    /// Reset internal state.
    pub fn reset(&mut self) {
        self.pending.clear();
        self.queue.clear();
    }
}

/// One commit-tick decision plus the text that should be flushed on this tick.
pub struct CommitTickOutput {
    pub committed_text: String,
    pub decision: ChunkingDecision,
    pub is_idle: bool,
}

/// Run a single commit tick: ask the policy, drain the chunker accordingly.
pub fn run_commit_tick(
    policy: &mut AdaptiveChunkingPolicy,
    chunker: &mut StreamChunker,
    now: Instant,
) -> CommitTickOutput {
    let snapshot = chunker.snapshot(now);
    let prior_mode = policy.mode();
    let decision = policy.decide(snapshot, now);

    if decision.mode != prior_mode {
        tracing::trace!(
            prior_mode = ?prior_mode,
            new_mode = ?decision.mode,
            queued_lines = snapshot.queued_lines,
            oldest_queued_age_ms = snapshot.oldest_age.map(|age| age.as_millis() as u64),
            entered_catch_up = decision.entered_catch_up,
            "stream chunking mode transition"
        );
    }

    let max = match decision.drain_plan {
        DrainPlan::Single => 1,
        DrainPlan::Batch(n) => n,
    };

    // Drain through the chunker; an empty queue under Smooth produces "".
    let committed_text = chunker.drain_lines(max);

    CommitTickOutput {
        committed_text,
        decision,
        is_idle: chunker.is_idle(),
    }
}

/// Split text into chunks, preserving each terminator. The final chunk is
/// included only if it ends with `\n` (this is enforced upstream in
/// `push_delta`, which only drains up through the last newline).
fn split_lines_keep_terminator(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            out.push(text[start..=i].to_string());
            start = i + 1;
        }
    }
    if start < text.len() {
        // This branch is unreachable for inputs produced by `push_delta`,
        // but stays defensive for direct callers.
        out.push(text[start..].to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::streaming::chunking::ChunkingMode;

    #[test]
    fn partial_code_fence_is_held_until_newline() {
        // Without the chunker, a stray ``` arriving mid-stream could render as
        // an opened fence. The chunker must not commit anything until the
        // line is terminated by `\n`.
        let mut chunker = StreamChunker::new();
        let mut policy = AdaptiveChunkingPolicy::new();
        let now = Instant::now();

        // Partial fence + content; no newline → nothing committed yet.
        chunker.push_delta("Here is code:\n");
        chunker.push_delta("```");
        let out = run_commit_tick(&mut policy, &mut chunker, now);
        assert_eq!(out.committed_text, "Here is code:\n");
        assert!(!chunker.is_idle(), "partial fence still buffered");

        // Close the fence line.
        chunker.push_delta("rust\n");
        let out = run_commit_tick(&mut policy, &mut chunker, now + Duration::from_millis(5));
        assert_eq!(out.committed_text, "```rust\n");
    }

    #[test]
    fn smooth_burst_emits_one_line_per_tick() {
        let mut chunker = StreamChunker::new();
        let mut policy = AdaptiveChunkingPolicy::new();
        let t0 = Instant::now();

        chunker.push_delta("a\nb\nc\n");
        // Each tick under Smooth pulls exactly one line.
        let out1 = run_commit_tick(&mut policy, &mut chunker, t0);
        assert_eq!(out1.decision.mode, ChunkingMode::Smooth);
        assert_eq!(out1.committed_text, "a\n");
        let out2 = run_commit_tick(&mut policy, &mut chunker, t0 + Duration::from_millis(20));
        assert_eq!(out2.committed_text, "b\n");
        let out3 = run_commit_tick(&mut policy, &mut chunker, t0 + Duration::from_millis(40));
        assert_eq!(out3.committed_text, "c\n");
        assert!(out3.is_idle);
    }

    #[test]
    fn large_burst_drains_in_catch_up() {
        // Eight lines arriving "at once" must trigger CatchUp on the first
        // commit tick and drain the full backlog in one go.
        let mut chunker = StreamChunker::new();
        let mut policy = AdaptiveChunkingPolicy::new();
        let now = Instant::now();

        let burst = "1\n2\n3\n4\n5\n6\n7\n8\n";
        chunker.push_delta(burst);
        let out = run_commit_tick(&mut policy, &mut chunker, now);
        assert_eq!(out.decision.mode, ChunkingMode::CatchUp);
        assert_eq!(out.committed_text, burst);
        assert!(out.is_idle);
    }

    #[test]
    fn finalize_drains_partial_tail() {
        // The final, possibly-incomplete line must be flushed by drain_remaining.
        let mut chunker = StreamChunker::new();
        chunker.push_delta("done\nno-newline-here");
        let drained = chunker.drain_remaining();
        assert_eq!(drained, "done\nno-newline-here");
        assert!(chunker.is_idle());
    }
}
