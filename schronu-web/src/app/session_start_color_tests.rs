use super::*;

#[derive(Clone)]
struct StartColorProps {
    rows: Vec<ListRowViewModel>,
    now_epoch_ms: i64,
    all: bool,
    locked: bool,
    active: bool,
    events: Arc<Mutex<Vec<String>>>,
}

fn start_color_root(props: StartColorProps) -> Element {
    rsx! {
        ListView {
            dates: Vec::new(),
            rows: props.rows,
            now_epoch_ms: props.now_epoch_ms,
            active_task_ids: if props.active { vec!["color-task".to_owned()] } else { Vec::new() },
            date_input_text: String::new(),
            date_input_error: None,
            filter_text: String::new(),
            all_tasks_status: props.all.then_some(crate::app::list_view::AllTasksViewStatus::Loaded),
            mutations_locked: props.locked,
            on_select_date: move |_| {},
            on_date_input_change: move |_| {},
            on_submit_date_input: move |_| {},
            on_start_session: move |(task, _): (SessionTask, bool)| props.events.lock().unwrap().push(task.task_id),
            on_filter_change: move |_| {},
        }
    }
}

#[test]
fn session_start_color_uses_task_time_with_inclusive_boundary_in_both_lists() {
    for all in [false, true] {
        for (start, future) in [
            (None, false),
            (Some(999), false),
            (Some(1_000), false),
            (Some(1_001), true),
        ] {
            let mut first = row("color-task", false, true);
            first.task.start_time_epoch_ms = start;
            first.defer_plan = None;
            if all {
                first.schedule_display = ScheduleDisplayViewModel::AllTasksDate {
                    label: "2026/09/05(土)".to_owned(),
                };
            }
            let mut second = first.clone();
            second.row_key.push_str(":split");
            second.schedule_display = ScheduleDisplayViewModel::Daily {
                start_hh_mm: "23:59".to_owned(),
                duration_minutes: Some(5),
            };
            let events = Arc::new(Mutex::new(Vec::new()));
            let mut dom = VirtualDom::new_with_props(
                start_color_root,
                StartColorProps {
                    rows: vec![first, second],
                    now_epoch_ms: 1_000,
                    all,
                    locked: false,
                    active: false,
                    events: events.clone(),
                },
            );
            let listeners = rebuild_with_click_listeners(&mut dom);
            let html = dioxus::ssr::render(&dom);
            assert_eq!(
                html.matches("class=\"session-start is-future\"").count(),
                if future { 2 } else { 0 },
                "{html}"
            );
            assert_eq!(
                html.matches("class=\"session-start\"").count(),
                if future { 0 } else { 2 },
                "{html}"
            );
            for id in listeners {
                dispatch_click(&dom, id);
            }
            assert_eq!(*events.lock().unwrap(), vec!["color-task", "color-task"]);
        }
    }
}

#[test]
fn future_session_start_preserves_active_and_lock_guards() {
    for (active, locked) in [(true, false), (false, true)] {
        let mut task_row = row("color-task", false, true);
        task_row.task.start_time_epoch_ms = Some(1_001);
        task_row.defer_plan = None;
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut dom = VirtualDom::new_with_props(
            start_color_root,
            StartColorProps {
                rows: vec![task_row],
                now_epoch_ms: 1_000,
                all: false,
                locked,
                active,
                events: events.clone(),
            },
        );
        let listeners = rebuild_with_click_listeners(&mut dom);
        let html = dioxus::ssr::render(&dom);
        assert!(html.contains("class=\"session-start is-future\""), "{html}");
        assert!(html.contains("disabled=true"), "{html}");
        for id in listeners {
            dispatch_click(&dom, id);
        }
        assert!(events.lock().unwrap().is_empty());
    }
}

#[cfg(feature = "web")]
#[test]
fn session_start_color_follows_existing_tick_through_page_model() {
    use crate::app::component_models::BrowserPageModel;
    use crate::client::work_sessions::{KeyValueStorage, StorageError};
    struct EmptyStorage;
    impl KeyValueStorage for EmptyStorage {
        fn get(&self, _key: &str) -> Result<Option<String>, StorageError> {
            Ok(None)
        }
        fn set(&self, _key: &str, _value: &str) -> Result<(), StorageError> {
            Ok(())
        }
    }
    let storage = EmptyStorage;
    let (mut state, _) = crate::app::component_runtime::initialize_client(&storage, 999);
    let mut task_row = row("color-task", false, true);
    task_row.task.start_time_epoch_ms = Some(1_000);
    for (now, future) in [(999, true), (1_000, false), (1_001, false), (999, true)] {
        assert_eq!(
            crate::app::component_runtime::reduce_component_action_at(
                &mut state,
                &storage,
                0,
                ComponentAction::Tick {
                    wall_now_epoch_ms: now
                }
            ),
            crate::client::state::ClientEffect::None
        );
        let model = BrowserPageModel::from_state(&state);
        assert_eq!(model.now_epoch_ms, now);
        let mut dom = VirtualDom::new_with_props(
            start_color_root,
            StartColorProps {
                rows: vec![task_row.clone()],
                now_epoch_ms: model.now_epoch_ms,
                all: false,
                locked: false,
                active: false,
                events: Arc::new(Mutex::new(Vec::new())),
            },
        );
        dom.rebuild_in_place();
        assert_eq!(
            dioxus::ssr::render(&dom).contains("class=\"session-start is-future\""),
            future
        );
    }
}
