#[derive(Debug, Clone)]
pub struct FieldCoercionError {
    pub pointer: String,
    pub message: String,
}

impl std::fmt::Display for FieldCoercionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.pointer, self.message)
    }
}

impl std::error::Error for FieldCoercionError {}

impl FieldCoercionError {
    pub fn unsupported(pointer: &str, action: &str) -> Self {
        Self {
            pointer: pointer.to_string(),
            message: format!("field does not support {action}"),
        }
    }
}
