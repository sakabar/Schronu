use super::{external_open_error, CommandError, ResolvedExternalRequest};
use crate::application::task_use_case::ApplicationError;
use crate::entity::task::TaskHandle;
use chrono::{DateTime, Local};
use std::collections::HashMap;
#[cfg(any(target_os = "macos", test))]
use std::process;
use uuid::Uuid;

pub(super) fn resolve_timer_shortcut(
    focused_task_opt: &Option<TaskHandle>,
    focus_started_at: DateTime<Local>,
    now: DateTime<Local>,
) -> Result<ResolvedExternalRequest, ApplicationError> {
    let Some(task) = focused_task_opt else {
        return Ok(ResolvedExternalRequest::Message(
            "フォーカス中のタスクがありません",
        ));
    };
    let estimated = task
        .get_estimated_work_seconds()
        .map_err(ApplicationError::TaskTree)?;
    let actual = task
        .get_actual_work_seconds()
        .map_err(ApplicationError::TaskTree)?;
    let elapsed = (now - focus_started_at).num_seconds().max(0);
    let remaining = i128::from(estimated) - i128::from(actual) - i128::from(elapsed);
    if remaining <= 0 {
        return Ok(ResolvedExternalRequest::Message(
            "見積もりの残り時間はありません",
        ));
    }
    Ok(ResolvedExternalRequest::TimerUrl {
        task_id: task.get_id().map_err(ApplicationError::TaskTree)?,
        seconds: remaining,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum TimerLaunchOutcome {
    Started,
    AlreadyRunning,
    StartedAlongsideAnother,
}

pub(super) type TimerSessions = HashMap<Uuid, (DateTime<Local>, i128)>;

pub(super) fn launch_timer_shortcut_for_session(
    timers: &mut TimerSessions,
    task_id: Uuid,
    seconds: i128,
    now: DateTime<Local>,
    launch: impl FnOnce(&str) -> Result<DateTime<Local>, CommandError>,
) -> Result<TimerLaunchOutcome, CommandError> {
    timers.retain(|_, (started_at, duration)| {
        i128::from((now - *started_at).num_milliseconds()) < *duration * 1000
    });
    if timers.contains_key(&task_id) {
        return Ok(TimerLaunchOutcome::AlreadyRunning);
    }
    let alongside_another = !timers.is_empty();
    let url = format!("shortcuts://run-shortcut?name=TimerForSchronu&input=text&text={seconds}");
    let launched_at = launch(&url)?;
    timers.insert(task_id, (launched_at, seconds));
    Ok(if alongside_another {
        TimerLaunchOutcome::StartedAlongsideAnother
    } else {
        TimerLaunchOutcome::Started
    })
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn shortcut_open_command(url: &str) -> process::Command {
    let mut command = process::Command::new("open");
    command.arg(url);
    command
}

pub(super) fn execute_open_timer_shortcut(url: &str) -> Result<(), CommandError> {
    #[cfg(target_os = "macos")]
    {
        let status = shortcut_open_command(url)
            .status()
            .map_err(|source| external_open_error("Shortcuts", source))?;
        if status.success() {
            Ok(())
        } else {
            Err(external_open_error(
                "Shortcuts",
                std::io::Error::other(format!("open exited with status {status}")),
            ))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = url;
        Err(external_open_error(
            "Shortcuts",
            std::io::Error::other("macOS専用です"),
        ))
    }
}
