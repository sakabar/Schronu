use super::command::{
    representative_valid_commands, Command, CommandAction, CommandKind, InteractiveShortcut,
};
use super::handler::{
    decide_finish_time_values, decide_time_values, handle, handle_breakdown_split_command,
    handle_command, handle_defer_command, handle_finish_placement_command, handle_project_command,
    handle_task_attribute_command, handle_task_tree_command, CommandContext, DeferCommandContext,
    DeferCommandError, ExternalRequest, FinishPlacementCommandContext, FocusChange, FocusSelection,
    FocusSessionEffect, HandlerError, NextUpResult, ProjectCommandContext,
    TaskAttributeCommandContext, TaskListOrder, TaskTreeCommandContext,
};
use super::renderer::{
    render_display_model, render_display_model_with_mode, AncestorTreeRow, DebugTreeRow,
    DisplayModel, LeafTreeRow, MessageLevel, RenderMode, SchronuWriter, TaskListDisplay,
    TaskListRow, TreeDisplay,
};
use chrono::{Local, NaiveDate, TimeZone};
use schronu::application::flatten_use_case::FlattenResult;
use schronu::application::pack_use_case::PackResult;
use schronu::application::task_use_case::ApplicationError;
use schronu::application::task_use_case::{BreakdownTaskInput, CompleteTaskInput, CreateTaskInput};
use schronu::entity::task::{TaskAttr, TaskHandle};
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

fn runtime_product_dispatch_source() -> &'static str {
    include_str!("runtime.rs")
        .split_once("fn execute_parsed(")
        .expect("runtime must retain the parsed command entrypoint")
        .1
        .split_once("fn apply_command_outcome(")
        .expect("parsed command path must remain bounded by outcome application")
        .0
}

fn handler_product_source() -> &'static str {
    include_str!("handler.rs")
        .split_once("#[cfg(test)]")
        .expect("handler product source must precede its test-only code")
        .0
}

fn finish_placement_handler_source() -> &'static str {
    handler_product_source()
        .split_once("pub(super) fn handle_finish_placement_command")
        .expect("handler must retain finish and placement dispatch")
        .1
        .split_once("pub(super) fn decide_finish_time_values(")
        .expect("finish and placement dispatch must remain bounded by finish time resolution")
        .0
}

fn runtime_interactive_dispatch_source() -> &'static str {
    include_str!("runtime.rs")
        .split_once("fn execute_interactive_command(")
        .expect("runtime must retain interactive dispatch")
        .1
        .split_once("struct InteractiveRepositoryState")
        .expect("interactive dispatch must remain bounded by its repository state")
        .0
}

fn assert_runtime_routes_to_handler(handler_call: &str, forbidden_fallback_tokens: &[&str]) {
    assert!(
        runtime_product_dispatch_source().contains("handle_command(parsed_command"),
        "product dispatch must route every typed command through handle_command"
    );
    let handler_name = handler_call
        .split_once('(')
        .map_or(handler_call, |(name, _)| name);
    assert!(
        handler_product_source().contains(handler_name),
        "the composite handler must retain the {handler_name} behavior route"
    );
    assert!(
        !include_str!("runtime.rs").contains("fn execute_with_config("),
        "legacy runtime dispatch must be removed"
    );
    for forbidden in forbidden_fallback_tokens {
        assert!(
            !runtime_product_dispatch_source().contains(forbidden),
            "runtime product dispatch must not match handler-owned command kind: {forbidden}"
        );
    }
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
fn 不正な時刻構文は正午へ補正せずnoneにする() {
    let now = Local.with_ymd_and_hms(2026, 8, 21, 12, 0, 0).unwrap();

    assert_eq!(decide_time_values(&["invalid".to_string()], &now), Ok(None));
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
    assert_eq!(open.focus_change, FocusChange::Keep);
    assert!(open.display.is_empty());

    let obsidian =
        handle(&no_arguments(CommandKind::Obsidian, "黒")).expect("obsidian is migrated");
    assert_eq!(obsidian.kind, CommandKind::Obsidian);
    assert_eq!(
        obsidian.external_request,
        Some(ExternalRequest::OpenObsidianRootSearch)
    );
    assert_eq!(obsidian.focus_change, FocusChange::Keep);
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
    assert_eq!(
        highest.focus_change,
        FocusChange::SelectionMode(FocusSelection::HighestPriority)
    );
    assert_eq!(
        highest.display,
        DisplayModel::Message {
            level: MessageLevel::Plain,
            text: "フォーカス選択モード: 高".to_string(),
        }
    );

    let lowest = handle(&Command::Action(CommandAction::FocusMode {
        kind: CommandKind::FocusLowest,
        canonical_name: "低",
        recent_days: Some(3),
    }))
    .expect("lowest focus mode is migrated");
    assert_eq!(
        lowest.focus_change,
        FocusChange::SelectionMode(FocusSelection::LowestPriority { recent_days: 3 })
    );
    assert_eq!(
        lowest.display,
        DisplayModel::Message {
            level: MessageLevel::Plain,
            text: "フォーカス選択モード: 低 3".to_string(),
        }
    );
}

#[test]
fn handler_owns_noop_but_leaves_unmigrated_commands_to_runtime() {
    let noop = handle(&Command::Noop).expect("noop has a structured outcome");
    assert_eq!(noop.kind, CommandKind::Noop);
    assert!(noop.display.is_empty());
    assert_eq!(noop.external_request, None);
    assert_eq!(noop.focus_change, FocusChange::Keep);

    let tuck_away = handle(&Command::TuckAway).expect("TuckAway has a structured outcome");
    assert_eq!(tuck_away.focus_session_effect, FocusSessionEffect::TuckAway);

    assert_eq!(handle(&Command::Estimate { minutes: 15 }), None);
}

#[test]
fn handler_has_no_runtime_or_external_io_dependency_and_no_command_reconstruction() {
    let open = handle(&no_arguments(CommandKind::Open, "ignored alias"))
        .expect("typed open command must produce an outcome");
    let focus = handle(&Command::Action(CommandAction::FocusMode {
        kind: CommandKind::FocusLowest,
        canonical_name: "ignored alias",
        recent_days: Some(7),
    }))
    .expect("typed focus command must produce an outcome");

    assert_eq!(
        open.external_request,
        Some(ExternalRequest::OpenFocusedLink)
    );
    assert_eq!(open.focus_change, FocusChange::Keep);
    assert_eq!(focus.external_request, None);
    assert_eq!(
        focus.focus_change,
        FocusChange::SelectionMode(FocusSelection::LowestPriority { recent_days: 7 })
    );

    let product_source = handler_product_source();
    for forbidden_dependency_or_reconstruction in [
        "super::runtime",
        "termion",
        "termion::",
        "std::env",
        "std::env::",
        "webbrowser",
        "webbrowser::",
        "TaskRepository",
        "run_repository_transaction",
        "legacy_tokens",
        "canonical_command",
    ] {
        assert!(
            !product_source.contains(forbidden_dependency_or_reconstruction),
            "handler product source must not depend on runtime, concrete repository I/O, or reconstruct parsed commands: {forbidden_dependency_or_reconstruction}"
        );
    }
}

struct TraceProjectContext {
    now: chrono::DateTime<Local>,
    focused_task_id: Option<Uuid>,
    focused_task: TaskHandle,
    created: Vec<CreateTaskInput>,
    breakdowns: Vec<BreakdownTaskInput>,
    created_attr_names: Vec<String>,
    estimates: Vec<(Uuid, i64)>,
    breakdown_error_after_successes: Option<usize>,
}

impl TraceProjectContext {
    fn new(now: chrono::DateTime<Local>) -> Self {
        Self {
            now,
            focused_task_id: Some(Uuid::from_u128(1)),
            focused_task: TaskHandle::with_identity("focused", Uuid::from_u128(1), now).unwrap(),
            created: Vec::new(),
            breakdowns: Vec::new(),
            created_attr_names: Vec::new(),
            estimates: Vec::new(),
            breakdown_error_after_successes: None,
        }
    }
}

impl ProjectCommandContext for TraceProjectContext {
    fn last_synced_time(&self) -> chrono::DateTime<Local> {
        self.now
    }

    fn focused_task(&mut self) -> Result<Option<TaskHandle>, ApplicationError> {
        Ok(Some(self.focused_task.clone()))
    }

    fn create_task(&mut self, input: CreateTaskInput) -> Result<Uuid, ApplicationError> {
        self.created.push(input);
        Ok(Uuid::from_u128(2))
    }

    fn breakdown_task(&mut self, input: BreakdownTaskInput) -> Result<Vec<Uuid>, ApplicationError> {
        if self.breakdown_error_after_successes == Some(self.breakdowns.len()) {
            return Err(ApplicationError::InvalidInput {
                field: "breakdown",
                reason: "injected partial failure",
            });
        }
        self.breakdowns.push(input);
        Ok(vec![Uuid::from_u128(3), Uuid::from_u128(4)])
    }

    fn create_task_attr(&mut self, name: &str) -> TaskAttr {
        self.created_attr_names.push(name.to_string());
        TaskAttr::with_identity(name, Uuid::from_u128(5), self.now)
    }

    fn set_estimate(&mut self, task_id: Uuid, minutes: i64) -> Result<(), ApplicationError> {
        self.estimates.push((task_id, minutes));
        Ok(())
    }

    fn focused_task_id(&self) -> Option<Uuid> {
        self.focused_task_id
    }

    fn set_focused_task_id(&mut self, task_id_opt: Option<Uuid>) {
        self.focused_task_id = task_id_opt;
    }
}

fn semantic_project_display(
    lines: &[String],
    error_opt: Option<&ApplicationError>,
) -> DisplayModel {
    let mut messages = lines
        .iter()
        .map(|line| DisplayModel::Message {
            level: MessageLevel::Plain,
            text: line.clone(),
        })
        .collect::<Vec<_>>();
    if let Some(error) = error_opt {
        messages.push(DisplayModel::Message {
            level: MessageLevel::Error,
            text: format!("操作エラー: {error}"),
        });
    }
    DisplayModel::Sequence(messages)
}

fn repeat_project_command() -> Command {
    Command::Action(CommandAction::Repeat {
        name: "typed routine".to_string(),
        estimated_minutes: 15,
        day: "月".to_string(),
        start_time: "09:00".to_string(),
        deadline_time: "10:00".to_string(),
    })
}

#[test]
fn project作成commandはhandlerがtyped_fieldを直接matchして所有する() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let mut context = TraceProjectContext::new(now);
    let command = Command::Action(CommandAction::NewProject {
        kind: CommandKind::NewProject,
        canonical_name: "ignored alias",
        name: "typed project".to_string(),
        estimated_minutes: Some(45),
    });

    let outcome = handle_project_command(&command, &mut context)
        .unwrap()
        .expect("project command must be owned by the handler");

    assert_eq!(outcome.kind, CommandKind::NewProject);
    assert_eq!(
        context.created,
        [CreateTaskInput {
            name: "typed project".to_string(),
            estimated_work_minutes: Some(45),
            pending_until: Some(Local.with_ymd_and_hms(2026, 8, 24, 6, 0, 0).unwrap()),
        }]
    );
    assert_eq!(context.focused_task_id, Some(Uuid::from_u128(2)));
    assert_runtime_routes_to_handler(
        "handle_project_command(parsed_command",
        &[
            "CommandKind::NewProject",
            "CommandKind::HobbyProject",
            "CommandKind::UnplannedProject",
            "CommandKind::Sequential",
            "CommandKind::Repeat",
            "CommandKind::Appointment",
            "CommandKind::Start",
        ],
    );

    let mut hobby_context = TraceProjectContext::new(now);
    handle_project_command(
        &Command::Action(CommandAction::NewProject {
            kind: CommandKind::HobbyProject,
            canonical_name: "ignored alias",
            name: "typed hobby".to_string(),
            estimated_minutes: Some(20),
        }),
        &mut hobby_context,
    )
    .unwrap()
    .expect("hobby project must be owned by the handler");
    assert_eq!(hobby_context.created[0].name, "typed hobby");
    assert_eq!(hobby_context.created[0].estimated_work_minutes, Some(20));
    assert_eq!(
        hobby_context.created[0].pending_until,
        Some(Local.with_ymd_and_hms(2026, 8, 24, 6, 0, 0).unwrap() + chrono::Duration::days(1399))
    );

    let mut unplanned_context = TraceProjectContext::new(now);
    handle_project_command(
        &Command::Action(CommandAction::NewProject {
            kind: CommandKind::UnplannedProject,
            canonical_name: "ignored alias",
            name: "typed unplanned".to_string(),
            estimated_minutes: Some(25),
        }),
        &mut unplanned_context,
    )
    .unwrap()
    .expect("unplanned project must be owned by the handler");
    assert_eq!(
        unplanned_context.created,
        [CreateTaskInput {
            name: "typed unplanned".to_string(),
            estimated_work_minutes: Some(25),
            pending_until: None,
        }]
    );

    let mut sequential_context = TraceProjectContext::new(now);
    let sequential = handle_project_command(
        &Command::Action(CommandAction::Sequential {
            name: "typed step".to_string(),
            estimated_minutes: 10,
            begin_index: 1,
            end_index: 2,
            suffix: Some("suffix".to_string()),
        }),
        &mut sequential_context,
    )
    .unwrap()
    .expect("sequential command must be owned by the handler");
    assert_eq!(sequential.kind, CommandKind::Sequential);
    assert_eq!(
        sequential_context.created_attr_names,
        ["typed step 2-suffix", "typed step 1-suffix"]
    );
    let step_2 = sequential_context
        .focused_task
        .get_children()
        .unwrap()
        .remove(0);
    let step_1 = step_2.get_children().unwrap().remove(0);
    assert_eq!(step_2.get_estimated_work_seconds().unwrap(), 10 * 60);
    assert_eq!(step_1.get_estimated_work_seconds().unwrap(), 10 * 60);
    assert!(sequential.display.is_empty());

    let mut repeat_context = TraceProjectContext::new(now);
    let repeat = handle_project_command(
        &Command::Action(CommandAction::Repeat {
            name: "typed routine".to_string(),
            estimated_minutes: 15,
            day: "月".to_string(),
            start_time: "09:00".to_string(),
            deadline_time: "10:00".to_string(),
        }),
        &mut repeat_context,
    )
    .unwrap()
    .expect("repeat command must be owned by the handler");
    assert_eq!(repeat_context.breakdowns.len(), 5);
    assert!(repeat_context
        .breakdowns
        .iter()
        .all(|input| { input.names == ["typed routine"] && input.pending_until.is_none() }));
    assert_eq!(repeat_context.estimates, vec![(Uuid::from_u128(1), 15); 5]);
    assert_eq!(
        repeat.display,
        semantic_project_display(
            &vec![format!("{} typed routine", Uuid::from_u128(3)); 5],
            None,
        )
    );

    let mut appointment_context = TraceProjectContext::new(now);
    appointment_context
        .focused_task
        .set_estimated_work_seconds(30 * 60)
        .unwrap();
    handle_project_command(
        &Command::Action(CommandAction::TimeExpression {
            kind: CommandKind::Appointment,
            canonical_name: "ignored alias",
            values: vec!["13:30".to_string()],
        }),
        &mut appointment_context,
    )
    .unwrap()
    .expect("appointment command must be owned by the handler");
    assert_eq!(
        appointment_context.focused_task.get_start_time().unwrap(),
        Local.with_ymd_and_hms(2026, 8, 23, 13, 30, 0).unwrap()
    );
    assert_eq!(
        appointment_context
            .focused_task
            .get_deadline_time_opt()
            .unwrap(),
        Some(Local.with_ymd_and_hms(2026, 8, 23, 14, 0, 0).unwrap())
    );

    let mut start_context = TraceProjectContext::new(now);
    handle_project_command(
        &Command::Action(CommandAction::TimeExpression {
            kind: CommandKind::Start,
            canonical_name: "ignored alias",
            values: vec!["14:30".to_string()],
        }),
        &mut start_context,
    )
    .unwrap()
    .expect("start command must be owned by the handler");
    assert_eq!(
        start_context.focused_task.get_start_time().unwrap(),
        Local.with_ymd_and_hms(2026, 8, 23, 14, 30, 0).unwrap()
    );
}

#[test]
fn project作成はlegacy_defaultでなくsemantic_emptyを返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let mut context = TraceProjectContext::new(now);
    let command = Command::Action(CommandAction::NewProject {
        kind: CommandKind::NewProject,
        canonical_name: "新",
        name: "semantic project".to_string(),
        estimated_minutes: Some(30),
    });

    let outcome = handle_project_command(&command, &mut context)
        .unwrap()
        .expect("project creation must be handled");

    assert_eq!(outcome.display, DisplayModel::Sequence(Vec::new()));
    assert_eq!(context.focused_task_id, Some(Uuid::from_u128(2)));
}

#[test]
fn breakdownとsplitはhandlerがplain_message_sequenceを構築する() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let mut breakdown_context = TraceProjectContext::new(now);
    let breakdown = Command::Action(CommandAction::TaskNames {
        names: vec!["first".to_string(), "second".to_string()],
    });

    let breakdown_outcome = handle_breakdown_split_command(&breakdown, &mut breakdown_context)
        .unwrap()
        .expect("breakdown must be handled");
    assert_eq!(
        breakdown_outcome.display,
        semantic_project_display(
            &[
                format!("{} first", Uuid::from_u128(3)),
                format!("{} second", Uuid::from_u128(4)),
            ],
            None,
        )
    );

    let mut split_context = TraceProjectContext::new(now);
    split_context
        .focused_task
        .set_estimated_work_seconds(30 * 60)
        .unwrap();
    let split = Command::Action(CommandAction::Split {
        minutes: 5,
        name: "typed split".to_string(),
    });

    let split_outcome = handle_breakdown_split_command(&split, &mut split_context)
        .unwrap()
        .expect("split must be handled");
    assert_eq!(
        split_outcome.display,
        semantic_project_display(&[format!("{} typed split", Uuid::from_u128(5))], None,)
    );
}

#[test]
fn breakdown即時errorはhandlerがsemantic_errorを返し状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let mut breakdown_context = TraceProjectContext::new(now);
    breakdown_context.breakdown_error_after_successes = Some(0);
    let breakdown_focus_before = breakdown_context.focused_task_id;
    let breakdown_estimate_before = breakdown_context
        .focused_task
        .get_estimated_work_seconds()
        .unwrap();
    let breakdown_children_before = breakdown_context.focused_task.get_children().unwrap().len();
    let breakdown = Command::Action(CommandAction::TaskNames {
        names: vec!["rejected child".to_string()],
    });

    let breakdown_outcome = handle_breakdown_split_command(&breakdown, &mut breakdown_context)
        .unwrap()
        .expect("reported breakdown error must remain a handled outcome");
    let breakdown_error = ApplicationError::InvalidInput {
        field: "breakdown",
        reason: "injected partial failure",
    };
    assert_eq!(
        breakdown_outcome.display,
        semantic_project_display(&[], Some(&breakdown_error))
    );
    assert_eq!(breakdown_context.focused_task_id, breakdown_focus_before);
    assert_eq!(
        breakdown_context
            .focused_task
            .get_estimated_work_seconds()
            .unwrap(),
        breakdown_estimate_before
    );
    assert_eq!(
        breakdown_context.focused_task.get_children().unwrap().len(),
        breakdown_children_before
    );
    assert!(breakdown_context.breakdowns.is_empty());
    assert!(breakdown_context.created_attr_names.is_empty());
}

#[test]
fn split検証errorはhandlerがsemantic_errorを返し状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let mut split_context = TraceProjectContext::new(now);
    split_context
        .focused_task
        .set_estimated_work_seconds(30 * 60)
        .unwrap();
    let split_focus_before = split_context.focused_task_id;
    let split_estimate_before = split_context
        .focused_task
        .get_estimated_work_seconds()
        .unwrap();
    let split_children_before = split_context.focused_task.get_children().unwrap().len();
    let split = Command::Action(CommandAction::Split {
        minutes: 5,
        name: "   ".to_string(),
    });

    let split_outcome = handle_breakdown_split_command(&split, &mut split_context)
        .unwrap()
        .expect("reported split validation error must remain a handled outcome");
    let split_error = ApplicationError::InvalidInput {
        field: "name",
        reason: "must not be blank",
    };
    assert_eq!(
        split_outcome.display,
        semantic_project_display(&[], Some(&split_error))
    );
    assert_eq!(split_context.focused_task_id, split_focus_before);
    assert_eq!(
        split_context
            .focused_task
            .get_estimated_work_seconds()
            .unwrap(),
        split_estimate_before
    );
    assert_eq!(
        split_context.focused_task.get_children().unwrap().len(),
        split_children_before
    );
    assert!(split_context.breakdowns.is_empty());
    assert!(split_context.created_attr_names.is_empty());
}

#[test]
fn repetition成功はhandlerがplain_messageを順序どおりcomposeする() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let repeat = repeat_project_command();
    let repeated_line = format!("{} typed routine", Uuid::from_u128(3));

    let mut success_context = TraceProjectContext::new(now);
    let success = handle_project_command(&repeat, &mut success_context)
        .unwrap()
        .expect("repetition must be handled");
    assert_eq!(
        success.display,
        semantic_project_display(&vec![repeated_line.clone(); 5], None)
    );
}

#[test]
fn repetition途中errorはhandlerが先行messageとerrorを順序どおりcomposeする() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let repeat = repeat_project_command();
    let repeated_line = format!("{} typed routine", Uuid::from_u128(3));
    let mut partial_error_context = TraceProjectContext::new(now);
    partial_error_context.breakdown_error_after_successes = Some(2);
    let partial_error = handle_project_command(&repeat, &mut partial_error_context)
        .unwrap()
        .expect("reported repetition error must remain a handled outcome");
    let expected_error = ApplicationError::InvalidInput {
        field: "breakdown",
        reason: "injected partial failure",
    };
    assert_eq!(
        partial_error.display,
        semantic_project_display(
            &[repeated_line.clone(), repeated_line],
            Some(&expected_error),
        )
    );
    assert_eq!(partial_error_context.breakdowns.len(), 2);
}

#[derive(Default)]
struct TraceTaskTreeContext {
    calls: Vec<String>,
}

impl TaskTreeCommandContext for TraceTaskTreeContext {
    fn show_tree(&mut self) -> Result<TreeDisplay, ApplicationError> {
        self.calls.push("tree".to_string());
        Ok(TreeDisplay::Debug {
            rows: vec![DebugTreeRow {
                debug: "tree".to_string(),
            }],
        })
    }

    fn show_ancestor(&mut self) -> Result<TreeDisplay, ApplicationError> {
        self.calls.push("ancestor".to_string());
        Ok(TreeDisplay::Ancestors {
            rows: vec![AncestorTreeRow {
                level: 0,
                task_id: Uuid::from_u128(1),
                first_available_date: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
                estimated_minutes: 15,
                name: "ancestor".to_string(),
            }],
        })
    }

    fn focus_root(&mut self) -> Result<(), ApplicationError> {
        self.calls.push("root".to_string());
        Ok(())
    }

    fn show_leaves(&mut self) -> Result<TreeDisplay, ApplicationError> {
        self.calls.push("leaves".to_string());
        Ok(TreeDisplay::Leaves {
            rows: vec![LeafTreeRow {
                remaining_count: 1,
                project_name: "project".to_string(),
                task_debug: "leaf".to_string(),
            }],
        })
    }

    fn show_task_list(
        &mut self,
        pattern: Option<&str>,
        order: TaskListOrder,
        resolve_pattern: bool,
    ) -> Result<DisplayModel, ApplicationError> {
        self.calls.push(format!(
            "list:{pattern:?}:{order:?}:resolve={resolve_pattern}"
        ));
        Ok(DisplayModel::TaskList(TaskListDisplay {
            rows: vec![TaskListRow::Message {
                text: "list".to_string(),
            }],
            category_work_seconds: vec![],
            category_denominator_seconds: 0,
        }))
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

    fn focus_children(&mut self) -> Result<Option<DisplayModel>, ApplicationError> {
        self.calls.push("children".to_string());
        Ok(Some(DisplayModel::Message {
            level: MessageLevel::Plain,
            text: "children".to_string(),
        }))
    }

    fn focus_deepest(&mut self) -> Result<Option<DisplayModel>, ApplicationError> {
        self.calls.push("deepest".to_string());
        Ok(Some(DisplayModel::Message {
            level: MessageLevel::Plain,
            text: "deepest".to_string(),
        }))
    }

    fn next_up(
        &mut self,
        name: &str,
        estimated_minutes: Option<i64>,
    ) -> Result<NextUpResult, ApplicationError> {
        self.calls
            .push(format!("next_up:{name}:{estimated_minutes:?}"));
        Ok(NextUpResult::NoDisplay)
    }
}

struct WriterFreeTaskTreeContext {
    base: TraceTaskTreeContext,
    focused_task_id: Option<Uuid>,
    children_display: bool,
    children_focus: Option<Uuid>,
    deepest_display: bool,
    deepest_focus: Option<Uuid>,
}

impl Default for WriterFreeTaskTreeContext {
    fn default() -> Self {
        Self {
            base: TraceTaskTreeContext::default(),
            focused_task_id: None,
            children_display: true,
            children_focus: Some(Uuid::from_u128(101)),
            deepest_display: true,
            deepest_focus: Some(Uuid::from_u128(202)),
        }
    }
}

impl TaskTreeCommandContext for WriterFreeTaskTreeContext {
    fn show_tree(&mut self) -> Result<TreeDisplay, ApplicationError> {
        self.base.show_tree()
    }

    fn show_ancestor(&mut self) -> Result<TreeDisplay, ApplicationError> {
        self.base.show_ancestor()
    }

    fn focus_root(&mut self) -> Result<(), ApplicationError> {
        self.base.focus_root()
    }

    fn show_leaves(&mut self) -> Result<TreeDisplay, ApplicationError> {
        self.base.show_leaves()
    }

    fn show_task_list(
        &mut self,
        pattern: Option<&str>,
        order: TaskListOrder,
        resolve_pattern: bool,
    ) -> Result<DisplayModel, ApplicationError> {
        self.base.show_task_list(pattern, order, resolve_pattern)
    }

    fn focus(&mut self, task_id: Uuid) {
        self.focused_task_id = Some(task_id);
    }

    fn pick(&mut self, task_id: Uuid) -> Result<(), ApplicationError> {
        self.focused_task_id = Some(task_id);
        Ok(())
    }

    fn focus_parent(&mut self) -> Result<(), ApplicationError> {
        Ok(())
    }

    fn focus_children(&mut self) -> Result<Option<DisplayModel>, ApplicationError> {
        self.focused_task_id = self.children_focus;
        Ok(self
            .children_display
            .then(|| ordered_task_tree_display("children")))
    }

    fn focus_deepest(&mut self) -> Result<Option<DisplayModel>, ApplicationError> {
        self.focused_task_id = self.deepest_focus;
        Ok(self
            .deepest_display
            .then(|| ordered_task_tree_display("deepest")))
    }

    fn next_up(
        &mut self,
        name: &str,
        estimated_minutes: Option<i64>,
    ) -> Result<NextUpResult, ApplicationError> {
        if name == "error" {
            return Ok(NextUpResult::ReportedError(
                ApplicationError::InvalidInput {
                    field: "name",
                    reason: "injected next-up failure",
                },
            ));
        }
        if name == "propagate-error" {
            return Err(ApplicationError::InvalidInput {
                field: "name",
                reason: "injected propagated failure",
            });
        }
        if name == "no-focus" {
            return Ok(NextUpResult::NoDisplay);
        }
        self.focused_task_id = Some(Uuid::from_u128(303));
        let _ = estimated_minutes;
        Ok(NextUpResult::NoDisplay)
    }
}

fn ordered_task_tree_display(label: &str) -> DisplayModel {
    DisplayModel::Sequence(vec![
        DisplayModel::Tree(TreeDisplay::Debug {
            rows: vec![DebugTreeRow {
                debug: format!("{label}:tree"),
            }],
        }),
        DisplayModel::Message {
            level: MessageLevel::Info,
            text: format!("{label}:message"),
        },
    ])
}

#[test]
fn task_tree_contextはtyped_displayとfocus変更をhandler_outcomeへ返す() {
    let cases = [
        (
            no_arguments(CommandKind::Children, "子"),
            Some(Uuid::from_u128(101)),
            vec![
                "raw:\n",
                "newline:children:tree",
                "raw:\n",
                "newline:[Info] children:message",
            ],
        ),
        (
            no_arguments(CommandKind::Deepest, "深"),
            Some(Uuid::from_u128(202)),
            vec![
                "raw:\n",
                "newline:deepest:tree",
                "raw:\n",
                "newline:[Info] deepest:message",
            ],
        ),
    ];

    for (command, expected_focus, expected_writes) in cases {
        let mut context = WriterFreeTaskTreeContext::default();
        let outcome = handle_task_tree_command(&command, &mut context)
            .unwrap()
            .expect("task-tree command must be handled");
        let mut writer = TraceWriter::default();
        render_display_model(&mut writer, &outcome.display).unwrap();

        assert_eq!(context.focused_task_id, expected_focus, "{command:?}");
        assert_eq!(writer.writes, expected_writes, "{command:?}");
    }
}

#[test]
fn task_tree_contextは表示なし分岐をempty_outcomeとして返す() {
    let cases = [
        (
            no_arguments(CommandKind::Children, "子"),
            WriterFreeTaskTreeContext {
                children_display: false,
                children_focus: None,
                ..WriterFreeTaskTreeContext::default()
            },
            None,
        ),
        (
            no_arguments(CommandKind::Children, "子"),
            WriterFreeTaskTreeContext {
                children_display: false,
                children_focus: Some(Uuid::from_u128(111)),
                ..WriterFreeTaskTreeContext::default()
            },
            Some(Uuid::from_u128(111)),
        ),
        (
            no_arguments(CommandKind::Deepest, "深"),
            WriterFreeTaskTreeContext {
                deepest_display: false,
                deepest_focus: Some(Uuid::from_u128(222)),
                ..WriterFreeTaskTreeContext::default()
            },
            Some(Uuid::from_u128(222)),
        ),
        (
            Command::Action(CommandAction::TaskWithEstimate {
                kind: CommandKind::NextUp,
                canonical_name: "上",
                name: "no-focus".to_string(),
                estimated_minutes: Some(15),
            }),
            WriterFreeTaskTreeContext::default(),
            None,
        ),
        (
            Command::Action(CommandAction::TaskWithEstimate {
                kind: CommandKind::NextUp,
                canonical_name: "上",
                name: "success".to_string(),
                estimated_minutes: Some(15),
            }),
            WriterFreeTaskTreeContext::default(),
            Some(Uuid::from_u128(303)),
        ),
    ];

    for (command, mut context, expected_focus) in cases {
        let outcome = handle_task_tree_command(&command, &mut context)
            .unwrap()
            .expect("task-tree command must be handled");

        assert!(
            outcome.display.is_empty(),
            "{command:?}: {:?}",
            outcome.display
        );
        assert_eq!(
            outcome.display,
            DisplayModel::Sequence(Vec::new()),
            "{command:?} must use the semantic empty model"
        );
        assert_eq!(context.focused_task_id, expected_focus, "{command:?}");
    }
}

#[test]
fn task_tree_contextはreported_errorを単独semantic_messageとして返す() {
    let command = Command::Action(CommandAction::TaskWithEstimate {
        kind: CommandKind::NextUp,
        canonical_name: "上",
        name: "error".to_string(),
        estimated_minutes: None,
    });
    let mut context = WriterFreeTaskTreeContext::default();

    let outcome = handle_task_tree_command(&command, &mut context)
        .unwrap()
        .expect("next-up error display remains a handled outcome");
    let mut writer = TraceWriter::default();
    render_display_model(&mut writer, &outcome.display).unwrap();

    assert_eq!(context.focused_task_id, None);
    assert_eq!(
        writer.writes,
        ["newline:[Error] 操作エラー: invalid input for name: injected next-up failure"]
    );
}

#[test]
fn task_tree_contextは伝播application_errorをerrorのまま返す() {
    let command = Command::Action(CommandAction::TaskWithEstimate {
        kind: CommandKind::NextUp,
        canonical_name: "上",
        name: "propagate-error".to_string(),
        estimated_minutes: None,
    });
    let mut context = WriterFreeTaskTreeContext::default();

    let actual = handle_task_tree_command(&command, &mut context);

    assert!(matches!(
        actual,
        Err(ApplicationError::InvalidInput {
            field: "name",
            reason: "injected propagated failure",
        })
    ));
    assert_eq!(context.focused_task_id, None);
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
        match command.kind() {
            CommandKind::Tree => assert_eq!(
                outcome.display,
                DisplayModel::Tree(TreeDisplay::Debug {
                    rows: vec![DebugTreeRow {
                        debug: "tree".to_string(),
                    }],
                })
            ),
            CommandKind::Ancestor => assert_eq!(
                outcome.display,
                DisplayModel::Tree(TreeDisplay::Ancestors {
                    rows: vec![AncestorTreeRow {
                        level: 0,
                        task_id: Uuid::from_u128(1),
                        first_available_date: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
                        estimated_minutes: 15,
                        name: "ancestor".to_string(),
                    }],
                })
            ),
            CommandKind::Leaves => assert_eq!(
                outcome.display,
                DisplayModel::Tree(TreeDisplay::Leaves {
                    rows: vec![LeafTreeRow {
                        remaining_count: 1,
                        project_name: "project".to_string(),
                        task_debug: "leaf".to_string(),
                    }],
                })
            ),
            CommandKind::ShowAll
            | CommandKind::Tail
            | CommandKind::Today
            | CommandKind::NonRepetitive => assert!(matches!(
                &outcome.display,
                DisplayModel::TaskList(TaskListDisplay { .. })
            )),
            _ => {}
        }
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
        );
        assert_eq!(!outcome.display.is_empty(), expects_display);
    }
}

#[test]
fn task_tree表示commandはruntime_fallbackに残さない() {
    let task_id = Uuid::from_u128(11);
    let commands = [
        Command::ShowAll {
            pattern: Some("typed pattern".to_string()),
        },
        Command::Focus { task_id },
        Command::Action(CommandAction::Pick { task_id }),
        Command::Action(CommandAction::TaskWithEstimate {
            kind: CommandKind::NextUp,
            canonical_name: "ignored alias",
            name: "typed task".to_string(),
            estimated_minutes: Some(20),
        }),
    ];
    let expected_calls = [
        "list:Some(\"typed pattern\"):ScheduledStartDesc:resolve=true".to_string(),
        format!("focus:{task_id}"),
        format!("pick:{task_id}"),
        "next_up:typed task:Some(20)".to_string(),
    ];

    for (command, expected_call) in commands.iter().zip(expected_calls) {
        let mut context = TraceTaskTreeContext::default();
        let outcome = handle_task_tree_command(command, &mut context)
            .unwrap()
            .expect("typed task tree command must be owned by the handler");
        assert_eq!(outcome.kind, command.kind());
        assert_eq!(context.calls, [expected_call]);
    }
    assert_runtime_routes_to_handler(
        "handle_task_tree_command(parsed_command",
        &[
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
        ],
    );
}

#[test]
fn breakdownとsplitはhandlerがtyped_fieldを直接matchして所有する() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let mut context = TraceProjectContext::new(now);
    let command = Command::Action(CommandAction::TaskNames {
        names: vec!["first".to_string(), "second".to_string()],
    });

    let outcome = handle_breakdown_split_command(&command, &mut context)
        .unwrap()
        .expect("typed breakdown command must be owned by the handler");

    assert_eq!(outcome.kind, CommandKind::Breakdown);
    assert_eq!(
        context.breakdowns,
        [BreakdownTaskInput {
            parent_id: Uuid::from_u128(1),
            names: vec!["first".to_string(), "second".to_string()],
            pending_until: None,
        }]
    );
    assert_eq!(context.focused_task_id, Some(Uuid::from_u128(3)));

    let split = Command::Action(CommandAction::Split {
        minutes: 5,
        name: "typed split".to_string(),
    });
    let mut split_context = TraceProjectContext::new(now);
    split_context
        .focused_task
        .set_estimated_work_seconds(30 * 60)
        .unwrap();
    let split_outcome = handle_breakdown_split_command(&split, &mut split_context)
        .unwrap()
        .expect("typed split command must be owned by the handler");
    assert_eq!(split_outcome.kind, CommandKind::Split);
    assert_eq!(split_context.created_attr_names, ["typed split"]);
    let split_child = split_context.focused_task.get_children().unwrap().remove(0);
    assert_eq!(split_child.get_estimated_work_seconds().unwrap(), 5 * 60);

    let wait = no_arguments(CommandKind::Wait, "ignored alias");
    let mut wait_context = TraceProjectContext::new(now);
    let wait_outcome = handle_breakdown_split_command(&wait, &mut wait_context)
        .unwrap()
        .expect("typed wait command must be owned by the handler");
    assert_eq!(wait_outcome.kind, CommandKind::Wait);

    assert_runtime_routes_to_handler(
        "handle_breakdown_split_command(parsed_command",
        &[
            "CommandKind::Breakdown",
            "CommandKind::Split",
            "CommandKind::Wait",
        ],
    );
}

#[derive(Default)]
struct TraceTaskAttributeContext {
    calls: Vec<String>,
}

impl TaskAttributeCommandContext for TraceTaskAttributeContext {
    fn set_deadline(&mut self, value: &str) -> Result<(), HandlerError> {
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

    fn set_category(&mut self, value: &str) -> Result<(), HandlerError> {
        self.calls.push(format!("category:{value}"));
        Ok(())
    }

    fn add_work(&mut self, minutes: Option<i64>) -> Result<(), ApplicationError> {
        self.calls.push(format!("work:{minutes:?}"));
        Ok(())
    }
}

#[test]
fn handler入口は直接構築された不正属性値をcontext実行前に拒否する() {
    let now = Local.with_ymd_and_hms(2026, 8, 27, 12, 0, 0).unwrap();
    let mut estimate_context = CompositeTraceContext::new(now);
    let estimate = handle_command(&Command::Estimate { minutes: -1 }, &mut estimate_context);
    assert!(matches!(
        estimate,
        Err(HandlerError::Application(ApplicationError::InvalidInput {
            field: "estimated_work_minutes",
            reason: "must not be negative",
        }))
    ));
    assert!(estimate_context.task_attribute.calls.is_empty());

    for (kind, canonical_name, field, reason, usage) in [
        (
            CommandKind::Category,
            "類",
            "category",
            "カテゴリが不正です",
            "類 <カテゴリ>",
        ),
        (
            CommandKind::Deadline,
            "〆",
            "deadline",
            "日時が不正です",
            "〆 <日付または時刻>",
        ),
    ] {
        let command = Command::Action(CommandAction::StringValue {
            kind,
            canonical_name,
            value: "invalid".to_string(),
        });
        let mut context = CompositeTraceContext::new(now);
        let Err(HandlerError::Parse(error)) = handle_command(&command, &mut context) else {
            panic!("{canonical_name} must preserve its parse-error classification");
        };
        assert_eq!(error.command(), canonical_name);
        assert_eq!(error.field(), field);
        assert_eq!(error.reason(), reason);
        assert_eq!(error.usage(), usage);
        assert!(context.task_attribute.calls.is_empty());
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
    let command = Command::Arrange {
        minutes: 37,
        includes_zero_estimate: true,
    };
    let mut context = TraceTaskAttributeContext::default();

    let outcome = handle_task_attribute_command(&command, &mut context)
        .unwrap()
        .expect("typed attribute command must be owned by the handler");

    assert_eq!(outcome.kind, CommandKind::Arrange);
    assert_eq!(context.calls, ["arrange:37:true"]);
    assert_runtime_routes_to_handler(
        "handle_task_attribute_command(parsed_command",
        &[
            "CommandKind::Deadline",
            "CommandKind::Estimate",
            "CommandKind::Arrange",
            "CommandKind::Actual",
            "CommandKind::Priority",
            "CommandKind::Category",
            "CommandKind::Work",
        ],
    );
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
    let commands = [
        Command::Defer {
            amount: 9,
            unit: "day".to_string(),
        },
        Command::InteractiveShortcut(InteractiveShortcut::NextMorning),
        Command::Action(CommandAction::ClearOrGather {
            kind: CommandKind::Gather,
            canonical_name: "ignored alias",
            values: vec!["10:30".to_string(), "8/24".to_string()],
        }),
    ];
    let expected_calls = [
        "defer:9:day",
        "next-morning",
        "clear-or-gather:Gather:10:30|8/24",
    ];

    for (command, expected_call) in commands.iter().zip(expected_calls) {
        let mut context = TraceDeferContext::default();
        let outcome = handle_defer_command(command, &mut context)
            .unwrap()
            .expect("typed defer command must be owned by the shared handler path");
        assert_eq!(outcome.kind, command.kind());
        assert_eq!(context.calls, [expected_call]);
    }
    assert_runtime_routes_to_handler(
        "handle_defer_command(parsed_command",
        &[
            "CommandKind::Defer =>",
            "CommandKind::DeferRoutines",
            "CommandKind::Escape",
            "CommandKind::Extrude",
            "CommandKind::Clear | CommandKind::Gather",
        ],
    );
    for forbidden_interactive_shortcut in [
        "Command::Defer { amount, unit }",
        "InteractiveShortcut::NextMorning",
        "InteractiveShortcut::NextWeek",
        "InteractiveShortcut::DeferRoutine",
        "InteractiveShortcut::FiveYears",
    ] {
        assert!(
            !runtime_interactive_dispatch_source().contains(forbidden_interactive_shortcut),
            "interactive defer shortcut must use the shared handler path: {forbidden_interactive_shortcut}"
        );
    }
}

struct TraceFinishPlacementContext {
    now: chrono::DateTime<Local>,
    focus_started_datetime: chrono::DateTime<Local>,
    focused_task: Option<TaskHandle>,
    tree_display: TreeDisplay,
    completion_response: FinishCompletionResponse,
    show_tree_error: bool,
    calls: Vec<String>,
    completion_inputs: Vec<CompleteTaskInput>,
    focus_updates: Vec<Option<Uuid>>,
}

impl TraceFinishPlacementContext {
    fn new(now: chrono::DateTime<Local>, focused_task: TaskHandle) -> Self {
        Self {
            now,
            focus_started_datetime: now,
            focused_task: Some(focused_task),
            tree_display: TreeDisplay::Debug {
                rows: vec![DebugTreeRow {
                    debug: "finish target".to_string(),
                }],
            },
            completion_response: FinishCompletionResponse::Success(Some(Uuid::from_u128(22))),
            show_tree_error: false,
            calls: Vec::new(),
            completion_inputs: Vec::new(),
            focus_updates: Vec::new(),
        }
    }

    fn semantic(now: chrono::DateTime<Local>, focused_task: TaskHandle) -> Self {
        Self {
            tree_display: finish_tree_display(),
            completion_response: FinishCompletionResponse::Success(Some(Uuid::from_u128(42))),
            ..Self::new(now, focused_task)
        }
    }

    fn record_focused_tree(&mut self) -> Result<TreeDisplay, ApplicationError> {
        self.calls.push("show-focused-tree".to_string());
        if self.show_tree_error {
            return Err(ApplicationError::InvalidInput {
                field: "tree",
                reason: "injected finish tree failure",
            });
        }
        Ok(self.tree_display.clone())
    }
}

impl FinishPlacementCommandContext for TraceFinishPlacementContext {
    fn last_synced_time(&self) -> chrono::DateTime<Local> {
        self.now
    }

    fn focus_started_datetime(&self) -> chrono::DateTime<Local> {
        self.focus_started_datetime
    }

    fn focused_task(&self) -> Result<Option<TaskHandle>, ApplicationError> {
        Ok(self.focused_task.clone())
    }

    fn show_focused_tree(&mut self) -> Result<TreeDisplay, ApplicationError> {
        self.record_focused_tree()
    }

    fn complete_focused_task(
        &mut self,
        input: CompleteTaskInput,
    ) -> Result<Option<Uuid>, ApplicationError> {
        self.calls.push("complete".to_string());
        self.completion_inputs.push(input);
        match self.completion_response {
            FinishCompletionResponse::Success(next_focus) => Ok(next_focus),
            FinishCompletionResponse::HasUndoneChildren(task_id) => {
                Err(ApplicationError::HasUndoneChildren(task_id))
            }
            FinishCompletionResponse::OtherError => Err(ApplicationError::InvalidInput {
                field: "finish",
                reason: "injected completion failure",
            }),
        }
    }

    fn set_focused_task_id(&mut self, task_id_opt: Option<Uuid>) {
        self.calls.push(format!("focus:{task_id_opt:?}"));
        self.focus_updates.push(task_id_opt);
    }

    fn pack(&mut self) -> Result<PackResult, ApplicationError> {
        self.calls.push("pack".to_string());
        Ok(PackResult::default())
    }

    fn flatten(&mut self) -> Result<FlattenResult, ApplicationError> {
        self.calls.push("flatten".to_string());
        Ok(FlattenResult::default())
    }
}

enum FinishCompletionResponse {
    Success(Option<Uuid>),
    HasUndoneChildren(Uuid),
    OtherError,
}

fn finish_tree_display() -> TreeDisplay {
    TreeDisplay::Debug {
        rows: vec![
            DebugTreeRow {
                debug: "[ ] finish parent".to_string(),
            },
            DebugTreeRow {
                debug: "    [ ] undone child".to_string(),
            },
        ],
    }
}

fn finish_command() -> Command {
    Command::Action(CommandAction::Finish {
        values: vec!["今".to_string()],
    })
}

#[test]
fn finish成功はsemantic_emptyとfocus変更を返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let task = TaskHandle::with_identity("finish target", Uuid::from_u128(21), now).unwrap();
    let mut context = TraceFinishPlacementContext::semantic(now, task);

    let outcome = handle_finish_placement_command(&finish_command(), &mut context)
        .unwrap()
        .expect("finish must be handled");

    assert_eq!(outcome.display, DisplayModel::Sequence(Vec::new()));
    assert_eq!(
        context.calls,
        [
            "complete",
            "focus:Some(00000000-0000-0000-0000-00000000002a)"
        ]
    );
    assert_eq!(context.focus_updates, [Some(Uuid::from_u128(42))]);
}

#[test]
fn finishのfocusなしと不正時刻とその他completion_errorは表示とfocusを変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let task = TaskHandle::with_identity("finish target", Uuid::from_u128(21), now).unwrap();

    let mut no_focus_context = TraceFinishPlacementContext::semantic(now, task.clone());
    no_focus_context.focused_task = None;
    let no_focus = handle_finish_placement_command(&finish_command(), &mut no_focus_context)
        .unwrap()
        .expect("finish without focus must be handled");
    assert_eq!(no_focus.display, DisplayModel::Sequence(Vec::new()));
    assert!(no_focus_context.calls.is_empty());
    assert!(no_focus_context.focus_updates.is_empty());

    let invalid_command = Command::Action(CommandAction::Finish {
        values: vec!["invalid".to_string()],
    });
    let mut invalid_context = TraceFinishPlacementContext::semantic(now, task.clone());
    let invalid = handle_finish_placement_command(&invalid_command, &mut invalid_context)
        .unwrap()
        .expect("invalid finish time must remain a handled no-op");
    assert_eq!(invalid.display, DisplayModel::Sequence(Vec::new()));
    assert!(invalid_context.calls.is_empty());
    assert!(invalid_context.completion_inputs.is_empty());
    assert!(invalid_context.focus_updates.is_empty());

    let mut completion_error_context = TraceFinishPlacementContext::semantic(now, task);
    completion_error_context.completion_response = FinishCompletionResponse::OtherError;
    let completion_error =
        handle_finish_placement_command(&finish_command(), &mut completion_error_context)
            .unwrap()
            .expect("reported completion error must remain handled");
    assert_eq!(completion_error.display, DisplayModel::Sequence(Vec::new()));
    assert_eq!(completion_error_context.calls, ["complete"]);
    assert_eq!(completion_error_context.completion_inputs.len(), 1);
    assert!(completion_error_context.focus_updates.is_empty());
}

#[test]
fn finishは未完了childrenと競合fallbackをtyped_treeとして順序どおり返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let parent = TaskHandle::with_identity("finish parent", Uuid::from_u128(21), now).unwrap();
    parent
        .create_child(TaskAttr::with_identity(
            "undone child",
            Uuid::from_u128(22),
            now,
        ))
        .unwrap();
    let expected_display = DisplayModel::Tree(finish_tree_display());
    let mut undone_context = TraceFinishPlacementContext::semantic(now, parent);

    let undone_outcome = handle_finish_placement_command(&finish_command(), &mut undone_context)
        .unwrap()
        .expect("finish with undone children must be handled");
    assert_eq!(undone_outcome.display, expected_display);
    assert_eq!(undone_context.calls, ["show-focused-tree"]);
    assert!(undone_context.completion_inputs.is_empty());
    assert!(undone_context.focus_updates.is_empty());

    let race_task = TaskHandle::with_identity("finish race", Uuid::from_u128(31), now).unwrap();
    let mut fallback_context = TraceFinishPlacementContext::semantic(now, race_task);
    fallback_context.completion_response =
        FinishCompletionResponse::HasUndoneChildren(Uuid::from_u128(31));

    let fallback_outcome =
        handle_finish_placement_command(&finish_command(), &mut fallback_context)
            .unwrap()
            .expect("finish fallback must be handled");
    assert_eq!(
        fallback_outcome.display,
        DisplayModel::Tree(finish_tree_display())
    );
    assert_eq!(fallback_context.calls, ["complete", "show-focused-tree"]);
    assert!(fallback_context.focus_updates.is_empty());
}

#[test]
fn finishのundone_tree取得errorは情報を保って伝播する() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let parent = TaskHandle::with_identity("finish parent", Uuid::from_u128(21), now).unwrap();
    parent
        .create_child(TaskAttr::with_identity(
            "undone child",
            Uuid::from_u128(22),
            now,
        ))
        .unwrap();
    let mut context = TraceFinishPlacementContext::semantic(now, parent);
    context.show_tree_error = true;

    let result = handle_finish_placement_command(&finish_command(), &mut context);

    assert_eq!(
        result,
        Err(ApplicationError::InvalidInput {
            field: "tree",
            reason: "injected finish tree failure",
        })
    );
    assert_eq!(context.calls, ["show-focused-tree"]);
    assert!(context.completion_inputs.is_empty());
    assert!(context.focus_updates.is_empty());
}

#[test]
fn finish_typed_treeは既存改行順をrendererへ委譲する() {
    let mut writer = TraceWriter::default();
    render_display_model(&mut writer, &DisplayModel::Tree(finish_tree_display())).unwrap();

    assert_eq!(
        writer.writes,
        [
            "raw:\n",
            "newline:[ ] finish parent",
            "newline:    [ ] undone child",
            "raw:\n",
        ]
    );
    assert_eq!(writer.flush_count, 0);
}

struct FinishFailingWriter {
    writes: Vec<String>,
    successful_newlines: usize,
    flush_count: usize,
}

impl Write for FinishFailingWriter {
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

impl SchronuWriter for FinishFailingWriter {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        if self.successful_newlines == 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "injected finish output failure",
            ));
        }
        self.successful_newlines += 1;
        self.writes.push(format!("newline:{message}"));
        Ok(())
    }

    fn supports_ansi_color(&self) -> bool {
        false
    }
}

#[test]
fn finish_typed_treeは途中出力とio_error分類をrendererで保持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let parent = TaskHandle::with_identity("finish parent", Uuid::from_u128(21), now).unwrap();
    parent
        .create_child(TaskAttr::with_identity(
            "undone child",
            Uuid::from_u128(22),
            now,
        ))
        .unwrap();
    let mut context = TraceFinishPlacementContext::semantic(now, parent);
    let outcome = handle_finish_placement_command(&finish_command(), &mut context)
        .unwrap()
        .expect("finish with undone children must be handled");
    let mut writer = FinishFailingWriter {
        writes: Vec::new(),
        successful_newlines: 0,
        flush_count: 0,
    };

    let error = render_display_model(&mut writer, &outcome.display).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    assert_eq!(error.to_string(), "injected finish output failure");
    assert_eq!(writer.writes, ["raw:\n", "newline:[ ] finish parent"]);
    assert_eq!(writer.flush_count, 0);
}

#[test]
fn packとflattenは既存semantic_modelを維持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let task = TaskHandle::with_identity("placement target", Uuid::from_u128(21), now).unwrap();

    for (command, expected_call, expected_variant) in [
        (
            no_arguments(CommandKind::Pack, "pack"),
            "pack",
            CommandKind::Pack,
        ),
        (
            no_arguments(CommandKind::Flatten, "flatten"),
            "flatten",
            CommandKind::Flatten,
        ),
    ] {
        let mut context = TraceFinishPlacementContext::semantic(now, task.clone());
        let outcome = handle_finish_placement_command(&command, &mut context)
            .unwrap()
            .expect("placement command must be handled");
        assert_eq!(outcome.kind, expected_variant);
        match expected_variant {
            CommandKind::Pack => assert!(matches!(outcome.display, DisplayModel::Pack(_))),
            CommandKind::Flatten => assert!(matches!(outcome.display, DisplayModel::Flatten(_))),
            _ => unreachable!(),
        }
        assert_eq!(context.calls, [expected_call]);
        assert!(context.focus_updates.is_empty());
    }
}

#[test]
fn 完了と配置commandはtyped値のままhandlerが所有してruntime_fallbackに残さない() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let focused_task =
        TaskHandle::with_identity("finish target", Uuid::from_u128(21), now).unwrap();
    let finish = Command::Action(CommandAction::Finish {
        values: vec!["09:45".to_string()],
    });
    let mut finish_context = TraceFinishPlacementContext::new(now, focused_task.clone());
    let finish_outcome = handle_finish_placement_command(&finish, &mut finish_context)
        .unwrap()
        .expect("typed finish command must be owned by the handler");
    assert_eq!(finish_outcome.kind, CommandKind::Finish);
    assert_eq!(
        finish_outcome.focus_session_effect,
        FocusSessionEffect::RestoreSelectionIfFocusChanged
    );
    assert_eq!(
        finish_context.calls,
        [
            "complete".to_string(),
            format!("focus:{:?}", Some(Uuid::from_u128(22))),
        ]
    );
    assert_eq!(
        finish_context.completion_inputs,
        [CompleteTaskInput {
            task_id: Uuid::from_u128(21),
            finished_at: Local.with_ymd_and_hms(2026, 8, 23, 9, 45, 0).unwrap(),
            additional_actual_work_seconds: 0,
        }]
    );

    for (command, expected_call) in [
        (no_arguments(CommandKind::Pack, "ignored alias"), "pack"),
        (
            no_arguments(CommandKind::Flatten, "ignored alias"),
            "flatten",
        ),
    ] {
        let mut context = TraceFinishPlacementContext::new(now, focused_task.clone());
        let outcome = handle_finish_placement_command(&command, &mut context)
            .unwrap()
            .expect("typed finish or placement command must be owned by the handler");
        assert_eq!(outcome.kind, command.kind());
        assert_eq!(context.calls, [expected_call]);
        assert!(context.completion_inputs.is_empty());
    }

    let unfocus = handle(&no_arguments(CommandKind::Unfocus, "ignored alias"))
        .expect("typed unfocus command must be owned by the handler");
    assert_eq!(unfocus.focus_change, FocusChange::Clear);
    assert_runtime_routes_to_handler(
        "handle_finish_placement_command(parsed_command",
        &[
            "CommandKind::Finish",
            "CommandKind::Pack",
            "CommandKind::Flatten",
            "CommandKind::Unfocus",
            "complete_task(",
            "pack_tasks_",
            "flatten_tasks_",
        ],
    );
    for forbidden_reconstruction in [
        "split_whitespace",
        "values[",
        "values.get(",
        "canonical_name",
    ] {
        assert!(
            !finish_placement_handler_source().contains(forbidden_reconstruction),
            "finish and placement handler must consume typed fields without reconstruction: {forbidden_reconstruction}"
        );
    }
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

struct CompositeTraceContext {
    project: TraceProjectContext,
    task_tree: TraceTaskTreeContext,
    task_attribute: TraceTaskAttributeContext,
    defer: TraceDeferContext,
    finish_placement: TraceFinishPlacementContext,
}

impl CompositeTraceContext {
    fn new(now: chrono::DateTime<Local>) -> Self {
        let focused_task =
            TaskHandle::with_identity("finish target", Uuid::from_u128(21), now).unwrap();
        Self {
            project: TraceProjectContext::new(now),
            task_tree: TraceTaskTreeContext::default(),
            task_attribute: TraceTaskAttributeContext::default(),
            defer: TraceDeferContext::default(),
            finish_placement: TraceFinishPlacementContext::new(now, focused_task),
        }
    }
}

impl ProjectCommandContext for CompositeTraceContext {
    fn last_synced_time(&self) -> chrono::DateTime<Local> {
        self.project.last_synced_time()
    }

    fn focused_task(&mut self) -> Result<Option<TaskHandle>, ApplicationError> {
        self.project.focused_task()
    }

    fn create_task(&mut self, input: CreateTaskInput) -> Result<Uuid, ApplicationError> {
        self.project.create_task(input)
    }

    fn breakdown_task(&mut self, input: BreakdownTaskInput) -> Result<Vec<Uuid>, ApplicationError> {
        self.project.breakdown_task(input)
    }

    fn create_task_attr(&mut self, name: &str) -> TaskAttr {
        self.project.create_task_attr(name)
    }

    fn set_estimate(&mut self, task_id: Uuid, minutes: i64) -> Result<(), ApplicationError> {
        self.project.set_estimate(task_id, minutes)
    }

    fn focused_task_id(&self) -> Option<Uuid> {
        self.project.focused_task_id()
    }

    fn set_focused_task_id(&mut self, task_id_opt: Option<Uuid>) {
        self.project.set_focused_task_id(task_id_opt);
    }
}

impl TaskTreeCommandContext for CompositeTraceContext {
    fn show_tree(&mut self) -> Result<TreeDisplay, ApplicationError> {
        self.task_tree.show_tree()
    }

    fn show_ancestor(&mut self) -> Result<TreeDisplay, ApplicationError> {
        self.task_tree.show_ancestor()
    }

    fn focus_root(&mut self) -> Result<(), ApplicationError> {
        self.task_tree.focus_root()
    }

    fn show_leaves(&mut self) -> Result<TreeDisplay, ApplicationError> {
        self.task_tree.show_leaves()
    }

    fn show_task_list(
        &mut self,
        pattern: Option<&str>,
        order: TaskListOrder,
        resolve_pattern: bool,
    ) -> Result<DisplayModel, ApplicationError> {
        self.task_tree
            .show_task_list(pattern, order, resolve_pattern)
    }

    fn focus(&mut self, task_id: Uuid) {
        self.task_tree.focus(task_id);
    }

    fn pick(&mut self, task_id: Uuid) -> Result<(), ApplicationError> {
        self.task_tree.pick(task_id)
    }

    fn focus_parent(&mut self) -> Result<(), ApplicationError> {
        self.task_tree.focus_parent()
    }

    fn focus_children(&mut self) -> Result<Option<DisplayModel>, ApplicationError> {
        self.task_tree.focus_children()
    }

    fn focus_deepest(&mut self) -> Result<Option<DisplayModel>, ApplicationError> {
        self.task_tree.focus_deepest()
    }

    fn next_up(
        &mut self,
        name: &str,
        estimated_minutes: Option<i64>,
    ) -> Result<NextUpResult, ApplicationError> {
        self.task_tree.next_up(name, estimated_minutes)
    }
}

impl TaskAttributeCommandContext for CompositeTraceContext {
    fn set_deadline(&mut self, value: &str) -> Result<(), HandlerError> {
        self.task_attribute.set_deadline(value)
    }

    fn set_estimate(&mut self, minutes: i64) -> Result<(), ApplicationError> {
        self.task_attribute.set_estimate(minutes)
    }

    fn arrange(
        &mut self,
        minutes: i64,
        includes_zero_estimate: bool,
    ) -> Result<(), ApplicationError> {
        self.task_attribute.arrange(minutes, includes_zero_estimate)
    }

    fn set_actual(&mut self, minutes: i64) -> Result<(), ApplicationError> {
        self.task_attribute.set_actual(minutes)
    }

    fn set_priority(&mut self, priority: i64) -> Result<(), ApplicationError> {
        self.task_attribute.set_priority(priority)
    }

    fn set_category(&mut self, value: &str) -> Result<(), HandlerError> {
        self.task_attribute.set_category(value)
    }

    fn add_work(&mut self, minutes: Option<i64>) -> Result<(), ApplicationError> {
        self.task_attribute.add_work(minutes)
    }
}

impl DeferCommandContext for CompositeTraceContext {
    fn defer(&mut self, amount: i64, unit: &str) -> Result<(), DeferCommandError> {
        self.defer.defer(amount, unit)
    }

    fn defer_expression(&mut self, values: &[String]) -> Result<(), DeferCommandError> {
        self.defer.defer_expression(values)
    }

    fn defer_next_morning(&mut self) -> Result<(), DeferCommandError> {
        self.defer.defer_next_morning()
    }

    fn defer_next_week(&mut self) -> Result<(), DeferCommandError> {
        self.defer.defer_next_week()
    }

    fn defer_routine(&mut self) -> Result<(), ApplicationError> {
        self.defer.defer_routine()
    }

    fn defer_five_years(&mut self) -> Result<(), DeferCommandError> {
        self.defer.defer_five_years()
    }

    fn defer_all_frequent_routines(&mut self) -> Result<(), ApplicationError> {
        self.defer.defer_all_frequent_routines()
    }

    fn prepare_escape(&mut self) -> Result<bool, ApplicationError> {
        self.defer.prepare_escape()
    }

    fn extrude(&mut self, step_days: Option<u16>) -> Result<(), ApplicationError> {
        self.defer.extrude(step_days)
    }

    fn clear_or_gather(
        &mut self,
        kind: CommandKind,
        values: &[String],
    ) -> Result<(), ApplicationError> {
        self.defer.clear_or_gather(kind, values)
    }
}

impl FinishPlacementCommandContext for CompositeTraceContext {
    fn last_synced_time(&self) -> chrono::DateTime<Local> {
        self.finish_placement.last_synced_time()
    }

    fn focus_started_datetime(&self) -> chrono::DateTime<Local> {
        self.finish_placement.focus_started_datetime()
    }

    fn focused_task(&self) -> Result<Option<TaskHandle>, ApplicationError> {
        self.finish_placement.focused_task()
    }

    fn show_focused_tree(&mut self) -> Result<TreeDisplay, ApplicationError> {
        FinishPlacementCommandContext::show_focused_tree(&mut self.finish_placement)
    }

    fn complete_focused_task(
        &mut self,
        input: CompleteTaskInput,
    ) -> Result<Option<Uuid>, ApplicationError> {
        self.finish_placement.complete_focused_task(input)
    }

    fn set_focused_task_id(&mut self, task_id_opt: Option<Uuid>) {
        self.finish_placement.set_focused_task_id(task_id_opt);
    }

    fn pack(&mut self) -> Result<PackResult, ApplicationError> {
        self.finish_placement.pack()
    }

    fn flatten(&mut self) -> Result<FlattenResult, ApplicationError> {
        self.finish_placement.flatten()
    }
}

fn assert_composite_command_context(_context: &mut dyn CommandContext) {}

#[test]
fn 全command_groupは単一handler入口からtyped_contextへdispatchされる() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let mut context = CompositeTraceContext::new(now);
    assert_composite_command_context(&mut context);

    let commands = [
        Command::Action(CommandAction::NewProject {
            kind: CommandKind::NewProject,
            canonical_name: "ignored alias",
            name: "typed project".to_string(),
            estimated_minutes: Some(45),
        }),
        Command::Action(CommandAction::TaskNames {
            names: vec!["typed first".to_string(), "typed second".to_string()],
        }),
        no_arguments(CommandKind::Tree, "ignored alias"),
        Command::Estimate { minutes: 30 },
        Command::Defer {
            amount: 2,
            unit: "日".to_string(),
        },
        no_arguments(CommandKind::Pack, "ignored alias"),
    ];

    let outcomes = commands
        .iter()
        .map(|command| {
            handle_command(command, &mut context)
                .unwrap()
                .expect("normal command must produce an outcome")
        })
        .collect::<Vec<_>>();

    assert_eq!(
        outcomes
            .iter()
            .map(|outcome| outcome.kind)
            .collect::<Vec<_>>(),
        [
            CommandKind::NewProject,
            CommandKind::Breakdown,
            CommandKind::Tree,
            CommandKind::Estimate,
            CommandKind::Defer,
            CommandKind::Pack,
        ]
    );
    assert_eq!(context.project.created.len(), 1);
    assert_eq!(
        context.project.breakdowns,
        [BreakdownTaskInput {
            parent_id: Uuid::from_u128(2),
            names: vec!["typed first".to_string(), "typed second".to_string()],
            pending_until: None,
        }]
    );
    assert_eq!(context.project.focused_task_id, Some(Uuid::from_u128(3)));
    assert_eq!(
        outcomes[1].display,
        semantic_project_display(
            &[
                format!("{} typed first", Uuid::from_u128(3)),
                format!("{} typed second", Uuid::from_u128(4)),
            ],
            None,
        )
    );
    assert_eq!(context.task_tree.calls, ["tree"]);
    assert_eq!(context.task_attribute.calls, ["estimate:30"]);
    assert_eq!(context.defer.calls, ["defer:2:日"]);
    assert_eq!(context.finish_placement.calls, ["pack"]);
}

#[test]
fn verify以外の全command_shapeは統一handler入口でoutcomeを返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();

    for command in representative_valid_commands() {
        let kind = command.kind();
        let mut context = CompositeTraceContext::new(now);
        let outcome = handle_command(&command, &mut context)
            .unwrap_or_else(|error| panic!("{kind:?} must be handled without error: {error}"));
        if kind == CommandKind::Verify {
            assert!(outcome.is_none(), "verify remains owned by runtime");
        } else {
            assert!(outcome.is_some(), "{kind:?} must produce an outcome");
        }
    }
}

#[test]
fn parserが生成しない未処理typed_commandはnoneを返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let mut context = CompositeTraceContext::new(now);
    let mismatched_command = no_arguments(CommandKind::NewProject, "invalid typed shape");

    assert_eq!(
        handle_command(&mismatched_command, &mut context).unwrap(),
        None
    );
}

#[test]
fn noopとopenとfocusも単一handler入口からstructured_outcomeを返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let mut context = CompositeTraceContext::new(now);

    let noop = handle_command(&Command::Noop, &mut context)
        .unwrap()
        .expect("noop must produce an outcome");
    assert_eq!(noop.kind, CommandKind::Noop);
    assert!(noop.display.is_empty());
    assert_eq!(noop.focus_change, FocusChange::Keep);

    let open = handle_command(
        &no_arguments(CommandKind::Open, "ignored alias"),
        &mut context,
    )
    .unwrap()
    .expect("open must produce an outcome");
    assert_eq!(
        open.external_request,
        Some(ExternalRequest::OpenFocusedLink)
    );
    assert_eq!(open.focus_change, FocusChange::Keep);

    let focus = handle_command(
        &Command::Action(CommandAction::FocusMode {
            kind: CommandKind::FocusLowest,
            canonical_name: "ignored alias",
            recent_days: Some(3),
        }),
        &mut context,
    )
    .unwrap()
    .expect("focus selection mode must produce an outcome");
    assert_eq!(
        focus.focus_change,
        FocusChange::SelectionMode(FocusSelection::LowestPriority { recent_days: 3 })
    );
    assert!(context.task_tree.calls.is_empty());
    assert_eq!(context.project.focused_task_id, Some(Uuid::from_u128(1)));

    let task_id = Uuid::from_u128(77);
    let focus_task = handle_command(&Command::Focus { task_id }, &mut context)
        .unwrap()
        .expect("explicit focus must produce an outcome");
    assert_eq!(focus_task.kind, CommandKind::Focus);
    assert_eq!(focus_task.focus_change, FocusChange::Set(task_id));
    assert!(context.task_tree.calls.is_empty());
    assert_eq!(context.project.focused_task_id, Some(Uuid::from_u128(1)));

    let unfocus = handle_command(
        &no_arguments(CommandKind::Unfocus, "ignored alias"),
        &mut context,
    )
    .unwrap()
    .expect("unfocus must produce an outcome");
    assert_eq!(unfocus.focus_change, FocusChange::Clear);
    assert!(context.task_tree.calls.is_empty());
    assert_eq!(context.project.focused_task_id, Some(Uuid::from_u128(1)));

    let highest = handle_command(
        &Command::Action(CommandAction::FocusMode {
            kind: CommandKind::FocusHighest,
            canonical_name: "ignored alias",
            recent_days: None,
        }),
        &mut context,
    )
    .unwrap()
    .expect("highest focus selection mode must produce an outcome");
    assert_eq!(
        highest.focus_change,
        FocusChange::SelectionMode(FocusSelection::HighestPriority)
    );
    assert!(context.task_tree.calls.is_empty());
    assert_eq!(context.project.focused_task_id, Some(Uuid::from_u128(1)));

    let verify = handle_command(
        &no_arguments(CommandKind::Verify, "ignored alias"),
        &mut context,
    )
    .unwrap();
    assert!(verify.is_none(), "verify remains owned by runtime");
}

#[test]
fn context_validation_errorは統一handler_errorとして呼び出し側へ返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let mut context = CompositeTraceContext::new(now);
    context.defer.escape_should_fail = true;
    let command = Command::Action(CommandAction::Escape {
        defer_expression: Some(vec!["2".to_string(), "日".to_string()]),
    });

    let result = handle_command(&command, &mut context);

    assert!(matches!(
        result,
        Err(HandlerError::Application(ApplicationError::InvalidInput {
            field: "estimated_work_seconds",
            reason: "injected escape failure",
        }))
    ));
}

#[test]
fn semantic_message_sequence_preserves_level_ansi_order_and_flush() {
    let display = DisplayModel::Sequence(vec![
        DisplayModel::Message {
            level: MessageLevel::Plain,
            text: "\x1b[1mraw".to_string(),
        },
        DisplayModel::Message {
            level: MessageLevel::Info,
            text: "line".to_string(),
        },
        DisplayModel::Message {
            level: MessageLevel::Error,
            text: "tail".to_string(),
        },
    ]);
    let mut writer = TraceWriter::default();
    render_display_model_with_mode(&mut writer, &display, RenderMode::Flushed).unwrap();
    assert_eq!(
        writer.writes,
        [
            "newline:\x1b[1mraw",
            "newline:[Info] line",
            "newline:[Error] tail",
        ]
    );
    assert_eq!(writer.flush_count, 1);
}
