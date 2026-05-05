//! Cost estimation for DeepSeek API usage.
//!
//! Pricing based on DeepSeek's published rates (per million tokens).

use chrono::{DateTime, TimeZone, Utc};

use crate::models::Usage;

/// Per-million-token pricing for a model.
struct ModelPricing {
    input_cache_hit_per_million: f64,
    input_cache_miss_per_million: f64,
    output_per_million: f64,
}

fn v4_pro_discount_ends_at() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 31, 15, 59, 0)
        .single()
        .expect("valid DeepSeek V4 Pro discount end timestamp")
}

/// Look up pricing for a model name.
fn pricing_for_model(model: &str) -> Option<ModelPricing> {
    pricing_for_model_at(model, Utc::now())
}

fn pricing_for_model_at(model: &str, now: DateTime<Utc>) -> Option<ModelPricing> {
    let lower = model.to_lowercase();
    if lower.starts_with("deepseek-ai/") {
        // NVIDIA NIM-hosted DeepSeek uses NVIDIA's catalog/account terms, not
        // DeepSeek Platform pricing. Avoid showing misleading DeepSeek costs.
        return None;
    }
    if !lower.contains("deepseek") {
        return None;
    }
    if lower.contains("v4-pro") || lower.contains("v4pro") {
        if now <= v4_pro_discount_ends_at() {
            // DeepSeek lists these as a limited-time 75% discount through
            // 2026-05-31 15:59 UTC.
            return Some(ModelPricing {
                input_cache_hit_per_million: 0.003625,
                input_cache_miss_per_million: 0.435,
                output_per_million: 0.87,
            });
        }
        Some(ModelPricing {
            input_cache_hit_per_million: 0.0145,
            input_cache_miss_per_million: 1.74,
            output_per_million: 3.48,
        })
    } else {
        // deepseek-v4-flash pricing.
        Some(ModelPricing {
            input_cache_hit_per_million: 0.0028,
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
    Some(calculate_turn_cost_with_pricing(
        pricing,
        input_tokens,
        output_tokens,
    ))
}

fn calculate_turn_cost_with_pricing(
    pricing: ModelPricing,
    input_tokens: u32,
    output_tokens: u32,
) -> f64 {
    let input_cost = (input_tokens as f64 / 1_000_000.0) * pricing.input_cache_miss_per_million;
    let output_cost = (output_tokens as f64 / 1_000_000.0) * pricing.output_per_million;
    input_cost + output_cost
}

/// Calculate cost from provider usage, honoring DeepSeek context-cache fields.
#[must_use]
pub fn calculate_turn_cost_from_usage(model: &str, usage: &Usage) -> Option<f64> {
    let pricing = pricing_for_model(model)?;
    Some(calculate_turn_cost_from_usage_with_pricing(pricing, usage))
}

fn calculate_turn_cost_from_usage_with_pricing(pricing: ModelPricing, usage: &Usage) -> f64 {
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
    hit_cost + miss_cost + output_cost
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_nim_deepseek_model_does_not_use_deepseek_platform_pricing() {
        assert!(calculate_turn_cost("deepseek-ai/deepseek-v4-pro", 1_000, 1_000).is_none());
    }

    #[test]
    fn v4_pro_uses_limited_time_discount_before_expiry() {
        let before_expiry = Utc
            .with_ymd_and_hms(2026, 5, 31, 15, 58, 59)
            .single()
            .unwrap();
        let pricing = pricing_for_model_at("deepseek-v4-pro", before_expiry).unwrap();

        assert_eq!(pricing.input_cache_hit_per_million, 0.003625);
        assert_eq!(pricing.input_cache_miss_per_million, 0.435);
        assert_eq!(pricing.output_per_million, 0.87);
    }

    #[test]
    fn v4_pro_returns_to_base_rates_after_discount_expiry() {
        let after_expiry = Utc
            .with_ymd_and_hms(2026, 5, 31, 16, 0, 0)
            .single()
            .unwrap();
        let pricing = pricing_for_model_at("deepseek-v4-pro", after_expiry).unwrap();

        assert_eq!(pricing.input_cache_hit_per_million, 0.0145);
        assert_eq!(pricing.input_cache_miss_per_million, 1.74);
        assert_eq!(pricing.output_per_million, 3.48);
    }

    #[test]
    fn v4_pro_discount_still_applies_just_before_old_may5_expiry() {
        // Regression for #267: extension to 2026-05-31 15:59 UTC.
        let after_old_expiry = Utc.with_ymd_and_hms(2026, 5, 6, 0, 0, 0).single().unwrap();
        let pricing = pricing_for_model_at("deepseek-v4-pro", after_old_expiry).unwrap();

        assert_eq!(pricing.input_cache_hit_per_million, 0.003625);
        assert_eq!(pricing.input_cache_miss_per_million, 0.435);
        assert_eq!(pricing.output_per_million, 0.87);
    }

    #[test]
    fn v4_flash_keeps_current_published_rates() {
        let now = Utc.with_ymd_and_hms(2026, 4, 25, 0, 0, 0).single().unwrap();
        let pricing = pricing_for_model_at("deepseek-v4-flash", now).unwrap();

        assert_eq!(pricing.input_cache_hit_per_million, 0.0028);
        assert_eq!(pricing.input_cache_miss_per_million, 0.14);
        assert_eq!(pricing.output_per_million, 0.28);
    }
}
