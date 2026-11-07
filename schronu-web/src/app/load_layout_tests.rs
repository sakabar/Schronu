use super::super::load_view::LoadView;
use super::super::view_test_support::{dispatch_click, rebuild_with_click_listeners};
use crate::{BandDay, BandDurations};
use dioxus::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Props {
    selected: Arc<Mutex<Vec<String>>>,
}

fn root(props: Props) -> Element {
    let rows = [(1, -1, 1), (0, 0, 0), (-1, 123 * 3600 + 45 * 60, 0)]
        .into_iter()
        .enumerate()
        .map(|(index, (rho, free, overflow))| BandDay {
            logical_date: format!("2026-11-{:02}", index + 7),
            accumulated_rho_diff_seconds: rho,
            accumulated_free_diff_seconds: free,
            durations: BandDurations {
                unavailable_seconds: 8 * 3600,
                elapsed_seconds: 0,
                repetitive_seconds: 0,
                non_repetitive_seconds: 16 * 3600 + overflow,
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
    let labels = ["日付", "余差累", "空差累", "超過", "帯グラフ"];
    let positions = labels.map(|label| heading.find(label).unwrap());
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(
        html.contains("<span>11/7</span><small>土</small>"),
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
fn 日別負荷の3数値は丸め前の正値を赤にしzero以下を緑にする() {
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
            ("is-within", "+00:00"),
            ("is-within", "+00:00"),
            ("is-within", "0:00"),
        ],
        [
            ("is-within", "-00:00"),
            ("is-over", "+123:45"),
            ("is-within", "0:00"),
        ],
    ];
    for (row, values) in rows.iter().zip(expected) {
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
    let start = chrono::NaiveDate::from_ymd_opt(2026, 11, 25).unwrap();
    let rows = (0..14)
        .map(|offset| BandDay {
            logical_date: (start + chrono::Duration::days(offset)).to_string(),
            accumulated_rho_diff_seconds: 0,
            accumulated_free_diff_seconds: 0,
            durations: BandDurations {
                unavailable_seconds: 8 * 3600,
                elapsed_seconds: 4 * 3600 * i64::from(offset == 0),
                repetitive_seconds: 5 * 3600,
                non_repetitive_seconds: 2 * 3600,
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
fn 日別負荷は月境界を跨ぐ14日を表示し最終日を選択できる() {
    let selected = Arc::default();
    let mut dom = VirtualDom::new_with_props(
        fourteen_day_root,
        Props {
            selected: Arc::clone(&selected),
        },
    );
    let listeners = rebuild_with_click_listeners(&mut dom);
    let html = dioxus::ssr::render(&dom);
    assert!(html.contains("今日から14日の負荷"), "{html}");
    assert_eq!(html.matches("<button class=\"load-day").count(), 14);
    assert!(html.contains("<span>11/25</span><small>水</small>"));
    assert!(html.contains("<span>12/8</span><small>火</small>"));
    assert!(!html.contains("14日間"));
    assert!(!html.contains('〜'));
    assert_eq!(listeners.len(), 17);
    dispatch_click(&dom, *listeners.last().unwrap());
    assert_eq!(*selected.lock().unwrap(), ["2026-12-08"]);
}
