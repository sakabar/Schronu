use dioxus::prelude::*;
use std::rc::Rc;

pub(crate) use crate::client::view_projection::ListRowViewModel;
use crate::SessionTask;

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
    filter_text: String,
    #[props(default)] mutations_locked: bool,
    on_select_date: EventHandler<String>,
    on_start_session: EventHandler<(SessionTask, bool)>,
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
                    DateButton { date, on_select_date }
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
            if no_matches {
                p { class: "task-filter-empty", role: "status", "一致するタスクがありません。" }
            } else {
                div { class: "task-table-scroll",
                    table { class: "task-table",
                        thead {
                            tr {
                                th { class: "session-heading", aria_label: "セッション操作", "" }
                                th { class: "schedule-heading", "予定" }
                                th { class: "deadline-heading", "締切" }
                                th { class: "task-heading", "タスク" }
                            }
                        }
                        tbody {
                            for row in filtered_rows {
                                TaskRow {
                                    active: active_task_ids.iter().any(|task_id| task_id == &row.task.task_id),
                                    row,
                                    mutations_locked,
                                    on_start_session,
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
fn DateButton(date: DateButtonViewModel, on_select_date: EventHandler<String>) -> Element {
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
            onclick: move |_| on_select_date.call(date.logical_date.clone()),
            "{date.label}"
        }
    }
}

#[component]
fn TaskRow(
    row: ListRowViewModel,
    active: bool,
    mutations_locked: bool,
    on_start_session: EventHandler<(SessionTask, bool)>,
) -> Element {
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
    }
}
