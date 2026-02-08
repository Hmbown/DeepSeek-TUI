//! Cost estimation for DeepSeek API usage.
//!
//! Pricing based on DeepSeek's published rates (per million tokens).

use serde_json::Value;

/// Per-million-token pricing for a model.
struct ModelPricing {
    input_per_million: f64,
    output_per_million: f64,
}

/// Look up pricing for a model name.
fn pricing_for_model(model: &str) -> Option<ModelPricing> {
    let lower = model.to_lowercase();
    if lower.contains("deepseek-reasoner") || lower.contains("deepseek-r1") {
        // DeepSeek-R1: $0.55/M input, $2.19/M output
        Some(ModelPricing {
            input_per_million: 0.55,
            output_per_million: 2.19,
        })
    } else if lower.contains("deepseek-v3.2") {
        // DeepSeek-V3.2 (with reasoning): same pricing tier as V3
        Some(ModelPricing {
            input_per_million: 0.27,
            output_per_million: 1.10,
        })
    } else if lower.contains("deepseek-chat") || lower.contains("deepseek-v3") {
        // DeepSeek-V3: $0.27/M input, $1.10/M output
        Some(ModelPricing {
            input_per_million: 0.27,
            output_per_million: 1.10,
        })
    } else if lower.contains("deepseek") {
        // Generic DeepSeek fallback (V3 pricing)
        Some(ModelPricing {
            input_per_million: 0.27,
            output_per_million: 1.10,
        })
    } else {
        None
    }
}

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

/// Calculate cost for a turn given token usage and model.
#[must_use]
pub fn calculate_turn_cost(model: &str, input_tokens: u32, output_tokens: u32) -> Option<f64> {
    let pricing = pricing_for_model(model)?;
    let input_cost = (input_tokens as f64 / 1_000_000.0) * pricing.input_per_million;
    let output_cost = (output_tokens as f64 / 1_000_000.0) * pricing.output_per_million;
    Some(input_cost + output_cost)
}

/// Format a USD cost for compact display.
#[must_use]
pub fn format_cost(cost: f64) -> String {
    if cost < 0.0001 {
        "<$0.01".to_string()
    } else if cost < 0.01 {
        format!("${:.4}", cost)
    } else if cost < 1.0 {
        format!("${:.3}", cost)
    } else {
        format!("${:.2}", cost)
    }
}
