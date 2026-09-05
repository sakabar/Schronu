#![cfg(feature = "server")]

use super::history_view::{HistoryEntryViewModel, HistoryView};
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
fn history_panel_is_closed_by_default_and_renders_all_display_fields() {
    let html = render(vec![
        HistoryEntryViewModel {
            occurred_at_hh_mm_ss: "11:25:03".to_owned(),
            operation: "record_session".to_owned(),
            task_id: Some("123e4567-e89b-12d3-a456-426614174000".to_owned()),
            locality: "server".to_owned(),
            outcome: "success".to_owned(),
            summary: "実績を記録しました。".to_owned(),
            failed: false,
        },
        HistoryEntryViewModel {
            occurred_at_hh_mm_ss: "11:26:10".to_owned(),
            operation: "discard_session".to_owned(),
            task_id: None,
            locality: "local".to_owned(),
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
        "record_session",
        "123e4567-e89b-12d3-a456-426614174000",
        "server",
        "success",
        "実績を記録しました。",
        "11:26:10",
        "discard_session",
        "local",
        "failure",
        "セッションを保持しました。",
    ] {
        assert!(html.contains(text), "missing {text}: {html}");
    }
    assert!(html.contains("history-entry is-failure"));
}

#[test]
fn empty_history_has_a_clear_message_and_never_invents_cli_commands() {
    let html = render(Vec::new());

    assert!(html.contains("履歴はありません。"));
    for invented_command in ["見 uuid", "働 分", "終", "外"] {
        assert!(!html.contains(invented_command), "{html}");
    }
}
