use super::list_view::DateButtonViewModel;
use crate::client::state::DiscardedSessionsViewState;
use crate::client::time_model::format_hh_mm_ss;
use dioxus::prelude::*;

#[component]
pub(crate) fn SummaryView(
    dates: Vec<DateButtonViewModel>,
    state: DiscardedSessionsViewState,
    #[props(default)] server_actions_blocked: bool,
    on_select_date: EventHandler<String>,
    on_retry: EventHandler<String>,
) -> Element {
    rsx! {
        section { class: "discard-summary", aria_label: "破棄セッション集計",
            nav { class: "date-pills", aria_label: "集計日",
                for date in dates {
                    button {
                        key: "{date.logical_date}",
                        class: if date.selected { "date-pill is-selected" } else { "date-pill" },
                        r#type: "button",
                        aria_pressed: date.selected,
                        disabled: server_actions_blocked,
                        onclick: move |_| {
                            if !server_actions_blocked {
                                on_select_date.call(date.logical_date.clone());
                            }
                        },
                        "{date.label}"
                    }
                }
            }
            if let DiscardedSessionsViewState::Loaded(summary) = state {
                h1 { "破棄時間 {format_hh_mm_ss(i128::from(summary.total_seconds))}" }
                if summary.events.is_empty() {
                    p { role: "status", "この日の破棄セッションはありません。" }
                } else {
                    section { class: "discard-task-totals", aria_label: "タスク別破棄時間",
                        h2 { "タスク別" }
                        ul {
                            for total in summary.task_totals {
                                li { "{total.task_name} {format_hh_mm_ss(i128::from(total.total_seconds))}" }
                            }
                        }
                    }
                    ol { class: "discard-events", aria_label: "破棄セッション内訳",
                        for event in summary.events {
                            li {
                                article {
                                    h2 { "{event.task_name_at_start}" }
                                    p {
                                        aria_label: "{event.task_name_at_start}の開始・終了時刻",
                                        time {
                                            datetime: "{format_datetime(event.started_at_epoch_ms)}",
                                            "{format_local_hh_mm(event.started_at_epoch_ms)}"
                                        }
                                        span { aria_hidden: "true", "–" }
                                        time {
                                            datetime: "{format_datetime(event.ended_at_epoch_ms)}",
                                            "{format_local_hh_mm(event.ended_at_epoch_ms)}"
                                        }
                                    }
                                    p { "{format_hh_mm_ss(i128::from(event.elapsed_seconds))} / {reason_label(&event.reason)}" }
                                }
                            }
                        }
                    }
                }
            } else if let DiscardedSessionsViewState::Loading { logical_date } = state {
                p { class: "discard-summary-loading", role: "status", aria_live: "polite",
                    "{logical_date}の破棄時間を読み込んでいます…"
                }
            } else if let DiscardedSessionsViewState::Error { logical_date, message } = state {
                section { class: "discard-summary-error", role: "alert",
                    p { "{message}" }
                    button {
                        r#type: "button",
                        disabled: server_actions_blocked,
                        onclick: move |_| {
                            if !server_actions_blocked {
                                on_retry.call(logical_date.clone());
                            }
                        },
                        "再試行"
                    }
                }
            } else {
                p { role: "status", "集計する日付を選択してください。" }
            }
        }
    }
}

fn format_datetime(epoch_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(epoch_ms)
        .map(|date| date.to_rfc3339())
        .unwrap_or_default()
}

#[cfg_attr(not(all(feature = "web", target_arch = "wasm32")), allow(dead_code))]
fn reason_label(reason: &str) -> &'static str {
    match reason {
        "web_discard_release" => "計測を破棄して解除",
        "web_discard_complete" => "計測を破棄して完了",
        "cli_unfocus" => "CLIでフォーカス解除",
        "cli_tuck_away" => "CLIでしまう",
        "cli_focus_switch" => "CLIでフォーカス切替",
        "cli_auto_switch" => "CLIで自動切替",
        "cli_normal_exit" => "CLIを正常終了",
        _ => "不明な理由",
    }
}

#[cfg_attr(not(all(feature = "web", target_arch = "wasm32")), allow(dead_code))]
fn format_local_hh_mm(epoch_ms: i64) -> String {
    #[cfg(all(feature = "web", target_arch = "wasm32"))]
    {
        return format_browser_hh_mm(epoch_ms);
    }

    #[cfg(not(all(feature = "web", target_arch = "wasm32")))]
    chrono::DateTime::from_timestamp_millis(epoch_ms)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "--:--".to_owned())
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
fn format_browser_hh_mm(epoch_ms: i64) -> String {
    let date = js_sys::Date::new_0();
    date.set_time(epoch_ms as f64);
    if !date.get_time().is_finite() {
        return "--:--".to_owned();
    }
    format!("{:02}:{:02}", date.get_hours(), date.get_minutes())
}
