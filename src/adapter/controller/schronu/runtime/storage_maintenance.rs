use std::io::stdout;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use termion::style;

use super::{
    error_display_model, map_command_parse_error, reconcile_interactive_state_after_reload,
    CliRepositoryTransactionError, CommandError, InteractiveRepositoryEventOutcome,
    InteractiveRepositoryState, RunError, CLI_LOCK_TIMEOUT,
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
use crate::application::interface::TaskRepositoryTrait;

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
    task_repository: &mut dyn TaskRepositoryTrait,
    state: &mut InteractiveRepositoryState<'_>,
    command: &str,
    parsed_command: &Result<Command, CommandParseError>,
    maintenance_kind: Option<CommandKind>,
    operation_now: DateTime<Local>,
) -> Option<InteractiveRepositoryEventOutcome> {
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
        return Some(
            match execute_restore_current_command(
                stdout,
                task_repository,
                snapshot_directory,
                pre_backup_directory,
                operation_now,
            ) {
                Ok(()) => match reconcile_interactive_state_after_reload(task_repository, state) {
                    Ok(changed) => {
                        if changed {
                            *state.focus_started_datetime = operation_now;
                        }
                        InteractiveRepositoryEventOutcome::CommandExecuted(
                            CommandKind::RestoreCurrent,
                            operation_now,
                        )
                    }
                    Err(error) => InteractiveRepositoryEventOutcome::Fatal(RunError::Command(
                        CommandError::Application(error),
                    )),
                },
                Err(RunError::CliRepositoryTransaction(error)) => {
                    InteractiveRepositoryEventOutcome::Retry(error)
                }
                Err(error) => InteractiveRepositoryEventOutcome::Fatal(error),
            },
        );
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
        return Some(
            match execute_backup_command_with_lock(
                stdout,
                snapshot_directory,
                operation_now,
                &storage_directory,
                &storage_lock,
            ) {
                Ok(()) => {
                    if let Err(error) = task_repository.reload_if_changed(operation_now) {
                        return Some(InteractiveRepositoryEventOutcome::Retry(
                            CliRepositoryTransactionError::Load(error),
                        ));
                    }
                    match reconcile_interactive_state_after_reload(task_repository, state) {
                        Ok(changed) => {
                            if changed {
                                *state.focus_started_datetime = operation_now;
                            }
                            InteractiveRepositoryEventOutcome::CommandExecuted(
                                CommandKind::Backup,
                                operation_now,
                            )
                        }
                        Err(error) => InteractiveRepositoryEventOutcome::Fatal(RunError::Command(
                            CommandError::Application(error),
                        )),
                    }
                }
                Err(RunError::CliRepositoryTransaction(error)) => {
                    InteractiveRepositoryEventOutcome::Retry(error)
                }
                Err(error) => InteractiveRepositoryEventOutcome::Fatal(error),
            },
        );
    }
    None
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
