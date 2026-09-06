#![cfg(feature = "server")]

use super::history_view::{format_invocation, HistoryEntryViewModel, HistoryView};
use crate::client::state::ServerActionInvocation;
use crate::{CompleteSessionRequest, ListTasksRequest, RecordSessionRequest};
use dioxus::prelude::*;

#[derive(Clone)]
struct RootProps {
    entries: Vec<HistoryEntryViewModel>,
}

fn root(props: RootProps) -> Element {
    rsx! { HistoryView { entries: props.entries } }
}

fn render(entries: Vec<HistoryEntryViewModel>) -> String {
    let mut dom = VirtualDom::new_with_props(root, RootProps { entries });
    dom.rebuild_in_place();
    dioxus::ssr::render(&dom)
}

#[test]
fn history_panel_is_closed_by_default_and_omits_locality() {
    let html = render(vec![
        HistoryEntryViewModel {
            occurred_at_hh_mm_ss: "11:25:03".to_owned(),
            invocation: "record_session(task_id: \"123e4567-e89b-12d3-a456-426614174000\", started_at_epoch_ms: 1000, expected_actual_work_seconds: 2000)".to_owned(),
            outcome: "success".to_owned(),
            summary: "実績を記録しました。".to_owned(),
            failed: false,
        },
        HistoryEntryViewModel {
            occurred_at_hh_mm_ss: "11:26:10".to_owned(),
            invocation: "complete_session(task_id: \"task<&>\", started_at_epoch_ms: 3000, expected_actual_work_seconds: 4000, record_elapsed_seconds: false)".to_owned(),
            outcome: "failure".to_owned(),
            summary: "セッションを保持しました。".to_owned(),
            failed: true,
        },
    ]);

    assert!(html.contains("<details class=\"history-panel\">"), "{html}");
    assert!(!html.contains("<details class=\"history-panel\" open"));
    assert!(html.contains("<summary>発火履歴</summary>"));
    for text in [
        "11:25:03",
        "record_session(task_id: &quot;123e4567-e89b-12d3-a456-426614174000&quot;, started_at_epoch_ms: 1000, expected_actual_work_seconds: 2000)",
        "success",
        "実績を記録しました。",
        "11:26:10",
        "complete_session(task_id: &quot;task&lt;&amp;&gt;&quot;, started_at_epoch_ms: 3000, expected_actual_work_seconds: 4000, record_elapsed_seconds: false)",
        "failure",
        "セッションを保持しました。",
    ] {
        assert!(html.contains(text), "missing {text}: {html}");
    }
    assert!(!html.contains("history-locality"), "{html}");
    assert!(!html.contains("history-task-id"), "{html}");
    assert!(!html.contains(">server<"), "{html}");
    assert!(!html.contains(">local<"), "{html}");
    assert!(html.contains("history-entry is-failure"));
}

#[test]
fn invocation_formatterは実action名と全引数を関数呼出し形式にする() {
    let task_id = "task<\"&>";
    assert_eq!(format_invocation(&ServerActionInvocation::Bootstrap), "bootstrap()");
    assert_eq!(
        format_invocation(&ServerActionInvocation::ListTasks(ListTasksRequest {
            logical_date: "2026-09-06".to_owned(),
        })),
        "list_tasks(logical_date: \"2026-09-06\")"
    );
    assert_eq!(
        format_invocation(&ServerActionInvocation::AutoSession),
        "auto_session()"
    );
    assert_eq!(
        format_invocation(&ServerActionInvocation::RecordSession(RecordSessionRequest {
            task_id: task_id.to_owned(),
            started_at_epoch_ms: 1_000,
            expected_actual_work_seconds: 2_000,
        })),
        "record_session(task_id: \"task<\\\"&>\", started_at_epoch_ms: 1000, expected_actual_work_seconds: 2000)"
    );
    for (record_elapsed_seconds, expected) in [
        (true, "record_elapsed_seconds: true"),
        (false, "record_elapsed_seconds: false"),
    ] {
        let formatted = format_invocation(&ServerActionInvocation::CompleteSession(
            CompleteSessionRequest {
                task_id: task_id.to_owned(),
                started_at_epoch_ms: 3_000,
                expected_actual_work_seconds: 4_000,
                record_elapsed_seconds,
            },
        ));
        assert!(formatted.starts_with("complete_session(task_id: \"task<\\\"&>\""));
        assert!(formatted.contains(expected));
        assert!(!formatted.contains("complete_session_without_recording"));
    }
}

#[test]
fn empty_history_has_a_clear_message_and_never_invents_cli_commands() {
    let html = render(Vec::new());

    assert!(html.contains("履歴はありません。"));
    for invented_command in ["見 uuid", "働 分", "終", "外"] {
        assert!(!html.contains(invented_command), "{html}");
    }
}
