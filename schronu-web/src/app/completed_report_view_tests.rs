#![cfg(feature = "server")]

use super::list_view::{CompletedListView, DateButtonViewModel, ListModeControl};
use crate::client::state::ListMode;
use crate::{CompletedTaskReport, CompletedTaskRow};
use chrono::{Local, TimeZone};
use dioxus::prelude::*;

fn completed_root() -> Element {
    rsx! {
        div {
            ListModeControl {
                selected_mode: ListMode::Completed,
                on_switch_list_mode: move |_| {},
            }
            CompletedListView {
                dates: vec![DateButtonViewModel {
                    logical_date: "2026-09-16".to_owned(),
                    label: "水 今日".to_owned(),
                    selected: true,
                }],
                report: Some(CompletedTaskReport {
                    rows: vec![
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
                        CompletedTaskRow {
                        task_id: "00000000-0000-4000-8000-000000000003".to_owned(),
                        task_name: "見積内".to_owned(),
                        project_name: "個人".to_owned(),
                        completed_at_epoch_ms: 1_789_551_725_000,
                        actual_work_seconds: 1,
                        estimated_work_seconds: 3,
                        },
                    ],
                    total_actual_work_seconds: 360_001,
                    available_seconds: 288_000,
                    recorded_percentage: Some(125),
                }),
                selected_logical_date: Some("2026-09-16".to_owned()),
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: String::new(),
                on_select_date: move |_| {},
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_filter_change: move |_| {},
            }
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
        "3件",
        "実績",
        "差",
        "Project",
        "タスク",
        "100:00:01",
        "+00:00:02",
        "+00:00:00",
        "-00:00:02",
        "Schronu",
        "設計を仕上げる",
    ] {
        assert!(html.contains(text), "missing {text}: {html}");
    }
    assert!(!html.contains(">全て<"), "{html}");
    assert!(!html.contains("session-start"), "{html}");
    assert!(!html.contains("task-defer"), "{html}");
    assert!(!html.contains("<th scope=\"col\">見積</th>"), "{html}");
    assert!(!html.contains(">99:59:59</td>"), "{html}");
    assert!(html.contains("completed-task-table-scroll"), "{html}");
    assert!(html.contains("<time"), "{html}");
    let expected_local_time = Local
        .timestamp_millis_opt(1_789_551_723_000)
        .single()
        .expect("valid timestamp")
        .format("%H:%M")
        .to_string();
    assert!(html.contains(&expected_local_time), "{html}");
    let seconds_precision = Local
        .timestamp_millis_opt(1_789_551_723_000)
        .single()
        .expect("valid timestamp")
        .format("%H:%M:%S")
        .to_string();
    assert!(
        !html.contains(&format!(">{seconds_precision}</time>")),
        "{html}"
    );
}

#[test]
fn 完了modeの表現不能な完了時刻は分精度placeholderへ退避する() {
    fn invalid_time_root() -> Element {
        rsx! {
            CompletedListView {
                dates: Vec::new(),
                report: Some(CompletedTaskReport {
                    rows: vec![CompletedTaskRow {
                        task_id: "00000000-0000-4000-8000-000000000001".to_owned(),
                        task_name: "範囲外".to_owned(),
                        project_name: "Schronu".to_owned(),
                        completed_at_epoch_ms: i64::MAX,
                        actual_work_seconds: 1,
                        estimated_work_seconds: 1,
                    }],
                    total_actual_work_seconds: 1,
                    available_seconds: 1,
                    recorded_percentage: Some(100),
                }),
                selected_logical_date: Some("2026-09-16".to_owned()),
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: String::new(),
                on_select_date: move |_| {},
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_filter_change: move |_| {},
            }
        }
    }

    let mut dom = VirtualDom::new(invalid_time_root);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);
    assert!(html.contains("aria-label=\"完了時刻 --:--\""), "{html}");
    assert!(html.contains(">--:--</time>"), "{html}");
    assert!(!html.contains("--:--:--"), "{html}");
}

#[test]
fn 完了modeは検索に依存しないsemanticな日次集計を表示する() {
    fn filtered_root() -> Element {
        rsx! {
            CompletedListView {
                dates: Vec::new(),
                report: Some(CompletedTaskReport {
                    rows: vec![CompletedTaskRow {
                        task_id: "00000000-0000-4000-8000-000000000001".to_owned(),
                        task_name: "検索対象外".to_owned(),
                        project_name: "Schronu".to_owned(),
                        completed_at_epoch_ms: 1_789_551_723_000,
                        actual_work_seconds: 360_001,
                        estimated_work_seconds: 360_001,
                    }],
                    total_actual_work_seconds: 360_001,
                    available_seconds: 288_000,
                    recorded_percentage: Some(125),
                }),
                selected_logical_date: Some("2026-09-16".to_owned()),
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: "不一致".to_owned(),
                on_select_date: move |_| {},
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_filter_change: move |_| {},
            }
        }
    }

    let mut dom = VirtualDom::new(filtered_root);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);

    assert!(html.contains("completed-report-summary"), "{html}");
    assert!(html.contains("<dl"), "{html}");
    for text in [
        "完了日の集計",
        "実績合計",
        "100:00:01",
        "利用可能",
        "80:00:00",
        "記録率",
        "125%",
        "0件",
        "一致するタスクがありません。",
    ] {
        assert!(html.contains(text), "missing {text}: {html}");
    }
}

#[test]
fn 完了modeの検索はtask名だけを対象にする() {
    fn filtered_root() -> Element {
        rsx! {
            CompletedListView {
                dates: Vec::new(),
                report: Some(CompletedTaskReport {
                    rows: vec![CompletedTaskRow {
                        task_id: "00000000-0000-4000-8000-000000000001".to_owned(),
                        task_name: "対象外".to_owned(),
                        project_name: "SCHRONU".to_owned(),
                        completed_at_epoch_ms: 1_789_551_723_000,
                        actual_work_seconds: 1,
                        estimated_work_seconds: 2,
                    }],
                    total_actual_work_seconds: 1,
                    available_seconds: 2,
                    recorded_percentage: Some(50),
                }),
                selected_logical_date: Some("2026-09-16".to_owned()),
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: " schronu ".to_owned(),
                on_select_date: move |_| {},
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_filter_change: move |_| {},
            }
        }
    }

    let mut dom = VirtualDom::new(filtered_root);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);
    assert!(html.contains("一致するタスクがありません。"), "{html}");
    assert!(html.contains("9月16日の完了"), "{html}");
    assert!(html.contains("0件"), "{html}");
    assert!(!html.contains(">SCHRONU<"), "{html}");
}

#[test]
fn 完了modeの空結果を明示する() {
    fn empty_root() -> Element {
        rsx! {
            CompletedListView {
                dates: Vec::new(),
                report: Some(CompletedTaskReport {
                    rows: Vec::new(),
                    total_actual_work_seconds: 0,
                    available_seconds: 28_800,
                    recorded_percentage: Some(0),
                }),
                selected_logical_date: Some("2026-09-16".to_owned()),
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: String::new(),
                on_select_date: move |_| {},
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_filter_change: move |_| {},
            }
        }
    }

    let mut dom = VirtualDom::new(empty_root);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);
    assert!(html.contains("0件"), "{html}");
    assert!(
        html.contains("この日に完了したタスクはありません。"),
        "{html}"
    );
    for text in [
        "実績合計",
        "00:00:00",
        "利用可能",
        "08:00:00",
        "記録率",
        "0%",
    ] {
        assert!(html.contains(text), "missing {text}: {html}");
    }
}

#[test]
fn 完了modeは利用可能時間ゼロの記録率を未定義として表示する() {
    fn unavailable_root() -> Element {
        rsx! {
            CompletedListView {
                dates: Vec::new(),
                report: Some(CompletedTaskReport {
                    rows: Vec::new(),
                    total_actual_work_seconds: 0,
                    available_seconds: 0,
                    recorded_percentage: None,
                }),
                selected_logical_date: Some("2026-09-16".to_owned()),
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: String::new(),
                on_select_date: move |_| {},
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_filter_change: move |_| {},
            }
        }
    }

    let mut dom = VirtualDom::new(unavailable_root);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);
    assert!(html.contains("記録率"), "{html}");
    assert!(html.contains(">--<"), "{html}");
}

#[test]
fn 完了report未取得時は集計を表示しない() {
    fn unloaded_root() -> Element {
        rsx! {
            CompletedListView {
                dates: Vec::new(),
                report: None,
                selected_logical_date: Some("2026-09-16".to_owned()),
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: String::new(),
                on_select_date: move |_| {},
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_filter_change: move |_| {},
            }
        }
    }

    let mut dom = VirtualDom::new(unloaded_root);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);
    assert!(!html.contains("completed-report-summary"), "{html}");
    assert!(
        html.contains("この日に完了したタスクはありません。"),
        "{html}"
    );
}

#[test]
fn 完了reportの日次集計契約をdocumentationへ明記する() {
    let readme = include_str!("../../../README.md");
    let requirements = include_str!("../../../docs/design/schronu_web_ui_requirements.md");
    let specification = include_str!("../../../docs/design/schronu_web_ui_specification.md");

    for document in [readme, requirements, specification] {
        for text in ["実績合計", "利用可能", "記録率"] {
            assert!(document.contains(text), "missing {text}");
        }
        assert!(!document.contains("日次合計は表示しない"));
    }
    assert!(readme.contains("実績が0秒の場合は見積時間"));
}
