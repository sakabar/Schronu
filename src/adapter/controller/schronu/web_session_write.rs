use crate::application::task_use_case::AddActualWorkInput;
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecordSessionRequest {
    pub task_id: String,
    pub started_at_epoch_ms: i64,
    pub expected_actual_work_seconds: i64,
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
    ElapsedTimeOverflow {
        started_at_epoch_ms: i64,
        observed_at_epoch_ms: i64,
    },
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
            Self::ElapsedTimeOverflow {
                started_at_epoch_ms,
                observed_at_epoch_ms,
            } => write!(
                formatter,
                "elapsed milliseconds overflow for observed_at_epoch_ms {observed_at_epoch_ms} and started_at_epoch_ms {started_at_epoch_ms}"
            ),
        }
    }
}

impl Error for WebSessionInputError {}

pub(super) fn prepare_add_actual_work_input(
    request: RecordSessionRequest,
    operation_now: DateTime<Local>,
) -> Result<AddActualWorkInput, WebSessionInputError> {
    let task_id =
        Uuid::parse_str(&request.task_id).map_err(|error| WebSessionInputError::InvalidTaskId {
            task_id: request.task_id.clone(),
            reason: error.to_string(),
        })?;
    if request.expected_actual_work_seconds < 0 {
        return Err(WebSessionInputError::NegativeExpectedActualWorkSeconds(
            request.expected_actual_work_seconds,
        ));
    }
    if DateTime::<Utc>::from_timestamp_millis(request.started_at_epoch_ms).is_none() {
        return Err(WebSessionInputError::StartedAtOutOfRange(
            request.started_at_epoch_ms,
        ));
    }

    let observed_at_epoch_ms = operation_now.timestamp_millis();
    let elapsed_milliseconds = observed_at_epoch_ms
        .checked_sub(request.started_at_epoch_ms)
        .ok_or(WebSessionInputError::ElapsedTimeOverflow {
            started_at_epoch_ms: request.started_at_epoch_ms,
            observed_at_epoch_ms,
        })?;
    if elapsed_milliseconds < 0 {
        return Err(WebSessionInputError::FutureStartedAt {
            started_at_epoch_ms: request.started_at_epoch_ms,
            observed_at_epoch_ms,
        });
    }

    Ok(AddActualWorkInput {
        task_id,
        additional_actual_work_seconds: elapsed_milliseconds / 1_000,
        expected_actual_work_seconds: Some(request.expected_actual_work_seconds),
    })
}
