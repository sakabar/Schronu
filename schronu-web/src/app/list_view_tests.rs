#![cfg(feature = "server")]

use std::sync::{Arc, Mutex};

use super::list_view::{DateButtonViewModel, ListRowViewModel, ListView};
use super::view_test_support::{dispatch_click, rebuild_with_click_listeners};
use crate::SessionTask;
use dioxus::prelude::*;

#[derive(Clone)]
struct RootProps {
    dates: Vec<DateButtonViewModel>,
    rows: Vec<ListRowViewModel>,
    active_task_ids: Vec<String>,
    tick_now_epoch_ms: i64,
    events: Arc<Mutex<Vec<String>>>,
}

fn root(props: RootProps) -> Element {
    let date_events = Arc::clone(&props.events);
    let task_events = Arc::clone(&props.events);
    rsx! {
        ListView {
            dates: props.dates,
            rows: props.rows,
            active_task_ids: props.active_task_ids,
            tick_now_epoch_ms: props.tick_now_epoch_ms,
            on_select_date: move |date: String| date_events.lock().unwrap().push(format!("date:{date}")),
            on_start_session: move |task: SessionTask| task_events
                .lock()
                .unwrap()
                .push(format!("task:{}:{}", task.task_id, task.task_name)),
        }
    }
}

fn build(props: RootProps) -> (VirtualDom, Vec<dioxus::dioxus_core::ElementId>) {
    let mut dom = VirtualDom::new_with_props(root, props);
    let listeners = rebuild_with_click_listeners(&mut dom);
    (dom, listeners)
}

fn task(task_id: &str, task_name: &str) -> SessionTask {
    SessionTask {
        task_id: task_id.to_owned(),
        task_name: task_name.to_owned(),
        estimated_work_seconds: 900,
        actual_work_seconds: 300,
    }
}

fn row(task_id: &str, deadline: Option<i64>, is_leaf: bool) -> ListRowViewModel {
    ListRowViewModel {
        task: task(task_id, &format!("task {task_id}")),
        deadline_label: Some("09/06 09:00".to_owned()),
        schedule_label: "11:25-11:28".to_owned(),
        deadline_epoch_ms: deadline,
        is_leaf,
    }
}

fn eight_dates() -> Vec<DateButtonViewModel> {
    ["土 今日", "日 明日", "月", "火", "水", "木", "金", "土"]
        .into_iter()
        .enumerate()
        .map(|(index, label)| DateButtonViewModel {
            logical_date: format!("2026-09-{:02}", 5 + index),
            label: label.to_owned(),
            selected: index == 0,
        })
        .collect()
}

#[test]
fn list_renders_eight_dates_selected_row_fields_and_visual_states() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (dom, _) = build(RootProps {
        dates: eight_dates(),
        rows: vec![
            row("leaf", Some(999), true),
            row("late", Some(1_001), false),
        ],
        active_task_ids: Vec::new(),
        tick_now_epoch_ms: 1_000,
        events: Arc::clone(&events),
    });
    let html = dioxus::ssr::render(&dom);

    assert_eq!(
        html.matches("<button class=\"date-pill").count(),
        8,
        "{html}"
    );
    assert!(html.contains("土 今日"));
    assert!(html.contains("日 明日"));
    assert!(html.contains("date-pill is-selected"));
    assert!(html.contains("09/06 09:00"));
    assert!(html.contains("11:25-11:28"));
    assert!(html.contains("task-name is-leaf"));
    assert_eq!(html.matches("deadline is-overdue").count(), 1);
    assert_eq!(html.matches("セッション").count(), 2);
    assert!(!html.contains("<a"));
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn active_uuid_disables_every_matching_row_but_not_other_tasks() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (dom, _) = build(RootProps {
        dates: Vec::new(),
        rows: vec![
            row("same", None, false),
            row("same", None, false),
            row("other", None, false),
        ],
        active_task_ids: vec!["same".to_owned()],
        tick_now_epoch_ms: 0,
        events,
    });
    let html = dioxus::ssr::render(&dom);

    assert_eq!(html.matches("disabled").count(), 2, "{html}");
}

#[test]
fn deadline_equal_to_now_is_not_overdue() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (dom, _) = build(RootProps {
        dates: Vec::new(),
        rows: vec![row("equal", Some(1_000), false)],
        active_task_ids: Vec::new(),
        tick_now_epoch_ms: 1_000,
        events,
    });
    assert!(!dioxus::ssr::render(&dom).contains("deadline is-overdue"));
}

#[test]
fn date_and_task_clicks_dispatch_exact_payload_once() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (date_dom, date_listeners) = build(RootProps {
        dates: vec![DateButtonViewModel {
            logical_date: "2026-09-12".to_owned(),
            label: "土".to_owned(),
            selected: false,
        }],
        rows: Vec::new(),
        active_task_ids: Vec::new(),
        tick_now_epoch_ms: 0,
        events: Arc::clone(&events),
    });
    dispatch_click(&date_dom, date_listeners[0]);
    assert_eq!(*events.lock().unwrap(), ["date:2026-09-12"]);

    events.lock().unwrap().clear();
    let (task_dom, task_listeners) = build(RootProps {
        dates: Vec::new(),
        rows: vec![row("task-id", None, false)],
        active_task_ids: Vec::new(),
        tick_now_epoch_ms: 0,
        events: Arc::clone(&events),
    });
    dispatch_click(&task_dom, task_listeners[0]);
    assert_eq!(*events.lock().unwrap(), ["task:task-id:task task-id"]);
}

#[test]
fn disabled_active_task_does_not_dispatch() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (dom, listeners) = build(RootProps {
        dates: Vec::new(),
        rows: vec![row("active", None, false)],
        active_task_ids: vec!["active".to_owned()],
        tick_now_epoch_ms: 0,
        events: Arc::clone(&events),
    });
    for listener in listeners {
        dispatch_click(&dom, listener);
    }
    assert!(events.lock().unwrap().is_empty());
}
