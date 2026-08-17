use super::command::{Command, CommandAction, CommandKind};
use super::handler::{
    handle, handle_task_tree_command, ExternalRequest, FocusRequest, TaskListOrder,
    TaskTreeCommandContext,
};
use super::renderer::{
    render_display_model, DisplayFragment, DisplayModel, DisplayRecorder, SchronuWriter,
};
use schronu::application::task_use_case::ApplicationError;
use std::io::Write;
use uuid::Uuid;

fn no_arguments(kind: CommandKind, canonical_name: &'static str) -> Command {
    Command::Action(CommandAction::NoArguments {
        kind,
        canonical_name,
    })
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
