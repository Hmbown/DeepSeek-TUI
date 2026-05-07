#[path = "../src/budget.rs"]
#[allow(dead_code)]
mod budget;

use budget::DailyCostCounter;

#[test]
fn daily_counter_persists_and_resets_at_next_local_midnight() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("daily_budget.json");
    let day = chrono::NaiveDate::from_ymd_opt(2026, 5, 7).expect("date");
    let next = chrono::NaiveDate::from_ymd_opt(2026, 5, 8).expect("date");

    let mut counter = DailyCostCounter::load_at(path.clone(), day);
    counter.usd = 12.34;
    counter.cny = 87.65;
    counter.save().expect("save");

    let same_day = DailyCostCounter::load_at(path.clone(), day);
    assert_eq!(same_day.usd, 12.34);
    assert_eq!(same_day.cny, 87.65);

    let next_day = DailyCostCounter::load_at(path, next);
    assert_eq!(next_day.usd, 0.0);
    assert_eq!(next_day.cny, 0.0);
}
