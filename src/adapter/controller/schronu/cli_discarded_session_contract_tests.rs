use super::command::{parse_command_tokens, ParseMode};

#[test]
fn discarded_command_is_available_in_both_cli_modes() {
    for mode in [ParseMode::Interactive, ParseMode::NonInteractive] {
        let japanese = parse_command_tokens(&["捨".to_string()], mode).unwrap();
        let english = parse_command_tokens(&["discarded".to_string()], mode).unwrap();

        assert_eq!(japanese.kind(), english.kind());
        assert_ne!(format!("{:?}", japanese.kind()), "ShowAll");
    }
}

#[test]
fn discarded_command_rejects_an_invalid_calendar_date_with_a_field_error() {
    let error = parse_command_tokens(
        &["捨".to_string(), "2026/2/30".to_string()],
        ParseMode::NonInteractive,
    )
    .unwrap_err();

    assert_eq!(error.field(), "logical_date");
    assert_eq!(error.usage(), "捨 [YYYY/M/D]");
}

#[test]
fn cli_runtime_connects_every_discard_reason_and_read_only_summary() {
    let source = format!(
        "{}{}",
        include_str!("runtime.rs"),
        include_str!("cli_discarded_session.rs")
    );
    for required in [
        "CliUnfocus",
        "CliTuckAway",
        "CliFocusSwitch",
        "CliAutoSwitch",
        "CliNormalExit",
        "discarded_sessions_on",
    ] {
        assert!(
            source.contains(required),
            "missing CLI contract: {required}"
        );
    }
}

#[test]
fn cli_discard_implementation_is_kept_out_of_the_large_runtime_module() {
    let module = include_str!("cli_discarded_session.rs");
    assert!(module.contains("append_focus_transition"));
    assert!(module.contains("discarded_sessions_display"));
    assert!(module.contains("DiscardedSessionJournalTrait"));
}
