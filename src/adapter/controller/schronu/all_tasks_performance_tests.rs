//! 「全て」一覧のserver側性能を手動で測るignored test。
//!
//! 契約testとは分離し、release buildで明示的に指定した場合だけ実行する。

use super::{build_all_task_rows, AllTaskPageDto, AllTaskRowDto, ServerSnapshot, WebSuccess};
use crate::application::schedule_use_case::get_schedule;
use crate::entity::task::TaskHandle;
use crate::test_support::TestTaskRepository;
use chrono::{Duration, Local, TimeZone};
use std::time::{Duration as StdDuration, Instant};
use uuid::Uuid;

const SAMPLE_COUNT: usize = 3;
const FIXTURE_SIZES: [usize; 2] = [501, 20_000];

#[test]
#[ignore = "manual release-mode performance measurement"]
fn measure_all_task_server_performance() {
    for segment_count in FIXTURE_SIZES {
        let operation_now = Local.with_ymd_and_hms(2026, 9, 5, 6, 0, 0).unwrap();
        let repository = TestTaskRepository::new(
            fixed_segment_tasks(operation_now, segment_count),
            operation_now,
        );

        let (schedule_time, schedule) = median_sample(|| get_schedule(&repository).unwrap());
        assert_eq!(schedule.len(), segment_count);

        let (row_time, rows) = median_sample(|| {
            build_all_task_rows(&schedule, operation_now).expect("fixture rows are representable")
        });
        assert_eq!(rows.len(), segment_count);

        let (first_page_time, first_page) = median_sample_with_setup(
            || rows.clone(),
            |page_rows| {
                let mut store = super::all_tasks::AllTaskSnapshots::default();
                store.first_page(snapshot(operation_now), page_rows)
            },
        );
        assert_eq!(first_page.data.rows.len(), segment_count.min(500));

        let (all_pages_time, pages) = median_sample_with_setup(
            || rows.clone(),
            |page_rows| collect_pages(page_rows, operation_now),
        );
        assert_eq!(
            pages.iter().map(|page| page.data.rows.len()).sum::<usize>(),
            segment_count
        );

        let (serialization_time, serialized_bytes) = median_sample(|| {
            pages
                .iter()
                .map(|page| serde_json::to_vec(page).unwrap().len())
                .sum::<usize>()
        });

        println!(
            concat!(
                "all_tasks_server fixture_segments={} samples={} ",
                "schedule_ms={:.3} row_materialization_ms={:.3} first_page_ms={:.3} ",
                "all_pages_ms={:.3} serialization_ms={:.3} pages={} serialized_bytes={}"
            ),
            segment_count,
            SAMPLE_COUNT,
            milliseconds(schedule_time),
            milliseconds(row_time),
            milliseconds(first_page_time),
            milliseconds(all_pages_time),
            milliseconds(serialization_time),
            pages.len(),
            serialized_bytes,
        );
    }
}

fn fixed_segment_tasks(operation_now: chrono::DateTime<Local>, count: usize) -> Vec<TaskHandle> {
    (0..count)
        .map(|index| {
            let id = Uuid::from_u128(0xa11_7000_0000_0000 + index as u128);
            let task = TaskHandle::with_identity(
                &format!("all-task-performance-{index:05}"),
                id,
                operation_now,
            )
            .unwrap();
            task.set_estimated_work_seconds(60).unwrap();
            task.set_start_time(operation_now + Duration::minutes(index as i64 * 2))
                .unwrap();
            task.set_fixed_start(true).unwrap();
            task
        })
        .collect()
}

fn snapshot(operation_now: chrono::DateTime<Local>) -> ServerSnapshot {
    ServerSnapshot {
        observed_at_epoch_ms: operation_now.timestamp_millis(),
        logical_date: "2026-09-05".to_owned(),
        buffer_seconds: 0,
    }
}

fn collect_pages(
    rows: Vec<AllTaskRowDto>,
    operation_now: chrono::DateTime<Local>,
) -> Vec<WebSuccess<AllTaskPageDto>> {
    let mut store = super::all_tasks::AllTaskSnapshots::default();
    let mut pages = Vec::new();
    let mut page = store.first_page(snapshot(operation_now), rows);
    loop {
        let next_cursor = page.data.next_cursor.clone();
        pages.push(page);
        let Some(cursor) = next_cursor else {
            break;
        };
        page = store.next_page(&cursor).unwrap();
    }
    pages
}

fn median_sample<T>(mut operation: impl FnMut() -> T) -> (StdDuration, T) {
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

fn median_sample_with_setup<I, T>(
    mut setup: impl FnMut() -> I,
    mut operation: impl FnMut(I) -> T,
) -> (StdDuration, T) {
    let mut samples = Vec::with_capacity(SAMPLE_COUNT);
    let mut last_output = None;
    for _ in 0..SAMPLE_COUNT {
        let input = setup();
        let started = Instant::now();
        last_output = Some(operation(input));
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    (
        samples[samples.len() / 2],
        last_output.expect("sample count is positive"),
    )
}

fn milliseconds(duration: StdDuration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}
