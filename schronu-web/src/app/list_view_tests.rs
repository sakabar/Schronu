#![cfg(feature = "server")]

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};

use super::list_view::{DateButtonViewModel, ListRowViewModel, ListView};
use super::view_test_support::{
    dispatch_click, dispatch_platform_event, rebuild_with_click_listeners,
    rebuild_with_event_listeners, rebuild_with_named_event_listeners, render_with_click_listeners,
};
use crate::SessionTask;
use dioxus::html::SerializedFormData;
use dioxus::prelude::*;

#[derive(Clone)]
struct RootProps {
    dates: Vec<DateButtonViewModel>,
    rows: Vec<ListRowViewModel>,
    active_task_ids: Vec<String>,
    filter_text: String,
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
            filter_text: props.filter_text,
            mutations_locked: false,
            on_select_date: move |date: String| date_events.lock().unwrap().push(format!("date:{date}")),
            on_start_session: move |(task, is_leaf): (SessionTask, bool)| task_events
                .lock()
                .unwrap()
                .push(format!("task:{}:{}:{is_leaf}", task.task_id, task.task_name)),
            on_filter_change: move |filter: String| props.events
                .lock()
                .unwrap()
                .push(format!("filter:{filter}")),
        }
    }
}

#[test]
fn carry_lockはsession追加だけを無効化し日付選択は維持する() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut dom = VirtualDom::new_with_props(
        root,
        RootProps {
            dates: vec![DateButtonViewModel {
                logical_date: "2026-09-12".to_owned(),
                label: "土".to_owned(),
                selected: false,
            }],
            rows: vec![row("task-id", false, true)],
            active_task_ids: Vec::new(),
            filter_text: String::new(),
            events: Arc::clone(&events),
        },
    );
    dom.rebuild_in_place();
    let unlocked = dioxus::ssr::render(&dom);
    assert!(!unlocked.contains("class=\"session-start\" disabled"));

    fn locked_root(props: RootProps) -> Element {
        let date_events = Arc::clone(&props.events);
        let task_events = Arc::clone(&props.events);
        rsx! {
            ListView {
                dates: props.dates,
                rows: props.rows,
                active_task_ids: props.active_task_ids,
                filter_text: props.filter_text,
                mutations_locked: true,
                on_select_date: move |date: String| date_events.lock().unwrap().push(format!("date:{date}")),
                on_start_session: move |(task, is_leaf): (SessionTask, bool)| task_events
                    .lock()
                    .unwrap()
                    .push(format!("task:{}:{is_leaf}", task.task_id)),
                on_filter_change: move |filter: String| props.events
                    .lock()
                    .unwrap()
                    .push(format!("filter:{filter}")),
            }
        }
    }

    let mut locked = VirtualDom::new_with_props(
        locked_root,
        RootProps {
            dates: vec![DateButtonViewModel {
                logical_date: "2026-09-12".to_owned(),
                label: "土".to_owned(),
                selected: false,
            }],
            rows: vec![row("task-id", false, true)],
            active_task_ids: Vec::new(),
            filter_text: String::new(),
            events: Arc::clone(&events),
        },
    );
    let ids = rebuild_with_click_listeners(&mut locked);
    let html = dioxus::ssr::render(&locked);
    assert!(
        html.contains("class=\"session-start\"") && html.contains("disabled=true"),
        "{html}"
    );
    for id in ids {
        dispatch_click(&locked, id);
    }
    assert_eq!(*events.lock().unwrap(), ["date:2026-09-12"]);
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

fn row(task_id: &str, misses_deadline: bool, is_leaf: bool) -> ListRowViewModel {
    named_row(
        task_id,
        &format!("task {task_id}"),
        misses_deadline,
        is_leaf,
    )
}

fn named_row(
    task_id: &str,
    task_name: &str,
    misses_deadline: bool,
    is_leaf: bool,
) -> ListRowViewModel {
    ListRowViewModel {
        task: task(task_id, task_name),
        deadline_label: "____-01:00".to_owned(),
        schedule_label: "11:25-11:28".to_owned(),
        misses_deadline,
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
        rows: vec![row("leaf", true, true), row("late", false, false)],
        active_task_ids: Vec::new(),
        filter_text: String::new(),
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
    assert!(html.contains("____-01:00"));
    assert!(html.contains("11:25-11:28"));
    assert!(html.contains("task-name is-leaf"));
    assert_eq!(html.matches("deadline is-overdue").count(), 1);
    assert_eq!(html.matches("<button class=\"session-start\"").count(), 1);
    assert!(
        html.contains("aria-label=\"task leaf: セッションに追加\""),
        "{html}"
    );
    assert!(
        html.contains("class=\"session-start-compact-label\" aria-hidden=\"true\">＋</span>"),
        "{html}"
    );
    assert!(html.contains(">セッション</span>"), "{html}");
    assert!(
        !html.contains("aria-label=\"task late: セッションに追加\""),
        "{html}"
    );
    assert!(!html.contains("<a"));
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn list_rowはresponsive表示用の意味別cellとlabelを持つ() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (dom, _) = build(RootProps {
        dates: Vec::new(),
        rows: vec![row("labeled", false, true)],
        active_task_ids: Vec::new(),
        filter_text: String::new(),
        events,
    });
    let html = dioxus::ssr::render(&dom);

    assert!(
        html.contains("class=\"deadline\" data-label=\"締切\""),
        "{html}"
    );
    assert!(
        html.contains("class=\"schedule-time\" data-label=\"予定\""),
        "{html}"
    );
    assert!(html.contains("class=\"deadline-heading\""), "{html}");
    assert!(html.contains("class=\"schedule-heading\""), "{html}");
    assert!(html.contains("class=\"task-heading\""), "{html}");
    assert!(html.contains("class=\"session-heading\""), "{html}");
    assert!(html.contains("class=\"task-name-scroll\""), "{html}");
    assert!(html.contains("tabindex=0"), "{html}");
    assert!(html.contains("class=\"session-cell\""), "{html}");

    let session_heading_position = html.find("class=\"session-heading\"").unwrap();
    let schedule_heading_position = html.find("class=\"schedule-heading\"").unwrap();
    let deadline_heading_position = html.find("class=\"deadline-heading\"").unwrap();
    let task_heading_position = html.find("class=\"task-heading\"").unwrap();
    assert!(
        session_heading_position < schedule_heading_position
            && schedule_heading_position < deadline_heading_position
            && deadline_heading_position < task_heading_position,
        "{html}"
    );

    let action_position = html.find("class=\"session-cell\"").unwrap();
    let schedule_position = html.find("class=\"schedule-time\"").unwrap();
    let deadline_position = html.find("class=\"deadline\"").unwrap();
    let task_position = html.find("class=\"task-name ").unwrap();
    assert!(
        action_position < schedule_position
            && schedule_position < deadline_position
            && deadline_position < task_position,
        "{html}"
    );
}

#[test]
fn listは46rem以下で可視header付きの高密度な一行tableになる() {
    let css = include_str!("../../assets/main.css");
    let desktop_list_layout = css
        .split_once("@media (max-width: 46rem)")
        .expect("mobile list breakpoint must exist")
        .0;
    let mobile_list_layout = css
        .split_once("@media (max-width: 46rem)")
        .expect("list card breakpoint must match the 44rem table plus 2rem shell gutters")
        .1;

    assert!(
        desktop_list_layout
            .contains(".task-table th,\n.task-table td {\n    padding: 0.9rem 1rem;"),
        "desktop table spacing must remain unchanged"
    );

    for required in [
        ".task-table-scroll {\n        overflow-x: visible;",
        ".task-table {\n        display: block;\n        min-width: 0;",
        ".task-table thead {\n        display: block;",
        ".task-table thead tr,\n    .task-row {\n        display: grid;",
        "grid-template-columns: 44px 5.75rem 5.5rem minmax(0, 1fr);",
        "grid-template-areas: \"action schedule deadline task\";",
        ".task-row {\n        min-height: 32px;",
        ".task-row:not(:last-child) {\n        border-bottom: 1px solid var(--line);",
        ".task-table td {\n        display: flex;\n        min-width: 0;\n        align-items: center;\n        padding: 0.125rem 0.35rem;",
        ".task-table .session-cell {\n        padding: 0;",
        ".deadline,\n    .schedule-time {\n        font-size: 0.68rem;",
        ".task-name {\n        overflow: hidden;\n        font-size: 0.75rem;",
        ".task-name-scroll {\n        min-width: 0;\n        overflow-x: auto;\n        overscroll-behavior-inline: contain;\n        white-space: nowrap;",
        "touch-action: pan-x pan-y pinch-zoom;",
        ".session-cell .session-start {\n        width: 44px;\n        min-height: 32px;",
    ] {
        assert!(mobile_list_layout.contains(required), "missing: {required}");
    }

    for removed_card_style in [
        "gap: 0.75rem;",
        "border-radius: 1rem;",
        "box-shadow: var(--shadow);",
    ] {
        let row_rule = mobile_list_layout
            .split_once(".task-row {")
            .expect("mobile task row rule must exist")
            .1
            .split_once('}')
            .unwrap()
            .0;
        assert!(!row_rule.contains(removed_card_style), "{row_rule}");
    }
}

#[test]
fn 幅46rem以下は曜日と検索領域を36pxへ圧縮する() {
    let css = include_str!("../../assets/main.css");
    let (desktop_layout, mobile_layout) = css
        .split_once("@media (max-width: 46rem)")
        .expect("mobile list breakpoint must exist");
    let narrow_layout = mobile_layout
        .split_once("@media (max-width: 34rem)")
        .expect("narrow viewport rule must exist")
        .1;

    for unchanged in [
        ".task-list-view {\n    display: grid;\n    gap: 1rem;",
        ".task-name-filter-input {\n    width: 100%;\n    min-width: 0;\n    min-height: max(2.75rem, 44px);",
    ] {
        assert!(desktop_layout.contains(unchanged), "missing: {unchanged}");
    }

    for required in [
        ".task-list-view {\n        gap: 0.5rem;",
        ".date-pills {\n        padding: 0.125rem 0.1rem 0.25rem;",
        ".date-pill {\n        height: 36px;\n        padding: 0.35rem 0.9rem;",
        ".task-name-filter-input {\n        height: 36px;\n        min-height: 36px;\n        padding: 0.4rem 0.7rem;",
        ".task-name-filter-clear {\n        width: 36px;\n        height: 36px;\n        min-width: 36px;\n        min-height: 36px;\n        padding: 0.25rem;",
    ] {
        assert!(mobile_layout.contains(required), "missing: {required}");
    }

    assert!(
        !narrow_layout.contains(".date-pill {"),
        "date pill sizing must be owned by the 46rem breakpoint"
    );
}

#[test]
fn 幅34rem以下はbufferと曜日間隔を圧縮する() {
    let css = include_str!("../../assets/main.css");
    let narrow_layout = css
        .split_once("@media (max-width: 34rem)")
        .expect("narrow viewport rule must exist")
        .1;

    for required in [
        ".buffer-panel {\n        margin-block: 0.75rem 1rem;\n        padding: 1.25rem 1rem;",
        ".buffer-value {\n        font-size: 3rem;",
        ".date-pills {\n        gap: 0.35rem;",
    ] {
        assert!(narrow_layout.contains(required), "missing: {required}");
    }
}

#[test]
fn active_uuid_disables_every_matching_row_but_not_other_tasks() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (dom, _) = build(RootProps {
        dates: Vec::new(),
        rows: vec![
            row("same", false, true),
            row("same", false, true),
            row("other", false, true),
        ],
        active_task_ids: vec!["same".to_owned()],
        filter_text: String::new(),
        events,
    });
    let html = dioxus::ssr::render(&dom);

    assert_eq!(html.matches("disabled").count(), 2, "{html}");
    assert_eq!(html.matches(">✓</span>").count(), 2, "{html}");
    assert_eq!(html.matches(">＋</span>").count(), 1, "{html}");
    assert_eq!(html.matches("セッション追加済み").count(), 2, "{html}");
}

#[test]
fn misses_deadlineがfalseなら赤色にしない() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (dom, _) = build(RootProps {
        dates: Vec::new(),
        rows: vec![row("equal", false, false)],
        active_task_ids: Vec::new(),
        filter_text: String::new(),
        events,
    });
    assert!(!dioxus::ssr::render(&dom).contains("deadline is-overdue"));
}

#[test]
fn date_and_leaf_task_clicks_dispatch_exact_payload_once() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (date_dom, date_listeners) = build(RootProps {
        dates: vec![DateButtonViewModel {
            logical_date: "2026-09-12".to_owned(),
            label: "土".to_owned(),
            selected: false,
        }],
        rows: Vec::new(),
        active_task_ids: Vec::new(),
        filter_text: String::new(),
        events: Arc::clone(&events),
    });
    dispatch_click(&date_dom, date_listeners[0]);
    assert_eq!(*events.lock().unwrap(), ["date:2026-09-12"]);

    events.lock().unwrap().clear();
    let (task_dom, task_listeners) = build(RootProps {
        dates: Vec::new(),
        rows: vec![row("task-id", false, true)],
        active_task_ids: Vec::new(),
        filter_text: String::new(),
        events: Arc::clone(&events),
    });
    dispatch_click(&task_dom, task_listeners[0]);
    assert_eq!(*events.lock().unwrap(), ["task:task-id:task task-id:true"]);
}

#[test]
fn rank非0のtaskは開始buttonとclick_listenerを持たない() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (dom, listeners) = build(RootProps {
        dates: Vec::new(),
        rows: vec![row("non-leaf", false, false)],
        active_task_ids: Vec::new(),
        filter_text: String::new(),
        events: Arc::clone(&events),
    });

    assert!(listeners.is_empty());
    assert!(!dioxus::ssr::render(&dom).contains("session-start"));
    assert!(
        dioxus::ssr::render(&dom).contains("class=\"session-cell\"></td>"),
        "rank非0でも列揃え用cellは維持する"
    );
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn disabled_active_task_does_not_dispatch() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (dom, listeners) = build(RootProps {
        dates: Vec::new(),
        rows: vec![row("active", false, true)],
        active_task_ids: vec!["active".to_owned()],
        filter_text: String::new(),
        events: Arc::clone(&events),
    });
    for listener in listeners {
        dispatch_click(&dom, listener);
    }
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn task_name_filterは前後空白を除いた大小無視の部分一致で全segmentを絞る() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let rows = vec![
        named_row("planning", "週次 Planning", false, true),
        named_row("implementation", "実装", false, true),
        named_row("planning", "週次 Planning", false, true),
    ];
    let (dom, _) = build(RootProps {
        dates: Vec::new(),
        rows,
        active_task_ids: Vec::new(),
        filter_text: "  PLAn  ".to_owned(),
        events,
    });
    let html = dioxus::ssr::render(&dom);

    assert_eq!(html.matches("週次 Planning").count(), 4, "{html}");
    assert!(!html.contains("実装"), "{html}");

    let (japanese_dom, _) = build(RootProps {
        dates: Vec::new(),
        rows: vec![
            named_row("design", "画面設計", false, true),
            named_row("implementation", "実装", false, true),
        ],
        active_task_ids: Vec::new(),
        filter_text: "設計".to_owned(),
        events: Arc::new(Mutex::new(Vec::new())),
    });
    let japanese_html = dioxus::ssr::render(&japanese_dom);
    assert!(japanese_html.contains("画面設計"), "{japanese_html}");
    assert!(!japanese_html.contains("実装"), "{japanese_html}");
}

#[test]
fn 空白だけのfilterは全rowを表示する() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (dom, _) = build(RootProps {
        dates: Vec::new(),
        rows: vec![
            named_row("design", "設計", false, true),
            named_row("implementation", "実装", false, true),
        ],
        active_task_ids: Vec::new(),
        filter_text: "   ".to_owned(),
        events,
    });
    let html = dioxus::ssr::render(&dom);

    assert!(html.contains("設計"), "{html}");
    assert!(html.contains("実装"), "{html}");
}

#[test]
fn 検索欄は日付buttonの下かつtableの上にあり入力を通知する() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut dom = VirtualDom::new_with_props(
        root,
        RootProps {
            dates: eight_dates(),
            rows: vec![row("task-id", false, true)],
            active_task_ids: Vec::new(),
            filter_text: String::new(),
            events: Arc::clone(&events),
        },
    );
    let input_listeners = rebuild_with_event_listeners(&mut dom, "input");
    let html = dioxus::ssr::render(&dom);
    let dates_position = html.find("class=\"date-pills\"").unwrap();
    let filter_position = html.find("class=\"task-name-filter\"").unwrap();
    let table_position = html.find("class=\"task-table-scroll\"").unwrap();

    assert!(dates_position < filter_position && filter_position < table_position);
    assert!(html.contains("aria-label=\"タスク名を検索\""), "{html}");
    assert!(html.contains("placeholder=\"タスク名を検索\""), "{html}");
    assert_eq!(input_listeners.len(), 1);

    dispatch_platform_event(
        &dom,
        "input",
        input_listeners[0],
        Box::new(SerializedFormData::new("設計".to_owned(), Vec::new())),
    );
    assert_eq!(*events.lock().unwrap(), ["filter:設計"]);
}

#[test]
fn clear_buttonは入力中だけ表示して空文字を一度通知する() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (dom, listeners) = build(RootProps {
        dates: Vec::new(),
        rows: Vec::new(),
        active_task_ids: Vec::new(),
        filter_text: "設計".to_owned(),
        events: Arc::clone(&events),
    });
    let html = dioxus::ssr::render(&dom);

    assert!(html.contains("aria-label=\"検索文字列をクリア\""), "{html}");
    assert_eq!(listeners.len(), 1);
    dispatch_click(&dom, listeners[0]);
    assert_eq!(*events.lock().unwrap(), ["filter:"]);

    let (empty_dom, _) = build(RootProps {
        dates: Vec::new(),
        rows: Vec::new(),
        active_task_ids: Vec::new(),
        filter_text: String::new(),
        events,
    });
    assert!(!dioxus::ssr::render(&empty_dom).contains("検索文字列をクリア"));
}

#[test]
fn filter一致なしはstatusを表示してtask操作を生成しない() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (dom, listeners) = build(RootProps {
        dates: Vec::new(),
        rows: vec![named_row("design", "設計", false, true)],
        active_task_ids: Vec::new(),
        filter_text: "実装".to_owned(),
        events,
    });
    let html = dioxus::ssr::render(&dom);

    assert!(html.contains("role=\"status\""), "{html}");
    assert!(html.contains("一致するタスクがありません。"), "{html}");
    assert!(!html.contains("session-start"), "{html}");
    assert_eq!(listeners.len(), 1, "input listener is not a click listener");
}

#[test]
fn task_name_filterはmobile幅とclearのtouch_targetを維持する() {
    let css = include_str!("../../assets/main.css");

    for required in [
        ".task-name-filter {",
        "width: 100%;",
        "min-width: 0;",
        ".task-name-filter-clear {",
        "min-width: max(2.75rem, 44px);",
        "min-height: max(2.75rem, 44px);",
    ] {
        assert!(css.contains(required), "missing: {required}");
    }
}

#[component]
fn StatefulFilterHarness(events: Rc<RefCell<Vec<String>>>) -> Element {
    let mut show_list = use_signal(|| true);
    let mut filter_text = use_signal(String::new);
    let date_events = Rc::clone(&events);
    let session_events = Rc::clone(&events);

    rsx! {
        button {
            r#type: "button",
            aria_label: "一覧tab切替",
            onclick: move |_| {
                let next = !*show_list.read();
                show_list.set(next);
            },
            "一覧tab切替"
        }
        if *show_list.read() {
            ListView {
                dates: Vec::new(),
                rows: vec![
                    named_row("design", "画面設計", false, false),
                    named_row("implementation", "実装", false, false),
                ],
                active_task_ids: Vec::new(),
                filter_text: filter_text.read().clone(),
                on_select_date: move |_| date_events.borrow_mut().push("server:date".to_owned()),
                on_start_session: move |_| session_events.borrow_mut().push("storage:session".to_owned()),
                on_filter_change: move |filter| filter_text.set(filter),
            }
        }
    }
}

#[test]
fn filter入力とclearは副作用なく再描画されtab往復でも条件を保持する() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut dom = VirtualDom::new_with_props(
        StatefulFilterHarness,
        StatefulFilterHarnessProps {
            events: Rc::clone(&events),
        },
    );
    let listeners = rebuild_with_named_event_listeners(&mut dom);
    let toggle_id = listeners
        .iter()
        .find_map(|(name, id)| (name == "click").then_some(*id))
        .unwrap();
    let input_id = listeners
        .iter()
        .find_map(|(name, id)| (name == "input").then_some(*id))
        .unwrap();

    dispatch_platform_event(
        &dom,
        "input",
        input_id,
        Box::new(SerializedFormData::new("設計".to_owned(), Vec::new())),
    );
    let clear_ids = render_with_click_listeners(&mut dom);
    let filtered_html = dioxus::ssr::render(&dom);
    assert!(filtered_html.contains("画面設計"), "{filtered_html}");
    assert!(!filtered_html.contains(">実装<"), "{filtered_html}");
    assert!(events.borrow().is_empty());

    dispatch_click(&dom, toggle_id);
    render_with_click_listeners(&mut dom);
    dispatch_click(&dom, toggle_id);
    let restored_clear_ids = render_with_click_listeners(&mut dom);
    let restored_html = dioxus::ssr::render(&dom);
    assert!(restored_html.contains("value=\"設計\""), "{restored_html}");
    assert!(restored_html.contains("画面設計"), "{restored_html}");
    assert!(!restored_html.contains(">実装<"), "{restored_html}");
    assert!(events.borrow().is_empty());

    assert_eq!(clear_ids.len(), 1);
    assert_eq!(restored_clear_ids.len(), 1);
    dispatch_click(&dom, restored_clear_ids[0]);
    render_with_click_listeners(&mut dom);
    let cleared_html = dioxus::ssr::render(&dom);
    assert!(cleared_html.contains("画面設計"), "{cleared_html}");
    assert!(cleared_html.contains(">実装<"), "{cleared_html}");
    assert!(!cleared_html.contains("検索文字列をクリア"));
    assert!(events.borrow().is_empty());
}
