use std::io::stdout;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use termion::style;

use super::{
    append_focus_transition, error_display_model, focus_snapshot, map_command_parse_error,
    reconcile_interactive_state_after_reload, CliRepositoryTrait, CliRepositoryTransactionError,
    CommandError, InteractiveRepositoryEventOutcome, InteractiveRepositoryState, RunError,
    CLI_LOCK_TIMEOUT,
};
use crate::adapter::controller::command::{Command, CommandKind, CommandParseError};
use crate::adapter::controller::renderer::{
    render_display_model, render_display_model_with_mode, writeln_newline, RenderMode,
    SchronuWriter,
};
use crate::adapter::controller::view::{
    backup_display, backup_verify_display, restore_current_display, restore_display,
};
use crate::adapter::gateway::storage_lock::{LockMode, StorageLock};
use crate::adapter::gateway::storage_snapshot::{
    create_snapshot_with_lock, restore_current_snapshot, restore_snapshot_to_alternate,
    verify_snapshot,
};
use crate::application::interface::{TaskRepositorySaveFailureDisposition, TaskRepositoryTrait};
use crate::entity::discarded_session::DiscardedSessionReason;
use uuid::Uuid;

pub(super) fn execute_non_interactive(
    task_repository: &mut dyn TaskRepositoryTrait,
    command: &Command,
    operation_now: DateTime<Local>,
) -> Option<Result<(), RunError>> {
    let result = match command {
        Command::Backup { snapshot_directory } => execute_backup_command(
            &mut stdout(),
            task_repository,
            snapshot_directory,
            operation_now,
        ),
        Command::BackupVerify { snapshot_directory } => {
            execute_backup_verify_command(&mut stdout(), snapshot_directory)
        }
        Command::Restore {
            snapshot_directory,
            destination_directory,
        } => execute_restore_command(
            &mut stdout(),
            snapshot_directory,
            destination_directory,
            Path::new(task_repository.get_project_storage_dir_name()),
        ),
        Command::RestoreCurrent {
            snapshot_directory,
            pre_backup_directory,
        } => execute_restore_current_command(
            &mut stdout(),
            task_repository,
            snapshot_directory,
            pre_backup_directory,
            operation_now,
        ),
        _ => return None,
    };
    Some(result)
}

pub(super) fn execute_interactive(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn CliRepositoryTrait,
    state: &mut InteractiveRepositoryState<'_>,
    command: &str,
    parsed_command: &Result<Command, CommandParseError>,
    maintenance_kind: Option<CommandKind>,
    operation_now: DateTime<Local>,
) -> Option<InteractiveRepositoryEventOutcome> {
    if matches!(
        maintenance_kind,
        Some(
            CommandKind::Backup
                | CommandKind::BackupVerify
                | CommandKind::Restore
                | CommandKind::RestoreCurrent
        )
    ) && state.focus_selection_mode.session_snapshot().is_none()
    {
        let snapshot = match focus_snapshot(task_repository, *state.focused_task_id_opt) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return Some(InteractiveRepositoryEventOutcome::Fatal(RunError::Command(
                    error.into(),
                )))
            }
        };
        state.focus_selection_mode.set_session_snapshot(snapshot);
    }
    if let Err(error) = parsed_command {
        if let Some(command_kind) = maintenance_kind {
            if let Err(output_error) =
                render_interactive_command_echo(stdout, command, operation_now).and_then(|()| {
                    render_display_model(
                        stdout,
                        &error_display_model(&map_command_parse_error(error.clone())),
                    )
                    .map_err(CommandError::Output)
                    .map_err(RunError::Command)
                })
            {
                return Some(InteractiveRepositoryEventOutcome::Fatal(output_error));
            }
            return Some(InteractiveRepositoryEventOutcome::CommandExecuted(
                command_kind,
                operation_now,
            ));
        }
    }
    if let Ok(Command::BackupVerify { snapshot_directory }) = parsed_command {
        if let Err(error) = render_interactive_command_echo(stdout, command, operation_now) {
            return Some(InteractiveRepositoryEventOutcome::Fatal(error));
        }
        return Some(
            match execute_backup_verify_command(stdout, snapshot_directory) {
                Ok(()) => InteractiveRepositoryEventOutcome::CommandExecuted(
                    CommandKind::BackupVerify,
                    operation_now,
                ),
                Err(error) => InteractiveRepositoryEventOutcome::Fatal(error),
            },
        );
    }
    if let Ok(Command::Restore {
        snapshot_directory,
        destination_directory,
    }) = parsed_command
    {
        if let Err(error) = render_interactive_command_echo(stdout, command, operation_now) {
            return Some(InteractiveRepositoryEventOutcome::Fatal(error));
        }
        return Some(
            match execute_restore_command(
                stdout,
                snapshot_directory,
                destination_directory,
                Path::new(task_repository.get_project_storage_dir_name()),
            ) {
                Ok(()) => InteractiveRepositoryEventOutcome::CommandExecuted(
                    CommandKind::Restore,
                    operation_now,
                ),
                Err(error) => InteractiveRepositoryEventOutcome::Fatal(error),
            },
        );
    }
    if let Ok(Command::RestoreCurrent {
        snapshot_directory,
        pre_backup_directory,
    }) = parsed_command
    {
        if let Err(error) = render_interactive_command_echo(stdout, command, operation_now) {
            return Some(InteractiveRepositoryEventOutcome::Fatal(error));
        }
        let storage_directory = PathBuf::from(task_repository.get_project_storage_dir_name());
        let result = (|| {
            let storage_lock = StorageLock::acquire_with_timeout(
                &storage_directory,
                LockMode::Cli,
                CLI_LOCK_TIMEOUT,
            )
            .map_err(CliRepositoryTransactionError::Lock)
            .map_err(RunError::CliRepositoryTransaction)?;
            let summary = restore_current_snapshot(
                &storage_directory,
                snapshot_directory,
                pre_backup_directory,
                &storage_lock,
            )
            .map_err(RunError::Snapshot)?;
            if let Err(error) =
                record_maintenance_auto_switch(task_repository, state, operation_now)
            {
                // restoreと後続journal saveは別commitなので、後続失敗時はpre-backupから補償復元する。
                rollback_restore_current(
                    task_repository,
                    &storage_directory,
                    pre_backup_directory,
                    &storage_lock,
                )?;
                return Err(error);
            }
            render_display_model_with_mode(
                stdout,
                &restore_current_display(&storage_directory, &summary),
                RenderMode::Flushed,
            )
            .map_err(CommandError::Output)
            .map_err(RunError::Command)
        })();
        return Some(maintenance_outcome(
            result,
            CommandKind::RestoreCurrent,
            operation_now,
        ));
    }
    if let Ok(Command::Backup { snapshot_directory }) = parsed_command {
        if let Err(error) = render_interactive_command_echo(stdout, command, operation_now) {
            return Some(InteractiveRepositoryEventOutcome::Fatal(error));
        }
        let storage_directory = PathBuf::from(task_repository.get_project_storage_dir_name());
        let storage_lock = match StorageLock::acquire_with_timeout(
            &storage_directory,
            LockMode::Cli,
            CLI_LOCK_TIMEOUT,
        ) {
            Ok(storage_lock) => storage_lock,
            Err(error) => {
                return Some(InteractiveRepositoryEventOutcome::Retry(
                    CliRepositoryTransactionError::Lock(error),
                ));
            }
        };
        // snapshot作成とrepository commitは1つのfilesystem transactionにできない。
        // Retryを安全にするため、focus/journalを先にcommitしてから非冪等なsnapshotを作る。
        if let Err(error) = record_maintenance_auto_switch(task_repository, state, operation_now) {
            return Some(maintenance_outcome(
                Err(error),
                CommandKind::Backup,
                operation_now,
            ));
        }
        return Some(
            match execute_backup_command_with_lock(
                stdout,
                snapshot_directory,
                operation_now,
                &storage_directory,
                &storage_lock,
            ) {
                Ok(()) => InteractiveRepositoryEventOutcome::CommandExecuted(
                    CommandKind::Backup,
                    operation_now,
                ),
                Err(RunError::CliRepositoryTransaction(error)) => {
                    InteractiveRepositoryEventOutcome::Retry(error)
                }
                Err(error) => InteractiveRepositoryEventOutcome::Fatal(error),
            },
        );
    }
    None
}

pub(super) fn rollback_restore_current(
    task_repository: &mut dyn CliRepositoryTrait,
    storage_directory: &Path,
    pre_backup_directory: &Path,
    storage_lock: &StorageLock,
) -> Result<(), RunError> {
    let rollback_backup_directory = pre_backup_directory.with_file_name(format!(
        ".schronu-failed-restore-{}",
        Uuid::new_v4().hyphenated()
    ));
    restore_current_snapshot(
        storage_directory,
        pre_backup_directory,
        &rollback_backup_directory,
        storage_lock,
    )
    .map_err(RunError::Snapshot)?;
    task_repository
        .load()
        .map_err(CliRepositoryTransactionError::Load)
        .map_err(RunError::CliRepositoryTransaction)?;
    for cleanup_directory in [&rollback_backup_directory, pre_backup_directory] {
        if !cleanup_directory.is_dir() {
            continue;
        }
        std::fs::remove_dir_all(cleanup_directory)
            .map_err(|error| {
                crate::application::interface::TaskRepositoryError::new(
                    crate::application::interface::TaskRepositoryOperation::Load,
                    error,
                )
            })
            .map_err(CliRepositoryTransactionError::Load)
            .map_err(RunError::CliRepositoryTransaction)?;
    }
    Ok(())
}

fn maintenance_outcome(
    result: Result<(), RunError>,
    command_kind: CommandKind,
    operation_now: DateTime<Local>,
) -> InteractiveRepositoryEventOutcome {
    match result {
        Ok(()) => InteractiveRepositoryEventOutcome::CommandExecuted(command_kind, operation_now),
        Err(RunError::CliRepositoryTransaction(error)) => {
            if matches!(
                &error,
                CliRepositoryTransactionError::Save(save_error)
                    if save_error.save_failure_disposition()
                        != Some(TaskRepositorySaveFailureDisposition::Retryable)
            ) {
                InteractiveRepositoryEventOutcome::Fatal(RunError::CliRepositoryTransaction(error))
            } else {
                InteractiveRepositoryEventOutcome::Retry(error)
            }
        }
        Err(RunError::Repository(error)) => {
            InteractiveRepositoryEventOutcome::Retry(CliRepositoryTransactionError::Load(error))
        }
        Err(error) => InteractiveRepositoryEventOutcome::Fatal(error),
    }
}

pub(super) fn record_maintenance_auto_switch(
    task_repository: &mut dyn CliRepositoryTrait,
    state: &mut InteractiveRepositoryState<'_>,
    operation_now: DateTime<Local>,
) -> Result<(), RunError> {
    let old_focus = *state.focused_task_id_opt;
    let old_last_focus = *state.last_focused_task_id_opt;
    let old_started = *state.focus_started_datetime;
    let old_selection_mode = state.focus_selection_mode.clone();
    let snapshot = state.focus_selection_mode.session_snapshot().cloned();
    let event_id = Uuid::new_v4();
    let result = (|| {
        task_repository
            .reload_if_changed(operation_now)
            .map_err(CliRepositoryTransactionError::Load)
            .map_err(RunError::CliRepositoryTransaction)?;
        let changed = reconcile_interactive_state_after_reload(task_repository, state)
            .map_err(CommandError::Application)
            .map_err(RunError::Command)?;
        append_focus_transition(
            task_repository,
            super::FocusTransition {
                snapshot: snapshot.as_ref(),
                old_focus_id: old_focus,
                new_focus_id: *state.focused_task_id_opt,
                started_at: old_started,
                ended_at: operation_now,
                event_id,
                reason: DiscardedSessionReason::CliAutoSwitch,
            },
        )
        .map_err(CommandError::from)
        .map_err(RunError::Command)?;
        let next_snapshot = if changed {
            focus_snapshot(task_repository, *state.focused_task_id_opt)
                .map_err(CommandError::from)
                .map_err(RunError::Command)?
        } else {
            snapshot.clone()
        };
        let should_save = task_repository
            .has_pending_changes()
            .map_err(crate::application::task_use_case::ApplicationError::TaskTree)
            .map_err(CommandError::Application)
            .map_err(RunError::Command)?;
        if should_save {
            task_repository
                .save()
                .map_err(CliRepositoryTransactionError::Save)
                .map_err(RunError::CliRepositoryTransaction)?;
        }
        Ok::<_, RunError>((changed, next_snapshot))
    })();
    match result {
        Ok((changed, next_snapshot)) => {
            if changed {
                *state.focus_started_datetime = operation_now;
            }
            state
                .focus_selection_mode
                .set_session_snapshot(next_snapshot);
            Ok(())
        }
        Err(error) => {
            *state.focused_task_id_opt = old_focus;
            *state.last_focused_task_id_opt = old_last_focus;
            *state.focus_started_datetime = old_started;
            *state.focus_selection_mode = old_selection_mode;
            if matches!(
                &error,
                RunError::CliRepositoryTransaction(CliRepositoryTransactionError::Save(
                    save_error
                )) if save_error.save_failure_disposition()
                    == Some(TaskRepositorySaveFailureDisposition::Retryable)
            ) {
                task_repository
                    .load()
                    .map_err(CliRepositoryTransactionError::Load)
                    .map_err(RunError::CliRepositoryTransaction)?;
            }
            Err(error)
        }
    }
}

fn render_interactive_command_echo(
    stdout: &mut dyn SchronuWriter,
    command: &str,
    operation_now: DateTime<Local>,
) -> Result<(), RunError> {
    writeln_newline(stdout, "")
        .and_then(|()| {
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
        })
        .and_then(|()| writeln_newline(stdout, ""))
        .and_then(|()| stdout.flush())
        .map_err(CommandError::Output)
        .map_err(RunError::Command)
}

fn execute_backup_command(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    snapshot_directory: &Path,
    operation_now: DateTime<Local>,
) -> Result<(), RunError> {
    let storage_directory = PathBuf::from(task_repository.get_project_storage_dir_name());
    let storage_lock =
        StorageLock::acquire_with_timeout(&storage_directory, LockMode::Cli, CLI_LOCK_TIMEOUT)
            .map_err(CliRepositoryTransactionError::Lock)?;
    execute_backup_command_with_lock(
        stdout,
        snapshot_directory,
        operation_now,
        &storage_directory,
        &storage_lock,
    )
}

fn execute_backup_verify_command(
    stdout: &mut dyn SchronuWriter,
    snapshot_directory: &Path,
) -> Result<(), RunError> {
    let summary = verify_snapshot(snapshot_directory).map_err(RunError::Snapshot)?;
    render_display_model_with_mode(
        stdout,
        &backup_verify_display(snapshot_directory, &summary),
        RenderMode::Flushed,
    )
    .map_err(CommandError::Output)
    .map_err(RunError::Command)
}

fn execute_restore_command(
    stdout: &mut dyn SchronuWriter,
    snapshot_directory: &Path,
    destination_directory: &Path,
    current_storage_directory: &Path,
) -> Result<(), RunError> {
    let summary = restore_snapshot_to_alternate(
        snapshot_directory,
        destination_directory,
        current_storage_directory,
    )
    .map_err(RunError::Snapshot)?;
    render_display_model_with_mode(
        stdout,
        &restore_display(destination_directory, &summary),
        RenderMode::Flushed,
    )
    .map_err(CommandError::Output)
    .map_err(RunError::Command)
}

fn execute_restore_current_command(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    snapshot_directory: &Path,
    pre_backup_directory: &Path,
    operation_now: DateTime<Local>,
) -> Result<(), RunError> {
    let storage_directory = PathBuf::from(task_repository.get_project_storage_dir_name());
    let storage_lock =
        StorageLock::acquire_with_timeout(&storage_directory, LockMode::Cli, CLI_LOCK_TIMEOUT)
            .map_err(CliRepositoryTransactionError::Lock)?;
    let summary = restore_current_snapshot(
        &storage_directory,
        snapshot_directory,
        pre_backup_directory,
        &storage_lock,
    )
    .map_err(RunError::Snapshot)?;
    task_repository
        .reload_if_changed(operation_now)
        .map_err(CliRepositoryTransactionError::Load)?;
    render_display_model_with_mode(
        stdout,
        &restore_current_display(&storage_directory, &summary),
        RenderMode::Flushed,
    )
    .map_err(CommandError::Output)
    .map_err(RunError::Command)
}

fn execute_backup_command_with_lock(
    stdout: &mut dyn SchronuWriter,
    snapshot_directory: &Path,
    operation_now: DateTime<Local>,
    storage_directory: &Path,
    storage_lock: &StorageLock,
) -> Result<(), RunError> {
    let summary = create_snapshot_with_lock(
        storage_directory,
        snapshot_directory,
        operation_now,
        storage_lock,
    )
    .map_err(RunError::Snapshot)?;
    render_display_model_with_mode(
        stdout,
        &backup_display(snapshot_directory, &summary),
        RenderMode::Flushed,
    )
    .map_err(CommandError::Output)
    .map_err(RunError::Command)
}
