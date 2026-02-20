//! Markdown stream collector for newline-gated rendering.
//!
//! This module implements the pattern from codex-rs where:
//! - Streaming text is buffered until a newline is reached
//! - Only complete lines are committed to the UI
//! - This prevents visual flashing of partial words
//! - Final content is emitted when the stream ends

#![allow(dead_code)]

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::palette;
/// Collects streaming text and commits complete lines.
#[derive(Debug, Clone)]
pub struct MarkdownStreamCollector {
    /// Buffer for incoming text
    buffer: String,
    /// Number of lines already committed
    committed_line_count: usize,
    /// Terminal width for wrapping
    width: Option<usize>,
    /// Whether the stream is still active
    is_streaming: bool,
    /// Whether this is a thinking block
    is_thinking: bool,
}

impl MarkdownStreamCollector {
    /// Create a new collector
    pub fn new(width: Option<usize>, is_thinking: bool) -> Self {
        Self {
            buffer: String::new(),
            committed_line_count: 0,
            width,
            is_streaming: true,
            is_thinking,
        }
    }

    /// Push new content to the buffer
    pub fn push(&mut self, content: &str) {
        self.buffer.push_str(content);
    }

    /// Get the current buffer content (for display during streaming)
    pub fn current_content(&self) -> &str {
        &self.buffer
    }

    /// Check if there are complete lines to commit
    pub fn has_complete_lines(&self) -> bool {
        self.buffer.contains('\n')
    }

    /// Commit complete lines and return them.
    /// Only lines ending with '\n' are committed.
    /// Returns the newly committed lines since last call.
    pub fn commit_complete_lines(&mut self) -> Vec<Line<'static>> {
        if self.buffer.is_empty() {
            return Vec::new();
        }

        // Find the last newline - only process up to there
        let Some(last_newline_idx) = self.buffer.rfind('\n') else {
            return Vec::new(); // No complete lines yet
        };

        // Extract the complete portion (up to and including last newline)
        let complete_portion = self.buffer[..=last_newline_idx].to_string();

        // Render all lines from the complete portion
        let all_lines = self.render_lines(&complete_portion);

        // Remove the committed portion from the buffer so finalize only emits the remainder
        self.buffer = self.buffer[last_newline_idx + 1..].to_string();
        self.committed_line_count = 0;

        all_lines
    }

    /// Finalize the stream and return any remaining content.
    /// Call this when the stream ends to emit the final incomplete line.
    pub fn finalize(&mut self) -> Vec<Line<'static>> {
        self.is_streaming = false;

        if self.buffer.is_empty() {
            return Vec::new();
        }

        // Render all remaining content
        let all_lines = self.render_lines(&self.buffer);

        // Return only the NEW lines since last commit
        let new_lines = if self.committed_line_count < all_lines.len() {
            all_lines[self.committed_line_count..].to_vec()
        } else {
            Vec::new()
        };

        // Mark as fully committed
        self.committed_line_count = all_lines.len();

        new_lines
    }

    /// Get all rendered lines (for final display after stream ends)
    pub fn all_lines(&self) -> Vec<Line<'static>> {
        self.render_lines(&self.buffer)
    }

    /// Render content into styled lines
    fn render_lines(&self, content: &str) -> Vec<Line<'static>> {
        let width = self.width.unwrap_or(80);
        let style = if self.is_thinking {
            Style::default()
                .fg(palette::STATUS_WARNING)
                .add_modifier(Modifier::DIM | Modifier::ITALIC)
        } else {
            Style::default()
        };

        let mut lines = Vec::new();

        for line in content.lines() {
            // Wrap long lines
            let wrapped = wrap_line(line, width);
            for wrapped_line in wrapped {
                lines.push(Line::from(Span::styled(wrapped_line, style)));
            }
        }

        // Handle trailing newline (add empty line)
        if content.ends_with('\n') {
            lines.push(Line::from(""));
        }

        lines
    }

    /// Check if the stream is still active
    pub fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    /// Get the raw buffer length
    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.committed_line_count = 0;
    }
}

/// Wrap a single line to fit within the given width
fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }

    let mut result = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for word in line.split_whitespace() {
        let word_width = word.width();

        if current_width == 0 {
            // First word on line
            current_line = word.to_string();
            current_width = word_width;
        } else if current_width + 1 + word_width <= width {
            // Word fits with space
            current_line.push(' ');
            current_line.push_str(word);
            current_width += 1 + word_width;
        } else {
            // Word doesn't fit, start new line
            result.push(current_line);
            current_line = word.to_string();
            current_width = word_width;
        }
    }

    if !current_line.is_empty() {
        result.push(current_line);
    }

    if result.is_empty() {
        vec![String::new()]
    } else {
        result
    }
}

/// State for managing multiple stream collectors (one per content block)
#[derive(Debug, Clone, Default)]
pub struct StreamingState {
    /// Collectors for each content block by index
    collectors: Vec<Option<MarkdownStreamCollector>>,
    /// Whether any stream is currently active
    pub is_active: bool,
    /// Accumulated text for display
    pub accumulated_text: String,
    /// Accumulated thinking for display
    pub accumulated_thinking: String,
}

impl StreamingState {
    /// Create a new streaming state
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a new text block
    pub fn start_text(&mut self, index: usize, width: Option<usize>) {
        self.ensure_capacity(index);
        self.collectors[index] = Some(MarkdownStreamCollector::new(width, false));
        self.is_active = true;
    }

    /// Start a new thinking block
    pub fn start_thinking(&mut self, index: usize, width: Option<usize>) {
        self.ensure_capacity(index);
        self.collectors[index] = Some(MarkdownStreamCollector::new(width, true));
        self.is_active = true;
    }

    /// Push content to a block
    pub fn push_content(&mut self, index: usize, content: &str) {
        if let Some(Some(collector)) = self.collectors.get_mut(index) {
            collector.push(content);
            // Update accumulated text
            if collector.is_thinking {
                self.accumulated_thinking.push_str(content);
            } else {
                self.accumulated_text.push_str(content);
            }
        }
    }

    /// Get newly committed lines from a block
    pub fn commit_lines(&mut self, index: usize) -> Vec<Line<'static>> {
        if let Some(Some(collector)) = self.collectors.get_mut(index) {
            collector.commit_complete_lines()
        } else {
            Vec::new()
        }
    }

    /// Finalize a block and get remaining lines
    pub fn finalize_block(&mut self, index: usize) -> Vec<Line<'static>> {
        if let Some(Some(collector)) = self.collectors.get_mut(index) {
            let lines = collector.finalize();
            // Check if all blocks are done
            self.check_active();
            lines
        } else {
            Vec::new()
        }
    }

    /// Finalize all blocks
    pub fn finalize_all(&mut self) -> Vec<(usize, Vec<Line<'static>>)> {
        let mut result = Vec::new();
        for (i, collector) in self.collectors.iter_mut().enumerate() {
            if let Some(c) = collector {
                let lines = c.finalize();
                if !lines.is_empty() {
                    result.push((i, lines));
                }
            }
        }
        self.is_active = false;
        result
    }

    /// Check if any stream is still active
    fn check_active(&mut self) {
        self.is_active = self.collectors.iter().any(|c| {
            c.as_ref()
                .is_some_and(MarkdownStreamCollector::is_streaming)
        });
    }

    /// Ensure capacity for the given index
    fn ensure_capacity(&mut self, index: usize) {
        while self.collectors.len() <= index {
            self.collectors.push(None);
        }
    }

    /// Reset the streaming state
    pub fn reset(&mut self) {
        self.collectors.clear();
        self.is_active = false;
        self.accumulated_text.clear();
        self.accumulated_thinking.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commit_complete_lines() {
        let mut collector = MarkdownStreamCollector::new(Some(80), false);

        // Push incomplete line
        collector.push("Hello ");
        let lines = collector.commit_complete_lines();
        assert!(lines.is_empty()); // No complete lines yet

        // Complete the line
        collector.push("World\n");
        let lines = collector.commit_complete_lines();
        assert_eq!(lines.len(), 2); // "Hello World" + empty line from trailing \n

        // Push more content
        collector.push("Second line");
        let lines = collector.commit_complete_lines();
        assert!(lines.is_empty()); // No new complete lines

        // Finalize
        let lines = collector.finalize();
        assert_eq!(lines.len(), 1); // "Second line"
    }

    #[test]
    fn test_wrap_line() {
        let result = wrap_line("This is a long line that should be wrapped", 20);
        assert!(result.len() > 1);
    }
}
