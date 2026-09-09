use crate::application::task_use_case::{AddActualWorkInput, CompleteTaskInput};
use crate::entity::discarded_session::{
    DiscardedSessionEvent, DiscardedSessionEventError, DiscardedSessionReason,
    DiscardedSessionSource,
};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecordSessionRequest {
    pub task_id: String,
    pub started_at_epoch_ms: i64,
    pub ended_at_epoch_ms: Option<i64>,
    pub expected_actual_work_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompleteSessionRequest {
    pub task_id: String,
    pub started_at_epoch_ms: i64,
    pub ended_at_epoch_ms: Option<i64>,
    pub expected_actual_work_seconds: i64,
    pub record_elapsed_seconds: bool,
    pub discard_event_id: Option<String>,
    pub task_name_at_start: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiscardSessionRequest {
    pub event_id: String,
    pub task_id: String,
    pub task_name_at_start: String,
    pub started_at_epoch_ms: i64,
    pub ended_at_epoch_ms: i64,
}

pub(super) struct PreparedCompleteSession {
    pub(super) input: CompleteTaskInput,
    pub(super) discarded_event: Option<DiscardedSessionEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecordSessionResult {
    pub actual_work_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebSessionInputError {
    InvalidTaskId {
        task_id: String,
        reason: String,
    },
    FutureStartedAt {
        started_at_epoch_ms: i64,
        observed_at_epoch_ms: i64,
    },
    NegativeExpectedActualWorkSeconds(i64),
    StartedAtOutOfRange(i64),
    EndedAtOutOfRange(i64),
    FutureEndedAt {
        ended_at_epoch_ms: i64,
        observed_at_epoch_ms: i64,
    },
    ElapsedTimeOverflow {
        started_at_epoch_ms: i64,
        observed_at_epoch_ms: i64,
    },
    InvalidEventId {
        event_id: String,
        reason: String,
    },
    MissingDiscardEventId,
    MissingTaskNameAtStart,
    DiscardedSession(DiscardedSessionEventError),
}

impl fmt::Display for WebSessionInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTaskId { task_id, reason } => {
                write!(formatter, "invalid task_id {task_id:?}: {reason}")
            }
            Self::FutureStartedAt {
                started_at_epoch_ms,
                observed_at_epoch_ms,
            } => write!(
                formatter,
                "started_at_epoch_ms {started_at_epoch_ms} is later than observed_at_epoch_ms {observed_at_epoch_ms}"
            ),
            Self::NegativeExpectedActualWorkSeconds(value) => write!(
                formatter,
                "expected_actual_work_seconds must not be negative: {value}"
            ),
            Self::StartedAtOutOfRange(value) => {
                write!(formatter, "started_at_epoch_ms is out of range: {value}")
            }
            Self::EndedAtOutOfRange(value) => {
                write!(formatter, "ended_at_epoch_ms is out of range: {value}")
            }
            Self::FutureEndedAt {
                ended_at_epoch_ms,
                observed_at_epoch_ms,
            } => write!(
                formatter,
                "ended_at_epoch_ms {ended_at_epoch_ms} is later than observed_at_epoch_ms {observed_at_epoch_ms}"
            ),
            Self::ElapsedTimeOverflow {
                started_at_epoch_ms,
                observed_at_epoch_ms,
            } => write!(
                formatter,
                "elapsed milliseconds overflow for observed_at_epoch_ms {observed_at_epoch_ms} and started_at_epoch_ms {started_at_epoch_ms}"
            ),
            Self::InvalidEventId { event_id, reason } => {
                write!(formatter, "invalid event_id {event_id:?}: {reason}")
            }
            Self::MissingDiscardEventId => formatter.write_str("discard_event_id is required"),
            Self::MissingTaskNameAtStart => formatter.write_str("task_name_at_start is required"),
            Self::DiscardedSession(error) => error.fmt(formatter),
        }
    }
}

impl Error for WebSessionInputError {}

pub(super) fn prepare_add_actual_work_input(
    request: RecordSessionRequest,
    operation_now: DateTime<Local>,
) -> Result<AddActualWorkInput, WebSessionInputError> {
    let task_id = validate_task_and_expected_actual_work(
        &request.task_id,
        request.expected_actual_work_seconds,
    )?;
    let (ended_at, _) = resolve_ended_at(request.ended_at_epoch_ms, operation_now)?;
    let additional_actual_work_seconds =
        calculate_elapsed_seconds(request.started_at_epoch_ms, ended_at)?;

    Ok(AddActualWorkInput {
        task_id,
        additional_actual_work_seconds,
        expected_actual_work_seconds: Some(request.expected_actual_work_seconds),
    })
}

pub(super) fn prepare_complete_task_input(
    request: CompleteSessionRequest,
    operation_now: DateTime<Local>,
) -> Result<PreparedCompleteSession, WebSessionInputError> {
    let task_id = validate_task_and_expected_actual_work(
        &request.task_id,
        request.expected_actual_work_seconds,
    )?;
    let (ended_at, finished_at) = resolve_ended_at(request.ended_at_epoch_ms, operation_now)?;
    let additional_actual_work_seconds = if request.record_elapsed_seconds {
        calculate_elapsed_seconds(request.started_at_epoch_ms, ended_at)?
    } else {
        0
    };

    let discarded_event = match (
        request.record_elapsed_seconds,
        request.discard_event_id.as_deref(),
        request.task_name_at_start.as_deref(),
    ) {
        (true, _, _) | (false, None, None) => None,
        (false, Some(event_id), Some(task_name_at_start)) => prepare_discarded_event(
            event_id,
            task_id,
            task_name_at_start,
            request.started_at_epoch_ms,
            ended_at.timestamp_millis(),
            DiscardedSessionReason::WebDiscardComplete,
        )?,
        (false, None, Some(_)) => return Err(WebSessionInputError::MissingDiscardEventId),
        (false, Some(_), None) => return Err(WebSessionInputError::MissingTaskNameAtStart),
    };

    Ok(PreparedCompleteSession {
        input: CompleteTaskInput {
            task_id,
            finished_at,
            additional_actual_work_seconds,
            expected_actual_work_seconds: Some(request.expected_actual_work_seconds),
        },
        discarded_event,
    })
}

pub(super) fn prepare_discard_session_event(
    request: DiscardSessionRequest,
) -> Result<Option<DiscardedSessionEvent>, WebSessionInputError> {
    let task_id =
        Uuid::parse_str(&request.task_id).map_err(|error| WebSessionInputError::InvalidTaskId {
            task_id: request.task_id.clone(),
            reason: error.to_string(),
        })?;
    prepare_discarded_event(
        &request.event_id,
        task_id,
        &request.task_name_at_start,
        request.started_at_epoch_ms,
        request.ended_at_epoch_ms,
        DiscardedSessionReason::WebDiscardRelease,
    )
}

fn prepare_discarded_event(
    event_id: &str,
    task_id: Uuid,
    task_name_at_start: &str,
    started_at_epoch_ms: i64,
    ended_at_epoch_ms: i64,
    reason: DiscardedSessionReason,
) -> Result<Option<DiscardedSessionEvent>, WebSessionInputError> {
    let event_id =
        Uuid::parse_str(event_id).map_err(|error| WebSessionInputError::InvalidEventId {
            event_id: event_id.to_owned(),
            reason: error.to_string(),
        })?;
    let started_at = DateTime::<Utc>::from_timestamp_millis(started_at_epoch_ms)
        .ok_or(WebSessionInputError::StartedAtOutOfRange(
            started_at_epoch_ms,
        ))?
        .with_timezone(&Local);
    let ended_at = DateTime::<Utc>::from_timestamp_millis(ended_at_epoch_ms)
        .ok_or(WebSessionInputError::EndedAtOutOfRange(ended_at_epoch_ms))?
        .with_timezone(&Local);
    DiscardedSessionEvent::new(
        event_id,
        task_id,
        task_name_at_start.to_owned(),
        started_at,
        ended_at,
        DiscardedSessionSource::Web,
        reason,
    )
    .map_err(WebSessionInputError::DiscardedSession)
}

fn validate_task_and_expected_actual_work(
    task_id: &str,
    expected_actual_work_seconds: i64,
) -> Result<Uuid, WebSessionInputError> {
    let task_id =
        Uuid::parse_str(task_id).map_err(|error| WebSessionInputError::InvalidTaskId {
            task_id: task_id.to_owned(),
            reason: error.to_string(),
        })?;
    if expected_actual_work_seconds < 0 {
        return Err(WebSessionInputError::NegativeExpectedActualWorkSeconds(
            expected_actual_work_seconds,
        ));
    }
    Ok(task_id)
}

fn calculate_elapsed_seconds(
    started_at_epoch_ms: i64,
    ended_at: DateTime<Local>,
) -> Result<i64, WebSessionInputError> {
    if DateTime::<Utc>::from_timestamp_millis(started_at_epoch_ms).is_none() {
        return Err(WebSessionInputError::StartedAtOutOfRange(
            started_at_epoch_ms,
        ));
    }

    let observed_at_epoch_ms = ended_at.timestamp_millis();
    let elapsed_milliseconds = observed_at_epoch_ms
        .checked_sub(started_at_epoch_ms)
        .ok_or(WebSessionInputError::ElapsedTimeOverflow {
            started_at_epoch_ms,
            observed_at_epoch_ms,
        })?;
    if elapsed_milliseconds < 0 {
        return Err(WebSessionInputError::FutureStartedAt {
            started_at_epoch_ms,
            observed_at_epoch_ms,
        });
    }
    Ok(elapsed_milliseconds / 1_000)
}

fn resolve_ended_at(
    ended_at_epoch_ms: Option<i64>,
    operation_now: DateTime<Local>,
) -> Result<(DateTime<Local>, DateTime<Local>), WebSessionInputError> {
    let Some(ended_at_epoch_ms) = ended_at_epoch_ms else {
        return Ok((operation_now, operation_now));
    };
    let ended_at = DateTime::<Utc>::from_timestamp_millis(ended_at_epoch_ms)
        .ok_or(WebSessionInputError::EndedAtOutOfRange(ended_at_epoch_ms))?
        .with_timezone(&Local);
    let finished_at = ended_at.min(operation_now);
    Ok((ended_at, finished_at))
}
