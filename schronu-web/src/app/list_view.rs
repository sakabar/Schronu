use dioxus::prelude::*;

use crate::SessionTask;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DateButtonViewModel {
    pub logical_date: String,
    pub label: String,
    pub selected: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListRowViewModel {
    pub task: SessionTask,
    pub deadline_label: Option<String>,
    pub schedule_label: String,
    pub deadline_epoch_ms: Option<i64>,
    pub is_leaf: bool,
}

#[component]
pub fn ListView(
    dates: Vec<DateButtonViewModel>,
    rows: Vec<ListRowViewModel>,
    active_task_ids: Vec<String>,
    tick_now_epoch_ms: i64,
    on_select_date: EventHandler<String>,
    on_start_session: EventHandler<SessionTask>,
) -> Element {
    rsx! {
        section { class: "task-list-view",
            nav { class: "date-pills", aria_label: "logical date",
                for date in dates {
                    DateButton { date, on_select_date }
                }
            }
            div { class: "task-table-scroll",
                table { class: "task-table",
                    thead {
                        tr {
                            th { "締切" }
                            th { "予定" }
                            th { "タスク" }
                            th { "" }
                        }
                    }
                    tbody {
                        for row in rows {
                            TaskRow {
                                active: active_task_ids.iter().any(|task_id| task_id == &row.task.task_id),
                                row,
                                tick_now_epoch_ms,
                                on_start_session,
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
    tick_now_epoch_ms: i64,
    on_start_session: EventHandler<SessionTask>,
) -> Element {
    let deadline_class = if row
        .deadline_epoch_ms
        .is_some_and(|deadline| tick_now_epoch_ms > deadline)
    {
        "deadline is-overdue"
    } else {
        "deadline"
    };
    let task_class = if row.is_leaf {
        "task-name is-leaf"
    } else {
        "task-name"
    };
    let deadline = row.deadline_label.as_deref().unwrap_or("—");
    let task = row.task.clone();

    rsx! {
        tr { class: "task-row",
            td { class: deadline_class, "{deadline}" }
            td { class: "schedule-time", "{row.schedule_label}" }
            td { class: task_class, "{row.task.task_name}" }
            td {
                button {
                    class: "session-start",
                    r#type: "button",
                    disabled: active,
                    onclick: move |_| {
                        if !active {
                            on_start_session.call(task.clone());
                        }
                    },
                    "セッション"
                }
            }
        }
    }
}
