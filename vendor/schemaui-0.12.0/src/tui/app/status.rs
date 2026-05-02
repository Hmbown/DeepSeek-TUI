#[derive(Debug, Clone)]
pub struct StatusLine {
    message: String,
}

pub const READY_STATUS: &str = "Ready. Press Ctrl+S to validate and save.";

impl Default for StatusLine {
    fn default() -> Self {
        Self {
            message: READY_STATUS.to_string(),
        }
    }
}

impl StatusLine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_raw(&mut self, msg: impl Into<String>) {
        self.message = msg.into();
    }

    pub fn ready(&mut self) {
        self.message = READY_STATUS.to_string();
    }

    pub fn editing(&mut self, label: &str) {
        self.message = format!("Editing {label}");
    }

    pub fn value_updated(&mut self) {
        self.message = "Value updated".to_string();
    }

    pub fn validation_passed(&mut self) {
        self.message = "Validation passed".to_string();
    }

    pub fn issues_remaining(&mut self, count: usize) {
        self.message = format!("{count} issue(s) remaining");
    }

    pub fn pending_exit(&mut self) {
        self.message = "Unsaved changes. Press Ctrl+Q again to quit without saving.".to_string();
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}
