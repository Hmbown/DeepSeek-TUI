//! Text selection state for the transcript view.

// === Types ===

/// A selection endpoint in the transcript (line/column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptSelectionPoint {
    pub line_index: usize,
    pub column: usize,
}

/// Current selection state in the transcript view.
#[derive(Debug, Clone, Copy, Default)]
pub struct TranscriptSelection {
    pub anchor: Option<TranscriptSelectionPoint>,
    pub head: Option<TranscriptSelectionPoint>,
    pub dragging: bool,
}

impl TranscriptSelection {
    /// Clear any active selection.
    pub fn clear(&mut self) {
        self.anchor = None;
        self.head = None;
        self.dragging = false;
    }

    /// Whether a full selection is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.anchor.is_some() && self.head.is_some()
    }

    /// Return selection endpoints ordered from start to end.
    #[must_use]
    pub fn ordered_endpoints(
        &self,
    ) -> Option<(TranscriptSelectionPoint, TranscriptSelectionPoint)> {
        let anchor = self.anchor?;
        let head = self.head?;
        if (head.line_index, head.column) < (anchor.line_index, anchor.column) {
            Some((head, anchor))
        } else {
            Some((anchor, head))
        }
    }
}
