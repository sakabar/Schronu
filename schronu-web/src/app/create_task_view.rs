use dioxus::prelude::*;

#[component]
pub fn CreateTaskView(
    name: String,
    minutes: String,
    error: Option<String>,
    success: Option<String>,
    blocked: bool,
    on_name_change: EventHandler<String>,
    on_minutes_change: EventHandler<String>,
    on_submit: EventHandler<()>,
) -> Element {
    rsx! {
        section { class: "create-task-view", aria_label: "新しいタスク",
            h2 { "新しいタスク" }
            p { class: "create-task-help", "作成したタスクは翌論理日06:00から有効になります。" }
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
