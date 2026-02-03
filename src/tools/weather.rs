//! Weather tool backed by Open-Meteo (no API key required).

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_str, optional_u64, required_str,
};
use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::Duration;

const DEFAULT_DAYS: u64 = 7;
const MAX_DAYS: u64 = 14;
const TIMEOUT_MS: u64 = 15_000;
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

#[derive(Debug, Clone, Serialize)]
struct WeatherRequest {
    location: String,
    start: String,
    duration: u64,
}

#[derive(Debug, Clone, Serialize)]
struct WeatherDay {
    date: String,
    temp_max_c: f64,
    temp_min_c: f64,
    temp_max_f: f64,
    temp_min_f: f64,
    precip_mm: f64,
}

#[derive(Debug, Clone, Serialize)]
struct WeatherResult {
    location: String,
    resolved_name: String,
    latitude: f64,
    longitude: f64,
    timezone: String,
    source: String,
    days: Vec<WeatherDay>,
}

#[derive(Debug, Clone, Serialize)]
struct WeatherResponse {
    results: Vec<WeatherResult>,
}

pub struct WeatherTool;

#[async_trait]
impl ToolSpec for WeatherTool {
    fn name(&self) -> &'static str {
        "weather"
    }

    fn description(&self) -> &'static str {
        "Get a daily weather forecast for a location."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "location": { "type": "string" },
                "start": { "type": "string", "description": "YYYY-MM-DD" },
                "duration": { "type": "integer", "description": "Number of days" },
                "weather": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "location": { "type": "string" },
                            "start": { "type": "string" },
                            "duration": { "type": "integer" }
                        },
                        "required": ["location"]
                    }
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly, ToolCapability::Network]
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let requests = parse_weather_requests(&input)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(TIMEOUT_MS))
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| {
                ToolError::execution_failed(format!("Failed to build HTTP client: {e}"))
            })?;

        let mut results = Vec::with_capacity(requests.len());
        for req in requests {
            let geo = geocode_location(&client, &req.location).await?;
            let forecast = fetch_forecast(&client, &geo, &req.start, req.duration).await?;
            results.push(WeatherResult {
                location: req.location,
                resolved_name: geo.name,
                latitude: geo.latitude,
                longitude: geo.longitude,
                timezone: forecast.timezone,
                source: "open-meteo".to_string(),
                days: forecast.days,
            });
        }

        ToolResult::json(&WeatherResponse { results })
            .map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

fn parse_weather_requests(input: &Value) -> Result<Vec<WeatherRequest>, ToolError> {
    if let Some(list) = input.get("weather").and_then(|v| v.as_array()) {
        let mut requests = Vec::new();
        for item in list {
            let location = required_str(item, "location")?.to_string();
            let start = optional_str(item, "start")
                .map(|s| s.to_string())
                .unwrap_or_else(|| today_string());
            let duration = optional_u64(item, "duration", DEFAULT_DAYS).clamp(1, MAX_DAYS);
            requests.push(WeatherRequest {
                location,
                start,
                duration,
            });
        }
        if requests.is_empty() {
            return Err(ToolError::invalid_input("weather list is empty"));
        }
        return Ok(requests);
    }

    let location = required_str(input, "location")?.to_string();
    let start = optional_str(input, "start")
        .map(|s| s.to_string())
        .unwrap_or_else(|| today_string());
    let duration = optional_u64(input, "duration", DEFAULT_DAYS).clamp(1, MAX_DAYS);

    Ok(vec![WeatherRequest {
        location,
        start,
        duration,
    }])
}

fn today_string() -> String {
    Utc::now().date_naive().format("%Y-%m-%d").to_string()
}

#[derive(Debug)]
struct GeoResult {
    name: String,
    latitude: f64,
    longitude: f64,
}

async fn geocode_location(
    client: &reqwest::Client,
    location: &str,
) -> Result<GeoResult, ToolError> {
    let encoded = url_encode(location);
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={encoded}&count=1&language=en&format=json"
    );
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| ToolError::execution_failed(format!("Geocoding request failed: {e}")))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| ToolError::execution_failed(format!("Failed to read response: {e}")))?;
    if !status.is_success() {
        return Err(ToolError::execution_failed(format!(
            "Geocoding failed: HTTP {}",
            status.as_u16()
        )));
    }
    let json: Value = serde_json::from_str(&body)
        .map_err(|e| ToolError::execution_failed(format!("Invalid geocoding JSON: {e}")))?;
    let results = json
        .get("results")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ToolError::execution_failed("No geocoding results"))?;
    let first = results
        .first()
        .ok_or_else(|| ToolError::execution_failed("No geocoding results"))?;
    let name = first
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(location)
        .to_string();
    let latitude = first
        .get("latitude")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| ToolError::execution_failed("Missing latitude"))?;
    let longitude = first
        .get("longitude")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| ToolError::execution_failed("Missing longitude"))?;
    Ok(GeoResult {
        name,
        latitude,
        longitude,
    })
}

#[derive(Debug)]
struct ForecastResult {
    timezone: String,
    days: Vec<WeatherDay>,
}

async fn fetch_forecast(
    client: &reqwest::Client,
    geo: &GeoResult,
    start: &str,
    duration: u64,
) -> Result<ForecastResult, ToolError> {
    let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .map_err(|_| ToolError::invalid_input("start must be YYYY-MM-DD"))?;
    let end_date = start_date
        .checked_add_signed(chrono::Duration::days((duration as i64).saturating_sub(1)))
        .ok_or_else(|| ToolError::invalid_input("Invalid duration"))?;
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&daily=temperature_2m_max,temperature_2m_min,precipitation_sum&timezone=auto&start_date={start}&end_date={end}",
        lat = geo.latitude,
        lon = geo.longitude,
        start = start_date.format("%Y-%m-%d"),
        end = end_date.format("%Y-%m-%d"),
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| ToolError::execution_failed(format!("Forecast request failed: {e}")))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| ToolError::execution_failed(format!("Failed to read response: {e}")))?;
    if !status.is_success() {
        return Err(ToolError::execution_failed(format!(
            "Forecast failed: HTTP {}",
            status.as_u16()
        )));
    }

    let json: Value = serde_json::from_str(&body)
        .map_err(|e| ToolError::execution_failed(format!("Invalid forecast JSON: {e}")))?;
    let timezone = json
        .get("timezone")
        .and_then(|v| v.as_str())
        .unwrap_or("UTC")
        .to_string();
    let daily = json
        .get("daily")
        .and_then(|v| v.as_object())
        .ok_or_else(|| ToolError::execution_failed("Missing daily forecast"))?;

    let dates = daily
        .get("time")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ToolError::execution_failed("Missing daily time"))?;
    let maxes = daily
        .get("temperature_2m_max")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ToolError::execution_failed("Missing temperature_2m_max"))?;
    let mins = daily
        .get("temperature_2m_min")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ToolError::execution_failed("Missing temperature_2m_min"))?;
    let precips = daily
        .get("precipitation_sum")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ToolError::execution_failed("Missing precipitation_sum"))?;

    let mut days = Vec::new();
    for idx in 0..dates.len() {
        let date = dates
            .get(idx)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let max_c = maxes.get(idx).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let min_c = mins.get(idx).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let precip = precips.get(idx).and_then(|v| v.as_f64()).unwrap_or(0.0);
        days.push(WeatherDay {
            date,
            temp_max_c: max_c,
            temp_min_c: min_c,
            temp_max_f: c_to_f(max_c),
            temp_min_f: c_to_f(min_c),
            precip_mm: precip,
        });
    }

    Ok(ForecastResult { timezone, days })
}

fn c_to_f(c: f64) -> f64 {
    c * 9.0 / 5.0 + 32.0
}

fn url_encode(input: &str) -> String {
    let mut encoded = String::new();
    for ch in input.bytes() {
        match ch {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(ch as char)
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{ch:02X}")),
        }
    }
    encoded
}
