use super::{Action, DisplayTimezone, OfflineState};
use crate::app::component::DisplayTimezoneSettings;
use crate::app::component_models::browser_now_epoch_ms;
use crate::app::list_view::{DateButtonViewModel, ListModeControl, ListView};
use crate::client::date_buttons::scheduled_date_button;
use crate::client::date_input::resolve_date_input;
use crate::client::state::ListMode;
use crate::client::view_projection::project_daily_rows;
use chrono::NaiveDate;
use dioxus::prelude::*;
use std::collections::BTreeMap;
use std::rc::Rc;

#[component]
pub(super) fn Schedule(
    state: Rc<OfflineState>,
    date: String,
    mut filter: Signal<String>,
    timezone: Signal<DisplayTimezone>,
    today: Option<NaiveDate>,
    blocked: bool,
    on_action: EventHandler<Action>,
    on_select: EventHandler<String>,
) -> Element {
    let mut deviation_order = use_signal(|| false);
    let mut date_input = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let dates = state
        .bundle
        .days
        .keys()
        .filter_map(|key| {
            let day = NaiveDate::parse_from_str(key, "%Y-%m-%d").ok()?;
            let label = today.map_or_else(
                || key.clone(),
                |today| scheduled_date_button(day, today).label,
            );
            Some(DateButtonViewModel {
                logical_date: key.clone(),
                label,
                selected: key == &date,
            })
        })
        .collect();
    let scheduled = state
        .bundle
        .days
        .get(&date)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let snapshot = &state.bundle.snapshot;
    let rows = project_daily_rows(
        scheduled,
        Some(&date),
        Some(snapshot),
        &|epoch| timezone.read().offset_at(epoch),
        true,
    );
    let statuses: BTreeMap<_, _> = scheduled
        .iter()
        .filter_map(|row| {
            let id = row.task.task_id.as_ref()?;
            let status = if state.conflicts.contains_key(id) || state.awaiting_refresh(id) {
                "要確認"
            } else if state.completed(id) {
                "完了"
            } else {
                return None;
            };
            Some((id.clone(), status))
        })
        .collect();
    let active_task_ids = state
        .sessions
        .iter()
        .map(|session| session.task_id.clone())
        .collect();
    let date_basis = timezone.read().list_date_label();
    rsx! {
        section { class: "list-screen",
            div { class: "list-toolbar",
                ListModeControl { selected_mode: ListMode::Scheduled, completed_disabled: true, on_switch_list_mode: |_| {} }
                DisplayTimezoneSettings { timezone, offline_blocked: true, on_prepare: |_| {} }
            }
            if let Some(label) = date_basis {
                p { class: "list-date-basis", "{label}" }
            }
            ListView {
                dates, rows, active_task_ids, offline_row_status: statuses,
                deviation_order: deviation_order(),
                on_toggle_deviation_order: move |_| { let next = !deviation_order(); deviation_order.set(next); },
                mutations_locked: blocked, now_epoch_ms: browser_now_epoch_ms(),
                date_input_text: date_input(), date_input_error: error(), filter_text: filter(),
                on_select_date: move |date| { date_input.set(String::new()); error.set(None); on_select.call(date); },
                on_date_input_change: move |text| { error.set(None); date_input.set(text); },
                on_submit_date_input: move |_| {
                    let base = today.map(|day| day.to_string()).unwrap_or_else(|| state.bundle.snapshot.logical_date.clone());
                    match resolve_date_input(&date_input(), &base) {
                        Ok(resolved) if state.bundle.days.contains_key(&resolved.logical_date) => {
                            error.set(None);
                            date_input.set(resolved.display_value);
                            on_select.call(resolved.logical_date);
                        }
                        Ok(_) => error.set(Some("保存した7日間の日付を選んでください。".into())),
                        Err(reason) => error.set(Some(reason.to_string())),
                    }
                },
                on_filter_change: move |text| filter.set(text),
                on_start_session: move |(task, _): (crate::SessionTask, bool)| on_action.call(Action::Start(task.task_id)),
            }
        }
    }
}
