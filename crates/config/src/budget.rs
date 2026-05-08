use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct BudgetConfig {
    #[serde(default, deserialize_with = "zero_as_none")]
    pub session_usd_soft: Option<f64>,
    #[serde(default, deserialize_with = "zero_as_none")]
    pub session_usd_hard: Option<f64>,
    #[serde(default, deserialize_with = "zero_as_none")]
    pub session_cny_soft: Option<f64>,
    #[serde(default, deserialize_with = "zero_as_none")]
    pub session_cny_hard: Option<f64>,
    #[serde(default, deserialize_with = "zero_as_none")]
    pub daily_usd_hard: Option<f64>,
    #[serde(default)]
    pub on_exceed: OnExceedStrategy,
    #[serde(default = "default_include_subagents")]
    pub include_subagents: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OnExceedStrategy {
    #[default]
    Pause,
    DowngradeToFlash,
    Stop,
}

fn default_include_subagents() -> bool {
    true
}

fn zero_as_none<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<f64>::deserialize(deserializer)?;
    Ok(value.filter(|v| *v > 0.0))
}
