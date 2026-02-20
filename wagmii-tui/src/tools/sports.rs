//! Sports tool for schedules and standings (ESPN public APIs).

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_str, optional_u64, required_str,
};
use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::Duration;

const TIMEOUT_MS: u64 = 15_000;
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

#[derive(Debug, Clone, Serialize)]
struct SportsGameTeam {
    name: String,
    abbreviation: String,
    score: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SportsGame {
    id: String,
    date: String,
    status: String,
    home: SportsGameTeam,
    away: SportsGameTeam,
}

#[derive(Debug, Clone, Serialize)]
struct SportsScheduleResponse {
    league: String,
    games: Vec<SportsGame>,
}

#[derive(Debug, Clone, Serialize)]
struct SportsStandingEntry {
    name: String,
    abbreviation: String,
    stats: Value,
}

#[derive(Debug, Clone, Serialize)]
struct SportsStandingsResponse {
    league: String,
    entries: Vec<SportsStandingEntry>,
}

pub struct SportsTool;

#[async_trait]
impl ToolSpec for SportsTool {
    fn name(&self) -> &'static str {
        "sports"
    }

    fn description(&self) -> &'static str {
        "Get sports schedules or standings for a league."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "fn": { "type": "string", "enum": ["schedule", "standings"] },
                "league": { "type": "string" },
                "team": { "type": "string" },
                "opponent": { "type": "string" },
                "date_from": { "type": "string", "description": "YYYY-MM-DD" },
                "date_to": { "type": "string", "description": "YYYY-MM-DD" },
                "num_games": { "type": "integer" },
                "locale": { "type": "string" }
            },
            "required": ["fn", "league"]
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
        let action = required_str(&input, "fn")?.to_lowercase();
        let league = required_str(&input, "league")?.to_lowercase();
        let team = optional_str(&input, "team").map(|s| s.to_string());
        let opponent = optional_str(&input, "opponent").map(|s| s.to_string());
        let date_from = optional_str(&input, "date_from").map(|s| s.to_string());
        let date_to = optional_str(&input, "date_to").map(|s| s.to_string());
        let num_games = optional_u64(&input, "num_games", 20) as usize;

        let (sport, league_code) = map_league(&league)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(TIMEOUT_MS))
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| {
                ToolError::execution_failed(format!("Failed to build HTTP client: {e}"))
            })?;

        match action.as_str() {
            "schedule" => {
                let schedule = fetch_schedule(
                    &client,
                    &sport,
                    &league_code,
                    date_from.as_deref(),
                    date_to.as_deref(),
                    team.as_deref(),
                    opponent.as_deref(),
                    num_games,
                )
                .await?;
                ToolResult::json(&schedule).map_err(|e| ToolError::execution_failed(e.to_string()))
            }
            "standings" => {
                let standings = fetch_standings(&client, &sport, &league_code).await?;
                ToolResult::json(&standings).map_err(|e| ToolError::execution_failed(e.to_string()))
            }
            _ => Err(ToolError::invalid_input("fn must be schedule or standings")),
        }
    }
}

fn map_league(league: &str) -> Result<(String, String), ToolError> {
    match league {
        "nba" => Ok(("basketball".to_string(), "nba".to_string())),
        "wnba" => Ok(("basketball".to_string(), "wnba".to_string())),
        "nfl" => Ok(("football".to_string(), "nfl".to_string())),
        "nhl" => Ok(("hockey".to_string(), "nhl".to_string())),
        "mlb" => Ok(("baseball".to_string(), "mlb".to_string())),
        "epl" => Ok(("soccer".to_string(), "eng.1".to_string())),
        "ncaamb" => Ok((
            "basketball".to_string(),
            "mens-college-basketball".to_string(),
        )),
        "ncaawb" => Ok((
            "basketball".to_string(),
            "womens-college-basketball".to_string(),
        )),
        "ipl" => Ok(("cricket".to_string(), "ipl".to_string())),
        _ => Err(ToolError::invalid_input("Unsupported league")),
    }
}

#[allow(clippy::too_many_arguments)]
async fn fetch_schedule(
    client: &reqwest::Client,
    sport: &str,
    league: &str,
    date_from: Option<&str>,
    date_to: Option<&str>,
    team: Option<&str>,
    opponent: Option<&str>,
    num_games: usize,
) -> Result<SportsScheduleResponse, ToolError> {
    let date_param = build_date_param(date_from, date_to)?;
    let url = if let Some(dates) = date_param {
        format!(
            "https://site.web.api.espn.com/apis/v2/sports/{sport}/{league}/scoreboard?dates={dates}"
        )
    } else {
        format!("https://site.web.api.espn.com/apis/v2/sports/{sport}/{league}/scoreboard")
    };

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| ToolError::execution_failed(format!("Schedule request failed: {e}")))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| ToolError::execution_failed(format!("Failed to read response: {e}")))?;
    if !status.is_success() {
        return Err(ToolError::execution_failed(format!(
            "Schedule failed: HTTP {}",
            status.as_u16()
        )));
    }

    let json: Value = serde_json::from_str(&body)
        .map_err(|e| ToolError::execution_failed(format!("Invalid schedule JSON: {e}")))?;
    let events = json
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut games = Vec::new();
    for event in events.iter() {
        let competitions = match event.get("competitions").and_then(|v| v.as_array()) {
            Some(list) => list,
            None => continue,
        };
        let competition = match competitions.first() {
            Some(comp) => comp,
            None => continue,
        };
        let competitors = match competition.get("competitors").and_then(|v| v.as_array()) {
            Some(list) => list,
            None => continue,
        };
        if competitors.len() < 2 {
            continue;
        }
        let (home, away) = split_competitors(competitors);
        if let (Some(home), Some(away)) = (home, away) {
            if let Some(team_filter) = team {
                if !team_matches(&home, team_filter) && !team_matches(&away, team_filter) {
                    continue;
                }
            }
            if let Some(opponent_filter) = opponent {
                if !team_matches(&home, opponent_filter) && !team_matches(&away, opponent_filter) {
                    continue;
                }
            }
            let status = competition
                .get("status")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.get("description"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();
            let game = SportsGame {
                id: event
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                date: event
                    .get("date")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                status,
                home,
                away,
            };
            games.push(game);
        }
        if games.len() >= num_games {
            break;
        }
    }

    Ok(SportsScheduleResponse {
        league: league.to_string(),
        games,
    })
}

fn split_competitors(competitors: &[Value]) -> (Option<SportsGameTeam>, Option<SportsGameTeam>) {
    let mut home = None;
    let mut away = None;
    for comp in competitors {
        let team = comp.get("team").and_then(|v| v.as_object());
        let name = team
            .and_then(|t| t.get("displayName"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let abbreviation = team
            .and_then(|t| t.get("abbreviation"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let score = comp
            .get("score")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let home_away = comp.get("homeAway").and_then(|v| v.as_str()).unwrap_or("");
        let team_info = SportsGameTeam {
            name,
            abbreviation,
            score,
        };
        if home_away == "home" {
            home = Some(team_info);
        } else {
            away = Some(team_info);
        }
    }
    (home, away)
}

fn team_matches(team: &SportsGameTeam, filter: &str) -> bool {
    let req = filter.to_lowercase();
    team.abbreviation.to_lowercase() == req || team.name.to_lowercase().contains(&req)
}

async fn fetch_standings(
    client: &reqwest::Client,
    sport: &str,
    league: &str,
) -> Result<SportsStandingsResponse, ToolError> {
    let url = format!("https://site.web.api.espn.com/apis/v2/sports/{sport}/{league}/standings");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| ToolError::execution_failed(format!("Standings request failed: {e}")))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| ToolError::execution_failed(format!("Failed to read response: {e}")))?;
    if !status.is_success() {
        return Err(ToolError::execution_failed(format!(
            "Standings failed: HTTP {}",
            status.as_u16()
        )));
    }
    let json: Value = serde_json::from_str(&body)
        .map_err(|e| ToolError::execution_failed(format!("Invalid standings JSON: {e}")))?;

    let entries = collect_standings_entries(&json);
    let mut output = Vec::new();
    for entry in entries {
        let team = entry.get("team").and_then(|v| v.as_object());
        let name = team
            .and_then(|t| t.get("displayName"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let abbreviation = team
            .and_then(|t| t.get("abbreviation"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let stats = entry
            .get("stats")
            .and_then(|v| v.as_array())
            .map(|stats| {
                let mut map = serde_json::Map::new();
                for stat in stats {
                    if let Some(name) = stat.get("name").and_then(|v| v.as_str()) {
                        if let Some(val) = stat.get("displayValue").or_else(|| stat.get("value")) {
                            map.insert(name.to_string(), val.clone());
                        }
                    }
                }
                Value::Object(map)
            })
            .unwrap_or_else(|| json!({}));
        output.push(SportsStandingEntry {
            name,
            abbreviation,
            stats,
        });
    }

    Ok(SportsStandingsResponse {
        league: league.to_string(),
        entries: output,
    })
}

fn collect_standings_entries(value: &Value) -> Vec<Value> {
    if let Some(entries) = value
        .get("standings")
        .and_then(|v| v.get("entries"))
        .and_then(|v| v.as_array())
    {
        return entries.clone();
    }

    if let Some(children) = value.get("children").and_then(|v| v.as_array()) {
        let mut entries = Vec::new();
        for child in children {
            entries.extend(collect_standings_entries(child));
        }
        return entries;
    }

    Vec::new()
}

fn build_date_param(
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> Result<Option<String>, ToolError> {
    let start = match date_from {
        Some(date) => NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|_| ToolError::invalid_input("date_from must be YYYY-MM-DD"))?,
        None => Utc::now().date_naive(),
    };

    if let Some(date_to) = date_to {
        let end = NaiveDate::parse_from_str(date_to, "%Y-%m-%d")
            .map_err(|_| ToolError::invalid_input("date_to must be YYYY-MM-DD"))?;
        if end < start {
            return Err(ToolError::invalid_input("date_to must be >= date_from"));
        }
        let diff = (end - start).num_days();
        if diff > 7 {
            return Ok(Some(start.format("%Y%m%d").to_string()));
        }
        return Ok(Some(format!(
            "{}-{}",
            start.format("%Y%m%d"),
            end.format("%Y%m%d")
        )));
    }

    Ok(Some(start.format("%Y%m%d").to_string()))
}
