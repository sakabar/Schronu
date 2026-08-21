use super::command::{Command, CommandAction, CommandKind, InteractiveShortcut};
use super::handler::{
    decide_finish_time_values, decide_time_values, handle, handle_defer_command,
    handle_task_attribute_command, handle_task_tree_command, DeferCommandContext,
    DeferCommandError, ExternalRequest, FocusRequest, TaskAttributeCommandContext, TaskListOrder,
    TaskTreeCommandContext,
};
use super::renderer::{
    render_display_model, DisplayFragment, DisplayModel, DisplayRecorder, SchronuWriter,
};
use chrono::{Local, NaiveDate, TimeZone};
use schronu::application::task_use_case::ApplicationError;
use std::io::Write;
use uuid::Uuid;

fn no_arguments(kind: CommandKind, canonical_name: &'static str) -> Command {
    Command::Action(CommandAction::NoArguments {
        kind,
        canonical_name,
    })
}

fn maximum_local_business_day_start() -> chrono::DateTime<Local> {
    let local_datetime = NaiveDate::MAX
        .and_hms_opt(6, 0, 0)
        .expect("maximum date at 06:00 must be valid");
    Local
        .from_local_datetime(&local_datetime)
        .single()
        .expect("maximum local date at 06:00 must be unambiguous")
}

#[test]
fn 明日と曜日の日時指定は次の業務日境界errorを保持する() {
    let now = maximum_local_business_day_start();

    for date_expression in ["明", "月"] {
        assert_eq!(
            decide_time_values(&["09:30".to_string(), date_expression.to_string()], &now),
            Err(ApplicationError::SubjectiveDateOutOfRange {
                operation: "next_business_day_start",
                datetime: now,
            })
        );
    }
}

#[test]
fn 明示日付と月日はsingleのlocal日時だけを返し不正日付はnoneにする() {
    let now = Local.with_ymd_and_hms(2026, 8, 21, 12, 0, 0).unwrap();

    assert_eq!(
        decide_time_values(&["09:30".to_string(), "2026/8/22".to_string()], &now),
        Ok(Some(Local.with_ymd_and_hms(2026, 8, 22, 9, 30, 0).unwrap()))
    );
    assert_eq!(
        decide_time_values(&["09:30".to_string(), "8/23".to_string()], &now),
        Ok(Some(Local.with_ymd_and_hms(2026, 8, 23, 9, 30, 0).unwrap()))
    );
    assert_eq!(
        decide_time_values(&["09:30".to_string()], &now),
        Ok(Some(Local.with_ymd_and_hms(2026, 8, 21, 9, 30, 0).unwrap()))
    );
    assert_eq!(
        decide_time_values(&["09:30".to_string(), "2026/2/30".to_string()], &now),
        Ok(None)
    );
}

#[test]
fn 完了時刻指定は日時errorを保持し省略と今と不正構文を区別する() {
    let now = maximum_local_business_day_start();

    assert_eq!(decide_finish_time_values(&[], &now), Ok(Some(now)));
    assert_eq!(
        decide_finish_time_values(&["今".to_string()], &now),
        Ok(Some(now))
    );
    assert_eq!(
        decide_finish_time_values(&["invalid".to_string()], &now),
        Ok(None)
    );
    assert_eq!(
        decide_finish_time_values(&["09:30".to_string(), "明".to_string()], &now),
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "next_business_day_start",
            datetime: now,
        })
    );
}

#[test]
fn handler_returns_structured_external_requests_without_opening_them() {
    let open = handle(&no_arguments(CommandKind::Open, "開")).expect("open is migrated");
    assert_eq!(open.kind, CommandKind::Open);
    assert_eq!(
        open.external_request,
        Some(ExternalRequest::OpenFocusedLink)
    );
    assert_eq!(open.focus_request, None);
    assert!(open.display.is_empty());

    let obsidian =
        handle(&no_arguments(CommandKind::Obsidian, "黒")).expect("obsidian is migrated");
    assert_eq!(obsidian.kind, CommandKind::Obsidian);
    assert_eq!(
        obsidian.external_request,
        Some(ExternalRequest::OpenObsidianRootSearch)
    );
    assert_eq!(obsidian.focus_request, None);
    assert!(obsidian.display.is_empty());
}

#[test]
fn handler_returns_focus_requests_and_the_existing_confirmation_display() {
    let highest = handle(&Command::Action(CommandAction::FocusMode {
        kind: CommandKind::FocusHighest,
        canonical_name: "高",
        recent_days: None,
    }))
    .expect("highest focus mode is migrated");
    assert_eq!(highest.kind, CommandKind::FocusHighest);
    assert_eq!(highest.focus_request, Some(FocusRequest::HighestPriority));
    assert_eq!(
        highest.display,
        DisplayModel::newline("フォーカス選択モード: 高")
    );

    let lowest = handle(&Command::Action(CommandAction::FocusMode {
        kind: CommandKind::FocusLowest,
        canonical_name: "低",
        recent_days: Some(3),
    }))
    .expect("lowest focus mode is migrated");
    assert_eq!(
        lowest.focus_request,
        Some(FocusRequest::LowestPriority { recent_days: 3 })
    );
    assert_eq!(
        lowest.display,
        DisplayModel::newline("フォーカス選択モード: 低 3")
    );
}

#[test]
fn handler_owns_noop_but_leaves_unmigrated_commands_to_runtime() {
    let noop = handle(&Command::Noop).expect("noop has a structured outcome");
    assert_eq!(noop.kind, CommandKind::Noop);
    assert!(noop.display.is_empty());
    assert_eq!(noop.external_request, None);
    assert_eq!(noop.focus_request, None);

    assert_eq!(handle(&Command::Estimate { minutes: 15 }), None);
}

#[test]
fn handler_has_no_runtime_or_external_io_dependency_and_no_command_reconstruction() {
    let source = include_str!("handler.rs");
    for forbidden in [
        "super::runtime",
        "termion",
        "std::env",
        "webbrowser",
        "TaskRepository",
        "run_repository_transaction",
        "legacy_tokens",
        "canonical_command",
    ] {
        assert!(
            !source.contains(forbidden),
            "handler must not contain forbidden dependency or reconstruction: {forbidden}"
        );
    }
}

#[test]
fn project作成commandはhandlerがtyped_fieldを直接matchして所有する() {
    let runtime_source = include_str!("runtime.rs");
    let legacy_dispatch = runtime_source
        .split_once("fn execute_with_config(")
        .expect("runtime must retain the typed fallback entrypoint")
        .1
        .split_once("fn execute_non_interactive_command(")
        .expect("runtime fallback must remain bounded by the non-interactive entrypoint")
        .0;
    for migrated_kind in [
        "CommandKind::NewProject",
        "CommandKind::HobbyProject",
        "CommandKind::UnplannedProject",
        "CommandKind::Sequential",
        "CommandKind::Repeat",
        "CommandKind::Appointment",
        "CommandKind::Start",
    ] {
        assert!(
            !legacy_dispatch.contains(migrated_kind),
            "migrated command must not remain in runtime fallback: {migrated_kind}"
        );
    }

    let handler_source = include_str!("handler.rs");
    for action_pattern in [
        "CommandAction::NewProject {",
        "CommandAction::Sequential {",
        "CommandAction::Repeat {",
        "CommandAction::TimeExpression {",
    ] {
        assert!(
            handler_source.contains(action_pattern),
            "handler must directly match typed action fields: {action_pattern}"
        );
    }
}

#[derive(Default)]
struct TraceTaskTreeContext {
    calls: Vec<String>,
}

impl TaskTreeCommandContext for TraceTaskTreeContext {
    fn supports_ansi_color(&self) -> bool {
        true
    }

    fn show_tree(&mut self, display: &mut dyn SchronuWriter) -> Result<(), ApplicationError> {
        self.calls.push("tree".to_string());
        display.write_all(b"tree").unwrap();
        Ok(())
    }

    fn show_ancestor(&mut self, display: &mut dyn SchronuWriter) -> Result<(), ApplicationError> {
        self.calls.push("ancestor".to_string());
        display.write_all(b"ancestor").unwrap();
        Ok(())
    }

    fn focus_root(&mut self) -> Result<(), ApplicationError> {
        self.calls.push("root".to_string());
        Ok(())
    }

    fn show_leaves(&mut self, display: &mut dyn SchronuWriter) -> Result<(), ApplicationError> {
        self.calls.push("leaves".to_string());
        display.write_all(b"leaves").unwrap();
        Ok(())
    }

    fn show_task_list(
        &mut self,
        display: &mut dyn SchronuWriter,
        pattern: Option<&str>,
        order: TaskListOrder,
        resolve_pattern: bool,
    ) -> Result<(), ApplicationError> {
        self.calls.push(format!(
            "list:{pattern:?}:{order:?}:resolve={resolve_pattern}"
        ));
        display.write_all(b"list").unwrap();
        Ok(())
    }

    fn focus(&mut self, task_id: Uuid) {
        self.calls.push(format!("focus:{task_id}"));
    }

    fn pick(&mut self, task_id: Uuid) -> Result<(), ApplicationError> {
        self.calls.push(format!("pick:{task_id}"));
        Ok(())
    }

    fn focus_parent(&mut self) -> Result<(), ApplicationError> {
        self.calls.push("parent".to_string());
        Ok(())
    }

    fn focus_children(&mut self, display: &mut dyn SchronuWriter) -> Result<(), ApplicationError> {
        self.calls.push("children".to_string());
        display.write_all(b"children").unwrap();
        Ok(())
    }

    fn focus_deepest(&mut self, display: &mut dyn SchronuWriter) -> Result<(), ApplicationError> {
        self.calls.push("deepest".to_string());
        display.write_all(b"deepest").unwrap();
        Ok(())
    }

    fn next_up(
        &mut self,
        display: &mut dyn SchronuWriter,
        name: &str,
        estimated_minutes: Option<i64>,
    ) -> Result<(), ApplicationError> {
        self.calls
            .push(format!("next_up:{name}:{estimated_minutes:?}"));
        display.write_all(b"next_up").unwrap();
        Ok(())
    }
}

#[test]
fn task_tree表示commandはhandlerがtyped_fieldから表示modelと操作要求を作る() {
    let task_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let commands = [
        no_arguments(CommandKind::Tree, "系"),
        no_arguments(CommandKind::Ancestor, "条"),
        no_arguments(CommandKind::Root, "根"),
        no_arguments(CommandKind::Leaves, "葉"),
        Command::ShowAll {
            pattern: Some("月".to_string()),
        },
        Command::Action(CommandAction::OptionalPattern {
            kind: CommandKind::Tail,
            canonical_name: "尾",
            pattern: None,
        }),
        no_arguments(CommandKind::Today, "今"),
        no_arguments(CommandKind::NonRepetitive, "単"),
        no_arguments(CommandKind::Calendar, "暦"),
        no_arguments(CommandKind::Band, "帯"),
        Command::Focus { task_id },
        Command::Action(CommandAction::Pick { task_id }),
        no_arguments(CommandKind::Parent, "親"),
        no_arguments(CommandKind::Children, "子"),
        no_arguments(CommandKind::Deepest, "深"),
        Command::Action(CommandAction::TaskWithEstimate {
            kind: CommandKind::NextUp,
            canonical_name: "上",
            name: "next".to_string(),
            estimated_minutes: Some(15),
        }),
    ];
    let expected_calls = [
        "tree",
        "ancestor",
        "root",
        "leaves",
        "list:Some(\"月\"):ScheduledStartDesc:resolve=true",
        "list:Some(\"今\"):LowPriorityTail:resolve=false",
        "list:Some(\"今\"):ScheduledStartDesc:resolve=false",
        "list:Some(\"単\"):ScheduledStartDesc:resolve=false",
        "list:Some(\"暦\"):ScheduledStartDesc:resolve=false",
        "list:Some(\"帯\"):ScheduledStartDesc:resolve=false",
        "focus:11111111-1111-1111-1111-111111111111",
        "pick:11111111-1111-1111-1111-111111111111",
        "parent",
        "children",
        "deepest",
        "next_up:next:Some(15)",
    ];

    for (command, expected_call) in commands.iter().zip(expected_calls) {
        let mut context = TraceTaskTreeContext::default();
        let outcome = handle_task_tree_command(command, &mut context)
            .unwrap()
            .expect("task tree command is migrated");
        assert_eq!(outcome.kind, command.kind());
        assert_eq!(context.calls, [expected_call]);
        let expects_display = matches!(
            command.kind(),
            CommandKind::Tree
                | CommandKind::Ancestor
                | CommandKind::Leaves
                | CommandKind::ShowAll
                | CommandKind::Tail
                | CommandKind::Today
                | CommandKind::NonRepetitive
                | CommandKind::Calendar
                | CommandKind::Band
                | CommandKind::Children
                | CommandKind::Deepest
                | CommandKind::NextUp
        );
        assert_eq!(!outcome.display.is_empty(), expects_display);
    }
}

#[test]
fn task_tree表示commandはruntime_fallbackに残さない() {
    let runtime_source = include_str!("runtime.rs");
    let legacy_dispatch = runtime_source
        .split_once("fn execute_with_config(")
        .expect("runtime must retain the typed fallback entrypoint")
        .1
        .split_once("fn execute_non_interactive_command(")
        .expect("runtime fallback must remain bounded by the non-interactive entrypoint")
        .0;
    for migrated_kind in [
        "CommandKind::Tree",
        "CommandKind::Ancestor",
        "CommandKind::Root",
        "CommandKind::Leaves",
        "CommandKind::ShowAll",
        "CommandKind::Tail",
        "CommandKind::Today",
        "CommandKind::NonRepetitive",
        "CommandKind::Calendar",
        "CommandKind::Band",
        "CommandKind::Focus =>",
        "CommandKind::Pick",
        "CommandKind::Parent",
        "CommandKind::Children",
        "CommandKind::Deepest",
        "CommandKind::NextUp",
    ] {
        assert!(
            !legacy_dispatch.contains(migrated_kind),
            "migrated command must not remain in runtime fallback: {migrated_kind}"
        );
    }

    let product_dispatch = runtime_source
        .split_once("fn execute_parsed(")
        .expect("runtime must retain the parsed command entrypoint")
        .1
        .split_once("struct RuntimeProjectCommandContext")
        .expect("parsed command entrypoint must remain bounded by its context")
        .0;
    assert!(
        product_dispatch.contains("handle_task_tree_command(parsed_command"),
        "product dispatch must route parsed task tree commands through the handler"
    );

    let handler_source = include_str!("handler.rs");
    for action_pattern in [
        "Command::ShowAll { pattern }",
        "Command::Focus { task_id }",
        "CommandAction::OptionalPattern {",
        "CommandAction::Pick { task_id }",
        "CommandAction::TaskWithEstimate {",
    ] {
        assert!(
            handler_source.contains(action_pattern),
            "handler must directly match typed action fields: {action_pattern}"
        );
    }
}

#[test]
fn breakdownとsplitはhandlerがtyped_fieldを直接matchして所有する() {
    let runtime_source = include_str!("runtime.rs");
    let legacy_dispatch = runtime_source
        .split_once("fn execute_with_config(")
        .expect("runtime must retain the typed fallback entrypoint")
        .1
        .split_once("fn execute_non_interactive_command(")
        .expect("runtime fallback must remain bounded by the non-interactive entrypoint")
        .0;
    for migrated_kind in [
        "CommandKind::Breakdown",
        "CommandKind::Split",
        "CommandKind::Wait",
    ] {
        assert!(
            !legacy_dispatch.contains(migrated_kind),
            "migrated command must not remain in runtime fallback: {migrated_kind}"
        );
    }

    let product_dispatch = runtime_source
        .split_once("fn execute_parsed(")
        .expect("runtime must retain the parsed command entrypoint")
        .1
        .split_once("struct RuntimeProjectCommandContext")
        .expect("parsed command entrypoint must remain bounded by its context")
        .0;
    assert!(
        product_dispatch.contains("handle_breakdown_split_command(parsed_command"),
        "product dispatch must route breakdown, split, and wait through the handler"
    );

    let handler_source = include_str!("handler.rs");
    for action_pattern in [
        "CommandAction::TaskNames { names }",
        "CommandAction::Split { minutes, name }",
        "kind: CommandKind::Wait",
    ] {
        assert!(
            handler_source.contains(action_pattern),
            "handler must directly match typed action fields: {action_pattern}"
        );
    }
}

#[derive(Default)]
struct TraceTaskAttributeContext {
    calls: Vec<String>,
}

impl TaskAttributeCommandContext for TraceTaskAttributeContext {
    fn set_deadline(&mut self, value: &str) -> Result<(), ApplicationError> {
        self.calls.push(format!("deadline:{value}"));
        Ok(())
    }

    fn set_estimate(&mut self, minutes: i64) -> Result<(), ApplicationError> {
        self.calls.push(format!("estimate:{minutes}"));
        Ok(())
    }

    fn arrange(
        &mut self,
        minutes: i64,
        includes_zero_estimate: bool,
    ) -> Result<(), ApplicationError> {
        self.calls
            .push(format!("arrange:{minutes}:{includes_zero_estimate}"));
        Ok(())
    }

    fn set_actual(&mut self, minutes: i64) -> Result<(), ApplicationError> {
        self.calls.push(format!("actual:{minutes}"));
        Ok(())
    }

    fn set_priority(&mut self, priority: i64) -> Result<(), ApplicationError> {
        self.calls.push(format!("priority:{priority}"));
        Ok(())
    }

    fn set_category(&mut self, value: &str) -> Result<(), ApplicationError> {
        self.calls.push(format!("category:{value}"));
        Ok(())
    }

    fn add_work(&mut self, minutes: Option<i64>) -> Result<(), ApplicationError> {
        self.calls.push(format!("work:{minutes:?}"));
        Ok(())
    }
}

#[test]
fn task属性更新commandはhandlerがtyped_fieldを直接matchして所有する() {
    let commands = [
        Command::Action(CommandAction::StringValue {
            kind: CommandKind::Deadline,
            canonical_name: "〆",
            value: "明".to_string(),
        }),
        Command::Estimate { minutes: 25 },
        Command::Arrange {
            minutes: 30,
            includes_zero_estimate: true,
        },
        Command::Action(CommandAction::IntegerValue {
            kind: CommandKind::Actual,
            canonical_name: "実",
            value: 35,
        }),
        Command::Action(CommandAction::IntegerValue {
            kind: CommandKind::Priority,
            canonical_name: "重",
            value: 7,
        }),
        Command::Action(CommandAction::StringValue {
            kind: CommandKind::Category,
            canonical_name: "類",
            value: "investment".to_string(),
        }),
        Command::Action(CommandAction::OptionalInteger {
            kind: CommandKind::Work,
            canonical_name: "働",
            value: Some(40),
        }),
    ];
    let expected_calls = [
        "deadline:明",
        "estimate:25",
        "arrange:30:true",
        "actual:35",
        "priority:7",
        "category:investment",
        "work:Some(40)",
    ];

    for (command, expected_call) in commands.iter().zip(expected_calls) {
        let mut context = TraceTaskAttributeContext::default();
        let outcome = handle_task_attribute_command(command, &mut context)
            .unwrap()
            .expect("task attribute command is migrated");
        assert_eq!(outcome.kind, command.kind());
        assert!(outcome.display.is_empty());
        assert_eq!(context.calls, [expected_call]);
    }
}

#[test]
fn task属性更新commandはruntime_fallbackに残さない() {
    let runtime_source = include_str!("runtime.rs");
    let legacy_dispatch = runtime_source
        .split_once("fn execute_with_config(")
        .expect("runtime must retain the typed fallback entrypoint")
        .1
        .split_once("fn execute_non_interactive_command(")
        .expect("runtime fallback must remain bounded by the non-interactive entrypoint")
        .0;
    for migrated_kind in [
        "CommandKind::Deadline",
        "CommandKind::Estimate",
        "CommandKind::Arrange",
        "CommandKind::Actual",
        "CommandKind::Priority",
        "CommandKind::Category",
        "CommandKind::Work",
    ] {
        assert!(
            !legacy_dispatch.contains(migrated_kind),
            "migrated command must not remain in runtime fallback: {migrated_kind}"
        );
    }

    let product_dispatch = runtime_source
        .split_once("fn execute_parsed(")
        .expect("runtime must retain the parsed command entrypoint")
        .1
        .split_once("struct RuntimeProjectCommandContext")
        .expect("parsed command entrypoint must remain bounded by its context")
        .0;
    assert!(
        product_dispatch.contains("handle_task_attribute_command(parsed_command"),
        "product dispatch must route task attribute commands through the handler"
    );

    let handler_source = include_str!("handler.rs");
    for action_pattern in [
        "Command::Estimate { minutes }",
        "Command::Arrange {",
        "kind: CommandKind::Deadline",
        "kind: CommandKind::Actual",
        "kind: CommandKind::Priority",
        "kind: CommandKind::Category",
        "kind: CommandKind::Work",
    ] {
        assert!(
            handler_source.contains(action_pattern),
            "handler must directly match typed action fields: {action_pattern}"
        );
    }

    for forbidden in ["minutes.to_string()", "value.to_string()"] {
        assert!(
            !handler_source.contains(forbidden),
            "handler must not reconstruct typed values: {forbidden}"
        );
    }
}

#[derive(Default)]
struct TraceDeferContext {
    calls: Vec<String>,
    escape_should_fail: bool,
}

impl DeferCommandContext for TraceDeferContext {
    fn defer(&mut self, amount: i64, unit: &str) -> Result<(), DeferCommandError> {
        self.calls.push(format!("defer:{amount}:{unit}"));
        Ok(())
    }

    fn defer_expression(&mut self, values: &[String]) -> Result<(), DeferCommandError> {
        self.calls.push(format!("expression:{}", values.join("|")));
        Ok(())
    }

    fn defer_next_morning(&mut self) -> Result<(), DeferCommandError> {
        self.calls.push("next-morning".to_string());
        Ok(())
    }

    fn defer_next_week(&mut self) -> Result<(), DeferCommandError> {
        self.calls.push("next-week".to_string());
        Ok(())
    }

    fn defer_routine(&mut self) -> Result<(), ApplicationError> {
        self.calls.push("defer-routine".to_string());
        Ok(())
    }

    fn defer_five_years(&mut self) -> Result<(), DeferCommandError> {
        self.calls.push("five-years".to_string());
        Ok(())
    }

    fn defer_all_frequent_routines(&mut self) -> Result<(), ApplicationError> {
        self.calls.push("defer-all-routines".to_string());
        Ok(())
    }

    fn prepare_escape(&mut self) -> Result<bool, ApplicationError> {
        if self.escape_should_fail {
            return Err(ApplicationError::InvalidInput {
                field: "estimated_work_seconds",
                reason: "injected escape failure",
            });
        }
        Ok(true)
    }

    fn extrude(&mut self, step_days: Option<u16>) -> Result<(), ApplicationError> {
        self.calls.push(format!("extrude:{step_days:?}"));
        Ok(())
    }

    fn clear_or_gather(
        &mut self,
        kind: CommandKind,
        values: &[String],
    ) -> Result<(), ApplicationError> {
        self.calls
            .push(format!("clear-or-gather:{kind:?}:{}", values.join("|")));
        Ok(())
    }
}

#[test]
fn escapeの見積更新errorは表示用outcomeに変換せず呼び出し側へ返す() {
    let command = Command::Action(CommandAction::Escape {
        defer_expression: Some(vec!["2".to_string(), "日".to_string()]),
    });
    let mut context = TraceDeferContext {
        escape_should_fail: true,
        ..TraceDeferContext::default()
    };

    let result = handle_defer_command(&command, &mut context);

    assert!(matches!(
        result,
        Err(ApplicationError::InvalidInput {
            field: "estimated_work_seconds",
            reason: "injected escape failure",
        })
    ));
    assert!(context.calls.is_empty());
}

#[test]
fn defer系commandはhandlerがtyped_fieldを直接matchして所有する() {
    let commands = [
        Command::Defer {
            amount: 3,
            unit: "日".to_string(),
        },
        Command::Action(CommandAction::TimeExpression {
            kind: CommandKind::Defer,
            canonical_name: "後",
            values: vec!["09:30".to_string(), "8/20".to_string()],
        }),
        Command::InteractiveShortcut(InteractiveShortcut::NextMorning),
        Command::InteractiveShortcut(InteractiveShortcut::NextWeek),
        Command::InteractiveShortcut(InteractiveShortcut::DeferRoutine),
        Command::InteractiveShortcut(InteractiveShortcut::FiveYears),
        no_arguments(CommandKind::DeferRoutines, "清"),
        Command::Action(CommandAction::Escape {
            defer_expression: None,
        }),
        Command::Action(CommandAction::Escape {
            defer_expression: Some(vec!["2".to_string(), "日".to_string()]),
        }),
        Command::Action(CommandAction::Extrude { step_days: None }),
        Command::Action(CommandAction::Extrude { step_days: Some(1) }),
        Command::Action(CommandAction::Extrude { step_days: Some(4) }),
        Command::Action(CommandAction::ClearOrGather {
            kind: CommandKind::Clear,
            canonical_name: "空",
            values: vec!["10:30".to_string()],
        }),
        Command::Action(CommandAction::ClearOrGather {
            kind: CommandKind::Gather,
            canonical_name: "集",
            values: vec!["09:00".to_string(), "8/20".to_string()],
        }),
    ];
    let expected_calls = [
        vec!["defer:3:日"],
        vec!["expression:09:30|8/20"],
        vec!["next-morning"],
        vec!["next-week"],
        vec!["defer-routine"],
        vec!["five-years"],
        vec!["defer-all-routines"],
        vec![],
        vec!["expression:2|日"],
        vec!["extrude:None"],
        vec!["extrude:Some(1)"],
        vec!["extrude:Some(4)"],
        vec!["clear-or-gather:Clear:10:30"],
        vec!["clear-or-gather:Gather:09:00|8/20"],
    ];

    for (command, expected_call) in commands.iter().zip(expected_calls) {
        let mut context = TraceDeferContext::default();
        let outcome = handle_defer_command(command, &mut context)
            .unwrap()
            .expect("defer command is migrated");
        assert_eq!(outcome.kind, command.kind());
        assert!(outcome.display.is_empty());
        assert_eq!(context.calls, expected_call);
    }
}

#[test]
fn defer系commandはruntime_fallbackとinteractive特別経路に残さない() {
    let runtime_source = include_str!("runtime.rs");
    let legacy_dispatch = runtime_source
        .split_once("fn execute_with_config(")
        .expect("runtime must retain the typed fallback entrypoint")
        .1
        .split_once("fn execute_non_interactive_command(")
        .expect("runtime fallback must remain bounded by the non-interactive entrypoint")
        .0;
    for migrated_kind in [
        "CommandKind::Defer =>",
        "CommandKind::DeferRoutines",
        "CommandKind::Escape",
        "CommandKind::Extrude",
        "CommandKind::Clear | CommandKind::Gather",
    ] {
        assert!(
            !legacy_dispatch.contains(migrated_kind),
            "migrated command must not remain in runtime fallback: {migrated_kind}"
        );
    }

    let product_dispatch = runtime_source
        .split_once("fn execute_parsed(")
        .expect("runtime must retain the parsed command entrypoint")
        .1
        .split_once("struct RuntimeProjectCommandContext")
        .expect("parsed command entrypoint must remain bounded by its context")
        .0;
    assert!(
        product_dispatch.contains("handle_defer_command(parsed_command"),
        "product dispatch must route defer commands through the handler"
    );

    let interactive_dispatch = runtime_source
        .split_once("fn execute_interactive_command(")
        .expect("runtime must retain interactive dispatch")
        .1
        .split_once("struct InteractiveRepositoryState")
        .expect("interactive dispatch must remain bounded")
        .0;
    for forbidden in [
        "Command::Defer { amount, unit }",
        "InteractiveShortcut::NextMorning",
        "InteractiveShortcut::NextWeek",
        "InteractiveShortcut::DeferRoutine",
        "InteractiveShortcut::FiveYears",
    ] {
        assert!(
            !interactive_dispatch.contains(forbidden),
            "interactive defer shortcut must use the shared handler path: {forbidden}"
        );
    }

    let handler_source = include_str!("handler.rs");
    for action_pattern in [
        "Command::Defer { amount, unit }",
        "CommandAction::TimeExpression {",
        "CommandAction::Escape { defer_expression }",
        "CommandAction::Extrude { step_days }",
        "CommandAction::ClearOrGather { kind, values, .. }",
    ] {
        assert!(
            handler_source.contains(action_pattern),
            "handler must directly match typed action fields: {action_pattern}"
        );
    }
}

#[test]
fn 完了と配置commandはtyped値のままhandlerが所有してruntime_fallbackに残さない() {
    let handler_source = include_str!("handler.rs");
    let handler_dispatch = handler_source
        .split_once("pub(super) fn handle_finish_placement_command(")
        .expect("handler must own the finish and placement dispatch")
        .1
        .split_once("\nfn report_result")
        .expect("finish and placement dispatch must remain bounded")
        .0;

    for action_pattern in [
        "CommandAction::Finish { values }",
        "kind: CommandKind::Pack",
        "kind: CommandKind::Flatten",
    ] {
        assert!(
            handler_dispatch.contains(action_pattern),
            "handler must directly match typed action fields: {action_pattern}"
        );
    }
    for forbidden in [
        "canonical_name",
        "canonical_command",
        "legacy_tokens",
        "split_whitespace",
        "values[",
        "values.get(",
    ] {
        assert!(
            !handler_dispatch.contains(forbidden),
            "handler must not reconstruct or index legacy command tokens: {forbidden}"
        );
    }

    let runtime_source = include_str!("runtime.rs");
    let product_dispatch = runtime_source
        .split_once("fn execute_parsed(")
        .expect("runtime must retain the parsed command entrypoint")
        .1
        .split_once("struct RuntimeProjectCommandContext")
        .expect("parsed command entrypoint must remain bounded by its context")
        .0;
    assert!(
        product_dispatch.contains("handle_finish_placement_command(parsed_command"),
        "product dispatch must route finish and placement commands through the handler"
    );

    let legacy_dispatch = runtime_source
        .split_once("fn execute_with_config(")
        .expect("runtime must retain the typed fallback entrypoint")
        .1
        .split_once("\n#[cfg(test)]\nfn execute_show_all_command_for_test(")
        .expect("runtime fallback must remain bounded by the non-interactive entrypoint")
        .0;
    for migrated_kind in [
        "CommandKind::Finish",
        "CommandKind::Pack",
        "CommandKind::Flatten",
        "CommandKind::Unfocus",
    ] {
        assert!(
            !legacy_dispatch.contains(migrated_kind),
            "migrated command must not remain in runtime fallback: {migrated_kind}"
        );
    }
    for forbidden in ["complete_task(", "pack_tasks_", "flatten_tasks_"] {
        assert!(
            !legacy_dispatch.contains(forbidden),
            "runtime fallback must not retain migrated command implementation: {forbidden}"
        );
    }
    assert!(
        !legacy_dispatch.contains("execute_"),
        "runtime fallback must not retain normal command implementation calls"
    );
}

#[derive(Default)]
struct TraceWriter {
    writes: Vec<String>,
    flush_count: usize,
}

impl Write for TraceWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.writes
            .push(format!("raw:{}", String::from_utf8_lossy(buffer)));
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flush_count += 1;
        Ok(())
    }
}

impl SchronuWriter for TraceWriter {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        self.writes.push(format!("newline:{message}"));
        Ok(())
    }
}

#[test]
fn display_recorder_and_renderer_preserve_raw_newline_write_order_and_flush() {
    let mut recorder = DisplayRecorder::default();
    recorder.write_all(b"\x1b[1mraw").unwrap();
    recorder.writeln_newline("line").unwrap();
    recorder.write_all(b"tail").unwrap();
    recorder.flush().unwrap();

    assert_eq!(
        recorder.model().fragments(),
        &[
            DisplayFragment::Raw(b"\x1b[1mraw".to_vec()),
            DisplayFragment::Newline("line".to_string()),
            DisplayFragment::Raw(b"tail".to_vec()),
            DisplayFragment::Flush,
        ]
    );

    let mut writer = TraceWriter::default();
    render_display_model(&mut writer, recorder.model()).unwrap();
    assert_eq!(
        writer.writes,
        ["raw:\x1b[1mraw", "newline:line", "raw:tail"]
    );
    assert_eq!(writer.flush_count, 1);
}
