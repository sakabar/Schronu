use super::{Action, DisplayTimezone, OfflineState};
use crate::app::component::DisplayTimezoneSettings;
use crate::app::component_models::browser_now_epoch_ms;
use crate::app::list_view::{CompletedListView, DateButtonViewModel, ListModeControl, ListView};
use crate::client::date_buttons::{logical_date_buttons_for_mode, scheduled_date_button};
use crate::client::date_input::resolve_date_input_for_mode;
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
    mut completed_ascending: Signal<bool>,
    timezone: Signal<DisplayTimezone>,
    today: Option<NaiveDate>,
    blocked: bool,
    on_action: EventHandler<Action>,
    on_select: EventHandler<String>,
) -> Element {
    let mut deviation_order = use_signal(|| false);
    let mut date_input = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut mode = use_signal(|| ListMode::Scheduled);
    let base = today
        .map(|day| day.to_string())
        .unwrap_or_else(|| state.bundle.snapshot.logical_date.clone());
    let date = if mode() == ListMode::Scheduled && !state.bundle.days.contains_key(&date) {
        state.bundle.snapshot.logical_date.clone()
    } else {
        date
    };
    let on_select_date = move |date| {
        date_input.set(String::new());
        error.set(None);
        on_select.call(date);
    };
    let on_date_input_change = move |text| {
        error.set(None);
        date_input.set(text);
    };
    let input_state = state.clone();
    let input_base = base.clone();
    let on_submit_date_input =
        move |_| match resolve_date_input_for_mode(&date_input(), &input_base, mode()) {
            Ok(resolved)
                if mode() == ListMode::Completed
                    || input_state.bundle.days.contains_key(&resolved.logical_date) =>
            {
                error.set(None);
                date_input.set(resolved.display_value);
                on_select.call(resolved.logical_date);
            }
            Ok(_) => error.set(Some("保存した7日間の日付を選んでください。".into())),
            Err(reason) => error.set(Some(reason.to_string())),
        };
    let on_filter_change = move |text| filter.set(text);
    let display_offset = use_callback(move |epoch| timezone.read().offset_at(epoch));
    let content = if mode() == ListMode::Completed {
        let dates = logical_date_buttons_for_mode(&base, ListMode::Completed)
            .unwrap_or_default()
            .into_iter()
            .map(|day| DateButtonViewModel {
                selected: day.logical_date == date,
                logical_date: day.logical_date,
                label: day.label,
            })
            .collect();
        rsx! {
            CompletedListView {
                dates, offline_rows: state.completions_on(&date), selected_logical_date: date.clone(),
                ascending: completed_ascending(), display_offset,
                on_toggle_order: move |_| { let next = !completed_ascending(); completed_ascending.set(next); },
                date_input_text: date_input(), date_input_error: error(), filter_text: filter(),
                on_select_date, on_date_input_change, on_submit_date_input, on_filter_change,
            }
        }
    } else {
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
        let mut rows = project_daily_rows(
            state.visible_schedule(&date),
            Some(&date),
            Some(&state.bundle.snapshot),
            &|epoch| timezone.read().offset_at(epoch),
            true,
        );
        for row in &mut rows {
            if let Some(id) = row.task.task_id.as_deref() {
                row.is_leaf = state.effective_leaf(id, row.is_leaf);
            }
        }
        let statuses: BTreeMap<_, _> = rows
            .iter()
            .filter_map(|row| {
                let id = row.task.task_id.as_ref()?;
                state.needs_review(id).then(|| (id.clone(), "要確認"))
            })
            .collect();
        let active_task_ids = state
            .sessions
            .iter()
            .map(|session| session.task_id.clone())
            .collect();
        rsx! {
            ListView {
                dates, rows, active_task_ids, offline_row_status: statuses,
                deviation_order: deviation_order(),
                on_toggle_deviation_order: move |_| { let next = !deviation_order(); deviation_order.set(next); },
                mutations_locked: blocked, now_epoch_ms: browser_now_epoch_ms(),
                date_input_text: date_input(), date_input_error: error(), filter_text: filter(),
                on_select_date, on_date_input_change, on_submit_date_input, on_filter_change,
                on_start_session: move |(task, _): (crate::SessionTask, bool)| on_action.call(Action::Start(task.task_id)),
            }
        }
    };
    let date_basis = timezone.read().list_date_label();
    rsx! {
        section { class: "list-screen",
            div { class: "list-toolbar",
                ListModeControl {
                    selected_mode: mode(), on_switch_list_mode: move |next| {
                        date_input.set(String::new());
                        error.set(None);
                        mode.set(next);
                    }
                }
                DisplayTimezoneSettings { timezone, offline_blocked: true, on_prepare: |_| {} }
            }
            if let Some(label) = date_basis {
                p { class: "list-date-basis", "{label}" }
            }
            {content}
        }
    }
}
