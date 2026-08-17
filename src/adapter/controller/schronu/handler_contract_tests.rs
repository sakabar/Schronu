use super::command::{Command, CommandAction, CommandKind};
use super::handler::{handle, ExternalRequest, FocusRequest};
use super::renderer::{
    render_display_model, DisplayFragment, DisplayModel, DisplayRecorder, SchronuWriter,
};
use std::io::Write;

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
