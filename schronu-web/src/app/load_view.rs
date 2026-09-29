#![cfg_attr(not(all(feature = "web", target_arch = "wasm32")), allow(dead_code))]

use crate::{BandDay, BandDurations};
use chrono::{Datelike, Local, TimeZone, Timelike, Weekday};
use dioxus::prelude::*;

const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

#[component]
pub(crate) fn LoadView(
    rows: Vec<BandDay>,
    observed_at_epoch_ms: Option<i64>,
    loading: bool,
    error: Option<String>,
    #[props(default)] server_actions_blocked: bool,
    on_refresh: EventHandler<()>,
    on_select_date: EventHandler<String>,
) -> Element {
    let view_class = if error.is_some() {
        "load-view has-error"
    } else {
        "load-view"
    };
    let updated = observed_at_epoch_ms
        .and_then(|epoch| Local.timestamp_millis_opt(epoch).single())
        .map(|datetime| {
            format!(
                "{}/{} {:02}:{:02} 更新",
                datetime.month(),
                datetime.day(),
                datetime.hour(),
                datetime.minute()
            )
        })
        .unwrap_or_else(|| "未取得".to_owned());
    rsx! {
        section { class: view_class, aria_label: "直近7日の負荷",
            div { class: "load-toolbar",
                div {
                    h2 { "直近7日の負荷" }
                    p { class: "load-updated", "{updated}" }
                }
                button {
                    class: "load-refresh",
                    r#type: "button",
                    disabled: loading || server_actions_blocked,
                    onclick: move |_| {
                        if !loading && !server_actions_blocked {
                            on_refresh.call(())
                        }
                    },
                    if loading { "更新中…" } else { "更新" }
                }
            }
            BandLegend {}
            if let Some(error) = error {
                p { class: "load-error", role: "alert", "{error}" }
            }
            if rows.is_empty() && loading {
                p { class: "load-status", role: "status", "負荷を取得しています…" }
            } else if rows.is_empty() {
                p { class: "load-status", "負荷は未取得です。" }
            } else {
                div { class: "load-days",
                    for (index, row) in rows.into_iter().enumerate() {
                        BandDayRow {
                            row,
                            today: index == 0,
                            disabled: server_actions_blocked,
                            on_select_date,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn BandLegend() -> Element {
    rsx! {
        div { class: "band-legend", aria_label: "帯の凡例",
            LegendItem { class: "is-unavailable", symbol: "#", label: "利用不可" }
            LegendItem { class: "is-elapsed", symbol: "x", label: "経過済み" }
            LegendItem { class: "is-repetitive", symbol: "=", label: "繰返" }
            LegendItem { class: "is-single", symbol: "-", label: "単発" }
            LegendItem { class: "is-leeway", symbol: ":", label: "余差" }
            LegendItem { class: "is-free", symbol: ".", label: "空き" }
            LegendItem { class: "is-overflow", symbol: ">", label: "超過" }
        }
    }
}

#[component]
fn LegendItem(class: &'static str, symbol: &'static str, label: &'static str) -> Element {
    rsx! {
        span { class: "band-legend-item",
            span { class: "band-swatch {class}", aria_hidden: "true" }
            "{symbol} {label}"
        }
    }
}

#[component]
fn BandDayRow(
    row: BandDay,
    today: bool,
    disabled: bool,
    on_select_date: EventHandler<String>,
) -> Element {
    let date = chrono::NaiveDate::parse_from_str(&row.logical_date, "%Y-%m-%d").ok();
    let date_label = date
        .map(|date| {
            format!(
                "{}/{}({})",
                date.month(),
                date.day(),
                weekday_jp(date.weekday())
            )
        })
        .unwrap_or_else(|| row.logical_date.clone());
    let segments = clipped_segments(row.durations);
    let remaining_capacity_seconds = SECONDS_PER_DAY
        .saturating_sub(segments.unavailable_seconds)
        .saturating_sub(segments.elapsed_seconds)
        .max(0);
    let used_seconds = raw_used_seconds(row.durations);
    let overflow_seconds = used_seconds.saturating_sub(SECONDS_PER_DAY);
    let remaining_aria = if today {
        format!(
            "残り枠{}、残り繰返{}、残り単発{}、残り余差{}、残り空き{}。",
            format_duration(remaining_capacity_seconds),
            format_duration(segments.repetitive_seconds),
            format_duration(segments.non_repetitive_seconds),
            format_duration(segments.rho_leeway_seconds),
            format_duration(segments.free_seconds),
        )
    } else {
        String::new()
    };
    let aria_label = format!(
        "{date_label}。利用不可{}、経過済み{}、繰返{}、単発{}、余差{}、空き{}、余差累{}、空差累{}、超過{}。{remaining_aria}押すと一覧を表示します。",
        format_duration(row.durations.unavailable_seconds),
        format_duration(row.durations.elapsed_seconds),
        format_duration(row.durations.repetitive_seconds),
        format_duration(row.durations.non_repetitive_seconds),
        format_duration(row.durations.rho_leeway_seconds),
        format_duration(segments.free_seconds),
        format_signed(row.accumulated_rho_diff_seconds),
        format_signed(row.accumulated_free_diff_seconds),
        format_duration(overflow_seconds),
    );
    let class = if today {
        "load-day is-today"
    } else {
        "load-day"
    };
    let logical_date = row.logical_date.clone();
    rsx! {
        button {
            class,
            r#type: "button",
            disabled,
            aria_label,
            onclick: move |_| {
                if !disabled {
                    on_select_date.call(logical_date.clone())
                }
            },
            span { class: "load-day-heading",
                span { class: "load-date", "{date_label}"
                    if today { span { class: "load-today", "今日" } }
                }
                span { class: "load-open-list", "一覧を見る ›" }
            }
            if today {
                span { class: "load-focus-group",
                    span { class: "load-band-caption load-focus-caption",
                        span { "残り枠 " small { "利用不可・経過済みを除外" } }
                        strong { "{format_unsigned(remaining_capacity_seconds)}" }
                    }
                    BandStack {
                        class: "load-focus-stack",
                        overflow_seconds,
                        capacity_seconds: remaining_capacity_seconds,
                        BandSegment {
                            class: "is-repetitive",
                            seconds: segments.repetitive_seconds,
                            capacity_seconds: remaining_capacity_seconds,
                        }
                        BandSegment {
                            class: "is-single",
                            seconds: segments.non_repetitive_seconds,
                            capacity_seconds: remaining_capacity_seconds,
                        }
                        BandSegment {
                            class: "is-leeway",
                            seconds: segments.rho_leeway_seconds,
                            capacity_seconds: remaining_capacity_seconds,
                        }
                        BandSegment {
                            class: "is-free",
                            seconds: segments.free_seconds,
                            capacity_seconds: remaining_capacity_seconds,
                        }
                    }
                }
                span { class: "load-band-caption load-overview-caption",
                    span { "1日全体" }
                    strong { "24:00" }
                }
            }
            BandStack {
                class: "load-overview-stack",
                overflow_seconds,
                capacity_seconds: SECONDS_PER_DAY,
                BandSegment { class: "is-unavailable", seconds: segments.unavailable_seconds, capacity_seconds: SECONDS_PER_DAY }
                BandSegment { class: "is-elapsed", seconds: segments.elapsed_seconds, capacity_seconds: SECONDS_PER_DAY }
                BandSegment { class: "is-repetitive", seconds: segments.repetitive_seconds, capacity_seconds: SECONDS_PER_DAY }
                BandSegment { class: "is-single", seconds: segments.non_repetitive_seconds, capacity_seconds: SECONDS_PER_DAY }
                BandSegment { class: "is-leeway", seconds: segments.rho_leeway_seconds, capacity_seconds: SECONDS_PER_DAY }
                BandSegment { class: "is-free", seconds: segments.free_seconds, capacity_seconds: SECONDS_PER_DAY }
            }
            span { class: "load-day-footer",
                span { class: "load-metrics",
                    span { class: "load-metric",
                        span { "余差累" }
                        strong {
                            class: if row.accumulated_rho_diff_seconds > 0 {
                                "load-metric-value is-over"
                            } else {
                                "load-metric-value is-within"
                            },
                            "{format_signed(row.accumulated_rho_diff_seconds)}"
                        }
                    }
                    span { class: "load-metric",
                        span { "空差累" }
                        strong {
                            class: if row.accumulated_free_diff_seconds > 0 {
                                "load-metric-value is-over"
                            } else {
                                "load-metric-value is-within"
                            },
                            "{format_signed(row.accumulated_free_diff_seconds)}"
                        }
                    }
                }
                if overflow_seconds > 0 {
                    span { class: "load-overflow", "超過 {format_unsigned(overflow_seconds)}" }
                }
            }
        }
    }
}

#[component]
fn BandStack(
    class: &'static str,
    overflow_seconds: i64,
    capacity_seconds: i64,
    children: Element,
) -> Element {
    let overflow_width = overflow_percentage(overflow_seconds, capacity_seconds);
    let overflow_class = if overflow_seconds > 0 {
        "load-overflow-fill"
    } else {
        "load-overflow-fill is-zero"
    };
    rsx! {
        span { class: "load-band-stack {class}",
            span { class: "load-band", aria_hidden: "true", {children} }
            span { class: "load-overflow-rail", aria_hidden: "true",
                span { class: overflow_class, style: "width:{overflow_width:.4}%" }
            }
        }
    }
}

#[component]
fn BandSegment(class: &'static str, seconds: i64, capacity_seconds: i64) -> Element {
    let width = if capacity_seconds > 0 {
        seconds.max(0).min(capacity_seconds) as f64 * 100.0 / capacity_seconds as f64
    } else {
        0.0
    };
    rsx! { span { class: "load-band-segment {class}", style: "width:{width:.4}%" } }
}

fn overflow_percentage(overflow_seconds: i64, capacity_seconds: i64) -> f64 {
    if overflow_seconds <= 0 {
        0.0
    } else if capacity_seconds <= 0 {
        100.0
    } else {
        overflow_seconds.min(capacity_seconds) as f64 * 100.0 / capacity_seconds as f64
    }
}

fn raw_used_seconds(durations: BandDurations) -> i64 {
    [
        durations.unavailable_seconds,
        durations.elapsed_seconds,
        durations.repetitive_seconds,
        durations.non_repetitive_seconds,
        durations.rho_leeway_seconds,
    ]
    .into_iter()
    .fold(0_i64, |sum, value| sum.saturating_add(value.max(0)))
}

fn clipped_segments(durations: BandDurations) -> BandSegments {
    let mut remaining = SECONDS_PER_DAY;
    let mut take = |seconds: i64| {
        let clipped = seconds.max(0).min(remaining);
        remaining -= clipped;
        clipped
    };
    BandSegments {
        unavailable_seconds: take(durations.unavailable_seconds),
        elapsed_seconds: take(durations.elapsed_seconds),
        repetitive_seconds: take(durations.repetitive_seconds),
        non_repetitive_seconds: take(durations.non_repetitive_seconds),
        rho_leeway_seconds: take(durations.rho_leeway_seconds),
        free_seconds: remaining,
    }
}

struct BandSegments {
    unavailable_seconds: i64,
    elapsed_seconds: i64,
    repetitive_seconds: i64,
    non_repetitive_seconds: i64,
    rho_leeway_seconds: i64,
    free_seconds: i64,
}

fn format_signed(seconds: i64) -> String {
    let sign = if seconds >= 0 { '+' } else { '-' };
    let minutes = seconds.unsigned_abs() / 60;
    format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
}

fn format_unsigned(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

fn format_duration(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    format!("{}時間{}分", minutes / 60, minutes % 60)
}

fn weekday_jp(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "月",
        Weekday::Tue => "火",
        Weekday::Wed => "水",
        Weekday::Thu => "木",
        Weekday::Fri => "金",
        Weekday::Sat => "土",
        Weekday::Sun => "日",
    }
}
