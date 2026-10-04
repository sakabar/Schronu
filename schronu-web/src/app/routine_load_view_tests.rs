use super::routine_load_view::{
    build_routine_load_projection, highlighted_dates, rows_for_scope, toggle_routine_selection,
    RoutineLoadScope, RoutineLoadTable,
};
use super::view_test_support::{dispatch_click, rebuild_with_click_listeners};
use crate::{RoutineLoadReport, RoutineLoadRow};
use chrono::{Duration, NaiveDate};
use dioxus::prelude::*;
use std::collections::BTreeMap;

fn date(value: &str) -> NaiveDate {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
}

fn report(rows: Vec<RoutineLoadRow>) -> RoutineLoadReport {
    let start = date("2026-10-03");
    let full_day_available_seconds_by_date = (0..28)
        .map(|offset| ((start + Duration::days(offset)).to_string(), 4 * 60 * 60))
        .collect();
    RoutineLoadReport {
        start_date: start.to_string(),
        end_date: (start + Duration::days(27)).to_string(),
        horizon_day_count: 28,
        full_day_available_seconds_by_date,
        rows,
    }
}

fn row(
    project_name: &str,
    routine_name: &str,
    routine_task_id: &str,
    work: &[(&str, i64)],
) -> RoutineLoadRow {
    let work_seconds_by_date = work
        .iter()
        .map(|(date, seconds)| ((*date).to_owned(), *seconds))
        .collect::<BTreeMap<_, _>>();
    let total_work_seconds = work_seconds_by_date.values().sum();
    RoutineLoadRow {
        project_task_id: format!("project-{project_name}"),
        project_name: project_name.to_owned(),
        routine_task_id: routine_task_id.to_owned(),
        routine_name: routine_name.to_owned(),
        repetition_interval_days: 3,
        total_work_seconds,
        occurrence_day_count: work_seconds_by_date.len(),
        average_work_seconds: Some(total_work_seconds / work_seconds_by_date.len() as i64),
        peak_date: work[0].0.to_owned(),
        peak_work_seconds: work.iter().map(|(_, seconds)| *seconds).max().unwrap(),
        work_seconds_by_date,
    }
}

#[test]
fn 投影は月曜始まりの空cellと28日を配置する() {
    let projection = build_routine_load_projection(&report(Vec::new())).unwrap();

    assert_eq!(projection.calendar_cells.len(), 35);
    assert!(projection.calendar_cells[..5].iter().all(Option::is_none));
    assert_eq!(
        projection.calendar_cells[5].as_ref().unwrap().date,
        date("2026-10-03")
    );
    assert_eq!(
        projection.calendar_cells[32].as_ref().unwrap().date,
        date("2026-10-30")
    );
    assert!(projection.calendar_cells[33..].iter().all(Option::is_none));
}

#[test]
fn 曜日投影は4日平均と実発生日数と全日容量比を返す() {
    let report = report(vec![row(
        "健康",
        "三日周期",
        "routine-1",
        &[("2026-10-06", 7_200), ("2026-10-09", 3_600)],
    )]);
    let projection = build_routine_load_projection(&report).unwrap();
    let tuesday = &projection.weekdays[1];
    let rows = rows_for_scope(&projection, RoutineLoadScope::Weekday(1));

    assert_eq!(tuesday.work_seconds, 7_200);
    assert_eq!(tuesday.available_seconds, 4 * 4 * 60 * 60);
    assert_eq!(tuesday.percentage, Some(13));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].work_seconds, 7_200);
    assert_eq!(rows[0].average_work_seconds, Some(1_800));
    assert_eq!(rows[0].percentage, Some(13));
    assert_eq!(rows[0].occurrence_count, 1);
}

#[test]
fn 割合は0容量を欠損とし100超を切り捨てず色だけ飽和する() {
    let mut report = report(vec![row(
        "健康",
        "長い繰返",
        "routine-1",
        &[("2026-10-03", 18_000), ("2026-10-04", 600)],
    )]);
    report
        .full_day_available_seconds_by_date
        .insert("2026-10-04".to_owned(), 0);
    let projection = build_routine_load_projection(&report).unwrap();

    assert_eq!(projection.days[0].percentage, Some(125));
    assert_eq!(projection.days[0].heat_percentage, 100);
    assert_eq!(projection.days[1].work_seconds, 600);
    assert_eq!(projection.days[1].percentage, None);
    assert_eq!(projection.days[1].heat_percentage, 0);
}

#[test]
fn 作業0秒の発生日を含む繰返でも拡張表示する() {
    let report = report(vec![row(
        "休息",
        "休日にダラダラする時間",
        "routine-zero",
        &[("2026-10-03", 0), ("2026-10-10", 0)],
    )]);

    let projection = build_routine_load_projection(&report).unwrap();

    assert_eq!(projection.days[0].work_seconds, 0);
    assert_eq!(
        highlighted_dates(&projection, Some("routine-zero")),
        [date("2026-10-03"), date("2026-10-10")]
            .into_iter()
            .collect()
    );
    let rows = rows_for_scope(
        &projection,
        RoutineLoadScope::Date(date("2026-10-03")),
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].work_seconds, 0);
}

#[test]
fn 範囲行は作業秒数降順とproject繰返uuid順に並ぶ() {
    let report = report(vec![
        row("B", "A", "routine-2", &[("2026-10-03", 600)]),
        row("A", "B", "routine-3", &[("2026-10-03", 600)]),
        row("A", "A", "routine-2", &[("2026-10-03", 600)]),
        row("A", "A", "routine-1", &[("2026-10-03", 600)]),
        row("Z", "Z", "routine-4", &[("2026-10-03", 1_200)]),
    ]);
    let projection = build_routine_load_projection(&report).unwrap();
    let rows = rows_for_scope(&projection, RoutineLoadScope::Date(date("2026-10-03")));

    assert_eq!(
        rows.iter()
            .map(|row| row.routine.routine_task_id.as_str())
            .collect::<Vec<_>>(),
        [
            "routine-4",
            "routine-1",
            "routine-2",
            "routine-3",
            "routine-2"
        ]
    );
}

#[test]
fn 選択再押下と全体切替は強調対象を解除する() {
    let projection = build_routine_load_projection(&report(vec![row(
        "健康",
        "運動",
        "routine-1",
        &[("2026-10-03", 600), ("2026-10-06", 600)],
    )]))
    .unwrap();

    assert_eq!(
        toggle_routine_selection(None, "routine-1"),
        Some("routine-1".to_owned())
    );
    assert_eq!(
        toggle_routine_selection(Some("routine-1"), "routine-1"),
        None
    );
    assert_eq!(
        highlighted_dates(&projection, Some("routine-1")),
        [date("2026-10-03"), date("2026-10-06")]
            .into_iter()
            .collect()
    );
    assert!(highlighted_dates(&projection, None).is_empty());
}

#[test]
fn 旧payloadはmapと範囲選択を出さず全体表へfallbackする() {
    let legacy = RoutineLoadReport {
        start_date: "2026-10-03".to_owned(),
        end_date: "2026-10-10".to_owned(),
        horizon_day_count: 8,
        full_day_available_seconds_by_date: BTreeMap::new(),
        rows: vec![row("健康", "運動", "routine-1", &[("2026-10-03", 600)])],
    };
    assert!(build_routine_load_projection(&legacy).is_none());

    fn root() -> Element {
        rsx! { RoutineLoadTable { report: Some(legacy_report()), loading: false } }
    }
    fn legacy_report() -> RoutineLoadReport {
        RoutineLoadReport {
            start_date: "2026-10-03".to_owned(),
            end_date: "2026-10-10".to_owned(),
            horizon_day_count: 8,
            full_day_available_seconds_by_date: BTreeMap::new(),
            rows: vec![row("健康", "運動", "routine-1", &[("2026-10-03", 600)])],
        }
    }
    let mut dom = VirtualDom::new(root);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);
    assert!(html.contains("8日合計"), "{html}");
    assert!(!html.contains("routine-load-map"), "{html}");
    assert!(!html.contains("範囲選択"), "{html}");
}

#[test]
fn 不正な日付内訳と集計overflowは拡張表示を行わない() {
    let base_row = row("健康", "運動", "routine-1", &[("2026-10-03", 600)]);

    let mut invalid_start = report(vec![base_row.clone()]);
    invalid_start.start_date = "2026-10-xx".to_owned();

    let mut missing_capacity = report(vec![base_row.clone()]);
    missing_capacity
        .full_day_available_seconds_by_date
        .remove("2026-10-03");

    let mut out_of_range = report(vec![base_row.clone()]);
    out_of_range.rows[0].work_seconds_by_date = BTreeMap::from([("2026-10-31".to_owned(), 600)]);

    let mut negative = report(vec![base_row]);
    negative.rows[0].work_seconds_by_date = BTreeMap::from([("2026-10-03".to_owned(), -1)]);
    negative.rows[0].total_work_seconds = -1;

    let daily_overflow = report(vec![
        row("A", "最大", "routine-max", &[("2026-10-03", i64::MAX)]),
        row("B", "追加", "routine-extra", &[("2026-10-03", 1)]),
    ]);

    let mut weekday_capacity_overflow = report(Vec::new());
    for seconds in weekday_capacity_overflow
        .full_day_available_seconds_by_date
        .values_mut()
    {
        *seconds = i64::MAX;
    }

    for invalid in [
        invalid_start,
        missing_capacity,
        out_of_range,
        negative,
        daily_overflow,
        weekday_capacity_overflow,
    ] {
        assert!(build_routine_load_projection(&invalid).is_none());
    }
}

#[test]
fn 新payloadはmapと全体表と操作可能なaria情報を初期表示する() {
    fn root() -> Element {
        rsx! {
            RoutineLoadTable {
                report: Some(report(vec![row(
                    "健康",
                    "運動",
                    "routine-1",
                    &[("2026-10-03", 3_600)],
                )])),
                loading: false,
            }
        }
    }
    let mut dom = VirtualDom::new(root);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);

    assert!(html.contains("class=\"routine-load-map\""), "{html}");
    assert!(html.contains("aria-label=\"範囲選択\""), "{html}");
    assert!(html.contains("aria-pressed=true>全体"), "{html}");
    assert!(html.contains("28日合計"), "{html}");
    assert!(html.contains("2026年10月3日"), "{html}");
    assert!(html.contains("繰返時間 01:00"), "{html}");
    assert!(html.contains("可処分時間比 25%"), "{html}");
}

#[test]
fn 曜日と日付の選択は列と行をlocalに切り替え行再押下で強調を解除する() {
    fn root() -> Element {
        rsx! {
            RoutineLoadTable {
                report: Some(report(vec![row(
                    "健康",
                    "三日周期",
                    "routine-1",
                    &[("2026-10-03", 3_600), ("2026-10-06", 7_200)],
                )])),
                loading: false,
            }
        }
    }
    let mut dom = VirtualDom::new(root);
    let click_ids = rebuild_with_click_listeners(&mut dom);
    assert_eq!(click_ids.len(), 37, "全体、曜日7、日付28、行1");

    let weekday_html = (0..click_ids.len())
        .find_map(|index| {
            let mut candidate = VirtualDom::new(root);
            let ids = rebuild_with_click_listeners(&mut candidate);
            dispatch_click(&candidate, ids[index]);
            candidate.render_immediate_to_vec();
            let html = dioxus::ssr::render(&candidate);
            (html.contains("曜日平均") && html.contains("00:30")).then_some(html)
        })
        .expect("火曜日buttonが曜日表へ切り替える");
    for heading in ["曜日平均", "可処分時間比", "発生"] {
        assert!(weekday_html.contains(heading), "{weekday_html}");
    }
    assert!(weekday_html.contains("00:30"), "{weekday_html}");
    assert!(weekday_html.contains("4回中1回"), "{weekday_html}");

    let date_html = (0..click_ids.len())
        .find_map(|index| {
            let mut candidate = VirtualDom::new(root);
            let ids = rebuild_with_click_listeners(&mut candidate);
            dispatch_click(&candidate, ids[index]);
            candidate.render_immediate_to_vec();
            let html = dioxus::ssr::render(&candidate);
            html.contains("当日時間").then_some(html)
        })
        .expect("日付buttonが当日表へ切り替える");
    for heading in ["当日時間", "可処分時間比", "28日合計"] {
        assert!(date_html.contains(heading), "{date_html}");
    }

    let (mut selected_dom, selected_id, selected_html) = (0..click_ids.len())
        .find_map(|index| {
            let mut candidate = VirtualDom::new(root);
            let ids = rebuild_with_click_listeners(&mut candidate);
            dispatch_click(&candidate, ids[index]);
            candidate.render_immediate_to_vec();
            let html = dioxus::ssr::render(&candidate);
            html.contains("routine-load-day is-emphasized")
                .then_some((candidate, ids[index], html))
        })
        .expect("繰返行buttonが発生日を強調する");
    assert!(selected_html.contains("routine-load-day is-emphasized"));
    assert!(selected_html.contains("三日周期を強調"));
    assert!(selected_html.contains("aria-pressed=true"));

    dispatch_click(&selected_dom, selected_id);
    selected_dom.render_immediate_to_vec();
    let cleared_html = dioxus::ssr::render(&selected_dom);
    assert!(!cleared_html.contains("routine-load-day is-emphasized"));
}

#[test]
fn 行選択後の全体曜日日付切替は日付強調を解除する() {
    fn root() -> Element {
        rsx! {
            RoutineLoadTable {
                report: Some(report(vec![row(
                    "健康",
                    "運動",
                    "routine-1",
                    &[("2026-10-03", 3_600), ("2026-10-06", 7_200)],
                )])),
                loading: false,
            }
        }
    }

    fn html_after_click(root: fn() -> Element, index: usize) -> String {
        let mut dom = VirtualDom::new(root);
        let ids = rebuild_with_click_listeners(&mut dom);
        dispatch_click(&dom, ids[index]);
        dom.render_immediate_to_vec();
        dioxus::ssr::render(&dom)
    }

    let mut initial_dom = VirtualDom::new(root);
    let ids = rebuild_with_click_listeners(&mut initial_dom);
    let initial_html = dioxus::ssr::render(&initial_dom);
    let row_index = (0..ids.len())
        .find(|index| html_after_click(root, *index).contains("routine-load-day is-emphasized"))
        .unwrap();
    let weekday_index = (0..ids.len())
        .find(|index| html_after_click(root, *index).contains("曜日平均"))
        .unwrap();
    let date_index = (0..ids.len())
        .find(|index| html_after_click(root, *index).contains("当日時間"))
        .unwrap();
    let overall_index = (0..ids.len())
        .find(|index| html_after_click(root, *index) == initial_html)
        .unwrap();

    for scope_index in [overall_index, weekday_index, date_index] {
        let mut dom = VirtualDom::new(root);
        let ids = rebuild_with_click_listeners(&mut dom);
        dispatch_click(&dom, ids[row_index]);
        dom.render_immediate_to_vec();
        assert!(dioxus::ssr::render(&dom).contains("routine-load-day is-emphasized"));

        dispatch_click(&dom, ids[scope_index]);
        dom.render_immediate_to_vec();
        assert!(!dioxus::ssr::render(&dom).contains("routine-load-day is-emphasized"));
    }
}
