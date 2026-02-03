//! Time tool for returning the current time at a given UTC offset.

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, required_str,
};
use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, Utc};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize)]
struct TimeRequest {
    utc_offset: String,
}

#[derive(Debug, Clone, Serialize)]
struct TimeResult {
    utc_offset: String,
    datetime: String,
    date: String,
    time: String,
}

#[derive(Debug, Clone, Serialize)]
struct TimeResponse {
    results: Vec<TimeResult>,
}

pub struct TimeTool;

#[async_trait]
impl ToolSpec for TimeTool {
    fn name(&self) -> &'static str {
        "time"
    }

    fn description(&self) -> &'static str {
        "Get the current time for a given UTC offset."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "utc_offset": {
                    "type": "string",
                    "description": "UTC offset like +03:00 or -07:00"
                },
                "time": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "utc_offset": { "type": "string" }
                        },
                        "required": ["utc_offset"]
                    }
                }
            }
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let requests = parse_time_requests(&input)?;
        let mut results = Vec::with_capacity(requests.len());

        for req in requests {
            let offset = parse_offset(&req.utc_offset)?;
            let now: DateTime<FixedOffset> = Utc::now().with_timezone(&offset);
            results.push(TimeResult {
                utc_offset: req.utc_offset,
                datetime: now.to_rfc3339(),
                date: now.format("%Y-%m-%d").to_string(),
                time: now.format("%H:%M:%S").to_string(),
            });
        }

        ToolResult::json(&TimeResponse { results })
            .map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

fn parse_time_requests(input: &Value) -> Result<Vec<TimeRequest>, ToolError> {
    if let Some(list) = input.get("time").and_then(|v| v.as_array()) {
        let mut requests = Vec::new();
        for item in list {
            let offset = required_str(item, "utc_offset")?.to_string();
            requests.push(TimeRequest { utc_offset: offset });
        }
        if requests.is_empty() {
            return Err(ToolError::invalid_input("time list is empty"));
        }
        return Ok(requests);
    }

    let offset = required_str(input, "utc_offset")?.to_string();
    Ok(vec![TimeRequest { utc_offset: offset }])
}

fn parse_offset(raw: &str) -> Result<FixedOffset, ToolError> {
    let raw = raw.trim();
    if raw.len() != 6 {
        return Err(ToolError::invalid_input(
            "utc_offset must be formatted like +03:00",
        ));
    }
    let sign = match &raw[0..1] {
        "+" => 1,
        "-" => -1,
        _ => {
            return Err(ToolError::invalid_input(
                "utc_offset must start with + or -",
            ))
        }
    };
    let hours: i32 = raw[1..3]
        .parse()
        .map_err(|_| ToolError::invalid_input("Invalid utc_offset hours"))?;
    let minutes: i32 = raw[4..6]
        .parse()
        .map_err(|_| ToolError::invalid_input("Invalid utc_offset minutes"))?;
    if &raw[3..4] != ":" {
        return Err(ToolError::invalid_input(
            "utc_offset must include ':' separator",
        ));
    }
    let total_seconds = sign * (hours * 3600 + minutes * 60);
    FixedOffset::east_opt(total_seconds)
        .ok_or_else(|| ToolError::invalid_input("Invalid utc_offset range"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_offset() {
        let offset = parse_offset("+03:00").expect("offset");
        assert_eq!(offset.local_minus_utc(), 3 * 3600);
    }
}
