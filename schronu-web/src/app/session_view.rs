use dioxus::prelude::*;

use crate::client::time_model::format_mm_ss;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by the pending app integration")
)]
pub enum SessionActionKind {
    Discard,
    Record,
    Complete,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionAction {
    pub task_id: String,
    pub kind: SessionActionKind,
}

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

#[component]
pub fn SessionView(
    sessions: Vec<SessionCardViewModel>,
    global_blocked: bool,
    on_auto_session: EventHandler<()>,
    on_action: EventHandler<SessionAction>,
) -> Element {
    if sessions.is_empty() {
        return rsx! {
            section { class: "session-empty",
                button {
                    class: "primary-action",
                    r#type: "button",
                    disabled: global_blocked,
                    onclick: move |_| {
                        if !global_blocked {
                            on_auto_session.call(());
                        }
                    },
                    "自動セッション"
                }
            }
        };
    }

    rsx! {
        section { class: "session-list",
            for session in sessions {
                SessionCard {
                    key: "{session.task_id}",
                    session,
                    global_blocked,
                    on_action,
                }
            }
        }
    }
}

#[component]
fn SessionCard(
    session: SessionCardViewModel,
    global_blocked: bool,
    on_action: EventHandler<SessionAction>,
) -> Element {
    let disabled = global_blocked
        || session.in_flight
        || session.manual_check_blocked
        || session.server_committed;
    let completion = session
        .completion_hh_mm
        .clone()
        .unwrap_or_else(|| "--:--".to_owned());
    let progress = session
        .progress_percent
        .map_or_else(|| "--%".to_owned(), |percent| format!("{percent}%"));
    let remaining_class = if session.remaining_seconds < 0 {
        "session-remaining is-overrun"
    } else {
        "session-remaining"
    };
    let normal_style = format!("width:{}%", session.normal_bar_percent.max(0));
    let overrun_style = format!("width:{}%", session.overrun_bar_percent.max(0));

    rsx! {
        article { class: "session-card",
            div { class: "session-card-heading",
                h2 { "{session.task_name}" }
                span { class: "session-progress-label", "{progress}" }
            }
            div { class: "session-time-row",
                time { "{session.started_at_hh_mm}" }
                span { aria_hidden: "true", "→" }
                time { "{completion}" }
            }
            div { class: "session-progress-scroll",
                div { class: "session-progress-track",
                    div { class: "session-progress-normal", style: normal_style }
                    div { class: "session-progress-overrun", style: overrun_style }
                }
            }
            p { class: remaining_class, "{format_mm_ss(session.remaining_seconds)}" }
            div { class: "session-actions",
                SessionActionButton {
                    label: "破棄して解除",
                    task_id: session.task_id.clone(),
                    kind: SessionActionKind::Discard,
                    disabled,
                    on_action,
                }
                SessionActionButton {
                    label: "記録して解除",
                    task_id: session.task_id.clone(),
                    kind: SessionActionKind::Record,
                    disabled,
                    on_action,
                }
                SessionActionButton {
                    label: "完了",
                    task_id: session.task_id,
                    kind: SessionActionKind::Complete,
                    disabled,
                    on_action,
                }
            }
        }
    }
}

#[component]
fn SessionActionButton(
    label: &'static str,
    task_id: String,
    kind: SessionActionKind,
    disabled: bool,
    on_action: EventHandler<SessionAction>,
) -> Element {
    rsx! {
        button {
            r#type: "button",
            disabled,
            onclick: move |_| {
                if !disabled {
                    on_action.call(SessionAction {
                        task_id: task_id.clone(),
                        kind,
                    });
                }
            },
            "{label}"
        }
    }
}
