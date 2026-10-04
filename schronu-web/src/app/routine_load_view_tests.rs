use super::routine_load_view::{
    build_routine_load_projection, highlighted_dates, rows_for_scope, toggle_routine_selection,
    RoutineLoadScope, RoutineLoadTable,
};
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
