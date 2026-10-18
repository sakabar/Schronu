use dioxus::prelude::*;
use std::collections::HashSet;

use crate::client::time_model::{format_hh_mm_ss, format_mm_ss};
pub(crate) use crate::client::view_projection::SessionCardViewModel;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(all(feature = "web", target_arch = "wasm32")), allow(dead_code))]
pub enum SessionActionKind {
    RestartWithoutRecording,
    Discard,
    Record,
    WorkSlice,
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

#[cfg_attr(
    not(any(test, all(feature = "web", target_arch = "wasm32"))),
    allow(dead_code)
)]
fn parse_auto_session_minutes(input: &str) -> Result<Option<i64>, &'static str> {
    if input.is_empty() {
        return Ok(None);
    }
    if !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("分数は正の整数で入力してください。");
    }
    match input.parse::<i64>() {
        Ok(minutes) if minutes > 0 && minutes <= i64::MAX / 60 => Ok(Some(minutes)),
        _ => Err("分数は1以上の有効な整数で入力してください。"),
    }
}

#[component]
pub fn SessionView(
    sessions: Vec<SessionCardViewModel>,
    #[props(default)] linkable_task_ids: HashSet<String>,
    global_blocked: bool,
    #[props(default)] mutations_locked: bool,
    #[props(default)] auto_session_in_flight: bool,
    #[props(default)] server_actions_blocked: bool,
    on_auto_session: EventHandler<Option<i64>>,
    on_action: EventHandler<SessionAction>,
    #[props(default)] on_open_link: EventHandler<String>,
    #[props(default)] timer_shortcut_supported: bool,
    #[props(default)] timer_server_busy: bool,
    #[props(default)] on_open_timer: Callback<String, Option<Result<(), String>>>,
) -> Element {
    let mut minutes_input = use_signal(String::new);
    let mut input_error = use_signal(|| None::<&'static str>);
    if sessions.is_empty() {
        return rsx! {
            section { class: "session-empty",
                div { class: "auto-session-controls",
                    label { r#for: "auto-session-minutes", "分数 (任意)" }
                    input {
                        id: "auto-session-minutes",
                        r#type: "text",
                        inputmode: "numeric",
                        value: minutes_input.read().clone(),
                        disabled: auto_session_in_flight || mutations_locked || server_actions_blocked,
                        oninput: move |event| {
                            minutes_input.set(event.value());
                            input_error.set(None);
                        },
                    }
                }
                button {
                    class: "primary-action",
                    r#type: "button",
                    disabled: auto_session_in_flight || mutations_locked || server_actions_blocked,
                    onclick: move |_| {
                        if !auto_session_in_flight && !mutations_locked && !server_actions_blocked {
                            match parse_auto_session_minutes(&minutes_input.read()) {
                                Ok(max_minutes) => on_auto_session.call(max_minutes),
                                Err(message) => input_error.set(Some(message)),
                            }
                        }
                    },
                    "自動セッション"
                }
                p { class: "auto-session-hint",
                    "指定分数未満の予定区間から選びます。"
                    br {}
                    "空欄では通常の自動選定です。"
                }
                if let Some(message) = *input_error.read() {
                    p { class: "error", role: "alert", "{message}" }
                }
            }
        };
    }

    rsx! {
        section { class: "session-list",
            for session in sessions {
                SessionCard {
                    key: "{session.task_id}",
                    linkable: linkable_task_ids.contains(&session.task_id),
                    session,
                    global_blocked,
                    mutations_locked,
                    server_actions_blocked,
                    on_action,
                    on_open_link,
                    timer_shortcut_supported,
                    timer_server_busy,
                    on_open_timer,
                }
            }
        }
    }
}

#[component]
fn SessionCard(
    session: SessionCardViewModel,
    linkable: bool,
    global_blocked: bool,
    mutations_locked: bool,
    server_actions_blocked: bool,
    on_action: EventHandler<SessionAction>,
    on_open_link: EventHandler<String>,
    timer_shortcut_supported: bool,
    timer_server_busy: bool,
    on_open_timer: Callback<String, Option<Result<(), String>>>,
) -> Element {
    let mut timer_result = use_signal(|| None::<Result<(), String>>);
    let timer_enabled = super::timer_shortcut::timer_enabled(
        &session,
        global_blocked,
        mutations_locked,
        timer_server_busy,
    );
    let timer_task_id = session.task_id.clone();
    let timer_label = format!("{}: 残り時間でタイマーを開く", session.task_name);
    let on_action = EventHandler::new(move |action: SessionAction| {
        if matches!(
            action.kind,
            SessionActionKind::RestartWithoutRecording
                | SessionActionKind::ResumeCompletionConflict
        ) {
            timer_result.set(None);
        }
        on_action.call(action);
    });
    let mut confirming_discard_completion = use_signal(|| false);
    use_effect(use_reactive(
        (&mutations_locked,),
        move |(mutations_locked,)| {
            if mutations_locked {
                confirming_discard_completion.set(false);
            }
        },
    ));
    let discard_disabled =
        session.in_flight || session.server_committed || mutations_locked || server_actions_blocked;
    let mutation_disabled = discard_disabled || global_blocked || session.manual_check_blocked;
    let restart_disabled = session.in_flight
        || session.server_committed
        || mutations_locked
        || global_blocked
        || session.manual_check_blocked;
    let resume_disabled = session.in_flight
        || session.server_committed
        || mutations_locked
        || global_blocked
        || session.manual_check_blocked;
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
    let (remaining_kind, remaining_label) = if session.remaining_seconds < 0 {
        ("超過", format!("超過時間 {remaining}"))
    } else {
        ("残り", format!("残り時間 {remaining}"))
    };
    let actual_at_start = format_mm_ss(i128::from(session.actual_work_seconds_at_start));
    let actual_at_start_label = format!("セッション開始時点の実績 {actual_at_start}");
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
    let open_task_id = session.task_id.clone();
    let open_task_label = format!("{}: 開", session.task_name);

    rsx! {
        article { class: "session-card",
            div { class: "session-card-heading",
                h2 {
                    if linkable {
                        button {
                            class: "session-task-link",
                            r#type: "button",
                            aria_label: open_task_label,
                            disabled: server_actions_blocked,
                            onclick: move |_| {
                                if !server_actions_blocked {
                                    on_open_link.call(open_task_id.clone());
                                }
                            },
                            "{session.task_name} ↗"
                        }
                    } else {
                        "{session.task_name}"
                    }
                }
                span { class: "session-progress-label", "{progress}" }
            }
            div { class: "session-timing",
                span { class: "session-countdown",
                    span { class: "session-countdown-label", aria_hidden: "true", "{remaining_kind}" }
                    strong { class: remaining_class, aria_label: remaining_label, "{remaining}" }
                    if timer_shortcut_supported {
                        button {
                            class: "session-timer-shortcut",
                            r#type: "button",
                            title: "残り時間でタイマーを開く",
                            aria_label: timer_label,
                            disabled: !timer_enabled,
                            onclick: move |_| {
                                timer_result.set(None);
                                if timer_enabled {
                                    timer_result.set(on_open_timer.call(timer_task_id.clone()));
                                }
                            },
                            svg {
                                width: "18", height: "18", view_box: "0 0 24 24",
                                fill: "none", stroke: "currentColor", stroke_width: "2",
                                stroke_linecap: "round", stroke_linejoin: "round",
                                "aria-hidden": "true", "focusable": "false",
                                circle { cx: "12", cy: "12", r: "9" }
                                path { d: "M12 7v5l3 2" }
                            }
                        }
                    }
                }
                span { class: "session-timing-meta",
                    span { class: "session-time-range",
                        time { aria_label: started_at_label, "{session.started_at_hh_mm}" }
                        span { aria_hidden: "true", "→" }
                        time { aria_label: completion_label, "{completion}" }
                    }
                    span {
                        class: "session-start-actual",
                        aria_label: actual_at_start_label,
                        "開始時実績 {actual_at_start}"
                    }
                }
                if let Some(result) = timer_result.read().as_ref() {
                    match result {
                        Ok(()) => rsx! { span { class: "session-timer-message", role: "status", "起動を要求しました。実行結果は確認できません。" } },
                        Err(message) => rsx! { span { class: "session-timer-message error", role: "alert", "{message}" } },
                    }
                }
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
                    span { class: "session-progress-estimate-marker", aria_hidden: "true" }
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
                            disabled: resume_disabled,
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
                        label: "計測を破棄して解除",
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
                    SessionActionButton {
                        class: "session-action-restart",
                        label: "計測を破棄して再開",
                        task_name: session.task_name.clone(),
                        task_id: session.task_id.clone(),
                        kind: SessionActionKind::RestartWithoutRecording,
                        disabled: restart_disabled,
                        on_action,
                    }
                    SessionActionButton {
                        class: "session-action-work-slice",
                        label: "記録して分割",
                        task_name: session.task_name.clone(),
                        task_id: session.task_id.clone(),
                        kind: SessionActionKind::WorkSlice,
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
