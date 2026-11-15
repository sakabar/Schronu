#[cfg(feature = "web")]
use super::local_time::JAPAN_UTC_OFFSET_MINUTES;
use super::local_time::{local_datetime, logical_date_with_offset};
use super::state::ClientState;
use super::time_model::{session_plan_dates, session_timing, SessionTiming};
use crate::{
    AllTaskRow, DeadlineDisplayKind, DeferMode, DeferPlan, ScheduleOccurrence, ScheduledTask,
    TaskDisplayKind,
};
use chrono::{Datelike, NaiveDate, Weekday};
use std::collections::HashSet;

const INVALID_TIME: &str = "--:--";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionCardViewModel {
    pub estimate_success_percentage: Option<u8>,
    pub future_pending_date_label: Option<String>,
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
    pub task: ScheduledTask,
    pub display_task_name: String,
    pub advanceable_days: i64,
    pub occurrence: ScheduleOccurrence,
    pub deadline_label: String,
    pub deadline_time: Option<String>,
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
    pub matching_task_count: usize,
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
    project_session_cards_with(
        state,
        |_| Some(utc_offset_minutes),
        |_| Some(utc_offset_minutes),
    )
}

pub fn project_list_rows(state: &ClientState, utc_offset_minutes: i32) -> Vec<ListRowViewModel> {
    project_list_rows_with(state, &|_| Some(utc_offset_minutes), false)
}

pub fn project_all_task_rows(rows: &[AllTaskRow]) -> Vec<ListRowViewModel> {
    project_visible_all_task_rows(rows, "", usize::MAX).rows
}

pub fn project_visible_all_task_rows(
    rows: &[AllTaskRow],
    filter: &str,
    visible_limit: usize,
) -> VisibleAllTaskRows {
    project_visible_all_task_rows_with_timezone(rows, filter, visible_limit, None)
}

pub(crate) fn project_visible_all_task_rows_with_timezone(
    rows: &[AllTaskRow],
    filter: &str,
    visible_limit: usize,
    offset_at: Option<&dyn Fn(i64) -> Option<i32>>,
) -> VisibleAllTaskRows {
    project_ordered_all_task_rows(rows.iter(), rows.len(), filter, visible_limit, offset_at)
}

#[cfg(all(feature = "web", test))]
pub(crate) fn project_all_tasks_by_deviation(
    rows: &[AllTaskRow],
    filter: &str,
    visible_limit: usize,
) -> VisibleAllTaskRows {
    project_all_tasks_by_deviation_with_timezone(rows, filter, visible_limit, None)
}

#[cfg(all(feature = "web", any(test, target_arch = "wasm32")))]
pub(crate) fn project_all_tasks_by_deviation_with_timezone(
    rows: &[AllTaskRow],
    filter: &str,
    visible_limit: usize,
    offset_at: Option<&dyn Fn(i64) -> Option<i32>>,
) -> VisibleAllTaskRows {
    let mut matching = rows
        .iter()
        .filter(|row| task_name_matches(filter, &row.task.task_name))
        .collect::<Vec<_>>();
    matching.sort_by_key(|row| std::cmp::Reverse(row.task.duration_stddev_seconds));
    let count = matching.len();
    project_ordered_all_task_rows(matching, count, "", visible_limit, offset_at)
}

fn project_ordered_all_task_rows<'a>(
    rows: impl IntoIterator<Item = &'a AllTaskRow>,
    row_count: usize,
    filter: &str,
    visible_limit: usize,
    offset_at: Option<&dyn Fn(i64) -> Option<i32>>,
) -> VisibleAllTaskRows {
    let mut previous_date = None;
    let mut projected_rows = Vec::with_capacity(visible_limit.min(row_count));
    let mut has_more = false;
    let mut identities = HashSet::new();
    let mut matching_task_count = 0;
    for row in rows {
        if !task_name_matches(filter, &row.task.task_name) {
            continue;
        }
        if task_count_identity(&row.task, &row.occurrence)
            .is_none_or(|identity| identities.insert(identity))
        {
            matching_task_count += 1;
        }
        if projected_rows.len() == visible_limit {
            has_more = true;
            continue;
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

        projected_rows.push(project_all_task_row(
            row,
            gap_before,
            date_boundary_before,
            deadline_time(row.deadline_epoch_ms, current_date, offset_at),
        ));
    }
    VisibleAllTaskRows {
        rows: projected_rows,
        has_more,
        matching_task_count,
    }
}

pub fn count_unique_list_tasks(rows: &[ListRowViewModel]) -> usize {
    let mut identities = HashSet::new();
    rows.iter()
        .filter(|row| {
            task_count_identity(&row.task, &row.occurrence)
                .is_none_or(|identity| identities.insert(identity))
        })
        .count()
}

fn task_count_identity<'a>(
    task: &'a ScheduledTask,
    occurrence: &'a ScheduleOccurrence,
) -> Option<(bool, &'a str)> {
    match occurrence {
        ScheduleOccurrence::Projected { occurrence_key, .. } => Some((true, occurrence_key)),
        ScheduleOccurrence::Actual { task_id } => Some((false, task_id)),
        ScheduleOccurrence::LegacyActual => task.task_id.as_deref().map(|id| (false, id)),
    }
}

pub fn task_name_matches(filter: &str, task_name: &str) -> bool {
    let filter = filter.trim();
    if filter.is_empty() {
        return true;
    }

    let filter = normalize_task_search_text(filter);
    let task_name = normalize_task_search_text(task_name);
    let filter: Vec<_> = task_search_chars(&filter).collect();
    let task_name: Vec<_> = task_search_chars(&task_name).collect();
    task_name.windows(filter.len()).any(|window| {
        filter
            .iter()
            .zip(window)
            .all(|(&(filter_base, filter_mark), &(task_base, task_mark))| {
                filter_base == task_base && filter_mark.is_none_or(|mark| task_mark == Some(mark))
            })
    })
}

fn task_search_chars(text: &str) -> impl Iterator<Item = (char, Option<char>)> + '_ {
    let mut chars = text.chars().peekable();
    std::iter::from_fn(move || {
        let ch = chars.next()?;
        let (base, mark) = match ch {
            'ゔ' => ('う', Some('\u{3099}')),
            'が' | 'ぎ' | 'ぐ' | 'げ' | 'ご' | 'ざ' | 'じ' | 'ず' | 'ぜ' | 'ぞ' | 'だ' | 'ぢ'
            | 'づ' | 'で' | 'ど' | 'ば' | 'び' | 'ぶ' | 'べ' | 'ぼ' | 'ゞ' => {
                (char::from_u32(ch as u32 - 1).unwrap(), Some('\u{3099}'))
            }
            'ぱ' | 'ぴ' | 'ぷ' | 'ぺ' | 'ぽ' => {
                (char::from_u32(ch as u32 - 2).unwrap(), Some('\u{309A}'))
            }
            // Kana such as わ with a combining mark have no precomposed hiragana.
            'ぁ'..='ゖ' | 'ゝ' => (
                ch,
                chars.next_if(|mark| matches!(mark, '\u{3099}' | '\u{309A}')),
            ),
            _ => (ch, None),
        };
        Some((base, mark))
    })
}

fn normalize_task_search_text(text: &str) -> String {
    // Full-width equivalents of U+FF66 through U+FF9F, in code point order.
    const HALFWIDTH_KANA: &str =
        "ヲァィゥェォャュョッーアイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワン\u{3099}\u{309A}";

    let lowercase = text.to_lowercase();
    let mut kana = lowercase
        .chars()
        .map(|ch| match ch {
            'ｦ'..='ﾟ' => HALFWIDTH_KANA
                .chars()
                .nth((ch as u32 - 'ｦ' as u32) as usize)
                .expect("half-width kana table covers U+FF66 through U+FF9F"),
            _ => ch,
        })
        .peekable();
    let mut normalized = String::with_capacity(lowercase.len());
    while let Some(ch) = kana.next() {
        let hiragana = match ch {
            'ァ'..='ヶ' | 'ヽ'..='ヾ' => char::from_u32(ch as u32 - 0x60).unwrap(),
            'ヷ'..='ヺ' => {
                normalized.push(['わ', 'ゐ', 'ゑ', 'を'][(ch as u32 - 'ヷ' as u32) as usize]);
                normalized.push('\u{3099}');
                continue;
            }
            _ => ch,
        };
        let composed = match (hiragana, kana.peek().copied()) {
            ('う', Some('\u{3099}')) => Some('ゔ'),
            (base, Some('\u{3099}'))
                if "かきくけこさしすせそたちつてとはひふへほゝ".contains(base) =>
            {
                char::from_u32(base as u32 + 1)
            }
            (base, Some('\u{309A}')) if "はひふへほ".contains(base) => {
                char::from_u32(base as u32 + 2)
            }
            _ => None,
        };
        if let Some(composed) = composed {
            kana.next();
            normalized.push(composed);
        } else {
            normalized.push(hiragana);
        }
    }
    normalized
}

fn project_all_task_row(
    row: &AllTaskRow,
    gap_before: Option<String>,
    date_boundary_before: bool,
    deadline_time: Option<String>,
) -> ListRowViewModel {
    ListRowViewModel {
        row_key: format!(
            "all:{}:{}:{}",
            row.occurrence
                .occurrence_key()
                .or(row.task.task_id.as_deref())
                .unwrap_or("legacy-actual"),
            row.schedule_date,
            row.segment_index
        ),
        task: row.task.clone(),
        display_task_name: row
            .display_task_name
            .clone()
            .unwrap_or_else(|| row.task.task_name.clone()),
        advanceable_days: row.advanceable_days,
        occurrence: row.occurrence.clone(),
        deadline_label: row.deadline_label.clone(),
        deadline_time,
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
pub(crate) fn project_session_cards_for_display(
    state: &ClientState,
    offset_at: &dyn Fn(i64) -> Option<i32>,
) -> Vec<SessionCardViewModel> {
    project_session_cards_with(state, offset_at, |_| Some(JAPAN_UTC_OFFSET_MINUTES))
}

#[cfg(any(test, feature = "web"))]
pub(crate) fn project_list_rows_for_display(
    state: &ClientState,
    offset_at: &dyn Fn(i64) -> Option<i32>,
) -> Vec<ListRowViewModel> {
    project_list_rows_with(state, offset_at, true)
}

fn project_session_cards_with(
    state: &ClientState,
    offset_at: impl Fn(i64) -> Option<i32>,
    logical_offset_at: impl Fn(i64) -> Option<i32>,
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
                future_pending_date_label: logical_date_with_offset(
                    state.tick_now_epoch_ms(),
                    &logical_offset_at,
                )
                .and_then(|today| {
                    let [start, pending] = session_plan_dates(
                        session.start_time_epoch_ms,
                        session.pending_until_epoch_ms,
                        &logical_offset_at,
                    );
                    if let Some(start) = start.filter(|start| *start > today) {
                        return Some(format!("{} 開始可能", start.format("%Y/%m/%d")));
                    }
                    pending
                        .filter(|pending| *pending > today)
                        .map(|pending| format!("{} 着手予定", pending.format("%Y/%m/%d")))
                }),
                task_id: session.task_id.clone(),
                task_name: session.task_name.clone(),
                started_at_hh_mm: format_with_offset_provider(
                    session.started_at_epoch_ms,
                    &offset_at,
                ),
                completion_hh_mm: completion_label(server_committed, timing, &offset_at),
                actual_work_seconds_at_start: session.actual_work_seconds_at_start,
                estimate_success_percentage: session.estimate_success_percentage,
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
    offset_at: &dyn Fn(i64) -> Option<i32>,
    use_display_timezone: bool,
) -> Vec<ListRowViewModel> {
    let list_date = state
        .selected_logical_date()
        .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok());
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
            let defer_confirmation = match row.defer_plan.as_ref().map(|plan| plan.mode) {
                Some(DeferMode::Normal) | None => None,
                Some(DeferMode::DeadlineLimited) => Some(DeferConfirmationViewModel {
                    kind: DeferConfirmationKind::DeadlineLimited,
                    detail_label: row
                        .defer_plan
                        .as_ref()
                        .and_then(|plan| plan.effective_pending_until_epoch_ms)
                        .and_then(|epoch_ms| {
                            offset_at(epoch_ms)
                                .map(|offset| format_local_month_day_hh_mm(epoch_ms, offset))
                        })
                        .unwrap_or_else(|| INVALID_TIME.to_owned()),
                }),
                Some(DeferMode::RoutinePeriod) => Some(DeferConfirmationViewModel {
                    kind: DeferConfirmationKind::RoutinePeriod,
                    detail_label: row
                        .defer_plan
                        .as_ref()
                        .and_then(|plan| plan.repetition_interval_days)
                        .map(|days| format!("{days}日"))
                        .unwrap_or_else(|| INVALID_TIME.to_owned()),
                }),
            };
            ListRowViewModel {
                row_key: format!(
                    "{}:{}:{}",
                    row.occurrence
                        .occurrence_key()
                        .or(row.task.task_id.as_deref())
                        .unwrap_or("legacy-actual"),
                    row.schedule_start_epoch_ms,
                    row.schedule_end_epoch_ms
                ),
                task: row.task.clone(),
                display_task_name: row
                    .display_task_name
                    .clone()
                    .unwrap_or_else(|| row.task.task_name.clone()),
                advanceable_days: row.advanceable_days,
                occurrence: row.occurrence.clone(),
                deadline_label: row.deadline_label.clone(),
                deadline_time: deadline_time(
                    row.deadline_epoch_ms,
                    list_date,
                    use_display_timezone.then_some(offset_at),
                ),
                schedule_display: ScheduleDisplayViewModel::Daily {
                    start_hh_mm: if use_display_timezone {
                        display_time_with_offset(row.schedule_start_epoch_ms, list_date, offset_at)
                            .unwrap_or_else(|| INVALID_TIME.to_owned())
                    } else {
                        format_with_offset_provider(row.schedule_start_epoch_ms, &offset_at)
                    },
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
                defer_plan: row.defer_plan.clone(),
                defer_confirmation,
            }
        })
        .collect()
}

fn deadline_time(
    epoch_ms: Option<i64>,
    list_date: Option<NaiveDate>,
    offset_at: Option<&dyn Fn(i64) -> Option<i32>>,
) -> Option<String> {
    display_time_with_offset(epoch_ms?, list_date, offset_at?)
}

fn display_time_with_offset(
    epoch_ms: i64,
    list_date: Option<NaiveDate>,
    offset_at: &dyn Fn(i64) -> Option<i32>,
) -> Option<String> {
    format_display_time(epoch_ms, offset_at(epoch_ms)?, list_date)
}

pub(crate) fn format_display_time(
    epoch_ms: i64,
    offset_minutes: i32,
    list_date: Option<NaiveDate>,
) -> Option<String> {
    let datetime = local_datetime(epoch_ms, offset_minutes)?;
    let datetime = datetime
        .naive_utc()
        .checked_add_offset(*datetime.offset())?;
    let format = if list_date.is_some_and(|date| date != datetime.date()) {
        "%m/%d %H:%M"
    } else {
        "%H:%M"
    };
    Some(datetime.format(format).to_string())
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

#[cfg(test)]
mod tests {
    use super::{format_display_time, normalize_task_search_text, task_name_matches};
    use chrono::{DateTime, NaiveDate};

    #[test]
    fn 時刻だけを変換し日本時間の日付と異なる表示には月日を添える() {
        let epoch = DateTime::parse_from_rfc3339("2026-11-15T09:00:00+09:00")
            .unwrap()
            .timestamp_millis();
        let list_date = NaiveDate::from_ymd_opt(2026, 11, 15);
        assert_eq!(
            format_display_time(epoch, 540, list_date).as_deref(),
            Some("09:00")
        );
        assert_eq!(
            format_display_time(epoch, -480, list_date).as_deref(),
            Some("11/14 16:00")
        );
        assert_eq!(
            format_display_time(epoch, 330, list_date).as_deref(),
            Some("05:30")
        );
        assert_eq!(format_display_time(i64::MAX, 540, list_date), None);
    }

    #[test]
    fn normalizes_kana_search_text() {
        for (input, expected) in [
            ("資料れびゅー2", "資料れびゅー2"),
            ("資料レビュー2", "資料れびゅー2"),
            ("資料ﾚﾋﾞｭｰ2", "資料れびゅー2"),
            ("がぱゔ", "がぱゔ"),
            ("ガパヴ", "がぱゔ"),
            ("ｶﾞﾊﾟｳﾞ", "がぱゔ"),
            ("カ\u{3099}ハ\u{309A}ウ\u{3099}", "がぱゔ"),
            ("か\u{3099}は\u{309A}う\u{3099}", "がぱゔ"),
            ("ヷヸヹヺヾ", "わ\u{3099}ゐ\u{3099}ゑ\u{3099}を\u{3099}ゞ"),
            ("ヲンヵヶヽ", "をんゕゖゝ"),
            ("確認2２、､。｡・･ABC", "確認2２、､。｡・･abc"),
            ("カガハパュユｰ", "かがはぱゅゆー"),
        ] {
            assert_eq!(normalize_task_search_text(input), expected, "{input:?}");
        }
    }

    #[test]
    fn search_allows_omitted_kana_marks_at_each_position() {
        for (filter, name) in [
            ("めか", "メガネ"),
            ("めか", "メカ"),
            ("は", "バ"),
            ("は", "ﾊﾟ"),
            ("わ", "ヷ"),
            ("めかぱ", "メガパ"),
            ("めがは", "ﾒｶﾞﾊﾟ"),
            ("わ\u{3099}", "ヷ"),
            ("めか\u{3099}", "メガネ"),
            ("かか", "カガカ"),
        ] {
            assert!(task_name_matches(filter, name), "{filter:?} in {name:?}");
        }
    }

    #[test]
    fn search_keeps_kana_and_non_kana_distinctions() {
        for (filter, name) in [
            ("めが", "メカ"),
            ("ば", "ハ"),
            ("ば", "パ"),
            ("ぱ", "バ"),
            ("わ\u{3099}", "ワ"),
            ("めかぱ", "メガバ"),
            ("めか\u{3099}", "メカ"),
            ("ゆ", "ュ"),
            ("こー", "コ"),
            ("確認２", "確認2"),
            ("か、", "カ､"),
            ("かくにん", "確認"),
        ] {
            assert!(!task_name_matches(filter, name), "{filter:?} in {name:?}");
        }
    }
}
