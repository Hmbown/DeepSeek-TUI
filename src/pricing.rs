//! Cost estimation placeholders for tool executions.
//!
//! DeepSeek CLI focuses on text-only workflows; no paid multimedia tools are exposed
//! by default, so cost estimates are currently unavailable.

use serde_json::Value;

/// Estimated cost for a tool execution
#[derive(Debug, Clone)]
pub struct CostEstimate {
    /// Minimum cost in USD
    pub min_usd: f64,
    /// Maximum cost in USD
    pub max_usd: f64,
    /// Cost breakdown explanation
    pub breakdown: String,
}

impl CostEstimate {
    #[must_use]
    #[allow(dead_code)]
    pub fn new(min_usd: f64, max_usd: f64, breakdown: impl Into<String>) -> Self {
        Self {
            min_usd,
            max_usd,
            breakdown: breakdown.into(),
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn fixed(usd: f64, breakdown: impl Into<String>) -> Self {
        Self::new(usd, usd, breakdown)
    }

    /// Format the cost for display
    #[must_use]
    pub fn display(&self) -> String {
        if (self.min_usd - self.max_usd).abs() < 0.0001 {
            format!("${:.4}", self.min_usd)
        } else {
            format!("${:.4} - ${:.4}", self.min_usd, self.max_usd)
        }
    }
}

/// Get cost estimate for a tool by name
#[must_use]
pub fn estimate_tool_cost(tool_name: &str, params: &Value) -> Option<CostEstimate> {
    let _ = (tool_name, params);
    None
}
