use super::CommandResult;
use crate::budget::BudgetState;
use crate::pricing::{CostCurrency, format_cost_amount};
use crate::tui::app::App;

const BUDGET_USAGE: &str = "/budget [set hard <usd>|set soft <usd>|extend <usd>|release|downgrade]";

pub fn budget(app: &mut App, arg: Option<&str>) -> CommandResult {
    let sub = arg.unwrap_or("").trim();
    if sub.is_empty() {
        return CommandResult::message(summary(app));
    }

    let parts = sub.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        ["set", "hard", amount] => set_hard(app, amount),
        ["set", "soft", amount] => set_soft(app, amount),
        ["extend", amount] => extend(app, amount),
        ["release"] => {
            app.session.budget_overrides.released = true;
            app.session.budget_overrides.session_usd_hard = None;
            app.session.budget_overrides.session_usd_soft = None;
            app.session.budget_state = BudgetState::Active;
            CommandResult::message("budget caps released for this session")
        }
        ["downgrade"] => super::switch_model(app, "deepseek-v4-flash"),
        ["help"] => CommandResult::message(BUDGET_USAGE),
        _ => CommandResult::error(format!("unknown budget command. Usage: {BUDGET_USAGE}")),
    }
}

fn set_hard(app: &mut App, amount: &str) -> CommandResult {
    let Ok(value) = parse_amount(amount) else {
        return CommandResult::error("hard cap must be a positive USD amount");
    };
    app.session.budget_overrides.released = false;
    app.session.budget_overrides.session_usd_hard = Some(value);
    app.session.budget_hard_fired_usd = false;
    CommandResult::message(format!(
        "session hard budget set to {}",
        format_cost_amount(value, CostCurrency::Usd)
    ))
}

fn set_soft(app: &mut App, amount: &str) -> CommandResult {
    let Ok(value) = parse_amount(amount) else {
        return CommandResult::error("soft cap must be a positive USD amount");
    };
    app.session.budget_overrides.released = false;
    app.session.budget_overrides.session_usd_soft = Some(value);
    app.session.budget_soft_fired_usd = false;
    CommandResult::message(format!(
        "session soft budget set to {}",
        format_cost_amount(value, CostCurrency::Usd)
    ))
}

fn extend(app: &mut App, amount: &str) -> CommandResult {
    let Ok(value) = parse_amount(amount) else {
        return CommandResult::error("extension must be a positive USD amount");
    };
    let current = app.displayed_session_cost_for_currency(CostCurrency::Usd);
    let base = app
        .session
        .budget_overrides
        .session_usd_hard
        .or_else(|| {
            app.budget_config
                .as_ref()
                .and_then(|cfg| cfg.session_usd_hard)
        })
        .unwrap_or(current);
    let new_cap = base.max(current) + value;
    app.session.budget_overrides.released = false;
    app.session.budget_overrides.session_usd_hard = Some(new_cap);
    app.session.budget_state = BudgetState::Active;
    app.session.budget_hard_fired_usd = false;
    CommandResult::message(format!(
        "session hard budget extended to {}",
        format_cost_amount(new_cap, CostCurrency::Usd)
    ))
}

fn parse_amount(raw: &str) -> Result<f64, ()> {
    let value = raw
        .trim()
        .trim_start_matches('$')
        .parse::<f64>()
        .map_err(|_| ())?;
    if value > 0.0 && value.is_finite() {
        Ok(value)
    } else {
        Err(())
    }
}

fn summary(app: &App) -> String {
    let config = app.budget_config.as_ref();
    let soft_usd = app
        .session
        .budget_overrides
        .session_usd_soft
        .or_else(|| config.and_then(|cfg| cfg.session_usd_soft));
    let hard_usd = app
        .session
        .budget_overrides
        .session_usd_hard
        .or_else(|| config.and_then(|cfg| cfg.session_usd_hard));
    let soft_cny = config.and_then(|cfg| cfg.session_cny_soft);
    let hard_cny = config.and_then(|cfg| cfg.session_cny_hard);
    let daily_usd = config.and_then(|cfg| cfg.daily_usd_hard);
    let strategy = config
        .map(|cfg| format!("{:?}", cfg.on_exceed))
        .unwrap_or_else(|| "disabled".to_string());

    format!(
        "Session: {} / {}\nCaps: soft {} {}, hard {} {}\nDaily: {} accumulated, hard {}\nState: {:?}\nStrategy: {}",
        format_cost_amount(app.session.session_cost, CostCurrency::Usd),
        format_cost_amount(app.session.session_cost_cny, CostCurrency::Cny),
        cap(soft_usd, CostCurrency::Usd),
        cap(soft_cny, CostCurrency::Cny),
        cap(hard_usd, CostCurrency::Usd),
        cap(hard_cny, CostCurrency::Cny),
        format_cost_amount(app.session.daily_cost.usd, CostCurrency::Usd),
        cap(daily_usd, CostCurrency::Usd),
        app.session.budget_state,
        strategy
    )
}

fn cap(value: Option<f64>, currency: CostCurrency) -> String {
    value
        .map(|v| format_cost_amount(v, currency))
        .unwrap_or_else(|| "unset".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::app::{App, TuiOptions};
    use std::path::PathBuf;

    fn test_app() -> App {
        App::new(
            TuiOptions {
                model: "deepseek-v4-pro".to_string(),
                workspace: PathBuf::from("."),
                config_path: None,
                config_profile: None,
                allow_shell: false,
                use_alt_screen: true,
                use_mouse_capture: false,
                use_bracketed_paste: true,
                max_subagents: 1,
                skills_dir: PathBuf::from("."),
                memory_path: PathBuf::from("memory.md"),
                notes_path: PathBuf::from("notes.txt"),
                mcp_config_path: PathBuf::from("mcp.json"),
                use_memory: false,
                start_in_agent_mode: false,
                skip_onboarding: true,
                yolo: false,
                resume_session_id: None,
                initial_input: None,
            },
            &Config::default(),
        )
    }

    #[test]
    fn budget_extend_resumes_from_paused() {
        let mut app = test_app();
        app.session.budget_state = BudgetState::Paused {
            reason: "test".to_string(),
        };
        app.session.budget_overrides.session_usd_hard = Some(1.0);

        let result = budget(&mut app, Some("extend 2"));

        assert!(!result.is_error);
        assert_eq!(app.session.budget_state, BudgetState::Active);
        assert_eq!(app.session.budget_overrides.session_usd_hard, Some(3.0));
    }
}
