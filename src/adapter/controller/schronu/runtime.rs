#![allow(unused_must_use)]

use super::command::{parse_command, Command, CommandKind, CommandParseError, ParseMode};
use super::command_context::*;
use super::handler::{
    handle, handle_command, handle_defer_command, CommandOutcome, ExternalRequest, FocusChange,
    FocusSelection, HandlerError,
};
#[cfg(test)]
use super::handler::{
    DeferCommandContext, DeferCommandError, TaskListOrder, TaskTreeCommandContext,
};
use super::interactive;
use super::renderer::{
    render_display_model, render_plain_display_model, writeln_newline, DisplayModel,
    ErrorCapturingWriter, SchronuWriter,
};
use super::view::*;
use chrono::{DateTime, Duration, Local};
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};
#[cfg(test)]
use regex::Regex;
use schronu::adapter::gateway::free_time_manager::FreeTimeManager;
use schronu::adapter::gateway::schronu_config::{load_schronu_config, SchronuConfig};
use schronu::adapter::gateway::storage_lock::{LockMode, StorageLock, StorageLockError};
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::daily_capacity::try_next_business_day_start;
#[cfg(test)]
use schronu::application::daily_capacity::try_subjective_date_start;
use schronu::application::interface::{BusyTimeSlotLoadError, FreeTimeManagerTrait};
use schronu::application::interface::{TaskRepositoryError, TaskRepositoryTrait};
#[cfg(test)]
use schronu::application::pack_use_case::pack_tasks_with_end_of_day_offset_minutes;
use schronu::application::repository_transaction::{
    run_repository_transaction, RepositoryTransactionError,
};
use schronu::application::task_use_case::{get_focus, ApplicationError, TaskFactory};
#[cfg(test)]
use schronu::entity::task::{ProjectCategory, TaskAttr, TaskTreeError};
use schronu::entity::task::{Status, TaskHandle};
#[cfg(test)]
use std::collections::HashMap;
use std::env;
use std::io::{stdout, Write};
use std::process;
use std::sync::OnceLock;

#[path = "../storage_directory.rs"]
mod storage_directory;
use std::time::Duration as StdDuration;
use storage_directory::resolve_project_storage_directory;
use termion::style;
use url::Url;
use uuid::Uuid;

#[cfg(test)]
use chrono::NaiveDate;

const CLI_LOCK_TIMEOUT: StdDuration = StdDuration::from_secs(1);

static ACTIVE_CONFIG: OnceLock<SchronuConfig> = OnceLock::new();

pub(super) fn active_config() -> &'static SchronuConfig {
    ACTIVE_CONFIG.get_or_init(SchronuConfig::default)
}

// パーセントエンコーディングする対象にスペースを追加する
const MY_ASCII_SET: &AsciiSet = &CONTROLS.add(b' ');
const OBSIDIAN_VAULT_ASCII_SET: &AsciiSet = &MY_ASCII_SET.add(b'&').add(b'=');

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FocusSelectionMode {
    HighestPriority,
    LowestPriority { recent_days: i64 },
    Explicit,
}

#[derive(Debug)]
enum RunError {
    Command(CommandError),
    BusyTimeSlots(BusyTimeSlotLoadError),
    Repository(TaskRepositoryError),
    CliRepositoryTransaction(CliRepositoryTransactionError),
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
    Output(std::io::Error),
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
            Self::Output(error) => write!(formatter, "出力エラー: {error}"),
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
            Self::Output(error) => Some(error),
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

pub(super) fn command_parse_error(
    command: &'static str,
    field: &'static str,
    reason: &'static str,
    usage: &'static str,
) -> CommandError {
    CommandError::Parse(CommandParseError::new(command, field, reason, usage))
}

fn map_command_parse_error(error: CommandParseError) -> CommandError {
    command_parse_error(
        error.command(),
        error.field(),
        error.reason(),
        error.usage(),
    )
}

fn error_display_model(error: &impl std::fmt::Display) -> DisplayModel {
    DisplayModel::newline(format!("[Error] {error}"))
}

pub(super) fn report_application_result<T>(
    stdout: &mut dyn SchronuWriter,
    result: Result<T, ApplicationError>,
) {
    if let Err(error) = result {
        let error = CommandError::Application(error);
        let _output_error = render_display_model(stdout, &error_display_model(&error))
            .map_err(CommandError::Output);
    }
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
        FocusSelection::HighestPriority => FocusSelectionMode::HighestPriority,
        FocusSelection::LowestPriority { recent_days } => {
            FocusSelectionMode::LowestPriority { recent_days }
        }
    }
}

fn select_focus_task_id(
    task_repository: &mut dyn TaskRepositoryTrait,
    focus_selection_mode: FocusSelectionMode,
) -> Result<Option<Uuid>, ApplicationError> {
    match focus_selection_mode {
        FocusSelectionMode::HighestPriority | FocusSelectionMode::Explicit => {
            Ok(get_focus(task_repository)?.map(|task| task.id))
        }
        FocusSelectionMode::LowestPriority { recent_days } => {
            let now = task_repository.get_last_synced_time();
            let first_business_day_start = try_next_business_day_start(now)?;
            let threshold_out_of_range = || ApplicationError::SubjectiveDateOutOfRange {
                operation: "defer_candidate_threshold",
                datetime: now,
            };
            let recent_duration =
                Duration::try_days(recent_days).ok_or_else(threshold_out_of_range)?;
            let recent_threshold = first_business_day_start
                .checked_add_signed(recent_duration)
                .ok_or_else(threshold_out_of_range)?;
            task_repository
                .get_defer_candidate_leaf_task_id(recent_threshold)
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
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &DateTime<Local>,
    parsed_command: &Command,
) -> Result<(), CommandError> {
    validate_non_interactive_command(parsed_command)?;
    let operation_now = task_repository.get_last_synced_time();
    validate_contextual_task_attribute_command(parsed_command, operation_now, active_config())?;
    let mut output = ErrorCapturingWriter::new(stdout);
    let mut next_id = Uuid::new_v4;
    let mut task_factory = TaskFactory::new(operation_now, &mut next_id);
    let outcome = {
        let supports_ansi_color = output.supports_ansi_color();
        let mut context = CliCommandContext {
            task_repository,
            free_time_manager,
            focused_task_id_opt,
            task_factory: &mut task_factory,
            focus_started_datetime: *focus_started_datetime,
            config: active_config(),
            supports_ansi_color,
        };
        handle_command(parsed_command, &mut context)?
    }
    .unwrap_or_else(|| unreachable!("Verify must be handled before command execution"));
    apply_command_outcome(
        &mut output,
        task_repository,
        focused_task_id_opt,
        OutcomeApplicationMode::Flushed,
        outcome,
        active_config(),
    )?;
    captured_output_result(&mut output)
}

fn captured_output_result(output: &mut ErrorCapturingWriter<'_>) -> Result<(), CommandError> {
    match output.take_error() {
        Some(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Some(error) => Err(CommandError::Output(error)),
        None => Ok(()),
    }
}

fn render_verify_flush(stdout: &mut dyn SchronuWriter) -> Result<(), CommandError> {
    let mut output = ErrorCapturingWriter::new(stdout);
    render_display_model(&mut output, &DisplayModel::flush()).map_err(CommandError::Output)?;
    captured_output_result(&mut output)
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
        render_display_model(stdout, &outcome.display).map_err(CommandError::Output)?;
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
        FocusChange::Clear => *focused_task_id_opt = None,
        FocusChange::Set(task_id) => *focused_task_id_opt = Some(task_id),
        FocusChange::SelectionMode(selection) => match &mut application_mode {
            OutcomeApplicationMode::InteractiveUnflushed(focus_selection_mode) => {
                **focus_selection_mode = focus_selection_mode_from_selection(selection);
                *focused_task_id_opt = None;
            }
            OutcomeApplicationMode::Flushed => {
                unreachable!("focus mode request must use the interactive outcome path")
            }
        },
    }

    if matches!(&application_mode, OutcomeApplicationMode::Flushed)
        && outcome.kind != CommandKind::Noop
    {
        render_display_model(stdout, &DisplayModel::flush()).map_err(CommandError::Output)?;
    }
    Ok(())
}

enum OutcomeApplicationMode<'a> {
    Flushed,
    InteractiveUnflushed(&'a mut FocusSelectionMode),
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

fn run_cli_repository_transaction<T>(
    task_repository: &mut dyn TaskRepositoryTrait,
    now: DateTime<Local>,
    operation: impl FnOnce(&mut dyn TaskRepositoryTrait) -> Result<T, CommandError>,
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
        RepositoryTransactionError::StateUncertain(error) => {
            RunError::from(CliRepositoryTransactionError::Save(error))
        }
    })
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
                    && *focus_selection_mode != FocusSelectionMode::Explicit
            }
            None => true,
        },
        None => true,
    };
    if !should_reselect {
        return Ok(false);
    }

    let previous_focus = *focused_task_id_opt;
    if *focus_selection_mode == FocusSelectionMode::Explicit {
        *focus_selection_mode = FocusSelectionMode::HighestPriority;
    }
    *focused_task_id_opt = select_focus_task_id(task_repository, *focus_selection_mode)?;
    Ok(previous_focus != *focused_task_id_opt)
}

pub(super) fn application() {
    let command_opt = parse_non_interactive_command(env::args().skip(1).collect());
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
    let result = match command_opt {
        Some(command) => {
            execute_non_interactive_command(&mut task_repository, &mut free_time_manager, &command)
        }
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
            render_plain_display_model(stderr, &error_display_model(&error)).unwrap();
            false
        }
    }
}

fn parse_non_interactive_command(args: Vec<String>) -> Option<String> {
    if args.is_empty() {
        return None;
    }

    Some(args.join(" "))
}

fn execute_non_interactive_command(
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    command: &str,
) -> Result<(), RunError> {
    execute_non_interactive_command_at(task_repository, free_time_manager, command, Local::now())
}

fn execute_non_interactive_command_at(
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    command: &str,
    operation_now: DateTime<Local>,
) -> Result<(), RunError> {
    let parsed_command = parse_command(command, ParseMode::NonInteractive)
        .map_err(map_command_parse_error)
        .map_err(RunError::Command)?;
    validate_non_interactive_command(&parsed_command).map_err(RunError::Command)?;
    if parsed_command.kind() == CommandKind::Verify {
        let _storage_lock = reload_repository_for_cli(task_repository, operation_now)?;
        println!("検証: OK");
        return Ok(());
    }
    free_time_manager.load_busy_time_slots_from_file(
        active_config()
            .busy_time_slots_yaml_path
            .to_str()
            .expect("config path was validated"),
    )?;

    let focus_started_datetime = operation_now;
    let mut stdout = stdout();
    run_cli_repository_transaction(task_repository, operation_now, |task_repository| {
        let mut focused_task_id_opt: Option<Uuid> =
            select_focus_task_id(task_repository, FocusSelectionMode::HighestPriority)?;
        execute_parsed(
            &mut stdout,
            task_repository,
            free_time_manager,
            &mut focused_task_id_opt,
            &focus_started_datetime,
            &parsed_command,
        )
    })?;
    Ok(())
}

fn try_save_before_exit(
    stdout: &mut dyn SchronuWriter,
    task_repository: &dyn TaskRepositoryTrait,
) -> bool {
    match task_repository.save() {
        Ok(()) => true,
        Err(error) => {
            writeln_newline(stdout, &format!("[Error] {error}")).unwrap();
            stdout.flush().unwrap();
            false
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
fn try_exit_interactive(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    now: DateTime<Local>,
) -> bool {
    if !try_save_before_exit(stdout, task_repository) {
        return false;
    }

    task_repository.sync_clock(now);
    let result = execute_show_all_tasks_with_config(
        stdout,
        focused_task_id_opt,
        task_repository,
        free_time_manager,
        &Some("帯".to_string()),
        TaskListDisplayOrder::ScheduledStartDesc,
        active_config(),
    );
    report_application_result(stdout, result);
    true
}

fn render_focused_task(
    stdout: &mut dyn SchronuWriter,
    task_repository: &dyn TaskRepositoryTrait,
    focused_task_id_opt: Option<Uuid>,
    last_focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &mut DateTime<Local>,
    now: DateTime<Local>,
) {
    let Some(focused_task_id) = focused_task_id_opt else {
        return;
    };
    let focused_task_opt = match task_repository.get_by_id(focused_task_id) {
        Ok(task) => task,
        Err(error) => {
            report_application_result::<()>(stdout, Err(ApplicationError::TaskTree(error)));
            return;
        }
    };

    if focused_task_id_opt != *last_focused_task_id_opt {
        *focus_started_datetime = now;
        *last_focused_task_id_opt = focused_task_id_opt;
    }

    let result = execute_show_ancestor(stdout, &focused_task_opt);
    report_application_result(stdout, result);

    if let Some(focused_task) = focused_task_opt {
        let project_category_opt = match focused_task.get_project_category_opt() {
            Ok(project_category_opt) => project_category_opt,
            Err(error) => {
                report_application_result::<()>(stdout, Err(ApplicationError::TaskTree(error)));
                return;
            }
        };
        writeln_newline(stdout, &format_focused_task_header(project_category_opt)).unwrap();
        writeln_newline(stdout, &format!("{:?}", focused_task.get_attr())).unwrap();

        let messages = match make_messages_about_focus(&focused_task, focus_started_datetime, &now)
        {
            Ok(messages) => messages,
            Err(error) => {
                report_application_result::<()>(stdout, Err(error));
                return;
            }
        };
        for message in messages {
            writeln_newline(stdout, &message).unwrap();
        }
        stdout.flush().unwrap();
    }
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
) {
    let result = execute_show_all_tasks_with_config(
        stdout,
        focus_state.focused_task_id_opt,
        task_repository,
        free_time_manager,
        &Some("帯".to_string()),
        TaskListDisplayOrder::ScheduledStartDesc,
        active_config(),
    );
    report_application_result(stdout, result);
    render_focused_task(
        stdout,
        task_repository,
        *focus_state.focused_task_id_opt,
        focus_state.last_focused_task_id_opt,
        focus_state.focus_started_datetime,
        now,
    );
}

fn should_suppress_leaf_tasks_after_command(line: &str) -> bool {
    matches!(
        line.chars().next(),
        Some('新')
            | Some('突')
            | Some('全')
            | Some('尾')
            | Some('今')
            | Some('明')
            | Some('近')
            | Some('週')
            | Some('末')
            | Some('翌')
            | Some('暦')
            | Some('帯')
            | Some('平')
            | Some('詰')
            | Some('葉')
            | Some('樹')
            | Some('清')
    ) || matches!(line.split_whitespace().next(), Some("band" | "pack"))
}

#[allow(clippy::too_many_arguments, unused_must_use)]
fn execute_interactive_command(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &DateTime<Local>,
    focus_selection_mode: &mut FocusSelectionMode,
    operation_now: DateTime<Local>,
    command: &str,
) -> Result<bool, CommandError> {
    let parsed_command =
        parse_command(command, ParseMode::Interactive).map_err(map_command_parse_error)?;
    if let Some(outcome) =
        handle(&parsed_command).filter(|outcome| outcome.focus_change != FocusChange::Keep)
    {
        apply_command_outcome(
            stdout,
            task_repository,
            focused_task_id_opt,
            OutcomeApplicationMode::InteractiveUnflushed(focus_selection_mode),
            outcome,
            active_config(),
        )?;
    } else if matches!(
        parsed_command,
        Command::Defer { .. } | Command::InteractiveShortcut(_)
    ) {
        let mut context = RuntimeDeferCommandContext {
            task_repository,
            focused_task_id_opt,
            config: active_config(),
        };
        let outcome = handle_defer_command(&parsed_command, &mut context)?
            .expect("interactive defer command must be handler-owned");
        apply_command_outcome(
            stdout,
            task_repository,
            focused_task_id_opt,
            OutcomeApplicationMode::InteractiveUnflushed(focus_selection_mode),
            outcome,
            active_config(),
        )?;
    } else {
        let command_result = if parsed_command.kind() == CommandKind::Verify {
            render_verify_flush(stdout)
        } else {
            execute_parsed(
                stdout,
                task_repository,
                free_time_manager,
                focused_task_id_opt,
                focus_started_datetime,
                &parsed_command,
            )
        };
        if let Err(error) = command_result {
            let _output_error = render_display_model(stdout, &error_display_model(&error))
                .map_err(CommandError::Output);
        }
        if matches!(parsed_command, Command::Focus { task_id } if *focused_task_id_opt == Some(task_id))
        {
            *focus_selection_mode = FocusSelectionMode::Explicit;
        }
    }

    task_repository.sync_clock(operation_now);
    reconcile_focus_after_reload(task_repository, focused_task_id_opt, focus_selection_mode)
        .map_err(CommandError::from)
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
    CommandExecuted(String, DateTime<Local>),
    Retry(CliRepositoryTransactionError),
    Exit,
    Fatal(RunError),
}

fn handle_interactive_submit_at(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    mut state: InteractiveRepositoryState<'_>,
    line: &str,
    operation_now: DateTime<Local>,
) -> InteractiveRepositoryEventOutcome {
    let command = line.trim().to_string();
    let transaction_result =
        run_cli_repository_transaction(task_repository, operation_now, |task_repository| {
            reconcile_interactive_state_after_reload(task_repository, &mut state, operation_now)?;
            writeln_newline(stdout, "").unwrap();
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
            .unwrap();
            writeln_newline(stdout, "").unwrap();
            stdout.flush().unwrap();

            if execute_interactive_command(
                stdout,
                task_repository,
                free_time_manager,
                state.focused_task_id_opt,
                state.focus_started_datetime,
                state.focus_selection_mode,
                operation_now,
                &command,
            )? {
                *state.last_focused_task_id_opt = None;
            }
            Ok(())
        });
    match transaction_result {
        Ok(()) => InteractiveRepositoryEventOutcome::CommandExecuted(command, operation_now),
        Err(error @ RunError::CliRepositoryTransaction(CliRepositoryTransactionError::Save(_))) => {
            InteractiveRepositoryEventOutcome::Fatal(error)
        }
        Err(RunError::CliRepositoryTransaction(error)) => {
            InteractiveRepositoryEventOutcome::Retry(error)
        }
        Err(RunError::Repository(error)) => {
            InteractiveRepositoryEventOutcome::Retry(CliRepositoryTransactionError::Load(error))
        }
        Err(error) => InteractiveRepositoryEventOutcome::Fatal(error),
    }
}

fn reconcile_interactive_state_after_reload(
    task_repository: &mut dyn TaskRepositoryTrait,
    state: &mut InteractiveRepositoryState<'_>,
    now: DateTime<Local>,
) -> Result<(), ApplicationError> {
    if reconcile_focus_after_reload(
        task_repository,
        state.focused_task_id_opt,
        state.focus_selection_mode,
    )? {
        *state.last_focused_task_id_opt = None;
        *state.focus_started_datetime = now;
    }
    Ok(())
}

fn handle_interactive_repository_event(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
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
            match reload_repository_for_cli(task_repository, now) {
                Ok(storage_lock) => {
                    reconcile_interactive_state_after_reload(task_repository, &mut state, now);
                    drop(storage_lock);
                    InteractiveRepositoryEventOutcome::Continue
                }
                Err(error) => InteractiveRepositoryEventOutcome::Retry(error),
            }
        }
        InteractiveRepositoryEvent::Exit => {
            let now = Local::now();
            match reload_repository_for_cli(task_repository, now) {
                Ok(_storage_lock) => {
                    reconcile_interactive_state_after_reload(task_repository, &mut state, now);
                    if try_exit_interactive(
                        stdout,
                        task_repository,
                        free_time_manager,
                        state.focused_task_id_opt,
                        now,
                    ) {
                        InteractiveRepositoryEventOutcome::Exit
                    } else {
                        InteractiveRepositoryEventOutcome::Continue
                    }
                }
                Err(error) => InteractiveRepositoryEventOutcome::Retry(error),
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

fn interactive_application(
    task_repository: &mut dyn TaskRepositoryTrait,
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

    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;
    let mut focused_task_id_opt = select_focus_task_id(task_repository, focus_selection_mode)
        .map_err(CommandError::from)
        .map_err(RunError::from)?;
    let mut last_focused_task_id_opt = None;
    let mut focus_started_datetime = now;

    interactive::run(now, |stdout, event| {
        if let interactive::DriverEvent::RenderScreen { now } = event {
            render_interactive_screen(
                stdout,
                task_repository,
                free_time_manager,
                FocusRenderState {
                    focused_task_id_opt: &mut focused_task_id_opt,
                    last_focused_task_id_opt: &mut last_focused_task_id_opt,
                    focus_started_datetime: &mut focus_started_datetime,
                },
                now,
            );
            return interactive::DriverOutcome::Continue;
        }

        let repository_event = match event {
            interactive::DriverEvent::Refresh => InteractiveRepositoryEvent::Refresh,
            interactive::DriverEvent::Submit { line } => {
                InteractiveRepositoryEvent::Submit { line }
            }
            interactive::DriverEvent::Exit => InteractiveRepositoryEvent::Exit,
            interactive::DriverEvent::Interrupted => InteractiveRepositoryEvent::Interrupted,
            interactive::DriverEvent::InputDisconnected => {
                InteractiveRepositoryEvent::InputDisconnected
            }
            interactive::DriverEvent::InputRead(error) => {
                InteractiveRepositoryEvent::InputRead(error)
            }
            interactive::DriverEvent::RenderScreen { .. } => unreachable!(),
        };
        let outcome = handle_interactive_repository_event(
            stdout,
            task_repository,
            free_time_manager,
            InteractiveRepositoryState {
                focused_task_id_opt: &mut focused_task_id_opt,
                last_focused_task_id_opt: &mut last_focused_task_id_opt,
                focus_started_datetime: &mut focus_started_datetime,
                focus_selection_mode: &mut focus_selection_mode,
            },
            repository_event,
        );

        match outcome {
            InteractiveRepositoryEventOutcome::Continue => interactive::DriverOutcome::Continue,
            InteractiveRepositoryEventOutcome::CommandExecuted(command, operation_now) => {
                if !should_suppress_leaf_tasks_after_command(&command) {
                    let result =
                        execute_show_leaf_tasks(stdout, task_repository, free_time_manager);
                    report_application_result(stdout, result);
                }
                render_focused_task(
                    stdout,
                    task_repository,
                    focused_task_id_opt,
                    &mut last_focused_task_id_opt,
                    &mut focus_started_datetime,
                    operation_now,
                );
                interactive::DriverOutcome::Submitted
            }
            InteractiveRepositoryEventOutcome::Retry(error) => {
                interactive::DriverOutcome::Retry(error)
            }
            InteractiveRepositoryEventOutcome::Exit => interactive::DriverOutcome::Exit,
            InteractiveRepositoryEventOutcome::Fatal(error) => {
                interactive::DriverOutcome::Fatal(error)
            }
        }
    })
}

#[cfg(test)]
include!("runtime_contract_tests.rs");
