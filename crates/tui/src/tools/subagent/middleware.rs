//! Agent middleware system — composable hooks that intercept sub-agent
//! lifecycle events (spawn, step, finish). Ordered via `@Next` / `@Prev`
//! anchor groups so middlewares can insert themselves at natural positions
//! in the execution pipeline.
//!
//! # Anchor system
//!
//! Each middleware declares one of:
//! - `MiddlewareAnchor::Next` — runs in the "next" group (default, typical
//!   for observation/logging middlewares).
//! - `MiddlewareAnchor::Prev` — runs in the "prev" group (typically for
//!   enforcement/filtering middlewares that should run first).
//! - `MiddlewareAnchor::Named(s)` — a named anchor; another middleware can
//!   reference this to order itself relative to a known point.
//!
//! Execution order: `Prev` group → `Next` group, alphabetically within each
//! group. Named anchors are sorted with their group by name.
//!
//! # Default hooks (all no-ops)
//!
//! Implementors override only the hooks they need. Hooks that return
//! `Err(...)` short-circuit: the chain stops and the error propagates
//! to the caller.

use anyhow::Result;
use async_trait::async_trait;

use super::SubAgentStatus;

// ── Anchor ──────────────────────────────────────────────────────────────────

/// Where a middleware sits in the pipeline order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MiddlewareAnchor {
    /// Runs in the "prev" group — before Next-group middlewares.
    Prev,
    /// Runs in the "next" group — after Prev-group middlewares.
    Next,
    /// A named anchor for targeted relative ordering.
    Named(String),
}

impl MiddlewareAnchor {
    /// Group key for sorting: 0 = Prev, 1 = Next (Named treated as Next).
    fn group(&self) -> u8 {
        match self {
            Self::Prev => 0,
            Self::Next | Self::Named(_) => 1,
        }
    }
}

// ── Middleware context ──────────────────────────────────────────────────────

/// Mutable context passed to each middleware hook invocation.
/// Carries arbitrary key-value data so middlewares can leave
/// breadcrumbs for later middlewares in the same chain pass.
#[derive(Debug, Default)]
pub struct MiddlewareContext {
    data: std::collections::HashMap<String, String>,
}

impl MiddlewareContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.data.insert(key.into(), value.into());
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.data.get(key).map(String::as_str)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

// ── Middleware trait ────────────────────────────────────────────────────────

/// Composable hook that intercepts sub-agent lifecycle events.
///
/// All hooks default to no-ops. Implementors override the hooks they
/// care about. Returning `Err(...)` short-circuits the chain.
#[async_trait]
pub trait AgentMiddleware: Send + Sync {
    /// Human-readable name for diagnostics/logging.
    fn name(&self) -> &'static str;

    /// Where this middleware sits in the pipeline order.
    fn anchor(&self) -> MiddlewareAnchor {
        MiddlewareAnchor::Next
    }

    /// Called when a sub-agent is about to spawn. The chain runs this
    /// before the agent enters its first step.
    async fn on_spawn(
        &self,
        _agent_id: &str,
        _agent_type: &str,
        _ctx: &mut MiddlewareContext,
    ) -> Result<()> {
        Ok(())
    }

    /// Called after each agent loop step completes (including the final
    /// tool call before finish).
    async fn on_step(
        &self,
        _agent_id: &str,
        _step: u32,
        _ctx: &mut MiddlewareContext,
    ) -> Result<()> {
        Ok(())
    }

    /// Called when the agent reaches a terminal state (Completed, Failed,
    /// Interrupted, or Cancelled). Runs after the final `on_step`.
    async fn on_finish(
        &self,
        _agent_id: &str,
        _status: &SubAgentStatus,
        _ctx: &mut MiddlewareContext,
    ) -> Result<()> {
        Ok(())
    }
}

// ── Chain ───────────────────────────────────────────────────────────────────

/// Ordered collection of middlewares that fire in sequence per event.
///
/// # Examples
///
/// ```
/// use deepseek_tui::tools::subagent::middleware::{
///     AgentMiddleware, MiddlewareAnchor, MiddlewareChain, MiddlewareContext,
/// };
/// use deepseek_tui::tools::subagent::SubAgentStatus;
/// use async_trait::async_trait;
///
/// struct Logger;
/// #[async_trait]
/// impl AgentMiddleware for Logger {
///     fn name(&self) -> &'static str { "logger" }
///     fn anchor(&self) -> MiddlewareAnchor { MiddlewareAnchor::Next }
/// }
///
/// let chain = MiddlewareChain::new(vec![Box::new(Logger)]);
/// ```
pub struct MiddlewareChain {
    middlewares: Vec<Box<dyn AgentMiddleware>>,
}

impl std::fmt::Debug for MiddlewareChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MiddlewareChain")
            .field("count", &self.middlewares.len())
            .field(
                "names",
                &self
                    .middlewares
                    .iter()
                    .map(|m| m.name())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Clone for MiddlewareChain {
    fn clone(&self) -> Self {
        // Can't clone Box<dyn AgentMiddleware> generically, so we
        // return an empty chain. MiddlewareChain is lightweight and
        // the caller should reconstruct from the source if needed.
        Self::default()
    }
}

impl Default for MiddlewareChain {
    fn default() -> Self {
        Self::empty()
    }
}

impl MiddlewareChain {
    /// Create an empty chain.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            middlewares: Vec::new(),
        }
    }

    /// Create a chain from a vec of middlewares. They are sorted by
    /// anchor group (Prev → Next) then alphabetically by name within
    /// each group.
    #[must_use]
    pub fn new(middlewares: Vec<Box<dyn AgentMiddleware>>) -> Self {
        let mut chain = Self { middlewares };
        chain.sort();
        chain
    }

    /// Add a middleware to the chain and re-sort.
    pub fn add(&mut self, mw: Box<dyn AgentMiddleware>) {
        self.middlewares.push(mw);
        self.sort();
    }

    /// Remove a middleware by name. Returns true if one was removed.
    pub fn remove(&mut self, name: &str) -> bool {
        let len_before = self.middlewares.len();
        self.middlewares.retain(|m| m.name() != name);
        self.middlewares.len() < len_before
    }

    /// Number of middlewares in the chain.
    #[must_use]
    pub fn len(&self) -> usize {
        self.middlewares.len()
    }

    /// Whether the chain is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.middlewares.is_empty()
    }

    /// Sort middlewares by anchor group then name.
    fn sort(&mut self) {
        self.middlewares.sort_by(|a, b| {
            a.anchor()
                .group()
                .cmp(&b.anchor().group())
                .then_with(|| a.name().cmp(b.name()))
        });
    }

    // ── Fire methods ──────────────────────────────────────────────────

    /// Fire `on_spawn` on every middleware in chain order.
    /// Short-circuits on first error.
    pub async fn fire_on_spawn(
        &self,
        agent_id: &str,
        agent_type: &str,
        ctx: &mut MiddlewareContext,
    ) -> Result<()> {
        for mw in &self.middlewares {
            mw.on_spawn(agent_id, agent_type, ctx).await?;
        }
        Ok(())
    }

    /// Fire `on_step` on every middleware in chain order.
    /// Short-circuits on first error.
    pub async fn fire_on_step(
        &self,
        agent_id: &str,
        step: u32,
        ctx: &mut MiddlewareContext,
    ) -> Result<()> {
        for mw in &self.middlewares {
            mw.on_step(agent_id, step, ctx).await?;
        }
        Ok(())
    }

    /// Fire `on_finish` on every middleware in chain order.
    /// Short-circuits on first error.
    pub async fn fire_on_finish(
        &self,
        agent_id: &str,
        status: &SubAgentStatus,
        ctx: &mut MiddlewareContext,
    ) -> Result<()> {
        for mw in &self.middlewares {
            mw.on_finish(agent_id, status, ctx).await?;
        }
        Ok(())
    }
}

// ── Built-in middlewares ────────────────────────────────────────────────────

/// Compaction guard middleware: tracks token usage and signals when context
/// exceeds a configurable threshold.
///
/// On each `on_step`, checks whether the estimated token count exceeds
/// `threshold` × `context_window`. When the threshold is crossed, sets
/// the `"compaction_needed"` key in [`MiddlewareContext`] to `"true"`.
///
/// The engine or parent agent is expected to check this flag after each
/// step and trigger compaction (e.g., via `/compact`).
///
/// # Thread safety
///
/// The token counter is an `Arc<AtomicU64>` shared between the middleware
/// and the caller. The caller updates it after each API response; the
/// middleware reads it on each step.
pub struct CompactionGuard {
    /// Shared token usage counter (prompt + completion tokens).
    token_count: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// The model's context window size in tokens (e.g. 1_048_576 for V4).
    context_window: u64,
    /// Fraction of context_window that triggers the guard (0.0–1.0).
    /// Default 0.80 = 80%.
    threshold: f64,
}

impl CompactionGuard {
    /// Create a new compaction guard.
    ///
    /// - `token_count`: shared counter the caller updates after each API response.
    /// - `context_window`: model context window size in tokens.
    /// - `threshold`: fraction of context_window that triggers the guard (e.g. 0.80).
    #[must_use]
    pub fn new(
        token_count: std::sync::Arc<std::sync::atomic::AtomicU64>,
        context_window: u64,
        threshold: f64,
    ) -> Self {
        Self {
            token_count,
            context_window,
            threshold: threshold.clamp(0.0, 1.0),
        }
    }

    /// Current usage as a fraction of the context window.
    #[must_use]
    pub fn usage_ratio(&self) -> f64 {
        let used = self.token_count.load(std::sync::atomic::Ordering::Relaxed) as f64;
        if self.context_window == 0 {
            return 0.0;
        }
        used / self.context_window as f64
    }

    /// Whether the threshold has been crossed.
    #[must_use]
    pub fn is_over_threshold(&self) -> bool {
        self.usage_ratio() >= self.threshold
    }
}

impl std::fmt::Debug for CompactionGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompactionGuard")
            .field(
                "token_count",
                &self.token_count.load(std::sync::atomic::Ordering::Relaxed),
            )
            .field("context_window", &self.context_window)
            .field("threshold", &self.threshold)
            .field("usage_ratio", &self.usage_ratio())
            .finish()
    }
}

#[async_trait]
impl AgentMiddleware for CompactionGuard {
    fn name(&self) -> &'static str {
        "compaction_guard"
    }

    fn anchor(&self) -> MiddlewareAnchor {
        MiddlewareAnchor::Prev
    }

    async fn on_step(&self, agent_id: &str, step: u32, ctx: &mut MiddlewareContext) -> Result<()> {
        if self.is_over_threshold() {
            let ratio = self.usage_ratio();
            ctx.set("compaction_needed", "true");
            ctx.set("compaction_usage_pct", format!("{:.0}", ratio * 100.0));
            tracing::warn!(
                agent_id = %agent_id,
                step = step,
                usage_pct = %format!("{:.0}", ratio * 100.0),
                threshold_pct = %format!("{:.0}", self.threshold * 100.0),
                "compaction threshold crossed"
            );
        } else {
            // Clear in case it was set by a previous check that's now stale.
            ctx.set("compaction_needed", "false");
        }
        Ok(())
    }
}

// ── Context handoff ─────────────────────────────────────────────────────────
// #664: Context-limit handoff as a replacement for routine compaction.
//
// When the CompactionGuard signals that context is near the limit, the
// engine can request a structured handoff prompt. The successor agent
// receives this prompt as a clean cache prefix — preserving V4's ~90%
// prefix-cache discount instead of paying the 10× cost of compaction.

use serde::{Deserialize, Serialize};

/// Structured handoff prompt for agent context transfer.
///
/// Captures everything a successor agent needs to continue the work
/// without replaying the full conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffPrompt {
    /// High-level summary of what's been accomplished.
    pub summary: String,
    /// Current plan state: completed steps, in-progress steps.
    pub progress: HandoffProgress,
    /// Open questions or blockers that the successor must address.
    pub blockers: Vec<String>,
    /// Open loops — tasks that were started but not completed.
    pub open_loops: Vec<String>,
    /// Key files and their current state.
    pub working_set: Vec<String>,
    /// The model that was being used.
    pub model: String,
    /// Timestamp when the handoff was created.
    pub timestamp: String,
    /// Token count at handoff time.
    pub tokens_used: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffProgress {
    /// Steps that have been completed.
    pub completed: Vec<String>,
    /// Steps currently in progress.
    pub in_progress: Vec<String>,
    /// Steps not yet started.
    pub pending: Vec<String>,
    /// Overall completion percentage.
    pub pct_complete: u8,
}

impl HandoffPrompt {
    /// Create a new handoff prompt with the given summary.
    #[must_use]
    pub fn new(summary: impl Into<String>, model: impl Into<String>, tokens_used: u64) -> Self {
        Self {
            summary: summary.into(),
            progress: HandoffProgress {
                completed: Vec::new(),
                in_progress: Vec::new(),
                pending: Vec::new(),
                pct_complete: 0,
            },
            blockers: Vec::new(),
            open_loops: Vec::new(),
            working_set: Vec::new(),
            model: model.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            tokens_used,
        }
    }

    /// Render the handoff prompt as a system-prompt-ready string.
    ///
    /// This is what gets seeded into the successor agent's first message.
    /// The format is designed to be a stable cache prefix — identical
    /// handoffs produce identical prefixes for V4 prefix-cache reuse.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("<handoff>\n");
        out.push_str(&format!(
            "timestamp: {}\nmodel: {}\ntokens_used: {}\n",
            self.timestamp, self.model, self.tokens_used
        ));
        out.push_str(&format!("\n## Summary\n{}\n", self.summary));

        if !self.progress.completed.is_empty() || self.progress.pct_complete > 0 {
            out.push_str(&format!(
                "\n## Progress ({pct}%)\n",
                pct = self.progress.pct_complete
            ));
            if !self.progress.completed.is_empty() {
                out.push_str("### Completed\n");
                for step in &self.progress.completed {
                    out.push_str(&format!("- [x] {step}\n"));
                }
            }
            if !self.progress.in_progress.is_empty() {
                out.push_str("### In Progress\n");
                for step in &self.progress.in_progress {
                    out.push_str(&format!("- [~] {step}\n"));
                }
            }
            if !self.progress.pending.is_empty() {
                out.push_str("### Pending\n");
                for step in &self.progress.pending {
                    out.push_str(&format!("- [ ] {step}\n"));
                }
            }
        }

        if !self.blockers.is_empty() {
            out.push_str("\n## Blockers\n");
            for blocker in &self.blockers {
                out.push_str(&format!("- {blocker}\n"));
            }
        }

        if !self.open_loops.is_empty() {
            out.push_str("\n## Open Loops\n");
            for loop_item in &self.open_loops {
                out.push_str(&format!("- {loop_item}\n"));
            }
        }

        if !self.working_set.is_empty() {
            out.push_str("\n## Working Set\n");
            for file in &self.working_set {
                out.push_str(&format!("- {file}\n"));
            }
        }

        out.push_str("\n## Instructions\n");
        out.push_str(
            "You are a successor agent continuing work from a prior session.\n\
             Review the summary, progress, and open loops above.\n\
             Continue from where the prior agent left off.\n\
             Do not re-do completed work.\n",
        );
        out.push_str("</handoff>");
        out
    }

    /// Builder: set summary.
    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }

    /// Builder: add a completed step.
    pub fn add_completed(mut self, step: impl Into<String>) -> Self {
        self.progress.completed.push(step.into());
        self
    }

    /// Builder: add an in-progress step.
    pub fn add_in_progress(mut self, step: impl Into<String>) -> Self {
        self.progress.in_progress.push(step.into());
        self
    }

    /// Builder: add a pending step.
    pub fn add_pending(mut self, step: impl Into<String>) -> Self {
        self.progress.pending.push(step.into());
        self
    }

    /// Builder: add a blocker.
    pub fn add_blocker(mut self, blocker: impl Into<String>) -> Self {
        self.blockers.push(blocker.into());
        self
    }

    /// Builder: add an open loop.
    pub fn add_open_loop(mut self, loop_item: impl Into<String>) -> Self {
        self.open_loops.push(loop_item.into());
        self
    }

    /// Builder: add a working-set file.
    pub fn add_file(mut self, file: impl Into<String>) -> Self {
        self.working_set.push(file.into());
        self
    }

    /// Builder: set progress percentage.
    pub fn with_pct_complete(mut self, pct: u8) -> Self {
        self.progress.pct_complete = pct;
        self
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    struct TestMw {
        name: &'static str,
        anchor: MiddlewareAnchor,
        calls: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl TestMw {
        fn new(
            name: &'static str,
            anchor: MiddlewareAnchor,
            calls: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        ) -> Self {
            Self {
                name,
                anchor,
                calls,
            }
        }
    }

    #[async_trait]
    impl AgentMiddleware for TestMw {
        fn name(&self) -> &'static str {
            self.name
        }

        fn anchor(&self) -> MiddlewareAnchor {
            self.anchor.clone()
        }

        async fn on_spawn(
            &self,
            agent_id: &str,
            agent_type: &str,
            _ctx: &mut MiddlewareContext,
        ) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("spawn:{}:{}:{}", self.name, agent_id, agent_type));
            Ok(())
        }

        async fn on_step(
            &self,
            agent_id: &str,
            step: u32,
            _ctx: &mut MiddlewareContext,
        ) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("step:{}:{}:{}", self.name, agent_id, step));
            Ok(())
        }

        async fn on_finish(
            &self,
            agent_id: &str,
            _status: &SubAgentStatus,
            _ctx: &mut MiddlewareContext,
        ) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("finish:{}:{}", self.name, agent_id));
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_empty_chain_noops() {
        let chain = MiddlewareChain::empty();
        let mut ctx = MiddlewareContext::new();
        assert!(chain.fire_on_spawn("a1", "general", &mut ctx).await.is_ok());
        assert!(chain.fire_on_step("a1", 1, &mut ctx).await.is_ok());
        assert!(
            chain
                .fire_on_finish("a1", &SubAgentStatus::Completed, &mut ctx)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_ordering_prev_before_next() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let a = TestMw::new("a", MiddlewareAnchor::Next, calls.clone());
        let b = TestMw::new("b", MiddlewareAnchor::Prev, calls.clone());

        let chain = MiddlewareChain::new(vec![Box::new(a), Box::new(b)]);
        let mut ctx = MiddlewareContext::new();

        chain.fire_on_spawn("x", "g", &mut ctx).await.unwrap();

        let recorded = calls.lock().unwrap();
        // b (Prev) fires before a (Next)
        assert!(recorded[0].starts_with("spawn:b:"), "got: {recorded:?}");
        assert!(recorded[1].starts_with("spawn:a:"), "got: {recorded:?}");
    }

    #[tokio::test]
    async fn test_short_circuit_on_error() {
        struct FailingMw;
        #[async_trait]
        impl AgentMiddleware for FailingMw {
            fn name(&self) -> &'static str {
                "failing"
            }
            async fn on_spawn(
                &self,
                _agent_id: &str,
                _agent_type: &str,
                _ctx: &mut MiddlewareContext,
            ) -> Result<()> {
                anyhow::bail!("intentional failure")
            }
        }

        struct NeverCalled;
        #[async_trait]
        impl AgentMiddleware for NeverCalled {
            fn name(&self) -> &'static str {
                "never"
            }
        }

        let chain = MiddlewareChain::new(vec![Box::new(FailingMw), Box::new(NeverCalled)]);
        let mut ctx = MiddlewareContext::new();

        let result = chain.fire_on_spawn("a", "g", &mut ctx).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("intentional failure")
        );
    }

    #[tokio::test]
    async fn test_context_passthrough() {
        struct Setter;
        #[async_trait]
        impl AgentMiddleware for Setter {
            fn name(&self) -> &'static str {
                "setter"
            }
            fn anchor(&self) -> MiddlewareAnchor {
                MiddlewareAnchor::Prev
            }
            async fn on_spawn(
                &self,
                _agent_id: &str,
                _agent_type: &str,
                ctx: &mut MiddlewareContext,
            ) -> Result<()> {
                ctx.set("key", "from_setter");
                Ok(())
            }
        }

        struct Getter {
            calls: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        }
        #[async_trait]
        impl AgentMiddleware for Getter {
            fn name(&self) -> &'static str {
                "getter"
            }
            async fn on_spawn(
                &self,
                _agent_id: &str,
                _agent_type: &str,
                ctx: &mut MiddlewareContext,
            ) -> Result<()> {
                self.calls
                    .lock()
                    .unwrap()
                    .push(ctx.get("key").unwrap_or("missing").to_string());
                Ok(())
            }
        }

        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let chain = MiddlewareChain::new(vec![
            Box::new(Setter),
            Box::new(Getter {
                calls: calls.clone(),
            }),
        ]);
        let mut ctx = MiddlewareContext::new();
        chain.fire_on_spawn("a", "g", &mut ctx).await.unwrap();

        assert_eq!(calls.lock().unwrap()[0], "from_setter");
    }

    #[tokio::test]
    async fn test_remove_by_name() {
        struct A;
        #[async_trait]
        impl AgentMiddleware for A {
            fn name(&self) -> &'static str {
                "A"
            }
        }
        struct B;
        #[async_trait]
        impl AgentMiddleware for B {
            fn name(&self) -> &'static str {
                "B"
            }
        }

        let mut chain = MiddlewareChain::new(vec![Box::new(A), Box::new(B)]);
        assert_eq!(chain.len(), 2);
        assert!(chain.remove("A"));
        assert_eq!(chain.len(), 1);
        assert!(!chain.remove("C"));
        assert_eq!(chain.len(), 1);
    }

    // ── CompactionGuard tests ────────────────────────────────────────

    #[tokio::test]
    async fn test_compaction_guard_sets_flag_when_over_threshold() {
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(900_000));
        let guard = CompactionGuard::new(counter, 1_000_000, 0.80);

        let mut ctx = MiddlewareContext::new();
        guard.on_step("agent_a", 42, &mut ctx).await.unwrap();

        assert_eq!(ctx.get("compaction_needed"), Some("true"));
        assert_eq!(ctx.get("compaction_usage_pct"), Some("90"));
    }

    #[tokio::test]
    async fn test_compaction_guard_clears_flag_when_under_threshold() {
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(500_000));
        let guard = CompactionGuard::new(counter, 1_000_000, 0.80);

        let mut ctx = MiddlewareContext::new();
        guard.on_step("agent_a", 1, &mut ctx).await.unwrap();

        assert_eq!(ctx.get("compaction_needed"), Some("false"));
    }

    #[test]
    fn test_compaction_guard_usage_ratio_at_boundaries() {
        let c = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let guard = CompactionGuard::new(c.clone(), 1_000_000, 0.80);
        assert!((guard.usage_ratio() - 0.0).abs() < f64::EPSILON);
        assert!(!guard.is_over_threshold());

        c.store(800_000, std::sync::atomic::Ordering::Relaxed);
        assert!((guard.usage_ratio() - 0.80).abs() < f64::EPSILON);
        assert!(guard.is_over_threshold());

        c.store(799_999, std::sync::atomic::Ordering::Relaxed);
        assert!(guard.usage_ratio() < 0.80);
        assert!(!guard.is_over_threshold());
    }

    #[test]
    fn test_compaction_guard_clamps_threshold() {
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let guard = CompactionGuard::new(counter, 1_000_000, 1.5);
        assert!((guard.threshold - 1.0).abs() < f64::EPSILON);

        let guard = CompactionGuard::new(
            std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            1_000_000,
            -0.1,
        );
        assert!((guard.threshold - 0.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_compaction_guard_as_middleware_in_chain() {
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(850_000));
        let guard = CompactionGuard::new(counter, 1_000_000, 0.80);

        struct Observer {
            saw_compaction: std::sync::Arc<std::sync::atomic::AtomicBool>,
        }
        #[async_trait]
        impl AgentMiddleware for Observer {
            fn name(&self) -> &'static str {
                "observer"
            }
            async fn on_step(
                &self,
                _id: &str,
                _step: u32,
                ctx: &mut MiddlewareContext,
            ) -> Result<()> {
                if ctx.get("compaction_needed") == Some("true") {
                    self.saw_compaction
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                }
                Ok(())
            }
        }

        let saw = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let chain = MiddlewareChain::new(vec![
            Box::new(guard),
            Box::new(Observer {
                saw_compaction: saw.clone(),
            }),
        ]);

        let mut ctx = MiddlewareContext::new();
        chain.fire_on_step("a", 10, &mut ctx).await.unwrap();

        assert!(saw.load(std::sync::atomic::Ordering::Relaxed));
    }

    // ── HandoffPrompt tests ──────────────────────────────────────────

    #[test]
    fn test_handoff_prompt_renders_all_sections() {
        let handoff = HandoffPrompt::new(
            "Built auth module, tested endpoints",
            "deepseek-v4-pro",
            800_000,
        )
        .add_completed("Implement login endpoint")
        .add_completed("Add JWT middleware")
        .add_in_progress("Write integration tests")
        .add_pending("Deploy to staging")
        .add_blocker("AWS credentials expired")
        .add_open_loop("Rate limiting not implemented")
        .add_file("src/auth/mod.rs")
        .add_file("tests/auth_integration.rs")
        .with_pct_complete(60);

        let rendered = handoff.render();

        assert!(rendered.contains("<handoff>"));
        assert!(rendered.contains("</handoff>"));
        assert!(rendered.contains("Built auth module"));
        assert!(rendered.contains("deepseek-v4-pro"));
        assert!(rendered.contains("800000"));
        assert!(rendered.contains("- [x] Implement login endpoint"));
        assert!(rendered.contains("- [~] Write integration tests"));
        assert!(rendered.contains("- [ ] Deploy to staging"));
        assert!(rendered.contains("AWS credentials expired"));
        assert!(rendered.contains("Rate limiting not implemented"));
        assert!(rendered.contains("src/auth/mod.rs"));
        assert!(rendered.contains("Progress (60%)"));
        assert!(rendered.contains("successor agent"));
    }

    #[test]
    fn test_handoff_prompt_minimal() {
        let handoff = HandoffPrompt::new("Summary only", "deepseek-v4-flash", 100_000);
        let rendered = handoff.render();

        assert!(rendered.contains("Summary only"));
        assert!(rendered.contains("deepseek-v4-flash"));
        assert!(!rendered.contains("## Progress"));
        assert!(!rendered.contains("## Blockers"));
    }

    #[test]
    fn test_handoff_prompt_serialization_round_trip() {
        let handoff = HandoffPrompt::new("Test summary", "deepseek-v4-pro", 500_000)
            .add_completed("Step 1")
            .add_blocker("Blocker 1")
            .with_pct_complete(50);

        let json = serde_json::to_string(&handoff).unwrap();
        let restored: HandoffPrompt = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.summary, "Test summary");
        assert_eq!(restored.model, "deepseek-v4-pro");
        assert_eq!(restored.tokens_used, 500_000);
        assert_eq!(restored.progress.completed, vec!["Step 1"]);
        assert_eq!(restored.blockers, vec!["Blocker 1"]);
        assert_eq!(restored.progress.pct_complete, 50);
    }
}
