use super::component::{
    app, BackgroundRefreshStatus, BufferPanel, InteractiveShell, LoadingOverlay, NavigationTabs,
    SessionChrome, UnavailableBufferPanel,
};
#[cfg(feature = "web")]
use super::component_models::{project_active_completed_report, BrowserPageModel};
use super::component_runtime::{
    component_action_from_date_button, component_action_from_date_input,
    component_action_from_session_action, component_actions_from_session_action, initialize_client,
    reduce_component_action_at, reset_task_name_filter_after_session_add, ComponentAction,
    ComponentOrchestrator,
};
use super::effect_dispatcher::ClientResponse;
use super::load_view::LoadView;
use super::session_view::{SessionAction, SessionActionKind};
use super::view_test_support::{dispatch_click, rebuild_with_click_listeners};
use crate::client::date_input::DateInputState;
use crate::client::state::{ActiveTab, ClientEffect, ServerFailure};
use crate::client::view_state::{load_view_state, store_view_state, StoredActiveList, ViewState};
use crate::client::work_sessions::{KeyValueStorage, StorageError};
use crate::{
    web_error_codes, BandDay, BandDurations, CompletedTaskRow, LoadData, RecordSessionResult,
    RetryAdvice, RoutineLoadReport, RoutineLoadRow, ScheduledTaskRow, ServerSnapshot, SessionTask,
    WebError, WebSuccess,
};
#[cfg(feature = "web")]
use crate::CompletedTaskReport;
use dioxus::dioxus_core::{AttributeValue, Mutation};
use dioxus::prelude::VirtualDom;
use dioxus::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct NavigationProps {
    active_tab: ActiveTab,
    events: Arc<Mutex<Vec<ActiveTab>>>,
}

#[test]
fn background更新はshellを塞がずstale状態と再試行を表示する() {
    fn refreshing() -> Element {
        rsx! {
            InteractiveShell { blocked: false,
                BackgroundRefreshStatus { has_cached_list: true, failed: false }
                button { "操作可能" }
            }
        }
    }
    let mut refreshing_dom = VirtualDom::new(refreshing);
    refreshing_dom.rebuild_in_place();
    let refreshing_html = dioxus::ssr::render(&refreshing_dom);
    assert!(refreshing_html.contains("前回の表示です。最新状態を確認中…"));
    assert!(!refreshing_html.contains(" inert"));
    assert!(!refreshing_html.contains("loading-overlay"));

    fn failed() -> Element {
        rsx! {
            BackgroundRefreshStatus {
                has_cached_list: true,
                failed: true,
                on_retry: move |_| {},
            }
        }
    }
    let mut failed_dom = VirtualDom::new(failed);
    failed_dom.rebuild_in_place();
    let failed_html = dioxus::ssr::render(&failed_dom);
    assert!(failed_html.contains("前回の表示です。最新状態を確認できませんでした。"));
    assert!(failed_html.contains(">再試行<"));
}

fn navigation_root(props: NavigationProps) -> dioxus::prelude::Element {
    rsx! {
        NavigationTabs {
            active_tab: props.active_tab,
            on_switch: move |tab| props.events.lock().unwrap().push(tab),
        }
    }
}

#[test]
fn 固定navigationは4tabの選択状態とcallbackを提供する() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut dom = VirtualDom::new_with_props(
        navigation_root,
        NavigationProps {
            active_tab: ActiveTab::History,
            events: Arc::clone(&events),
        },
    );
    let ids = rebuild_with_click_listeners(&mut dom);
    let html = dioxus::ssr::render(&dom);

    assert!(html.contains("<nav class=\"tabs\""), "{html}");
    assert_eq!(html.matches("class=\"tab-button").count(), 4, "{html}");
    for label in ["セッション", "一覧", "負荷", "発火履歴"] {
        assert!(html.contains(label), "missing {label}: {html}");
    }
    assert!(
        html.contains(
            "class=\"tab-button is-selected\" type=\"button\" aria-pressed=true>発火履歴"
        ),
        "{html}"
    );

    for id in ids {
        dispatch_click(&dom, id);
    }
    assert_eq!(
        *events.lock().unwrap(),
        [
            ActiveTab::History,
            ActiveTab::Load,
            ActiveTab::List,
            ActiveTab::Session
        ]
    );
}

fn render_band_load_view(dom: &mut VirtualDom) -> String {
    rebuild_with_click_listeners(dom);
    dioxus::ssr::render(dom)
}

#[test]
fn 負荷viewは日次帯と累積差分と超過を表示して日付を通知する() {
    fn root() -> Element {
        rsx! {
            LoadView {
                rows: vec![
                    BandDay {
                        logical_date: "2026-09-27".to_owned(),
                        accumulated_rho_diff_seconds: 75 * 60,
                        accumulated_free_diff_seconds: -45 * 60,
                        durations: BandDurations {
                            unavailable_seconds: 10 * 60 * 60,
                            elapsed_seconds: 6 * 60 * 60,
                            repetitive_seconds: 90 * 60,
                            non_repetitive_seconds: 7 * 60 * 60,
                            rho_leeway_seconds: 60 * 60,
                        },
                    },
                    BandDay {
                        logical_date: "2026-09-28".to_owned(),
                        accumulated_rho_diff_seconds: 0,
                        accumulated_free_diff_seconds: 30 * 60,
                        durations: BandDurations {
                            unavailable_seconds: 0,
                            elapsed_seconds: 0,
                            repetitive_seconds: 0,
                            non_repetitive_seconds: 0,
                            rho_leeway_seconds: 0,
                        },
                    },
                ],
                observed_at_epoch_ms: Some(1_790_490_720_000),
                loading: false,
                error: None,
                on_refresh: move |_| {},
                on_select_date: move |_| {},
            }
        }
    }

    let mut dom = VirtualDom::new(root);
    let html = render_band_load_view(&mut dom);

    assert!(html.contains("今日から7日の負荷"), "{html}");
    assert!(html.contains("9/27(日)"), "{html}");
    assert!(
        html.contains("余差累+01:15、空差累-00:45"),
        "{html}"
    );
    assert!(
        html.contains(
            "<span>余差累</span><strong class=\"load-metric-value is-over\">+01:15"
        ),
        "{html}"
    );
    assert!(
        html.contains(
            "<span>空差累</span><strong class=\"load-metric-value is-within\">-00:45"
        ),
        "{html}"
    );
    assert!(
        html.contains(
            "<span>余差累</span><strong class=\"load-metric-value is-within\">+00:00"
        ),
        "{html}"
    );
    assert!(
        html.contains(
            "<span>空差累</span><strong class=\"load-metric-value is-over\">+00:30"
        ),
        "{html}"
    );
    assert!(html.contains("超過 01:30"), "{html}");
    assert!(html.contains("超過0時間0分"), "{html}");
}

#[test]
fn 負荷viewは日別負荷を初期表示して繰返集計表へlocal切替する() {
    #[derive(Clone)]
    struct Props {
        refresh_count: Arc<Mutex<usize>>,
    }

    fn root(props: Props) -> Element {
        let refresh_count = Arc::clone(&props.refresh_count);
        rsx! {
            LoadView {
                rows: vec![band_day("2026-10-03", 0)],
                routine_load_report: Some(RoutineLoadReport {
                    start_date: "2026-10-03".to_owned(),
                    end_date: "2026-10-30".to_owned(),
                    horizon_day_count: 28,
                    rows: vec![
                        RoutineLoadRow {
                            project_task_id: "project-1".to_owned(),
                            project_name: "健康".to_owned(),
                            routine_task_id: "routine-1".to_owned(),
                            routine_name: "運動".to_owned(),
                            repetition_interval_days: 2,
                            total_work_seconds: 28 * 60 * 60,
                            occurrence_day_count: 14,
                            average_work_seconds: Some(2 * 60 * 60),
                            peak_date: "2026-10-03".to_owned(),
                            peak_work_seconds: 2 * 60 * 60,
                        },
                        RoutineLoadRow {
                            project_task_id: "project-2".to_owned(),
                            project_name: "生活".to_owned(),
                            routine_task_id: "routine-2".to_owned(),
                            routine_name: "端数のある繰返".to_owned(),
                            repetition_interval_days: 1,
                            total_work_seconds: 11 * 60,
                            occurrence_day_count: 3,
                            average_work_seconds: Some(4 * 60),
                            peak_date: "2026-10-03".to_owned(),
                            peak_work_seconds: 3 * 60 + 40,
                        },
                    ],
                }),
                observed_at_epoch_ms: None,
                loading: false,
                error: None,
                on_refresh: move |_| *refresh_count.lock().unwrap() += 1,
                on_select_date: move |_| {},
            }
        }
    }

    let refresh_count = Arc::new(Mutex::new(0));
    let mut dom = VirtualDom::new_with_props(
        root,
        Props {
            refresh_count: Arc::clone(&refresh_count),
        },
    );
    let click_ids = rebuild_with_click_listeners(&mut dom);
    let initial_html = dioxus::ssr::render(&dom);

    assert!(initial_html.contains("今日から7日の負荷"), "{initial_html}");
    assert!(initial_html.contains("帯の凡例"), "{initial_html}");
    assert!(!initial_html.contains("routine-load-table"), "{initial_html}");
    assert!(
        initial_html.contains("aria-pressed=true>日別負荷"),
        "{initial_html}"
    );
    let daily_tab = initial_html.find(">日別負荷</button>").unwrap();
    let routine_tab = initial_html.find(">繰返負荷</button>").unwrap();
    assert!(daily_tab < routine_tab, "{initial_html}");
    assert_eq!(click_ids.len(), 4, "更新と2表示tabと日別rowが初期表示される");

    dispatch_click(&dom, click_ids[2]);
    dom.render_immediate_to_vec();
    let routine_html = dioxus::ssr::render(&dom);

    assert!(
        routine_html.contains("今日から27日後までの繰返負荷"),
        "{routine_html}"
    );
    assert!(routine_html.contains("10/3〜10/30"), "{routine_html}");
    for (class, heading) in [
        ("routine-load-interval", "間隔"),
        ("routine-load-total", "28日合計"),
        ("routine-load-occurrences", "発生日数"),
        ("routine-load-average", "1日平均"),
        ("routine-load-subject", "プロジェクト / 繰返"),
    ] {
        assert!(
            routine_html.contains(&format!(
                "<th class=\"{class}\" scope=\"col\">{heading}</th>"
            )),
            "missing {heading}: {routine_html}"
        );
    }
    let table_header = &routine_html[routine_html.find("<thead>").unwrap()
        ..routine_html.find("</thead>").unwrap()];
    let heading_positions = [
        "間隔",
        "28日合計",
        "発生日数",
        "1日平均",
        "プロジェクト / 繰返",
    ]
    .map(|heading| table_header.find(heading).unwrap());
    assert!(
        heading_positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{table_header}"
    );
    let table_body = &routine_html[routine_html.find("<tbody>").unwrap()
        ..routine_html.find("</tbody>").unwrap()];
    let value_positions = ["2日", "28:00", "14日", "02:00", "運動"]
        .map(|value| table_body.find(value).unwrap());
    assert!(
        value_positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{table_body}"
    );
    for cell in [
        "<td class=\"routine-load-interval\">2日</td>",
        "<td class=\"routine-load-total routine-load-number\">28:00</td>",
        "<td class=\"routine-load-occurrences routine-load-number\">14日</td>",
        "<td class=\"routine-load-average routine-load-number\">02:00</td>",
        "<th class=\"routine-load-subject\" scope=\"row\"><div class=\"routine-load-subject-scroll\" tabindex=0><strong class=\"routine-load-name task-kind-repetitive\">運動</strong><span class=\"routine-load-project\">健康</span></div></th>",
    ] {
        assert!(table_body.contains(cell), "missing {cell}: {table_body}");
    }
    for value in ["運動", "健康", "2日", "28:00", "14日", "02:00"] {
        assert!(routine_html.contains(value), "missing {value}: {routine_html}");
    }
    assert!(
        table_body.contains(
            "<td class=\"routine-load-average routine-load-number\">00:04</td>"
        ),
        "{table_body}"
    );
    assert!(!table_body.contains("00:03"), "{table_body}");
    assert!(!routine_html.contains("週平均"), "{routine_html}");
    assert!(!routine_html.contains("routine-load-weekly"), "{routine_html}");
    assert!(!table_body.contains("日ごと"), "{table_body}");
    assert!(
        routine_html.contains("aria-pressed=true>繰返負荷"),
        "{routine_html}"
    );
    assert!(
        routine_html.contains("class=\"load-mode-tabs\" role=\"group\" aria-label=\"負荷表示\""),
        "{routine_html}"
    );
    assert!(!routine_html.contains("role=\"tablist\""), "{routine_html}");
    assert!(!routine_html.contains("帯の凡例"), "{routine_html}");
    assert_eq!(*refresh_count.lock().unwrap(), 0, "表示切替で通信を要求しない");
}

#[test]
fn 負荷viewは繰返集計の取得中と取得済み空状態を区別する() {
    fn loading() -> Element {
        rsx! {
            LoadView {
                rows: Vec::new(),
                routine_load_report: None,
                observed_at_epoch_ms: None,
                loading: true,
                error: None,
                on_refresh: move |_| {},
                on_select_date: move |_| {},
            }
        }
    }
    let mut loading_dom = VirtualDom::new(loading);
    let loading_click_ids = rebuild_with_click_listeners(&mut loading_dom);
    let daily_loading_html = dioxus::ssr::render(&loading_dom);
    assert!(
        daily_loading_html.contains("負荷を取得しています…"),
        "{daily_loading_html}"
    );
    assert!(
        !daily_loading_html.contains("繰返負荷を取得しています…"),
        "{daily_loading_html}"
    );
    dispatch_click(&loading_dom, loading_click_ids[2]);
    loading_dom.render_immediate_to_vec();
    let loading_html = dioxus::ssr::render(&loading_dom);
    assert!(
        loading_html.contains("繰返負荷を取得しています…"),
        "{loading_html}"
    );
    assert!(loading_html.contains("更新中…"), "{loading_html}");

    fn empty() -> Element {
        rsx! {
            LoadView {
                rows: Vec::new(),
                routine_load_report: Some(RoutineLoadReport {
                    start_date: "2026-10-03".to_owned(),
                    end_date: "2026-10-05".to_owned(),
                    horizon_day_count: 3,
                    rows: Vec::new(),
                }),
                observed_at_epoch_ms: None,
                loading: false,
                error: None,
                on_refresh: move |_| {},
                on_select_date: move |_| {},
            }
        }
    }
    let mut empty_dom = VirtualDom::new(empty);
    let empty_click_ids = rebuild_with_click_listeners(&mut empty_dom);
    let daily_empty_html = dioxus::ssr::render(&empty_dom);
    assert!(
        daily_empty_html.contains("負荷は未取得です。"),
        "{daily_empty_html}"
    );
    assert!(
        !daily_empty_html.contains("今日から2日後までに発生する繰返負荷はありません。"),
        "{daily_empty_html}"
    );
    dispatch_click(&empty_dom, empty_click_ids[2]);
    empty_dom.render_immediate_to_vec();
    let empty_html = dioxus::ssr::render(&empty_dom);
    assert!(empty_html.contains("今日から2日後までの繰返負荷"), "{empty_html}");
    assert!(empty_html.contains("10/3〜10/5"), "{empty_html}");
    assert!(
        empty_html.contains("今日から2日後までに発生する繰返負荷はありません。"),
        "{empty_html}"
    );
}

#[test]
fn 負荷viewは当日だけ残り枠を全体barの上へ表示する() {
    fn root() -> Element {
        let today = BandDay {
            logical_date: "2026-09-27".to_owned(),
            accumulated_rho_diff_seconds: 0,
            accumulated_free_diff_seconds: 0,
            durations: BandDurations {
                unavailable_seconds: 8 * 60 * 60,
                elapsed_seconds: 8 * 60 * 60,
                repetitive_seconds: 60 * 60,
                non_repetitive_seconds: 2 * 60 * 60,
                rho_leeway_seconds: 60 * 60,
            },
        };
        let future = BandDay {
            logical_date: "2026-09-28".to_owned(),
            durations: BandDurations {
                elapsed_seconds: 0,
                ..today.durations
            },
            ..today
        };
        rsx! {
            LoadView {
                rows: vec![today, future],
                observed_at_epoch_ms: None,
                loading: false,
                error: None,
                on_refresh: move |_| {},
                on_select_date: move |_| {},
            }
        }
    }

    let mut dom = VirtualDom::new(root);
    let html = render_band_load_view(&mut dom);
    let today_start = html
        .find("<button class=\"load-day is-today\"")
        .expect("today row must exist");
    let future_start = html
        .find("<button class=\"load-day\"")
        .expect("future row must exist");
    let today_html = &html[today_start..future_start];
    let future_html = &html[future_start..];

    assert_eq!(today_html.matches("class=\"load-band\"").count(), 2, "{html}");
    assert_eq!(future_html.matches("class=\"load-band\"").count(), 1, "{html}");
    assert!(
        today_html.find("残り枠").unwrap() < today_html.find("1日全体").unwrap(),
        "{html}"
    );
    let focus_start = today_html
        .find("<span class=\"load-focus-group\"")
        .expect("remaining focus group must exist");
    let overview_start = today_html
        .find("<span class=\"load-band-caption load-overview-caption\"")
        .expect("full-day overview caption must exist");
    let focus_html = &today_html[focus_start..overview_start];
    assert!(focus_html.contains("08:00"), "{html}");
    for expected in [
        "残り枠8時間0分",
        "残り繰返1時間0分",
        "残り単発2時間0分",
        "残り余差1時間0分",
        "残り空き4時間0分",
    ] {
        assert!(today_html.contains(expected), "missing {expected}: {html}");
    }
    for expected in ["width:12.5000%", "width:25.0000%", "width:50.0000%"] {
        assert!(focus_html.contains(expected), "missing {expected}: {html}");
    }
}

#[test]
fn 負荷viewは右寄せ超過railを当日の二尺度と未来日へ表示する() {
    fn root() -> Element {
        let today = BandDay {
            logical_date: "2026-09-27".to_owned(),
            accumulated_rho_diff_seconds: 0,
            accumulated_free_diff_seconds: 0,
            durations: BandDurations {
                unavailable_seconds: 8 * 60 * 60,
                elapsed_seconds: 8 * 60 * 60,
                repetitive_seconds: 4 * 60 * 60,
                non_repetitive_seconds: 5 * 60 * 60,
                rho_leeway_seconds: 3 * 60 * 60,
            },
        };
        let future = BandDay {
            logical_date: "2026-09-28".to_owned(),
            durations: BandDurations {
                elapsed_seconds: 0,
                ..today.durations
            },
            ..today
        };
        rsx! {
            LoadView {
                rows: vec![today, future],
                observed_at_epoch_ms: None,
                loading: false,
                error: None,
                on_refresh: move |_| {},
                on_select_date: move |_| {},
            }
        }
    }

    let mut dom = VirtualDom::new(root);
    let html = render_band_load_view(&mut dom);
    let today_start = html
        .find("<button class=\"load-day is-today\"")
        .expect("today row must exist");
    let future_start = html
        .find("<button class=\"load-day\"")
        .expect("future row must exist");
    let today_html = &html[today_start..future_start];
    let future_html = &html[future_start..];

    assert_eq!(today_html.matches("class=\"load-overflow-rail\"").count(), 2, "{html}");
    assert_eq!(future_html.matches("class=\"load-overflow-rail\"").count(), 1, "{html}");
    assert_eq!(today_html.matches("aria-hidden=\"true\"").count(), 4, "{html}");
    assert!(today_html.contains("width:50.0000%"), "{html}");
    assert!(today_html.contains("width:16.6667%"), "{html}");
    assert!(future_html.contains("width:0.0000%"), "{html}");
    assert!(future_html.contains("is-zero"), "{html}");
    assert!(today_html.contains("超過 04:00"), "{html}");
    assert!(today_html.contains("超過4時間0分"), "{html}");
}

#[test]
fn 負荷viewは残り容量zeroを空barとして表示する() {
    fn root() -> Element {
        rsx! {
            LoadView {
                rows: vec![BandDay {
                    logical_date: "2026-09-27".to_owned(),
                    accumulated_rho_diff_seconds: 0,
                    accumulated_free_diff_seconds: 0,
                    durations: BandDurations {
                        unavailable_seconds: 24 * 60 * 60,
                        elapsed_seconds: 0,
                        repetitive_seconds: 60 * 60,
                        non_repetitive_seconds: 0,
                        rho_leeway_seconds: 0,
                    },
                }],
                observed_at_epoch_ms: None,
                loading: false,
                error: None,
                on_refresh: move |_| {},
                on_select_date: move |_| {},
            }
        }
    }

    let mut dom = VirtualDom::new(root);
    let html = render_band_load_view(&mut dom);
    let focus_start = html
        .find("<span class=\"load-focus-group\"")
        .expect("remaining focus group must exist");
    let overview_start = html
        .find("<span class=\"load-band-caption load-overview-caption\"")
        .expect("full-day overview caption must exist");
    let focus_html = &html[focus_start..overview_start];

    assert!(html.contains("残り枠0時間0分"), "{html}");
    assert_eq!(focus_html.matches("width:0.0000%").count(), 4, "{html}");
    assert!(focus_html.contains("width:100.0000%"), "{html}");
    assert!(html.contains("width:4.1667%"), "{html}");
    assert!(!html.contains("NaN"), "{html}");
    assert!(!html.contains("inf"), "{html}");
}

#[test]
fn 負荷viewは24時間以上の超過railを満幅へ打ち切る() {
    fn root() -> Element {
        rsx! {
            LoadView {
                rows: vec![BandDay {
                    logical_date: "2026-09-27".to_owned(),
                    accumulated_rho_diff_seconds: 0,
                    accumulated_free_diff_seconds: 0,
                    durations: BandDurations {
                        unavailable_seconds: 24 * 60 * 60,
                        elapsed_seconds: 0,
                        repetitive_seconds: 30 * 60 * 60,
                        non_repetitive_seconds: 0,
                        rho_leeway_seconds: 0,
                    },
                }],
                observed_at_epoch_ms: None,
                loading: false,
                error: None,
                on_refresh: move |_| {},
                on_select_date: move |_| {},
            }
        }
    }

    let mut dom = VirtualDom::new(root);
    let html = render_band_load_view(&mut dom);

    assert_eq!(
        html.matches("class=\"load-overflow-fill\" style=\"width:100.0000%\"")
            .count(),
        2,
        "{html}"
    );
    assert!(!html.contains("NaN"), "{html}");
    assert!(!html.contains("inf"), "{html}");
}

#[test]
fn 負荷viewはerror時にcompact固定高を解除するclassを付ける() {
    fn root() -> Element {
        rsx! {
            LoadView {
                rows: vec![band_day("2026-09-27", 0)],
                observed_at_epoch_ms: None,
                loading: false,
                error: Some("負荷の更新に失敗しました。".to_owned()),
                on_refresh: move |_| {},
                on_select_date: move |_| {},
            }
        }
    }

    let mut dom = VirtualDom::new(root);
    let html = render_band_load_view(&mut dom);

    assert!(html.contains("<section class=\"load-view has-error\""), "{html}");
}

#[test]
fn 背景更新中の負荷操作はbuttonを無効化する() {
    fn root() -> Element {
        rsx! {
            LoadView {
                rows: vec![band_day("2026-09-27", 0)],
                observed_at_epoch_ms: None,
                loading: false,
                error: None,
                server_actions_blocked: true,
                on_refresh: move |_| {},
                on_select_date: move |_| {},
            }
        }
    }

    let mut dom = VirtualDom::new(root);
    let html = render_band_load_view(&mut dom);

    assert_eq!(html.matches(" disabled").count(), 2, "{html}");
}

#[test]
fn 日付入力actionは正規化して選択し日付buttonは入力をclearする() {
    let mut date_input = DateInputState::default();
    date_input.edit("9/16".to_owned());

    let action = component_action_from_date_input(
        &mut date_input,
        "2026-09-16",
        crate::client::state::ListMode::Scheduled,
    )
        .expect("valid date input must create one action");
    assert!(matches!(
        action,
        ComponentAction::SelectDate(ref date) if date == "2026-09-16"
    ));
    assert_eq!(date_input.text(), "2026/9/16");

    let action =
        component_action_from_date_button(&mut date_input, "2026-09-17".to_owned());
    assert!(matches!(
        action,
        ComponentAction::SelectDate(ref date) if date == "2026-09-17"
    ));
    assert_eq!(date_input.text(), "");
    assert_eq!(date_input.error(), None);
}

#[test]
fn session操作は対応するcomponent_actionへ変換する() {
    for (kind, expected) in [
        (
            SessionActionKind::RestartWithoutRecording,
            "restart_without_recording",
        ),
        (SessionActionKind::Discard, "discard"),
        (SessionActionKind::Record, "record"),
        (SessionActionKind::Complete, "complete"),
        (
            SessionActionKind::CompleteWithoutRecording,
            "complete_without_recording",
        ),
        (
            SessionActionKind::ResumeCompletionConflict,
            "resume_conflict",
        ),
        (
            SessionActionKind::ConfirmCompletionConflict,
            "confirm_conflict",
        ),
    ] {
        let action = component_action_from_session_action(SessionAction {
            task_id: "task".to_owned(),
            kind,
        });
        let actual = match action {
            ComponentAction::RestartSessionWithoutRecording(task_id) if task_id == "task" => {
                "restart_without_recording"
            }
            ComponentAction::DiscardSession(task_id) if task_id == "task" => "discard",
            ComponentAction::RecordSession(task_id) if task_id == "task" => "record",
            ComponentAction::CompleteSession(task_id) if task_id == "task" => "complete",
            ComponentAction::CompleteSessionWithoutRecording(task_id) if task_id == "task" => {
                "complete_without_recording"
            }
            ComponentAction::ResumeCompletionConflict(task_id) if task_id == "task" => {
                "resume_conflict"
            }
            ComponentAction::ConfirmCompletionConflict(task_id) if task_id == "task" => {
                "confirm_conflict"
            }
            _ => "unexpected",
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn 計測再開と三終了操作はbrowser時刻のtick後にdispatchする() {
    for kind in [
        SessionActionKind::RestartWithoutRecording,
        SessionActionKind::Record,
        SessionActionKind::Complete,
        SessionActionKind::CompleteWithoutRecording,
    ] {
        let actions = component_actions_from_session_action(
            SessionAction {
                task_id: "task".to_owned(),
                kind,
            },
            60_000,
        );

        assert!(matches!(
            actions.first(),
            Some(ComponentAction::Tick {
                wall_now_epoch_ms: 60_000
            })
        ));
        assert_eq!(actions.len(), 2);
    }

    let discard = component_actions_from_session_action(
        SessionAction {
            task_id: "task".to_owned(),
            kind: SessionActionKind::Discard,
        },
        60_000,
    );
    assert!(matches!(
        discard.as_slice(),
        [ComponentAction::DiscardSession(task_id)] if task_id == "task"
    ));
}

#[test]
fn 初期化はstorage失敗時もbootstrapを一度だけ要求する() {
    let storage = MemoryStorage::failing_reads();

    let (state, effect) = initialize_client(&storage, 1_000);

    assert_eq!(effect, ClientEffect::Bootstrap { request_id: 1 });
    assert!(state.storage_write_blocked());
    assert!(state.mutation_globally_blocked());
    assert!(state.sessions().is_empty());
}

#[test]
fn component_actionは仕様の六操作だけをserver_effectへ変換する() {
    let storage = MemoryStorage::default();
    let (mut state, bootstrap) = initialize_client(&storage, 1_000);
    assert!(matches!(bootstrap, ClientEffect::Bootstrap { .. }));

    for action in [
        ComponentAction::SwitchTab(ActiveTab::List),
        ComponentAction::Tick {
            wall_now_epoch_ms: 2_000,
        },
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
        ComponentAction::RestartSessionWithoutRecording(RECORD_ID.to_owned()),
        ComponentAction::DiscardSession(RECORD_ID.to_owned()),
        ComponentAction::ConfirmRepositoryChecked,
    ] {
        assert_eq!(
            reduce_component_action_at(&mut state, &storage, 2_000, action),
            ClientEffect::None
        );
    }

    assert!(matches!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_000,
            ComponentAction::SelectDate("2026-09-05".to_owned())
        ),
        ClientEffect::ListTasks { request, .. } if request.logical_date == "2026-09-05"
    ));
    assert!(matches!(
        reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::AutoSession),
        ClientEffect::AutoSession { .. }
    ));

    let defer_storage = MemoryStorage::default();
    let (mut defer_state, _) = initialize_client(&defer_storage, 1_000);
    defer_state.restore_view_state(&ViewState {
        snapshot: snapshot(1_000),
        list_mode: crate::client::state::ListMode::Scheduled,
        list: Some(StoredActiveList::Scheduled {
            logical_date: "2026-09-05".to_owned(),
            rows: Vec::new(),
        }),
        active_tab: ActiveTab::List,
        task_name_filter: String::new(),
        date_input_text: String::new(),
    });
    assert!(matches!(
        reduce_component_action_at(
            &mut defer_state,
            &defer_storage,
            2_000,
            ComponentAction::DeferTask {
                task_id: RECORD_ID.to_owned(),
                expected_plan: crate::DeferPlan {
                    mode: crate::DeferMode::Normal,
                    requested_pending_until_epoch_ms: 1_000,
                    effective_pending_until_epoch_ms: None,
                    repetition_interval_days: None,
                },
            }
        ),
        ClientEffect::DeferTask { request, .. }
            if request.task_id == RECORD_ID
                && request.selected_logical_date == "2026-09-05"
                && request.expected_plan.mode == crate::DeferMode::Normal
    ));

    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_000,
            ComponentAction::AddSession {
                task: task(RECORD_ID),
                is_leaf: true,
            }
        ),
        ClientEffect::None
    );
    assert!(matches!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_000,
            ComponentAction::RecordSession(RECORD_ID.to_owned())
        ),
        ClientEffect::RecordSession { request, .. } if request.task_id == RECORD_ID
    ));

    let other_storage = MemoryStorage::default();
    let (mut other_state, _) = initialize_client(&other_storage, 1_000);
    reduce_component_action_at(
        &mut other_state,
        &other_storage,
        1_000,
        ComponentAction::AddSession {
            task: task(COMPLETE_ID),
            is_leaf: true,
        },
    );
    assert!(matches!(
        reduce_component_action_at(
            &mut other_state,
            &other_storage,
            1_000,
            ComponentAction::CompleteSession(COMPLETE_ID.to_owned())
        ),
        ClientEffect::CompleteSession { request, .. } if request.task_id == COMPLETE_ID
            && request.record_elapsed_seconds
    ));

    let discard_complete_storage = MemoryStorage::default();
    let (mut discard_complete_state, _) = initialize_client(&discard_complete_storage, 1_000);
    reduce_component_action_at(
        &mut discard_complete_state,
        &discard_complete_storage,
        1_000,
        ComponentAction::AddSession {
            task: task(COMPLETE_ID),
            is_leaf: true,
        },
    );
    assert!(matches!(
        reduce_component_action_at(
            &mut discard_complete_state,
            &discard_complete_storage,
            1_000,
            ComponentAction::CompleteSessionWithoutRecording(COMPLETE_ID.to_owned())
        ),
        ClientEffect::CompleteSession { request, .. } if request.task_id == COMPLETE_ID
            && !request.record_elapsed_seconds
    ));
}

#[test]
fn 負荷tab進入と更新はload_bandを要求する() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);

    let entered = reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::SwitchTab(ActiveTab::Load),
    );
    assert!(matches!(entered, ClientEffect::LoadBand { .. }));
    assert!(!ComponentOrchestrator::new().effect_is_background(&entered));

    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            1_000,
            ComponentAction::Tick {
                wall_now_epoch_ms: 2_000,
            },
        ),
        ClientEffect::None
    );
    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            1_000,
            ComponentAction::SwitchTab(ActiveTab::History),
        ),
        ClientEffect::None
    );

    let refreshed = reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::RefreshLoad,
    );
    assert!(matches!(refreshed, ClientEffect::LoadBand { .. }));
}

#[test]
fn 負荷取得はstale_responseを捨て失敗時に直前の表示を保持する() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    let first_id = match state.request_load_band() {
        ClientEffect::LoadBand { request_id } => request_id,
        effect => panic!("unexpected effect: {effect:?}"),
    };
    let second_id = match state.request_load_band() {
        ClientEffect::LoadBand { request_id } => request_id,
        effect => panic!("unexpected effect: {effect:?}"),
    };

    state.apply_load_band_result(
        first_id,
        Ok(WebSuccess {
            snapshot: load_snapshot(1_100),
            data: load_data(vec![band_day("2026-09-27", 1)]),
        }),
    );
    assert!(state.band_rows().is_empty());

    state.apply_load_band_result(
        second_id,
        Ok(WebSuccess {
            snapshot: load_snapshot(1_200),
            data: load_data(vec![band_day("2026-09-27", 2)]),
        }),
    );
    assert_eq!(state.band_rows()[0].durations.unavailable_seconds, 2);

    let failed_id = match state.request_load_band() {
        ClientEffect::LoadBand { request_id } => request_id,
        effect => panic!("unexpected effect: {effect:?}"),
    };
    state.apply_load_band_result(
        failed_id,
        Err(ServerFailure::Transport("offline".to_owned())),
    );
    assert_eq!(state.band_rows()[0].durations.unavailable_seconds, 2);
    assert!(state.band_error().is_some());
}

#[test]
fn 負荷取得は7日帯と28日繰返負荷を同時に置換する() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    let request_id = match state.request_load_band() {
        ClientEffect::LoadBand { request_id } => request_id,
        effect => panic!("unexpected effect: {effect:?}"),
    };
    state.apply_load_band_result(
        request_id,
        Ok(WebSuccess {
            snapshot: load_snapshot(1_100),
            data: LoadData {
                band_days: vec![band_day("2026-10-03", 1)],
                routine_load: RoutineLoadReport {
                    start_date: "2026-10-03".to_owned(),
                    end_date: "2026-10-30".to_owned(),
                    horizon_day_count: 28,
                    rows: vec![RoutineLoadRow {
                        project_task_id: "project".to_owned(),
                        project_name: "生活".to_owned(),
                        routine_task_id: "routine".to_owned(),
                        routine_name: "日次".to_owned(),
                        repetition_interval_days: 1,
                        total_work_seconds: 3_600,
                        occurrence_day_count: 4,
                        average_work_seconds: Some(900),
                        peak_date: "2026-10-03".to_owned(),
                        peak_work_seconds: 900,
                    }],
                },
            },
        }),
    );

    assert_eq!(state.band_rows().len(), 1);
    assert_eq!(state.routine_load_report().unwrap().rows[0].routine_name, "日次");
}

#[test]
fn 負荷日付は一覧tabへ移動して対象日を取得する() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    state.switch_tab(ActiveTab::Load);

    let effect = reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::SelectLoadDate("2026-09-29".to_owned()),
    );

    assert_eq!(state.active_tab(), ActiveTab::List);
    assert!(matches!(
        effect,
        ClientEffect::ListTasks { request, .. } if request.logical_date == "2026-09-29"
    ));
}

#[test]
fn 復元した負荷tabはbootstrap後に負荷を取得する() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    state.restore_view_state(&ViewState {
        snapshot: load_snapshot(900),
        list_mode: crate::client::state::ListMode::Scheduled,
        list: None,
        active_tab: ActiveTab::Load,
        task_name_filter: String::new(),
        date_input_text: String::new(),
    });

    let effect = state.apply_bootstrap_result(1, Ok(load_snapshot(1_100)));

    assert!(matches!(effect, ClientEffect::LoadBand { .. }));
}

#[test]
fn 復元した負荷tabはbootstrap失敗時に負荷を取得しない() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    state.restore_view_state(&ViewState {
        snapshot: load_snapshot(900),
        list_mode: crate::client::state::ListMode::Scheduled,
        list: None,
        active_tab: ActiveTab::Load,
        task_name_filter: String::new(),
        date_input_text: String::new(),
    });

    let effect = state.apply_bootstrap_result(
        1,
        Err(ServerFailure::Transport("offline".to_owned())),
    );

    assert_eq!(effect, ClientEffect::None);
}

fn load_snapshot(observed_at_epoch_ms: i64) -> ServerSnapshot {
    ServerSnapshot {
        observed_at_epoch_ms,
        logical_date: "2026-09-27".to_owned(),
        buffer_seconds: 0,
    }
}

fn band_day(logical_date: &str, unavailable_seconds: i64) -> BandDay {
    BandDay {
        logical_date: logical_date.to_owned(),
        accumulated_rho_diff_seconds: 0,
        accumulated_free_diff_seconds: 0,
        durations: BandDurations {
            unavailable_seconds,
            elapsed_seconds: 0,
            repetitive_seconds: 0,
            non_repetitive_seconds: 0,
            rho_leeway_seconds: 0,
        },
    }
}

fn load_data(band_days: Vec<BandDay>) -> LoadData {
    LoadData {
        band_days,
        routine_load: RoutineLoadReport {
            start_date: "2026-09-27".to_owned(),
            end_date: "2026-10-24".to_owned(),
            horizon_day_count: 28,
            rows: Vec::new(),
        },
    }
}

#[test]
fn 一覧のセッション追加後はセッションtabへ切り替えられserver通信を発生させない() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    let mut task_name_filter = "  実装  ".to_owned();
    let mut date_input = DateInputState::default();
    date_input.edit("2026/9/16".to_owned());

    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::SwitchTab(ActiveTab::List),
    );
    let previous_session_count = state.sessions().len();
    let effect = reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    reset_task_name_filter_after_session_add(
        &mut task_name_filter,
        previous_session_count,
        state.sessions().len(),
    );

    assert_eq!(effect, ClientEffect::None);
    assert_eq!(state.sessions().len(), 1);
    assert_eq!(state.active_tab(), ActiveTab::Session);
    assert_eq!(
        state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::Normal
    );
    assert!(state.history().is_empty());
    assert!(task_name_filter.is_empty());
    assert_eq!(date_input.text(), "2026/9/16");
    assert_eq!(date_input.error(), None);
}

#[test]
fn 製品orchestratorの一覧追加は成功時だけ検索を解除し日付状態を保つ() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();
    let bootstrap = orchestrator.mount(&storage, 1_000);
    let bootstrap_request_id = match bootstrap {
        ClientEffect::Bootstrap { request_id } => request_id,
        _ => panic!("mount must request bootstrap"),
    };
    orchestrator.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id: bootstrap_request_id,
            result: Ok(ServerSnapshot {
                observed_at_epoch_ms: 1_000,
                logical_date: "2026-09-05".to_owned(),
                buffer_seconds: 60,
            }),
        },
    );
    orchestrator.action(
        &storage,
        1_000,
        ComponentAction::SwitchTab(ActiveTab::List),
    );
    let list_effect = orchestrator.action(
        &storage,
        1_000,
        ComponentAction::SelectDate("2026-09-16".to_owned()),
    );
    let request_id = match list_effect {
        ClientEffect::ListTasks { request_id, .. } => request_id,
        _ => panic!("date selection must request the list"),
    };
    orchestrator.apply_response(
        &storage,
        ClientResponse::ListTasks {
            request_id,
            requested_date: "2026-09-16".to_owned(),
            result: Ok(WebSuccess {
                snapshot: ServerSnapshot {
                    observed_at_epoch_ms: 1_000,
                    logical_date: "2026-09-05".to_owned(),
                    buffer_seconds: 60,
                },
                data: Vec::new(),
            }),
        },
    );
    let mut date_input = DateInputState::default();
    date_input.edit("不正".to_owned());
    assert_eq!(date_input.submit("2026-09-05"), None);
    assert!(date_input.error().is_some());
    orchestrator.edit_task_name_filter(&storage, "実装".to_owned());
    let previous_history_len = orchestrator.state().unwrap().history().len();

    let effect = orchestrator.start_session_from_list(
        &storage,
        1_000,
        task(RECORD_ID),
        true,
    );

    let state = orchestrator.state().unwrap();
    assert_eq!(effect, ClientEffect::None);
    assert_eq!(state.active_tab(), ActiveTab::Session);
    assert_eq!(state.selected_logical_date(), Some("2026-09-16"));
    assert_eq!(state.history().len(), previous_history_len);
    assert!(orchestrator.task_name_filter().is_empty());
    assert_eq!(date_input.text(), "不正");
    assert!(date_input.error().is_some());
}

#[test]
fn 製品orchestratorの一覧追加は保存失敗時に検索を保つ() {
    let storage = MemoryStorage::failing_writes();
    let mut orchestrator = ComponentOrchestrator::new();
    orchestrator.mount(&storage, 1_000);
    orchestrator.action(
        &storage,
        1_000,
        ComponentAction::SwitchTab(ActiveTab::List),
    );
    orchestrator.edit_task_name_filter(&storage, "保存失敗".to_owned());

    let effect = orchestrator.start_session_from_list(
        &storage,
        1_000,
        task(RECORD_ID),
        true,
    );

    assert_eq!(effect, ClientEffect::None);
    assert!(orchestrator.state().unwrap().sessions().is_empty());
    assert_eq!(orchestrator.task_name_filter(), "保存失敗");
}

#[test]
fn 一覧のセッション追加が拒否された場合は一覧tabに留まる() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);

    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::SwitchTab(ActiveTab::List),
    );

    for (task, is_leaf) in [(task(RECORD_ID), true), (task(COMPLETE_ID), false)] {
        let mut task_name_filter = "絞り込み中".to_owned();
        let previous_session_count = state.sessions().len();
        let effect = reduce_component_action_at(
            &mut state,
            &storage,
            1_000,
            ComponentAction::AddSession { task, is_leaf },
        );
        reset_task_name_filter_after_session_add(
            &mut task_name_filter,
            previous_session_count,
            state.sessions().len(),
        );

        assert_eq!(effect, ClientEffect::None);
        assert_eq!(state.sessions().len(), 1);
        assert_eq!(state.active_tab(), ActiveTab::List);
        assert_eq!(task_name_filter, "絞り込み中");
    }

    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    let mut task_name_filter = "ロック中".to_owned();
    let previous_session_count = state.sessions().len();
    let effect = reduce_component_action_at(
        &mut state,
        &storage,
        1_001,
        ComponentAction::AddSession {
            task: task(COMPLETE_ID),
            is_leaf: true,
        },
    );
    reset_task_name_filter_after_session_add(
        &mut task_name_filter,
        previous_session_count,
        state.sessions().len(),
    );

    assert_eq!(effect, ClientEffect::None);
    assert_eq!(state.sessions().len(), 1);
    assert_eq!(state.active_tab(), ActiveTab::List);
    assert_eq!(task_name_filter, "ロック中");
}

#[test]
fn 一覧のセッション保存失敗時は一覧tabに留まる() {
    let storage = MemoryStorage::failing_writes();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::SwitchTab(ActiveTab::List),
    );

    let mut task_name_filter = "保存失敗".to_owned();
    let previous_session_count = state.sessions().len();
    let effect = reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    reset_task_name_filter_after_session_add(
        &mut task_name_filter,
        previous_session_count,
        state.sessions().len(),
    );

    assert_eq!(effect, ClientEffect::None);
    assert!(state.sessions().is_empty());
    assert_eq!(state.active_tab(), ActiveTab::List);
    assert_eq!(task_name_filter, "保存失敗");
}

#[test]
fn 持ち歩きロックはguard対象actionの期限を延長し最初のsession追加成功時は再ロックする() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );

    for action in [
        ComponentAction::AutoSession,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
        ComponentAction::RestartSessionWithoutRecording(RECORD_ID.to_owned()),
        ComponentAction::DiscardSession(RECORD_ID.to_owned()),
        ComponentAction::RecordSession(RECORD_ID.to_owned()),
        ComponentAction::CompleteSession(RECORD_ID.to_owned()),
        ComponentAction::CompleteSessionWithoutRecording(RECORD_ID.to_owned()),
        ComponentAction::ResumeCompletionConflict(RECORD_ID.to_owned()),
        ComponentAction::ConfirmCompletionConflict(RECORD_ID.to_owned()),
        ComponentAction::ConfirmRepositoryChecked,
    ] {
        assert_eq!(
            reduce_component_action_at(&mut state, &storage, 1_001, action),
            ClientEffect::None
        );
    }

    for action in [
        ComponentAction::AutoSession,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
        ComponentAction::RestartSessionWithoutRecording(RECORD_ID.to_owned()),
        ComponentAction::DiscardSession(RECORD_ID.to_owned()),
        ComponentAction::RecordSession(RECORD_ID.to_owned()),
        ComponentAction::CompleteSession(RECORD_ID.to_owned()),
        ComponentAction::CompleteSessionWithoutRecording(RECORD_ID.to_owned()),
        ComponentAction::ResumeCompletionConflict(RECORD_ID.to_owned()),
        ComponentAction::ConfirmCompletionConflict(RECORD_ID.to_owned()),
        ComponentAction::ConfirmRepositoryChecked,
    ] {
        let action_storage = MemoryStorage::default();
        let (mut action_state, _) = initialize_client(&action_storage, 1_000);
        reduce_component_action_at(
            &mut action_state,
            &action_storage,
            1_000,
            ComponentAction::EnableCarryLock,
        );
        reduce_component_action_at(
            &mut action_state,
            &action_storage,
            2_000,
            ComponentAction::ArmCarryLock,
        );

        let adds_first_session =
            matches!(&action, ComponentAction::AddSession { is_leaf: true, .. });
        let _ = reduce_component_action_at(&mut action_state, &action_storage, 2_001, action);

        let expected = if adds_first_session {
            crate::client::carry_lock::CarryLockMode::Locked
        } else {
            crate::client::carry_lock::CarryLockMode::ArmedUntil(17_001)
        };
        assert_eq!(action_state.carry_lock_mode(), expected);
    }

    reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::ArmCarryLock);
    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_001,
            ComponentAction::SwitchTab(ActiveTab::List)
        ),
        ClientEffect::None
    );
    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_002,
            ComponentAction::Tick {
                wall_now_epoch_ms: 2_002,
            }
        ),
        ClientEffect::None
    );
    assert!(matches!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_002,
            ComponentAction::SelectDate("2026-09-05".to_owned())
        ),
        ClientEffect::ListTasks { .. }
    ));
    assert_eq!(
        state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::ArmedUntil(17_000),
        "閲覧操作では無操作期限を延長しない"
    );
    assert!(matches!(
        reduce_component_action_at(&mut state, &storage, 2_003, ComponentAction::AutoSession),
        ClientEffect::AutoSession { .. }
    ));
    assert_eq!(
        state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::ArmedUntil(17_003)
    );
    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            10_000,
            ComponentAction::AddSession {
                task: task(RECORD_ID),
                is_leaf: true,
            }
        ),
        ClientEffect::None
    );
    assert_eq!(
        state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::Locked,
        "最初のsession追加成功後は期限延長より即時再ロックを優先する"
    );
    reduce_component_action_at(
        &mut state,
        &storage,
        2_004,
        ComponentAction::DisableCarryLock,
    );

    let failing_storage = MemoryStorage::failing_writes();
    let (mut failing_state, _) = initialize_client(&failing_storage, 1_000);
    reduce_component_action_at(
        &mut failing_state,
        &failing_storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(
        &mut failing_state,
        &failing_storage,
        2_000,
        ComponentAction::ArmCarryLock,
    );
    reduce_component_action_at(
        &mut failing_state,
        &failing_storage,
        3_000,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    assert_eq!(
        failing_state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::ArmedUntil(18_000),
        "localStorage失敗でもdispatch時点から期限を延長する"
    );
}

#[test]
fn 一時許可中に一覧から最初のsessionを追加すると即時再ロックする() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::ArmCarryLock);

    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_001,
            ComponentAction::AddSession {
                task: task(RECORD_ID),
                is_leaf: true,
            },
        ),
        ClientEffect::None
    );

    assert_eq!(state.sessions().len(), 1);
    assert_eq!(
        state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::Locked
    );
}

#[test]
fn sessionが0件から1件へ増えない追加では一時許可を維持する() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    reduce_component_action_at(
        &mut state,
        &storage,
        1_001,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::ArmCarryLock);

    reduce_component_action_at(
        &mut state,
        &storage,
        2_001,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    assert_eq!(state.sessions().len(), 1);
    assert_eq!(
        state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::ArmedUntil(17_001)
    );

    reduce_component_action_at(
        &mut state,
        &storage,
        2_002,
        ComponentAction::AddSession {
            task: task(COMPLETE_ID),
            is_leaf: true,
        },
    );
    assert_eq!(state.sessions().len(), 2);
    assert_eq!(
        state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::ArmedUntil(17_002)
    );

    let failing_storage = MemoryStorage::failing_writes();
    let (mut failing_state, _) = initialize_client(&failing_storage, 1_000);
    reduce_component_action_at(
        &mut failing_state,
        &failing_storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(
        &mut failing_state,
        &failing_storage,
        2_000,
        ComponentAction::ArmCarryLock,
    );
    reduce_component_action_at(
        &mut failing_state,
        &failing_storage,
        2_001,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    assert!(failing_state.sessions().is_empty());
    assert_eq!(
        failing_state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::ArmedUntil(17_001)
    );
}

#[test]
fn armed期限判定はactionのmonotonic時刻を使い期限切れ操作を遮断する() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::ArmCarryLock);

    assert_eq!(
        reduce_component_action_at(&mut state, &storage, 17_000, ComponentAction::AutoSession),
        ClientEffect::None
    );
    assert!(state.carry_lock_locked());
}

#[test]
fn armedはmonotonic時刻が期限へ到達した時点でlockedへ戻る() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::ArmCarryLock);

    reduce_component_action_at(
        &mut state,
        &storage,
        16_999,
        ComponentAction::Tick {
            wall_now_epoch_ms: i64::MAX,
        },
    );
    assert!(!state.carry_lock_locked());

    reduce_component_action_at(
        &mut state,
        &storage,
        17_000,
        ComponentAction::Tick {
            wall_now_epoch_ms: i64::MIN,
        },
    );
    assert!(state.carry_lock_locked());
}

#[test]
fn wall_clock変動にかかわらずarmedはmonotonic_15秒境界で失効する() {
    let storage = MemoryStorage::default();
    let (mut tick_state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut tick_state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(
        &mut tick_state,
        &storage,
        2_000,
        ComponentAction::ArmCarryLock,
    );
    reduce_component_action_at(
        &mut tick_state,
        &storage,
        2_500,
        ComponentAction::Tick {
            wall_now_epoch_ms: 50_000,
        },
    );
    assert_eq!(
        tick_state.carry_lock_mode(),
        crate::client::carry_lock::CarryLockMode::ArmedUntil(17_000)
    );
    reduce_component_action_at(
        &mut tick_state,
        &storage,
        3_000,
        ComponentAction::Tick {
            wall_now_epoch_ms: -50_000,
        },
    );
    assert_eq!(tick_state.tick_now_epoch_ms(), -50_000);
    assert!(!tick_state.carry_lock_locked());

    reduce_component_action_at(
        &mut tick_state,
        &storage,
        16_999,
        ComponentAction::Tick {
            wall_now_epoch_ms: i64::MAX,
        },
    );
    assert!(!tick_state.carry_lock_locked());

    reduce_component_action_at(
        &mut tick_state,
        &storage,
        17_000,
        ComponentAction::Tick {
            wall_now_epoch_ms: i64::MIN,
        },
    );
    assert!(tick_state.carry_lock_locked());
}

#[test]
fn armedはmonotonic時計が後退した時点でlockedへ戻る() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::ArmCarryLock);
    reduce_component_action_at(
        &mut state,
        &storage,
        2_500,
        ComponentAction::Tick {
            wall_now_epoch_ms: 2_500,
        },
    );

    reduce_component_action_at(
        &mut state,
        &storage,
        2_499,
        ComponentAction::Tick {
            wall_now_epoch_ms: 2_499,
        },
    );

    assert!(state.carry_lock_locked());
}

#[test]
fn armedの即時再lock_actionは次の変更操作を遮断する() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    reduce_component_action_at(
        &mut state,
        &storage,
        1_000,
        ComponentAction::EnableCarryLock,
    );
    reduce_component_action_at(&mut state, &storage, 2_000, ComponentAction::ArmCarryLock);

    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            2_001,
            ComponentAction::RelockCarryLock,
        ),
        ClientEffect::None
    );
    assert!(state.carry_lock_locked());
    assert_eq!(
        reduce_component_action_at(&mut state, &storage, 2_002, ComponentAction::AutoSession),
        ClientEffect::None
    );
}

#[test]
#[cfg(feature = "web")]
fn carry_lock_warningはbrowser_page_modelのwarningsへ合流する() {
    let storage = MemoryStorage::failing_carry_lock_reads();
    let (state, _) = initialize_client(&storage, 1_000);

    let model = BrowserPageModel::from_state(&state);

    assert!(model
        .warnings
        .iter()
        .any(|warning| warning.contains("持ち歩きロック")));
    let BrowserPageModel {
        active_tab,
        buffer,
        sessions,
        rows,
        active_task_ids,
        dates,
        history,
        warnings,
        safety_warning,
        display_error,
        global_blocked,
        can_confirm,
        auto_session_in_flight,
        auto_session_empty,
        carry_lock,
        ..
    } = model;
    let _ = (
        active_tab,
        buffer,
        sessions,
        rows,
        active_task_ids,
        dates,
        history,
        warnings,
        safety_warning,
        display_error,
        global_blocked,
        can_confirm,
        auto_session_in_flight,
        auto_session_empty,
        carry_lock,
    );
}
