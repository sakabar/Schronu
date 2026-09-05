use super::state::ClientState;
use super::time_model::{session_timing, SessionTiming};
use crate::SessionTask;
use chrono::{DateTime, FixedOffset, NaiveTime, Utc};

const LOGICAL_DATE_BOUNDARY_HOUR: u32 = 6;
const INVALID_TIME: &str = "--:--";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionCardViewModel {
    pub task_id: String,
    pub task_name: String,
    pub started_at_hh_mm: String,
    pub completion_hh_mm: Option<String>,
    pub progress_percent: Option<i128>,
    pub normal_bar_percent: i128,
    pub overrun_bar_percent: i128,
    pub remaining_seconds: i128,
    pub in_flight: bool,
    pub manual_check_blocked: bool,
    pub server_committed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListRowViewModel {
    pub task: SessionTask,
    pub deadline_label: Option<String>,
    pub schedule_label: String,
    pub deadline_epoch_ms: Option<i64>,
    pub is_leaf: bool,
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

pub fn format_deadline_label(
    epoch_ms: i64,
    selected_logical_date: &str,
    utc_offset_minutes: i32,
) -> Option<String> {
    Some(format_deadline_label_inner(
        epoch_ms,
        selected_logical_date,
        utc_offset_minutes,
    ))
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
            let display_now = if server_committed {
                session.started_at_epoch_ms
            } else {
                state.tick_now_epoch_ms()
            };
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
                progress_percent: timing.progress_percent,
                normal_bar_percent: timing.normal_bar_percent,
                overrun_bar_percent: timing.overrun_bar_percent,
                remaining_seconds: timing.remaining_seconds,
                in_flight: state.is_session_in_flight(&session.task_id),
                manual_check_blocked: state.is_session_manual_check_blocked(&session.task_id),
                server_committed,
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
    let selected_logical_date = state.selected_logical_date().unwrap_or_default();
    state
        .scheduled_rows()
        .iter()
        .map(|row| ListRowViewModel {
            task: row.task.clone(),
            deadline_label: row.deadline_epoch_ms.map(|epoch_ms| {
                offset_at(epoch_ms).map_or_else(
                    || INVALID_TIME.to_owned(),
                    |offset| format_deadline_label_inner(epoch_ms, selected_logical_date, offset),
                )
            }),
            schedule_label: format!(
                "{}-{}",
                format_with_offset_provider(row.schedule_start_epoch_ms, &offset_at),
                format_with_offset_provider(row.schedule_end_epoch_ms, &offset_at)
            ),
            deadline_epoch_ms: row.deadline_epoch_ms,
            is_leaf: row.is_leaf,
        })
        .collect()
}

fn format_with_offset_provider(epoch_ms: i64, offset_at: &impl Fn(i64) -> Option<i32>) -> String {
    offset_at(epoch_ms)
        .map(|offset| format_local_hh_mm(epoch_ms, offset))
        .unwrap_or_else(|| INVALID_TIME.to_owned())
}

fn format_deadline_label_inner(
    epoch_ms: i64,
    selected_logical_date: &str,
    utc_offset_minutes: i32,
) -> String {
    let Some(date_time) = local_datetime(epoch_ms, utc_offset_minutes) else {
        return INVALID_TIME.to_owned();
    };
    if logical_date(&date_time).as_deref() == Some(selected_logical_date) {
        date_time.format("%H:%M").to_string()
    } else {
        date_time.format("%m/%d %H:%M").to_string()
    }
}

fn local_datetime(epoch_ms: i64, utc_offset_minutes: i32) -> Option<DateTime<FixedOffset>> {
    let offset_seconds = utc_offset_minutes.checked_mul(60)?;
    let offset = FixedOffset::east_opt(offset_seconds)?;
    Some(DateTime::<Utc>::from_timestamp_millis(epoch_ms)?.with_timezone(&offset))
}

fn logical_date(date_time: &DateTime<FixedOffset>) -> Option<String> {
    let boundary = NaiveTime::from_hms_opt(LOGICAL_DATE_BOUNDARY_HOUR, 0, 0)?;
    let mut date = date_time.date_naive();
    if date_time.time() < boundary {
        date = date.pred_opt()?;
    }
    Some(date.format("%Y-%m-%d").to_string())
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
