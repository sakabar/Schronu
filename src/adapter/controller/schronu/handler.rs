use super::command::{Command, CommandAction, CommandKind};
use super::renderer::{DisplayModel, DisplayRecorder, SchronuWriter};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExternalRequest {
    OpenFocusedLink,
    OpenObsidianRootSearch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FocusRequest {
    HighestPriority,
    LowestPriority { recent_days: i64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CommandOutcome {
    pub(super) kind: CommandKind,
    pub(super) display: DisplayModel,
    pub(super) external_request: Option<ExternalRequest>,
    pub(super) focus_request: Option<FocusRequest>,
}

impl CommandOutcome {
    fn empty(kind: CommandKind) -> Self {
        Self {
            kind,
            display: DisplayModel::default(),
            external_request: None,
            focus_request: None,
        }
    }
}

pub(super) fn handle(command: &Command) -> Option<CommandOutcome> {
    let kind = command.kind();
    let mut outcome = CommandOutcome::empty(kind);

    match command {
        Command::Noop => {}
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Open,
            ..
        }) => outcome.external_request = Some(ExternalRequest::OpenFocusedLink),
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Obsidian,
            ..
        }) => outcome.external_request = Some(ExternalRequest::OpenObsidianRootSearch),
        Command::Action(CommandAction::FocusMode {
            kind: CommandKind::FocusHighest,
            ..
        }) => {
            outcome.focus_request = Some(FocusRequest::HighestPriority);
            outcome.display = DisplayModel::newline("フォーカス選択モード: 高");
        }
        Command::Action(CommandAction::FocusMode {
            kind: CommandKind::FocusLowest,
            recent_days,
            ..
        }) => {
            let recent_days = recent_days.unwrap_or(0);
            outcome.focus_request = Some(FocusRequest::LowestPriority { recent_days });
            let label = if recent_days == 0 {
                "低".to_string()
            } else {
                format!("低 {recent_days}")
            };
            let mut display = DisplayRecorder::default();
            display
                .writeln_newline(&format!("フォーカス選択モード: {label}"))
                .expect("display recording is infallible");
            outcome.display = display.model().clone();
        }
        _ => return None,
    }

    Some(outcome)
}
