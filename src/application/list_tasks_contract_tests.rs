use super::interface::{TaskRepositoryError, TaskRepositoryTrait};
use super::task_use_case::{
    list_tasks, ApplicationError, ListTasksFilter, TaskPeriodField, TaskPeriodFilter,
};
use crate::entity::task::{ProjectCategory, Status, TaskAttr, TaskHandle};
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

    fn sync_clock(&mut self, now: DateTime<Local>) {
        self.now = now;
    }

    fn get_last_synced_time(&self) -> DateTime<Local> {
        self.now
    }

    fn get_highest_priority_project(&mut self) -> Option<&TaskHandle> {
        self.projects.first()
    }

    fn get_highest_priority_leaf_task_id(&mut self) -> Option<Uuid> {
        None
    }

    fn get_defer_candidate_leaf_task_id(&mut self, _recent_days: i64) -> Option<Uuid> {
        None
    }

    fn get_by_id(&self, id: Uuid) -> Option<TaskHandle> {
        self.projects.iter().find_map(|task| task.get_by_id(id))
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
    let first_root = TaskHandle::new("root 1");
    first_root.sync_clock(now);
    let first_child = first_root.create_as_last_child(TaskAttr::new("child 1"));
    let grandchild = first_child.create_as_last_child(TaskAttr::new("grandchild"));
    let second_child = first_root.create_as_last_child(TaskAttr::new("child 2"));
    second_child.set_orig_status(Status::Done);
    let second_root = TaskHandle::new("root 2");
    second_root.sync_clock(now);
    let repository = TestTaskRepository::new(vec![first_root.clone(), second_root.clone()], now);

    let actual = list_tasks(&repository, no_filter()).unwrap();

    assert_eq!(
        actual.iter().map(|task| task.id).collect::<Vec<_>>(),
        vec![
            first_root.get_id(),
            first_child.get_id(),
            grandchild.get_id(),
            second_child.get_id(),
            second_root.get_id(),
        ]
    );
    assert_eq!(actual[3].status, Status::Done);
}

#[test]
fn list_tasks_statusは期限経過を反映した実効状態をorで絞る() {
    let now = fixed_now();
    let expired_pending = TaskHandle::new("期限経過Pending");
    expired_pending.set_start_time(now - Duration::hours(1));
    expired_pending.set_pending_until(now - Duration::minutes(1));
    expired_pending.set_orig_status(Status::Pending);
    expired_pending.sync_clock(now);
    let active_pending = TaskHandle::new("Pending");
    active_pending.set_start_time(now - Duration::hours(1));
    active_pending.set_pending_until(now + Duration::hours(1));
    active_pending.set_orig_status(Status::Pending);
    active_pending.sync_clock(now);
    let done = TaskHandle::new("Done");
    done.set_orig_status(Status::Done);
    done.sync_clock(now);
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
        vec![expired_pending.get_id(), done.get_id()]
    );
    assert_eq!(actual[0].original_status, Status::Pending);
    assert_eq!(actual[0].status, Status::Todo);
}

#[test]
fn list_tasks_category内はorでstatusとはandで絞る() {
    let now = fixed_now();
    let investment = TaskHandle::new("投資Todo");
    investment.set_project_category_opt(Some(ProjectCategory::Investment));
    let uncategorized = TaskHandle::new("未分類Todo");
    let earning = TaskHandle::new("獲得Todo");
    earning.set_project_category_opt(Some(ProjectCategory::Earning));
    let done_investment = TaskHandle::new("投資Done");
    done_investment.set_project_category_opt(Some(ProjectCategory::Investment));
    done_investment.set_orig_status(Status::Done);
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
        vec![investment.get_id(), uncategorized.get_id()]
    );
}

#[test]
fn list_tasks_period_status_categoryの全filterをandで絞る() {
    let now = fixed_now();
    let matched = TaskHandle::new("matched");
    matched.set_create_time(now + Duration::minutes(30));
    matched.set_project_category_opt(Some(ProjectCategory::Investment));
    let outside_period = TaskHandle::new("outside period");
    outside_period.set_create_time(now - Duration::minutes(1));
    outside_period.set_project_category_opt(Some(ProjectCategory::Investment));
    let done = TaskHandle::new("done");
    done.set_create_time(now + Duration::minutes(30));
    done.set_project_category_opt(Some(ProjectCategory::Investment));
    done.set_orig_status(Status::Done);
    let earning = TaskHandle::new("earning");
    earning.set_create_time(now + Duration::minutes(30));
    earning.set_project_category_opt(Some(ProjectCategory::Earning));
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
        vec![matched.get_id()]
    );
}

#[test]
fn list_tasks_created_deadline_completedの期間を半開区間で絞る() {
    let now = fixed_now();
    let before = TaskHandle::new("before");
    before.set_create_time(now - Duration::seconds(1));
    before.set_deadline_time_opt(Some(now - Duration::seconds(1)));
    before.set_end_time_opt(Some(now - Duration::seconds(1)));
    let from = TaskHandle::new("from");
    from.set_create_time(now);
    from.set_deadline_time_opt(Some(now));
    from.set_end_time_opt(Some(now));
    let inside = TaskHandle::new("inside");
    inside.set_create_time(now + Duration::minutes(30));
    inside.set_deadline_time_opt(Some(now + Duration::minutes(30)));
    inside.set_end_time_opt(Some(now + Duration::minutes(30)));
    let until = TaskHandle::new("until");
    until.set_create_time(now + Duration::hours(1));
    until.set_deadline_time_opt(Some(now + Duration::hours(1)));
    until.set_end_time_opt(Some(now + Duration::hours(1)));
    let missing = TaskHandle::new("missing");
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
            vec![from.get_id(), inside.get_id()]
        );
    }
}

#[test]
fn list_tasks_periodは指定された時刻fieldだけを参照する() {
    let now = fixed_now();
    let outside = now - Duration::hours(1);
    let inside = now + Duration::minutes(30);
    let created = TaskHandle::new("created");
    created.set_create_time(inside);
    created.set_deadline_time_opt(Some(outside));
    created.set_end_time_opt(Some(outside));
    let deadline = TaskHandle::new("deadline");
    deadline.set_create_time(outside);
    deadline.set_deadline_time_opt(Some(inside));
    deadline.set_end_time_opt(Some(outside));
    let completed = TaskHandle::new("completed");
    completed.set_create_time(outside);
    completed.set_deadline_time_opt(Some(outside));
    completed.set_end_time_opt(Some(inside));
    let repository = TestTaskRepository::new(
        vec![created.clone(), deadline.clone(), completed.clone()],
        now,
    );

    for (field, expected_id) in [
        (TaskPeriodField::CreatedAt, created.get_id()),
        (TaskPeriodField::Deadline, deadline.get_id()),
        (TaskPeriodField::CompletedAt, completed.get_id()),
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
    let low_priority = TaskHandle::new("低優先度");
    low_priority.sync_clock(now);
    low_priority.set_start_time(now + Duration::hours(1));
    low_priority.set_estimated_work_seconds(10 * 3600);
    low_priority.set_priority(88);
    let high_priority = TaskHandle::new("高優先度");
    high_priority.sync_clock(now);
    high_priority.set_start_time(now + Duration::hours(6));
    high_priority.set_estimated_work_seconds(3600);
    high_priority.set_priority(89);
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
        vec![low_priority.get_id(), high_priority.get_id()]
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
        vec![low_priority.get_id()]
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
    let repository = TestTaskRepository::new(vec![TaskHandle::new("task")], now);

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
