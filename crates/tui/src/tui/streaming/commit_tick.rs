//! Commit-tick scheduler that drains a stream chunker according to policy.
//!
//! Bridges [`AdaptiveChunkingPolicy`] with a concrete [`StreamChunker`] queue.
//! Callers feed raw text deltas via [`StreamChunker::push_delta`], then call
//! [`run_commit_tick`] on every commit beat to obtain the next small text
//! slice to flush to the transcript on this beat.
//!
//! The chunker is the unit of streaming — one per active block (assistant /
//! thinking). Tool output is unbuffered and bypasses this path.

use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

use unicode_segmentation::UnicodeSegmentation;

use super::chunking::AdaptiveChunkingPolicy;
use super::chunking::ChunkingDecision;
use super::chunking::DrainPlan;
use super::chunking::QueueSnapshot;

const GRAPHEMES_PER_MICRO_CHUNK: usize = 1;
const CATCH_UP_MAX_MICRO_CHUNKS: usize = 12;

/// Buffers raw stream deltas and emits committed text in small display chunks.
#[derive(Debug, Default)]
pub struct StreamChunker {
    /// Bytes received but not yet split into display chunks. Normally empty;
    /// retained so `drain_remaining` has a lossless place to pull from if we
    /// ever decide to hold a tail for a future markdown-sensitive mode.
    pending: String,
    /// Small grapheme-aligned chunks waiting to be flushed to the transcript.
    queue: VecDeque<QueuedChunk>,
}

#[derive(Debug, Clone)]
struct QueuedChunk {
    text: String,
    enqueued_at: Instant,
}

impl StreamChunker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a raw model delta. Returns whether at least one new display chunk was queued.
    pub fn push_delta(&mut self, delta: &str) -> bool {
        if delta.is_empty() {
            return false;
        }
        self.pending.push_str(delta);

        let now = Instant::now();
        let committed = std::mem::take(&mut self.pending);
        let mut produced = false;
        for chunk in split_into_micro_chunks(&committed) {
            if chunk.is_empty() {
                continue;
            }
            self.queue.push_back(QueuedChunk {
                text: chunk,
                enqueued_at: now,
            });
            produced = true;
        }
        produced
    }

    /// Number of display chunks currently queued for commit.
    pub fn queued_lines(&self) -> usize {
        self.queue.len()
    }

    /// Age of the oldest queued chunk, if any.
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

    /// Drain `max_lines` queued chunks and return them as concatenated text.
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
        DrainPlan::Batch(n) => n.min(CATCH_UP_MAX_MICRO_CHUNKS),
    };

    // Drain through the chunker; an empty queue under Smooth produces "".
    let committed_text = chunker.drain_lines(max);

    CommitTickOutput {
        committed_text,
        decision,
        is_idle: chunker.is_idle(),
    }
}

/// Split text into grapheme-aligned chunks. Newlines force a boundary so
/// markdown layout still settles quickly, but prose no longer waits for a full
/// line before becoming visible.
fn split_into_micro_chunks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut graphemes = 0usize;

    for grapheme in UnicodeSegmentation::graphemes(text, true) {
        current.push_str(grapheme);
        graphemes += 1;

        if grapheme == "\n" || graphemes >= GRAPHEMES_PER_MICRO_CHUNK {
            out.push(std::mem::take(&mut current));
            graphemes = 0;
        }
    }

    if !current.is_empty() {
        out.push(current);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::streaming::chunking::ChunkingMode;

    #[test]
    fn prose_streams_before_newline() {
        let mut chunker = StreamChunker::new();
        let mut policy = AdaptiveChunkingPolicy::new();
        let now = Instant::now();

        chunker.push_delta("hello world");
        let out = run_commit_tick(&mut policy, &mut chunker, now);
        assert_eq!(out.committed_text, "h");
        assert!(!chunker.is_idle(), "remaining prose should keep dripping");

        let out = run_commit_tick(&mut policy, &mut chunker, now + Duration::from_millis(5));
        assert_eq!(out.committed_text, "e");
    }

    #[test]
    fn smooth_burst_emits_one_micro_chunk_per_tick() {
        let mut chunker = StreamChunker::new();
        let mut policy = AdaptiveChunkingPolicy::new();
        let t0 = Instant::now();

        chunker.push_delta("abc");
        // Each tick under Smooth pulls exactly one grapheme.
        let out1 = run_commit_tick(&mut policy, &mut chunker, t0);
        assert_eq!(out1.decision.mode, ChunkingMode::Smooth);
        assert_eq!(out1.committed_text, "a");
        let out2 = run_commit_tick(&mut policy, &mut chunker, t0 + Duration::from_millis(20));
        assert_eq!(out2.committed_text, "b");
        let out3 = run_commit_tick(&mut policy, &mut chunker, t0 + Duration::from_millis(40));
        assert_eq!(out3.committed_text, "c");
        assert!(out3.is_idle);
    }

    #[test]
    fn smooth_stream_keeps_combining_marks_with_base_letter() {
        let mut chunker = StreamChunker::new();
        let mut policy = AdaptiveChunkingPolicy::new();
        let t0 = Instant::now();

        chunker.push_delta("e\u{301}x");
        let out1 = run_commit_tick(&mut policy, &mut chunker, t0);
        assert_eq!(out1.committed_text, "e\u{301}");
        let out2 = run_commit_tick(&mut policy, &mut chunker, t0 + Duration::from_millis(20));
        assert_eq!(out2.committed_text, "x");
    }

    #[test]
    fn large_burst_drains_in_catch_up_without_full_jump() {
        // A large text burst arriving "at once" must trigger CatchUp on the first
        // commit tick without dumping the full backlog in one jump.
        let mut chunker = StreamChunker::new();
        let mut policy = AdaptiveChunkingPolicy::new();
        let now = Instant::now();

        let burst = "abcdefghijklmnopqrstuvwxyz".repeat(8);
        let expected_prefix: String = burst
            .chars()
            .take(CATCH_UP_MAX_MICRO_CHUNKS * GRAPHEMES_PER_MICRO_CHUNK)
            .collect();
        chunker.push_delta(&burst);
        let out = run_commit_tick(&mut policy, &mut chunker, now);
        assert_eq!(out.decision.mode, ChunkingMode::CatchUp);
        assert_eq!(out.committed_text, expected_prefix);
        assert!(!out.is_idle);
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
