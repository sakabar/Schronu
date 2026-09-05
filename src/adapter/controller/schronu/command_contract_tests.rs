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
    let error = parse_command("後 2 DAYS extra", ParseMode::NonInteractive).unwrap_err();
    assert_eq!(error.field(), "arguments");
    assert_eq!(error.reason(), "引数の個数が正しくありません");
    assert_eq!(error.usage(), "後 <量> [単位]");
}

#[test]
fn focusは全aliasとmodeで先頭argumentだけを受理する() {
    let task_id = Uuid::new_v4();

    for alias in ["見", "focus", "fc"] {
        for mode in [ParseMode::Interactive, ParseMode::NonInteractive] {
            let input = format!("{alias} {task_id} A _______");
            assert_eq!(
                parse_command(&input, mode).unwrap(),
                Command::Focus { task_id },
                "input: {input}, mode: {mode:?}"
            );

            let invalid_first = format!("{alias} invalid {task_id}");
            let error = parse_command(&invalid_first, mode).unwrap_err();
            assert_eq!(error.field(), "task_id", "input: {invalid_first}");
            assert_eq!(
                error.reason(),
                "UUIDで指定してください",
                "input: {invalid_first}"
            );
        }
    }
}

#[test]
fn all_commands_enforce_argument_bounds() {
    struct Case {
        command: &'static str,
        mode: ParseMode,
        valid_arguments: &'static [&'static str],
        minimum: usize,
        maximum: Option<usize>,
        usage: &'static str,
    }

    let cases = [
        Case {
            command: "樹",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "樹",
        },
        Case {
            command: "条",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "条",
        },
        Case {
            command: "根",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "根",
        },
        Case {
            command: "葉",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "葉",
        },
        Case {
            command: "今",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "今",
        },
        Case {
            command: "単",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "単",
        },
        Case {
            command: "暦",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "暦",
        },
        Case {
            command: "帯",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "帯",
        },
        Case {
            command: "開",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "開",
        },
        Case {
            command: "黒",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "黒",
        },
        Case {
            command: "外",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "外",
        },
        Case {
            command: "親",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "親",
        },
        Case {
            command: "子",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "子",
        },
        Case {
            command: "深",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "深",
        },
        Case {
            command: "待",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "待",
        },
        Case {
            command: "清",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "清",
        },
        Case {
            command: "平",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "平",
        },
        Case {
            command: "詰",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "詰",
        },
        Case {
            command: "高",
            mode: ParseMode::Interactive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "高",
        },
        Case {
            command: "検証",
            mode: ParseMode::NonInteractive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "検証",
        },
        Case {
            command: "tuck",
            mode: ParseMode::Interactive,
            valid_arguments: &[],
            minimum: 0,
            maximum: Some(0),
            usage: "tuck",
        },
        Case {
            command: "全",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["pattern"],
            minimum: 0,
            maximum: Some(1),
            usage: "全 [pattern]",
        },
        Case {
            command: "尾",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["pattern"],
            minimum: 0,
            maximum: Some(1),
            usage: "尾 [pattern]",
        },
        Case {
            command: "選",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["00000000-0000-0000-0000-000000000001"],
            minimum: 0,
            maximum: Some(1),
            usage: "選 [task_id]",
        },
        Case {
            command: "働",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["15"],
            minimum: 0,
            maximum: Some(1),
            usage: "働 [minutes]",
        },
        Case {
            command: "押",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["2"],
            minimum: 0,
            maximum: Some(1),
            usage: "押 [days]",
        },
        Case {
            command: "低",
            mode: ParseMode::Interactive,
            valid_arguments: &["2"],
            minimum: 0,
            maximum: Some(1),
            usage: "低 [days]",
        },
        Case {
            command: "逃",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["1", "秒"],
            minimum: 0,
            maximum: Some(2),
            usage: "逃 [量] [単位]",
        },
        Case {
            command: "終",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["今", "09:00"],
            minimum: 0,
            maximum: Some(2),
            usage: "終 [日付] [時刻]",
        },
        Case {
            command: "見",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["00000000-0000-0000-0000-000000000001"],
            minimum: 1,
            maximum: None,
            usage: "見 <task_id>",
        },
        Case {
            command: "〆",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["今"],
            minimum: 1,
            maximum: Some(1),
            usage: "〆 <日付または時刻>",
        },
        Case {
            command: "予",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["15"],
            minimum: 1,
            maximum: Some(1),
            usage: "予 <分>",
        },
        Case {
            command: "実",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["15"],
            minimum: 1,
            maximum: Some(1),
            usage: "実 <integer>",
        },
        Case {
            command: "重",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["1"],
            minimum: 1,
            maximum: Some(1),
            usage: "重 <integer>",
        },
        Case {
            command: "類",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["資"],
            minimum: 1,
            maximum: Some(1),
            usage: "類 <カテゴリ>",
        },
        Case {
            command: "新",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["task", "15"],
            minimum: 1,
            maximum: Some(2),
            usage: "新 <name> [minutes]",
        },
        Case {
            command: "遊",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["task", "15"],
            minimum: 1,
            maximum: Some(2),
            usage: "遊 <name> [minutes]",
        },
        Case {
            command: "突",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["task", "15"],
            minimum: 1,
            maximum: Some(2),
            usage: "突 <name> [minutes]",
        },
        Case {
            command: "約",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["今", "09:00"],
            minimum: 1,
            maximum: Some(2),
            usage: "約 <日付> [時刻]",
        },
        Case {
            command: "始",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["今", "09:00"],
            minimum: 1,
            maximum: Some(2),
            usage: "始 <日付> [時刻]",
        },
        Case {
            command: "上",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["task", "15"],
            minimum: 1,
            maximum: Some(2),
            usage: "上 <name> [minutes]",
        },
        Case {
            command: "揃",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["15", "全"],
            minimum: 1,
            maximum: Some(2),
            usage: "揃 <分> [全]",
        },
        Case {
            command: "後",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["1", "秒"],
            minimum: 1,
            maximum: Some(2),
            usage: "後 <量> [単位]",
        },
        Case {
            command: "空",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["今", "09:00"],
            minimum: 1,
            maximum: Some(2),
            usage: "空 <日付> [時刻]",
        },
        Case {
            command: "集",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["今", "09:00"],
            minimum: 1,
            maximum: Some(2),
            usage: "集 <日付> [時刻]",
        },
        Case {
            command: "割",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["15", "child"],
            minimum: 2,
            maximum: Some(2),
            usage: "割 <minutes> <name>",
        },
        Case {
            command: "連",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["task", "15", "1", "2", "suffix"],
            minimum: 4,
            maximum: Some(5),
            usage: "連 <name> <minutes> <begin> <end> [suffix]",
        },
        Case {
            command: "繰",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["task", "15", "月", "09:00", "10:00"],
            minimum: 5,
            maximum: Some(5),
            usage: "繰 <name> <minutes> <day> <start> <deadline>",
        },
        Case {
            command: "下",
            mode: ParseMode::NonInteractive,
            valid_arguments: &["task", "child", "leaf"],
            minimum: 1,
            maximum: None,
            usage: "下 <name>...",
        },
    ];

    for case in cases {
        let input_at_minimum = std::iter::once(case.command)
            .chain(case.valid_arguments[..case.minimum].iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            parse_command(&input_at_minimum, case.mode).is_ok(),
            "minimum boundary must parse: {input_at_minimum}"
        );

        if case.minimum > 0 {
            let input_below_minimum = std::iter::once(case.command)
                .chain(case.valid_arguments[..case.minimum - 1].iter().copied())
                .collect::<Vec<_>>()
                .join(" ");
            assert_arity_error(&input_below_minimum, case.mode, case.command, case.usage);
        }

        if let Some(maximum) = case.maximum {
            let input_at_maximum = std::iter::once(case.command)
                .chain(case.valid_arguments[..maximum].iter().copied())
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                parse_command(&input_at_maximum, case.mode).is_ok(),
                "maximum boundary must parse: {input_at_maximum}"
            );
            assert_arity_error(
                &format!("{input_at_maximum} extra"),
                case.mode,
                case.command,
                case.usage,
            );
        } else {
            let unbounded_input = std::iter::once(case.command)
                .chain(case.valid_arguments.iter().copied())
                .chain(["another"])
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                parse_command(&unbounded_input, case.mode).is_ok(),
                "unbounded arguments must parse: {unbounded_input}"
            );
        }
    }
}

fn assert_arity_error(input: &str, mode: ParseMode, command: &str, usage: &str) {
    let error = parse_command(input, mode).unwrap_err();
    assert_eq!(error.command(), command, "input: {input}");
    assert_eq!(error.field(), "arguments", "input: {input}");
    assert_eq!(
        error.reason(),
        "引数の個数が正しくありません",
        "input: {input}"
    );
    assert_eq!(error.usage(), usage, "input: {input}");
}

#[test]
fn pick_accepts_an_omitted_task_id() {
    for input in ["選", "pick"] {
        assert_eq!(
            parse_command(input, ParseMode::NonInteractive).unwrap(),
            Command::Action(CommandAction::Pick { task_id: None }),
            "input: {input}"
        );
    }

    let task_id = Uuid::new_v4();
    assert_eq!(
        parse_command(&format!("選 {task_id}"), ParseMode::NonInteractive).unwrap(),
        Command::Action(CommandAction::Pick {
            task_id: Some(task_id)
        })
    );

    let error = parse_command("選 invalid", ParseMode::NonInteractive).unwrap_err();
    assert_eq!(error.field(), "task_id");
    assert_eq!(error.reason(), "UUIDで指定してください");
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

    for input in ["tuck", "伏", "t"] {
        assert_eq!(
            parse_command(input, ParseMode::Interactive).unwrap(),
            Command::TuckAway
        );
        let error = parse_command(input, ParseMode::NonInteractive).unwrap_err();
        assert_eq!(error.command(), "tuck");
        assert_eq!(error.field(), "mode");
        assert_eq!(error.usage(), "tuck");
    }

    let shortcuts = [
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
            }
        );
    }

    assert_eq!(
        parse_command("後 1 秒", ParseMode::Interactive).unwrap(),
        Command::Defer {
            amount: 1,
            unit: "秒".to_string()
        }
    );
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
    assert_eq!(deadline_error.field(), "arguments");
    assert_eq!(deadline_error.reason(), "引数の個数が正しくありません");
    assert_eq!(deadline_error.usage(), "〆 <日付または時刻>");
    let category_error = parse_command("類", ParseMode::NonInteractive).unwrap_err();
    assert_eq!(category_error.field(), "arguments");
    assert_eq!(category_error.reason(), "引数の個数が正しくありません");
    assert_eq!(category_error.usage(), "類 <カテゴリ>");
}

#[test]
fn arity_and_typed_value_errors_share_the_canonical_command_usage() {
    for (arity_input, typed_input, command, usage) in [
        (
            "defer 1 DAY extra",
            "defer invalid DAY",
            "後",
            "後 <量> [単位]",
        ),
        (
            "new task 15 extra",
            "new task invalid",
            "新",
            "新 <name> [minutes]",
        ),
        ("actual 1 extra", "actual invalid", "実", "実 <integer>"),
    ] {
        let arity_error = parse_command(arity_input, ParseMode::NonInteractive).unwrap_err();
        let typed_error = parse_command(typed_input, ParseMode::NonInteractive).unwrap_err();

        assert_eq!(arity_error.command(), command, "input: {arity_input}");
        assert_eq!(typed_error.command(), command, "input: {typed_input}");
        assert_eq!(arity_error.usage(), usage, "input: {arity_input}");
        assert_eq!(typed_error.usage(), usage, "input: {typed_input}");
    }
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
fn extrude_distinguishes_an_omitted_argument_from_an_invalid_one() {
    assert_eq!(
        parse_command("押", ParseMode::NonInteractive).unwrap(),
        Command::Action(CommandAction::Extrude { step_days: None })
    );
    for (input, expected) in [("extrude 0", 0), ("extrude 15", 15), ("押 65535", 65535)] {
        assert_eq!(
            parse_command(input, ParseMode::NonInteractive).unwrap(),
            Command::Action(CommandAction::Extrude {
                step_days: Some(expected)
            }),
            "input: {input}"
        );
    }

    for input in ["extrude invalid", "押 65536"] {
        let error = parse_command(input, ParseMode::NonInteractive).unwrap_err();
        assert_eq!(error.command(), "押", "input: {input}");
        assert_eq!(error.field(), "step_days", "input: {input}");
        assert_eq!(
            error.reason(),
            "0以上65535以下の整数で指定してください",
            "input: {input}"
        );
        assert_eq!(error.usage(), "押 [days]", "input: {input}");
    }
}

#[test]
fn arrange_accepts_only_the_explicit_all_flags() {
    assert_eq!(
        parse_command("揃 15", ParseMode::NonInteractive).unwrap(),
        Command::Arrange {
            minutes: 15,
            includes_zero_estimate: false,
        }
    );
    for input in ["揃 15 全", "arrange 15 all"] {
        assert_eq!(
            parse_command(input, ParseMode::NonInteractive).unwrap(),
            Command::Arrange {
                minutes: 15,
                includes_zero_estimate: true,
            },
            "input: {input}"
        );
    }

    let error = parse_command("揃 15 invalid", ParseMode::NonInteractive).unwrap_err();
    assert_eq!(error.command(), "揃");
    assert_eq!(error.field(), "includes_zero_estimate");
    assert_eq!(error.reason(), "全またはallで指定してください");
    assert_eq!(error.usage(), "揃 <分> [全]");

    let error = parse_command("揃 invalid unknown", ParseMode::NonInteractive).unwrap_err();
    assert_eq!(error.command(), "揃");
    assert_eq!(error.field(), "estimated_work_minutes");
    assert_eq!(error.reason(), "整数で指定してください");
    assert_eq!(error.usage(), "揃 <分> [全]");
}

#[test]
fn runtime_routes_both_product_entry_paths_through_the_shared_parser() {
    let source = include_str!("runtime.rs");
    assert!(source.contains("parse_command(command, ParseMode::NonInteractive)"));
    assert!(source.contains("parse_command(command, ParseMode::Interactive)"));
}
