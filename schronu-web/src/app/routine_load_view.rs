#![cfg_attr(not(all(feature = "web", target_arch = "wasm32")), allow(dead_code))]

use super::load_view_format::format_unsigned;
use crate::{RoutineLoadReport, RoutineLoadRow};
use chrono::{Datelike, Duration, NaiveDate};
use dioxus::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

const HORIZON_DAY_COUNT: usize = 28;
const WEEKDAY_LABELS: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RoutineLoadScope {
    Overall,
    Weekday(usize),
    Date(NaiveDate),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DailyLoadProjection {
    pub(super) date: NaiveDate,
    pub(super) work_seconds: i64,
    pub(super) available_seconds: i64,
    pub(super) difference_seconds: i128,
    pub(super) percentage: Option<i128>,
    pub(super) heat_percentage: u8,
    uses_remaining_capacity: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct WeekdayLoadProjection {
    pub(super) work_seconds: i64,
    pub(super) available_seconds: i64,
    pub(super) percentage: Option<i128>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProjectedRoutine {
    row: RoutineLoadRow,
    work_seconds_by_date: BTreeMap<NaiveDate, i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RoutineLoadProjection {
    pub(super) days: Vec<DailyLoadProjection>,
    pub(super) weekdays: Vec<WeekdayLoadProjection>,
    pub(super) calendar_cells: Vec<Option<DailyLoadProjection>>,
    routines: Vec<ProjectedRoutine>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ScopeRoutineRow {
    pub(super) routine: RoutineLoadRow,
    pub(super) work_seconds: i64,
    pub(super) average_work_seconds: Option<i64>,
    pub(super) percentage: Option<i128>,
    pub(super) occurrence_count: usize,
}

#[component]
pub(super) fn RoutineLoadTable(
    report: Option<RoutineLoadReport>,
    loading: bool,
    #[props(default)] today_remaining_available_seconds: Option<i64>,
) -> Element {
    let mut scope = use_signal(|| RoutineLoadScope::Overall);
    let mut selected_routine_id = use_signal(|| None::<String>);
    let Some(report) = report else {
        return if loading {
            rsx! { p { class: "load-status", role: "status", "繰返負荷を取得しています…" } }
        } else {
            rsx! { p { class: "load-status", "繰返負荷は未取得です。" } }
        };
    };
    let Some(projection) =
        build_routine_load_projection(&report, today_remaining_available_seconds)
    else {
        return rsx! { LegacyRoutineLoadTable { report } };
    };

    let active_scope = scope();
    let selected_id = selected_routine_id();
    let highlighted = highlighted_dates(&projection, selected_id.as_deref());
    let rows = rows_for_scope(&projection, active_scope);
    let range = report_range(&report);
    let scope_label = match active_scope {
        RoutineLoadScope::Overall => "全体".to_owned(),
        RoutineLoadScope::Weekday(index) => format!("{}曜日", WEEKDAY_LABELS[index]),
        RoutineLoadScope::Date(date) => format!("{}/{}", date.month(), date.day()),
    };

    rsx! {
        div { class: "routine-load-summary",
            strong { "{range}" }
            span { "{report.rows.len()}件の繰返" }
        }
        div { class: "routine-load-scope-controls", role: "group", aria_label: "範囲選択",
            button {
                class: if active_scope == RoutineLoadScope::Overall { "routine-load-overall is-selected" } else { "routine-load-overall" },
                r#type: "button",
                aria_pressed: active_scope == RoutineLoadScope::Overall,
                onclick: move |_| {
                    scope.set(RoutineLoadScope::Overall);
                    selected_routine_id.set(None);
                },
                "全体"
            }
        }
        div { class: "routine-load-map",
            for (index, weekday) in projection.weekdays.iter().enumerate() {
                {
                    let percentage = format_percentage(weekday.percentage);
                    let class = load_cell_class(
                        "routine-load-weekday",
                        active_scope == RoutineLoadScope::Weekday(index),
                        false,
                        is_at_capacity(weekday.percentage),
                    );
                    rsx! {
                        button {
                            class: "{class}",
                            r#type: "button",
                            aria_pressed: active_scope == RoutineLoadScope::Weekday(index),
                            aria_label: "{WEEKDAY_LABELS[index]}曜日、可処分時間比 {percentage}",
                            onclick: move |_| {
                                scope.set(RoutineLoadScope::Weekday(index));
                                selected_routine_id.set(None);
                            },
                            strong { "{WEEKDAY_LABELS[index]}" }
                            span { "{percentage}" }
                        }
                    }
                }
            }
            for cell in projection.calendar_cells.iter() {
                if let Some(day) = cell {
                    {
                        let day = day.clone();
                        let date = day.date;
                        let date_label = format!("{}年{}月{}日", date.year(), date.month(), date.day());
                        let difference_label = format_signed_time(day.difference_seconds);
                        let percentage = format_percentage(day.percentage);
                        let percentage_label = if day.uses_remaining_capacity {
                            "残り可処分時間比"
                        } else {
                            "可処分時間比"
                        };
                        let selected = active_scope == RoutineLoadScope::Date(date);
                        let emphasized = highlighted.contains(&date);
                        let class = load_cell_class(
                            "routine-load-day",
                            selected,
                            emphasized,
                            is_day_at_capacity(&day),
                        );
                        let aria_label = format!(
                            "{date_label}、繰返負荷との差 {difference_label}、{percentage_label} {percentage}、{}",
                            if selected { "選択中" } else { "未選択" }
                        );
                        rsx! {
                            button {
                                class: "{class}",
                                r#type: "button",
                                aria_pressed: selected,
                                aria_label: "{aria_label}",
                                style: "--routine-load-intensity: {day.heat_percentage};",
                                onclick: move |_| {
                                    scope.set(RoutineLoadScope::Date(date));
                                    selected_routine_id.set(None);
                                },
                                strong { "{date.day()}" }
                                span { "{difference_label}" }
                                span { "{percentage}" }
                            }
                        }
                    }
                } else {
                    div { class: "routine-load-day-empty", aria_hidden: "true" }
                }
            }
        }
        div { class: "routine-load-selection-summary",
            strong { "{scope_label}" }
            span { "削減候補 {rows.len()}件" }
        }
        if rows.is_empty() {
            p { class: "load-status", "選択範囲に発生する繰返負荷はありません。" }
        } else {
            ScopedRoutineLoadTable {
                rows,
                scope: active_scope,
                uses_remaining_capacity: matches!(
                    active_scope,
                    RoutineLoadScope::Date(date)
                        if projection.days.iter().any(|day| {
                            day.date == date && day.uses_remaining_capacity
                        })
                ),
                selected_routine_id: selected_id,
                on_select: move |routine_id: String| {
                    selected_routine_id.set(toggle_routine_selection(
                        selected_routine_id().as_deref(),
                        &routine_id,
                    ));
                },
            }
        }
    }
}

#[component]
fn ScopedRoutineLoadTable(
    rows: Vec<ScopeRoutineRow>,
    scope: RoutineLoadScope,
    uses_remaining_capacity: bool,
    selected_routine_id: Option<String>,
    on_select: EventHandler<String>,
) -> Element {
    rsx! {
        div { class: "routine-load-table-wrap",
            table { class: "routine-load-table",
                thead {
                    tr {
                        th { class: "routine-load-interval", scope: "col", "間隔" }
                        match scope {
                            RoutineLoadScope::Overall => rsx! {
                                th { class: "routine-load-total", scope: "col", "28日合計" }
                                th { class: "routine-load-occurrences", scope: "col", "発生日数" }
                                th { class: "routine-load-average", scope: "col", "1日平均" }
                            },
                            RoutineLoadScope::Weekday(_) => rsx! {
                                th { class: "routine-load-average", scope: "col", "曜日平均" }
                                th { class: "routine-load-total", scope: "col", "可処分時間比" }
                                th { class: "routine-load-occurrences", scope: "col", "発生" }
                            },
                            RoutineLoadScope::Date(_) => rsx! {
                                th { class: "routine-load-average", scope: "col", "当日時間" }
                                th { class: "routine-load-total", scope: "col",
                                    if uses_remaining_capacity {
                                        "残り可処分時間比"
                                    } else {
                                        "可処分時間比"
                                    }
                                }
                                th { class: "routine-load-occurrences", scope: "col", "28日合計" }
                            },
                        }
                        th { class: "routine-load-subject", scope: "col", "プロジェクト / 繰返" }
                    }
                }
                tbody {
                    for display_row in rows {
                        {
                            let routine_id = display_row.routine.routine_task_id.clone();
                            let is_selected = selected_routine_id.as_deref() == Some(&routine_id);
                            rsx! {
                                tr { class: if is_selected { "is-selected" } else { "" },
                                    td { class: "routine-load-interval", "{display_row.routine.repetition_interval_days}日" }
                                    match scope {
                                        RoutineLoadScope::Overall => rsx! {
                                            td { class: "routine-load-total routine-load-number", "{format_unsigned(display_row.routine.total_work_seconds)}" }
                                            td { class: "routine-load-occurrences routine-load-number", "{display_row.routine.occurrence_day_count}日" }
                                            td { class: "routine-load-average routine-load-number", "{format_optional_time(display_row.average_work_seconds)}" }
                                        },
                                        RoutineLoadScope::Weekday(_) => rsx! {
                                            td { class: "routine-load-average routine-load-number", "{format_optional_time(display_row.average_work_seconds)}" }
                                            td { class: "routine-load-total routine-load-number", "{format_percentage(display_row.percentage)}" }
                                            td { class: "routine-load-occurrences routine-load-number", "4回中{display_row.occurrence_count}回" }
                                        },
                                        RoutineLoadScope::Date(_) => rsx! {
                                            td { class: "routine-load-average routine-load-number", "{format_unsigned(display_row.work_seconds)}" }
                                            td { class: "routine-load-total routine-load-number", "{format_percentage(display_row.percentage)}" }
                                            td { class: "routine-load-occurrences routine-load-number", "{format_unsigned(display_row.routine.total_work_seconds)}" }
                                        },
                                    }
                                    th { class: "routine-load-subject", scope: "row",
                                        button {
                                            class: "routine-load-subject-button",
                                            r#type: "button",
                                            aria_pressed: is_selected,
                                            aria_label: "{display_row.routine.project_name} / {display_row.routine.routine_name}を強調",
                                            onclick: move |_| on_select.call(routine_id.clone()),
                                            strong { class: "routine-load-name task-kind-repetitive", "{display_row.routine.routine_name}" }
                                            span { class: "routine-load-project", "{display_row.routine.project_name}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn LegacyRoutineLoadTable(report: RoutineLoadReport) -> Element {
    let range = report_range(&report);
    let total_heading = format!("{}日合計", report.horizon_day_count);
    let end_offset = report.horizon_day_count.saturating_sub(1);
    rsx! {
        div { class: "routine-load-summary",
            strong { "{range}" }
            span { "{report.rows.len()}件の繰返" }
        }
        if report.rows.is_empty() {
            p { class: "load-status", "今日から{end_offset}日後までに発生する繰返負荷はありません。" }
        } else {
            div { class: "routine-load-table-wrap",
                table { class: "routine-load-table",
                    thead {
                        tr {
                            th { class: "routine-load-interval", scope: "col", "間隔" }
                            th { class: "routine-load-total", scope: "col", "{total_heading}" }
                            th { class: "routine-load-occurrences", scope: "col", "発生日数" }
                            th { class: "routine-load-average", scope: "col", "1日平均" }
                            th { class: "routine-load-subject", scope: "col", "プロジェクト / 繰返" }
                        }
                    }
                    tbody {
                        for row in report.rows {
                            LegacyRoutineLoadTableRow { row }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn LegacyRoutineLoadTableRow(row: RoutineLoadRow) -> Element {
    rsx! {
        tr {
            td { class: "routine-load-interval", "{row.repetition_interval_days}日" }
            td { class: "routine-load-total routine-load-number", "{format_unsigned(row.total_work_seconds)}" }
            td { class: "routine-load-occurrences routine-load-number", "{row.occurrence_day_count}日" }
            td { class: "routine-load-average routine-load-number",
                if let Some(average_work_seconds) = row.display_average_work_seconds() {
                    "{format_unsigned(average_work_seconds)}"
                } else {
                    "--:--"
                }
            }
            th { class: "routine-load-subject", scope: "row",
                div { class: "routine-load-subject-scroll", tabindex: 0,
                    strong { class: "routine-load-name task-kind-repetitive", "{row.routine_name}" }
                    span { class: "routine-load-project", "{row.project_name}" }
                }
            }
        }
    }
}

pub(super) fn build_routine_load_projection(
    report: &RoutineLoadReport,
    today_remaining_available_seconds: Option<i64>,
) -> Option<RoutineLoadProjection> {
    if report.horizon_day_count != HORIZON_DAY_COUNT as u64 {
        return None;
    }
    let start_date = parse_date(&report.start_date)?;
    let expected_end_date = start_date.checked_add_signed(Duration::days(27))?;
    if parse_date(&report.end_date)? != expected_end_date
        || report.full_day_available_seconds_by_date.len() != HORIZON_DAY_COUNT
    {
        return None;
    }

    let expected_dates = (0..HORIZON_DAY_COUNT)
        .map(|offset| start_date.checked_add_signed(Duration::days(offset as i64)))
        .collect::<Option<Vec<_>>>()?;
    let available_by_date = expected_dates
        .iter()
        .map(|date| {
            let seconds = *report
                .full_day_available_seconds_by_date
                .get(&date.to_string())?;
            (seconds >= 0).then_some((*date, seconds))
        })
        .collect::<Option<BTreeMap<_, _>>>()?;

    let routines = report
        .rows
        .iter()
        .map(|row| project_routine(row, start_date, expected_end_date))
        .collect::<Option<Vec<_>>>()?;
    let today_remaining_available_seconds =
        today_remaining_available_seconds.filter(|seconds| *seconds >= 0);
    let days = expected_dates
        .iter()
        .map(|date| {
            let work_seconds = routines.iter().try_fold(0_i64, |total, routine| {
                total.checked_add(
                    routine
                        .work_seconds_by_date
                        .get(date)
                        .copied()
                        .unwrap_or_default(),
                )
            })?;
            let uses_remaining_capacity =
                *date == start_date && today_remaining_available_seconds.is_some();
            let available_seconds = if uses_remaining_capacity {
                today_remaining_available_seconds?
            } else {
                available_by_date[date]
            };
            let percentage = rounded_percentage(work_seconds, available_seconds);
            let difference_seconds = i128::from(work_seconds) - i128::from(available_seconds);
            Some(DailyLoadProjection {
                date: *date,
                work_seconds,
                available_seconds,
                difference_seconds,
                percentage,
                heat_percentage: heat_percentage(percentage),
                uses_remaining_capacity,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let weekdays = (0..7)
        .map(|weekday_index| {
            let mut matching_days = days
                .iter()
                .filter(|day| day.date.weekday().num_days_from_monday() as usize == weekday_index);
            let (work_seconds, available_seconds) =
                matching_days.try_fold((0_i64, 0_i64), |(work_total, available_total), day| {
                    Some((
                        work_total.checked_add(day.work_seconds)?,
                        available_total.checked_add(day.available_seconds)?,
                    ))
                })?;
            Some(WeekdayLoadProjection {
                work_seconds,
                available_seconds,
                percentage: rounded_percentage(work_seconds, available_seconds),
            })
        })
        .collect::<Option<Vec<_>>>()?;

    let leading_blanks = start_date.weekday().num_days_from_monday() as usize;
    let mut calendar_cells = vec![None; leading_blanks];
    calendar_cells.extend(days.iter().cloned().map(Some));
    let trailing_blanks = (7 - calendar_cells.len() % 7) % 7;
    calendar_cells.extend(std::iter::repeat_n(None, trailing_blanks));

    Some(RoutineLoadProjection {
        days,
        weekdays,
        calendar_cells,
        routines,
    })
}

fn project_routine(
    row: &RoutineLoadRow,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Option<ProjectedRoutine> {
    if row.work_seconds_by_date.is_empty() {
        return None;
    }
    let work_seconds_by_date = row
        .work_seconds_by_date
        .iter()
        .map(|(date, seconds)| {
            let date = parse_date(date)?;
            (*seconds >= 0 && date >= start_date && date <= end_date).then_some((date, *seconds))
        })
        .collect::<Option<BTreeMap<_, _>>>()?;
    let total = work_seconds_by_date
        .values()
        .try_fold(0_i64, |total, seconds| total.checked_add(*seconds))?;
    if total != row.total_work_seconds || work_seconds_by_date.len() != row.occurrence_day_count {
        return None;
    }
    Some(ProjectedRoutine {
        row: row.clone(),
        work_seconds_by_date,
    })
}

pub(super) fn rows_for_scope(
    projection: &RoutineLoadProjection,
    scope: RoutineLoadScope,
) -> Vec<ScopeRoutineRow> {
    let mut rows = projection
        .routines
        .iter()
        .filter_map(|routine| match scope {
            RoutineLoadScope::Overall => Some(ScopeRoutineRow {
                routine: routine.row.clone(),
                work_seconds: routine.row.total_work_seconds,
                average_work_seconds: routine.row.display_average_work_seconds(),
                percentage: None,
                occurrence_count: routine.row.occurrence_day_count,
            }),
            RoutineLoadScope::Weekday(weekday_index) => {
                let matching = routine.work_seconds_by_date.iter().filter(|(date, _)| {
                    date.weekday().num_days_from_monday() as usize == weekday_index
                });
                let (work_seconds, occurrence_count) = matching
                    .fold((0_i64, 0_usize), |(total, count), (_, seconds)| {
                        (total.saturating_add(*seconds), count + 1)
                    });
                (occurrence_count > 0).then(|| ScopeRoutineRow {
                    routine: routine.row.clone(),
                    work_seconds,
                    average_work_seconds: work_seconds.checked_div(4),
                    percentage: projection.weekdays.get(weekday_index).and_then(|weekday| {
                        rounded_percentage(work_seconds, weekday.available_seconds)
                    }),
                    occurrence_count,
                })
            }
            RoutineLoadScope::Date(date) => {
                let work_seconds = routine.work_seconds_by_date.get(&date).copied()?;
                let available_seconds = projection
                    .days
                    .iter()
                    .find(|day| day.date == date)?
                    .available_seconds;
                Some(ScopeRoutineRow {
                    routine: routine.row.clone(),
                    work_seconds,
                    average_work_seconds: None,
                    percentage: rounded_percentage(work_seconds, available_seconds),
                    occurrence_count: 1,
                })
            }
        })
        .collect::<Vec<_>>();
    if scope != RoutineLoadScope::Overall {
        rows.sort_by(|left, right| {
            right
                .work_seconds
                .cmp(&left.work_seconds)
                .then_with(|| left.routine.project_name.cmp(&right.routine.project_name))
                .then_with(|| left.routine.routine_name.cmp(&right.routine.routine_name))
                .then_with(|| {
                    left.routine
                        .routine_task_id
                        .cmp(&right.routine.routine_task_id)
                })
                .then_with(|| {
                    left.routine
                        .project_task_id
                        .cmp(&right.routine.project_task_id)
                })
        });
    }
    rows
}

pub(super) fn toggle_routine_selection(current: Option<&str>, clicked: &str) -> Option<String> {
    (current != Some(clicked)).then(|| clicked.to_owned())
}

pub(super) fn highlighted_dates(
    projection: &RoutineLoadProjection,
    selected_routine_id: Option<&str>,
) -> BTreeSet<NaiveDate> {
    let Some(selected_routine_id) = selected_routine_id else {
        return BTreeSet::new();
    };
    projection
        .routines
        .iter()
        .find(|routine| routine.row.routine_task_id == selected_routine_id)
        .map(|routine| routine.work_seconds_by_date.keys().copied().collect())
        .unwrap_or_default()
}

fn rounded_percentage(work_seconds: i64, available_seconds: i64) -> Option<i128> {
    if work_seconds < 0 || available_seconds <= 0 {
        return None;
    }
    let work_seconds = i128::from(work_seconds);
    let available_seconds = i128::from(available_seconds);
    work_seconds
        .checked_mul(100)?
        .checked_add(available_seconds / 2)?
        .checked_div(available_seconds)
}

fn heat_percentage(percentage: Option<i128>) -> u8 {
    percentage
        .unwrap_or_default()
        .clamp(0, 100)
        .try_into()
        .unwrap_or_default()
}

fn format_percentage(percentage: Option<i128>) -> String {
    percentage
        .map(|percentage| format!("{percentage}%"))
        .unwrap_or_else(|| "--".to_owned())
}

pub(super) fn format_signed_time(seconds: i128) -> String {
    let total_minutes = seconds / 60;
    let sign = if total_minutes > 0 {
        "+"
    } else if total_minutes < 0 {
        "-"
    } else {
        ""
    };
    let minutes = total_minutes.unsigned_abs();
    format!("{sign}{}:{:02}", minutes / 60, minutes % 60)
}

fn format_optional_time(seconds: Option<i64>) -> String {
    seconds
        .map(format_unsigned)
        .unwrap_or_else(|| "--:--".to_owned())
}

fn is_at_capacity(percentage: Option<i128>) -> bool {
    percentage.is_some_and(|percentage| percentage >= 100)
}

fn is_day_at_capacity(day: &DailyLoadProjection) -> bool {
    day.available_seconds == 0 || is_at_capacity(day.percentage)
}

fn load_cell_class(base: &str, selected: bool, emphasized: bool, at_capacity: bool) -> String {
    let mut class = base.to_owned();
    for (enabled, modifier) in [
        (selected, " is-selected"),
        (emphasized, " is-emphasized"),
        (at_capacity, " is-at-capacity"),
    ] {
        if enabled {
            class.push_str(modifier);
        }
    }
    class
}

fn report_range(report: &RoutineLoadReport) -> String {
    format!(
        "{}〜{}",
        format_short_date(&report.start_date),
        format_short_date(&report.end_date)
    )
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

fn format_short_date(value: &str) -> String {
    parse_date(value)
        .map(|date| format!("{}/{}", date.month(), date.day()))
        .unwrap_or_else(|| value.to_owned())
}
