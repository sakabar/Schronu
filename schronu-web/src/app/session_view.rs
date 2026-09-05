use dioxus::prelude::*;

use crate::client::time_model::format_mm_ss;
pub(crate) use crate::client::view_projection::SessionCardViewModel;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(all(feature = "web", target_arch = "wasm32")), allow(dead_code))]
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

#[component]
pub fn SessionView(
    sessions: Vec<SessionCardViewModel>,
    global_blocked: bool,
    #[props(default)] auto_session_in_flight: bool,
    on_auto_session: EventHandler<()>,
    on_action: EventHandler<SessionAction>,
) -> Element {
    if sessions.is_empty() {
        return rsx! {
            section { class: "session-empty",
                button {
                    class: "primary-action",
                    r#type: "button",
                    disabled: auto_session_in_flight,
                    onclick: move |_| {
                        if !auto_session_in_flight {
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
    let discard_disabled = session.in_flight || session.server_committed;
    let mutation_disabled = discard_disabled || global_blocked || session.manual_check_blocked;
    let completion = session
        .completion_hh_mm
        .clone()
        .unwrap_or_else(|| "--:--".to_owned());
    let progress = session
        .progress_percent
        .map_or_else(|| "--%".to_owned(), |percent| format!("{percent}%"));
    let progress_aria = session.progress_percent.map_or_else(
        || "進捗を計算できません".to_owned(),
        |percent| format!("{percent}%"),
    );
    let progress_now = session
        .progress_percent
        .map(|percent| percent.clamp(0, 100).to_string());
    let progress_label = format!("{}の進捗", session.task_name);
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
            div {
                class: "session-progress-scroll",
                role: "progressbar",
                aria_label: progress_label,
                aria_valuemin: "0",
                aria_valuemax: "100",
                aria_valuenow: progress_now,
                aria_valuetext: progress_aria,
                div { class: "session-progress-track",
                    div { class: "session-progress-normal", style: normal_style }
                    div { class: "session-progress-overrun", style: overrun_style }
                }
            }
            p { class: remaining_class, "{format_mm_ss(session.remaining_seconds)}" }
            div { class: "session-actions",
                SessionActionButton {
                    label: "破棄して解除",
                    task_name: session.task_name.clone(),
                    task_id: session.task_id.clone(),
                    kind: SessionActionKind::Discard,
                    disabled: discard_disabled,
                    on_action,
                }
                SessionActionButton {
                    label: "記録して解除",
                    task_name: session.task_name.clone(),
                    task_id: session.task_id.clone(),
                    kind: SessionActionKind::Record,
                    disabled: mutation_disabled,
                    on_action,
                }
                SessionActionButton {
                    label: "完了",
                    task_name: session.task_name,
                    task_id: session.task_id,
                    kind: SessionActionKind::Complete,
                    disabled: mutation_disabled,
                    on_action,
                }
            }
        }
    }
}

#[component]
fn SessionActionButton(
    label: &'static str,
    task_name: String,
    task_id: String,
    kind: SessionActionKind,
    disabled: bool,
    on_action: EventHandler<SessionAction>,
) -> Element {
    let aria_label = format!("{task_name}: {label}");
    rsx! {
        button {
            r#type: "button",
            aria_label,
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
