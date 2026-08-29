use super::schedule_use_case::get_schedule;
use super::task_use_case::get_task;
use crate::entity::task::{Status, TaskHandle};
use crate::test_support::TestTaskRepository;
use chrono::{DateTime, Duration, FixedOffset, Local, NaiveDate, TimeZone};
use uuid::Uuid;

fn is_externally_visible_function_declaration(source_line: &str) -> bool {
    let line = source_line.trim_start();
    let declaration = if let Some(declaration) = line.strip_prefix("pub ") {
        declaration
    } else if let Some(scoped_visibility) = line.strip_prefix("pub(") {
        let Some((_, declaration)) = scoped_visibility.split_once(") ") else {
            return false;
        };
        declaration
    } else {
        return false;
    };

    declaration.starts_with("fn ") || declaration.contains(" fn ")
}

fn fixed_now() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap()
}

#[test]
fn get_scheduleは借用競合をtask_tree_errorとして返す() {
    let task = crate::test_support::new_task_handle("借用競合").unwrap();
    let repository = TestTaskRepository::new(vec![task.clone()], fixed_now());

    let actual = task.with_exclusive_data_borrow_for_test(|| get_schedule(&repository));

    assert_eq!(
        actual,
        Err(super::task_use_case::ApplicationError::TaskTree(
            crate::entity::task::TaskTreeError::Borrow
        ))
    );
}

fn task_with_schedule(
    name: &str,
    start: DateTime<Local>,
    work_seconds: i64,
    priority: i64,
) -> TaskHandle {
    let task = crate::test_support::new_task_handle(name).unwrap();
    task.sync_clock(start).unwrap();
    task.set_start_time(start).unwrap();
    task.set_estimated_work_seconds(work_seconds).unwrap();
    task.set_priority(priority).unwrap();
    task
}

#[test]
fn get_schedule_締切を優先しdoneを除外してtask_viewを返す() {
    let now = fixed_now();
    let deadline_task = task_with_schedule("締切あり", now, 15 * 60, 1);
    deadline_task
        .set_deadline_time_opt(Some(now + Duration::hours(2)))
        .unwrap();
    let high_priority_task = task_with_schedule("高優先度", now, 30 * 60, 99);
    let done_task = task_with_schedule("完了済み", now, 10 * 60, 100);
    done_task.set_orig_status(Status::Done).unwrap();
    let repository = TestTaskRepository::new(
        vec![
            high_priority_task.clone(),
            done_task.clone(),
            deadline_task.clone(),
        ],
        now,
    );
    let views_before = repository
        .projects()
        .iter()
        .map(|task| {
            get_task(&repository, task.get_id().unwrap())
                .unwrap()
                .unwrap()
        })
        .collect::<Vec<_>>();

    let actual = get_schedule(&repository).unwrap();

    assert_eq!(actual.len(), 2);
    assert_eq!(actual[0].task.id, deadline_task.get_id().unwrap());
    assert_eq!(actual[0].task.name, "締切あり");
    assert_eq!(actual[0].scheduled_start, now);
    assert_eq!(actual[0].scheduled_end, now + Duration::minutes(15));
    assert_eq!(actual[0].scheduled_work_seconds, 15 * 60);
    assert_eq!(actual[0].total_work_seconds, 15 * 60);
    assert_eq!(actual[0].rank, 0);
    assert_eq!(actual[1].task.id, high_priority_task.get_id().unwrap());
    assert_eq!(actual[1].scheduled_start, now + Duration::minutes(15));
    assert!(!actual
        .iter()
        .any(|entry| entry.task.id == done_task.get_id().unwrap()));
    assert_eq!(repository.save_count(), 0);
    assert_eq!(
        repository
            .projects()
            .iter()
            .map(|task| get_task(&repository, task.get_id().unwrap())
                .unwrap()
                .unwrap())
            .collect::<Vec<_>>(),
        views_before
    );
}

#[test]
fn get_schedule_i64最小値付近でも優先度の高いtaskを先に配置する() {
    let now = fixed_now();
    let lowest = task_with_schedule("最低", now, 15 * 60, i64::MIN);
    let next = task_with_schedule("次", now, 15 * 60, i64::MIN + 1);
    let repository = TestTaskRepository::new(vec![lowest.clone(), next.clone()], now);

    let actual = get_schedule(&repository).unwrap();

    assert_eq!(actual[0].task.id, next.get_id().unwrap());
    assert_eq!(actual[1].task.id, lowest.get_id().unwrap());
}

#[test]
fn get_scheduleは候補の次論理日開始計算不能を伝搬しtaskを変更しない() {
    let local_datetime = NaiveDate::MAX.and_hms_opt(6, 0, 0).unwrap();
    let out_of_range_start = DateTime::<Local>::from_naive_utc_and_offset(
        local_datetime,
        FixedOffset::east_opt(0).unwrap(),
    );
    let first = task_with_schedule("範囲外1", out_of_range_start, 15 * 60, 1);
    let second = task_with_schedule("範囲外2", out_of_range_start, 15 * 60, 2);
    let repository =
        TestTaskRepository::new(vec![first.clone(), second.clone()], out_of_range_start);
    let original_views = repository
        .projects()
        .iter()
        .map(|task| {
            get_task(&repository, task.get_id().unwrap())
                .unwrap()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let original_revisions = [
        first.get_persistent_mutation_revision().unwrap(),
        second.get_persistent_mutation_revision().unwrap(),
    ];

    let actual = get_schedule(&repository);

    assert_eq!(
        actual,
        Err(
            super::task_use_case::ApplicationError::LogicalDateOutOfRange {
                operation: "next_logical_date_start",
                datetime: out_of_range_start,
            }
        )
    );
    assert_eq!(
        repository
            .projects()
            .iter()
            .map(|task| get_task(&repository, task.get_id().unwrap())
                .unwrap()
                .unwrap())
            .collect::<Vec<_>>(),
        original_views
    );
    assert_eq!(
        [
            first.get_persistent_mutation_revision().unwrap(),
            second.get_persistent_mutation_revision().unwrap(),
        ],
        original_revisions
    );
    assert_eq!(repository.save_count(), 0);
}

#[test]
fn get_scheduleは複数の日時errorからuuid順先頭候補を決定的に返す() {
    let lower_id_datetime = DateTime::<Local>::from_naive_utc_and_offset(
        NaiveDate::MAX.and_hms_opt(6, 0, 0).unwrap(),
        FixedOffset::east_opt(0).unwrap(),
    );
    let higher_id_datetime = DateTime::<Local>::from_naive_utc_and_offset(
        NaiveDate::MAX.and_hms_opt(7, 0, 0).unwrap(),
        FixedOffset::east_opt(0).unwrap(),
    );
    let lower_id = Uuid::from_u128(1);
    let higher_id = Uuid::from_u128(2);
    let lower_id_task = TaskHandle::with_identity("低UUID", lower_id, fixed_now()).unwrap();
    lower_id_task.sync_clock(lower_id_datetime).unwrap();
    lower_id_task.set_start_time(lower_id_datetime).unwrap();
    lower_id_task.set_estimated_work_seconds(15 * 60).unwrap();
    let higher_id_task = TaskHandle::with_identity("高UUID", higher_id, fixed_now()).unwrap();
    higher_id_task.sync_clock(higher_id_datetime).unwrap();
    higher_id_task.set_start_time(higher_id_datetime).unwrap();
    higher_id_task.set_estimated_work_seconds(15 * 60).unwrap();
    let repository = TestTaskRepository::new(vec![higher_id_task, lower_id_task], fixed_now());
    let expected = Err(
        super::task_use_case::ApplicationError::LogicalDateOutOfRange {
            operation: "next_logical_date_start",
            datetime: lower_id_datetime,
        },
    );

    for _ in 0..64 {
        assert_eq!(get_schedule(&repository), expected);
    }
}

#[test]
fn get_schedule_pending解除後に子を配置し親をその後へ置く() {
    let now = fixed_now();
    let parent = task_with_schedule("親", now, 0, 5);
    let mut child_attr = crate::test_support::new_task_attr("子");
    child_attr.set_estimated_work_seconds(15 * 60);
    child_attr.set_start_time(now);
    let child = parent.create_as_last_child(child_attr);
    child.sync_clock(now).unwrap();
    child.set_pending_until(now + Duration::hours(2)).unwrap();
    child.set_orig_status(Status::Pending).unwrap();
    let repository = TestTaskRepository::new(vec![parent.clone()], now);

    let actual = get_schedule(&repository).unwrap();
    let child_schedule = actual
        .iter()
        .find(|entry| entry.task.id == child.get_id().unwrap())
        .unwrap();
    let parent_schedule = actual
        .iter()
        .find(|entry| entry.task.id == parent.get_id().unwrap())
        .unwrap();

    assert_eq!(
        child_schedule.first_available_time,
        now + Duration::hours(2)
    );
    assert_eq!(child_schedule.scheduled_start, now + Duration::hours(2));
    assert_eq!(
        parent_schedule.scheduled_start,
        child_schedule.scheduled_end
    );
    assert_eq!(parent_schedule.rank, 1);
}

#[test]
fn get_schedule_非atomic_taskを未来の高優先度taskの前後へ分割する() {
    let now = fixed_now();
    let low_priority = task_with_schedule("低優先度", now + Duration::hours(1), 10 * 3600, 88);
    let high_priority = task_with_schedule("高優先度", now + Duration::hours(6), 3600, 89);
    let repository =
        TestTaskRepository::new(vec![low_priority.clone(), high_priority.clone()], now);

    let actual = get_schedule(&repository).unwrap();
    let low_segments = actual
        .iter()
        .filter(|entry| entry.task.id == low_priority.get_id().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(low_segments.len(), 2);
    assert_eq!(low_segments[0].scheduled_start, now + Duration::hours(1));
    assert_eq!(low_segments[0].scheduled_end, now + Duration::hours(6));
    assert_eq!(low_segments[1].scheduled_start, now + Duration::hours(7));
    assert_eq!(low_segments[1].scheduled_end, now + Duration::hours(12));
    assert_eq!(
        low_segments
            .iter()
            .map(|entry| entry.scheduled_work_seconds)
            .sum::<i64>(),
        10 * 3600
    );
}

#[test]
fn get_schedule_atomic_taskを分割せず連続枠へ配置する() {
    let now = fixed_now();
    let atomic_task = task_with_schedule("atomic", now + Duration::hours(1), 10 * 3600, 88);
    atomic_task.set_atomic(true).unwrap();
    let high_priority = task_with_schedule("高優先度", now + Duration::hours(6), 3600, 89);
    let repository = TestTaskRepository::new(vec![atomic_task.clone(), high_priority], now);

    let actual = get_schedule(&repository).unwrap();
    let atomic_segments = actual
        .iter()
        .filter(|entry| entry.task.id == atomic_task.get_id().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(atomic_segments.len(), 1);
    assert_eq!(atomic_segments[0].scheduled_start, now + Duration::hours(7));
    assert_eq!(atomic_segments[0].scheduled_end, now + Duration::hours(17));
}

#[test]
fn get_scheduleはfixed_start属性をpolicyへ渡し指定開始を保持する() {
    let now = fixed_now();
    let flexible = task_with_schedule("flexible", now, 2 * 60 * 60, 99);
    let fixed_start = now + Duration::hours(1);
    let fixed = task_with_schedule("fixed", fixed_start, 60 * 60, 0);
    fixed.set_fixed_start(true).unwrap();
    let repository = TestTaskRepository::new(vec![flexible, fixed.clone()], now);

    let scheduled = get_schedule(&repository).unwrap();
    let fixed_segment = scheduled
        .iter()
        .find(|segment| segment.task.id == fixed.get_id().unwrap())
        .unwrap();

    assert_eq!(fixed_segment.scheduled_start, fixed_start);
    assert_eq!(
        fixed_segment.scheduled_end,
        fixed_start + Duration::hours(1)
    );
}

#[test]
fn get_scheduleは表現不能なfixed_flexible_atomic終了時刻を構造化errorにする() {
    let now = fixed_now();

    for (name, fixed_start, atomic) in [
        ("fixed", true, false),
        ("flexible", false, false),
        ("atomic", false, true),
    ] {
        let task = task_with_schedule(name, now, i64::MAX, 0);
        task.set_fixed_start(fixed_start).unwrap();
        task.set_atomic(atomic).unwrap();
        let task_id = task.get_id().unwrap();
        let repository = TestTaskRepository::new(vec![task], now);

        assert_eq!(
            get_schedule(&repository),
            Err(super::task_use_case::ApplicationError::ScheduleTimeOutOfRange {
                task_id,
                start_time: now,
                work_seconds: i64::MAX,
            }),
            "{name} must report an out-of-range schedule segment"
        );
    }
}

#[test]
fn get_scheduleはpending中fixedの日時範囲errorに指定開始を保持する() {
    let now = fixed_now();
    let fixed_start = now + Duration::hours(1);
    let task = task_with_schedule("pending-fixed", fixed_start, i64::MAX, 0);
    task.set_fixed_start(true).unwrap();
    task.set_pending_until(fixed_start + Duration::hours(4))
        .unwrap();
    task.set_orig_status(Status::Pending).unwrap();
    let task_id = task.get_id().unwrap();
    let repository = TestTaskRepository::new(vec![task], now);

    assert_eq!(
        get_schedule(&repository),
        Err(super::task_use_case::ApplicationError::ScheduleTimeOutOfRange {
            task_id,
            start_time: fixed_start,
            work_seconds: i64::MAX,
        })
    );
}

#[test]
fn get_scheduleはdependencyを持つfixedの日時範囲errorに指定開始を保持する() {
    let now = fixed_now();
    let fixed_start = now + Duration::hours(1);
    let parent = task_with_schedule("fixed-parent", fixed_start, i64::MAX, 0);
    parent.set_fixed_start(true).unwrap();
    let parent_id = parent.get_id().unwrap();
    let mut child_attr = crate::test_support::new_task_attr("dependency");
    child_attr.set_start_time(now);
    child_attr.set_estimated_work_seconds(2 * 60 * 60);
    parent.create_as_last_child(child_attr);
    let repository = TestTaskRepository::new(vec![parent], now);

    assert_eq!(
        get_schedule(&repository),
        Err(super::task_use_case::ApplicationError::ScheduleTimeOutOfRange {
            task_id: parent_id,
            start_time: fixed_start,
            work_seconds: i64::MAX,
        })
    );
}

#[test]
fn scheduling_policyの単一入口と選択責務を固定する() {
    let policy_source = include_str!("scheduling_policy.rs");
    let use_case_source = include_str!("schedule_use_case.rs");
    let externally_visible_policy_functions = policy_source
        .lines()
        .map(str::trim_start)
        .filter(|line| is_externally_visible_function_declaration(line))
        .collect::<Vec<_>>();
    let candidate_builder = use_case_source
        .split_once("fn build_schedule_candidates(")
        .expect("schedule use case must retain candidate construction")
        .1
        .split_once("fn calculate_remaining_work_seconds(")
        .expect("candidate construction must remain bounded by remaining work calculation")
        .0;

    assert_eq!(
        externally_visible_policy_functions,
        ["pub(super) fn schedule_tasks_by_priority_with_metrics("],
        "scheduling policy must expose only its minimum-visibility production entrypoint"
    );
    assert!(
        !policy_source.lines().map(str::trim_start).any(|line| {
            (line.starts_with("pub use ")
                || line.starts_with("pub(crate) use ")
                || line.starts_with("pub(super) use ")
                || line.starts_with("pub(in "))
                && line.contains(" use ")
        }),
        "scheduling policy must not expose a second entrypoint through a re-export"
    );
    assert!(
        policy_source.contains("pub(super) fn schedule_tasks_by_priority_with_metrics("),
        "scheduling policy must retain its single production entrypoint"
    );
    assert!(
        use_case_source.contains("schedule_tasks_by_priority_with_metrics(&candidates"),
        "schedule use case must delegate placement to the scheduling policy entrypoint"
    );
    assert!(
        candidate_builder.contains("attributes.sort_by_key(|(id, _)| *id);"),
        "candidate construction must retain deterministic UUID error selection"
    );
    assert_eq!(
        use_case_source.matches(".sort_by").count(),
        1,
        "schedule use case must not add a selection-order sort"
    );
    for forbidden_selection_marker in [
        "let sort_key",
        "sort_by(|",
        "deadline_time.is_none()",
        "Reverse(",
        "!task.get_priority()",
    ] {
        assert!(
            !use_case_source.contains(forbidden_selection_marker),
            "schedule use case must not own scheduling selection marker: {forbidden_selection_marker}"
        );
    }
}

#[test]
fn policy入口検出はvisibilityとfunction修飾を独立に扱う() {
    for declaration in [
        "pub fn direct() {}",
        "pub async fn asynchronous() {}",
        "pub const fn constant() {}",
        "pub unsafe fn unsafe_entry() {}",
        "pub extern \"C\" fn external() {}",
        "pub(crate) async fn crate_asynchronous() {}",
        "pub(super) const fn parent_constant() {}",
        "pub(in crate::application) unsafe fn scoped_unsafe() {}",
    ] {
        assert!(
            is_externally_visible_function_declaration(declaration),
            "externally visible function declaration must be detected: {declaration}"
        );
    }

    for declaration in [
        "fn private() {}",
        "async fn private_asynchronous() {}",
        "pub struct NotAFunction;",
        "pub(crate) const NOT_A_FUNCTION: i64 = 0;",
    ] {
        assert!(
            !is_externally_visible_function_declaration(declaration),
            "non-entrypoint declaration must not be detected: {declaration}"
        );
    }
}
