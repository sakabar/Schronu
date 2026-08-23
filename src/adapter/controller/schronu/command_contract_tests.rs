use super::command::{
    command_with_minimum_valid_arguments, parse_command, Command, CommandAction, CommandKind,
    InteractiveShortcut, ParseMode,
};
use uuid::Uuid;

#[test]
fn all_aliases_parse_to_the_same_typed_command_kind() {
    let aliases = [
        (&["新", "new"][..], CommandKind::NewProject),
        (&["遊", "hobby"][..], CommandKind::HobbyProject),
        (&["突", "unplanned"][..], CommandKind::UnplannedProject),
        (&["連", "sequential", "seq"][..], CommandKind::Sequential),
        (&["繰", "repeat"][..], CommandKind::Repeat),
        (&["約", "appointment"][..], CommandKind::Appointment),
        (&["始", "start"][..], CommandKind::Start),
        (&["樹", "tree"][..], CommandKind::Tree),
        (&["条", "祖", "ancestor", "anc"][..], CommandKind::Ancestor),
        (&["根", "root"][..], CommandKind::Root),
        (&["葉", "leaves", "leaf", "lf"][..], CommandKind::Leaves),
        (&["全", "all"][..], CommandKind::ShowAll),
        (&["尾"][..], CommandKind::Tail),
        (&["今", "today"][..], CommandKind::Today),
        (&["単", "non_repetitive"][..], CommandKind::NonRepetitive),
        (&["暦", "cal"][..], CommandKind::Calendar),
        (&["帯", "band"][..], CommandKind::Band),
        (&["見", "focus", "fc"][..], CommandKind::Focus),
        (&["選", "pick"][..], CommandKind::Pick),
        (&["開", "open", "op"][..], CommandKind::Open),
        (&["黒", "obs"][..], CommandKind::Obsidian),
        (&["外", "unfocus", "ufc"][..], CommandKind::Unfocus),
        (&["親", "parent"][..], CommandKind::Parent),
        (&["子", "children", "ch"][..], CommandKind::Children),
        (&["深", "deep", "deepest"][..], CommandKind::Deepest),
        (&["上", "nextup", "nu"][..], CommandKind::NextUp),
        (&["下", "breakdown", "bd"][..], CommandKind::Breakdown),
        (&["割", "split", "sp"][..], CommandKind::Split),
        (&["待", "wait"][..], CommandKind::Wait),
        (&["〆", "締", "deadline"][..], CommandKind::Deadline),
        (&["予", "estimate", "es"][..], CommandKind::Estimate),
        (&["揃", "arrange", "arr"][..], CommandKind::Arrange),
        (&["実", "actual", "ac"][..], CommandKind::Actual),
        (&["重", "priority", "pr"][..], CommandKind::Priority),
        (&["類", "category", "cat"][..], CommandKind::Category),
        (&["働", "work", "wk"][..], CommandKind::Work),
        (&["後", "defer"][..], CommandKind::Defer),
        (
            &["清", "defer_all_frequent_routines"][..],
            CommandKind::DeferRoutines,
        ),
        (&["逃", "escape", "esc"][..], CommandKind::Escape),
        (&["平", "flatten", "flat"][..], CommandKind::Flatten),
        (&["詰", "pack"][..], CommandKind::Pack),
        (&["押", "extrude"][..], CommandKind::Extrude),
        (&["空", "clear"][..], CommandKind::Clear),
        (&["集", "gather"][..], CommandKind::Gather),
        (&["終", "finish", "fin"][..], CommandKind::Finish),
        (&["検証"][..], CommandKind::Verify),
    ];

    for (names, expected) in aliases {
        for name in names {
            let input = command_with_minimum_valid_arguments(name);
            let actual = parse_command(&input, ParseMode::NonInteractive).unwrap();
            assert_eq!(actual.kind(), expected, "alias: {name}");
        }
    }
}

#[test]
fn parser_converts_command_fields_to_typed_values() {
    assert_eq!(
        parse_command("予 45", ParseMode::NonInteractive).unwrap(),
        Command::Estimate { minutes: 45 }
    );
    let task_id = Uuid::new_v4();
    assert_eq!(
        parse_command(&format!("見 {task_id}"), ParseMode::NonInteractive).unwrap(),
        Command::Focus { task_id }
    );
    assert_eq!(
        parse_command("揃 15 全", ParseMode::NonInteractive).unwrap(),
        Command::Arrange {
            minutes: 15,
            includes_zero_estimate: true,
        }
    );
    assert_eq!(
        parse_command("実 -3", ParseMode::NonInteractive).unwrap(),
        Command::Action(CommandAction::IntegerValue {
            kind: CommandKind::Actual,
            canonical_name: "実",
            value: -3,
        })
    );
    assert_eq!(
        parse_command("defer 1 DAY", ParseMode::Interactive).unwrap(),
        Command::Defer {
            amount: 1,
            unit: "day".to_string(),
        }
    );
    assert_eq!(
        parse_command("後 2 DAYS extra", ParseMode::NonInteractive).unwrap(),
        Command::Action(CommandAction::TimeExpression {
            kind: CommandKind::Defer,
            canonical_name: "後",
            values: vec!["2".to_string(), "DAYS".to_string(), "extra".to_string()],
        })
    );
}

#[test]
fn parser_distinguishes_noop_search_fallback_and_interactive_shortcuts() {
    assert_eq!(
        parse_command("", ParseMode::NonInteractive).unwrap(),
        Command::Noop
    );
    assert_eq!(
        parse_command("   ", ParseMode::NonInteractive).unwrap(),
        Command::Noop
    );
    assert_eq!(
        parse_command("# memo", ParseMode::NonInteractive).unwrap(),
        Command::Noop
    );
    assert_eq!(
        parse_command("unknown words", ParseMode::NonInteractive).unwrap(),
        Command::ShowAll {
            pattern: Some("unknown".to_string()),
        }
    );
    assert_eq!(
        parse_command("0001 task", ParseMode::NonInteractive).unwrap(),
        Command::Noop
    );
    assert_eq!(
        parse_command(" 0001 task", ParseMode::NonInteractive).unwrap(),
        Command::ShowAll {
            pattern: Some("0001".to_string()),
        }
    );

    let shortcuts = [
        (
            "t",
            Command::Defer {
                amount: 1,
                unit: "秒".to_string(),
            },
        ),
        (
            "h",
            Command::Defer {
                amount: 1,
                unit: "時間".to_string(),
            },
        ),
        (
            "D",
            Command::Defer {
                amount: 86_400,
                unit: "秒".to_string(),
            },
        ),
        (
            "d",
            Command::InteractiveShortcut(InteractiveShortcut::NextMorning),
        ),
        (
            "w",
            Command::InteractiveShortcut(InteractiveShortcut::NextWeek),
        ),
        (
            "W",
            Command::InteractiveShortcut(InteractiveShortcut::DeferRoutine),
        ),
        (
            "y",
            Command::InteractiveShortcut(InteractiveShortcut::FiveYears),
        ),
    ];
    for (input, expected) in shortcuts {
        assert_eq!(
            parse_command(input, ParseMode::Interactive).unwrap(),
            expected
        );
        assert_eq!(
            parse_command(input, ParseMode::NonInteractive).unwrap(),
            Command::ShowAll {
                pattern: Some(input.to_string())
            },
        );
    }
}

#[test]
fn parse_errors_preserve_field_reason_usage_and_display_contract() {
    let error = parse_command("予 x", ParseMode::NonInteractive).unwrap_err();
    assert_eq!(error.command(), "予");
    assert_eq!(error.field(), "estimated_work_minutes");
    assert_eq!(error.reason(), "整数で指定してください");
    assert_eq!(error.usage(), "予 <分>");
    assert_eq!(
        error.to_string(),
        "入力エラー: estimated_work_minutes: 整数で指定してください (コマンド: 予, 使い方: 予 <分>)"
    );

    let deadline_error = parse_command("〆", ParseMode::NonInteractive).unwrap_err();
    assert_eq!(deadline_error.field(), "deadline");
    assert_eq!(deadline_error.usage(), "〆 <日付または時刻>");
    let category_error = parse_command("類", ParseMode::NonInteractive).unwrap_err();
    assert_eq!(category_error.field(), "category");
    assert_eq!(category_error.usage(), "類 <カテゴリ>");
}

#[test]
fn focus_selection_modes_are_interactive_only_and_validate_arguments() {
    for input in ["高", "high", "hi", "highest", "低", "low", "lo", "lowest"] {
        assert!(matches!(
            parse_command(input, ParseMode::Interactive).unwrap(),
            Command::Action(CommandAction::FocusMode { .. })
        ));
        assert_eq!(
            parse_command(input, ParseMode::NonInteractive).unwrap(),
            Command::ShowAll {
                pattern: Some(input.split_whitespace().next().unwrap().to_string())
            }
        );
    }

    for input in ["高 1", "低 -1", "低 1 2"] {
        assert!(
            parse_command(input, ParseMode::Interactive).is_err(),
            "{input}"
        );
    }
    assert!(parse_command("低", ParseMode::Interactive).is_ok());
    assert!(parse_command("低 0", ParseMode::Interactive).is_ok());
}

#[test]
fn extrude_without_an_argument_is_noop_and_invalid_days_fall_back_to_one() {
    assert_eq!(
        parse_command("押", ParseMode::NonInteractive).unwrap(),
        Command::Action(CommandAction::Extrude { step_days: None })
    );
    assert_eq!(
        parse_command("extrude invalid", ParseMode::NonInteractive).unwrap(),
        Command::Action(CommandAction::Extrude { step_days: Some(1) })
    );
}

#[test]
fn runtime_routes_both_product_entry_paths_through_the_shared_parser() {
    let source = include_str!("runtime.rs");
    assert!(source.contains("parse_command(command, ParseMode::NonInteractive)"));
    assert!(source.contains("parse_command(command, ParseMode::Interactive)"));
}
