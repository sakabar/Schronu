use super::cli_discarded_session::{
    append_focus_transition, discarded_sessions_display, focus_snapshot, CliRepositoryTrait,
    FocusTransition,
};
#[cfg(test)]
use super::command::ParseMode;
use super::command::{
    parse_interactive_command, parse_interactive_command_with_maintenance_kind,
    parse_non_interactive_command_tokens, validate_command_input, Command, CommandKind,
    CommandParseError, CommandValidationError,
};
use super::command_context::*;
#[cfg(test)]
use super::command_test_support::parse_command;
use super::handler::{
    handle_command, CommandOutcome, ExternalRequest, FocusChange, FocusSelection,
    FocusSessionEffect, HandlerError,
};
#[cfg(test)]
use super::handler::{
    DeferCommandContext, DeferCommandError, NextUpResult, TaskListOrder, TaskTreeCommandContext,
};
use super::interactive;
#[cfg(test)]
use super::renderer::{
    format_band_day_row, format_focus_progress, format_focused_task_header, format_signed_seconds,
    BandDayRow, BandDurations, FocusDisplay, BAND_SEGMENTS,
};
use super::renderer::{
    render_display_model, render_display_model_with_mode, render_plain_display_model,
    writeln_newline, DisplayModel, ErrorCapturingWriter, MessageLevel, RenderMode, SchronuWriter,
};
use super::view::*;
use crate::adapter::gateway::free_time_manager::FreeTimeManager;
use crate::adapter::gateway::schronu_config::{load_schronu_config, SchronuConfig};
use crate::adapter::gateway::storage_lock::{LockMode, StorageLock, StorageLockError};
use crate::adapter::gateway::storage_snapshot::SnapshotError;
use crate::adapter::gateway::task_repository::TaskRepository;
#[cfg(test)]
use crate::application::daily_capacity::try_logical_date_start;
use crate::application::daily_capacity::{try_logical_date, try_next_logical_date_start};
use crate::application::interface::{BusyTimeSlotLoadError, FreeTimeManagerTrait};
use crate::application::interface::{TaskRepositoryError, TaskRepositoryTrait};
#[cfg(test)]
use crate::application::pack_use_case::pack_tasks_with_end_of_day_offset_minutes;
use crate::application::repository_transaction::{
    run_repository_transaction, RepositoryTransactionError,
};
use crate::application::task_use_case::{get_focus_excluding, ApplicationError, TaskFactory};
use crate::entity::discarded_session::DiscardedSessionReason;
#[cfg(test)]
use crate::entity::task::{ProjectCategory, TaskAttr, TaskTreeError};
use crate::entity::task::{Status, TaskHandle};
use chrono::{DateTime, Duration, Local};
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};
#[cfg(test)]
use regex::Regex;
#[cfg(test)]
use std::collections::HashMap;
use std::env;
use std::io::{stdout, Write};
use std::process;
use std::sync::OnceLock;

use super::storage_directory;
use std::time::Duration as StdDuration;
use storage_directory::resolve_project_storage_directory;
use termion::style;
use url::Url;
use uuid::Uuid;

#[cfg(test)]
use chrono::NaiveDate;

const CLI_LOCK_TIMEOUT: StdDuration = StdDuration::from_secs(1);

#[path = "runtime/storage_maintenance.rs"]
mod storage_maintenance;

static ACTIVE_CONFIG: OnceLock<SchronuConfig> = OnceLock::new();

pub(super) fn active_config() -> &'static SchronuConfig {
    ACTIVE_CONFIG.get_or_init(SchronuConfig::default)
}

// パーセントエンコーディングする対象にスペースを追加する
const MY_ASCII_SET: &AsciiSet = &CONTROLS.add(b' ');
const OBSIDIAN_VAULT_ASCII_SET: &AsciiSet = &MY_ASCII_SET.add(b'&').add(b'=');

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum FocusSelectionMode {
    HighestPriority {
        tucked_task_ids: Vec<Uuid>,
        is_explicit: bool,
        pending_exit: Option<PendingExit>,
    },
    LowestPriority {
        recent_days: i64,
        tucked_task_ids: Vec<Uuid>,
        is_explicit: bool,
        pending_exit: Option<PendingExit>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PendingExit {
    ended_at: DateTime<Local>,
    auto_event_id: Uuid,
    exit_event_id: Uuid,
}

impl FocusSelectionMode {
    pub(super) fn highest_priority() -> Self {
        Self::HighestPriority {
            tucked_task_ids: Vec::new(),
            is_explicit: false,
            pending_exit: None,
        }
    }

    fn lowest_priority(recent_days: i64) -> Self {
        Self::LowestPriority {
            recent_days,
            tucked_task_ids: Vec::new(),
            is_explicit: false,
            pending_exit: None,
        }
    }

    fn tuck_away(&mut self, task_id: Uuid) {
        let tucked_task_ids = match self {
            Self::HighestPriority {
                tucked_task_ids, ..
            }
            | Self::LowestPriority {
                tucked_task_ids, ..
            } => tucked_task_ids,
        };
        if !tucked_task_ids.contains(&task_id) {
            tucked_task_ids.push(task_id);
        }
        self.set_explicit(false);
    }

    fn is_explicit(&self) -> bool {
        match self {
            Self::HighestPriority { is_explicit, .. }
            | Self::LowestPriority { is_explicit, .. } => *is_explicit,
        }
    }

    fn set_explicit(&mut self, explicit: bool) {
        match self {
            Self::HighestPriority { is_explicit, .. }
            | Self::LowestPriority { is_explicit, .. } => *is_explicit = explicit,
        }
    }

    fn pending_exit(&self) -> Option<&PendingExit> {
        match self {
            Self::HighestPriority { pending_exit, .. }
            | Self::LowestPriority { pending_exit, .. } => pending_exit.as_ref(),
        }
    }

    fn set_pending_exit(&mut self, pending: Option<PendingExit>) {
        match self {
            Self::HighestPriority { pending_exit, .. }
            | Self::LowestPriority { pending_exit, .. } => *pending_exit = pending,
        }
    }
}

#[derive(Debug)]
enum RunError {
    Command(CommandError),
    BusyTimeSlots(BusyTimeSlotLoadError),
    Repository(TaskRepositoryError),
    CliRepositoryTransaction(CliRepositoryTransactionError),
    Snapshot(SnapshotError),
    InteractiveIo(interactive::InteractiveIoError),
    InputDisconnected {
        save_error_opt: Option<TaskRepositoryError>,
    },
    InputRead {
        input_error: std::io::Error,
        save_error_opt: Option<TaskRepositoryError>,
    },
    InputDisconnectedWithRepository {
        repository_error: CliRepositoryTransactionError,
    },
    InputReadWithRepository {
        input_error: std::io::Error,
        repository_error: CliRepositoryTransactionError,
    },
    Interrupted,
}

#[derive(Debug)]
pub(super) enum CommandError {
    Parse(CommandParseError),
    Application(ApplicationError),
    DiscardedSession(super::cli_discarded_session::CliDiscardedSessionError),
    Output(std::io::Error),
    #[cfg_attr(not(test), allow(dead_code))]
    ExitSaveDiagnostic {
        save_error: TaskRepositoryError,
        output_error: std::io::Error,
    },
    ExternalOpen {
        target: &'static str,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(formatter),
            Self::Application(error) => write!(formatter, "操作エラー: {error}"),
            Self::DiscardedSession(error) => error.fmt(formatter),
            Self::Output(error) => write!(formatter, "出力エラー: {error}"),
            Self::ExitSaveDiagnostic {
                save_error,
                output_error,
            } => write!(
                formatter,
                "終了前の保存に失敗しました: {save_error}; additionally, failed to display the save error: {output_error}"
            ),
            Self::ExternalOpen { target, source } => {
                write!(formatter, "外部起動エラー ({target}): {source}")
            }
        }
    }
}

impl std::error::Error for CommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Application(error) => Some(error),
            Self::DiscardedSession(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::ExitSaveDiagnostic { save_error, .. } => Some(save_error),
            Self::ExternalOpen { source, .. } => Some(source.as_ref()),
        }
    }
}

fn external_open_error(
    target: &'static str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> CommandError {
    CommandError::ExternalOpen {
        target,
        source: Box::new(source),
    }
}

impl From<ApplicationError> for CommandError {
    fn from(error: ApplicationError) -> Self {
        Self::Application(error)
    }
}

impl From<HandlerError> for CommandError {
    fn from(error: HandlerError) -> Self {
        match error {
            HandlerError::Parse(error) => Self::Parse(error),
            HandlerError::Application(error) => Self::Application(error),
        }
    }
}

impl From<CommandValidationError> for CommandError {
    fn from(error: CommandValidationError) -> Self {
        match error {
            CommandValidationError::Parse(error) => Self::Parse(error),
            CommandValidationError::Application(error) => Self::Application(error),
        }
    }
}

impl From<super::cli_discarded_session::CliDiscardedSessionError> for CommandError {
    fn from(error: super::cli_discarded_session::CliDiscardedSessionError) -> Self {
        Self::DiscardedSession(error)
    }
}

fn map_command_parse_error(error: CommandParseError) -> CommandError {
    CommandError::Parse(error)
}

fn error_display_model(error: &impl std::fmt::Display) -> DisplayModel {
    DisplayModel::Message {
        level: MessageLevel::Error,
        text: error.to_string(),
    }
}

fn verify_display_model() -> DisplayModel {
    DisplayModel::Message {
        level: MessageLevel::Plain,
        text: "検証: OK".to_string(),
    }
}

pub(super) fn report_application_result<T>(
    stdout: &mut dyn SchronuWriter,
    result: Result<T, ApplicationError>,
) -> Result<(), CommandError> {
    if let Err(error) = result {
        let error = CommandError::Application(error);
        render_display_model(stdout, &error_display_model(&error)).map_err(CommandError::Output)?;
    }
    Ok(())
}

#[derive(Debug)]
enum CliRepositoryTransactionError {
    Lock(StorageLockError),
    Load(TaskRepositoryError),
    Save(TaskRepositoryError),
}

impl From<TaskRepositoryError> for RunError {
    fn from(error: TaskRepositoryError) -> Self {
        Self::Repository(error)
    }
}
impl From<CommandError> for RunError {
    fn from(error: CommandError) -> Self {
        Self::Command(error)
    }
}
impl From<BusyTimeSlotLoadError> for RunError {
    fn from(error: BusyTimeSlotLoadError) -> Self {
        Self::BusyTimeSlots(error)
    }
}

impl From<CliRepositoryTransactionError> for RunError {
    fn from(error: CliRepositoryTransactionError) -> Self {
        match error {
            CliRepositoryTransactionError::Load(error) => Self::Repository(error),
            error => Self::CliRepositoryTransaction(error),
        }
    }
}

impl std::fmt::Display for CliRepositoryTransactionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lock(error) => write!(formatter, "CLI repository Lock failed: {error}"),
            Self::Load(error) => write!(formatter, "CLI repository Load failed: {error}"),
            Self::Save(error) => write!(formatter, "CLI repository Save failed: {error}"),
        }
    }
}

impl std::error::Error for CliRepositoryTransactionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lock(error) => Some(error),
            Self::Load(error) | Self::Save(error) => Some(error),
        }
    }
}

impl std::fmt::Display for RunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command(error) => error.fmt(formatter),
            Self::BusyTimeSlots(error) => error.fmt(formatter),
            Self::Repository(error) => error.fmt(formatter),
            Self::CliRepositoryTransaction(error) => error.fmt(formatter),
            Self::Snapshot(error) => error.fmt(formatter),
            Self::InteractiveIo(error) => error.fmt(formatter),
            Self::InputDisconnected {
                save_error_opt: Some(error),
            } => write!(
                formatter,
                "interactive input channel disconnected; additionally, {error}"
            ),
            Self::InputDisconnected {
                save_error_opt: None,
            } => write!(formatter, "interactive input channel disconnected"),
            Self::InputRead {
                input_error,
                save_error_opt: Some(error),
            } => write!(
                formatter,
                "failed to read interactive input: {input_error}; additionally, {error}"
            ),
            Self::InputRead {
                input_error,
                save_error_opt: None,
            } => write!(formatter, "failed to read interactive input: {input_error}"),
            Self::InputDisconnectedWithRepository { repository_error } => write!(
                formatter,
                "interactive input channel disconnected; additionally, {repository_error}"
            ),
            Self::InputReadWithRepository {
                input_error,
                repository_error,
            } => write!(
                formatter,
                "failed to read interactive input: {input_error}; additionally, {repository_error}"
            ),
            Self::Interrupted => write!(formatter, "interactive input interrupted"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Command(error) => Some(error),
            Self::BusyTimeSlots(error) => Some(error),
            Self::Repository(error) => Some(error),
            Self::CliRepositoryTransaction(error) => Some(error),
            Self::Snapshot(error) => Some(error),
            Self::InteractiveIo(error) => Some(error),
            Self::InputDisconnected { save_error_opt } => save_error_opt
                .as_ref()
                .map(|error| error as &(dyn std::error::Error + 'static)),
            Self::InputRead { input_error, .. } => Some(input_error),
            Self::InputDisconnectedWithRepository { repository_error } => Some(repository_error),
            Self::InputReadWithRepository { input_error, .. } => Some(input_error),
            Self::Interrupted => None,
        }
    }
}

fn focus_selection_mode_from_selection(selection: FocusSelection) -> FocusSelectionMode {
    match selection {
        FocusSelection::HighestPriority => FocusSelectionMode::highest_priority(),
        FocusSelection::LowestPriority { recent_days } => {
            FocusSelectionMode::lowest_priority(recent_days)
        }
    }
}

pub(super) fn select_focus_task_id(
    task_repository: &mut dyn TaskRepositoryTrait,
    focus_selection_mode: &FocusSelectionMode,
) -> Result<Option<Uuid>, ApplicationError> {
    match focus_selection_mode {
        FocusSelectionMode::HighestPriority {
            tucked_task_ids, ..
        } => Ok(get_focus_excluding(task_repository, tucked_task_ids)?.map(|task| task.id)),
        FocusSelectionMode::LowestPriority {
            recent_days,
            tucked_task_ids,
            ..
        } => {
            let now = task_repository.get_last_synced_time();
            let first_logical_date_start = try_next_logical_date_start(now)?;
            let threshold_out_of_range = || ApplicationError::LogicalDateOutOfRange {
                operation: "defer_candidate_threshold",
                datetime: now,
            };
            let recent_duration =
                Duration::try_days(*recent_days).ok_or_else(threshold_out_of_range)?;
            let recent_threshold = first_logical_date_start
                .checked_add_signed(recent_duration)
                .ok_or_else(threshold_out_of_range)?;
            task_repository
                .get_defer_candidate_leaf_task_id(recent_threshold, tucked_task_ids)
                .map_err(ApplicationError::TaskTree)
        }
    }
}

#[cfg(test)]
include!("runtime_test_support.rs");

#[cfg(test)]
#[path = "runtime_unit_tests.rs"]
mod tests;

// 文字列の中からhttpから始まる部分文字列でURLとして解釈できる一番長い文字列を抽出する
fn extract_url(s: &str) -> Option<String> {
    // "http"が始まるインデックスを探す
    if let Some(start) = s.find("http") {
        // "http"から始まる部分文字列を取得する
        let (_, http_str) = s.split_at(start);

        // 末尾の文字を必ずNGにするために、番兵として日本語の文字を置く
        let chars: Vec<char> = (http_str.to_owned() + "あ").chars().collect();

        // その中で二分探索する
        let mut ok: usize = 0;
        let mut ng: usize = chars.len();

        let mut mid = (ok + ng) / 2;

        while ng - ok > 1 {
            let cand_str: String = chars[0..mid].iter().collect();
            let encoded_cand_str: String =
                percent_encode(cand_str.as_bytes(), MY_ASCII_SET).to_string();

            // Url::parse()は未パーセントエンコーディングの文字列(日本語)も受け付けてしまう。
            // もし cand_str == encoded_cand_str なら、日本語が混ざっていないということ
            if Url::parse(&cand_str).is_ok() && cand_str == encoded_cand_str {
                ok = mid;
            } else {
                ng = mid;
            }

            mid = (ok + ng) / 2;
        }

        let ans: String = chars[0..ok].iter().collect();
        Some(ans)
    } else {
        None
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ResolvedExternalRequest {
    BrowserUrl(String),
    ObsidianUrl(String),
}

fn resolve_external_request(
    request: ExternalRequest,
    focused_task_opt: &Option<TaskHandle>,
    config: &SchronuConfig,
) -> Result<Option<ResolvedExternalRequest>, ApplicationError> {
    match request {
        ExternalRequest::OpenFocusedLink => {
            let mut task_opt = focused_task_opt.clone();
            while let Some(task) = &task_opt {
                if let Some(url) =
                    extract_url(&task.get_name().map_err(ApplicationError::TaskTree)?)
                {
                    return Ok(Some(ResolvedExternalRequest::BrowserUrl(url)));
                }
                task_opt = task.parent().map_err(ApplicationError::TaskTree)?;
            }
            Ok(None)
        }
        ExternalRequest::OpenObsidianRootSearch => focused_task_opt
            .as_ref()
            .map(|focused_task| {
                make_obsidian_root_task_search_url_with_vault(
                    focused_task,
                    &config.obsidian_vault_name,
                )
                .map(ResolvedExternalRequest::ObsidianUrl)
            })
            .transpose(),
    }
}

fn execute_open_link(url: &str) -> Result<(), CommandError> {
    webbrowser::open(url).map_err(|source| external_open_error("browser", source))?;
    Ok(())
}

fn make_obsidian_search_url_with_vault(query: &str, vault_name: &str) -> String {
    format!(
        "obsidian://search?vault={}&query={}",
        percent_encode(vault_name.as_bytes(), OBSIDIAN_VAULT_ASCII_SET),
        percent_encode(query.as_bytes(), MY_ASCII_SET)
    )
}

fn make_obsidian_root_task_search_url_with_vault(
    focused_task: &TaskHandle,
    vault_name: &str,
) -> Result<String, ApplicationError> {
    let root_task_id = focused_task
        .root()
        .and_then(|root| root.get_id())
        .map_err(ApplicationError::TaskTree)?;
    Ok(make_obsidian_search_url_with_vault(
        &root_task_id.hyphenated().to_string(),
        vault_name,
    ))
}

fn open_obsidian_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = process::Command::new("open")
            .arg(url)
            .status()
            .map_err(|err| err.to_string())?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("open exited with status {}", status))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        webbrowser::open(url).map_err(|err| err.to_string())
    }
}

fn execute_open_obsidian_root_task_search_with_config(url: &str) -> Result<(), CommandError> {
    open_obsidian_url(url)
        .map_err(|source| external_open_error("Obsidian", std::io::Error::other(source)))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn execute_parsed(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn CliRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &DateTime<Local>,
    parsed_command: &Command,
    mut application_mode: OutcomeApplicationMode<'_>,
) -> Result<(), CommandError> {
    let operation_now = task_repository.get_last_synced_time();
    let previous_focused_task_id_opt = *focused_task_id_opt;
    let was_explicit = application_mode
        .focus_selection_mode_mut()
        .is_some_and(|mode| mode.is_explicit());
    let mut next_id = Uuid::new_v4;
    let mut task_factory = TaskFactory::new(operation_now, &mut next_id);
    let mut outcome = {
        let mut context = CliCommandContext {
            task_repository,
            free_time_manager,
            focused_task_id_opt,
            task_factory: &mut task_factory,
            focus_started_datetime: *focus_started_datetime,
            config: active_config(),
        };
        handle_command(parsed_command, &mut context)?
    }
    .unwrap_or_else(|| unreachable!("Verify must be handled before command execution"));
    if let Some(requested_date) = outcome.discarded_logical_date {
        let logical_date = requested_date
            .map(Ok)
            .unwrap_or_else(|| try_logical_date(operation_now))
            .map_err(CommandError::Application)?;
        outcome.display = discarded_sessions_display(task_repository, logical_date)?;
    }
    if let Some(focus_selection_mode) = application_mode.focus_selection_mode_mut() {
        match outcome.focus_session_effect {
            FocusSessionEffect::TuckAway => {
                focus_selection_mode.set_explicit(false);
                if let Some(task_id) = previous_focused_task_id_opt {
                    focus_selection_mode.tuck_away(task_id);
                }
                *focused_task_id_opt = None;
            }
            FocusSessionEffect::RestoreSelectionIfFocusChanged
                if *focused_task_id_opt != previous_focused_task_id_opt =>
            {
                focus_selection_mode.set_explicit(false);
                if was_explicit {
                    *focused_task_id_opt = None;
                }
            }
            FocusSessionEffect::Keep | FocusSessionEffect::RestoreSelectionIfFocusChanged => {}
        }
    }
    match application_mode {
        OutcomeApplicationMode::InteractiveUnflushed(focus_selection_mode) => {
            apply_command_outcome(
                stdout,
                task_repository,
                focused_task_id_opt,
                OutcomeApplicationMode::InteractiveUnflushed(focus_selection_mode),
                outcome,
                active_config(),
            )
        }
        application_mode @ (OutcomeApplicationMode::Flushed
        | OutcomeApplicationMode::InteractiveFlushed(_)) => {
            let mut output = ErrorCapturingWriter::new(stdout);
            apply_command_outcome(
                &mut output,
                task_repository,
                focused_task_id_opt,
                application_mode,
                outcome,
                active_config(),
            )?;
            captured_output_result(&mut output)
        }
    }
}

fn captured_output_result(output: &mut ErrorCapturingWriter<'_>) -> Result<(), CommandError> {
    match output.take_error() {
        Some(error) => classify_output_error(error).map_err(CommandError::Output),
        None => Ok(()),
    }
}

fn classify_output_error(error: std::io::Error) -> Result<(), std::io::Error> {
    if error.kind() == std::io::ErrorKind::BrokenPipe {
        Ok(())
    } else {
        Err(error)
    }
}

fn classify_interactive_run_result(
    result: Result<(), interactive::DriverRunError<RunError>>,
) -> Result<(), RunError> {
    match result {
        Ok(()) => Ok(()),
        Err(interactive::DriverRunError::Io(
            error @ interactive::InteractiveIoError::RawMode(_),
        )) => Err(RunError::InteractiveIo(error)),
        Err(interactive::DriverRunError::Io(interactive::InteractiveIoError::Output(error))) => {
            classify_output_error(error).map_err(|error| {
                RunError::InteractiveIo(interactive::InteractiveIoError::Output(error))
            })
        }
        Err(interactive::DriverRunError::Handler(RunError::Command(CommandError::Output(
            error,
        )))) => classify_output_error(error)
            .map_err(|error| RunError::Command(CommandError::Output(error))),
        Err(interactive::DriverRunError::Handler(error)) => Err(error),
    }
}

#[cfg(test)]
mod interactive_output_classification_tests {
    use super::*;

    #[test]
    fn driver_outputのbroken_pipeだけを正常終了に分類する() {
        let broken_pipe = interactive::DriverRunError::Io(interactive::InteractiveIoError::Output(
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed pipe"),
        ));
        assert!(classify_interactive_run_result(Err(broken_pipe)).is_ok());

        let permission_denied = interactive::DriverRunError::Io(
            interactive::InteractiveIoError::Output(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "terminal denied output",
            )),
        );
        let error = classify_interactive_run_result(Err(permission_denied)).unwrap_err();
        assert!(matches!(
            error,
            RunError::InteractiveIo(interactive::InteractiveIoError::Output(ref source))
                if source.kind() == std::io::ErrorKind::PermissionDenied
        ));
        assert!(std::error::Error::source(&error).is_some());
    }

    #[test]
    fn runtime描画のbroken_pipeもdriver出力と同じ分類を使う() {
        let broken_pipe =
            interactive::DriverRunError::Handler(RunError::Command(CommandError::Output(
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "closed pipe"),
            )));
        assert!(classify_interactive_run_result(Err(broken_pipe)).is_ok());
    }
}

fn apply_command_outcome(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    mut application_mode: OutcomeApplicationMode<'_>,
    outcome: CommandOutcome,
    config: &SchronuConfig,
) -> Result<(), CommandError> {
    if !outcome.display.is_empty() {
        render_display_model_with_mode(stdout, &outcome.display, RenderMode::Unflushed)
            .map_err(CommandError::Output)?;
    }

    if let Some(request) = outcome.external_request {
        let focused_task_opt = match focused_task_id_opt {
            Some(task_id) => task_repository
                .get_by_id(*task_id)
                .map_err(ApplicationError::TaskTree)?,
            None => None,
        };
        if let Some(resolved_request) =
            resolve_external_request(request, &focused_task_opt, config)?
        {
            match resolved_request {
                ResolvedExternalRequest::BrowserUrl(url) => execute_open_link(&url)?,
                ResolvedExternalRequest::ObsidianUrl(url) => {
                    execute_open_obsidian_root_task_search_with_config(&url)?
                }
            }
        }
    }

    match outcome.focus_change {
        FocusChange::Keep => {}
        FocusChange::Clear => {
            *focused_task_id_opt = None;
            if let OutcomeApplicationMode::InteractiveFlushed(focus_selection_mode)
            | OutcomeApplicationMode::InteractiveUnflushed(focus_selection_mode) =
                &mut application_mode
            {
                focus_selection_mode.set_explicit(false);
            }
        }
        FocusChange::Set(task_id) => {
            *focused_task_id_opt = Some(task_id);
            if let OutcomeApplicationMode::InteractiveFlushed(focus_selection_mode)
            | OutcomeApplicationMode::InteractiveUnflushed(focus_selection_mode) =
                &mut application_mode
            {
                focus_selection_mode.set_explicit(true);
            }
        }
        FocusChange::SelectionMode(selection) => match &mut application_mode {
            OutcomeApplicationMode::InteractiveFlushed(focus_selection_mode)
            | OutcomeApplicationMode::InteractiveUnflushed(focus_selection_mode) => {
                **focus_selection_mode = focus_selection_mode_from_selection(selection);
                *focused_task_id_opt = None;
            }
            OutcomeApplicationMode::Flushed => {
                unreachable!("focus mode request must use the interactive outcome path")
            }
        },
    }

    if matches!(
        &application_mode,
        OutcomeApplicationMode::Flushed | OutcomeApplicationMode::InteractiveFlushed(_)
    ) && outcome.kind != CommandKind::Noop
    {
        render_display_model_with_mode(stdout, &DisplayModel::empty(), RenderMode::Flushed)
            .map_err(CommandError::Output)?;
    }
    Ok(())
}

enum OutcomeApplicationMode<'a> {
    Flushed,
    InteractiveFlushed(&'a mut FocusSelectionMode),
    InteractiveUnflushed(&'a mut FocusSelectionMode),
}

impl OutcomeApplicationMode<'_> {
    fn propagates_error(&self) -> bool {
        matches!(self, Self::InteractiveUnflushed(_))
    }

    fn focus_selection_mode_mut(&mut self) -> Option<&mut FocusSelectionMode> {
        match self {
            Self::InteractiveFlushed(mode) | Self::InteractiveUnflushed(mode) => Some(*mode),
            Self::Flushed => None,
        }
    }
}

// 削除できない時はNoneを返す。例えば、文字列が空の時
fn reload_repository_for_cli(
    task_repository: &mut dyn TaskRepositoryTrait,
    now: DateTime<Local>,
) -> Result<StorageLock, CliRepositoryTransactionError> {
    let storage_lock = StorageLock::acquire_with_timeout(
        task_repository.get_project_storage_dir_name().as_ref(),
        LockMode::Cli,
        CLI_LOCK_TIMEOUT,
    )
    .map_err(CliRepositoryTransactionError::Lock)?;
    task_repository
        .reload_if_changed(now)
        .map_err(CliRepositoryTransactionError::Load)?;
    Ok(storage_lock)
}

fn execute_verify_command(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    operation_now: DateTime<Local>,
) -> Result<(), RunError> {
    let _storage_lock = reload_repository_for_cli(task_repository, operation_now)?;
    render_display_model_with_mode(stdout, &verify_display_model(), RenderMode::Flushed)
        .map_err(CommandError::Output)
        .map_err(RunError::Command)
}

fn run_cli_repository_transaction<T>(
    task_repository: &mut dyn CliRepositoryTrait,
    now: DateTime<Local>,
    operation: impl FnOnce(&mut dyn CliRepositoryTrait) -> Result<T, CommandError>,
) -> Result<T, RunError> {
    let storage_directory = task_repository.get_project_storage_dir_name().to_string();
    run_repository_transaction(
        task_repository,
        now,
        || {
            StorageLock::acquire_with_timeout(
                storage_directory.as_ref(),
                LockMode::Cli,
                CLI_LOCK_TIMEOUT,
            )
        },
        |repository| {
            let output = operation(repository)?;
            let should_save = repository
                .has_pending_changes()
                .map_err(ApplicationError::TaskTree)
                .map_err(CommandError::Application)?;
            Ok::<_, CommandError>((output, should_save))
        },
    )
    .map_err(|error| match error {
        RepositoryTransactionError::Lock(error) => {
            RunError::from(CliRepositoryTransactionError::Lock(error))
        }
        RepositoryTransactionError::Load(error) => {
            RunError::from(CliRepositoryTransactionError::Load(error))
        }
        RepositoryTransactionError::Operation(error) => RunError::from(error),
        RepositoryTransactionError::SaveFailed(error)
        | RepositoryTransactionError::StateUncertain(error) => {
            RunError::from(CliRepositoryTransactionError::Save(error))
        }
    })
}

fn run_cli_repository_read_transaction<T>(
    task_repository: &mut dyn CliRepositoryTrait,
    now: DateTime<Local>,
    operation: impl FnOnce(&mut dyn CliRepositoryTrait) -> Result<T, CommandError>,
) -> Result<T, RunError> {
    let _storage_lock = reload_repository_for_cli(task_repository, now)?;
    operation(task_repository).map_err(RunError::Command)
}

fn reconcile_focus_after_reload(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focus_selection_mode: &mut FocusSelectionMode,
) -> Result<bool, ApplicationError> {
    let should_reselect = match *focused_task_id_opt {
        Some(focused_task_id) => match task_repository
            .get_by_id(focused_task_id)
            .map_err(ApplicationError::TaskTree)?
        {
            Some(focused_task) => {
                focused_task
                    .get_status()
                    .map_err(ApplicationError::TaskTree)?
                    == Status::Done
                    && !focus_selection_mode.is_explicit()
            }
            None => true,
        },
        None => true,
    };
    if !should_reselect {
        return Ok(false);
    }

    let previous_focus = *focused_task_id_opt;
    focus_selection_mode.set_explicit(false);
    *focused_task_id_opt = select_focus_task_id(task_repository, focus_selection_mode)?;
    Ok(previous_focus != *focused_task_id_opt)
}

pub(super) fn application() {
    let command_tokens_opt = parse_non_interactive_command(env::args().skip(1).collect());
    let config = match load_schronu_config(env::var_os("SCHRONU_CONFIG_PATH")) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[Error] {error}");
            process::exit(1);
        }
    };
    let _ = ACTIVE_CONFIG.set(config);
    let project_storage_directory =
        match resolve_project_storage_directory(env::var_os("SCHRONU_STORAGE_DIR")) {
            Ok(directory) => directory,
            Err(error) => {
                eprintln!("[Error] {error}");
                process::exit(1);
            }
        };
    let mut task_repository = TaskRepository::new(
        project_storage_directory
            .to_str()
            .expect("storage path was validated"),
    );
    let mut free_time_manager = FreeTimeManager::new();

    // controllerで実体を見るのを避けるために、1つ関数を切る
    let result = match command_tokens_opt {
        Some(command_tokens) => execute_non_interactive_command(
            &mut task_repository,
            &mut free_time_manager,
            &command_tokens,
        ),
        None => interactive_application(&mut task_repository, &mut free_time_manager),
    };
    if !report_run_result(&mut std::io::stderr(), result) {
        process::exit(1);
    }
}

fn report_run_result(stderr: &mut dyn Write, result: Result<(), RunError>) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => {
            if render_plain_display_model(stderr, &error_display_model(&error)).is_err() {
                return false;
            }
            false
        }
    }
}

fn parse_non_interactive_command(args: Vec<String>) -> Option<Vec<String>> {
    if args.is_empty() {
        return None;
    }

    Some(args)
}

fn execute_non_interactive_command(
    task_repository: &mut dyn CliRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    command_tokens: &[String],
) -> Result<(), RunError> {
    execute_non_interactive_command_at(
        task_repository,
        free_time_manager,
        command_tokens,
        Local::now(),
    )
}

fn execute_non_interactive_command_at(
    task_repository: &mut dyn CliRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    command_tokens: &[String],
    operation_now: DateTime<Local>,
) -> Result<(), RunError> {
    let parsed_command = parse_non_interactive_command_tokens(command_tokens)
        .map_err(map_command_parse_error)
        .map_err(RunError::Command)?;
    validate_command_input(&parsed_command)
        .map_err(CommandError::from)
        .map_err(RunError::Command)?;
    if parsed_command.kind() == CommandKind::Verify {
        let mut stdout = stdout();
        return execute_verify_command(&mut stdout, task_repository, operation_now);
    }
    if let Some(result) = storage_maintenance::execute_non_interactive(
        task_repository,
        &parsed_command,
        operation_now,
    ) {
        return result;
    }
    free_time_manager.load_busy_time_slots_from_file(
        active_config()
            .busy_time_slots_yaml_path
            .to_str()
            .expect("config path was validated"),
    )?;

    let focus_started_datetime = operation_now;
    let mut stdout = stdout();
    let execute = |task_repository: &mut dyn CliRepositoryTrait| {
        let mut focused_task_id_opt: Option<Uuid> =
            select_focus_task_id(task_repository, &FocusSelectionMode::highest_priority())?;
        execute_parsed(
            &mut stdout,
            task_repository,
            free_time_manager,
            &mut focused_task_id_opt,
            &focus_started_datetime,
            &parsed_command,
            OutcomeApplicationMode::Flushed,
        )
    };
    if parsed_command.is_read_only_repository_command() {
        run_cli_repository_read_transaction(task_repository, operation_now, execute)?;
    } else {
        run_cli_repository_transaction(task_repository, operation_now, execute)?;
    }
    Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
fn try_save_before_exit(
    stdout: &mut dyn SchronuWriter,
    task_repository: &dyn TaskRepositoryTrait,
) -> Result<bool, CommandError> {
    match task_repository.save() {
        Ok(()) => Ok(true),
        Err(error) => {
            if let Err(output_error) = render_display_model_with_mode(
                stdout,
                &error_display_model(&error),
                RenderMode::Flushed,
            ) {
                return Err(CommandError::ExitSaveDiagnostic {
                    save_error: error,
                    output_error,
                });
            }
            Ok(false)
        }
    }
}

fn handle_input_disconnected(task_repository: &dyn TaskRepositoryTrait) -> RunError {
    RunError::InputDisconnected {
        save_error_opt: task_repository.save().err(),
    }
}

fn handle_input_read_error(
    task_repository: &dyn TaskRepositoryTrait,
    input_error: std::io::Error,
) -> RunError {
    RunError::InputRead {
        input_error,
        save_error_opt: task_repository.save().err(),
    }
}

fn handle_input_disconnected_with_reload(
    task_repository: &mut dyn TaskRepositoryTrait,
) -> RunError {
    match reload_repository_for_cli(task_repository, Local::now()) {
        Ok(_storage_lock) => handle_input_disconnected(task_repository),
        Err(repository_error) => RunError::InputDisconnectedWithRepository { repository_error },
    }
}

fn handle_input_read_error_with_reload(
    task_repository: &mut dyn TaskRepositoryTrait,
    input_error: std::io::Error,
) -> RunError {
    match reload_repository_for_cli(task_repository, Local::now()) {
        Ok(_storage_lock) => handle_input_read_error(task_repository, input_error),
        Err(repository_error) => RunError::InputReadWithRepository {
            input_error,
            repository_error,
        },
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg_attr(not(test), allow(dead_code))]
fn try_exit_interactive(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    now: DateTime<Local>,
) -> Result<bool, CommandError> {
    if !try_save_before_exit(stdout, task_repository)? {
        return Ok(false);
    }
    prepare_exit_interactive(
        stdout,
        task_repository,
        free_time_manager,
        focused_task_id_opt,
        now,
    )?;
    Ok(true)
}

fn prepare_exit_interactive(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    now: DateTime<Local>,
) -> Result<(), CommandError> {
    task_repository
        .sync_clock(now)
        .map_err(ApplicationError::TaskTree)
        .map_err(CommandError::Application)?;
    render_interactive_band(
        stdout,
        focused_task_id_opt,
        task_repository,
        free_time_manager,
    )?;
    Ok(())
}

fn render_interactive_band(
    stdout: &mut dyn SchronuWriter,
    focused_task_id_opt: &mut Option<Uuid>,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<(), CommandError> {
    let result = build_show_all_tasks_display_with_config(
        focused_task_id_opt,
        task_repository,
        free_time_manager,
        &Some("帯".to_string()),
        TaskListDisplayOrder::ScheduledStartDesc,
        active_config(),
    );
    match result {
        Ok(display) => render_display_model(stdout, &display).map_err(CommandError::Output),
        Err(error) => report_application_result::<()>(stdout, Err(error)),
    }
}

fn render_focus_from_source(
    stdout: &mut dyn SchronuWriter,
    source: &dyn FocusDisplaySource,
) -> Result<(), CommandError> {
    match source.build_ancestors() {
        Ok(display) => render_display_model(stdout, &display).map_err(CommandError::Output)?,
        Err(error) => report_application_result::<()>(stdout, Err(error))?,
    }

    let Some(header) = source.build_header() else {
        return Ok(());
    };
    let header = match header {
        Ok(header) => header,
        Err(error) => {
            report_application_result::<()>(stdout, Err(error))?;
            return Ok(());
        }
    };
    render_display_model(stdout, &DisplayModel::Focus(header)).map_err(CommandError::Output)?;

    let Some(timing) = source.build_timing() else {
        return Ok(());
    };
    let timing = match timing {
        Ok(timing) => timing,
        Err(error) => {
            report_application_result::<()>(stdout, Err(error))?;
            return Ok(());
        }
    };
    render_display_model(stdout, &DisplayModel::Focus(timing)).map_err(CommandError::Output)?;
    stdout.flush().map_err(CommandError::Output)
}

fn render_focused_task(
    stdout: &mut dyn SchronuWriter,
    task_repository: &dyn TaskRepositoryTrait,
    focused_task_id_opt: Option<Uuid>,
    last_focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &mut DateTime<Local>,
    now: DateTime<Local>,
) -> Result<(), CommandError> {
    let Some(focused_task_id) = focused_task_id_opt else {
        return Ok(());
    };
    let focused_task_opt = match task_repository.get_by_id(focused_task_id) {
        Ok(task) => task,
        Err(error) => {
            report_application_result::<()>(stdout, Err(ApplicationError::TaskTree(error)))?;
            return Ok(());
        }
    };

    if focused_task_id_opt != *last_focused_task_id_opt {
        *focus_started_datetime = now;
        *last_focused_task_id_opt = focused_task_id_opt;
    }

    render_focus_from_source(
        stdout,
        &TaskFocusDisplaySource {
            focused_task_opt: focused_task_opt.as_ref(),
            focus_started_datetime,
            now,
        },
    )
}

struct FocusRenderState<'a> {
    focused_task_id_opt: &'a mut Option<Uuid>,
    last_focused_task_id_opt: &'a mut Option<Uuid>,
    focus_started_datetime: &'a mut DateTime<Local>,
}

fn render_interactive_screen(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focus_state: FocusRenderState,
    now: DateTime<Local>,
) -> Result<(), CommandError> {
    render_interactive_band(
        stdout,
        focus_state.focused_task_id_opt,
        task_repository,
        free_time_manager,
    )?;
    render_focused_task(
        stdout,
        task_repository,
        *focus_state.focused_task_id_opt,
        focus_state.last_focused_task_id_opt,
        focus_state.focus_started_datetime,
        now,
    )
}

struct InteractiveCommandExecution {
    kind: CommandKind,
}

fn command_discard_reason(kind: CommandKind) -> Option<DiscardedSessionReason> {
    match kind {
        CommandKind::Unfocus => Some(DiscardedSessionReason::CliUnfocus),
        CommandKind::TuckAway => Some(DiscardedSessionReason::CliTuckAway),
        CommandKind::Focus
        | CommandKind::Pick
        | CommandKind::Root
        | CommandKind::Parent
        | CommandKind::Children
        | CommandKind::Deepest
        | CommandKind::NextUp => Some(DiscardedSessionReason::CliFocusSwitch),
        CommandKind::FocusHighest | CommandKind::FocusLowest => {
            Some(DiscardedSessionReason::CliAutoSwitch)
        }
        CommandKind::Work | CommandKind::Finish => None,
        _ => Some(DiscardedSessionReason::CliAutoSwitch),
    }
}

fn interactive_outcome_application_mode<'a>(
    command: &Command,
    focus_selection_mode: &'a mut FocusSelectionMode,
) -> OutcomeApplicationMode<'a> {
    if matches!(
        command,
        Command::Defer { .. } | Command::InteractiveShortcut(_)
    ) || matches!(
        command.kind(),
        CommandKind::TuckAway
            | CommandKind::Unfocus
            | CommandKind::FocusHighest
            | CommandKind::FocusLowest
    ) {
        OutcomeApplicationMode::InteractiveUnflushed(focus_selection_mode)
    } else {
        OutcomeApplicationMode::InteractiveFlushed(focus_selection_mode)
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_interactive_command(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn CliRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &DateTime<Local>,
    focus_selection_mode: &mut FocusSelectionMode,
    operation_now: DateTime<Local>,
    command: &str,
) -> Result<InteractiveCommandExecution, CommandError> {
    let parsed_command = parse_interactive_command(command).map_err(map_command_parse_error)?;
    let kind = parsed_command.kind();
    let is_read_only = parsed_command.is_read_only_repository_command();
    let (command_result, propagates_error) = if kind == CommandKind::Verify {
        let mut output = ErrorCapturingWriter::new(stdout);
        (
            render_display_model_with_mode(
                &mut output,
                &DisplayModel::empty(),
                RenderMode::Flushed,
            )
            .map_err(CommandError::Output)
            .and_then(|()| captured_output_result(&mut output)),
            false,
        )
    } else {
        let application_mode =
            interactive_outcome_application_mode(&parsed_command, focus_selection_mode);
        let propagates_error = application_mode.propagates_error();
        (
            execute_parsed(
                stdout,
                task_repository,
                free_time_manager,
                focused_task_id_opt,
                focus_started_datetime,
                &parsed_command,
                application_mode,
            ),
            propagates_error,
        )
    };
    if let Err(error) = command_result {
        if propagates_error {
            return Err(error);
        }
        render_display_model(stdout, &error_display_model(&error)).map_err(CommandError::Output)?;
    }

    if !is_read_only {
        task_repository
            .sync_clock(operation_now)
            .map_err(ApplicationError::TaskTree)
            .map_err(CommandError::Application)?;
        reconcile_focus_after_reload(task_repository, focused_task_id_opt, focus_selection_mode)
            .map_err(CommandError::from)?;
    }
    Ok(InteractiveCommandExecution { kind })
}

struct InteractiveRepositoryState<'a> {
    focused_task_id_opt: &'a mut Option<Uuid>,
    last_focused_task_id_opt: &'a mut Option<Uuid>,
    focus_started_datetime: &'a mut DateTime<Local>,
    focus_selection_mode: &'a mut FocusSelectionMode,
}

enum InteractiveRepositoryEvent<'a> {
    Submit { line: &'a str },
    Refresh,
    Exit,
    InputDisconnected,
    InputRead(std::io::Error),
    Interrupted,
}

enum InteractiveRepositoryEventOutcome {
    Continue,
    CommandExecuted(CommandKind, DateTime<Local>),
    Retry(CliRepositoryTransactionError),
    Exit,
    Fatal(RunError),
}

fn handle_interactive_submit_at(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn CliRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    mut state: InteractiveRepositoryState<'_>,
    line: &str,
    operation_now: DateTime<Local>,
) -> InteractiveRepositoryEventOutcome {
    let command = line.trim().to_string();
    let (parsed_command, maintenance_kind) =
        parse_interactive_command_with_maintenance_kind(&command);
    if let Some(outcome) = storage_maintenance::execute_interactive(
        stdout,
        task_repository,
        &mut state,
        &command,
        &parsed_command,
        maintenance_kind,
        operation_now,
    ) {
        return outcome;
    }
    let original_focused_task_id = *state.focused_task_id_opt;
    let original_last_focused_task_id = *state.last_focused_task_id_opt;
    let original_focus_started = *state.focus_started_datetime;
    let original_selection_mode = state.focus_selection_mode.clone();
    let original_snapshot = match focus_snapshot(task_repository, original_focused_task_id) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return InteractiveRepositoryEventOutcome::Fatal(RunError::Command(error.into()))
        }
    };
    let auto_event_id = Uuid::new_v4();
    let command_event_id = Uuid::new_v4();
    let is_read_only = parsed_command
        .as_ref()
        .is_ok_and(Command::is_read_only_repository_command);
    let execute = |task_repository: &mut dyn CliRepositoryTrait| {
        let auto_changed = if is_read_only {
            false
        } else {
            reconcile_interactive_state_after_reload(task_repository, &mut state)?
        };
        let command_focus_started = if auto_changed {
            operation_now
        } else {
            original_focus_started
        };
        let command_old_focus = *state.focused_task_id_opt;
        let command_snapshot = focus_snapshot(task_repository, command_old_focus)?;
        writeln_newline(stdout, "").map_err(CommandError::Output)?;
        writeln_newline(
            stdout,
            &format!(
                "{}{}> {}{}",
                style::Bold,
                operation_now.format("%Y/%m/%d %H:%M:%S.%f"),
                command,
                style::Reset
            ),
        )
        .map_err(CommandError::Output)?;
        writeln_newline(stdout, "").map_err(CommandError::Output)?;
        stdout.flush().map_err(CommandError::Output)?;

        let execution = execute_interactive_command(
            stdout,
            task_repository,
            free_time_manager,
            state.focused_task_id_opt,
            &command_focus_started,
            state.focus_selection_mode,
            operation_now,
            &command,
        )?;
        if !is_read_only {
            append_focus_transition(
                task_repository,
                FocusTransition {
                    snapshot: original_snapshot.as_ref(),
                    old_focus_id: original_focused_task_id,
                    new_focus_id: command_old_focus,
                    started_at: original_focus_started,
                    ended_at: operation_now,
                    event_id: auto_event_id,
                    reason: DiscardedSessionReason::CliAutoSwitch,
                },
            )?;
            if let Some(reason) = command_discard_reason(execution.kind) {
                append_focus_transition(
                    task_repository,
                    FocusTransition {
                        snapshot: command_snapshot.as_ref(),
                        old_focus_id: command_old_focus,
                        new_focus_id: *state.focused_task_id_opt,
                        started_at: command_focus_started,
                        ended_at: operation_now,
                        event_id: command_event_id,
                        reason,
                    },
                )?;
            }
        }
        let focus_changed = original_focused_task_id != *state.focused_task_id_opt;
        if focus_changed {
            *state.last_focused_task_id_opt = None;
        }
        Ok((execution.kind, focus_changed))
    };
    let transaction_result = if is_read_only {
        run_cli_repository_read_transaction(task_repository, operation_now, execute)
    } else {
        run_cli_repository_transaction(task_repository, operation_now, execute)
    };
    match transaction_result {
        Ok((command_kind, focus_changed)) => {
            if focus_changed {
                *state.focus_started_datetime = operation_now;
            }
            InteractiveRepositoryEventOutcome::CommandExecuted(command_kind, operation_now)
        }
        Err(error @ RunError::CliRepositoryTransaction(CliRepositoryTransactionError::Save(_))) => {
            *state.focused_task_id_opt = original_focused_task_id;
            *state.last_focused_task_id_opt = original_last_focused_task_id;
            *state.focus_started_datetime = original_focus_started;
            *state.focus_selection_mode = original_selection_mode;
            InteractiveRepositoryEventOutcome::Fatal(error)
        }
        Err(RunError::CliRepositoryTransaction(error)) => {
            *state.focused_task_id_opt = original_focused_task_id;
            *state.last_focused_task_id_opt = original_last_focused_task_id;
            *state.focus_started_datetime = original_focus_started;
            *state.focus_selection_mode = original_selection_mode;
            InteractiveRepositoryEventOutcome::Retry(error)
        }
        Err(RunError::Repository(error)) => {
            *state.focused_task_id_opt = original_focused_task_id;
            *state.last_focused_task_id_opt = original_last_focused_task_id;
            *state.focus_started_datetime = original_focus_started;
            *state.focus_selection_mode = original_selection_mode;
            InteractiveRepositoryEventOutcome::Retry(CliRepositoryTransactionError::Load(error))
        }
        Err(error) => {
            *state.focused_task_id_opt = original_focused_task_id;
            *state.last_focused_task_id_opt = original_last_focused_task_id;
            *state.focus_started_datetime = original_focus_started;
            *state.focus_selection_mode = original_selection_mode;
            InteractiveRepositoryEventOutcome::Fatal(error)
        }
    }
}

fn reconcile_interactive_state_after_reload(
    task_repository: &mut dyn TaskRepositoryTrait,
    state: &mut InteractiveRepositoryState<'_>,
) -> Result<bool, ApplicationError> {
    let changed = reconcile_focus_after_reload(
        task_repository,
        state.focused_task_id_opt,
        state.focus_selection_mode,
    )?;
    if changed {
        *state.last_focused_task_id_opt = None;
    }
    Ok(changed)
}

fn handle_interactive_repository_event(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn CliRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    mut state: InteractiveRepositoryState<'_>,
    event: InteractiveRepositoryEvent<'_>,
) -> InteractiveRepositoryEventOutcome {
    match event {
        InteractiveRepositoryEvent::Submit { line } => handle_interactive_submit_at(
            stdout,
            task_repository,
            free_time_manager,
            state,
            line,
            Local::now(),
        ),
        InteractiveRepositoryEvent::Refresh => {
            let now = Local::now();
            let old_focus = *state.focused_task_id_opt;
            let old_started = *state.focus_started_datetime;
            let old_last_focus = *state.last_focused_task_id_opt;
            let old_selection_mode = state.focus_selection_mode.clone();
            let snapshot = match focus_snapshot(task_repository, old_focus) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return InteractiveRepositoryEventOutcome::Fatal(RunError::Command(
                        error.into(),
                    ))
                }
            };
            let event_id = Uuid::new_v4();
            match run_cli_repository_transaction(task_repository, now, |repository| {
                let changed = reconcile_interactive_state_after_reload(repository, &mut state)?;
                append_focus_transition(
                    repository,
                    FocusTransition {
                        snapshot: snapshot.as_ref(),
                        old_focus_id: old_focus,
                        new_focus_id: *state.focused_task_id_opt,
                        started_at: old_started,
                        ended_at: now,
                        event_id,
                        reason: DiscardedSessionReason::CliAutoSwitch,
                    },
                )?;
                Ok(changed)
            }) {
                Ok(changed) => {
                    if changed {
                        *state.focus_started_datetime = now;
                    }
                    InteractiveRepositoryEventOutcome::Continue
                }
                Err(error) => {
                    *state.focused_task_id_opt = old_focus;
                    *state.last_focused_task_id_opt = old_last_focus;
                    *state.focus_started_datetime = old_started;
                    *state.focus_selection_mode = old_selection_mode;
                    match error {
                        RunError::CliRepositoryTransaction(
                            error @ CliRepositoryTransactionError::Save(_),
                        ) => InteractiveRepositoryEventOutcome::Fatal(
                            RunError::CliRepositoryTransaction(error),
                        ),
                        RunError::CliRepositoryTransaction(error) => {
                            InteractiveRepositoryEventOutcome::Retry(error)
                        }
                        RunError::Repository(error) => InteractiveRepositoryEventOutcome::Retry(
                            CliRepositoryTransactionError::Load(error),
                        ),
                        error => InteractiveRepositoryEventOutcome::Fatal(error),
                    }
                }
            }
        }
        InteractiveRepositoryEvent::Exit => {
            if state.focus_selection_mode.pending_exit().is_none() {
                state
                    .focus_selection_mode
                    .set_pending_exit(Some(PendingExit {
                        ended_at: Local::now(),
                        auto_event_id: Uuid::new_v4(),
                        exit_event_id: Uuid::new_v4(),
                    }));
            }
            let pending_exit = state
                .focus_selection_mode
                .pending_exit()
                .expect("pending exit was initialized")
                .clone();
            let now = pending_exit.ended_at;
            let old_focus = *state.focused_task_id_opt;
            let old_started = *state.focus_started_datetime;
            let old_last_focus = *state.last_focused_task_id_opt;
            let old_selection_mode = state.focus_selection_mode.clone();
            let snapshot = match focus_snapshot(task_repository, old_focus) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return InteractiveRepositoryEventOutcome::Fatal(RunError::Command(
                        error.into(),
                    ))
                }
            };
            let auto_event_id = pending_exit.auto_event_id;
            let exit_event_id = pending_exit.exit_event_id;
            match run_cli_repository_transaction(task_repository, now, |repository| {
                let auto_changed =
                    reconcile_interactive_state_after_reload(repository, &mut state)?;
                let exit_snapshot = focus_snapshot(repository, *state.focused_task_id_opt)?;
                let exit_started = if auto_changed { now } else { old_started };
                repository
                    .sync_clock(now)
                    .map_err(ApplicationError::TaskTree)
                    .map_err(CommandError::Application)?;
                append_focus_transition(
                    repository,
                    FocusTransition {
                        snapshot: snapshot.as_ref(),
                        old_focus_id: old_focus,
                        new_focus_id: *state.focused_task_id_opt,
                        started_at: old_started,
                        ended_at: now,
                        event_id: auto_event_id,
                        reason: DiscardedSessionReason::CliAutoSwitch,
                    },
                )?;
                append_focus_transition(
                    repository,
                    FocusTransition {
                        snapshot: exit_snapshot.as_ref(),
                        old_focus_id: *state.focused_task_id_opt,
                        new_focus_id: None,
                        started_at: exit_started,
                        ended_at: now,
                        event_id: exit_event_id,
                        reason: DiscardedSessionReason::CliNormalExit,
                    },
                )?;
                Ok((true, auto_changed))
            }) {
                Ok((may_exit, auto_changed)) => {
                    state.focus_selection_mode.set_pending_exit(None);
                    if auto_changed {
                        *state.focus_started_datetime = now;
                    }
                    if let Err(error) = prepare_exit_interactive(
                        stdout,
                        task_repository,
                        free_time_manager,
                        state.focused_task_id_opt,
                        now,
                    ) {
                        return InteractiveRepositoryEventOutcome::Fatal(RunError::Command(error));
                    }
                    debug_assert!(may_exit);
                    InteractiveRepositoryEventOutcome::Exit
                }
                Err(error) => {
                    *state.focused_task_id_opt = old_focus;
                    *state.last_focused_task_id_opt = old_last_focus;
                    *state.focus_started_datetime = old_started;
                    *state.focus_selection_mode = old_selection_mode;
                    match error {
                        RunError::CliRepositoryTransaction(
                            CliRepositoryTransactionError::Save(save_error),
                        ) if save_error.save_failure_disposition()
                            == Some(crate::application::interface::TaskRepositorySaveFailureDisposition::Retryable) => {
                            InteractiveRepositoryEventOutcome::Retry(
                                CliRepositoryTransactionError::Save(save_error),
                            )
                        }
                        RunError::CliRepositoryTransaction(
                            CliRepositoryTransactionError::Save(save_error),
                        ) => match render_display_model_with_mode(
                            stdout,
                            &error_display_model(&save_error),
                            RenderMode::Flushed,
                        ) {
                            Ok(()) => InteractiveRepositoryEventOutcome::Fatal(
                                RunError::CliRepositoryTransaction(
                                    CliRepositoryTransactionError::Save(save_error),
                                ),
                            ),
                            Err(output_error) => InteractiveRepositoryEventOutcome::Fatal(
                                RunError::Command(CommandError::ExitSaveDiagnostic {
                                    save_error,
                                    output_error,
                                }),
                            ),
                        },
                        RunError::CliRepositoryTransaction(error) => {
                            InteractiveRepositoryEventOutcome::Retry(error)
                        }
                        RunError::Repository(error) => InteractiveRepositoryEventOutcome::Retry(
                            CliRepositoryTransactionError::Load(error),
                        ),
                        error => InteractiveRepositoryEventOutcome::Fatal(error),
                    }
                }
            }
        }
        InteractiveRepositoryEvent::InputDisconnected => InteractiveRepositoryEventOutcome::Fatal(
            handle_input_disconnected_with_reload(task_repository),
        ),
        InteractiveRepositoryEvent::InputRead(input_error) => {
            InteractiveRepositoryEventOutcome::Fatal(handle_input_read_error_with_reload(
                task_repository,
                input_error,
            ))
        }
        InteractiveRepositoryEvent::Interrupted => {
            InteractiveRepositoryEventOutcome::Fatal(RunError::Interrupted)
        }
    }
}

fn load_busy_time_slots_for_interactive_application(
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    busy_time_slots_file_path: &str,
) -> Result<(), RunError> {
    free_time_manager.load_busy_time_slots_from_file(busy_time_slots_file_path)?;
    Ok(())
}

struct InteractiveDriverState<'a> {
    task_repository: &'a mut dyn CliRepositoryTrait,
    free_time_manager: &'a mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &'a mut Option<Uuid>,
    last_focused_task_id_opt: &'a mut Option<Uuid>,
    focus_started_datetime: &'a mut DateTime<Local>,
    focus_selection_mode: &'a mut FocusSelectionMode,
}

fn handle_interactive_driver_event(
    stdout: &mut dyn SchronuWriter,
    state: InteractiveDriverState<'_>,
    event: interactive::DriverEvent<'_>,
) -> interactive::DriverOutcome<CliRepositoryTransactionError, RunError> {
    if let interactive::DriverEvent::RenderScreen { now } = event {
        return match render_interactive_screen(
            stdout,
            state.task_repository,
            state.free_time_manager,
            FocusRenderState {
                focused_task_id_opt: state.focused_task_id_opt,
                last_focused_task_id_opt: state.last_focused_task_id_opt,
                focus_started_datetime: state.focus_started_datetime,
            },
            now,
        ) {
            Ok(()) => interactive::DriverOutcome::Continue,
            Err(error) => interactive::DriverOutcome::Fatal(RunError::Command(error)),
        };
    }

    let repository_event = match event {
        interactive::DriverEvent::Refresh => InteractiveRepositoryEvent::Refresh,
        interactive::DriverEvent::Submit { line } => InteractiveRepositoryEvent::Submit { line },
        interactive::DriverEvent::Exit => InteractiveRepositoryEvent::Exit,
        interactive::DriverEvent::Interrupted => InteractiveRepositoryEvent::Interrupted,
        interactive::DriverEvent::InputDisconnected => {
            InteractiveRepositoryEvent::InputDisconnected
        }
        interactive::DriverEvent::InputRead(error) => InteractiveRepositoryEvent::InputRead(error),
        interactive::DriverEvent::RenderScreen { .. } => unreachable!(),
    };
    let outcome = handle_interactive_repository_event(
        stdout,
        state.task_repository,
        state.free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: state.focused_task_id_opt,
            last_focused_task_id_opt: state.last_focused_task_id_opt,
            focus_started_datetime: state.focus_started_datetime,
            focus_selection_mode: state.focus_selection_mode,
        },
        repository_event,
    );

    match outcome {
        InteractiveRepositoryEventOutcome::Continue => interactive::DriverOutcome::Continue,
        InteractiveRepositoryEventOutcome::CommandExecuted(command_kind, operation_now) => {
            if !interactive::should_suppress_leaf_tasks_after_command(command_kind) {
                let result = match build_leaf_tree_display(state.task_repository) {
                    Ok(tree) => render_display_model(stdout, &DisplayModel::Tree(tree))
                        .map_err(CommandError::Output),
                    Err(error) => report_application_result::<()>(stdout, Err(error)),
                };
                if let Err(error) = result {
                    return interactive::DriverOutcome::Fatal(RunError::Command(error));
                }
            }
            if let Err(error) = render_focused_task(
                stdout,
                state.task_repository,
                *state.focused_task_id_opt,
                state.last_focused_task_id_opt,
                state.focus_started_datetime,
                operation_now,
            ) {
                return interactive::DriverOutcome::Fatal(RunError::Command(error));
            }
            interactive::DriverOutcome::Submitted
        }
        InteractiveRepositoryEventOutcome::Retry(error) => interactive::DriverOutcome::Retry(error),
        InteractiveRepositoryEventOutcome::Exit => interactive::DriverOutcome::Exit,
        InteractiveRepositoryEventOutcome::Fatal(error) => interactive::DriverOutcome::Fatal(error),
    }
}

fn interactive_application(
    task_repository: &mut dyn CliRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<(), RunError> {
    let now = Local::now();
    drop(reload_repository_for_cli(task_repository, now)?);
    load_busy_time_slots_for_interactive_application(
        free_time_manager,
        active_config()
            .busy_time_slots_yaml_path
            .to_str()
            .expect("config path was validated"),
    )?;

    let mut focus_selection_mode = FocusSelectionMode::highest_priority();
    let mut focused_task_id_opt = select_focus_task_id(task_repository, &focus_selection_mode)
        .map_err(CommandError::from)
        .map_err(RunError::from)?;
    let mut last_focused_task_id_opt = None;
    let mut focus_started_datetime = now;

    let result = interactive::run(now, |stdout, event| {
        handle_interactive_driver_event(
            stdout,
            InteractiveDriverState {
                task_repository,
                free_time_manager,
                focused_task_id_opt: &mut focused_task_id_opt,
                last_focused_task_id_opt: &mut last_focused_task_id_opt,
                focus_started_datetime: &mut focus_started_datetime,
                focus_selection_mode: &mut focus_selection_mode,
            },
            event,
        )
    });
    classify_interactive_run_result(result)
}

#[cfg(test)]
include!("runtime_contract_tests.rs");

#[cfg(test)]
include!("interactive_io_contract_tests.rs");
