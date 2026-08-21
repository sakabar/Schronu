use super::interface::{TaskRepositoryError, TaskRepositoryTrait};
use super::task_use_case::{
    list_tasks, ApplicationError, ListTasksFilter, TaskPeriodField, TaskPeriodFilter,
};
use crate::entity::task::{ProjectCategory, Status, TaskHandle};
use chrono::{DateTime, Duration, Local, TimeZone};
use uuid::Uuid;

struct TestTaskRepository {
    projects: Vec<TaskHandle>,
    now: DateTime<Local>,
}

impl TestTaskRepository {
    fn new(projects: Vec<TaskHandle>, now: DateTime<Local>) -> Self {
        Self { projects, now }
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
        _recent_threshold: DateTime<Local>,
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

fn no_filter() -> ListTasksFilter {
    ListTasksFilter {
        period: None,
        statuses: vec![],
        categories: vec![],
    }
}

#[test]
fn list_tasks_filterなしならdoneを含む全taskをpre_orderで返す() {
    let now = fixed_now();
    let first_root = crate::test_support::new_task_handle("root 1").unwrap();
    first_root.sync_clock(now).unwrap();
    let first_child =
        first_root.create_as_last_child(crate::test_support::new_task_attr("child 1"));
    let grandchild =
        first_child.create_as_last_child(crate::test_support::new_task_attr("grandchild"));
    let second_child =
        first_root.create_as_last_child(crate::test_support::new_task_attr("child 2"));
    second_child.set_orig_status(Status::Done).unwrap();
    let second_root = crate::test_support::new_task_handle("root 2").unwrap();
    second_root.sync_clock(now).unwrap();
    let repository = TestTaskRepository::new(vec![first_root.clone(), second_root.clone()], now);

    let actual = list_tasks(&repository, no_filter()).unwrap();

    assert_eq!(
        actual.iter().map(|task| task.id).collect::<Vec<_>>(),
        vec![
            first_root.get_id().unwrap(),
            first_child.get_id().unwrap(),
            grandchild.get_id().unwrap(),
            second_child.get_id().unwrap(),
            second_root.get_id().unwrap(),
        ]
    );
    assert_eq!(actual[3].status, Status::Done);
}

#[test]
fn list_tasks_statusは期限経過を反映した実効状態をorで絞る() {
    let now = fixed_now();
    let expired_pending = crate::test_support::new_task_handle("期限経過Pending").unwrap();
    expired_pending
        .set_start_time(now - Duration::hours(1))
        .unwrap();
    expired_pending
        .set_pending_until(now - Duration::minutes(1))
        .unwrap();
    expired_pending.set_orig_status(Status::Pending).unwrap();
    expired_pending.sync_clock(now).unwrap();
    let active_pending = crate::test_support::new_task_handle("Pending").unwrap();
    active_pending
        .set_start_time(now - Duration::hours(1))
        .unwrap();
    active_pending
        .set_pending_until(now + Duration::hours(1))
        .unwrap();
    active_pending.set_orig_status(Status::Pending).unwrap();
    active_pending.sync_clock(now).unwrap();
    let done = crate::test_support::new_task_handle("Done").unwrap();
    done.set_orig_status(Status::Done).unwrap();
    done.sync_clock(now).unwrap();
    let repository = TestTaskRepository::new(
        vec![
            expired_pending.clone(),
            active_pending.clone(),
            done.clone(),
        ],
        now,
    );
    let mut filter = no_filter();
    filter.statuses = vec![Status::Todo, Status::Done];

    let actual = list_tasks(&repository, filter).unwrap();

    assert_eq!(
        actual.iter().map(|task| task.id).collect::<Vec<_>>(),
        vec![expired_pending.get_id().unwrap(), done.get_id().unwrap()]
    );
    assert_eq!(actual[0].original_status, Status::Pending);
    assert_eq!(actual[0].status, Status::Todo);
}

#[test]
fn list_tasks_category内はorでstatusとはandで絞る() {
    let now = fixed_now();
    let investment = crate::test_support::new_task_handle("投資Todo").unwrap();
    investment
        .set_project_category_opt(Some(ProjectCategory::Investment))
        .unwrap();
    let uncategorized = crate::test_support::new_task_handle("未分類Todo").unwrap();
    let earning = crate::test_support::new_task_handle("獲得Todo").unwrap();
    earning
        .set_project_category_opt(Some(ProjectCategory::Earning))
        .unwrap();
    let done_investment = crate::test_support::new_task_handle("投資Done").unwrap();
    done_investment
        .set_project_category_opt(Some(ProjectCategory::Investment))
        .unwrap();
    done_investment.set_orig_status(Status::Done).unwrap();
    let repository = TestTaskRepository::new(
        vec![
            investment.clone(),
            uncategorized.clone(),
            earning,
            done_investment,
        ],
        now,
    );
    let mut filter = no_filter();
    filter.statuses = vec![Status::Todo];
    filter.categories = vec![Some(ProjectCategory::Investment), None];

    let actual = list_tasks(&repository, filter).unwrap();

    assert_eq!(
        actual.iter().map(|task| task.id).collect::<Vec<_>>(),
        vec![
            investment.get_id().unwrap(),
            uncategorized.get_id().unwrap()
        ]
    );
}

#[test]
fn list_tasks_period_status_categoryの全filterをandで絞る() {
    let now = fixed_now();
    let matched = crate::test_support::new_task_handle("matched").unwrap();
    matched
        .set_create_time(now + Duration::minutes(30))
        .unwrap();
    matched
        .set_project_category_opt(Some(ProjectCategory::Investment))
        .unwrap();
    let outside_period = crate::test_support::new_task_handle("outside period").unwrap();
    outside_period
        .set_create_time(now - Duration::minutes(1))
        .unwrap();
    outside_period
        .set_project_category_opt(Some(ProjectCategory::Investment))
        .unwrap();
    let done = crate::test_support::new_task_handle("done").unwrap();
    done.set_create_time(now + Duration::minutes(30)).unwrap();
    done.set_project_category_opt(Some(ProjectCategory::Investment))
        .unwrap();
    done.set_orig_status(Status::Done).unwrap();
    let earning = crate::test_support::new_task_handle("earning").unwrap();
    earning
        .set_create_time(now + Duration::minutes(30))
        .unwrap();
    earning
        .set_project_category_opt(Some(ProjectCategory::Earning))
        .unwrap();
    let repository =
        TestTaskRepository::new(vec![matched.clone(), outside_period, done, earning], now);

    let actual = list_tasks(
        &repository,
        ListTasksFilter {
            period: Some(TaskPeriodFilter {
                field: TaskPeriodField::CreatedAt,
                from: now,
                until: now + Duration::hours(1),
            }),
            statuses: vec![Status::Todo],
            categories: vec![Some(ProjectCategory::Investment)],
        },
    )
    .unwrap();

    assert_eq!(
        actual.iter().map(|task| task.id).collect::<Vec<_>>(),
        vec![matched.get_id().unwrap()]
    );
}

#[test]
fn list_tasks_created_deadline_completedの期間を半開区間で絞る() {
    let now = fixed_now();
    let before = crate::test_support::new_task_handle("before").unwrap();
    before.set_create_time(now - Duration::seconds(1)).unwrap();
    before
        .set_deadline_time_opt(Some(now - Duration::seconds(1)))
        .unwrap();
    before
        .set_end_time_opt(Some(now - Duration::seconds(1)))
        .unwrap();
    let from = crate::test_support::new_task_handle("from").unwrap();
    from.set_create_time(now).unwrap();
    from.set_deadline_time_opt(Some(now)).unwrap();
    from.set_end_time_opt(Some(now)).unwrap();
    let inside = crate::test_support::new_task_handle("inside").unwrap();
    inside.set_create_time(now + Duration::minutes(30)).unwrap();
    inside
        .set_deadline_time_opt(Some(now + Duration::minutes(30)))
        .unwrap();
    inside
        .set_end_time_opt(Some(now + Duration::minutes(30)))
        .unwrap();
    let until = crate::test_support::new_task_handle("until").unwrap();
    until.set_create_time(now + Duration::hours(1)).unwrap();
    until
        .set_deadline_time_opt(Some(now + Duration::hours(1)))
        .unwrap();
    until
        .set_end_time_opt(Some(now + Duration::hours(1)))
        .unwrap();
    let missing = crate::test_support::new_task_handle("missing").unwrap();
    let repository = TestTaskRepository::new(
        vec![before, from.clone(), inside.clone(), until, missing],
        now,
    );

    for field in [
        TaskPeriodField::CreatedAt,
        TaskPeriodField::Deadline,
        TaskPeriodField::CompletedAt,
    ] {
        let actual = list_tasks(
            &repository,
            ListTasksFilter {
                period: Some(TaskPeriodFilter {
                    field,
                    from: now,
                    until: now + Duration::hours(1),
                }),
                statuses: vec![],
                categories: vec![],
            },
        )
        .unwrap();

        assert_eq!(
            actual.iter().map(|task| task.id).collect::<Vec<_>>(),
            vec![from.get_id().unwrap(), inside.get_id().unwrap()]
        );
    }
}

#[test]
fn list_tasks_periodは指定された時刻fieldだけを参照する() {
    let now = fixed_now();
    let outside = now - Duration::hours(1);
    let inside = now + Duration::minutes(30);
    let created = crate::test_support::new_task_handle("created").unwrap();
    created.set_create_time(inside).unwrap();
    created.set_deadline_time_opt(Some(outside)).unwrap();
    created.set_end_time_opt(Some(outside)).unwrap();
    let deadline = crate::test_support::new_task_handle("deadline").unwrap();
    deadline.set_create_time(outside).unwrap();
    deadline.set_deadline_time_opt(Some(inside)).unwrap();
    deadline.set_end_time_opt(Some(outside)).unwrap();
    let completed = crate::test_support::new_task_handle("completed").unwrap();
    completed.set_create_time(outside).unwrap();
    completed.set_deadline_time_opt(Some(outside)).unwrap();
    completed.set_end_time_opt(Some(inside)).unwrap();
    let repository = TestTaskRepository::new(
        vec![created.clone(), deadline.clone(), completed.clone()],
        now,
    );

    for (field, expected_id) in [
        (TaskPeriodField::CreatedAt, created.get_id().unwrap()),
        (TaskPeriodField::Deadline, deadline.get_id().unwrap()),
        (TaskPeriodField::CompletedAt, completed.get_id().unwrap()),
    ] {
        let actual = list_tasks(
            &repository,
            ListTasksFilter {
                period: Some(TaskPeriodFilter {
                    field,
                    from: now,
                    until: now + Duration::hours(1),
                }),
                statuses: vec![],
                categories: vec![],
            },
        )
        .unwrap();

        assert_eq!(
            actual.iter().map(|task| task.id).collect::<Vec<_>>(),
            vec![expected_id]
        );
    }
}

#[test]
fn list_tasks_scheduled_start期間は分割taskを重複させない() {
    let now = fixed_now();
    let low_priority = crate::test_support::new_task_handle("低優先度").unwrap();
    low_priority.sync_clock(now).unwrap();
    low_priority
        .set_start_time(now + Duration::hours(1))
        .unwrap();
    low_priority.set_estimated_work_seconds(10 * 3600).unwrap();
    low_priority.set_priority(88).unwrap();
    let high_priority = crate::test_support::new_task_handle("高優先度").unwrap();
    high_priority.sync_clock(now).unwrap();
    high_priority
        .set_start_time(now + Duration::hours(6))
        .unwrap();
    high_priority.set_estimated_work_seconds(3600).unwrap();
    high_priority.set_priority(89).unwrap();
    let repository =
        TestTaskRepository::new(vec![low_priority.clone(), high_priority.clone()], now);

    let actual = list_tasks(
        &repository,
        ListTasksFilter {
            period: Some(TaskPeriodFilter {
                field: TaskPeriodField::ScheduledStart,
                from: now,
                until: now + Duration::hours(13),
            }),
            statuses: vec![],
            categories: vec![],
        },
    )
    .unwrap();

    assert_eq!(
        actual.iter().map(|task| task.id).collect::<Vec<_>>(),
        vec![
            low_priority.get_id().unwrap(),
            high_priority.get_id().unwrap()
        ]
    );

    let second_segment_only = list_tasks(
        &repository,
        ListTasksFilter {
            period: Some(TaskPeriodFilter {
                field: TaskPeriodField::ScheduledStart,
                from: now + Duration::hours(7),
                until: now + Duration::hours(8),
            }),
            statuses: vec![],
            categories: vec![],
        },
    )
    .unwrap();
    assert_eq!(
        second_segment_only
            .iter()
            .map(|task| task.id)
            .collect::<Vec<_>>(),
        vec![low_priority.get_id().unwrap()]
    );

    let until_exclusive = list_tasks(
        &repository,
        ListTasksFilter {
            period: Some(TaskPeriodFilter {
                field: TaskPeriodField::ScheduledStart,
                from: now + Duration::hours(5),
                until: now + Duration::hours(6),
            }),
            statuses: vec![],
            categories: vec![],
        },
    )
    .unwrap();
    assert!(until_exclusive.is_empty());
}

#[test]
fn list_tasks_fromがuntil以上ならinvalid_inputを返す() {
    let now = fixed_now();
    let repository = TestTaskRepository::new(
        vec![crate::test_support::new_task_handle("task").unwrap()],
        now,
    );

    for from in [now, now + Duration::seconds(1)] {
        let actual = list_tasks(
            &repository,
            ListTasksFilter {
                period: Some(TaskPeriodFilter {
                    field: TaskPeriodField::CreatedAt,
                    from,
                    until: now,
                }),
                statuses: vec![],
                categories: vec![],
            },
        );

        assert_eq!(
            actual,
            Err(ApplicationError::InvalidInput {
                field: "period",
                reason: "from must be earlier than until",
            })
        );
    }
}
