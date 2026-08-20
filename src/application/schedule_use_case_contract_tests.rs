use super::interface::{TaskRepositoryError, TaskRepositoryTrait};
use super::schedule_use_case::get_schedule;
use super::task_use_case::get_task;
use crate::entity::task::{Status, TaskHandle};
use chrono::{DateTime, Duration, FixedOffset, Local, NaiveDate, TimeZone};
use std::cell::Cell;
use uuid::Uuid;

struct TestTaskRepository {
    projects: Vec<TaskHandle>,
    now: DateTime<Local>,
    save_count: Cell<usize>,
}

impl TestTaskRepository {
    fn new(projects: Vec<TaskHandle>, now: DateTime<Local>) -> Self {
        Self {
            projects,
            now,
            save_count: Cell::new(0),
        }
    }
}

impl TaskRepositoryTrait for TestTaskRepository {
    fn get_project_storage_dir_name(&self) -> &str {
        "unused"
    }

    fn get_all_projects(&self) -> Vec<&TaskHandle> {
        self.projects.iter().collect()
    }

    fn load(&mut self) -> Result<(), TaskRepositoryError> {
        Ok(())
    }

    fn save(&self) -> Result<(), TaskRepositoryError> {
        self.save_count.set(self.save_count.get() + 1);
        Ok(())
    }

    fn sync_clock(
        &mut self,
        now: DateTime<Local>,
    ) -> Result<(), crate::entity::task::TaskTreeError> {
        self.now = now;
        Ok(())
    }

    fn get_last_synced_time(&self) -> DateTime<Local> {
        self.now
    }

    fn get_highest_priority_project(&mut self) -> Option<&TaskHandle> {
        self.projects.first()
    }

    fn get_highest_priority_leaf_task_id(
        &mut self,
    ) -> Result<Option<Uuid>, crate::entity::task::TaskTreeError> {
        Ok(None)
    }

    fn get_defer_candidate_leaf_task_id(
        &mut self,
        _recent_days: i64,
    ) -> Result<Option<Uuid>, crate::entity::task::TaskTreeError> {
        Ok(None)
    }

    fn get_by_id(
        &self,
        id: Uuid,
    ) -> Result<Option<TaskHandle>, crate::entity::task::TaskTreeError> {
        for task in &self.projects {
            if let Some(found) = task.get_by_id(id)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    fn start_new_project(
        &mut self,
        root_task: TaskHandle,
    ) -> Result<(), crate::entity::task::TaskTreeError> {
        self.projects.push(root_task);
        Ok(())
    }
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
        .projects
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
    assert_eq!(repository.save_count.get(), 0);
    assert_eq!(
        repository
            .projects
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
fn get_scheduleは候補の次業務日開始計算不能を伝搬しtaskを変更しない() {
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
        .projects
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
            super::task_use_case::ApplicationError::SubjectiveDateOutOfRange {
                operation: "next_business_day_start",
                datetime: out_of_range_start,
            }
        )
    );
    assert_eq!(
        repository
            .projects
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
    assert_eq!(repository.save_count.get(), 0);
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
