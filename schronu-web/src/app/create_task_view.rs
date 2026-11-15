use crate::TaskCreationDay;
use dioxus::prelude::*;

#[component]
pub fn CreateTaskView(
    name: String,
    minutes: String,
    creation_day: TaskCreationDay,
    error: Option<String>,
    success: Option<String>,
    blocked: bool,
    on_name_change: EventHandler<String>,
    on_minutes_change: EventHandler<String>,
    on_creation_day_change: EventHandler<TaskCreationDay>,
    on_submit: EventHandler<()>,
) -> Element {
    rsx! {
        section { class: "create-task-view", aria_label: "新しいタスク",
            h2 { "新しいタスク" }
            form {
                onsubmit: move |event| {
                    event.prevent_default();
                    if !blocked { on_submit.call(()); }
                },
                label { r#for: "create-task-name", "タスク名" }
                input {
                    id: "create-task-name",
                    r#type: "text",
                    value: name,
                    autocomplete: "off",
                    oninput: move |event| on_name_change.call(event.value()),
                }
                label { r#for: "create-task-minutes", "見積時間 (分、省略時15分)" }
                input {
                    id: "create-task-minutes",
                    r#type: "number",
                    min: "0",
                    step: "1",
                    placeholder: "15",
                    value: minutes,
                    oninput: move |event| on_minutes_change.call(event.value()),
                }
                fieldset { class: "create-task-day", disabled: blocked,
                    legend { "作成日" }
                    label {
                        input {
                            r#type: "radio",
                            name: "creation-day",
                            value: "today",
                            checked: creation_day == TaskCreationDay::Today,
                            onchange: move |_| on_creation_day_change.call(TaskCreationDay::Today),
                        }
                        "今日の論理日で作成"
                    }
                    label {
                        input {
                            r#type: "radio",
                            name: "creation-day",
                            value: "seven_days_later",
                            checked: creation_day == TaskCreationDay::SevenDaysLater,
                            onchange: move |_| on_creation_day_change.call(TaskCreationDay::SevenDaysLater),
                        }
                        "7日後の論理日で作成"
                    }
                }
                if let Some(error) = error {
                    p { class: "create-task-error", role: "alert", "{error}" }
                }
                button { r#type: "submit", disabled: blocked, "タスクを作成" }
            }
            if let Some(success) = success {
                p { class: "create-task-success", role: "status", "{success}" }
            }
        }
    }
}
