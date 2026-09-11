use super::state::ClientState;
use super::time_model::{session_timing, SessionTiming};
use crate::SessionTask;
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, Timelike, Utc};

const INVALID_TIME: &str = "--:--";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionCardViewModel {
    pub task_id: String,
    pub task_name: String,
    pub started_at_hh_mm: String,
    pub completion_hh_mm: Option<String>,
    pub actual_work_seconds_at_start: i64,
    pub progress_percent: Option<i128>,
    pub normal_bar_percent: i128,
    pub overrun_bar_percent: i128,
    pub remaining_seconds: i128,
    pub in_flight: bool,
    pub manual_check_blocked: bool,
    pub server_committed: bool,
    pub completion_conflict: Option<CompletionConflictViewModel>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionConflictViewModel {
    pub current_actual_work_seconds: i64,
    pub measured_elapsed_seconds: i64,
    pub record_elapsed_seconds: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListRowViewModel {
    pub task: SessionTask,
    pub deadline_label: String,
    pub schedule_label: String,
    pub misses_deadline: bool,
    pub is_leaf: bool,
    pub defer_confirmation: Option<DeferConfirmationViewModel>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeferConfirmationKind {
    DueToday,
    Overdue,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeferConfirmationViewModel {
    pub kind: DeferConfirmationKind,
    pub deadline_datetime_label: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferClassificationError {
    InvalidCurrentTime,
    InvalidDeadline,
    LogicalDateOutOfRange,
}

pub fn project_session_cards(
    state: &ClientState,
    utc_offset_minutes: i32,
) -> Vec<SessionCardViewModel> {
    project_session_cards_with(state, |_| Some(utc_offset_minutes))
}

pub fn project_list_rows(state: &ClientState, utc_offset_minutes: i32) -> Vec<ListRowViewModel> {
    project_list_rows_with(state, |_| Some(utc_offset_minutes))
}

pub fn format_local_hh_mm(epoch_ms: i64, utc_offset_minutes: i32) -> String {
    local_datetime(epoch_ms, utc_offset_minutes)
        .map(|date_time| date_time.format("%H:%M").to_string())
        .unwrap_or_else(|| INVALID_TIME.to_owned())
}

#[cfg(feature = "web")]
pub fn project_session_cards_for_browser(state: &ClientState) -> Vec<SessionCardViewModel> {
    project_session_cards_with(state, browser_utc_offset_minutes)
}

#[cfg(feature = "web")]
pub fn project_list_rows_for_browser(state: &ClientState) -> Vec<ListRowViewModel> {
    project_list_rows_with(state, browser_utc_offset_minutes)
}

fn project_session_cards_with(
    state: &ClientState,
    offset_at: impl Fn(i64) -> Option<i32>,
) -> Vec<SessionCardViewModel> {
    state
        .sessions()
        .iter()
        .map(|session| {
            let server_committed = state.is_session_committed_blocked(&session.task_id);
            let actual_work_seconds = state
                .display_actual_work_seconds(&session.task_id)
                .unwrap_or(session.actual_work_seconds_at_start);
            let display_now = state
                .session_stopped_at_epoch_ms(&session.task_id)
                .unwrap_or_else(|| {
                    if server_committed {
                        session.started_at_epoch_ms
                    } else {
                        state.tick_now_epoch_ms()
                    }
                });
            let timing = session_timing(
                session.started_at_epoch_ms,
                session.estimated_work_seconds_at_start,
                actual_work_seconds,
                display_now,
            );
            SessionCardViewModel {
                task_id: session.task_id.clone(),
                task_name: session.task_name.clone(),
                started_at_hh_mm: format_with_offset_provider(
                    session.started_at_epoch_ms,
                    &offset_at,
                ),
                completion_hh_mm: completion_label(server_committed, timing, &offset_at),
                actual_work_seconds_at_start: session.actual_work_seconds_at_start,
                progress_percent: timing.progress_percent,
                normal_bar_percent: timing.normal_bar_percent,
                overrun_bar_percent: timing.overrun_bar_percent,
                remaining_seconds: timing.remaining_seconds,
                in_flight: state.is_session_in_flight(&session.task_id),
                manual_check_blocked: state.is_session_manual_check_blocked(&session.task_id),
                server_committed,
                completion_conflict: state.completion_conflict(&session.task_id).map(|conflict| {
                    let measured_milliseconds = (i128::from(conflict.ended_at_epoch_ms)
                        - i128::from(conflict.original_request.started_at_epoch_ms))
                    .max(0);
                    CompletionConflictViewModel {
                        current_actual_work_seconds: conflict.current_actual_work_seconds,
                        measured_elapsed_seconds: i64::try_from(measured_milliseconds / 1_000)
                            .expect(
                            "the difference between two i64 millisecond epochs fits in i64 seconds",
                        ),
                        record_elapsed_seconds: conflict.original_request.record_elapsed_seconds,
                    }
                }),
            }
        })
        .collect()
}

fn completion_label(
    server_committed: bool,
    timing: SessionTiming,
    offset_at: &impl Fn(i64) -> Option<i32>,
) -> Option<String> {
    if server_committed {
        return None;
    }
    timing
        .estimated_completion_epoch_ms
        .map(|epoch_ms| format_with_offset_provider(epoch_ms, offset_at))
}

fn project_list_rows_with(
    state: &ClientState,
    offset_at: impl Fn(i64) -> Option<i32>,
) -> Vec<ListRowViewModel> {
    let current_epoch_ms = state.tick_now_epoch_ms();
    let current_offset = offset_at(current_epoch_ms);
    state
        .scheduled_rows()
        .iter()
        .map(|row| {
            let defer_confirmation = row.deadline_epoch_ms.and_then(|deadline_epoch_ms| {
                let Some(current_offset) = current_offset else {
                    return Some(DeferConfirmationViewModel {
                        kind: DeferConfirmationKind::Unknown,
                        deadline_datetime_label: INVALID_TIME.to_owned(),
                    });
                };
                let Some(offset) = offset_at(deadline_epoch_ms) else {
                    return Some(DeferConfirmationViewModel {
                        kind: DeferConfirmationKind::Unknown,
                        deadline_datetime_label: INVALID_TIME.to_owned(),
                    });
                };
                let deadline_datetime_label =
                    format_local_month_day_hh_mm(deadline_epoch_ms, offset);
                match classify_defer_confirmation(
                    Some(deadline_epoch_ms),
                    offset,
                    current_epoch_ms,
                    current_offset,
                ) {
                    Ok(Some(kind)) => Some(DeferConfirmationViewModel {
                        kind,
                        deadline_datetime_label,
                    }),
                    Ok(None) => None,
                    Err(_) => Some(DeferConfirmationViewModel {
                        kind: DeferConfirmationKind::Unknown,
                        deadline_datetime_label,
                    }),
                }
            });
            ListRowViewModel {
                task: row.task.clone(),
                deadline_label: row.deadline_label.clone(),
                schedule_label: format!(
                    "{}-{}",
                    format_with_offset_provider(row.schedule_start_epoch_ms, &offset_at),
                    format_with_offset_provider(row.schedule_end_epoch_ms, &offset_at)
                ),
                misses_deadline: row.misses_deadline,
                is_leaf: row.is_leaf,
                defer_confirmation,
            }
        })
        .collect()
}

fn classify_defer_confirmation(
    deadline_epoch_ms: Option<i64>,
    deadline_utc_offset_minutes: i32,
    current_epoch_ms: i64,
    current_utc_offset_minutes: i32,
) -> Result<Option<DeferConfirmationKind>, DeferClassificationError> {
    let Some(deadline_epoch_ms) = deadline_epoch_ms else {
        return Ok(None);
    };
    let current_logical_date = logical_date(current_epoch_ms, current_utc_offset_minutes)
        .map_err(|_| DeferClassificationError::InvalidCurrentTime)?;
    let deadline_logical_date = logical_date(deadline_epoch_ms, deadline_utc_offset_minutes)
        .map_err(|_| DeferClassificationError::InvalidDeadline)?;
    match deadline_logical_date.cmp(&current_logical_date) {
        std::cmp::Ordering::Less => Ok(Some(DeferConfirmationKind::Overdue)),
        std::cmp::Ordering::Equal => Ok(Some(DeferConfirmationKind::DueToday)),
        std::cmp::Ordering::Greater => Ok(None),
    }
}

fn logical_date(
    epoch_ms: i64,
    utc_offset_minutes: i32,
) -> Result<NaiveDate, DeferClassificationError> {
    let datetime = local_datetime(epoch_ms, utc_offset_minutes)
        .ok_or(DeferClassificationError::LogicalDateOutOfRange)?;
    if datetime.hour() < 6 {
        datetime
            .date_naive()
            .pred_opt()
            .ok_or(DeferClassificationError::LogicalDateOutOfRange)
    } else {
        Ok(datetime.date_naive())
    }
}

fn format_local_month_day_hh_mm(epoch_ms: i64, utc_offset_minutes: i32) -> String {
    local_datetime(epoch_ms, utc_offset_minutes)
        .map(|datetime| {
            format!(
                "{}/{} {}",
                datetime.month(),
                datetime.day(),
                datetime.format("%H:%M")
            )
        })
        .unwrap_or_else(|| INVALID_TIME.to_owned())
}

fn format_with_offset_provider(epoch_ms: i64, offset_at: &impl Fn(i64) -> Option<i32>) -> String {
    offset_at(epoch_ms)
        .map(|offset| format_local_hh_mm(epoch_ms, offset))
        .unwrap_or_else(|| INVALID_TIME.to_owned())
}

fn local_datetime(epoch_ms: i64, utc_offset_minutes: i32) -> Option<DateTime<FixedOffset>> {
    let offset_seconds = utc_offset_minutes.checked_mul(60)?;
    let offset = FixedOffset::east_opt(offset_seconds)?;
    Some(DateTime::<Utc>::from_timestamp_millis(epoch_ms)?.with_timezone(&offset))
}

#[cfg(feature = "web")]
fn browser_utc_offset_minutes(epoch_ms: i64) -> Option<i32> {
    DateTime::<Utc>::from_timestamp_millis(epoch_ms)?;
    let date = js_sys::Date::new_0();
    date.set_time(epoch_ms as f64);
    let utc_minus_local = date.get_timezone_offset();
    if !utc_minus_local.is_finite()
        || utc_minus_local.fract() != 0.0
        || utc_minus_local < f64::from(i32::MIN)
        || utc_minus_local > f64::from(i32::MAX)
    {
        return None;
    }
    (-(utc_minus_local as i64)).try_into().ok()
}

#[cfg(test)]
mod defer_confirmation_tests {
    use super::{classify_defer_confirmation, DeferConfirmationKind};

    fn epoch_ms(value: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(value)
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn 締切logical_dateが今日以前だけ先送り確認を要求する() {
        let before_boundary = epoch_ms("2026-09-12T05:59:00+09:00");
        let at_boundary = epoch_ms("2026-09-12T06:00:00+09:00");

        assert_eq!(
            classify_defer_confirmation(
                Some(epoch_ms("2026-09-12T05:59:00+09:00")),
                540,
                before_boundary,
                540,
            ),
            Ok(Some(DeferConfirmationKind::DueToday))
        );
        assert_eq!(
            classify_defer_confirmation(
                Some(epoch_ms("2026-09-11T05:59:00+09:00")),
                540,
                before_boundary,
                540,
            ),
            Ok(Some(DeferConfirmationKind::Overdue))
        );
        assert_eq!(
            classify_defer_confirmation(
                Some(epoch_ms("2026-09-12T06:00:00+09:00")),
                540,
                before_boundary,
                540,
            ),
            Ok(None)
        );
        assert_eq!(
            classify_defer_confirmation(
                Some(epoch_ms("2026-09-12T05:59:00+09:00")),
                540,
                at_boundary,
                540,
            ),
            Ok(Some(DeferConfirmationKind::Overdue))
        );
        assert_eq!(
            classify_defer_confirmation(None, 540, before_boundary, 540),
            Ok(None)
        );
    }
}
