use super::state::ClientState;
use super::time_model::{session_timing, SessionTiming};
use crate::{AllTaskRow, DeadlineDisplayKind, DeferMode, DeferPlan, SessionTask, TaskDisplayKind};
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, Utc, Weekday};

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
    pub row_key: String,
    pub task: SessionTask,
    pub deadline_label: String,
    pub schedule_display: ScheduleDisplayViewModel,
    pub gap_before: Option<String>,
    pub date_boundary_before: bool,
    pub misses_deadline: bool,
    pub task_display_kind: TaskDisplayKind,
    pub deadline_display_kind: DeadlineDisplayKind,
    pub is_leaf: bool,
    pub defer_plan: Option<DeferPlan>,
    pub defer_confirmation: Option<DeferConfirmationViewModel>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScheduleDisplayViewModel {
    Daily {
        start_hh_mm: String,
        duration_minutes: Option<u64>,
    },
    AllTasksDate {
        label: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VisibleAllTaskRows {
    pub rows: Vec<ListRowViewModel>,
    pub has_more: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeferConfirmationKind {
    DeadlineLimited,
    RoutinePeriod,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeferConfirmationViewModel {
    pub kind: DeferConfirmationKind,
    pub detail_label: String,
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

pub fn project_all_task_rows(rows: &[AllTaskRow]) -> Vec<ListRowViewModel> {
    project_visible_all_task_rows(rows, "", usize::MAX).rows
}

pub fn project_visible_all_task_rows(
    rows: &[AllTaskRow],
    filter: &str,
    visible_limit: usize,
) -> VisibleAllTaskRows {
    let mut previous_date = None;
    let mut projected_rows = Vec::with_capacity(visible_limit.min(rows.len()));
    let mut has_more = false;
    for row in rows {
        if !task_name_matches(filter, &row.task.task_name) {
            continue;
        }
        if projected_rows.len() == visible_limit {
            has_more = true;
            break;
        }

        let current_date = NaiveDate::parse_from_str(&row.schedule_date, "%Y-%m-%d").ok();
        let gap_before = date_gap_label(previous_date, current_date);
        let date_boundary_before =
            logical_dates_are_adjacent(previous_date, current_date) && gap_before.is_none();
        previous_date = current_date.map(|date| {
            previous_date
                .map(|previous| previous.max(date))
                .unwrap_or(date)
        });

        projected_rows.push(project_all_task_row(row, gap_before, date_boundary_before));
    }
    VisibleAllTaskRows {
        rows: projected_rows,
        has_more,
    }
}

pub fn task_name_matches(filter: &str, task_name: &str) -> bool {
    let normalized_filter = filter.trim().to_lowercase();
    normalized_filter.is_empty() || task_name.to_lowercase().contains(&normalized_filter)
}

fn project_all_task_row(
    row: &AllTaskRow,
    gap_before: Option<String>,
    date_boundary_before: bool,
) -> ListRowViewModel {
    ListRowViewModel {
        row_key: format!("all:{}", row.segment_index),
        task: row.task.clone(),
        deadline_label: row.deadline_label.clone(),
        schedule_display: ScheduleDisplayViewModel::AllTasksDate {
            label: all_task_schedule_label(&row.schedule_date),
        },
        gap_before,
        date_boundary_before,
        misses_deadline: row.misses_deadline,
        task_display_kind: row.task_display_kind,
        deadline_display_kind: row.deadline_display_kind,
        is_leaf: row.is_leaf,
        defer_plan: None,
        defer_confirmation: None,
    }
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
    let mut display_cursor_epoch_ms = match (state.selected_logical_date(), state.snapshot()) {
        (Some(selected_date), Some(snapshot)) if selected_date == snapshot.logical_date => {
            Some(snapshot.observed_at_epoch_ms)
        }
        _ => None,
    };
    state
        .scheduled_rows()
        .iter()
        .map(|row| {
            let gap_before = minute_gap_label(display_cursor_epoch_ms, row.schedule_start_epoch_ms);
            display_cursor_epoch_ms = Some(
                display_cursor_epoch_ms
                    .map(|cursor| cursor.max(row.schedule_end_epoch_ms))
                    .unwrap_or(row.schedule_end_epoch_ms),
            );
            let defer_confirmation = match row.defer_plan.mode {
                DeferMode::Normal => None,
                DeferMode::DeadlineLimited => Some(DeferConfirmationViewModel {
                    kind: DeferConfirmationKind::DeadlineLimited,
                    detail_label: row
                        .defer_plan
                        .effective_pending_until_epoch_ms
                        .and_then(|epoch_ms| {
                            offset_at(epoch_ms)
                                .map(|offset| format_local_month_day_hh_mm(epoch_ms, offset))
                        })
                        .unwrap_or_else(|| INVALID_TIME.to_owned()),
                }),
                DeferMode::RoutinePeriod => Some(DeferConfirmationViewModel {
                    kind: DeferConfirmationKind::RoutinePeriod,
                    detail_label: row
                        .defer_plan
                        .repetition_interval_days
                        .map(|days| format!("{days}日"))
                        .unwrap_or_else(|| INVALID_TIME.to_owned()),
                }),
            };
            ListRowViewModel {
                row_key: format!(
                    "{}:{}:{}",
                    row.task.task_id, row.schedule_start_epoch_ms, row.schedule_end_epoch_ms
                ),
                task: row.task.clone(),
                deadline_label: row.deadline_label.clone(),
                schedule_display: ScheduleDisplayViewModel::Daily {
                    start_hh_mm: format_with_offset_provider(
                        row.schedule_start_epoch_ms,
                        &offset_at,
                    ),
                    duration_minutes: schedule_duration_minutes(
                        row.schedule_start_epoch_ms,
                        row.schedule_end_epoch_ms,
                    ),
                },
                gap_before,
                date_boundary_before: false,
                misses_deadline: row.misses_deadline,
                task_display_kind: row.task_display_kind,
                deadline_display_kind: effective_deadline_display_kind(
                    row.deadline_display_kind,
                    row.misses_deadline,
                ),
                is_leaf: row.is_leaf,
                defer_plan: Some(row.defer_plan.clone()),
                defer_confirmation,
            }
        })
        .collect()
}

fn schedule_duration_minutes(start_epoch_ms: i64, end_epoch_ms: i64) -> Option<u64> {
    let duration_ms = i128::from(end_epoch_ms) - i128::from(start_epoch_ms);
    if duration_ms < 0 {
        return None;
    }
    u64::try_from((duration_ms + 59_999) / 60_000).ok()
}

fn minute_gap_label(cursor_epoch_ms: Option<i64>, next_start_epoch_ms: i64) -> Option<String> {
    let gap_minutes = next_start_epoch_ms
        .checked_sub(cursor_epoch_ms?)?
        .checked_div(60_000)?;
    (gap_minutes > 0).then(|| format!("{gap_minutes}分間の空き時間"))
}

fn date_gap_label(
    previous_date: Option<NaiveDate>,
    current_date: Option<NaiveDate>,
) -> Option<String> {
    let gap_days = current_date?
        .signed_duration_since(previous_date?)
        .num_days()
        - 1;
    (gap_days > 0).then(|| format!("{gap_days}日間の空き時間"))
}

fn logical_dates_are_adjacent(
    previous_date: Option<NaiveDate>,
    current_date: Option<NaiveDate>,
) -> bool {
    previous_date
        .zip(current_date)
        .is_some_and(|(previous, current)| current.signed_duration_since(previous).num_days() == 1)
}

fn effective_deadline_display_kind(
    deadline_display_kind: DeadlineDisplayKind,
    misses_deadline: bool,
) -> DeadlineDisplayKind {
    if misses_deadline && deadline_display_kind == DeadlineDisplayKind::None {
        DeadlineDisplayKind::Overrun
    } else {
        deadline_display_kind
    }
}

fn all_task_schedule_label(schedule_date: &str) -> String {
    let Ok(date) = NaiveDate::parse_from_str(schedule_date, "%Y-%m-%d") else {
        return schedule_date.to_owned();
    };
    let weekday = match date.weekday() {
        Weekday::Mon => "月",
        Weekday::Tue => "火",
        Weekday::Wed => "水",
        Weekday::Thu => "木",
        Weekday::Fri => "金",
        Weekday::Sat => "土",
        Weekday::Sun => "日",
    };
    format!("{}({weekday})", date.format("%Y/%m/%d"))
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
