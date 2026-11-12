use super::super::load_view::LoadView;
use super::super::view_test_support::{dispatch_click, rebuild_with_click_listeners};
use crate::{BandDay, BandDurations};
use dioxus::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Props {
    selected: Arc<Mutex<Vec<String>>>,
    start_date: Option<chrono::NaiveDate>,
}

fn root(props: Props) -> Element {
    let rows = [(1, -1, 1), (0, 0, 0), (-1, 123 * 3600 + 45 * 60, -1)]
        .into_iter()
        .enumerate()
        .map(|(index, (rho, free, day_difference))| BandDay {
            logical_date: format!("2026-11-{:02}", index + 7),
            accumulated_rho_diff_seconds: rho,
            accumulated_free_diff_seconds: free,
            durations: BandDurations {
                unavailable_seconds: 8 * 3600,
                elapsed_seconds: 0,
                repetitive_seconds: 0,
                non_repetitive_seconds: 16 * 3600 + day_difference,
                rho_leeway_seconds: 0,
            },
        })
        .collect();
    rsx! {
        LoadView {
            rows,
            observed_at_epoch_ms: None,
            loading: false,
            error: None,
            on_refresh: move |_| {},
            on_select_date: move |date| props.selected.lock().unwrap().push(date),
        }
    }
}

fn render() -> String {
    let mut dom = VirtualDom::new_with_props(
        root,
        Props {
            selected: Arc::default(),
            start_date: None,
        },
    );
    dom.rebuild_in_place();
    dioxus::ssr::render(&dom)
}

#[test]
fn 日別負荷は共通見出しと日付数値帯の順に表示する() {
    let html = render();
    let heading = html
        .split("load-column-headings")
        .nth(1)
        .unwrap()
        .split("load-days")
        .next()
        .unwrap();
    let labels = ["日付", "余差累", "空差累", "超過", "時間内訳"];
    let positions = labels.map(|label| heading.find(label).unwrap());
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(
        html.contains("<span>11/7</span><span class=\"load-weekday is-saturday\">(土)</span>"),
        "{html}"
    );
    assert!(!html.contains("load-today"));
    assert!(!html.contains("一覧を見る"));
    assert!(!html.contains("load-overview-caption"));
    assert!(!html.contains("24:00"));
    for row in html.split("<button class=\"load-day").skip(1) {
        let row = row.split("</button>").next().unwrap();
        let date = row.find("class=\"load-date\"").unwrap();
        let first_value = row.find("class=\"load-metric-value").unwrap();
        let bands = row.find("class=\"load-bands\"").unwrap();
        assert!(date < first_value && first_value < bands, "{row}");
        assert_eq!(row.matches("class=\"load-metric-value").count(), 3);
    }
}

#[test]
fn 日別負荷は余差累と超過のゼロを灰色にし丸め前の正負を維持する() {
    let html = render();
    let rows = html
        .split("<button class=\"load-day")
        .skip(1)
        .collect::<Vec<_>>();
    let expected = [
        [
            ("is-over", "+00:00"),
            ("is-within", "-00:00"),
            ("is-over", "00:00"),
        ],
        [
            ("is-zero", "+00:00"),
            ("is-within", "+00:00"),
            ("is-zero", "0:00"),
        ],
        [
            ("is-within", "-00:00"),
            ("is-over", "+99:59"),
            ("is-within", "-00:00"),
        ],
    ];
    let aria_values = ["超過0時間0分1秒", "超過0時間0分", "超過マイナス0時間0分1秒"];
    assert_eq!(rows.len(), expected.len());
    for ((row, values), aria_value) in rows.iter().zip(expected).zip(aria_values) {
        assert!(row.contains(&format!("{aria_value}。")), "{row}");
        let rendered = row
            .split("</button>")
            .next()
            .unwrap()
            .split("<strong class=\"load-metric-value ")
            .skip(1)
            .collect::<Vec<_>>();
        assert_eq!(rendered.len(), 3);
        for (actual, (class, value)) in rendered.iter().zip(values) {
            assert!(
                actual.starts_with(&format!("{class}\">{value}</strong>")),
                "{actual}"
            );
        }
    }
}

#[test]
fn 日別負荷の各行は対象日の一覧選択を通知する() {
    let selected = Arc::default();
    let mut dom = VirtualDom::new_with_props(
        root,
        Props {
            selected: Arc::clone(&selected),
            start_date: None,
        },
    );
    let listeners = rebuild_with_click_listeners(&mut dom);
    assert_eq!(listeners.len(), 6);
    for id in &listeners[3..] {
        dispatch_click(&dom, *id);
    }
    assert_eq!(
        *selected.lock().unwrap(),
        ["2026-11-07", "2026-11-08", "2026-11-09"]
    );
}

fn fourteen_day_root(props: Props) -> Element {
    let start = props
        .start_date
        .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(2026, 11, 25).unwrap());
    let rows = (0..14)
        .map(|offset| BandDay {
            logical_date: (start + chrono::Duration::days(offset)).to_string(),
            accumulated_rho_diff_seconds: 0,
            accumulated_free_diff_seconds: 0,
            durations: BandDurations {
                unavailable_seconds: 8 * 3600,
                elapsed_seconds: 4 * 3600 * i64::from(offset == 0),
                repetitive_seconds: 5 * 3600,
                non_repetitive_seconds: if offset == 0 { 330 * 60 } else { 2 * 3600 },
                rho_leeway_seconds: 3600,
            },
        })
        .collect();
    rsx! {
        LoadView {
            rows,
            observed_at_epoch_ms: None,
            loading: false,
            error: None,
            on_refresh: move |_| {},
            on_select_date: move |date| props.selected.lock().unwrap().push(date),
        }
    }
}

#[test]
fn 日別負荷の超過は余差を除き帯の余差と空きを維持する() {
    let mut dom = VirtualDom::new_with_props(
        fourteen_day_root,
        Props {
            selected: Arc::default(),
            start_date: None,
        },
    );
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);
    let today = html
        .split_once("<button class=\"load-day is-today\"")
        .unwrap()
        .1
        .split_once("</button>")
        .unwrap()
        .0;
    assert!(today.contains("load-metric-value is-within\">-01:30</strong>"));
    assert!(today.contains("超過マイナス1時間30分。"));
    assert!(today.contains("余差1時間0分、空き0時間30分"));
    assert_eq!(
        today
            .matches("load-overflow-fill is-zero\" style=\"width:0.0000%")
            .count(),
        2
    );
    assert!(today.contains("load-band-segment is-leeway\" style=\"width:4.1667%"));
    assert!(today.contains("load-band-segment is-free\" style=\"width:2.0833%"));
}

#[test]
fn 日別負荷は月境界を跨ぐ14日を表示し最終日を選択できる() {
    let selected = Arc::default();
    let mut dom = VirtualDom::new_with_props(
        fourteen_day_root,
        Props {
            selected: Arc::clone(&selected),
            start_date: None,
        },
    );
    let listeners = rebuild_with_click_listeners(&mut dom);
    let html = dioxus::ssr::render(&dom);
    assert!(html.contains("今日から14日の負荷"), "{html}");
    assert_eq!(html.matches("<button class=\"load-day").count(), 14);
    assert!(html.contains("<span>11/25</span><span class=\"load-weekday\">(水)</span>"));
    assert!(html.contains("<span>12/8</span><span class=\"load-weekday\">(火)</span>"));
    assert!(!html.contains("14日間"));
    assert!(!html.contains('〜'));
    assert_eq!(listeners.len(), 17);
    dispatch_click(&dom, *listeners.last().unwrap());
    assert_eq!(*selected.lock().unwrap(), ["2026-12-08"]);
}

#[test]
fn 日別負荷は日曜と月曜の間で週を分け月境界でも日付選択を維持する() {
    let selected = Arc::default();
    let mut dom = VirtualDom::new_with_props(
        fourteen_day_root,
        Props {
            selected: Arc::clone(&selected),
            start_date: None,
        },
    );
    let listeners = rebuild_with_click_listeners(&mut dom);
    let html = dioxus::ssr::render(&dom);
    let weeks = html
        .split("<div class=\"load-week\">")
        .skip(1)
        .collect::<Vec<_>>();
    assert_eq!(weeks.len(), 3, "{html}");
    let dates = [
        ("11/25", "11/29", 5),
        ("11/30", "12/6", 7),
        ("12/7", "12/8", 2),
    ];
    for (week, (first, last, count)) in weeks.iter().zip(dates) {
        assert_eq!(week.matches("<button class=\"load-day").count(), count);
        assert!(week.contains(&format!("<span>{first}</span>")));
        assert!(week.contains(&format!("<span>{last}</span>")));
    }
    for id in &listeners[3..] {
        dispatch_click(&dom, *id);
    }
    let start = chrono::NaiveDate::from_ymd_opt(2026, 11, 25).unwrap();
    let expected = (0..14)
        .map(|offset| (start + chrono::Duration::days(offset)).to_string())
        .collect::<Vec<_>>();
    assert_eq!(*selected.lock().unwrap(), expected);
}

#[test]
fn 日別負荷は月曜開始時に先頭の空週を追加しない() {
    for (start, first, second) in [
        ((2026, 11, 30), "11/30", "12/7"),
        ((2026, 5, 25), "5/25", "6/1"),
    ] {
        let mut dom = VirtualDom::new_with_props(
            fourteen_day_root,
            Props {
                selected: Arc::default(),
                start_date: chrono::NaiveDate::from_ymd_opt(start.0, start.1, start.2),
            },
        );
        dom.rebuild_in_place();
        let html = dioxus::ssr::render(&dom);
        let weeks = html
            .split("<div class=\"load-week\">")
            .skip(1)
            .collect::<Vec<_>>();
        assert_eq!(weeks.len(), 2, "{html}");
        for (week, date) in weeks.iter().zip([first, second]) {
            assert!(week.contains(&format!(
                "<span>{date}</span><span class=\"load-weekday\">(月)</span>"
            )));
            assert_eq!(week.matches("<button class=\"load-day").count(), 7);
        }
    }
}

#[test]
fn 日別負荷は更新時刻を表示せず括弧を含む週末曜日を色分けする() {
    let html = render();
    assert!(!html.contains("load-updated"));
    assert!(
        html.contains("<span class=\"load-weekday is-saturday\">(土)</span>"),
        "{html}"
    );
    assert!(
        html.contains("<span class=\"load-weekday is-sunday\">(日)</span>"),
        "{html}"
    );
    assert!(
        html.contains("<span class=\"load-weekday\">(月)</span>"),
        "{html}"
    );
    assert!(html.contains("空差累+123時間45分") || html.contains("空差累+123:45"));
}

fn bounded_values_root() -> Element {
    rsx! {
        LoadView {
            rows: vec![BandDay {
                logical_date: "2026-12-31".to_owned(),
                accumulated_rho_diff_seconds: -100 * 3600,
                accumulated_free_diff_seconds: 100 * 3600,
                durations: BandDurations {
                    unavailable_seconds: 0, elapsed_seconds: 0,
                    repetitive_seconds: 0, non_repetitive_seconds: 147 * 3600,
                    rho_leeway_seconds: 0,
                },
            }],
            observed_at_epoch_ms: None, loading: false, error: None,
            on_refresh: move |_| {}, on_select_date: move |_| {},
        }
    }
}

#[test]
fn 日別負荷の2桁表示は上限を超える実値をariaへ保持する() {
    let mut dom = VirtualDom::new(bounded_values_root);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);
    assert!(html.contains(">-99:59</strong>"));
    assert!(html.contains(">+99:59</strong>"));
    assert!(html.contains(">99:59</strong>"));
    assert!(html.contains("余差累-100:00、空差累+100:00、超過123時間0分"));
    assert!(html.contains("<span>12/31</span><span class=\"load-weekday\">(木)</span>"));
}
