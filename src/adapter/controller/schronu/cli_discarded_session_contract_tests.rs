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
fn tuck_aliases_share_the_interactive_parse_contract() {
    for alias in ["伏", "tuck", "t"] {
        let command = parse_command_tokens(&[alias.to_string()], ParseMode::Interactive).unwrap();
        assert_eq!(command.kind(), super::command::CommandKind::TuckAway);
    }
}

#[test]
fn cli_discard_implementation_is_kept_out_of_the_large_runtime_module() {
    let module = include_str!("cli_discarded_session.rs");
    assert!(module.contains("append_focus_transition"));
    assert!(module.contains("discarded_sessions_display"));
    assert!(module.contains("DiscardedSessionJournalTrait"));
}
