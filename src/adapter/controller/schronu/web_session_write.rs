use crate::application::task_use_case::{
    AddActualWorkInput, CompleteTaskInput, DeferMode, DeferTaskPlan,
};
use chrono::{DateTime, Local, NaiveDate, Utc};
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
pub struct DeferTaskRequest {
    pub task_id: String,
    pub selected_logical_date: String,
    pub expected_plan: DeferPlanRequest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeferPlanRequest {
    pub mode: DeferMode,
    pub requested_pending_until_epoch_ms: i64,
    pub effective_pending_until_epoch_ms: Option<i64>,
    pub repetition_interval_days: Option<i64>,
}

pub(super) struct PreparedDeferTaskInput {
    pub task_id: Uuid,
    pub selected_logical_date: NaiveDate,
    pub expected_plan: DeferTaskPlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompleteSessionRequest {
    pub task_id: String,
    pub started_at_epoch_ms: i64,
    pub ended_at_epoch_ms: Option<i64>,
    pub expected_actual_work_seconds: i64,
    pub record_elapsed_seconds: bool,
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
    InvalidSelectedLogicalDate(String),
    InvalidDeferPlan(String),
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
}

impl fmt::Display for WebSessionInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTaskId { task_id, reason } => {
                write!(formatter, "invalid task_id {task_id:?}: {reason}")
            }
            Self::InvalidSelectedLogicalDate(value) => {
                write!(formatter, "invalid selected_logical_date: {value:?}")
            }
            Self::InvalidDeferPlan(reason) => write!(formatter, "invalid defer plan: {reason}"),
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

pub(super) fn prepare_defer_task_input(
    request: DeferTaskRequest,
) -> Result<PreparedDeferTaskInput, WebSessionInputError> {
    let expected_plan = prepare_defer_plan(request.expected_plan)?;
    Ok(PreparedDeferTaskInput {
        task_id: parse_task_id(&request.task_id)?,
        selected_logical_date: NaiveDate::parse_from_str(
            &request.selected_logical_date,
            "%Y-%m-%d",
        )
        .map_err(|_| {
            WebSessionInputError::InvalidSelectedLogicalDate(request.selected_logical_date)
        })?,
        expected_plan,
    })
}

fn prepare_defer_plan(plan: DeferPlanRequest) -> Result<DeferTaskPlan, WebSessionInputError> {
    let requested_pending_until = parse_defer_epoch(
        "requested_pending_until_epoch_ms",
        plan.requested_pending_until_epoch_ms,
    )?;
    let effective_pending_until = plan
        .effective_pending_until_epoch_ms
        .map(|value| parse_defer_epoch("effective_pending_until_epoch_ms", value))
        .transpose()?;
    let valid_shape = match plan.mode {
        DeferMode::Normal => {
            effective_pending_until.is_none() && plan.repetition_interval_days.is_none()
        }
        DeferMode::DeadlineLimited => {
            effective_pending_until.is_some_and(|effective| effective < requested_pending_until)
                && plan.repetition_interval_days.is_none()
        }
        DeferMode::RoutinePeriod => {
            effective_pending_until.is_none()
                && plan.repetition_interval_days.is_some_and(|days| days > 0)
        }
    };
    if !valid_shape {
        return Err(WebSessionInputError::InvalidDeferPlan(
            "fields do not match mode".to_owned(),
        ));
    }
    Ok(DeferTaskPlan {
        mode: plan.mode,
        requested_pending_until,
        effective_pending_until,
        repetition_interval_days: plan.repetition_interval_days,
    })
}

fn parse_defer_epoch(field: &str, value: i64) -> Result<DateTime<Local>, WebSessionInputError> {
    DateTime::<Utc>::from_timestamp_millis(value)
        .map(|date_time| date_time.with_timezone(&Local))
        .ok_or_else(|| WebSessionInputError::InvalidDeferPlan(format!("{field} is out of range")))
}

pub(super) fn prepare_complete_task_input(
    request: CompleteSessionRequest,
    operation_now: DateTime<Local>,
) -> Result<CompleteTaskInput, WebSessionInputError> {
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

    Ok(CompleteTaskInput {
        task_id,
        finished_at,
        additional_actual_work_seconds,
        expected_actual_work_seconds: Some(request.expected_actual_work_seconds),
    })
}

fn validate_task_and_expected_actual_work(
    task_id: &str,
    expected_actual_work_seconds: i64,
) -> Result<Uuid, WebSessionInputError> {
    let task_id = parse_task_id(task_id)?;
    if expected_actual_work_seconds < 0 {
        return Err(WebSessionInputError::NegativeExpectedActualWorkSeconds(
            expected_actual_work_seconds,
        ));
    }
    Ok(task_id)
}

fn parse_task_id(task_id: &str) -> Result<Uuid, WebSessionInputError> {
    Uuid::parse_str(task_id).map_err(|error| WebSessionInputError::InvalidTaskId {
        task_id: task_id.to_owned(),
        reason: error.to_string(),
    })
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
