//! 120 FPS draw-rate cap for the TUI render loop.
//!
//! Adapted from
//! [`codex-rs/tui/src/tui/frame_rate_limiter.rs`](https://github.com/openai/codex)
//! — same intent, slightly simpler since our render loop is poll-based
//! rather than scheduler-based. We only need to clamp `terminal.draw` calls
//! to a minimum interval; the existing `needs_redraw` flag already coalesces
//! multiple state mutations into one draw when several events fire between
//! polls.
//!
//! ## Why
//!
//! When the model streams a long assistant response, every SSE chunk flips
//! `App.needs_redraw = true`. Without a cap, the main loop happily redraws
//! the entire screen on every chunk — sometimes >300 frames/sec for a few
//! hundred ms of streaming. The user can't perceive frames faster than
//! ~60-120 FPS, and ratatui's diff-and-flush has real cost (wrap, style,
//! crossterm `queue!`), so this is pure waste.
//!
//! ## Behavior
//!
//! - Default state: never clamps.
//! - After `mark_emitted(t)` is called, subsequent `clamp_deadline(t')`
//!   returns `max(t', t + MIN_FRAME_INTERVAL)`.
//! - The render loop calls `clamp_deadline(now)` and:
//!   - if the result == `now`, it's safe to draw immediately.
//!   - if the result > `now`, the loop should sleep / shorten its poll
//!     timeout to wake up at exactly that instant.
//!
//! See `crates/tui/src/tui/ui.rs` (`run_app`) for the integration point.

use std::time::Duration;
use std::time::Instant;

/// 120 FPS minimum frame interval (≈8.33ms).
pub const MIN_FRAME_INTERVAL: Duration = Duration::from_nanos(8_333_334);

/// Remembers the most recent emitted draw, allowing deadlines to be clamped
/// forward so the next draw never lands sooner than `MIN_FRAME_INTERVAL`
/// after the last one.
#[derive(Debug, Default)]
pub struct FrameRateLimiter {
    last_emitted_at: Option<Instant>,
}

impl FrameRateLimiter {
    /// Returns `requested`, clamped forward if it would exceed the maximum
    /// frame rate.
    #[must_use]
    pub fn clamp_deadline(&self, requested: Instant) -> Instant {
        let Some(last_emitted_at) = self.last_emitted_at else {
            return requested;
        };
        let min_allowed = last_emitted_at
            .checked_add(MIN_FRAME_INTERVAL)
            .unwrap_or(last_emitted_at);
        requested.max(min_allowed)
    }

    /// Records that a draw was emitted at `emitted_at`.
    pub fn mark_emitted(&mut self, emitted_at: Instant) {
        self.last_emitted_at = Some(emitted_at);
    }

    /// `Some(d)` if the next draw must wait `d` from `now`. `None` if a draw
    /// is allowed right now. Used by the render loop to shorten its poll
    /// timeout so it wakes up exactly when drawing is allowed.
    #[must_use]
    pub fn time_until_next_draw(&self, now: Instant) -> Option<Duration> {
        let clamped = self.clamp_deadline(now);
        if clamped <= now {
            None
        } else {
            Some(clamped - now)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_does_not_clamp() {
        let t0 = Instant::now();
        let limiter = FrameRateLimiter::default();
        assert_eq!(limiter.clamp_deadline(t0), t0);
        assert!(limiter.time_until_next_draw(t0).is_none());
    }

    #[test]
    fn clamps_to_min_interval_since_last_emit() {
        let t0 = Instant::now();
        let mut limiter = FrameRateLimiter::default();

        assert_eq!(limiter.clamp_deadline(t0), t0);
        limiter.mark_emitted(t0);

        let too_soon = t0 + Duration::from_millis(1);
        assert_eq!(limiter.clamp_deadline(too_soon), t0 + MIN_FRAME_INTERVAL);
    }

    #[test]
    fn time_until_next_draw_reports_remaining_window() {
        let t0 = Instant::now();
        let mut limiter = FrameRateLimiter::default();
        limiter.mark_emitted(t0);

        let after_4ms = t0 + Duration::from_millis(4);
        let remaining = limiter.time_until_next_draw(after_4ms).unwrap();
        // ≈ 4.33ms remaining (8.33 - 4)
        assert!(
            remaining > Duration::from_micros(4_000) && remaining < Duration::from_millis(5),
            "expected ~4.33ms, got {remaining:?}"
        );
    }

    #[test]
    fn time_until_next_draw_none_after_interval_elapsed() {
        let t0 = Instant::now();
        let mut limiter = FrameRateLimiter::default();
        limiter.mark_emitted(t0);

        let well_past = t0 + Duration::from_millis(50);
        assert!(limiter.time_until_next_draw(well_past).is_none());
    }
}
