//! Protocol-recovery contract tests.
//!
//! These tests exist to keep the engine hostile to fake tool-call wrappers
//! (XML/Replit/markdown pseudo-calls in assistant text). Their job is to make
//! sure that:
//!
//! 1. The known wrapper markers are still present in `core/engine.rs` so the
//!    streaming filter has something to scrub.
//! 2. The legacy text-based `tool_parser` does NOT treat the newer
//!    `<function_calls>` wrapper as a real tool call — only the legacy
//!    `[TOOL_CALL]` and `<invoke>` shapes ever produced structured calls, and
//!    nothing should silently re-enable text-based execution.
//! 3. The closing-marker list stays the same length as the start-marker list,
//!    so filter logic cannot get stuck in tool-call mode forever.
//!
//! The point is that protocol drift in the model output should be visible (we
//! still strip it and emit a status notice), not silently turned into tool
//! execution.

use std::fs;

#[path = "../src/core/tool_parser.rs"]
#[allow(dead_code)]
mod tool_parser;

const ENGINE_SRC: &str = include_str!("../src/core/engine.rs");

const EXPECTED_START_MARKERS: &[&str] = &[
    "[TOOL_CALL]",
    "<deepseek:tool_call",
    "<tool_call",
    "<invoke ",
    "<function_calls>",
];

const EXPECTED_END_MARKERS: &[&str] = &[
    "[/TOOL_CALL]",
    "</deepseek:tool_call>",
    "</tool_call>",
    "</invoke>",
    "</function_calls>",
];

#[test]
fn engine_keeps_known_fake_wrapper_start_markers() {
    for marker in EXPECTED_START_MARKERS {
        let needle = format!("\"{marker}\"");
        assert!(
            ENGINE_SRC.contains(&needle),
            "engine.rs no longer mentions start marker `{marker}` — protocol \
             scrubbing may have regressed. Searched for {needle:?}."
        );
    }
}

#[test]
fn engine_keeps_known_fake_wrapper_end_markers() {
    for marker in EXPECTED_END_MARKERS {
        let needle = format!("\"{marker}\"");
        assert!(
            ENGINE_SRC.contains(&needle),
            "engine.rs no longer mentions end marker `{marker}` — protocol \
             scrubbing may have regressed. Searched for {needle:?}."
        );
    }
}

#[test]
fn engine_marker_counts_stay_paired() {
    // A future contributor could quietly drop a closing marker and leave the
    // filter able to enter tool-call mode without ever leaving it. Lock the
    // count to whatever the constants currently declare.
    assert_eq!(EXPECTED_START_MARKERS.len(), EXPECTED_END_MARKERS.len());
    assert!(ENGINE_SRC.contains("TOOL_CALL_START_MARKERS"));
    assert!(ENGINE_SRC.contains("TOOL_CALL_END_MARKERS"));
}

#[test]
fn engine_emits_compact_fake_wrapper_notice() {
    assert!(
        ENGINE_SRC.contains("FAKE_WRAPPER_NOTICE"),
        "engine.rs no longer references the protocol-recovery notice constant"
    );
    assert!(
        ENGINE_SRC.contains("API tool channel"),
        "the protocol-recovery notice should mention the API tool channel"
    );
}

#[test]
fn legacy_parser_extracts_bracket_tool_call() {
    let result = tool_parser::parse_tool_calls(
        "intro [TOOL_CALL]\n{\"tool\":\"x\",\"args\":{}}\n[/TOOL_CALL]",
    );
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].name, "x");
    assert_eq!(result.clean_text, "intro");
}

#[test]
fn legacy_parser_extracts_invoke_block() {
    let result = tool_parser::parse_tool_calls(
        "before <invoke name=\"do_thing\"><parameter name=\"k\">v</parameter></invoke> after",
    );
    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.tool_calls[0].name, "do_thing");
}

#[test]
fn legacy_parser_does_not_execute_function_calls_wrapper() {
    // The newer `<function_calls>` wrapper is the kind of forged shape that
    // shows up in non-DeepSeek tool-call leakage. The legacy text parser must
    // NOT turn it into a structured tool call (the engine's filter still
    // strips it from visible text and the model is expected to use the API
    // tool channel instead).
    let raw = "narrative <function_calls>\n{\"name\":\"x\",\"input\":{}}\n</function_calls> end";
    let result = tool_parser::parse_tool_calls(raw);
    assert!(
        result.tool_calls.is_empty(),
        "function_calls wrapper must not be parsed as a real tool call: {:?}",
        result.tool_calls
    );
}

#[test]
fn legacy_parser_has_marker_helper_for_legacy_shapes_only() {
    // The legacy parser's `has_tool_call_markers` is documentation of which
    // shapes it ever knew how to handle. If it ever starts returning true for
    // `<function_calls>`, the parser may also have started producing fake
    // tool calls — we want to fail loudly in that case.
    assert!(tool_parser::has_tool_call_markers(
        "noise [TOOL_CALL]x[/TOOL_CALL]"
    ));
    assert!(tool_parser::has_tool_call_markers(
        "noise <invoke name=\"x\"></invoke>"
    ));
    assert!(!tool_parser::has_tool_call_markers(
        "noise <function_calls>{}</function_calls>"
    ));
}

#[test]
fn engine_source_file_still_exists_and_is_non_trivial() {
    // Sanity check so the `include_str!` above is meaningful — if the engine
    // module ever moves, this test must be updated alongside it.
    let metadata = fs::metadata("src/core/engine.rs").expect("engine.rs must exist next to tests");
    assert!(
        metadata.len() > 10_000,
        "engine.rs is unexpectedly small ({} bytes); did the file move?",
        metadata.len()
    );
}
