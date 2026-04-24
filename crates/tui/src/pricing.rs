//! Cost estimation for DeepSeek API usage.
//!
//! Pricing based on DeepSeek's published rates (per million tokens).

use crate::models::Usage;

/// Per-million-token pricing for a model.
struct ModelPricing {
    input_cache_hit_per_million: f64,
    input_cache_miss_per_million: f64,
    output_per_million: f64,
}

/// Look up pricing for a model name.
fn pricing_for_model(model: &str) -> Option<ModelPricing> {
    let lower = model.to_lowercase();
    if !lower.contains("deepseek") {
        return None;
    }
    if lower.contains("v4-pro") || lower.contains("v4pro") {
        Some(ModelPricing {
            input_cache_hit_per_million: 0.145,
            input_cache_miss_per_million: 1.74,
            output_per_million: 3.48,
        })
    } else {
        // deepseek-v4-flash and legacy aliases (deepseek-chat, deepseek-reasoner,
        // deepseek-v3*) all price as v4-flash.
        Some(ModelPricing {
            input_cache_hit_per_million: 0.028,
            input_cache_miss_per_million: 0.14,
            output_per_million: 0.28,
        })
    }
}

/// Calculate cost for a turn given token usage and model.
#[must_use]
#[allow(dead_code)]
pub fn calculate_turn_cost(model: &str, input_tokens: u32, output_tokens: u32) -> Option<f64> {
    let pricing = pricing_for_model(model)?;
    let input_cost = (input_tokens as f64 / 1_000_000.0) * pricing.input_cache_miss_per_million;
    let output_cost = (output_tokens as f64 / 1_000_000.0) * pricing.output_per_million;
    Some(input_cost + output_cost)
}

/// Calculate cost from provider usage, honoring DeepSeek context-cache fields.
#[must_use]
pub fn calculate_turn_cost_from_usage(model: &str, usage: &Usage) -> Option<f64> {
    let pricing = pricing_for_model(model)?;
    let hit_tokens = usage.prompt_cache_hit_tokens.unwrap_or(0);
    let miss_tokens = usage
        .prompt_cache_miss_tokens
        .unwrap_or_else(|| usage.input_tokens.saturating_sub(hit_tokens));
    let accounted_input = hit_tokens.saturating_add(miss_tokens);
    let uncategorized_input = usage.input_tokens.saturating_sub(accounted_input);

    let hit_cost = (hit_tokens as f64 / 1_000_000.0) * pricing.input_cache_hit_per_million;
    let miss_cost = ((miss_tokens.saturating_add(uncategorized_input)) as f64 / 1_000_000.0)
        * pricing.input_cache_miss_per_million;
    let output_cost = (usage.output_tokens as f64 / 1_000_000.0) * pricing.output_per_million;
    Some(hit_cost + miss_cost + output_cost)
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
