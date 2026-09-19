use dioxus::prelude::*;
use std::rc::Rc;

#[cfg(test)]
pub(crate) use crate::client::view_projection::DeferConfirmationViewModel;
pub(crate) use crate::client::view_projection::{DeferConfirmationKind, ListRowViewModel};
use crate::{DeferPlan, SessionTask};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DateButtonViewModel {
    pub logical_date: String,
    pub label: String,
    pub selected: bool,
}

#[component]
pub fn ListView(
    dates: Vec<DateButtonViewModel>,
    rows: Vec<ListRowViewModel>,
    active_task_ids: Vec<String>,
    date_input_text: String,
    date_input_error: Option<String>,
    filter_text: String,
    #[props(default)] mutations_locked: bool,
    #[props(default)] mutation_globally_blocked: bool,
    #[props(default)] server_actions_blocked: bool,
    on_select_date: EventHandler<String>,
    on_date_input_change: EventHandler<String>,
    on_submit_date_input: EventHandler<()>,
    on_start_session: EventHandler<(SessionTask, bool)>,
    #[props(default)] on_defer_task: EventHandler<(String, DeferPlan)>,
    on_filter_change: EventHandler<String>,
) -> Element {
    let mut filter_input = use_signal(|| None::<Rc<MountedData>>);
    let normalized_filter = filter_text.trim().to_lowercase();
    let task_name_matches = |task_name: &str| {
        normalized_filter.is_empty() || task_name.to_lowercase().contains(&normalized_filter)
    };
    let filtered_rows = rows
        .into_iter()
        .filter(|row| task_name_matches(&row.task.task_name))
        .collect::<Vec<_>>();
    let no_matches = !normalized_filter.is_empty() && filtered_rows.is_empty();

    rsx! {
        section { class: "task-list-view",
            nav { class: "date-pills", aria_label: "logical date",
                for date in dates {
                    DateButton { date, disabled: server_actions_blocked, on_select_date }
                }
            }
            div { class: "list-controls",
                form {
                    class: "date-jump-form",
                    aria_label: "日付へ移動",
                    onsubmit: move |event| {
                        event.prevent_default();
                        if !server_actions_blocked {
                            on_submit_date_input.call(());
                        }
                    },
                    div { class: "date-jump-controls",
                        input {
                            class: "date-jump-input",
                            r#type: "text",
                            value: date_input_text.clone(),
                            aria_label: "表示する日付",
                            aria_invalid: date_input_error.is_some(),
                            aria_describedby: date_input_error
                                .as_ref()
                                .map(|_| "date-input-error"),
                            placeholder: "例: 6/18",
                            autocomplete: "off",
                            oninput: move |event| on_date_input_change.call(event.value()),
                        }
                        button {
                            class: "date-jump-submit",
                            r#type: "submit",
                            disabled: date_input_text.trim().is_empty() || server_actions_blocked,
                            "表示"
                        }
                    }
                    if let Some(ref error) = date_input_error {
                        p { id: "date-input-error", class: "date-input-error", role: "alert", "{error}" }
                    }
                }
                div { class: "task-name-filter", role: "search",
                    input {
                        class: "task-name-filter-input",
                        r#type: "text",
                        value: filter_text.clone(),
                        aria_label: "タスク名を検索",
                        placeholder: "タスク名を検索",
                        onmounted: move |element| filter_input.set(Some(element.data())),
                        oninput: move |event| on_filter_change.call(event.value()),
                    }
                    if !filter_text.is_empty() {
                        button {
                            class: "task-name-filter-clear",
                            r#type: "button",
                            aria_label: "検索文字列をクリア",
                            onclick: move |_| async move {
                                on_filter_change.call(String::new());
                                if let Some(input) = filter_input.cloned() {
                                    let _ = input.set_focus(true).await;
                                }
                            },
                            "×"
                        }
                    }
                }
            }
            if no_matches {
                p { class: "task-filter-empty", role: "status", "一致するタスクがありません。" }
            } else {
                div { class: "task-table-scroll",
                    table { class: "task-table",
                        thead {
                            tr {
                                th { class: "session-heading", aria_label: "task操作", "" }
                                th { class: "schedule-heading", "予定" }
                                th { class: "deadline-heading", "締切" }
                                th { class: "task-heading", "タスク" }
                            }
                        }
                        tbody {
                            for row in filtered_rows {
                                TaskRow {
                                    key: "{row.row_key}",
                                    active: active_task_ids.iter().any(|task_id| task_id == &row.task.task_id),
                                    row,
                                    mutations_locked,
                                    mutation_globally_blocked,
                                    server_actions_blocked,
                                    on_start_session,
                                    on_defer_task,
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
fn DateButton(
    date: DateButtonViewModel,
    disabled: bool,
    on_select_date: EventHandler<String>,
) -> Element {
    let class = if date.selected {
        "date-pill is-selected"
    } else {
        "date-pill"
    };
    rsx! {
        button {
            class,
            r#type: "button",
            aria_pressed: date.selected,
            disabled,
            onclick: move |_| {
                if !disabled {
                    on_select_date.call(date.logical_date.clone());
                }
            },
            "{date.label}"
        }
    }
}

#[component]
fn TaskRow(
    row: ListRowViewModel,
    active: bool,
    mutations_locked: bool,
    mutation_globally_blocked: bool,
    server_actions_blocked: bool,
    on_start_session: EventHandler<(SessionTask, bool)>,
    on_defer_task: EventHandler<(String, DeferPlan)>,
) -> Element {
    let mut confirming_defer = use_signal(|| false);
    let deadline_class = if row.misses_deadline {
        "deadline is-overdue"
    } else {
        "deadline"
    };
    let task_class = if row.is_leaf {
        "task-name is-leaf"
    } else {
        "task-name"
    };
    let deadline = &row.deadline_label;
    let button_label = if active {
        format!("{}: セッション追加済み", row.task.task_name)
    } else {
        format!("{}: セッションに追加", row.task.task_name)
    };
    let button_text = if active { "✓" } else { "＋" };
    let task = row.task.clone();
    let defer_task_id = row.task.task_id.clone();
    let defer_task_id_on_confirm = defer_task_id.clone();
    let defer_plan = row.defer_plan.clone();
    let defer_plan_on_confirm = defer_plan.clone();
    let defer_confirmation = row.defer_confirmation.clone();
    let confirmation_message = defer_confirmation.as_ref().map(|confirmation| {
        let status = match confirmation.kind {
            DeferConfirmationKind::DeadlineLimited => "締切までの余裕がありません",
            DeferConfirmationKind::RoutinePeriod => "次の周期へ送ります",
        };
        format!(
            "{}は{}({})。先送りしますか?",
            row.task.task_name, status, confirmation.detail_label
        )
    });
    let is_leaf = row.is_leaf;

    rsx! {
        tr { class: "task-row",
            td { class: "session-cell",
                if is_leaf {
                    button {
                        class: "session-start",
                        r#type: "button",
                        aria_label: button_label,
                        disabled: active || mutations_locked,
                        onclick: move |_| {
                            if !active && !mutations_locked {
                                on_start_session.call((task.clone(), is_leaf));
                            }
                        },
                        span { class: "session-start-full-label", "セッション" }
                        span { class: "session-start-compact-label", aria_hidden: "true", "{button_text}" }
                    }
                    button {
                        class: "task-defer",
                        r#type: "button",
                        aria_label: format!("{}: 先送り", row.task.task_name),
                        disabled: active || mutations_locked || mutation_globally_blocked || server_actions_blocked,
                        onclick: move |_| {
                            if !active && !mutations_locked && !mutation_globally_blocked && !server_actions_blocked {
                                if defer_confirmation.is_some() {
                                    confirming_defer.set(true);
                                } else {
                                    on_defer_task.call((defer_task_id.clone(), defer_plan.clone()));
                                }
                            }
                        },
                        span { class: "task-defer-full-label", "先送り" }
                        span { class: "task-defer-compact-label", aria_hidden: "true", "→" }
                    }
                }
            }
            td { class: "schedule-time", "data-label": "予定", "{row.schedule_label}" }
            td { class: deadline_class, "data-label": "締切", "{deadline}" }
            td { class: task_class,
                div {
                    class: "task-name-scroll",
                    tabindex: 0,
                    "{row.task.task_name}"
                }
            }
        }
        if confirming_defer() {
            tr { class: "task-defer-confirmation-row",
                td { colspan: "4",
                    div {
                        class: "task-defer-confirmation",
                        role: "group",
                        tabindex: "-1",
                        aria_live: "assertive",
                        aria_label: format!("{}: 先送りの確認", row.task.task_name),
                        onmounted: move |element| async move {
                            let _ = element.data().set_focus(true).await;
                        },
                        p { "{confirmation_message.as_deref().unwrap_or_default()}" }
                        div { class: "task-defer-confirmation-actions",
                            button {
                                r#type: "button",
                                onclick: move |_| confirming_defer.set(false),
                                "キャンセル"
                            }
                            button {
                                class: "task-defer-confirm",
                                r#type: "button",
                                disabled: active || mutations_locked || mutation_globally_blocked || server_actions_blocked,
                                onclick: move |_| {
                                    if !active && !mutations_locked && !mutation_globally_blocked && !server_actions_blocked {
                                        confirming_defer.set(false);
                                        on_defer_task.call((defer_task_id_on_confirm.clone(), defer_plan_on_confirm.clone()));
                                    }
                                },
                                "先送りする"
                            }
                        }
                    }
                }
            }
        }
    }
}
