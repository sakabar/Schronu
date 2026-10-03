#![cfg(feature = "server")]

use super::list_view::{DateButtonViewModel, ListView};
use crate::client::state::ListMode;
use crate::CompletedTaskRow;
use dioxus::prelude::*;

fn completed_root() -> Element {
    rsx! {
        ListView {
            dates: vec![DateButtonViewModel {
                logical_date: "2026-09-16".to_owned(),
                label: "水 今日".to_owned(),
                selected: true,
            }],
            rows: Vec::new(),
            completed_rows: vec![
                CompletedTaskRow {
                    task_id: "00000000-0000-4000-8000-000000000001".to_owned(),
                    task_name: "設計を仕上げる".to_owned(),
                    project_name: "Schronu".to_owned(),
                    completed_at_epoch_ms: 1_789_551_723_000,
                    actual_work_seconds: 360_001,
                    estimated_work_seconds: 359_999,
                },
                CompletedTaskRow {
                    task_id: "00000000-0000-4000-8000-000000000002".to_owned(),
                    task_name: "差分なし".to_owned(),
                    project_name: "個人".to_owned(),
                    completed_at_epoch_ms: 1_789_551_724_000,
                    actual_work_seconds: 0,
                    estimated_work_seconds: 0,
                },
            ],
            list_mode: ListMode::Completed,
            selected_logical_date: Some("2026-09-16".to_owned()),
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

#[test]
fn 完了modeはread_only表と件数と全情報を表示する() {
    let mut dom = VirtualDom::new(completed_root);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);

    for text in [
        "予定",
        "完了",
        "9月16日の完了",
        "2件",
        "実績",
        "見積",
        "差",
        "Project",
        "タスク",
        "100:00:01",
        "99:59:59",
        "+00:00:02",
        "+00:00:00",
        "Schronu",
        "設計を仕上げる",
    ] {
        assert!(html.contains(text), "missing {text}: {html}");
    }
    assert!(!html.contains(">全て<"), "{html}");
    assert!(!html.contains("session-start"), "{html}");
    assert!(!html.contains("task-defer"), "{html}");
    assert!(html.contains("completed-task-table-scroll"), "{html}");
    assert!(html.contains("<time"), "{html}");
}

#[test]
fn 完了modeの検索はtask名だけを対象にする() {
    fn filtered_root() -> Element {
        rsx! {
            ListView {
                dates: Vec::new(),
                rows: Vec::new(),
                completed_rows: vec![CompletedTaskRow {
                    task_id: "00000000-0000-4000-8000-000000000001".to_owned(),
                    task_name: "対象外".to_owned(),
                    project_name: "SCHRONU".to_owned(),
                    completed_at_epoch_ms: 1_789_551_723_000,
                    actual_work_seconds: 1,
                    estimated_work_seconds: 2,
                }],
                list_mode: ListMode::Completed,
                selected_logical_date: Some("2026-09-16".to_owned()),
                active_task_ids: Vec::new(),
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: " schronu ".to_owned(),
                on_select_date: move |_| {},
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_start_session: move |_| {},
                on_filter_change: move |_| {},
            }
        }
    }

    let mut dom = VirtualDom::new(filtered_root);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);
    assert!(html.contains("一致するタスクがありません。"), "{html}");
    assert!(!html.contains(">SCHRONU<"), "{html}");
}
