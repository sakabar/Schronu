use super::list_view::DateButtonViewModel;
use super::summary_view::SummaryView;
use super::view_test_support::{dispatch_click, rebuild_with_click_listeners};
use crate::client::state::DiscardedSessionsViewState;
use crate::{DiscardedSessionDay, DiscardedSessionEvent, DiscardedSessionTaskTotal};
use dioxus::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct RootProps {
    state: DiscardedSessionsViewState,
    server_actions_blocked: bool,
    selected: Arc<Mutex<Vec<String>>>,
    retried: Arc<Mutex<Vec<String>>>,
}

fn root(props: RootProps) -> Element {
    rsx! {
        SummaryView {
            dates: dates(),
            state: props.state,
            server_actions_blocked: props.server_actions_blocked,
            on_select_date: move |date| props.selected.lock().unwrap().push(date),
            on_retry: move |date| props.retried.lock().unwrap().push(date),
        }
    }
}

fn render(state: DiscardedSessionsViewState) -> String {
    let mut dom = VirtualDom::new_with_props(
        root,
        RootProps {
            state,
            server_actions_blocked: false,
            selected: Arc::new(Mutex::new(Vec::new())),
            retried: Arc::new(Mutex::new(Vec::new())),
        },
    );
    dom.rebuild_in_place();
    dioxus::ssr::render(&dom)
}

#[test]
fn loadedは日次合計task別合計event時刻と全日本語理由を表示する() {
    let html = render(DiscardedSessionsViewState::Loaded(
        summary_with_all_reasons(),
    ));

    for expected in [
        "破棄時間 01:02:03",
        "タスク別",
        "開始時の名前 01:02:03",
        "計測を破棄して解除",
        "計測を破棄して完了",
        "CLIでフォーカス解除",
        "CLIでしまう",
        "CLIでフォーカス切替",
        "CLIで自動切替",
        "CLIを正常終了",
        "00:00:01",
    ] {
        assert!(html.contains(expected), "missing {expected}: {html}");
    }
    assert!(html.contains("aria-label=\"破棄セッション集計\""));
    assert!(html.contains("aria-label=\"タスク別破棄時間\""));
    assert!(html.contains("aria-label=\"破棄セッション内訳\""));
    assert!(html.contains("aria-label=\"開始時の名前の開始・終了時刻\""));
    assert!(html.contains("<time datetime=\"1970-01-01T00:00:00+00:00\""));
}

#[test]
fn idle_empty_loading_errorを混同せずerrorだけ再試行できる() {
    let idle = render(DiscardedSessionsViewState::Idle);
    assert!(idle.contains("集計する日付を選択してください。"));
    assert!(!idle.contains("読み込んでいます"));

    let empty = render(DiscardedSessionsViewState::Loaded(DiscardedSessionDay {
        logical_date: "2026-09-10".to_owned(),
        total_seconds: 0,
        task_totals: Vec::new(),
        events: Vec::new(),
    }));
    assert!(empty.contains("破棄時間 00:00:00"));
    assert!(empty.contains("この日の破棄セッションはありません。"));

    let loading = render(DiscardedSessionsViewState::Loading {
        logical_date: "2026-09-11".to_owned(),
    });
    assert!(loading.contains("2026-09-11の破棄時間を読み込んでいます…"));
    assert!(!loading.contains("2026-09-10"));
    assert!(!loading.contains("再試行"));

    let selected = Arc::new(Mutex::new(Vec::new()));
    let retried = Arc::new(Mutex::new(Vec::new()));
    let mut dom = VirtualDom::new_with_props(
        root,
        RootProps {
            state: DiscardedSessionsViewState::Error {
                logical_date: "2026-09-11".to_owned(),
                message: "集計を取得できませんでした。".to_owned(),
            },
            server_actions_blocked: false,
            selected,
            retried: Arc::clone(&retried),
        },
    );
    let listeners = rebuild_with_click_listeners(&mut dom);
    let html = dioxus::ssr::render(&dom);
    assert!(html.contains("role=\"alert\""));
    assert!(html.contains("集計を取得できませんでした。"));
    assert!(!html.contains("読み込んでいます"));
    for listener in listeners {
        dispatch_click(&dom, listener);
    }
    assert_eq!(*retried.lock().unwrap(), ["2026-09-11"]);
}

#[test]
fn 日付controlとmobile_cssは一覧と同じ横scroll契約を使う() {
    let selected = Arc::new(Mutex::new(Vec::new()));
    let mut dom = VirtualDom::new_with_props(
        root,
        RootProps {
            state: DiscardedSessionsViewState::Idle,
            server_actions_blocked: false,
            selected: Arc::clone(&selected),
            retried: Arc::new(Mutex::new(Vec::new())),
        },
    );
    let listeners = rebuild_with_click_listeners(&mut dom);
    let html = dioxus::ssr::render(&dom);
    assert!(html.contains("class=\"date-pills\""));
    assert!(html.contains("aria-pressed=true"));
    for listener in listeners {
        dispatch_click(&dom, listener);
    }
    assert!(selected
        .lock()
        .unwrap()
        .iter()
        .any(|date| date == "2026-09-10"));

    let css = include_str!("../../assets/main.css");
    assert!(css.contains(".date-pills {"));
    assert!(css.contains("overflow-x: auto"));
    assert!(css.contains("@media (max-width: 46rem)"));
    assert!(css.contains(".discard-summary-error button"));
    assert!(css.contains("min-height: max(2.75rem, 44px)"));
}

#[test]
fn 通信可能時だけ日付選択と再試行をdispatchできる() {
    for (server_actions_blocked, expected_calls) in [(false, 1), (true, 0)] {
        let selected = Arc::new(Mutex::new(Vec::new()));
        let retried = Arc::new(Mutex::new(Vec::new()));
        let mut dom = VirtualDom::new_with_props(
            root,
            RootProps {
                state: DiscardedSessionsViewState::Error {
                    logical_date: "2026-09-11".to_owned(),
                    message: "集計を取得できませんでした。".to_owned(),
                },
                server_actions_blocked,
                selected: Arc::clone(&selected),
                retried: Arc::clone(&retried),
            },
        );
        let listeners = rebuild_with_click_listeners(&mut dom);
        let html = dioxus::ssr::render(&dom);
        assert_eq!(
            html.matches(" disabled").count(),
            if server_actions_blocked { 3 } else { 0 }
        );
        for listener in listeners {
            dispatch_click(&dom, listener);
        }
        assert_eq!(selected.lock().unwrap().len(), expected_calls * 2);
        assert_eq!(retried.lock().unwrap().len(), expected_calls);
    }
}

fn dates() -> Vec<DateButtonViewModel> {
    vec![
        DateButtonViewModel {
            logical_date: "2026-09-10".to_owned(),
            label: "木 今日".to_owned(),
            selected: true,
        },
        DateButtonViewModel {
            logical_date: "2026-09-11".to_owned(),
            label: "金 明日".to_owned(),
            selected: false,
        },
    ]
}

fn summary_with_all_reasons() -> DiscardedSessionDay {
    let reasons = [
        "web_discard_release",
        "web_discard_complete",
        "cli_unfocus",
        "cli_tuck_away",
        "cli_focus_switch",
        "cli_auto_switch",
        "cli_normal_exit",
    ];
    DiscardedSessionDay {
        logical_date: "2026-09-10".to_owned(),
        total_seconds: 3_723,
        task_totals: vec![DiscardedSessionTaskTotal {
            task_id: "task".to_owned(),
            task_name: "開始時の名前".to_owned(),
            total_seconds: 3_723,
        }],
        events: reasons
            .into_iter()
            .enumerate()
            .map(|(index, reason)| DiscardedSessionEvent {
                event_id: format!("event-{index}"),
                task_id: "task".to_owned(),
                task_name_at_start: "開始時の名前".to_owned(),
                started_at_epoch_ms: (index as i64) * 2_000,
                ended_at_epoch_ms: (index as i64) * 2_000 + 1_000,
                elapsed_seconds: 1,
                source: if reason.starts_with("web_") {
                    "web"
                } else {
                    "cli"
                }
                .to_owned(),
                reason: reason.to_owned(),
            })
            .collect(),
    }
}
