use super::command::{parse_command, Command, CommandAction, CommandKind, ParseMode};

#[test]
fn interactive_parser_preserves_quoted_task_names_for_every_task_name_command() {
    struct Case {
        input: &'static str,
        expected_name: &'static str,
    }

    let cases = [
        Case {
            input: "新 'alpha beta' 15",
            expected_name: "alpha beta",
        },
        Case {
            input: "遊 \"alpha beta\" 15",
            expected_name: "alpha beta",
        },
        Case {
            input: r"突 'alpha\beta' 15",
            expected_name: r"alpha\beta",
        },
        Case {
            input: "連 pre\" middle \"post 15 1 2",
            expected_name: "pre middle post",
        },
        Case {
            input: r#"繰 "alpha\"beta\\gamma" 15 月 09:00 10:00"#,
            expected_name: r#"alpha"beta\gamma"#,
        },
        Case {
            input: r"上 alpha\ beta 15",
            expected_name: "alpha beta",
        },
        Case {
            input: "下\u{3000}'alpha beta'\u{3000}\"gamma delta\"",
            expected_name: "alpha beta\0gamma delta",
        },
        Case {
            input: "割 15 pre' middle 'post",
            expected_name: "pre middle post",
        },
    ];

    for case in cases {
        let parsed = parse_command(case.input, ParseMode::Interactive).unwrap();
        let actual_names = task_names(&parsed);
        let expected_names = case.expected_name.split('\0').collect::<Vec<_>>();
        assert_eq!(actual_names, expected_names, "input: {:?}", case.input);
    }
}

#[test]
fn interactive_parser_preserves_an_empty_quoted_argument() {
    assert_eq!(
        parse_command("新 ''", ParseMode::Interactive).unwrap(),
        Command::Action(CommandAction::NewProject {
            kind: CommandKind::NewProject,
            canonical_name: "新",
            name: String::new(),
            estimated_minutes: None,
        })
    );
}

#[test]
fn interactive_parser_reports_typed_errors_for_incomplete_lexemes() {
    let cases = [
        ("新 'unfinished", "single quoteが閉じられていません"),
        ("新 \"unfinished", "double quoteが閉じられていません"),
        (r"新 unfinished\", "末尾のbackslashにescape対象がありません"),
    ];

    for (input, expected_reason) in cases {
        let error = parse_command(input, ParseMode::Interactive).unwrap_err();
        assert_eq!(error.command(), "入力", "input: {input:?}");
        assert_eq!(error.field(), "syntax", "input: {input:?}");
        assert_eq!(error.reason(), expected_reason, "input: {input:?}");
        assert_eq!(error.usage(), "<command> [arguments]", "input: {input:?}");
    }
}

#[test]
fn ignored_interactive_input_bypasses_lexer_errors() {
    for input in [
        "   \u{3000}",
        "# 'unfinished",
        " \u{3000}# \"unfinished",
        r"0\",
    ] {
        assert_eq!(
            parse_command(input, ParseMode::Interactive).unwrap(),
            Command::Noop,
            "input: {input:?}"
        );
    }
}

#[test]
fn noninteractive_string_parser_does_not_apply_interactive_lexer_rules() {
    assert_eq!(
        parse_command(r#"新 "alpha" 15"#, ParseMode::NonInteractive).unwrap(),
        Command::Action(CommandAction::NewProject {
            kind: CommandKind::NewProject,
            canonical_name: "新",
            name: "\"alpha\"".to_string(),
            estimated_minutes: Some(15),
        })
    );
    assert_eq!(
        parse_command(r"新 alpha\beta", ParseMode::NonInteractive).unwrap(),
        Command::Action(CommandAction::NewProject {
            kind: CommandKind::NewProject,
            canonical_name: "新",
            name: r"alpha\beta".to_string(),
            estimated_minutes: None,
        })
    );

    let error = parse_command("新 'alpha beta'", ParseMode::NonInteractive).unwrap_err();
    assert_eq!(error.command(), "新");
    assert_eq!(error.field(), "estimated_work_minutes");
    assert_eq!(error.reason(), "整数で指定してください");
    assert_eq!(error.usage(), "新 <name> [minutes]");
}

#[test]
fn lexer_keeps_existing_alias_and_arity_contracts() {
    assert_eq!(
        parse_command("new 'alpha beta'", ParseMode::Interactive)
            .unwrap()
            .kind(),
        CommandKind::NewProject
    );

    let error = parse_command("新 'alpha beta' 15 extra", ParseMode::Interactive).unwrap_err();
    assert_eq!(error.command(), "新");
    assert_eq!(error.field(), "arguments");
    assert_eq!(error.reason(), "引数の個数が正しくありません");
    assert_eq!(error.usage(), "新 <name> [minutes]");
}

fn task_names(command: &Command) -> Vec<&str> {
    match command {
        Command::Action(CommandAction::NewProject { name, .. })
        | Command::Action(CommandAction::Sequential { name, .. })
        | Command::Action(CommandAction::Repeat { name, .. })
        | Command::Action(CommandAction::TaskWithEstimate { name, .. })
        | Command::Action(CommandAction::Split { name, .. }) => vec![name],
        Command::Action(CommandAction::TaskNames { names }) => {
            names.iter().map(String::as_str).collect()
        }
        other => panic!("task-name command expected, got {other:?}"),
    }
}
