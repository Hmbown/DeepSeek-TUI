//! Finance tool for stock/crypto pricing.

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec, required_str,
};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use std::time::Duration;

const TIMEOUT_MS: u64 = 15_000;
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

#[derive(Debug, Clone, Serialize)]
struct FinanceRequest {
    ticker: String,
    instrument_type: String,
    market: String,
}

#[derive(Debug, Clone, Serialize)]
struct FinanceResult {
    ticker: String,
    instrument_type: String,
    market: String,
    source: String,
    price: Option<f64>,
    currency: Option<String>,
    as_of: Option<String>,
    details: Value,
}

#[derive(Debug, Clone, Serialize)]
struct FinanceResponse {
    results: Vec<FinanceResult>,
}

pub struct FinanceTool;

#[async_trait]
impl ToolSpec for FinanceTool {
    fn name(&self) -> &'static str {
        "finance"
    }

    fn description(&self) -> &'static str {
        "Get the latest price for a stock, fund, index, or cryptocurrency."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ticker": { "type": "string" },
                "type": { "type": "string", "enum": ["equity", "fund", "crypto", "index"] },
                "market": { "type": "string" },
                "finance": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "ticker": { "type": "string" },
                            "type": { "type": "string" },
                            "market": { "type": "string" }
                        },
                        "required": ["ticker", "type"]
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
        let requests = parse_finance_requests(&input)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(TIMEOUT_MS))
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| ToolError::execution_failed(format!("Failed to build HTTP client: {e}")))?;

        let mut results = Vec::with_capacity(requests.len());
        for req in requests {
            let instrument_type = req.instrument_type.to_lowercase();
            let result = if instrument_type == "crypto" {
                fetch_crypto_price(&client, &req).await?
            } else {
                fetch_stooq_price(&client, &req).await?
            };
            results.push(result);
        }

        ToolResult::json(&FinanceResponse { results })
            .map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

fn parse_finance_requests(input: &Value) -> Result<Vec<FinanceRequest>, ToolError> {
    if let Some(list) = input.get("finance").and_then(|v| v.as_array()) {
        let mut requests = Vec::new();
        for item in list {
            let ticker = required_str(item, "ticker")?.to_string();
            let instrument_type = required_str(item, "type")?.to_string();
            let market = item
                .get("market")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            requests.push(FinanceRequest {
                ticker,
                instrument_type,
                market,
            });
        }
        if requests.is_empty() {
            return Err(ToolError::invalid_input("finance list is empty"));
        }
        return Ok(requests);
    }

    let ticker = required_str(input, "ticker")?.to_string();
    let instrument_type = required_str(input, "type")?.to_string();
    let market = input
        .get("market")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(vec![FinanceRequest {
        ticker,
        instrument_type,
        market,
    }])
}

async fn fetch_crypto_price(
    client: &reqwest::Client,
    req: &FinanceRequest,
) -> Result<FinanceResult, ToolError> {
    let search_url = format!(
        "https://api.coingecko.com/api/v3/search?query={}",
        url_encode(&req.ticker)
    );
    let search_resp = client
        .get(&search_url)
        .send()
        .await
        .map_err(|e| ToolError::execution_failed(format!("CoinGecko search failed: {e}")))?;
    let status = search_resp.status();
    let body = search_resp
        .text()
        .await
        .map_err(|e| ToolError::execution_failed(format!("Failed to read response: {e}")))?;
    if !status.is_success() {
        return Err(ToolError::execution_failed(format!(
            "CoinGecko search failed: HTTP {}",
            status.as_u16()
        )));
    }
    let json: Value = serde_json::from_str(&body)
        .map_err(|e| ToolError::execution_failed(format!("Invalid CoinGecko JSON: {e}")))?;
    let coins = json
        .get("coins")
        .and_then(|v| v.as_array())
        .ok_or_else(|| ToolError::execution_failed("CoinGecko returned no coins"))?;
    let ticker_lower = req.ticker.to_lowercase();
    let selected = coins
        .iter()
        .find(|coin| {
            coin.get("symbol")
                .and_then(|v| v.as_str())
                .map(|s| s.eq_ignore_ascii_case(&ticker_lower))
                .unwrap_or(false)
        })
        .or_else(|| coins.first())
        .ok_or_else(|| ToolError::execution_failed("CoinGecko returned no results"))?;

    let id = selected
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::execution_failed("Missing CoinGecko id"))?;
    let symbol = selected
        .get("symbol")
        .and_then(|v| v.as_str())
        .unwrap_or(&req.ticker)
        .to_string();
    let name = selected
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&req.ticker)
        .to_string();

    let price_url = format!(
        "https://api.coingecko.com/api/v3/simple/price?ids={id}&vs_currencies=usd&include_last_updated_at=true"
    );
    let price_resp = client
        .get(&price_url)
        .send()
        .await
        .map_err(|e| ToolError::execution_failed(format!("CoinGecko price failed: {e}")))?;
    let status = price_resp.status();
    let body = price_resp
        .text()
        .await
        .map_err(|e| ToolError::execution_failed(format!("Failed to read response: {e}")))?;
    if !status.is_success() {
        return Err(ToolError::execution_failed(format!(
            "CoinGecko price failed: HTTP {}",
            status.as_u16()
        )));
    }
    let json: Value = serde_json::from_str(&body)
        .map_err(|e| ToolError::execution_failed(format!("Invalid CoinGecko price JSON: {e}")))?;
    let price = json
        .get(id)
        .and_then(|v| v.get("usd"))
        .and_then(|v| v.as_f64());
    let last_updated = json
        .get(id)
        .and_then(|v| v.get("last_updated_at"))
        .and_then(|v| v.as_i64())
        .map(|ts| format!("{ts}"));

    Ok(FinanceResult {
        ticker: req.ticker.clone(),
        instrument_type: req.instrument_type.clone(),
        market: req.market.clone(),
        source: "coingecko".to_string(),
        price,
        currency: Some("USD".to_string()),
        as_of: last_updated,
        details: json!({
            "id": id,
            "symbol": symbol,
            "name": name,
        }),
    })
}

async fn fetch_stooq_price(
    client: &reqwest::Client,
    req: &FinanceRequest,
) -> Result<FinanceResult, ToolError> {
    let symbol = normalize_stooq_symbol(&req.ticker, &req.market);
    let url = format!(
        "https://stooq.com/q/l/?s={symbol}&f=sd2t2ohlcv&h&e=csv"
    );
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| ToolError::execution_failed(format!("Stooq request failed: {e}")))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| ToolError::execution_failed(format!("Failed to read response: {e}")))?;
    if !status.is_success() {
        return Err(ToolError::execution_failed(format!(
            "Stooq failed: HTTP {}",
            status.as_u16()
        )));
    }

    let mut lines = body.lines();
    let _header = lines.next();
    let data = lines
        .next()
        .ok_or_else(|| ToolError::execution_failed("Stooq returned no data"))?;
    let fields: Vec<&str> = data.split(',').collect();
    if fields.len() < 8 {
        return Err(ToolError::execution_failed("Stooq data malformed"));
    }
    if fields[1] == "N/D" {
        return Err(ToolError::execution_failed("Stooq returned no data"));
    }

    let date = fields[1].to_string();
    let time = fields[2].to_string();
    let open = parse_f64(fields[3]);
    let high = parse_f64(fields[4]);
    let low = parse_f64(fields[5]);
    let close = parse_f64(fields[6]);
    let volume = parse_f64(fields[7]);

    Ok(FinanceResult {
        ticker: req.ticker.clone(),
        instrument_type: req.instrument_type.clone(),
        market: req.market.clone(),
        source: "stooq".to_string(),
        price: close,
        currency: None,
        as_of: Some(format!("{date} {time}")),
        details: json!({
            "symbol": symbol,
            "open": open,
            "high": high,
            "low": low,
            "close": close,
            "volume": volume,
            "date": date,
            "time": time,
        }),
    })
}

fn normalize_stooq_symbol(ticker: &str, market: &str) -> String {
    if ticker.contains('.') {
        return ticker.to_lowercase();
    }
    let suffix = match market.to_lowercase().as_str() {
        "usa" | "us" => ".us",
        "uk" | "gb" => ".uk",
        "jp" | "japan" => ".jp",
        "de" | "germany" => ".de",
        "fr" | "france" => ".fr",
        "ca" | "canada" => ".ca",
        _ => "",
    };
    format!("{}{}", ticker.to_lowercase(), suffix)
}

fn parse_f64(input: &str) -> Option<f64> {
    input.parse::<f64>().ok()
}

fn url_encode(input: &str) -> String {
    let mut encoded = String::new();
    for ch in input.bytes() {
        match ch {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~' => encoded.push(ch as char),
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{ch:02X}")),
        }
    }
    encoded
}
