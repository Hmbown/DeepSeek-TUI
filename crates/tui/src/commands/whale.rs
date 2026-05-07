use super::CommandResult;
use crate::tui::app::{App, AppAction};

pub fn whale(app: &mut App, arg: Option<&str>) -> CommandResult {
    let profile =
        crate::whale_pet::profile_for_seed(&crate::whale_pet::workspace_seed(&app.workspace));
    let Some(prompt) = arg.map(str::trim).filter(|value| !value.is_empty()) else {
        return CommandResult::message(crate::whale_pet::render_message(
            &profile,
            &crate::whale_pet::fake_inner_os("idle"),
            "在。今天这条鲸鱼已经孵好了，丢一句代码怨念过来。",
            "local",
        ));
    };

    if matches!(
        prompt.to_ascii_lowercase().as_str(),
        "hatch" | "stats" | "pet"
    ) {
        return CommandResult::message(crate::whale_pet::render_message(
            &profile,
            &crate::whale_pet::fake_inner_os(prompt),
            "摸到了。属性不重 roll，但它现在明显更想吐槽。",
            "local",
        ));
    }

    CommandResult::action(AppAction::WhalePet {
        prompt: prompt.to_string(),
    })
}
