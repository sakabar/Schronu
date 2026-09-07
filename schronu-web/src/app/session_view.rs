use dioxus::prelude::*;

use crate::client::time_model::{format_hh_mm_ss, format_mm_ss};
pub(crate) use crate::client::view_projection::SessionCardViewModel;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(all(feature = "web", target_arch = "wasm32")), allow(dead_code))]
pub enum SessionActionKind {
    Discard,
    Record,
    Complete,
    CompleteWithoutRecording,
    ResumeCompletionConflict,
    ConfirmCompletionConflict,
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
    #[props(default)] mutations_locked: bool,
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
                    disabled: auto_session_in_flight || mutations_locked,
                    onclick: move |_| {
                        if !auto_session_in_flight && !mutations_locked {
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
                    mutations_locked,
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
    mutations_locked: bool,
    on_action: EventHandler<SessionAction>,
) -> Element {
    let mut confirming_discard_completion = use_signal(|| false);
    use_effect(use_reactive(
        (&mutations_locked,),
        move |(mutations_locked,)| {
            if mutations_locked {
                confirming_discard_completion.set(false);
            }
        },
    ));
    let discard_disabled = session.in_flight || session.server_committed || mutations_locked;
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
    let remaining = format_mm_ss(session.remaining_seconds);
    let remaining_label = if session.remaining_seconds < 0 {
        format!("超過時間 {remaining}")
    } else {
        format!("残り時間 {remaining}")
    };
    let started_at_label = format!("開始時刻 {}", session.started_at_hh_mm);
    let completion_label = format!("完了予定時刻 {completion}");
    let normal_style = format!("width:calc({}% / 1.5)", session.normal_bar_percent.max(0));
    let overrun_style = format!("width:calc({}% / 1.5)", session.overrun_bar_percent.max(0));
    let conflict_message = session.completion_conflict.map(|conflict| {
        let current = format_hh_mm_ss(i128::from(conflict.current_actual_work_seconds));
        if conflict.record_elapsed_seconds {
            let measured = format_hh_mm_ss(i128::from(conflict.measured_elapsed_seconds));
            format!(
                "タスクの実績時間が更新されています。現在の実績 {current} に、このセッションの計測 {measured} を加えて完了しますか?"
            )
        } else {
            format!(
                "タスクの実績時間が更新されています。現在の実績 {current} を維持して完了しますか?"
            )
        }
    });
    let conflict_confirm_label = session.completion_conflict.map_or("", |conflict| {
        if conflict.record_elapsed_seconds {
            "加算して完了"
        } else {
            "実績を維持して完了"
        }
    });

    rsx! {
        article { class: "session-card",
            div { class: "session-card-heading",
                h2 { "{session.task_name}" }
                span { class: "session-progress-label", "{progress}" }
            }
            div { class: "session-timing",
                span { class: "session-time-range",
                    time { aria_label: started_at_label, "{session.started_at_hh_mm}" }
                    span { aria_hidden: "true", "→" }
                    time { aria_label: completion_label, "{completion}" }
                }
                span { class: remaining_class, aria_label: remaining_label, "{remaining}" }
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
            if session.completion_conflict.is_some() {
                div {
                    class: "session-discard-completion-confirmation",
                    role: "group",
                    aria_label: "{session.task_name}: 実績時間の競合確認",
                    p { "{conflict_message.as_deref().unwrap_or_default()}" }
                    div { class: "session-confirmation-actions",
                        SessionActionButton {
                            class: "session-action-resume-conflict",
                            label: "計測を再開",
                            task_name: session.task_name.clone(),
                            task_id: session.task_id.clone(),
                            kind: SessionActionKind::ResumeCompletionConflict,
                            disabled: mutation_disabled,
                            on_action,
                        }
                        SessionActionButton {
                            class: "session-action-confirm-conflict",
                            label: conflict_confirm_label,
                            task_name: session.task_name.clone(),
                            task_id: session.task_id.clone(),
                            kind: SessionActionKind::ConfirmCompletionConflict,
                            disabled: mutation_disabled,
                            on_action,
                        }
                    }
                }
            } else if confirming_discard_completion() {
                div {
                    class: "session-discard-completion-confirmation",
                    role: "group",
                    aria_label: "{session.task_name}: 計測を破棄して完了の確認",
                    p { "このセッションの計測時間は記録されません。タスクを完了しますか?" }
                    div { class: "session-confirmation-actions",
                        button {
                            r#type: "button",
                            onclick: move |_| {
                                confirming_discard_completion.set(false);
                            },
                            "キャンセル"
                        }
                        button {
                            class: "session-action-complete-without-recording",
                            r#type: "button",
                            disabled: mutation_disabled,
                            aria_label: "{session.task_name}: 計測を破棄して完了を確定",
                            onclick: move |_| {
                                if !mutation_disabled {
                                    on_action.call(SessionAction {
                                        task_id: session.task_id.clone(),
                                        kind: SessionActionKind::CompleteWithoutRecording,
                                    });
                                }
                            },
                            "計測を破棄して完了"
                        }
                    }
                }
            } else {
                div { class: "session-actions",
                    SessionActionButton {
                        class: "session-action-discard",
                        label: "破棄して解除",
                        task_name: session.task_name.clone(),
                        task_id: session.task_id.clone(),
                        kind: SessionActionKind::Discard,
                        disabled: discard_disabled,
                        on_action,
                    }
                    SessionActionButton {
                        class: "session-action-record",
                        label: "記録して解除",
                        task_name: session.task_name.clone(),
                        task_id: session.task_id.clone(),
                        kind: SessionActionKind::Record,
                        disabled: mutation_disabled,
                        on_action,
                    }
                    SessionDiscardCompletionButton {
                        task_name: session.task_name.clone(),
                        disabled: mutation_disabled,
                        on_confirm: move |_| confirming_discard_completion.set(true),
                    }
                    SessionActionButton {
                        class: "session-action-complete",
                        label: "記録して完了",
                        task_name: session.task_name.clone(),
                        task_id: session.task_id.clone(),
                        kind: SessionActionKind::Complete,
                        disabled: mutation_disabled,
                        on_action,
                    }
                }
            }
        }
    }
}

#[component]
fn SessionDiscardCompletionButton(
    task_name: String,
    disabled: bool,
    on_confirm: EventHandler<()>,
) -> Element {
    let aria_label = format!("{task_name}: 計測を破棄して完了");
    rsx! {
        button {
            class: "session-action-complete-without-recording",
            r#type: "button",
            aria_label,
            disabled,
            onclick: move |_| {
                if !disabled {
                    on_confirm.call(());
                }
            },
            "計測を破棄して完了"
        }
    }
}

#[component]
fn SessionActionButton(
    class: &'static str,
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
            class,
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
