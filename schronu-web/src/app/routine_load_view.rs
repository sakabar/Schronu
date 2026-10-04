#![cfg_attr(not(all(feature = "web", target_arch = "wasm32")), allow(dead_code))]

use crate::{RoutineLoadReport, RoutineLoadRow};
use chrono::Datelike;
use dioxus::prelude::*;

#[component]
pub(super) fn RoutineLoadTable(report: Option<RoutineLoadReport>, loading: bool) -> Element {
    let Some(report) = report else {
        return if loading {
            rsx! { p { class: "load-status", role: "status", "繰返負荷を取得しています…" } }
        } else {
            rsx! { p { class: "load-status", "繰返負荷は未取得です。" } }
        };
    };
    let range = format!(
        "{}〜{}",
        format_short_date(&report.start_date),
        format_short_date(&report.end_date)
    );
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
                            RoutineLoadTableRow { row }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn RoutineLoadTableRow(row: RoutineLoadRow) -> Element {
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

fn format_short_date(value: &str) -> String {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(|date| format!("{}/{}", date.month(), date.day()))
        .unwrap_or_else(|_| value.to_owned())
}

fn format_unsigned(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}
