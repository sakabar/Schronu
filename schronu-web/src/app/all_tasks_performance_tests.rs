//! 「全て」一覧のclient側性能を手動で測るignored test。
//!
//! 契約testとは分離し、release buildで明示的に指定した場合だけ実行する。

use super::list_view::{AllTasksViewStatus, ListView};
use crate::client::view_projection::{
    project_visible_all_task_rows, task_name_matches, ListRowViewModel,
};
use crate::{AllTaskRow, SessionTask};
use dioxus::prelude::*;
use std::time::{Duration, Instant};

const SAMPLE_COUNT: usize = 3;
const FIXTURE_SIZES: [usize; 2] = [501, 20_000];

#[derive(Clone, PartialEq, Props)]
struct RenderHarnessProps {
    rows: Vec<ListRowViewModel>,
}

fn render_harness(props: RenderHarnessProps) -> Element {
    rsx! {
        ListView {
            dates: Vec::new(),
            rows: props.rows,
            active_task_ids: Vec::new(),
            date_input_text: String::new(),
            date_input_error: None,
            filter_text: String::new(),
            all_tasks_status: Some(AllTasksViewStatus::Loaded),
            visible_row_limit: Some(500),
            on_select_date: move |_| {},
            on_select_all_tasks: move |_| {},
            on_date_input_change: move |_| {},
            on_submit_date_input: move |_| {},
            on_start_session: move |_| {},
            on_filter_change: move |_| {},
        }
    }
}

#[test]
#[ignore = "manual release-mode performance measurement"]
fn measure_all_task_client_performance() {
    for segment_count in FIXTURE_SIZES {
        let rows = all_task_rows(segment_count);
        let (search_time, matched_rows) = median_sample(|| {
            rows.iter()
                .filter(|row| task_name_matches("  検索対象 task  ", &row.task.task_name))
                .count()
        });
        assert_eq!(matched_rows, segment_count.div_ceil(10));

        let (projection_time, projected) =
            median_sample(|| project_visible_all_task_rows(&rows, "  検索対象 task  ", 500));
        assert_eq!(projected.rows.len(), segment_count.div_ceil(10).min(500));
        assert_eq!(projected.has_more, matched_rows > 500);

        let (render_time, rendered_bytes) = median_sample(|| {
            let mut dom = VirtualDom::new_with_props(
                render_harness,
                RenderHarnessProps {
                    rows: projected.rows.clone(),
                },
            );
            dom.rebuild_in_place();
            dioxus::ssr::render(&dom).len()
        });

        println!(
            concat!(
                "all_tasks_client fixture_segments={} samples={} ",
                "search_ms={:.3} matched_rows={} projection_500_ms={:.3} ",
                "dioxus_render_500_ms={:.3} rendered_bytes={}"
            ),
            segment_count,
            SAMPLE_COUNT,
            milliseconds(search_time),
            matched_rows,
            milliseconds(projection_time),
            milliseconds(render_time),
            rendered_bytes,
        );
    }
}

fn all_task_rows(count: usize) -> Vec<AllTaskRow> {
    (0..count)
        .map(|index| AllTaskRow {
            task: SessionTask {
                task_id: format!("00000000-0000-4000-8000-{index:012x}"),
                task_name: if index % 10 == 0 {
                    format!("検索対象 TASK {index:05}")
                } else {
                    format!("通常タスク {index:05}")
                },
                estimated_work_seconds: 1_800,
                actual_work_seconds: 0,
            },
            segment_index: index,
            schedule_date: "2026-09-05".to_owned(),
            deadline_epoch_ms: None,
            deadline_label: "____-01:00".to_owned(),
            misses_deadline: false,
            is_leaf: index % 7 != 0,
        })
        .collect()
}

fn median_sample<T>(mut operation: impl FnMut() -> T) -> (Duration, T) {
    let mut samples = Vec::with_capacity(SAMPLE_COUNT);
    let mut last_output = None;
    for _ in 0..SAMPLE_COUNT {
        let started = Instant::now();
        last_output = Some(operation());
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    (
        samples[samples.len() / 2],
        last_output.expect("sample count is positive"),
    )
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}
