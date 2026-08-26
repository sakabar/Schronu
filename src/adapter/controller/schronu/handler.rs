use super::command::{Command, CommandAction, CommandKind, CommandParseError, InteractiveShortcut};
use super::renderer::{
    DisplayModel, FlattenDisplay, FlattenReason, FlattenReasonSummary, FlattenRow,
    FlattenUnresolvedDay, MessageLevel, PackDisplay, PackRow, TreeDisplay,
};
use chrono::{DateTime, Datelike, Days, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use regex::Regex;
use schronu::application::daily_capacity::{
    try_local_date_and_time, try_next_business_day_start, try_subjective_date,
};
use schronu::application::flatten_use_case::{FlattenResult, UnresolvedReason};
use schronu::application::pack_use_case::PackResult;
use schronu::application::task_use_case::{
    estimated_work_seconds_from_minutes, validate_task_name, ApplicationError, BreakdownTaskInput,
    CompleteTaskInput, CreateTaskInput,
};
use schronu::entity::task::{TaskAttr, TaskHandle};
use std::cmp::min;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExternalRequest {
    OpenFocusedLink,
    OpenObsidianRootSearch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FocusSelection {
    HighestPriority,
    LowestPriority { recent_days: i64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FocusChange {
    Keep,
    Clear,
    Set(Uuid),
    SelectionMode(FocusSelection),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CommandOutcome {
    pub(super) kind: CommandKind,
    pub(super) display: DisplayModel,
    pub(super) external_request: Option<ExternalRequest>,
    pub(super) focus_change: FocusChange,
}

#[allow(dead_code)] // The legacy runtime dispatch coexists until its dedicated migration commit.
pub(super) trait CommandContext:
    ProjectCommandContext
    + TaskTreeCommandContext
    + TaskAttributeCommandContext
    + DeferCommandContext
    + FinishPlacementCommandContext
{
}

impl<T> CommandContext for T where
    T: ProjectCommandContext
        + TaskTreeCommandContext
        + TaskAttributeCommandContext
        + DeferCommandContext
        + FinishPlacementCommandContext
        + ?Sized
{
}

pub(super) trait ProjectCommandContext {
    fn last_synced_time(&self) -> DateTime<Local>;
    fn focused_task(&mut self) -> Result<Option<TaskHandle>, ApplicationError>;
    fn create_task(&mut self, input: CreateTaskInput) -> Result<Uuid, ApplicationError>;
    fn breakdown_task(&mut self, input: BreakdownTaskInput) -> Result<Vec<Uuid>, ApplicationError>;
    fn create_task_attr(&mut self, name: &str) -> TaskAttr;
    fn set_estimate(&mut self, task_id: Uuid, minutes: i64) -> Result<(), ApplicationError>;
    fn focused_task_id(&self) -> Option<Uuid>;
    fn set_focused_task_id(&mut self, task_id_opt: Option<Uuid>);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TaskListOrder {
    ScheduledStartDesc,
    LowPriorityTail,
}

#[derive(Debug)]
pub(super) enum NextUpResult {
    NoDisplay,
    ReportedError(ApplicationError),
}

pub(super) trait TaskTreeCommandContext {
    fn show_tree(&mut self) -> Result<TreeDisplay, ApplicationError>;
    fn show_ancestor(&mut self) -> Result<TreeDisplay, ApplicationError>;
    fn focus_root(&mut self) -> Result<(), ApplicationError>;
    fn show_leaves(&mut self) -> Result<TreeDisplay, ApplicationError>;
    fn show_task_list(
        &mut self,
        pattern: Option<&str>,
        order: TaskListOrder,
        resolve_pattern: bool,
    ) -> Result<DisplayModel, ApplicationError>;
    fn focus(&mut self, task_id: Uuid);
    fn pick(&mut self, task_id: Uuid) -> Result<(), ApplicationError>;
    fn focus_parent(&mut self) -> Result<(), ApplicationError>;
    fn focus_children(&mut self) -> Result<Option<DisplayModel>, ApplicationError>;
    fn focus_deepest(&mut self) -> Result<Option<DisplayModel>, ApplicationError>;
    fn next_up(
        &mut self,
        name: &str,
        estimated_minutes: Option<i64>,
    ) -> Result<NextUpResult, ApplicationError>;
}

pub(super) trait TaskAttributeCommandContext {
    fn set_deadline(&mut self, value: &str) -> Result<(), ApplicationError>;
    fn set_estimate(&mut self, minutes: i64) -> Result<(), ApplicationError>;
    fn arrange(
        &mut self,
        minutes: i64,
        includes_zero_estimate: bool,
    ) -> Result<(), ApplicationError>;
    fn set_actual(&mut self, minutes: i64) -> Result<(), ApplicationError>;
    fn set_priority(&mut self, priority: i64) -> Result<(), ApplicationError>;
    fn set_category(&mut self, value: &str) -> Result<(), ApplicationError>;
    fn add_work(&mut self, minutes: Option<i64>) -> Result<(), ApplicationError>;
}

pub(super) trait DeferCommandContext {
    fn defer(&mut self, amount: i64, unit: &str) -> Result<(), DeferCommandError>;
    fn defer_expression(&mut self, values: &[String]) -> Result<(), DeferCommandError>;
    fn defer_next_morning(&mut self) -> Result<(), DeferCommandError>;
    fn defer_next_week(&mut self) -> Result<(), DeferCommandError>;
    fn defer_routine(&mut self) -> Result<(), ApplicationError>;
    fn defer_five_years(&mut self) -> Result<(), DeferCommandError>;
    fn defer_all_frequent_routines(&mut self) -> Result<(), ApplicationError>;
    fn prepare_escape(&mut self) -> Result<bool, ApplicationError>;
    fn extrude(&mut self, step_days: Option<u16>) -> Result<(), ApplicationError>;
    fn clear_or_gather(
        &mut self,
        kind: CommandKind,
        values: &[String],
    ) -> Result<(), ApplicationError>;
}

pub(super) trait FinishPlacementCommandContext {
    fn last_synced_time(&self) -> DateTime<Local>;
    fn focus_started_datetime(&self) -> DateTime<Local>;
    fn focused_task(&self) -> Result<Option<TaskHandle>, ApplicationError>;
    fn show_focused_tree(&mut self) -> Result<TreeDisplay, ApplicationError>;
    fn complete_focused_task(
        &mut self,
        input: CompleteTaskInput,
    ) -> Result<Option<Uuid>, ApplicationError>;
    fn set_focused_task_id(&mut self, task_id_opt: Option<Uuid>);
    fn pack(&mut self) -> Result<PackResult, ApplicationError>;
    fn flatten(&mut self) -> Result<FlattenResult, ApplicationError>;
}

#[derive(Debug)]
pub(super) enum DeferCommandError {
    Parse(CommandParseError),
    Application(ApplicationError),
}

#[derive(Debug)]
pub(super) enum HandlerError {
    Parse(CommandParseError),
    Application(ApplicationError),
}

impl std::fmt::Display for HandlerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(formatter),
            Self::Application(error) => write!(formatter, "操作エラー: {error}"),
        }
    }
}

impl std::error::Error for HandlerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Application(error) => Some(error),
        }
    }
}

impl From<ApplicationError> for HandlerError {
    fn from(error: ApplicationError) -> Self {
        Self::Application(error)
    }
}

impl From<DeferCommandError> for HandlerError {
    fn from(error: DeferCommandError) -> Self {
        match error {
            DeferCommandError::Parse(error) => Self::Parse(error),
            DeferCommandError::Application(error) => Self::Application(error),
        }
    }
}

impl std::fmt::Display for DeferCommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(formatter),
            Self::Application(error) => write!(formatter, "操作エラー: {error}"),
        }
    }
}

impl From<ApplicationError> for DeferCommandError {
    fn from(error: ApplicationError) -> Self {
        Self::Application(error)
    }
}

impl CommandOutcome {
    fn empty(kind: CommandKind) -> Self {
        Self {
            kind,
            display: DisplayModel::default(),
            external_request: None,
            focus_change: FocusChange::Keep,
        }
    }

    fn semantic_empty(kind: CommandKind) -> Self {
        let mut outcome = Self::empty(kind);
        outcome.display = DisplayModel::empty();
        outcome
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
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Unfocus,
            ..
        }) => outcome.focus_change = FocusChange::Clear,
        Command::Action(CommandAction::FocusMode {
            kind: CommandKind::FocusHighest,
            ..
        }) => {
            outcome.focus_change = FocusChange::SelectionMode(FocusSelection::HighestPriority);
            outcome.display = DisplayModel::Message {
                level: MessageLevel::Plain,
                text: "フォーカス選択モード: 高".to_string(),
            };
        }
        Command::Action(CommandAction::FocusMode {
            kind: CommandKind::FocusLowest,
            recent_days,
            ..
        }) => {
            let recent_days = recent_days.unwrap_or(0);
            outcome.focus_change =
                FocusChange::SelectionMode(FocusSelection::LowestPriority { recent_days });
            let label = if recent_days == 0 {
                "低".to_string()
            } else {
                format!("低 {recent_days}")
            };
            outcome.display = DisplayModel::Message {
                level: MessageLevel::Plain,
                text: format!("フォーカス選択モード: {label}"),
            };
        }
        _ => return None,
    }

    Some(outcome)
}

pub(super) fn handle_command<C: CommandContext + ?Sized>(
    command: &Command,
    context: &mut C,
) -> Result<Option<CommandOutcome>, HandlerError> {
    if command.kind() == CommandKind::Verify {
        return Ok(None);
    }
    if let Command::Focus { task_id } = command {
        let mut outcome = CommandOutcome::empty(CommandKind::Focus);
        outcome.focus_change = FocusChange::Set(*task_id);
        return Ok(Some(outcome));
    }
    if let Some(outcome) = handle(command) {
        return Ok(Some(outcome));
    }
    if let Some(outcome) = handle_project_command(command, context)? {
        return Ok(Some(outcome));
    }
    if let Some(outcome) = handle_breakdown_split_command(command, context)? {
        return Ok(Some(outcome));
    }
    if let Some(outcome) = handle_task_attribute_command(command, context)? {
        return Ok(Some(outcome));
    }
    if let Some(outcome) = handle_defer_command(command, context)? {
        return Ok(Some(outcome));
    }
    if let Some(outcome) = handle_finish_placement_command(command, context)? {
        return Ok(Some(outcome));
    }
    if let Some(outcome) = handle_task_tree_command(command, context)? {
        return Ok(Some(outcome));
    }

    Ok(None)
}

pub(super) fn handle_task_tree_command<C: TaskTreeCommandContext + ?Sized>(
    command: &Command,
    context: &mut C,
) -> Result<Option<CommandOutcome>, ApplicationError> {
    let mut semantic_display = None;
    let kind = command.kind();

    match command {
        Command::ShowAll { pattern } => {
            semantic_display = Some(context.show_task_list(
                pattern.as_deref(),
                TaskListOrder::ScheduledStartDesc,
                true,
            )?)
        }
        Command::Focus { task_id } => context.focus(*task_id),
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Tree,
            ..
        }) => semantic_display = Some(DisplayModel::Tree(context.show_tree()?)),
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Ancestor,
            ..
        }) => semantic_display = Some(DisplayModel::Tree(context.show_ancestor()?)),
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Root,
            ..
        }) => context.focus_root()?,
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Leaves,
            ..
        }) => semantic_display = Some(DisplayModel::Tree(context.show_leaves()?)),
        Command::Action(CommandAction::OptionalPattern {
            kind: CommandKind::Tail,
            pattern,
            ..
        }) => {
            semantic_display = Some(context.show_task_list(
                Some(pattern.as_deref().unwrap_or("今")),
                TaskListOrder::LowPriorityTail,
                false,
            )?)
        }
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Today,
            ..
        }) => {
            semantic_display = Some(context.show_task_list(
                Some("今"),
                TaskListOrder::ScheduledStartDesc,
                false,
            )?)
        }
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::NonRepetitive,
            ..
        }) => {
            semantic_display = Some(context.show_task_list(
                Some("単"),
                TaskListOrder::ScheduledStartDesc,
                false,
            )?)
        }
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Calendar,
            ..
        }) => {
            semantic_display = Some(context.show_task_list(
                Some("暦"),
                TaskListOrder::ScheduledStartDesc,
                false,
            )?)
        }
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Band,
            ..
        }) => {
            semantic_display = Some(context.show_task_list(
                Some("帯"),
                TaskListOrder::ScheduledStartDesc,
                false,
            )?)
        }
        Command::Action(CommandAction::Pick { task_id }) => context.pick(*task_id)?,
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Parent,
            ..
        }) => context.focus_parent()?,
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Children,
            ..
        }) => semantic_display = context.focus_children()?,
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Deepest,
            ..
        }) => semantic_display = context.focus_deepest()?,
        Command::Action(CommandAction::TaskWithEstimate {
            kind: CommandKind::NextUp,
            name,
            estimated_minutes,
            ..
        }) => {
            semantic_display = match context.next_up(name, *estimated_minutes)? {
                NextUpResult::NoDisplay => None,
                NextUpResult::ReportedError(error) => Some(DisplayModel::Message {
                    level: MessageLevel::Error,
                    text: HandlerError::Application(error).to_string(),
                }),
            }
        }
        _ => return Ok(None),
    }

    let mut outcome = CommandOutcome::empty(kind);
    if let Some(display) = semantic_display {
        outcome.display = display;
    } else {
        return Ok(Some(CommandOutcome::semantic_empty(kind)));
    }
    Ok(Some(outcome))
}

pub(super) fn handle_project_command<C: ProjectCommandContext + ?Sized>(
    command: &Command,
    context: &mut C,
) -> Result<Option<CommandOutcome>, ApplicationError> {
    let action = match command {
        Command::Action(action) => action,
        _ => return Ok(None),
    };
    let kind = command.kind();

    match action {
        CommandAction::NewProject {
            kind,
            name,
            estimated_minutes,
            ..
        } => {
            let defer_days_opt = match kind {
                CommandKind::NewProject => Some(1),
                CommandKind::HobbyProject => Some(1400),
                CommandKind::UnplannedProject => None,
                _ => return Ok(None),
            };
            execute_start_new_project(context, name, defer_days_opt, *estimated_minutes)?;
            Ok(Some(project_command_outcome(*kind, Vec::new(), None)))
        }
        CommandAction::Sequential {
            name,
            estimated_minutes,
            begin_index,
            end_index,
            suffix,
        } => {
            let (Ok(begin_index), Ok(end_index)) =
                (u64::try_from(*begin_index), u64::try_from(*end_index))
            else {
                return Ok(Some(project_command_outcome(kind, Vec::new(), None)));
            };
            if begin_index > end_index {
                return Ok(Some(project_command_outcome(kind, Vec::new(), None)));
            }
            let focused_task_opt = context.focused_task()?;
            let suffix = suffix
                .as_ref()
                .map_or_else(String::new, |suffix| format!("-{suffix}"));
            let result = execute_breakdown_sequentially(
                context,
                &focused_task_opt,
                name,
                *estimated_minutes,
                begin_index,
                end_index,
                &suffix,
            );
            let error = result.err();
            Ok(Some(project_command_outcome(kind, Vec::new(), error)))
        }
        CommandAction::Repeat {
            name,
            estimated_minutes,
            day,
            start_time,
            deadline_time,
        } => {
            let mut lines = Vec::new();
            let result = execute_create_repetition_task(
                context,
                name,
                day,
                *estimated_minutes,
                start_time,
                deadline_time,
                &mut lines,
            );
            Ok(Some(project_command_outcome(kind, lines, result.err())))
        }
        CommandAction::TimeExpression {
            kind: CommandKind::Appointment,
            values,
            ..
        } => {
            let now = context.last_synced_time();
            if let Some(start_time) = decide_time_values(values, &now)? {
                let focused_task_opt = context.focused_task()?;
                execute_make_appointment(&focused_task_opt, start_time)?;
            }
            Ok(Some(project_command_outcome(kind, Vec::new(), None)))
        }
        CommandAction::TimeExpression {
            kind: CommandKind::Start,
            values,
            ..
        } => {
            let now = context.last_synced_time();
            if let Some(start_time) = decide_time_values(values, &now)? {
                if let Some(task) = context.focused_task()? {
                    task.set_start_time(start_time)
                        .map_err(ApplicationError::TaskTree)?;
                }
            }
            Ok(Some(project_command_outcome(kind, Vec::new(), None)))
        }
        _ => Ok(None),
    }
}

pub(super) fn handle_breakdown_split_command<C: ProjectCommandContext + ?Sized>(
    command: &Command,
    context: &mut C,
) -> Result<Option<CommandOutcome>, ApplicationError> {
    let action = match command {
        Command::Action(action) => action,
        _ => return Ok(None),
    };
    let kind = command.kind();
    let mut lines = Vec::new();
    let mut reported_error = None;

    match action {
        CommandAction::TaskNames { names } => {
            if !names.is_empty() && !names.iter().any(|name| name.parse::<i64>().is_ok()) {
                let names = names.iter().map(String::as_str).collect::<Vec<_>>();
                match execute_breakdown(context, &names, &None) {
                    Ok(Some(child_ids)) => append_project_lines(&mut lines, &child_ids, &names),
                    Ok(None) => {}
                    Err(error) => reported_error = Some(error),
                }
            }
        }
        CommandAction::Split { minutes, name } => {
            let focused_task = context.focused_task()?;
            match execute_split(context, &focused_task, name, *minutes) {
                Ok(Some(task_id)) => lines.push(format!("{task_id} {name}")),
                Ok(None) => {}
                Err(error) => reported_error = Some(error),
            }
        }
        CommandAction::NoArguments {
            kind: CommandKind::Wait,
            ..
        } => {
            if let Some(focused_task) = context.focused_task()? {
                let _result = focused_task
                    .set_is_on_other_side(true)
                    .map_err(ApplicationError::TaskTree);
            }
        }
        _ => return Ok(None),
    }

    Ok(Some(project_command_outcome(kind, lines, reported_error)))
}

pub(super) fn handle_task_attribute_command<C: TaskAttributeCommandContext + ?Sized>(
    command: &Command,
    context: &mut C,
) -> Result<Option<CommandOutcome>, ApplicationError> {
    let kind = command.kind();

    match command {
        Command::Action(CommandAction::StringValue {
            kind: CommandKind::Deadline,
            value,
            ..
        }) => context.set_deadline(value)?,
        Command::Estimate { minutes } => context.set_estimate(*minutes)?,
        Command::Arrange {
            minutes,
            includes_zero_estimate,
        } => context.arrange(*minutes, *includes_zero_estimate)?,
        Command::Action(CommandAction::IntegerValue {
            kind: CommandKind::Actual,
            value,
            ..
        }) => context.set_actual(*value)?,
        Command::Action(CommandAction::IntegerValue {
            kind: CommandKind::Priority,
            value,
            ..
        }) => context.set_priority(*value)?,
        Command::Action(CommandAction::StringValue {
            kind: CommandKind::Category,
            value,
            ..
        }) => context.set_category(value)?,
        Command::Action(CommandAction::OptionalInteger {
            kind: CommandKind::Work,
            value,
            ..
        }) => context.add_work(*value)?,
        _ => return Ok(None),
    }

    Ok(Some(CommandOutcome::empty(kind)))
}

pub(super) fn handle_defer_command<C: DeferCommandContext + ?Sized>(
    command: &Command,
    context: &mut C,
) -> Result<Option<CommandOutcome>, ApplicationError> {
    let kind = command.kind();
    let reported_result = match command {
        Command::Defer { amount, unit } => Some(context.defer(*amount, unit)),
        Command::Action(CommandAction::TimeExpression {
            kind: CommandKind::Defer,
            values,
            ..
        }) => Some(context.defer_expression(values)),
        Command::InteractiveShortcut(InteractiveShortcut::NextMorning) => {
            Some(context.defer_next_morning())
        }
        Command::InteractiveShortcut(InteractiveShortcut::NextWeek) => {
            Some(context.defer_next_week())
        }
        Command::InteractiveShortcut(InteractiveShortcut::DeferRoutine) => {
            let _result = context.defer_routine();
            return Ok(Some(CommandOutcome::empty(kind)));
        }
        Command::InteractiveShortcut(InteractiveShortcut::FiveYears) => {
            Some(context.defer_five_years())
        }
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::DeferRoutines,
            ..
        }) => {
            context.defer_all_frequent_routines()?;
            return Ok(Some(CommandOutcome::empty(kind)));
        }
        Command::Action(CommandAction::Escape { defer_expression }) => {
            if !context.prepare_escape()? {
                return Ok(Some(CommandOutcome::empty(kind)));
            }
            let Some(values) = defer_expression else {
                return Ok(Some(CommandOutcome::empty(kind)));
            };
            let result = context.defer_expression(values);
            return Ok(Some(outcome_from_reported_defer_result(kind, result)));
        }
        Command::Action(CommandAction::Extrude { step_days }) => {
            context.extrude(*step_days)?;
            return Ok(Some(CommandOutcome::empty(kind)));
        }
        Command::Action(CommandAction::ClearOrGather { kind, values, .. }) => {
            context.clear_or_gather(*kind, values)?;
            return Ok(Some(CommandOutcome::empty(command.kind())));
        }
        _ => return Ok(None),
    };

    Ok(reported_result.map(|result| outcome_from_reported_defer_result(kind, result)))
}

pub(super) fn handle_finish_placement_command<C: FinishPlacementCommandContext + ?Sized>(
    command: &Command,
    context: &mut C,
) -> Result<Option<CommandOutcome>, ApplicationError> {
    let kind = command.kind();
    let mut semantic_display = None;

    match command {
        Command::Action(CommandAction::Finish { values }) => {
            let Some(focused_task) = context.focused_task()? else {
                return Ok(Some(CommandOutcome::semantic_empty(kind)));
            };
            if focused_task
                .has_undone_children()
                .map_err(ApplicationError::TaskTree)?
            {
                semantic_display = Some(DisplayModel::Tree(context.show_focused_tree()?));
            } else {
                let now = context.last_synced_time();
                if let Some(finished_at) = decide_finish_time_values(values, &now)? {
                    let additional_actual_work_seconds = if values.is_empty() {
                        let focus_duration_seconds =
                            (now - context.focus_started_datetime()).num_seconds();
                        if focus_duration_seconds >= 60 {
                            focus_duration_seconds
                        } else {
                            0
                        }
                    } else {
                        0
                    };
                    let input = CompleteTaskInput {
                        task_id: focused_task.get_id().map_err(ApplicationError::TaskTree)?,
                        finished_at,
                        additional_actual_work_seconds,
                    };
                    match context.complete_focused_task(input) {
                        Ok(next_focus_task_id) => {
                            context.set_focused_task_id(next_focus_task_id);
                        }
                        Err(ApplicationError::HasUndoneChildren(_)) => {
                            semantic_display =
                                Some(DisplayModel::Tree(context.show_focused_tree()?));
                        }
                        Err(_) => {}
                    }
                }
            }
        }
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Pack,
            ..
        }) => {
            semantic_display = Some(DisplayModel::Pack(pack_display(context.pack()?)));
        }
        Command::Action(CommandAction::NoArguments {
            kind: CommandKind::Flatten,
            ..
        }) => {
            semantic_display = Some(DisplayModel::Flatten(flatten_display(context.flatten()?)));
        }
        _ => return Ok(None),
    }

    Ok(Some(match semantic_display {
        Some(display) => {
            let mut outcome = CommandOutcome::empty(kind);
            outcome.display = display;
            outcome
        }
        None => CommandOutcome::semantic_empty(kind),
    }))
}

pub(super) fn pack_display(result: PackResult) -> PackDisplay {
    PackDisplay {
        rows: result
            .packed_tasks
            .into_iter()
            .map(|packed| PackRow {
                source_date: packed.source_date,
                target_date: packed.target_date,
                work_seconds: packed.work_seconds,
                priority: packed.priority,
                task_id: packed.task_id,
                name: packed.name,
            })
            .collect(),
        skipped_count: result.skipped_tasks.len(),
    }
}

fn flatten_display(result: FlattenResult) -> FlattenDisplay {
    FlattenDisplay {
        rows: result
            .flattened_tasks
            .into_iter()
            .map(|flattened| FlattenRow {
                source_date: flattened.source_date,
                target_date: flattened.target_date,
                work_seconds: flattened.work_seconds,
                priority: flattened.priority,
                task_id: flattened.task_id,
                name: flattened.name,
            })
            .collect(),
        overflowed_task_count: result.overflowed_task_count,
        overflowed_work_seconds: result.overflowed_work_seconds,
        had_overload: result.had_overload,
        unresolved_days: result
            .unresolved_overloads
            .into_iter()
            .map(|unresolved| FlattenUnresolvedDay {
                date: unresolved.date,
                excess_work_seconds: unresolved.excess_work_seconds,
                reasons: unresolved
                    .reasons
                    .into_iter()
                    .map(|summary| FlattenReasonSummary {
                        reason: match summary.reason {
                            UnresolvedReason::OnOtherSide => FlattenReason::OnOtherSide,
                            UnresolvedReason::CrossesBusinessDay => {
                                FlattenReason::CrossesBusinessDay
                            }
                            UnresolvedReason::ExceedsDailyCapacity => {
                                FlattenReason::ExceedsDailyCapacity
                            }
                            UnresolvedReason::OwnDeadline => FlattenReason::OwnDeadline,
                            UnresolvedReason::RelatedDeadline => FlattenReason::RelatedDeadline,
                            UnresolvedReason::Other => FlattenReason::Other,
                        },
                        task_count: summary.task_count,
                        representative_task_id: summary.representative_task_id,
                        representative_task_name: summary.representative_task_name,
                    })
                    .collect(),
            })
            .collect(),
    }
}

pub(super) fn decide_finish_time_values(
    values: &[String],
    now: &DateTime<Local>,
) -> Result<Option<DateTime<Local>>, ApplicationError> {
    let hhmmss_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})(?::(\d{1,2}))?$").unwrap();
    let yyyymmdd_reg = Regex::new(r"^\d{2,4}/\d{1,2}/\d{1,2}$").unwrap();
    let mmdd_reg = Regex::new(r"^\d{1,2}/\d{1,2}$").unwrap();
    let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];

    let build_finish_time = |hhmmss: &str,
                             date: Option<&String>|
     -> Result<Option<DateTime<Local>>, ApplicationError> {
        let Some(captures) = hhmmss_reg.captures(hhmmss) else {
            return Ok(None);
        };
        let Some(hours) = captures[1].parse::<u32>().ok() else {
            return Ok(None);
        };
        let Some(minutes) = captures[2].parse::<u32>().ok() else {
            return Ok(None);
        };
        let Some(seconds) = captures
            .get(3)
            .map(|value| value.as_str().parse().ok())
            .unwrap_or(Some(0))
        else {
            return Ok(None);
        };
        let mut time_values = vec![format!("{hours}:{minutes}")];
        if let Some(date) = date {
            time_values.push(date.clone());
        }
        let Some(naive_datetime) = decide_naive_datetime_values(&time_values, now, seconds)? else {
            return Ok(None);
        };
        resolve_date_and_time(naive_datetime.date(), naive_datetime.time()).map(Some)
    };

    match values {
        [] => Ok(Some(*now)),
        [value] if matches!(value.as_str(), "今" | "now") => Ok(Some(*now)),
        [time] if hhmmss_reg.is_match(time) => build_finish_time(time, None),
        [time, date]
            if hhmmss_reg.is_match(time)
                && (yyyymmdd_reg.is_match(date)
                    || mmdd_reg.is_match(date)
                    || date.starts_with('明')
                    || days_of_week.contains(&date.as_str())) =>
        {
            build_finish_time(time, Some(date))
        }
        _ => Ok(None),
    }
}

fn project_command_outcome(
    kind: CommandKind,
    lines: Vec<String>,
    reported_error: Option<ApplicationError>,
) -> CommandOutcome {
    let mut messages = lines
        .into_iter()
        .map(|text| DisplayModel::Message {
            level: MessageLevel::Plain,
            text,
        })
        .collect::<Vec<_>>();
    if let Some(error) = reported_error {
        messages.push(DisplayModel::Message {
            level: MessageLevel::Error,
            text: format!("操作エラー: {error}"),
        });
    }

    let mut outcome = CommandOutcome::empty(kind);
    outcome.display = DisplayModel::Sequence(messages);
    outcome
}

fn append_project_lines(lines: &mut Vec<String>, task_ids: &[Uuid], task_names: &[&str]) {
    lines.extend(
        task_ids
            .iter()
            .zip(task_names)
            .map(|(task_id, task_name)| format!("{task_id} {task_name}")),
    );
}

fn outcome_from_reported_defer_result(
    kind: CommandKind,
    result: Result<(), DeferCommandError>,
) -> CommandOutcome {
    let mut outcome = CommandOutcome::empty(kind);
    if let Err(error) = result {
        outcome.display = DisplayModel::Message {
            level: MessageLevel::Error,
            text: error.to_string(),
        };
    }
    outcome
}

fn execute_start_new_project<C: ProjectCommandContext + ?Sized>(
    context: &mut C,
    name: &str,
    defer_days_opt: Option<i64>,
    estimated_work_minutes_opt: Option<i64>,
) -> Result<(), ApplicationError> {
    validate_task_name(name, "name")?;
    if let Some(estimated_work_minutes) = estimated_work_minutes_opt {
        estimated_work_seconds_from_minutes(estimated_work_minutes)?;
    }
    let pending_until = if let Some(defer_days) = defer_days_opt {
        let now = context.last_synced_time();
        let next_business_day_start = try_next_business_day_start(now)?;
        let offset_days =
            defer_days
                .checked_sub(1)
                .ok_or(ApplicationError::SubjectiveDateOutOfRange {
                    operation: "next_business_day_start",
                    datetime: now,
                })?;
        let offset =
            Duration::try_days(offset_days).ok_or(ApplicationError::SubjectiveDateOutOfRange {
                operation: "next_business_day_start",
                datetime: now,
            })?;
        Some(next_business_day_start.checked_add_signed(offset).ok_or(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "next_business_day_start",
                datetime: now,
            },
        )?)
    } else {
        None
    };
    let task_id = context.create_task(CreateTaskInput {
        name: name.to_string(),
        estimated_work_minutes: estimated_work_minutes_opt,
        pending_until,
    })?;
    context.set_focused_task_id(Some(task_id));
    Ok(())
}

fn execute_make_appointment(
    focused_task_opt: &Option<TaskHandle>,
    start_time: DateTime<Local>,
) -> Result<(), ApplicationError> {
    if let Some(task) = focused_task_opt {
        task.make_appointment(start_time)
            .map_err(ApplicationError::TaskTree)?;
    }
    Ok(())
}

fn execute_breakdown_sequentially<C: ProjectCommandContext + ?Sized>(
    context: &mut C,
    focused_task_opt: &Option<TaskHandle>,
    name: &str,
    estimated_work_minutes: i64,
    begin_index: u64,
    end_index: u64,
    suffix: &str,
) -> Result<Option<Uuid>, ApplicationError> {
    validate_task_name(name, "name")?;
    let estimated_work_seconds = estimated_work_seconds_from_minutes(estimated_work_minutes)?;

    if let Some(focused_task) = focused_task_opt {
        let grand_child_task = focused_task
            .create_sequential_children(
                name,
                estimated_work_seconds,
                begin_index,
                end_index,
                suffix,
                |child_name| context.create_task_attr(child_name),
            )
            .map_err(ApplicationError::TaskTree)?;
        let grand_child_task_id = grand_child_task
            .get_id()
            .map_err(ApplicationError::TaskTree)?;
        context.set_focused_task_id(Some(grand_child_task_id));
        return Ok(Some(grand_child_task_id));
    }
    Ok(None)
}

fn execute_breakdown<C: ProjectCommandContext + ?Sized>(
    context: &mut C,
    new_task_names: &[&str],
    pending_until_opt: &Option<DateTime<Local>>,
) -> Result<Option<Vec<Uuid>>, ApplicationError> {
    let Some(parent_id) = context.focused_task_id() else {
        return Ok(None);
    };
    let names = new_task_names
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    let child_ids = context.breakdown_task(BreakdownTaskInput {
        parent_id,
        names,
        pending_until: *pending_until_opt,
    })?;
    context.set_focused_task_id(child_ids.first().copied());
    Ok(Some(child_ids))
}

fn execute_split<C: ProjectCommandContext + ?Sized>(
    context: &mut C,
    focused_task_opt: &Option<TaskHandle>,
    new_task_name: &str,
    splitted_work_minutes: i64,
) -> Result<Option<Uuid>, ApplicationError> {
    validate_task_name(new_task_name, "name")?;

    let Some(focused_task) = focused_task_opt else {
        return Ok(None);
    };
    let focused_estimated_work_seconds = focused_task
        .get_estimated_work_seconds()
        .map_err(ApplicationError::TaskTree)?;
    let splitted_work_seconds = if splitted_work_minutes > 0 {
        min(
            estimated_work_seconds_from_minutes(splitted_work_minutes)?,
            focused_estimated_work_seconds,
        )
    } else {
        let retained_work_minutes =
            splitted_work_minutes
                .checked_abs()
                .ok_or(ApplicationError::InvalidInput {
                    field: "splitted_work_minutes",
                    reason: "absolute value is too large",
                })?;
        let retained_work_seconds = estimated_work_seconds_from_minutes(retained_work_minutes)?;
        if focused_estimated_work_seconds > retained_work_seconds {
            focused_estimated_work_seconds - retained_work_seconds
        } else {
            0
        }
    };

    focused_task
        .set_estimated_work_seconds(focused_estimated_work_seconds - splitted_work_seconds)
        .map_err(ApplicationError::TaskTree)?;

    let mut new_task_attr = context.create_task_attr(new_task_name);
    new_task_attr.set_estimated_work_seconds(splitted_work_seconds);
    if let Some(deadline_time) = focused_task
        .get_deadline_time_opt()
        .map_err(ApplicationError::TaskTree)?
    {
        new_task_attr.set_deadline_time_opt(Some(deadline_time));
    }

    let new_task = focused_task
        .create_child(new_task_attr)
        .map_err(ApplicationError::TaskTree)?;
    let new_task_id = new_task.get_id().map_err(ApplicationError::TaskTree)?;
    context.set_focused_task_id(Some(new_task_id));
    Ok(Some(new_task_id))
}

#[allow(clippy::too_many_arguments)]
fn execute_create_repetition_task<C: ProjectCommandContext + ?Sized>(
    context: &mut C,
    name: &str,
    day: &str,
    estimated_work_minutes: i64,
    _start_time: &str,
    _deadline_time: &str,
    lines: &mut Vec<String>,
) -> Result<Option<Uuid>, ApplicationError> {
    estimated_work_seconds_from_minutes(estimated_work_minutes)?;
    let Some(child_ids) = execute_breakdown(context, &[name], &None)? else {
        return Ok(None);
    };
    append_project_lines(lines, &child_ids, &[name]);
    let repetition_parent_task_opt = context.focused_task()?;
    if let Some(task_id) = repetition_parent_task_opt
        .map(|task| task.get_id())
        .transpose()
        .map_err(ApplicationError::TaskTree)?
    {
        context.set_estimate(task_id, estimated_work_minutes)?;
    }

    let task_num = if day == "毎" { 7 } else { 4 };
    if let Some(repetition_parent_task_id) = context.focused_task_id() {
        for _ in 0..task_num {
            let Some(child_ids) = execute_breakdown(context, &[name], &None)? else {
                return Ok(None);
            };
            append_project_lines(lines, &child_ids, &[name]);
            let child_task_opt = context.focused_task()?;
            if let Some(task_id) = child_task_opt
                .map(|task| task.get_id())
                .transpose()
                .map_err(ApplicationError::TaskTree)?
            {
                context.set_estimate(task_id, estimated_work_minutes)?;
            }
            context.set_focused_task_id(Some(repetition_parent_task_id));
        }
        Ok(Some(repetition_parent_task_id))
    } else {
        Ok(None)
    }
}

pub(super) fn decide_time_values(
    values: &[String],
    now: &DateTime<Local>,
) -> Result<Option<DateTime<Local>>, ApplicationError> {
    let Some(naive_datetime) = decide_naive_datetime_values(values, now, 0)? else {
        return Ok(None);
    };
    resolve_date_and_time(naive_datetime.date(), naive_datetime.time()).map(Some)
}

fn decide_naive_datetime_values(
    values: &[String],
    now: &DateTime<Local>,
    seconds: u32,
) -> Result<Option<NaiveDateTime>, ApplicationError> {
    let Some(start_hhmm_str) = values.first() else {
        return Ok(None);
    };
    let start_date_str = values.get(1).map_or("dummy", String::as_str);
    let hhmm_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})$").unwrap();
    let Some(captures) = hhmm_reg.captures(start_hhmm_str) else {
        return Ok(None);
    };
    let (Some(hh), Some(mm)) = (
        captures[1].parse::<u32>().ok(),
        captures[2].parse::<u32>().ok(),
    ) else {
        return Ok(None);
    };
    let Some(time) = NaiveTime::from_hms_opt(hh, mm, seconds) else {
        return Ok(None);
    };
    let yyyymmdd_reg = Regex::new(r"^(\d{2,4})/(\d{1,2})/(\d{1,2})$").unwrap();
    let mmdd_reg = Regex::new(r"^(\d{1,2})/(\d{1,2})$").unwrap();

    if let Some(captures) = yyyymmdd_reg.captures(start_date_str) {
        let (Some(raw_year), Some(month), Some(day)) = (
            captures[1].parse::<i32>().ok(),
            captures[2].parse::<u32>().ok(),
            captures[3].parse::<u32>().ok(),
        ) else {
            return Ok(None);
        };
        let year = if raw_year < 100 {
            raw_year + 2000
        } else {
            raw_year
        };
        let Some(date) = NaiveDate::from_ymd_opt(year, month, day) else {
            return Ok(None);
        };
        return Ok(Some(date.and_time(time)));
    }
    if let Some(captures) = mmdd_reg.captures(start_date_str) {
        let (Some(month), Some(day)) = (
            captures[1].parse::<u32>().ok(),
            captures[2].parse::<u32>().ok(),
        ) else {
            return Ok(None);
        };
        let Some(mut date) = NaiveDate::from_ymd_opt(now.year(), month, day) else {
            return Ok(None);
        };
        let mut answer = date.and_time(time);
        if answer < now.naive_local() {
            let next_year =
                now.year()
                    .checked_add(1)
                    .ok_or(ApplicationError::SubjectiveDateOutOfRange {
                        operation: "upcoming_calendar_date",
                        datetime: *now,
                    })?;
            let Some(next_date) = NaiveDate::from_ymd_opt(next_year, month, day) else {
                return Ok(None);
            };
            date = next_date;
            answer = date.and_time(time);
        }
        return Ok(Some(answer));
    }
    if start_date_str.starts_with('明') {
        let next_day = try_next_business_day_start(*now)?;
        return Ok(Some(next_day.date_naive().and_time(time)));
    }
    let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];
    if days_of_week.contains(&start_date_str) {
        let next_business_day_start = try_next_business_day_start(*now)?;
        let subjective_date = try_subjective_date(*now)?;
        let current_index = subjective_date.weekday().num_days_from_monday() as usize;
        let target_index = days_of_week
            .iter()
            .position(|day| day == &start_date_str)
            .unwrap();
        let difference = (7 + target_index - current_index) % 7;
        let days = if difference == 0 {
            7
        } else {
            difference as i64
        };
        let Some(target_date) = next_business_day_start
            .date_naive()
            .checked_add_days(Days::new((days - 1) as u64))
        else {
            return Err(ApplicationError::SubjectiveDateOutOfRange {
                operation: "weekday_date",
                datetime: *now,
            });
        };
        return Ok(Some(target_date.and_time(time)));
    }
    Ok(Some(now.date_naive().and_time(time)))
}

fn resolve_date_and_time(
    date: NaiveDate,
    time: NaiveTime,
) -> Result<DateTime<Local>, ApplicationError> {
    try_local_date_and_time(date, time)
}

#[cfg(test)]
mod datetime_resolution_tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone, Timelike};

    #[test]
    fn 完了時刻は指定秒を含むnaive日時を構築してから1回だけlocal変換する() {
        let now = Local.with_ymd_and_hms(2026, 10, 30, 12, 0, 0).unwrap();
        let date_values = ["1:30".to_string(), "2026/11/1".to_string()];
        let expected_naive = NaiveDate::from_ymd_opt(2026, 11, 1)
            .unwrap()
            .and_hms_opt(1, 30, 45)
            .unwrap();

        let naive = decide_naive_datetime_values(&date_values, &now, 45)
            .expect("date resolution must succeed")
            .expect("explicit date and time must resolve to a naive datetime");
        let localized =
            decide_finish_time_values(&["1:30:45".to_string(), "2026/11/1".to_string()], &now)
                .expect("application local datetime resolution must succeed")
                .expect("the local datetime must be Single in the test timezone");

        assert_eq!(naive, expected_naive);
        assert_eq!(naive.second(), 45);
        assert_eq!(localized.naive_local(), expected_naive);
        assert_eq!(localized.second(), 45);
    }
}

#[cfg(test)]
mod task_generation_context_tests {
    use super::*;
    use chrono::TimeZone;
    use std::collections::VecDeque;

    struct FixedIdentityProjectCommandContext {
        now: DateTime<Local>,
        next_ids: VecDeque<Uuid>,
        focused_task_id_opt: Option<Uuid>,
        created_task_inputs: Vec<CreateTaskInput>,
        created_task_attr_names: Vec<String>,
        focused_task_updates: Vec<Option<Uuid>>,
    }

    impl FixedIdentityProjectCommandContext {
        fn new(now: DateTime<Local>, next_ids: impl IntoIterator<Item = Uuid>) -> Self {
            Self {
                now,
                next_ids: next_ids.into_iter().collect(),
                focused_task_id_opt: None,
                created_task_inputs: Vec::new(),
                created_task_attr_names: Vec::new(),
                focused_task_updates: Vec::new(),
            }
        }
    }

    impl ProjectCommandContext for FixedIdentityProjectCommandContext {
        fn last_synced_time(&self) -> DateTime<Local> {
            self.now
        }

        fn focused_task(&mut self) -> Result<Option<TaskHandle>, ApplicationError> {
            unreachable!("this contract test passes the focused task explicitly")
        }

        fn create_task(&mut self, input: CreateTaskInput) -> Result<Uuid, ApplicationError> {
            self.created_task_inputs.push(input);
            Ok(self
                .next_ids
                .pop_front()
                .expect("the fixed identity sequence must cover every created task"))
        }

        fn breakdown_task(
            &mut self,
            _input: BreakdownTaskInput,
        ) -> Result<Vec<Uuid>, ApplicationError> {
            unreachable!("this contract test exercises direct child creation")
        }

        fn create_task_attr(&mut self, name: &str) -> TaskAttr {
            self.created_task_attr_names.push(name.to_string());
            TaskAttr::with_identity(
                name,
                self.next_ids
                    .pop_front()
                    .expect("the fixed identity sequence must cover every created task"),
                self.now,
            )
        }

        fn set_estimate(&mut self, _task_id: Uuid, _minutes: i64) -> Result<(), ApplicationError> {
            unreachable!("this contract test does not set estimates through the context")
        }

        fn focused_task_id(&self) -> Option<Uuid> {
            self.focused_task_id_opt
        }

        fn set_focused_task_id(&mut self, task_id_opt: Option<Uuid>) {
            self.focused_task_id_opt = task_id_opt;
            self.focused_task_updates.push(task_id_opt);
        }
    }

    fn task(name: &str, id: u128, now: DateTime<Local>) -> TaskHandle {
        TaskHandle::with_identity(name, Uuid::from_u128(id), now)
            .expect("test task creation must succeed")
    }

    #[test]
    fn project作成は次の業務日境界を算出できない場合に副作用なくerrorを返す() {
        let local_datetime = chrono::NaiveDate::MAX
            .and_hms_opt(6, 0, 0)
            .expect("maximum date at 06:00 must be valid");
        let now = Local
            .from_local_datetime(&local_datetime)
            .single()
            .expect("maximum local date at 06:00 must be unambiguous");
        let unused_id = Uuid::from_u128(100);
        let mut context = FixedIdentityProjectCommandContext::new(now, [unused_id]);
        let command = Command::Action(CommandAction::NewProject {
            kind: CommandKind::NewProject,
            canonical_name: "新",
            name: "project".to_string(),
            estimated_minutes: Some(30),
        });

        let actual = handle_project_command(&command, &mut context);

        assert_eq!(
            actual,
            Err(ApplicationError::SubjectiveDateOutOfRange {
                operation: "next_business_day_start",
                datetime: now,
            })
        );
        assert!(context.created_task_inputs.is_empty());
        assert!(context.created_task_attr_names.is_empty());
        assert_eq!(context.next_ids, [unused_id]);
        assert_eq!(context.focused_task_id_opt, None);
        assert!(context.focused_task_updates.is_empty());
    }

    #[test]
    fn splitはcontextが供給するidentityで子taskを生成する() {
        let now = Local.with_ymd_and_hms(2026, 8, 19, 12, 34, 56).unwrap();
        let child_id = Uuid::from_u128(101);
        let root = task("root", 1, now);
        root.set_estimated_work_seconds(90 * 60).unwrap();
        let mut context = FixedIdentityProjectCommandContext::new(now, [child_id]);

        let actual = execute_split(&mut context, &Some(root.clone()), "child", 30)
            .expect("split must succeed")
            .expect("split must create a child");

        let child = root.get_children().unwrap().remove(0);
        assert_eq!(actual, child_id);
        assert_eq!(child.get_id().unwrap(), child_id);
        assert_eq!(child.get_create_time().unwrap(), now);
        assert_eq!(context.focused_task_id(), Some(child_id));
        assert!(context.next_ids.is_empty());
    }

    #[test]
    fn sequentialは実生成順にcontextのidentityを消費して同じ時刻を共有する() {
        let now = Local.with_ymd_and_hms(2026, 8, 19, 12, 34, 56).unwrap();
        let creation_order_ids = [
            Uuid::from_u128(201),
            Uuid::from_u128(202),
            Uuid::from_u128(203),
        ];
        let root = task("root", 1, now);
        let mut context =
            FixedIdentityProjectCommandContext::new(now, creation_order_ids.iter().copied());

        let actual =
            execute_breakdown_sequentially(&mut context, &Some(root.clone()), "step", 10, 1, 3, "")
                .expect("sequential creation must succeed")
                .expect("sequential creation must return the deepest child");

        let step_3 = root.get_children().unwrap().remove(0);
        let step_2 = step_3.get_children().unwrap().remove(0);
        let step_1 = step_2.get_children().unwrap().remove(0);
        for (task, expected_name, expected_id) in [
            (&step_3, "step 3", creation_order_ids[0]),
            (&step_2, "step 2", creation_order_ids[1]),
            (&step_1, "step 1", creation_order_ids[2]),
        ] {
            assert_eq!(task.get_name().unwrap(), expected_name);
            assert_eq!(task.get_id().unwrap(), expected_id);
            assert_eq!(task.get_create_time().unwrap(), now);
        }
        assert_eq!(actual, creation_order_ids[2]);
        assert_eq!(context.focused_task_id(), Some(creation_order_ids[2]));
        assert!(context.next_ids.is_empty());
    }
}
