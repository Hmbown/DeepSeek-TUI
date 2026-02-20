# Wagmii TUI - AI Handoff Document

## Issues Found and Fixed

### ✅ FIXED: Orphaned Tool Calls Bug (Critical)
**Location:** `src/compaction.rs`, function `enforce_tool_call_pairs`

**Problem:** When auto-compaction runs, the code only ensured that pinned tool results had their corresponding tool calls pinned. However, it did NOT ensure that pinned tool calls had their results pinned. This caused "An assistant message with 'tool_calls' must be followed by tool messages" API errors.

**Fix Applied:** Added bidirectional enforcement:
- Pass 1: If tool result is pinned → ensure tool call is pinned (existing)
- Pass 2: If tool call is pinned → ensure tool result is pinned (NEW)

**Code Change:** See lines 349-430 in `src/compaction.rs`

---

## Issues Still Open / Needs Work

### 1. 🎨 UI Footer Redesign (Kimi CLI Style)
**Priority:** High
**Location:** `src/tui/ui.rs`, function `render_footer`

**Current State:** Footer shows mode badge, context bar, and key hints in a cluttered layout.

**Desired State (Kimi CLI style):**
```
00:07  yolo  agent (kimi-for-coding, thinking)                    context: 0.0%
```

**Requirements:**
- Left side: Time (HH:MM), mode (lowercase, colored), agent info with model and status
- Right side: "context: X.X%" with decimal precision
- Status shows "thinking" when loading
- Clean spacing and colors
- Handle narrow terminals gracefully

**Implementation Notes:**
- Use `chrono::Local::now()` for time (already a dependency)
- Mode colors already defined in `palette.rs`
- Need to add `get_context_percent_decimal()` function
- See Kimi CLI screenshot for exact styling

---

### 2. ✨ Thinking vs Normal Chat Delineation
**Priority:** High
**Location:** `src/tui/ui.rs` (streaming), `src/tui/history.rs` (display)

**Problem:** Thinking blocks and normal assistant responses are not visually distinct enough.

**Desired Behavior:**
- Thinking blocks should be clearly marked with a distinct style
- Use the typing indicator animation for thinking state
- Collapsible thinking sections (optional)
- Different background or border for thinking content

**Current Implementation:**
- `streaming.rs` has `MarkdownStreamCollector` with `is_thinking` flag
- `STATUS_WARNING` color used for thinking text
- `wagmii_thinking_label()` cycles through taglines

**Improvements Needed:**
- Add a visual container around thinking blocks
- Show thinking duration
- Option to show/hide thinking (already have `show_thinking` setting)
- Animated thinking indicator (braille patterns already implemented)

---

### 3. 🧠 Intelligent Compaction UX
**Priority:** Medium
**Location:** `src/compaction.rs`, `src/core/engine.rs`

**Current State:** Auto-compaction happens silently in background. Users see "Auto-compacting context..." then "Auto-compaction complete".

**Desired State:**
- Visual indicator in footer showing compaction status
- Preview of what will be compacted (if user requests)
- Graceful degradation when compaction fails (already partly implemented)
- Option to manually trigger compaction
- Show how many messages were summarized vs kept

**Implementation Ideas:**
- Add compaction progress to status bar
- `/compact` command to manually trigger
- Show summary stats: "Compacted 50 messages → 1 summary, kept 4 recent"

---

### 4. 🎭 "Alive and Animated" Feel
**Priority:** Medium
**Location:** Various TUI components

**Current State:** Basic spinner animations exist but feel static.

**Desired State:**
- More lively animations during thinking/loading
- Smooth transitions between states
- Visual feedback on user actions (subtle highlights)
- Progressive disclosure of content

**Specific Improvements:**
- Braille typing indicator for streaming (already exists, use more prominently)
- Pulse animation for the mode badge when active
- Smooth scroll behavior
- Fade-in for new messages
- Visual distinction between user/assistant/system messages

---

### 5. 🐛 Potential Issue: Escape Key Handling After Plan Mode
**Priority:** Medium (needs verification)
**Location:** `src/tui/ui.rs`, event loop

**User Report:** "it seems like there's something wrong with the mode where like it isn't able to have an esc happen and then respond correctly to the next message"

**Investigation Notes:**
- ESC key currently cancels loading OR clears input OR sets mode to Normal
- After Plan mode completes and shows the "1-4" prompt, ESC might be interfering
- Check if `plan_prompt_pending` state is properly handled with ESC

**Test Case:**
1. Enter Plan mode
2. Let plan complete
3. See "Plan ready. Choose next step" prompt
4. Press ESC
5. Try to send a message - does it work?

---

### 6. 🎨 Header Redesign
**Priority:** Low
**Location:** `src/tui/widgets/header.rs`

**Current State:** Shows `[MODE] model | context% ●`

**Desired State:** Cleaner, more Kimi-like:
- Remove brackets around mode
- Model name in muted color
- Context as percentage bar or number
- Streaming indicator integrated cleanly

---

### 7. 📊 Context Window Visualization
**Priority:** Low
**Location:** `src/tui/ui.rs`

**Current State:** Context bar uses block characters `[████░░░░░░] 40%`

**Desired State:** More subtle visualization:
- Could be just the percentage with color coding
- Or a minimal progress bar
- Show token count on hover/selection

---

## Architecture Notes

### Key Files:
- `src/tui/ui.rs` - Main event loop and rendering
- `src/tui/app.rs` - App state and mode management
- `src/tui/widgets/` - UI components (header, chat, composer)
- `src/compaction.rs` - Message compaction logic
- `src/core/engine.rs` - Backend API communication
- `src/palette.rs` - Colors and theme

### Mode Enum:
```rust
pub enum AppMode {
    Normal,
    Agent,
    Yolo,
    Plan,
    Rlm,
    Duo,
}
```

### Color Constants (from palette.rs):
- `MODE_NORMAL = Gray`
- `MODE_AGENT = Bright blue (80, 150, 255)`
- `MODE_YOLO = Warning red (255, 100, 100)`
- `MODE_PLAN = Orange (255, 170, 60)`
- `MODE_RLM = Purple (180, 100, 255)`
- `MODE_DUO = Teal (100, 220, 180)`
- `WAGMII_SKY = Light blue (106, 174, 242)`
- `TEXT_MUTED = DarkGray`
- `TEXT_DIM = Gray`

---

## Prompt for Next AI

```markdown
You are working on the Wagmii TUI, a Rust terminal UI for the Wagmii AI API. 

Your mission: Make the TUI feel ALIVE, animated, and polished like Kimi CLI.

### Current State
- Tool call bug has been fixed (bidirectional enforcement in compaction.rs)
- Basic streaming and animations exist but feel static
- Footer layout needs redesign to match Kimi CLI style

### Your Tasks

1. **Redesign the Footer** (`src/tui/ui.rs` - `render_footer`)
   - Match Kimi CLI: "00:07  yolo  agent (model, thinking)    context: 0.0%"
   - Time on left, context percentage on right
   - Lowercase mode names with distinct colors
   - "thinking" status when `app.is_loading` is true
   - Clean spacing, minimal visual noise

2. **Enhance Thinking/Chat Delineation**
   - Make thinking blocks visually distinct (border, background, or indentation)
   - Ensure typing indicator is prominent during streaming
   - Add smooth transitions between states

3. **Add Life to the UI**
   - Subtle animations (pulse, fade, smooth scroll)
   - Visual feedback on user input
   - Progressive content reveal
   - Make it feel responsive and "alive"

4. **Polish Compaction UX**
   - Better status messages
   - Optional manual compaction command
   - Visual indicator when near context limit

### Guidelines
- Keep changes focused and minimal
- Follow existing code style
- Use existing color palette
- Test with different terminal widths
- Preserve all existing functionality

### Run Commands
- Build: `cargo build`
- Test: `cargo test`
- Run: `cargo run -- --yolo` (or without --yolo for normal mode)

Make it beautiful! 🎨✨
```

---

## Testing Checklist

### Tool Call Bug Fix
- [ ] Create a plan that uses tools
- [ ] Let auto-compaction trigger
- [ ] Verify no "orphaned tool_calls" API errors
- [ ] Verify tool results appear correctly

### UI Improvements
- [ ] Footer matches Kimi CLI style
- [ ] Time displays correctly
- [ ] Mode colors are correct
- [ ] Context percentage updates
- [ ] Narrow terminal handling works

### Animation & Feel
- [ ] Thinking blocks are distinct
- [ ] Typing indicator is visible
- [ ] Smooth state transitions
- [ ] No flickering or visual glitches

---

*Document created: 2026-01-29*
*Last AI: Initial assessment and bug fix*
*Next AI: UI/UX polish and animation*
