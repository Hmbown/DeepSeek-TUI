use std::fs;
use std::path::PathBuf;

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum BudgetState {
    #[default]
    Active,
    SoftWarned,
    Paused {
        reason: String,
    },
    Stopped,
}

#[derive(Debug, Clone, Default)]
pub struct BudgetOverrides {
    pub session_usd_soft: Option<f64>,
    pub session_usd_hard: Option<f64>,
    pub released: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyBudgetSnapshot {
    pub date: String,
    pub usd: f64,
    pub cny: f64,
}

#[derive(Debug, Clone)]
pub struct DailyCostCounter {
    path: PathBuf,
    date: NaiveDate,
    pub usd: f64,
    pub cny: f64,
}

impl Default for DailyCostCounter {
    fn default() -> Self {
        Self {
            path: default_daily_budget_path(),
            date: Local::now().date_naive(),
            usd: 0.0,
            cny: 0.0,
        }
    }
}

impl DailyCostCounter {
    pub fn load_default() -> Self {
        let path = default_daily_budget_path();
        Self::load_at(path, Local::now().date_naive())
    }

    pub fn load_at(path: PathBuf, today: NaiveDate) -> Self {
        let mut counter = Self {
            path,
            date: today,
            usd: 0.0,
            cny: 0.0,
        };
        let Ok(raw) = fs::read_to_string(&counter.path) else {
            return counter;
        };
        let Ok(snapshot) = serde_json::from_str::<DailyBudgetSnapshot>(&raw) else {
            return counter;
        };
        if snapshot.date == today.to_string() {
            counter.usd = snapshot.usd.max(0.0);
            counter.cny = snapshot.cny.max(0.0);
        } else {
            let _ = counter.save();
        }
        counter
    }

    pub fn accrue(&mut self, usd: f64, cny: f64) {
        self.reset_if_needed(Local::now().date_naive());
        self.usd += usd.max(0.0);
        self.cny += cny.max(0.0);
        let _ = self.save();
    }

    pub fn reset_if_needed(&mut self, today: NaiveDate) {
        if self.date != today {
            self.date = today;
            self.usd = 0.0;
            self.cny = 0.0;
            let _ = self.save();
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let snapshot = DailyBudgetSnapshot {
            date: self.date.to_string(),
            usd: self.usd,
            cny: self.cny,
        };
        let raw = serde_json::to_string_pretty(&snapshot).map_err(std::io::Error::other)?;
        fs::write(&self.path, raw)
    }
}

fn default_daily_budget_path() -> PathBuf {
    dirs::home_dir()
        .map(|home| {
            home.join(".deepseek")
                .join("state")
                .join("daily_budget.json")
        })
        .unwrap_or_else(|| {
            PathBuf::from(".deepseek")
                .join("state")
                .join("daily_budget.json")
        })
}
