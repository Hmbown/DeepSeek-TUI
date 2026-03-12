//! Cost estimation for DeepSeek API usage.
//!
//! Pricing based on DeepSeek's published rates (per million tokens).

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
#[allow(dead_code)]
pub fn format_cost(cost: f64) -> String {
    if cost < 0.0001 {
        "<$0.0001".to_string()
    } else if cost < 0.01 {
        format!("${:.4}", cost)
    } else if cost < 1.0 {
        format!("${:.3}", cost)
    } else {
        format!("${:.2}", cost)
    }
}
