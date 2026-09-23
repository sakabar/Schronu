#![cfg(feature = "server")]

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};

use super::component_runtime::{
    component_action_from_date_button, component_action_from_date_input, ComponentAction,
};
use super::list_view::{
    DateButtonViewModel, DeferConfirmationKind, DeferConfirmationViewModel, ListRowViewModel,
    ListView,
};
use super::view_test_support::{
    dispatch_click, dispatch_platform_event, rebuild_with_click_listeners,
    rebuild_with_event_listeners, rebuild_with_named_event_listeners, render_with_click_listeners,
};
use crate::client::date_input::DateInputState;
use crate::{DeadlineDisplayKind, DeferMode, SessionTask, TaskDisplayKind};
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
    let date_input_events = Arc::clone(&props.events);
    let date_submit_events = Arc::clone(&props.events);
    let task_events = Arc::clone(&props.events);
    let defer_events = Arc::clone(&props.events);
    rsx! {
        ListView {
            dates: props.dates,
            rows: props.rows,
            active_task_ids: props.active_task_ids,
            date_input_text: String::new(),
            date_input_error: None,
            filter_text: props.filter_text,
            mutations_locked: false,
            on_select_date: move |date: String| date_events.lock().unwrap().push(format!("date:{date}")),
            on_date_input_change: move |date: String| date_input_events
                .lock()
                .unwrap()
                .push(format!("date-input:{date}")),
            on_submit_date_input: move |_| date_submit_events
                .lock()
                .unwrap()
                .push("date-submit".to_owned()),
            on_start_session: move |(task, is_leaf): (SessionTask, bool)| task_events
                .lock()
                .unwrap()
                .push(format!("task:{}:{}:{is_leaf}", task.task_id, task.task_name)),
            on_defer_task: move |(task_id, _): (String, crate::DeferPlan)| defer_events
                .lock()
                .unwrap()
                .push(format!("defer:{task_id}")),
            on_filter_change: move |filter: String| props.events
                .lock()
                .unwrap()
                .push(format!("filter:{filter}")),
        }
    }
}

fn globally_blocked_root(props: RootProps) -> Element {
    let defer_events = Arc::clone(&props.events);
    rsx! {
        ListView {
            dates: props.dates,
            rows: props.rows,
            active_task_ids: props.active_task_ids,
            date_input_text: String::new(),
            date_input_error: None,
            filter_text: props.filter_text,
            mutation_globally_blocked: true,
            on_select_date: move |_| {},
            on_date_input_change: move |_| {},
            on_submit_date_input: move |_| {},
            on_start_session: move |_| {},
            on_defer_task: move |(task_id, _): (String, crate::DeferPlan)| defer_events
                .lock()
                .unwrap()
                .push(format!("defer:{task_id}")),
            on_filter_change: move |_| {},
        }
    }
}

#[test]
fn mutation_safety全体停止は先送りbuttonを無効化する() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut dom = VirtualDom::new_with_props(
        globally_blocked_root,
        RootProps {
            dates: Vec::new(),
            rows: vec![row("task-id", false, true)],
            active_task_ids: Vec::new(),
            filter_text: String::new(),
            events: Arc::clone(&events),
        },
    );
    let listeners = rebuild_with_click_listeners(&mut dom);
    let html = dioxus::ssr::render(&dom);
    assert!(!html.contains("class=\"session-start\" disabled"), "{html}");
    assert!(html.contains("先送り\" disabled=true"), "{html}");

    dispatch_click(&dom, listeners[1]);
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn carry_lockはsession追加と先送りを無効化し日付選択は維持する() {
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
    assert!(!unlocked.contains("先送り\" disabled=true"));

    fn locked_root(props: RootProps) -> Element {
        let date_events = Arc::clone(&props.events);
        let task_events = Arc::clone(&props.events);
        rsx! {
            ListView {
                dates: props.dates,
                rows: props.rows,
                active_task_ids: props.active_task_ids,
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: props.filter_text,
                mutations_locked: true,
                on_select_date: move |date: String| date_events.lock().unwrap().push(format!("date:{date}")),
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_start_session: move |(task, is_leaf): (SessionTask, bool)| task_events
                    .lock()
                    .unwrap()
                    .push(format!("task:{}:{is_leaf}", task.task_id)),
                on_defer_task: move |_| {},
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
    assert!(html.contains("先送り\" disabled=true"), "{html}");
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
        row_key: format!("row:{task_id}"),
        task: task(task_id, task_name),
        deadline_label: "____-01:00".to_owned(),
        schedule_label: "11:25-11:28".to_owned(),
        misses_deadline,
        task_display_kind: TaskDisplayKind::NonRepetitive,
        deadline_display_kind: if misses_deadline {
            DeadlineDisplayKind::Overrun
        } else {
            DeadlineDisplayKind::None
        },
        is_leaf,
        defer_plan: Some(crate::DeferPlan {
            mode: DeferMode::Normal,
            requested_pending_until_epoch_ms: 1_000,
            effective_pending_until_epoch_ms: None,
            repetition_interval_days: None,
        }),
        defer_confirmation: None,
    }
}

#[test]
fn listはtask種類と締切種類を親子にかかわらずclassへ反映する() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut fixed = named_row("fixed", "fixed leaf", false, true);
    fixed.task_display_kind = TaskDisplayKind::Fixed;
    fixed.deadline_display_kind = DeadlineDisplayKind::None;
    let mut repetitive = named_row("repetitive", "repetitive parent", false, false);
    repetitive.task_display_kind = TaskDisplayKind::Repetitive;
    repetitive.deadline_display_kind = DeadlineDisplayKind::Today;
    let mut non_repetitive = named_row("one-shot", "one-shot leaf", false, true);
    non_repetitive.task_display_kind = TaskDisplayKind::NonRepetitive;
    non_repetitive.deadline_display_kind = DeadlineDisplayKind::Future;
    let mut overrun_parent = named_row("overrun", "overrun parent", true, false);
    overrun_parent.deadline_display_kind = DeadlineDisplayKind::Overrun;
    let (dom, _) = build(RootProps {
        dates: Vec::new(),
        rows: vec![fixed, repetitive, non_repetitive, overrun_parent],
        active_task_ids: Vec::new(),
        filter_text: String::new(),
        events,
    });
    let html = dioxus::ssr::render(&dom);

    assert!(
        html.contains("class=\"task-name task-kind-fixed is-leaf\""),
        "{html}"
    );
    assert!(
        html.contains("class=\"task-name task-kind-repetitive\""),
        "{html}"
    );
    assert!(
        html.contains("class=\"task-name task-kind-non-repetitive is-leaf\""),
        "{html}"
    );
    assert!(
        html.contains("class=\"deadline deadline-kind-today\""),
        "{html}"
    );
    assert!(
        html.contains("class=\"deadline deadline-kind-future\""),
        "{html}"
    );
    assert_eq!(
        html.matches("deadline deadline-kind-overrun").count(),
        1,
        "{html}"
    );
}

#[test]
fn list配色は意味別tokenを使いleafは太字だけを担う() {
    let css = include_str!("../../assets/main.css");
    for token in [
        "--task-fixed: #516f82;",
        "--task-repetitive: #0069c2;",
        "--task-non-repetitive: #a44a00;",
        "--deadline-today: #9a5a00;",
        "--deadline-future: #196846;",
    ] {
        assert!(css.contains(token), "missing token: {token}");
    }
    assert!(css.contains(".deadline.deadline-kind-overrun {\n    color: var(--red);"));
    assert!(css.contains(".task-name.task-kind-fixed {\n    color: var(--task-fixed);"));
    assert!(css.contains(".task-name.task-kind-repetitive {\n    color: var(--task-repetitive);"));
    assert!(css
        .contains(".task-name.task-kind-non-repetitive {\n    color: var(--task-non-repetitive);"));
    let leaf_rule = css
        .split_once(".task-name.is-leaf {")
        .expect("leaf rule")
        .1
        .split_once('}')
        .unwrap()
        .0;
    assert!(leaf_rule.contains("font-weight: 750;"));
    assert!(!leaf_rule.contains("color:"), "{leaf_rule}");
}

fn confirmation_row(task_id: &str, kind: DeferConfirmationKind) -> ListRowViewModel {
    let mut row = named_row(task_id, &format!("task {task_id}"), false, true);
    row.defer_confirmation = Some(DeferConfirmationViewModel {
        kind,
        detail_label: "9/12 05:59".to_owned(),
    });
    row.defer_plan.as_mut().unwrap().mode = match kind {
        DeferConfirmationKind::DeadlineLimited => DeferMode::DeadlineLimited,
        DeferConfirmationKind::RoutinePeriod => DeferMode::RoutinePeriod,
    };
    row
}

#[test]
fn task_name_matchesはtrim_unicode_lowercase部分一致と空検索を共有する() {
    use crate::client::view_projection::task_name_matches;

    assert!(task_name_matches("  PLAn  ", "週次 Planning"));
    assert!(task_name_matches("ω", "Ωタスク"));
    assert!(task_name_matches("   ", "任意"));
    assert!(!task_name_matches("設計", "実装"));
}

#[test]
fn all一覧は先頭buttonとinline状態を表示する() {
    use super::list_view::AllTasksViewStatus;

    let mut dom = VirtualDom::new(|| {
        rsx! {
            ListView {
                dates: eight_dates(),
                rows: Vec::new(),
                active_task_ids: Vec::new(),
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: "hidden".to_owned(),
                all_tasks_status: Some(AllTasksViewStatus::Loading),
                on_select_date: move |_| {},
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_start_session: move |_| {},
                on_filter_change: move |_| {},
            }
        }
    });
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);
    let all_position = html.find("全て").unwrap();
    let today_position = html.find("土 今日").unwrap();
    assert!(all_position < today_position, "{html}");
    assert_eq!(html.matches("aria-pressed=true").count(), 1, "{html}");
    assert!(html.contains("全てのタスクを取得中です。"), "{html}");
    assert!(html.contains("class=\"date-jump-form\""), "{html}");
    assert!(!html.contains("class=\"task-name-filter\""), "{html}");
    assert!(!html.contains("class=\"task-table-scroll\""), "{html}");
}

#[test]
fn all取得失敗と無効化はinlineから再試行できる() {
    use super::list_view::AllTasksViewStatus;

    #[derive(Clone)]
    struct StatusHarnessProps {
        status: AllTasksViewStatus,
        events: Arc<Mutex<Vec<String>>>,
    }
    fn status_harness(props: StatusHarnessProps) -> Element {
        let retry_events = Arc::clone(&props.events);
        rsx! {
            ListView {
                dates: Vec::new(),
                rows: Vec::new(),
                active_task_ids: Vec::new(),
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: String::new(),
                all_tasks_status: Some(props.status),
                on_select_date: move |_| {},
                on_select_all_tasks: move |_| props.events.lock().unwrap().push("refresh".to_owned()),
                on_retry_all_tasks: move |_| retry_events.lock().unwrap().push("retry".to_owned()),
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_start_session: move |_| {},
                on_filter_change: move |_| {},
            }
        }
    }

    let failed_events = Arc::new(Mutex::new(Vec::new()));
    let mut failed = VirtualDom::new_with_props(
        status_harness,
        StatusHarnessProps {
            status: AllTasksViewStatus::Failed("再取得してください。".to_owned()),
            events: Arc::clone(&failed_events),
        },
    );
    let failed_ids = rebuild_with_click_listeners(&mut failed);
    let failed_html = dioxus::ssr::render(&failed);
    assert!(
        failed_html.contains("再取得してください。"),
        "{failed_html}"
    );
    dispatch_click(&failed, *failed_ids.last().unwrap());
    assert_eq!(*failed_events.lock().unwrap(), ["retry"]);

    let invalidated_events = Arc::new(Mutex::new(Vec::new()));
    let mut invalidated = VirtualDom::new_with_props(
        status_harness,
        StatusHarnessProps {
            status: AllTasksViewStatus::Invalidated,
            events: Arc::clone(&invalidated_events),
        },
    );
    let invalidated_ids = rebuild_with_click_listeners(&mut invalidated);
    let invalidated_html = dioxus::ssr::render(&invalidated);
    assert!(
        invalidated_html.contains("タスクが更新されました。"),
        "{invalidated_html}"
    );
    dispatch_click(&invalidated, *invalidated_ids.last().unwrap());
    assert_eq!(*invalidated_events.lock().unwrap(), ["refresh"]);
}

#[test]
fn all一覧は500行ずつ描画し親は空操作cellとなる() {
    use super::list_view::AllTasksViewStatus;

    let rows = (0..501)
        .map(|index| {
            named_row(
                &format!("task-{index}"),
                &format!("task {index}"),
                false,
                index != 499,
            )
        })
        .map(|mut row| {
            row.defer_plan = None;
            row
        })
        .collect();
    #[component]
    fn AllRowsHarness(rows: Vec<ListRowViewModel>) -> Element {
        rsx! {
        ListView {
                dates: Vec::new(),
                rows,
                active_task_ids: vec!["task-0".to_owned()],
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: String::new(),
                all_tasks_status: Some(AllTasksViewStatus::Loaded),
                visible_row_limit: Some(500),
                on_select_date: move |_| {},
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_start_session: move |_| {},
                on_filter_change: move |_| {},
            }
        }
    }
    let mut dom = VirtualDom::new_with_props(AllRowsHarness, AllRowsHarnessProps { rows });
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);
    assert_eq!(html.matches("class=\"task-row\"").count(), 500, "{html}");
    assert!(html.contains("さらに表示"), "{html}");
    assert!(
        html.contains("class=\"task-table all-task-table\""),
        "{html}"
    );
    assert!(!html.contains("先送り"), "{html}");
    assert!(html.contains("セッション追加済み"), "{html}");
    assert!(html.contains("<td class=\"session-cell\"></td>"), "{html}");
}

#[test]
fn 日付別一覧は500行を超えても段階描画しない() {
    #[component]
    fn DailyRowsHarness(rows: Vec<ListRowViewModel>) -> Element {
        rsx! {
            ListView {
                dates: Vec::new(),
                rows,
                active_task_ids: Vec::new(),
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: String::new(),
                on_select_date: move |_| {},
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_start_session: move |_| {},
                on_filter_change: move |_| {},
            }
        }
    }

    let rows = (0..501)
        .map(|index| row(&format!("daily-{index}"), false, true))
        .collect();
    let mut dom = VirtualDom::new_with_props(DailyRowsHarness, DailyRowsHarnessProps { rows });
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);
    assert_eq!(html.matches("class=\"task-row\"").count(), 501, "{html}");
    assert!(!html.contains("さらに表示"), "{html}");
}

#[test]
fn all一覧の列幅はviewportによらず固定する() {
    let css = include_str!("../../assets/main.css");
    let universal = css.split_once("@media (max-width: 46rem)").unwrap().0;
    assert!(universal.contains(
        ".task-table.all-task-table thead tr,\n.task-table.all-task-table .task-row {\n    grid-template-columns: 44px 8.25rem 5.5rem minmax(0, 1fr);"
    ));
    for breakpoint in ["320px", "360px", "46rem", "1024px"] {
        assert!(!css.contains(&format!(
            "@media (max-width: {breakpoint}) {{\n    .all-task-table"
        )));
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
        9,
        "{html}"
    );
    assert!(html.contains("土 今日"));
    assert!(html.contains("日 明日"));
    assert!(html.contains("date-pill is-selected"));
    assert!(html.contains("____-01:00"));
    assert!(html.contains("11:25-11:28"));
    assert!(html.contains("task-name task-kind-non-repetitive is-leaf"));
    assert_eq!(html.matches("deadline deadline-kind-overrun").count(), 1);
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
    assert!(html.contains("aria-label=\"task leaf: 先送り\""), "{html}");
    assert!(html.contains("先送り</span>"), "{html}");
    assert!(
        html.contains("class=\"task-defer-compact-label\" aria-hidden=\"true\">→</span>"),
        "{html}"
    );
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
fn listは全幅で可視header付きの高密度な一行tableになる() {
    let css = include_str!("../../assets/main.css");
    let (universal_list_layout, responsive_layout) = css
        .split_once("@media (max-width: 46rem)")
        .expect("navigation breakpoint must exist");
    let mobile_only_layout = responsive_layout
        .split_once("@media (max-width: 34rem)")
        .expect("narrow viewport rule must exist")
        .0;

    for required in [
        ".task-table-scroll {\n    overflow-x: visible;",
        ".task-table {\n    display: block;\n    min-width: 0;",
        ".task-table thead {\n    display: block;",
        ".task-table thead tr,\n.task-row {\n    display: grid;",
        "grid-template-columns: 88px 5.75rem 5.5rem minmax(0, 1fr);",
        "grid-template-areas: \"action schedule deadline task\";",
        ".task-row {\n    min-height: 32px;",
        ".task-row:not(:last-child) {\n    border-bottom: 1px solid var(--line);",
        ".task-table td {\n    display: flex;\n    min-width: 0;\n    align-items: center;\n    padding: 0.125rem 0.35rem;",
        ".task-table .session-cell {\n    padding: 0;",
        ".deadline,\n.schedule-time {\n    font-size: 0.68rem;",
        ".task-name {\n    overflow: hidden;\n    font-size: 0.75rem;",
        ".task-name-scroll {\n    min-width: 0;\n    overflow-x: auto;\n    overflow-y: hidden;\n    overscroll-behavior-inline: contain;\n    white-space: nowrap;",
        "touch-action: pan-x pan-y pinch-zoom;",
        ".session-cell .session-start,\n.session-cell .task-defer {\n    width: 44px;\n    min-height: 32px;",
        ".session-start-compact-label {\n    display: inline;",
        ".task-defer-compact-label {\n    display: inline;",
        ".session-start-full-label,\n.task-defer-full-label {\n    display: none;",
    ] {
        assert!(
            universal_list_layout.contains(required),
            "missing universal rule: {required}"
        );
    }

    for removed_card_style in [
        "gap: 0.75rem;",
        "border-radius: 1rem;",
        "box-shadow: var(--shadow);",
    ] {
        let row_rule = universal_list_layout
            .split_once(".task-row {")
            .expect("universal task row rule must exist")
            .1
            .split_once('}')
            .unwrap()
            .0;
        assert!(!row_rule.contains(removed_card_style), "{row_rule}");
    }

    for selector in [
        ".task-list-view",
        ".list-controls",
        ".date-pills",
        ".date-pill",
        ".task-name-filter-input",
        ".date-jump-input",
        ".task-table",
        ".task-row",
        ".task-name-scroll",
        ".session-start-compact-label",
        ".task-defer-compact-label",
        ".session-start-full-label",
        ".task-defer-full-label",
    ] {
        assert!(
            !mobile_only_layout.contains(selector),
            "list layout must not depend on viewport width: {selector}"
        );
    }
}

#[test]
fn 一覧操作領域は全幅で36pxに統一する() {
    let css = include_str!("../../assets/main.css");
    let (universal_layout, responsive_layout) = css
        .split_once("@media (max-width: 46rem)")
        .expect("navigation breakpoint must exist");
    let narrow_layout = responsive_layout
        .split_once("@media (max-width: 34rem)")
        .expect("narrow viewport rule must exist")
        .1;

    for required in [
        ".task-list-view {\n    display: grid;\n    gap: 0.5rem;",
        ".date-pills {\n    display: flex;\n    gap: 0.5rem;\n    overflow-x: auto;\n    padding: 0.125rem 0.1rem 0.25rem;",
        ".date-pill {\n    flex: 0 0 auto;\n    height: 36px;\n    border-radius: 999px;\n    padding: 0.35rem 0.9rem;",
        ".task-name-filter-input {\n    width: 100%;\n    min-width: 0;\n    height: 36px;\n    min-height: 36px;\n    padding: 0.4rem 0.7rem;",
        ".task-name-filter-clear {\n    flex: 0 0 auto;\n    width: 36px;\n    height: 36px;\n    min-width: 36px;\n    min-height: 36px;\n    padding: 0.25rem;",
    ] {
        assert!(universal_layout.contains(required), "missing: {required}");
    }

    assert!(
        !narrow_layout.contains(".date-pill {"),
        "date pill sizing must be owned by the universal layout"
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

    assert_eq!(
        html.matches("セッション追加済み\" disabled=true").count(),
        2,
        "{html}"
    );
    assert_eq!(html.matches(">✓</span>").count(), 2, "{html}");
    assert_eq!(html.matches(">＋</span>").count(), 1, "{html}");
    assert_eq!(html.matches("セッション追加済み").count(), 2, "{html}");
    assert_eq!(
        html.matches(": 先送り\" disabled=true").count(),
        2,
        "{html}"
    );
}

#[test]
fn misses_deadlineがfalseなら超過classを付けない() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (dom, _) = build(RootProps {
        dates: Vec::new(),
        rows: vec![row("equal", false, false)],
        active_task_ids: Vec::new(),
        filter_text: String::new(),
        events,
    });
    assert!(!dioxus::ssr::render(&dom).contains("deadline-kind-overrun"));
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
    dispatch_click(&date_dom, date_listeners[1]);
    assert_eq!(*events.lock().unwrap(), ["date:2026-09-12"]);

    events.lock().unwrap().clear();
    let (task_dom, task_listeners) = build(RootProps {
        dates: Vec::new(),
        rows: vec![row("task-id", false, true)],
        active_task_ids: Vec::new(),
        filter_text: String::new(),
        events: Arc::clone(&events),
    });
    dispatch_click(&task_dom, task_listeners[1]);
    assert_eq!(*events.lock().unwrap(), ["task:task-id:task task-id:true"]);

    events.lock().unwrap().clear();
    dispatch_click(&task_dom, task_listeners[2]);
    assert_eq!(*events.lock().unwrap(), ["defer:task-id"]);
}

#[test]
fn 期限余裕不足の先送りは確認後だけdispatchしキャンセルできる() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (mut cancel_dom, initial_ids) = build(RootProps {
        dates: Vec::new(),
        rows: vec![confirmation_row(
            "due-today",
            DeferConfirmationKind::DeadlineLimited,
        )],
        active_task_ids: Vec::new(),
        filter_text: String::new(),
        events: Arc::clone(&events),
    });
    dispatch_click(&cancel_dom, initial_ids[2]);
    let confirmation_ids = render_with_click_listeners(&mut cancel_dom);
    let html = dioxus::ssr::render(&cancel_dom);
    assert!(events.lock().unwrap().is_empty());
    assert!(html.contains("締切までの余裕がありません"), "{html}");
    assert!(html.contains("9/12 05:59"), "{html}");
    assert!(html.contains("tabindex=\"-1\""), "{html}");
    assert!(html.contains("aria-live=\"assertive\""), "{html}");
    assert!(html.contains("キャンセル"), "{html}");
    assert!(html.contains("先送りする"), "{html}");
    dispatch_click(&cancel_dom, confirmation_ids[0]);
    cancel_dom.render_immediate_to_vec();
    assert!(events.lock().unwrap().is_empty());
    assert!(!dioxus::ssr::render(&cancel_dom).contains("締切までの余裕がありません"));

    let (mut confirm_dom, initial_ids) = build(RootProps {
        dates: Vec::new(),
        rows: vec![confirmation_row(
            "routine",
            DeferConfirmationKind::RoutinePeriod,
        )],
        active_task_ids: Vec::new(),
        filter_text: String::new(),
        events: Arc::clone(&events),
    });
    dispatch_click(&confirm_dom, initial_ids[2]);
    let confirmation_ids = render_with_click_listeners(&mut confirm_dom);
    let html = dioxus::ssr::render(&confirm_dom);
    assert!(html.contains("次の周期へ送ります"), "{html}");
    dispatch_click(&confirm_dom, confirmation_ids[1]);
    assert_eq!(*events.lock().unwrap(), ["defer:routine"]);
}

#[component]
fn ReplacingConfirmationRowsHarness(events: Rc<RefCell<Vec<String>>>) -> Element {
    let mut show_first = use_signal(|| true);
    let rows = if show_first() {
        vec![
            confirmation_row("first", DeferConfirmationKind::DeadlineLimited),
            confirmation_row("second", DeferConfirmationKind::DeadlineLimited),
        ]
    } else {
        vec![confirmation_row(
            "second",
            DeferConfirmationKind::DeadlineLimited,
        )]
    };
    let defer_events = Rc::clone(&events);
    rsx! {
        button {
            r#type: "button",
            aria_label: "先頭rowを除去",
            onclick: move |_| show_first.set(false),
            "先頭rowを除去"
        }
        ListView {
            dates: Vec::new(),
            rows,
            active_task_ids: Vec::new(),
            date_input_text: String::new(),
            date_input_error: None,
            filter_text: String::new(),
            on_select_date: move |_| {},
            on_date_input_change: move |_| {},
            on_submit_date_input: move |_| {},
            on_start_session: move |_| {},
            on_defer_task: move |(task_id, _): (String, crate::DeferPlan)| defer_events.borrow_mut().push(task_id),
            on_filter_change: move |_| {},
        }
    }
}

#[test]
fn row差替えで先送り確認stateを別taskへ継承しない() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut dom = VirtualDom::new_with_props(
        ReplacingConfirmationRowsHarness,
        ReplacingConfirmationRowsHarnessProps {
            events: Rc::clone(&events),
        },
    );
    let initial_ids = rebuild_with_click_listeners(&mut dom);
    dispatch_click(&dom, initial_ids[3]);
    dom.render_immediate_to_vec();
    assert!(dioxus::ssr::render(&dom).contains("task firstは締切までの余裕がありません"));

    dispatch_click(&dom, initial_ids[0]);
    dom.render_immediate_to_vec();
    let html = dioxus::ssr::render(&dom);
    assert!(!html.contains("先送りしますか?"), "{html}");
    assert!(html.contains("task second"), "{html}");
    assert!(events.borrow().is_empty());
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

    assert_eq!(listeners.len(), 1, "全てbuttonだけがclick listenerを持つ");
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

    assert_eq!(html.matches("週次 Planning").count(), 6, "{html}");
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
    let controls_position = html.find("class=\"list-controls\"").unwrap();
    let date_input_position = html.find("class=\"date-jump-form\"").unwrap();
    let filter_position = html.find("class=\"task-name-filter\"").unwrap();
    let table_position = html.find("class=\"task-table-scroll\"").unwrap();

    assert!(
        dates_position < controls_position
            && controls_position < date_input_position
            && date_input_position < filter_position
            && filter_position < table_position
    );
    assert!(html.contains("aria-label=\"表示する日付\""), "{html}");
    assert!(html.contains("placeholder=\"例: 6/18\""), "{html}");
    assert!(html.contains(">表示</button>"), "{html}");
    assert!(html.contains("aria-label=\"タスク名を検索\""), "{html}");
    assert!(html.contains("placeholder=\"タスク名を検索\""), "{html}");
    assert_eq!(input_listeners.len(), 2);

    dispatch_platform_event(
        &dom,
        "input",
        input_listeners[1],
        Box::new(SerializedFormData::new("設計".to_owned(), Vec::new())),
    );
    assert_eq!(*events.lock().unwrap(), ["filter:設計"]);
}

#[test]
fn 日付入力とform_submitは別々のeventを通知する() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut dom = VirtualDom::new_with_props(
        root,
        RootProps {
            dates: Vec::new(),
            rows: Vec::new(),
            active_task_ids: Vec::new(),
            filter_text: String::new(),
            events: Arc::clone(&events),
        },
    );
    let listeners = rebuild_with_named_event_listeners(&mut dom);
    let date_input_id = listeners
        .iter()
        .find_map(|(name, id)| (name == "input").then_some(*id))
        .unwrap();
    let submit_id = listeners
        .iter()
        .find_map(|(name, id)| (name == "submit").then_some(*id))
        .unwrap();

    dispatch_platform_event(
        &dom,
        "input",
        date_input_id,
        Box::new(SerializedFormData::new("9/16".to_owned(), Vec::new())),
    );
    dispatch_platform_event(
        &dom,
        "submit",
        submit_id,
        Box::new(SerializedFormData::new(String::new(), Vec::new())),
    );

    assert_eq!(*events.lock().unwrap(), ["date-input:9/16", "date-submit"]);
}

#[test]
fn 日付入力errorはfieldと関連付けて表示する() {
    let mut dom = VirtualDom::new(|| {
        rsx! {
            ListView {
                dates: Vec::new(),
                rows: Vec::new(),
                active_task_ids: Vec::new(),
                date_input_text: "bad".to_owned(),
                date_input_error: Some("M/DまたはYYYY/M/D形式で入力してください。".to_owned()),
                filter_text: String::new(),
                on_select_date: move |_| {},
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_start_session: move |_| {},
                on_filter_change: move |_| {},
            }
        }
    });
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);

    assert!(html.contains("aria-invalid=true"), "{html}");
    assert!(
        html.contains("aria-describedby=\"date-input-error\""),
        "{html}"
    );
    assert!(html.contains("id=\"date-input-error\""), "{html}");
    assert!(
        html.contains("class=\"date-input-error\" role=\"alert\""),
        "{html}"
    );
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
    assert_eq!(listeners.len(), 2);
    dispatch_click(&dom, listeners[1]);
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
    assert_eq!(
        listeners.len(),
        2,
        "all button and clear button remain clickable"
    );
}

#[test]
fn task_name_filterは全幅でcompactなclear操作を使う() {
    let css = include_str!("../../assets/main.css");
    let universal_layout = css
        .split_once("@media (max-width: 46rem)")
        .expect("navigation breakpoint must exist")
        .0;

    for required in [
        ".task-name-filter {",
        "width: 100%;",
        "min-width: 0;",
        ".task-name-filter-clear {",
        "width: 36px;",
        "height: 36px;",
        "min-width: 36px;",
        "min-height: 36px;",
    ] {
        assert!(universal_layout.contains(required), "missing: {required}");
    }
}

#[test]
fn 日付入力は全幅で検索の上に並ぶ() {
    let css = include_str!("../../assets/main.css");
    let (universal_layout, responsive_layout) = css
        .split_once("@media (max-width: 46rem)")
        .expect("navigation breakpoint must exist");
    let mobile_only_layout = responsive_layout
        .split_once("@media (max-width: 34rem)")
        .expect("narrow viewport rule must exist")
        .0;

    for required in [
        ".list-controls {\n    display: grid;",
        "grid-template-columns: minmax(0, 1fr);",
        ".date-jump-controls {\n    display: grid;",
        "grid-template-columns: minmax(0, 1fr) auto;",
        ".date-jump-input {\n    width: 100%;",
        ".date-jump-input,\n.date-jump-submit {\n    height: 36px;",
        "min-height: 36px;",
    ] {
        assert!(universal_layout.contains(required), "missing: {required}");
    }

    assert!(!mobile_only_layout.contains(".list-controls"));
    assert!(!mobile_only_layout.contains(".date-jump-input"));
}

#[component]
fn StatefulDateInputHarness(events: Rc<RefCell<Vec<String>>>) -> Element {
    let mut show_list = use_signal(|| true);
    let mut date_input = use_signal(DateInputState::default);
    let input_text = date_input.read().text().to_owned();
    let input_error = date_input.read().error().map(|error| error.to_string());
    let button_events = Rc::clone(&events);
    let submit_events = Rc::clone(&events);

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
                dates: vec![DateButtonViewModel {
                    logical_date: "2026-09-17".to_owned(),
                    label: "木".to_owned(),
                    selected: false,
                }],
                rows: Vec::new(),
                active_task_ids: Vec::new(),
                date_input_text: input_text,
                date_input_error: input_error,
                filter_text: String::new(),
                on_select_date: move |date| {
                    let action = component_action_from_date_button(&mut date_input.write(), date);
                    if let ComponentAction::SelectDate(date) = action {
                        button_events.borrow_mut().push(format!("server:{date}"));
                    }
                },
                on_date_input_change: move |text| date_input.write().edit(text),
                on_submit_date_input: move |_| {
                    if let Some(ComponentAction::SelectDate(date)) =
                        component_action_from_date_input(&mut date_input.write(), "2026-09-16")
                    {
                        submit_events.borrow_mut().push(format!("server:{date}"));
                    }
                },
                on_start_session: move |_| {},
                on_filter_change: move |_| {},
            }
        }
    }
}

#[test]
fn 日付入力は正規化後もtab往復で保持され日付buttonでclearされる() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut dom = VirtualDom::new_with_props(
        StatefulDateInputHarness,
        StatefulDateInputHarnessProps {
            events: Rc::clone(&events),
        },
    );
    let listeners = rebuild_with_named_event_listeners(&mut dom);
    let toggle_id = listeners
        .iter()
        .find_map(|(name, id)| (name == "click").then_some(*id))
        .unwrap();
    let date_input_id = listeners
        .iter()
        .find_map(|(name, id)| (name == "input").then_some(*id))
        .unwrap();
    let submit_id = listeners
        .iter()
        .find_map(|(name, id)| (name == "submit").then_some(*id))
        .unwrap();

    dispatch_platform_event(
        &dom,
        "input",
        date_input_id,
        Box::new(SerializedFormData::new("9/16".to_owned(), Vec::new())),
    );
    dispatch_platform_event(
        &dom,
        "submit",
        submit_id,
        Box::new(SerializedFormData::new(String::new(), Vec::new())),
    );
    dom.render_immediate_to_vec();
    let submitted_html = dioxus::ssr::render(&dom);
    assert!(
        submitted_html.contains("value=\"2026/9/16\""),
        "{submitted_html}"
    );
    assert_eq!(*events.borrow(), ["server:2026-09-16"]);

    dispatch_click(&dom, toggle_id);
    dom.render_immediate_to_vec();
    dispatch_click(&dom, toggle_id);
    let restored_click_ids = render_with_click_listeners(&mut dom);
    let restored_html = dioxus::ssr::render(&dom);
    assert!(
        restored_html.contains("value=\"2026/9/16\""),
        "{restored_html}"
    );

    assert_eq!(restored_click_ids.len(), 2);
    dispatch_click(&dom, *restored_click_ids.last().unwrap());
    dom.render_immediate_to_vec();
    let cleared_html = dioxus::ssr::render(&dom);
    assert!(cleared_html.contains("class=\"date-jump-input\" type=\"text\" value=\"\""));
    assert_eq!(*events.borrow(), ["server:2026-09-16", "server:2026-09-17"]);
}

#[component]
fn BackgroundBlockedDateHarness(events: Rc<RefCell<Vec<String>>>) -> Element {
    let date_events = Rc::clone(&events);
    let defer_events = Rc::clone(&events);
    rsx! {
        ListView {
            dates: vec![DateButtonViewModel {
                logical_date: "2026-09-17".to_owned(),
                label: "木".to_owned(),
                selected: false,
            }],
            rows: vec![row("defer", false, true)],
            active_task_ids: Vec::new(),
            date_input_text: "9/17".to_owned(),
            date_input_error: None,
            filter_text: String::new(),
            server_actions_blocked: true,
            on_select_date: move |_| date_events.borrow_mut().push("date".to_owned()),
            on_date_input_change: move |_| {},
            on_submit_date_input: move |_| events.borrow_mut().push("submit".to_owned()),
            on_start_session: move |_| {},
            on_defer_task: move |_| defer_events.borrow_mut().push("defer".to_owned()),
            on_filter_change: move |_| {},
        }
    }
}

#[test]
fn background更新中は日付buttonとenter送信をuiで拒否する() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut dom = VirtualDom::new_with_props(
        BackgroundBlockedDateHarness,
        BackgroundBlockedDateHarnessProps {
            events: Rc::clone(&events),
        },
    );
    let listeners = rebuild_with_named_event_listeners(&mut dom);
    let html = dioxus::ssr::render(&dom);
    assert_eq!(html.matches("disabled").count(), 3, "{html}");

    let submit_id = listeners
        .iter()
        .find_map(|(name, id)| (name == "submit").then_some(*id))
        .unwrap();
    dispatch_platform_event(
        &dom,
        "submit",
        submit_id,
        Box::new(SerializedFormData::new(String::new(), Vec::new())),
    );
    assert!(events.borrow().is_empty());
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
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: filter_text.read().clone(),
                on_select_date: move |_| date_events.borrow_mut().push("server:date".to_owned()),
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
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
        .rev()
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
    assert_eq!(restored_clear_ids.len(), 2);
    dispatch_click(&dom, *restored_clear_ids.last().unwrap());
    render_with_click_listeners(&mut dom);
    let cleared_html = dioxus::ssr::render(&dom);
    assert!(cleared_html.contains("画面設計"), "{cleared_html}");
    assert!(cleared_html.contains(">実装<"), "{cleared_html}");
    assert!(!cleared_html.contains("検索文字列をクリア"));
    assert!(events.borrow().is_empty());
}
