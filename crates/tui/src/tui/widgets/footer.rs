//! Footer bar widget displaying mode, status, model, and auxiliary chips.
//!
//! `FooterWidget` is a pure render of a [`FooterProps`] struct: all content
//! (labels, colors, span clusters) is computed once per redraw at a higher
//! level, then `FooterWidget::new(props).render(area, buf)` paints the
//! result. The widget owns no `App` knowledge; this mirrors the layout used
//! by `HeaderWidget` (and Codex's `bottom_pane::footer::Footer`).

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::palette;
use crate::tui::app::{App, AppMode};

use super::Renderable;

/// Pre-computed data the footer needs to render.
///
/// All fields are owned `String` / `Vec<Span<'static>>` values so the props
/// can be built once per redraw and then handed to a borrow-free widget.
#[derive(Debug, Clone)]
pub struct FooterProps {
    /// The current model identifier shown after the mode chip.
    pub model: String,
    /// `"agent"` / `"yolo"` / `"plan"` — the canonical setting label.
    pub mode_label: &'static str,
    /// Color used for the mode chip.
    pub mode_color: Color,
    /// Status label like `"ready"`, `"thinking ⌫"`, `"working"`. When the
    /// label equals `"ready"` the footer hides the status segment entirely.
    pub state_label: String,
    /// Color used for the status label.
    pub state_color: Color,
    /// Coherence chip spans (empty when no active intervention).
    pub coherence: Vec<Span<'static>>,
    /// Sub-agent count chip spans (empty when zero in-flight).
    pub agents: Vec<Span<'static>>,
    /// Reasoning-replay chip spans (empty when zero / not applicable).
    pub reasoning_replay: Vec<Span<'static>>,
    /// Cache-hit-rate chip spans (empty when no usage reported).
    pub cache: Vec<Span<'static>>,
    /// Session-cost chip spans (empty when below the display threshold).
    pub cost: Vec<Span<'static>>,
    /// Optional toast that, when present, replaces the left status line.
    pub toast: Option<FooterToast>,
    /// When `Some(frame_idx)`, the gap between the left status line and the
    /// right-hand chips is filled with an animated water-spout strip keyed
    /// off `frame_idx` (deterministic given the frame). `None` keeps the gap
    /// as plain whitespace, which is the idle/ready state.
    pub working_strip_frame: Option<u64>,
}

/// One frame of the footer's water-spout animation. `col` is the cell index
/// inside the strip, `width` the strip's total width, `frame` the discrete
/// frame counter. Returns the glyph that should appear in that cell on that
/// frame.
///
/// Visual: two box-drawing "arcs" (`╭───╮`) sweeping horizontally at
/// independent speeds across a calm water surface (`─`). Arcs that meet
/// blend into a wider arch, giving the criss-cross fountain feel. Purely
/// deterministic given (col, width, frame) so unit tests can pin frames.
#[must_use]
pub fn footer_working_strip_glyph_at(col: usize, width: usize, frame: u64) -> char {
    if width == 0 {
        return ' ';
    }

    // Half-width of an arc (so a full arc spans `2*ARC_HALF + 1` columns:
    // `╭`, `─`...`─`, `╮`). Three keeps the arcs `╭───╮` — five glyphs wide,
    // matching the issue's sketch.
    const ARC_HALF: i64 = 2;
    let arc_span = ARC_HALF * 2 + 1; // 5

    // Two arcs at independent speeds drifting through a wrap-around span
    // wider than the strip itself, so each arc enters from the left, sweeps
    // across, and exits on the right before re-entering. Phase offsets keep
    // them from synchronising at frame 0.
    let cycle = (width as i64).max(arc_span) + arc_span * 2;
    let frame = frame as i64;
    let pos1 = (frame).rem_euclid(cycle) - arc_span;
    let pos2 = (frame * 2 + (cycle / 3) + 7).rem_euclid(cycle) - arc_span;

    arc_glyph_for(col as i64, pos1)
        .or_else(|| arc_glyph_for(col as i64, pos2))
        .unwrap_or('\u{2500}') // ─  — calm water surface
}

/// Helper: returns the glyph for column `col` if it falls inside an arc
/// centred at `pos`, else `None`. An arc is `╭───╮` shaped — left cup, three
/// dashes, right cup — five columns wide.
fn arc_glyph_for(col: i64, pos: i64) -> Option<char> {
    let dist = col - pos;
    match dist {
        -2 => Some('\u{256D}'),     // ╭  arc rising from the left
        -1..=1 => Some('\u{2500}'), // ─  arc top
        2 => Some('\u{256E}'),      // ╮  arc falling on the right
        _ => None,
    }
}

/// Build the per-frame water-spout string of `width` characters. Empty string
/// when width is 0. The result is the same visual width as requested (one
/// char per column for box-drawing chars) and is safe to drop into a `Span`
/// between the footer's left and right segments.
#[must_use]
pub fn footer_working_strip_string(width: usize, frame: u64) -> String {
    let mut out = String::with_capacity(width * 4);
    for col in 0..width {
        out.push(footer_working_strip_glyph_at(col, width, frame));
    }
    out
}

/// Pulse `working` through `working`, `working.`, `working..`, `working...`
/// keyed off `frame`. The cycle period is 4 frames (matching the four
/// states), so adjacent ticks visibly differ. Returns a static-friendly
/// `String` so callers can drop it into a `Span::styled` without lifetime
/// gymnastics.
#[must_use]
pub fn footer_working_label(frame: u64) -> String {
    let dots = (frame % 4) as usize;
    let mut out = String::with_capacity(7 + dots);
    out.push_str("working");
    for _ in 0..dots {
        out.push('.');
    }
    out
}

/// Build a "N agents" chip span list when there are sub-agents in flight.
/// Empty list when N == 0 hides the chip entirely. Singular for N == 1
/// reads naturally; plural otherwise.
#[must_use]
pub fn footer_agents_chip(running: usize) -> Vec<Span<'static>> {
    if running == 0 {
        return Vec::new();
    }
    let text = if running == 1 {
        "1 agent".to_string()
    } else {
        format!("{running} agents")
    };
    vec![Span::styled(
        text,
        Style::default().fg(palette::DEEPSEEK_SKY),
    )]
}

/// A status toast routed to the footer's left segment for a short time.
#[derive(Debug, Clone)]
pub struct FooterToast {
    pub text: String,
    pub color: Color,
}

impl FooterProps {
    /// Build footer props from common app state. Helpers in `tui/ui.rs`
    /// (e.g. `footer_state_label`, `footer_coherence_spans`) supply the
    /// pre-styled spans and labels — this constructor just bundles them.
    ///
    /// Argument fan-out is intentional: each input maps 1:1 to a piece of
    /// pre-computed footer content the caller resolved from `App`. Forcing
    /// these into a builder would obscure the call site without making the
    /// data flow any clearer.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn from_app(
        app: &App,
        toast: Option<FooterToast>,
        state_label: &'static str,
        state_color: Color,
        coherence: Vec<Span<'static>>,
        agents: Vec<Span<'static>>,
        reasoning_replay: Vec<Span<'static>>,
        cache: Vec<Span<'static>>,
        cost: Vec<Span<'static>>,
    ) -> Self {
        let (mode_label, mode_color) = mode_style(app.mode);
        Self {
            model: app.model.clone(),
            mode_label,
            mode_color,
            state_label: state_label.to_string(),
            state_color,
            coherence,
            agents,
            reasoning_replay,
            cache,
            cost,
            toast,
            working_strip_frame: None,
        }
    }
}

fn mode_style(mode: AppMode) -> (&'static str, Color) {
    let label = match mode {
        AppMode::Agent => "agent",
        AppMode::Yolo => "yolo",
        AppMode::Plan => "plan",
    };
    let color = match mode {
        AppMode::Agent => palette::MODE_AGENT,
        AppMode::Yolo => palette::MODE_YOLO,
        AppMode::Plan => palette::MODE_PLAN,
    };
    (label, color)
}

/// Pure-render footer. Build once per frame, then `render(area, buf)`.
pub struct FooterWidget {
    props: FooterProps,
}

impl FooterWidget {
    #[must_use]
    pub fn new(props: FooterProps) -> Self {
        Self { props }
    }

    fn auxiliary_spans(&self, max_width: usize) -> Vec<Span<'static>> {
        let parts: Vec<&Vec<Span<'static>>> = [
            &self.props.coherence,
            &self.props.agents,
            &self.props.reasoning_replay,
            &self.props.cache,
            &self.props.cost,
        ]
        .into_iter()
        .filter(|spans| !spans.is_empty())
        .collect();

        // Try to fit as many parts as possible, dropping from the end.
        for end in (0..=parts.len()).rev() {
            let mut combined: Vec<Span<'static>> = Vec::new();
            for (i, part) in parts[..end].iter().enumerate() {
                if i > 0 {
                    combined.push(Span::raw("  "));
                }
                combined.extend(part.iter().cloned());
            }
            if span_width(&combined) <= max_width {
                return combined;
            }
        }
        Vec::new()
    }

    fn toast_spans(toast: &FooterToast, max_width: usize) -> Vec<Span<'static>> {
        let truncated = truncate_to_width(&toast.text, max_width.max(1));
        vec![Span::styled(truncated, Style::default().fg(toast.color))]
    }

    /// Build the left status line with priority-ordered hint dropping.
    ///
    /// Priority order (highest to lowest — last to drop):
    /// 1. Mode label (always visible at any width; truncated only as a last resort)
    /// 2. Model name (always visible when width ≥ enough for "mode · model"; then truncated mid-word)
    /// 3. Status label (e.g. "working", "draft") — drops first when space is tight
    ///
    /// At every width ≥40 cols the line never wraps mid-hint: the widget chooses
    /// one of (`mode · model · status`, `mode · model`, `mode`) and renders
    /// that single line within `max_width`. Below 40 cols the same fallback
    /// applies; the model is allowed to truncate with an ellipsis only when
    /// the status label has already been dropped.
    fn status_line_spans(&self, max_width: usize) -> Vec<Span<'static>> {
        if max_width == 0 {
            return Vec::new();
        }

        let mode_label = self.props.mode_label;
        let sep = " \u{00B7} ";
        let model = self.props.model.as_str();
        let show_status = self.props.state_label != "ready";
        let status_label = self.props.state_label.as_str();

        let mode_w = mode_label.width();
        let sep_w = sep.width();
        let model_w = UnicodeWidthStr::width(model);
        let status_w = status_label.width();

        // Tier 1: mode · model · status — full footer, no truncation.
        let full_w = mode_w + sep_w + model_w + sep_w + status_w;
        if show_status && full_w <= max_width {
            return self.build_status_line_spans(mode_label, model.to_string(), Some(status_label));
        }

        // Tier 2: mode · model — drop status first.
        let mode_model_w = mode_w + sep_w + model_w;
        if mode_model_w <= max_width {
            return self.build_status_line_spans(mode_label, model.to_string(), None);
        }

        // Tier 3: mode · <truncated model> — keep both labels visible by
        // ellipsizing the model name. Only do this when there is enough room
        // for at least the ellipsis ("..."). Below that we drop to mode-only.
        let prefix_w = mode_w + sep_w;
        if prefix_w < max_width {
            let model_budget = max_width - prefix_w;
            if model_budget >= 4 {
                let truncated = truncate_to_width(model, model_budget);
                if !truncated.is_empty() {
                    return self.build_status_line_spans(mode_label, truncated, None);
                }
            }
        }

        // Tier 4: mode-only. If even the mode label cannot fit, truncate it
        // so the footer never wraps to a second row.
        if mode_w <= max_width {
            return vec![Span::styled(
                mode_label.to_string(),
                Style::default().fg(self.props.mode_color),
            )];
        }
        vec![Span::styled(
            truncate_to_width(mode_label, max_width),
            Style::default().fg(self.props.mode_color),
        )]
    }

    fn build_status_line_spans(
        &self,
        mode_label: &'static str,
        model_label: String,
        status: Option<&str>,
    ) -> Vec<Span<'static>> {
        let sep = " \u{00B7} ";
        let mut spans = vec![
            Span::styled(
                mode_label.to_string(),
                Style::default().fg(self.props.mode_color),
            ),
            Span::styled(sep.to_string(), Style::default().fg(palette::TEXT_DIM)),
            Span::styled(model_label, Style::default().fg(palette::TEXT_HINT)),
        ];
        if let Some(status_label) = status {
            spans.push(Span::styled(
                sep.to_string(),
                Style::default().fg(palette::TEXT_DIM),
            ));
            spans.push(Span::styled(
                status_label.to_string(),
                Style::default().fg(self.props.state_color),
            ));
        }
        spans
    }
}

impl Renderable for FooterWidget {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let available_width = area.width as usize;
        if available_width == 0 {
            return;
        }

        let right_spans = self.auxiliary_spans(available_width);
        let right_width = span_width(&right_spans);
        let min_gap = if right_width > 0 { 2 } else { 0 };
        let max_left_width = available_width
            .saturating_sub(right_width)
            .saturating_sub(min_gap)
            .max(1);

        let left_spans = if let Some(toast) = self.props.toast.as_ref() {
            Self::toast_spans(toast, max_left_width)
        } else {
            self.status_line_spans(max_left_width)
        };

        let left_width = span_width(&left_spans);
        let spacer_width = available_width.saturating_sub(left_width + right_width);

        // When a turn is in flight, fill the gap with a thin animated water-
        // spout strip; otherwise the gap stays as plain whitespace.
        let spacer_span = match self.props.working_strip_frame {
            Some(frame) if spacer_width > 0 => Span::styled(
                footer_working_strip_string(spacer_width, frame),
                Style::default().fg(palette::DEEPSEEK_SKY),
            ),
            _ => Span::raw(" ".repeat(spacer_width)),
        };

        let mut all_spans = left_spans;
        all_spans.push(spacer_span);
        all_spans.extend(right_spans);

        let paragraph = Paragraph::new(Line::from(all_spans));
        paragraph.render(area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

fn span_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.width()).sum()
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width <= 3 {
        return text.chars().take(max_width).collect();
    }

    let mut out = String::new();
    let mut width = 0usize;
    let limit = max_width.saturating_sub(3);
    for ch in text.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > limit {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::{FooterProps, FooterWidget, Renderable};
    use crate::config::Config;
    use crate::palette;
    use crate::tui::app::{App, AppMode, TuiOptions};
    use ratatui::{style::Color, text::Span};
    use std::path::PathBuf;

    fn make_app() -> App {
        let options = TuiOptions {
            model: "deepseek-v4-flash".to_string(),
            workspace: PathBuf::from("."),
            allow_shell: false,
            use_alt_screen: true,
            use_mouse_capture: false,
            use_bracketed_paste: true,
            max_subagents: 1,
            skills_dir: PathBuf::from("."),
            memory_path: PathBuf::from("memory.md"),
            notes_path: PathBuf::from("notes.txt"),
            mcp_config_path: PathBuf::from("mcp.json"),
            use_memory: false,
            start_in_agent_mode: true,
            skip_onboarding: true,
            yolo: false,
            resume_session_id: None,
        };
        let mut app = App::new(options, &Config::default());
        // App::new may pick up `default_model` from a local user Settings
        // file, which overrides the option above. Pin the model explicitly
        // so these tests are independent of any host-side configuration.
        app.model = "deepseek-v4-flash".to_string();
        app
    }

    fn idle_props_for(app: &App) -> FooterProps {
        FooterProps::from_app(
            app,
            None,
            "ready",
            palette::TEXT_MUTED,
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
        )
    }

    #[test]
    fn from_app_idle_state_carries_ready_label_and_no_chips() {
        let app = make_app();
        let props = idle_props_for(&app);

        assert_eq!(props.state_label, "ready");
        assert_eq!(props.state_color, palette::TEXT_MUTED);
        assert_eq!(props.mode_label, "agent");
        assert_eq!(props.mode_color, palette::MODE_AGENT);
        assert_eq!(props.model, "deepseek-v4-flash");
        assert!(props.coherence.is_empty());
        assert!(props.agents.is_empty());
        assert!(props.cache.is_empty());
        assert!(props.cost.is_empty());
        assert!(props.reasoning_replay.is_empty());
        assert!(props.toast.is_none());
    }

    #[test]
    fn from_app_loading_state_uses_thinking_label_and_warning_color() {
        let app = make_app();
        let props = FooterProps::from_app(
            &app,
            None,
            "thinking \u{238B}",
            palette::STATUS_WARNING,
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
        );

        assert!(props.state_label.starts_with("thinking"));
        assert_eq!(props.state_color, palette::STATUS_WARNING);
    }

    // ---- agents chip wording ----
    #[test]
    fn footer_agents_chip_is_empty_when_no_agents_running() {
        let chip = super::footer_agents_chip(0);
        assert!(chip.is_empty(), "0 agents in flight → no chip");
    }

    #[test]
    fn footer_agents_chip_uses_singular_for_one() {
        let chip = super::footer_agents_chip(1);
        assert_eq!(chip.len(), 1);
        assert_eq!(chip[0].content.as_ref(), "1 agent");
    }

    #[test]
    fn footer_agents_chip_uses_plural_for_many() {
        let chip = super::footer_agents_chip(3);
        assert_eq!(chip.len(), 1);
        assert_eq!(chip[0].content.as_ref(), "3 agents");
    }

    #[test]
    fn footer_agents_chip_renders_into_widget() {
        let app = make_app();
        let agents = super::footer_agents_chip(2);
        let props = FooterProps::from_app(
            &app,
            None,
            "ready",
            palette::TEXT_MUTED,
            Vec::<Span<'static>>::new(),
            agents,
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
        );
        let widget = FooterWidget::new(props);
        let area = ratatui::layout::Rect::new(0, 0, 60, 1);
        let mut buf = ratatui::buffer::Buffer::empty(area);
        widget.render(area, &mut buf);
        let rendered: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(
            rendered.contains("2 agents"),
            "expected agents chip in render: {rendered:?}",
        );
    }

    #[test]
    fn from_app_mode_color_matches_mode_for_each_variant() {
        let mut app = make_app();
        let cases = [
            (AppMode::Agent, "agent", palette::MODE_AGENT),
            (AppMode::Yolo, "yolo", palette::MODE_YOLO),
            (AppMode::Plan, "plan", palette::MODE_PLAN),
        ];
        for (mode, expected_label, expected_color) in cases {
            app.mode = mode;
            let props = idle_props_for(&app);
            assert_eq!(
                props.mode_label, expected_label,
                "label mismatch for {mode:?}",
            );
            assert_eq!(
                props.mode_color, expected_color,
                "color mismatch for {mode:?}",
            );
        }
    }

    #[test]
    fn render_emits_mode_and_model_when_idle() {
        let app = make_app();
        let props = idle_props_for(&app);
        let widget = FooterWidget::new(props);

        let area = ratatui::layout::Rect::new(0, 0, 60, 1);
        let mut buf = ratatui::buffer::Buffer::empty(area);
        widget.render(area, &mut buf);

        let rendered: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(rendered.contains("agent"));
        assert!(rendered.contains("deepseek-v4-flash"));
        assert!(!rendered.contains("ready"));
    }

    #[test]
    fn working_strip_string_width_matches_request() {
        // The strip must produce exactly `width` characters per frame —
        // otherwise the spacer math in `FooterWidget::render` would
        // mis-align the right-hand chips. (Glyphs are all ASCII / Latin-1
        // so char count equals visual width here.)
        for width in [0usize, 1, 8, 60, 200] {
            let s = super::footer_working_strip_string(width, 7);
            assert_eq!(s.chars().count(), width, "width {width} mismatch");
        }
    }

    #[test]
    fn working_strip_glyph_is_deterministic_per_frame() {
        // Same (col, width, frame) → same glyph. Different `frame` values
        // produce different overall strings, which is what makes the
        // animation visible.
        let a = super::footer_working_strip_string(40, 1);
        let b = super::footer_working_strip_string(40, 1);
        assert_eq!(a, b, "deterministic given the same frame");
        let c = super::footer_working_strip_string(40, 2);
        assert_ne!(a, c, "advancing the frame must change the strip");
    }

    #[test]
    fn working_strip_renders_glyphs_only_when_frame_is_some() {
        // Idle: spacer is plain whitespace. Active: spacer contains the
        // box-drawing animation glyphs (`╭` rising-arc, `╮` falling-arc,
        // `─` water surface) and visibly differs from the idle render.
        let app = make_app();
        let mut props = idle_props_for(&app);

        let area = ratatui::layout::Rect::new(0, 0, 80, 1);
        let mut buf = ratatui::buffer::Buffer::empty(area);
        FooterWidget::new(props.clone()).render(area, &mut buf);
        let idle: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();

        props.working_strip_frame = Some(13);
        let mut buf2 = ratatui::buffer::Buffer::empty(area);
        FooterWidget::new(props).render(area, &mut buf2);
        let active: String = (0..area.width).map(|x| buf2[(x, 0)].symbol()).collect();

        assert_ne!(
            idle, active,
            "active footer must visibly differ from idle one"
        );
        assert!(
            active.contains('\u{256D}')   // ╭
                || active.contains('\u{256E}') // ╮
                || active.contains('\u{2500}'), // ─
            "active strip must contain at least one animation glyph: {active:?}",
        );
    }

    #[test]
    fn working_strip_arc_position_advances_with_frame() {
        // At least one arc cup (╭) must shift columns between consecutive
        // frames so the animation reads as drift, not a static pattern.
        let width = 32;
        let f0 = super::footer_working_strip_string(width, 1);
        let f1 = super::footer_working_strip_string(width, 2);
        // Collect the columns that hold a left-arc `╭` glyph in each frame.
        let cups = |s: &str| -> Vec<usize> {
            s.chars()
                .enumerate()
                .filter_map(|(i, c)| (c == '\u{256D}').then_some(i))
                .collect()
        };
        let p0 = cups(&f0);
        let p1 = cups(&f1);
        assert_ne!(p0, p1, "arc cup positions must advance between frames");
    }

    #[test]
    fn working_strip_renders_full_arc_when_room() {
        // A frame at which arc 1 is fully inside the strip should render
        // the canonical `╭───╮` shape — the artistic centrepiece of the
        // animation. Arc 1's leading edge starts at `frame % cycle - 5`,
        // so frame == 5 puts the arc's left cup exactly at column 0.
        // (See `footer_working_strip_glyph_at` for the math.)
        let s = super::footer_working_strip_string(40, 5);
        assert!(
            s.contains("\u{256D}\u{2500}\u{2500}\u{2500}\u{256E}"),
            "expected ╭───╮ arc somewhere in the strip: {s:?}",
        );
    }

    #[test]
    fn working_label_pulses_dots_through_full_cycle() {
        // The label sequence `working` → `working.` → `working..` →
        // `working...` then wraps back. Each frame is a discrete tick;
        // the cycle is exactly 4 frames so adjacent ticks visibly differ.
        assert_eq!(super::footer_working_label(0), "working");
        assert_eq!(super::footer_working_label(1), "working.");
        assert_eq!(super::footer_working_label(2), "working..");
        assert_eq!(super::footer_working_label(3), "working...");
        assert_eq!(
            super::footer_working_label(4),
            "working",
            "wraps back at frame 4",
        );
        assert_eq!(super::footer_working_label(7), "working...");
    }

    /// Render the footer at `width` and return the visible single-line text.
    fn render_at_width(props: FooterProps, width: u16) -> String {
        let area = ratatui::layout::Rect::new(0, 0, width, 1);
        let mut buf = ratatui::buffer::Buffer::empty(area);
        FooterWidget::new(props).render(area, &mut buf);
        (0..area.width)
            .map(|x| buf[(x, 0)].symbol())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    fn props_with_status(state: &str) -> FooterProps {
        let app = make_app();
        FooterProps::from_app(
            &app,
            None,
            // Production state labels are `&'static str`; for tests we leak a
            // copy to match that lifetime.
            Box::leak(state.to_string().into_boxed_str()),
            palette::DEEPSEEK_SKY,
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
        )
    }

    /// Issue #88 — at the widest tier the footer shows mode · model · status
    /// without any truncation.
    #[test]
    fn footer_priority_drop_full_at_120_cols() {
        let props = props_with_status("working");
        let line = render_at_width(props, 120);
        assert!(line.contains("agent"), "mode visible: {line:?}");
        assert!(
            line.contains("deepseek-v4-flash"),
            "model visible: {line:?}"
        );
        assert!(line.contains("working"), "status visible: {line:?}");
        assert!(!line.contains("..."), "no truncation expected: {line:?}");
    }

    #[test]
    fn footer_priority_drop_full_at_100_cols() {
        let props = props_with_status("working");
        let line = render_at_width(props, 100);
        assert!(line.contains("agent"));
        assert!(line.contains("deepseek-v4-flash"));
        assert!(line.contains("working"));
    }

    /// At 80 cols the short status label "working" still fits alongside mode +
    /// model. The line never wraps mid-hint.
    #[test]
    fn footer_priority_drop_full_at_80_cols() {
        let props = props_with_status("working");
        let line = render_at_width(props, 80);
        assert!(line.contains("agent"));
        assert!(line.contains("deepseek-v4-flash"));
        assert!(!line.contains("..."), "no mid-word truncation: {line:?}");
        assert!(line.len() <= 80, "fits in 80 cols: {line:?}");
    }

    /// Status drops before the model is truncated. With a longer status label
    /// at 40 cols the status segment is dropped to keep mode + model intact.
    #[test]
    fn footer_priority_drop_status_first_at_40_cols() {
        let props = props_with_status("refreshing context");
        // "agent · deepseek-v4-flash · refreshing context" = 46 cols. At 40
        // the status label drops, keeping mode + model verbatim.
        let line = render_at_width(props, 40);
        assert!(line.contains("agent"), "mode kept: {line:?}");
        assert!(
            line.contains("deepseek-v4-flash"),
            "model kept verbatim: {line:?}"
        );
        assert!(
            !line.contains("refreshing"),
            "status dropped before model truncated: {line:?}",
        );
        assert!(line.len() <= 40, "fits in 40 cols: {line:?}");
    }

    /// At 60 cols mode + model + a long status all just fit (49 cols), so the
    /// whole line is preserved.
    #[test]
    fn footer_priority_drop_full_at_60_cols() {
        let props = props_with_status("working");
        let line = render_at_width(props, 60);
        assert!(line.contains("agent"));
        assert!(line.contains("deepseek-v4-flash"));
        assert!(line.contains("working"));
    }

    /// Below 30 cols the model truncates with an ellipsis only after the
    /// status label has already been dropped. Mode label always survives.
    #[test]
    fn footer_priority_drop_truncates_model_only_when_status_already_gone() {
        let props = props_with_status("working");
        let line = render_at_width(props, 20);
        assert!(line.starts_with("agent"), "mode stays at front: {line:?}");
        assert!(
            line.contains("..."),
            "model truncated as last resort: {line:?}"
        );
        assert!(!line.contains("working"), "status dropped: {line:?}");
    }

    #[test]
    fn render_swaps_toast_for_status_line() {
        let app = make_app();
        let toast = super::FooterToast {
            text: "session saved".to_string(),
            color: Color::Green,
        };
        let props = FooterProps::from_app(
            &app,
            Some(toast),
            "ready",
            palette::TEXT_MUTED,
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
            Vec::<Span<'static>>::new(),
        );
        let widget = FooterWidget::new(props);

        let area = ratatui::layout::Rect::new(0, 0, 60, 1);
        let mut buf = ratatui::buffer::Buffer::empty(area);
        widget.render(area, &mut buf);

        let rendered: String = (0..area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(rendered.contains("session saved"));
        assert!(!rendered.contains("agent"));
        assert!(!rendered.contains("deepseek-v4-flash"));
    }
}
