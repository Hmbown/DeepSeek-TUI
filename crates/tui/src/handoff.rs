/// Context-pressure thresholds for handoff prompts. Reserved for the
/// planned handoff implementation (context-ratio tracking + system-prompt
/// injection). Not yet called by the engine turn loop.
#[allow(dead_code)]
pub const THRESHOLDS: [(f32, &str); 3] = [
    (0.9, "Context at 90%: stop and write handoff to .deepseek/handoff.md now"),
    (0.8, "Context at 80%: draft handoff to .deepseek/handoff.md"),
    (0.7, "Context at 70%: consider wrapping current sub-task"),
];

/// Return the highest-threshold message for the given context ratio, or
/// `None` when below the lowest threshold.
#[allow(dead_code)]
pub fn threshold_message(ratio: f32) -> Option<&'static str> {
    THRESHOLDS.iter().find(|(t, _)| ratio >= *t).map(|(_, m)| *m)
}
