use super::*;
use crate::test_support::TestTaskRepository;
use chrono::TimeZone;
use std::cell::Cell;
use std::panic::{catch_unwind, AssertUnwindSafe};

fn fixed_now() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap()
}

fn create_task_with_fresh_factory(
    repository: &mut dyn TaskRepositoryTrait,
    input: CreateTaskInput,
) -> Result<Uuid, ApplicationError> {
    let mut next_id = Uuid::new_v4;
    let mut factory = TaskFactory::new(fixed_now(), &mut next_id);
    create_task(repository, input, &mut factory)
}

fn breakdown_task_with_fresh_factory(
    repository: &mut dyn TaskRepositoryTrait,
    input: BreakdownTaskInput,
) -> Result<Vec<Uuid>, ApplicationError> {
    let mut next_id = Uuid::new_v4;
    let mut factory = TaskFactory::new(fixed_now(), &mut next_id);
    breakdown_task(repository, input, &mut factory)
}

fn complete_task_with_fresh_factory(
    repository: &mut dyn TaskRepositoryTrait,
    input: CompleteTaskInput,
) -> Result<CompleteTaskOutput, ApplicationError> {
    let mut next_id = Uuid::new_v4;
    let mut factory = TaskFactory::new(fixed_now(), &mut next_id);
    complete_task(repository, input, &mut factory)
}

fn next_child_after_finish(
    repetition_anchor: RepetitionAnchor,
    days_in_advance: i64,
    focused_start_time: DateTime<Local>,
    focused_deadline_time_opt: Option<DateTime<Local>>,
    finished_at: DateTime<Local>,
) -> TaskHandle {
    let parent_task = crate::test_support::new_task_handle("ルーチン").unwrap();
    parent_task
        .set_repetition_interval_days_opt(Some(7))
        .unwrap();
    parent_task
        .set_repetition_anchor(repetition_anchor)
        .unwrap();
    parent_task.set_days_in_advance(days_in_advance).unwrap();
    parent_task
        .set_start_time(Local.with_ymd_and_hms(2026, 5, 10, 9, 30, 15).unwrap())
        .unwrap();
    parent_task
        .set_deadline_time_opt(Some(
            Local.with_ymd_and_hms(2026, 5, 10, 23, 59, 59).unwrap(),
        ))
        .unwrap();

    let mut child_task_attr =
        TaskAttr::with_identity("ルーチン(5/16)", Uuid::new_v4(), finished_at);
    child_task_attr.set_start_time(focused_start_time);
    child_task_attr.set_deadline_time_opt(focused_deadline_time_opt);
    let child_task = parent_task.create_as_last_child(child_task_attr);

    let mut repository = TestTaskRepository::new(vec![parent_task.clone()], finished_at);
    complete_task_with_fresh_factory(
        &mut repository,
        CompleteTaskInput {
            task_id: child_task.get_id().unwrap(),
            finished_at,
            additional_actual_work_seconds: 0,
        },
    )
    .unwrap();

    parent_task
        .get_children()
        .unwrap()
        .into_iter()
        .find(|task| task.get_status().unwrap() != Status::Done)
        .expect("next repetition child")
}

#[test]
fn get_task_親子関係を含むviewを返す() {
    let root = crate::test_support::new_task_handle("親").unwrap();
    root.set_priority(5).unwrap();
    root.set_project_category_opt(Some(ProjectCategory::Investment))
        .unwrap();
    let child = root.create_as_last_child(crate::test_support::new_task_attr("子"));
    let repository = TestTaskRepository::new(vec![root.clone()], fixed_now());

    let actual = get_task(&repository, child.get_id().unwrap())
        .unwrap()
        .unwrap();

    assert_eq!(actual.id, child.get_id().unwrap());
    assert_eq!(actual.root_id, root.get_id().unwrap());
    assert_eq!(actual.parent_id, Some(root.get_id().unwrap()));
    assert!(actual.child_ids.is_empty());
    assert_eq!(actual.name, "子");
    assert_eq!(actual.priority, 5);
    assert_eq!(actual.project_category, Some(ProjectCategory::Investment));
}

#[test]
fn get_task_task_viewの全属性を返す() {
    let now = fixed_now();
    let pending_until = Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap();
    let create_time = Local.with_ymd_and_hms(2026, 8, 1, 9, 0, 0).unwrap();
    let start_time = Local.with_ymd_and_hms(2026, 8, 10, 10, 0, 0).unwrap();
    let end_time = Local.with_ymd_and_hms(2026, 8, 11, 11, 0, 0).unwrap();
    let deadline_time = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
    let root = crate::test_support::new_task_handle("全属性").unwrap();
    root.set_orig_status(Status::Pending).unwrap();
    root.set_pending_until(pending_until).unwrap();
    root.set_priority(7).unwrap();
    root.set_create_time(create_time).unwrap();
    root.set_start_time(start_time).unwrap();
    root.set_end_time_opt(Some(end_time)).unwrap();
    root.set_deadline_time_opt(Some(deadline_time)).unwrap();
    root.set_estimated_work_seconds(1_800).unwrap();
    root.set_actual_work_seconds(900).unwrap();
    root.set_atomic(true).unwrap();
    root.set_is_on_other_side(true).unwrap();
    root.set_repetition_interval_days_opt(Some(7)).unwrap();
    root.set_repetition_anchor(RepetitionAnchor::Completion)
        .unwrap();
    root.set_days_in_advance(2).unwrap();
    root.set_project_category_opt(Some(ProjectCategory::Recovery))
        .unwrap();
    root.sync_clock(now).unwrap();
    let child = root.create_as_last_child(crate::test_support::new_task_attr("子"));
    let repository = TestTaskRepository::new(vec![root.clone()], now);

    let actual = get_task(&repository, root.get_id().unwrap())
        .unwrap()
        .unwrap();

    assert_eq!(actual.id, root.get_id().unwrap());
    assert_eq!(actual.root_id, root.get_id().unwrap());
    assert_eq!(actual.parent_id, None);
    assert_eq!(actual.child_ids, vec![child.get_id().unwrap()]);
    assert_eq!(actual.name, "全属性");
    assert_eq!(actual.status, Status::Pending);
    assert_eq!(actual.original_status, Status::Pending);
    assert!(actual.is_on_other_side);
    assert!(actual.atomic);
    assert_eq!(actual.pending_until, Some(pending_until));
    assert_eq!(actual.priority, 7);
    assert_eq!(actual.create_time, create_time);
    assert_eq!(actual.start_time, start_time);
    assert_eq!(actual.end_time, Some(end_time));
    assert_eq!(actual.deadline_time, Some(deadline_time));
    assert_eq!(actual.estimated_work_seconds, 1_800);
    assert_eq!(actual.actual_work_seconds, 900);
    assert_eq!(actual.repetition_interval_days, Some(7));
    assert_eq!(actual.repetition_anchor, RepetitionAnchor::Completion);
    assert_eq!(actual.days_in_advance, 2);
    assert_eq!(actual.project_category, Some(ProjectCategory::Recovery));
}

#[test]
fn get_task_未知uuidはnoneを返す() {
    let repository = TestTaskRepository::new(vec![], fixed_now());
    assert_eq!(get_task(&repository, Uuid::new_v4()), Ok(None));
}

#[test]
fn get_task_pendingでなければpending_untilはnoneを返す() {
    let task = crate::test_support::new_task_handle("未延期").unwrap();
    let repository = TestTaskRepository::new(vec![task.clone()], fixed_now());

    let actual = get_task(&repository, task.get_id().unwrap())
        .unwrap()
        .unwrap();

    assert_eq!(actual.original_status, Status::Todo);
    assert_eq!(actual.pending_until, None);
}

#[test]
fn get_focus_最高優先度leafのviewを返す() {
    let root = crate::test_support::new_task_handle("親").unwrap();
    let child = root.create_as_last_child(crate::test_support::new_task_attr("子"));
    let mut repository = TestTaskRepository::new(vec![root], fixed_now());
    repository.set_highest_priority_leaf_task_id(Some(child.get_id().unwrap()));

    assert_eq!(
        get_focus(&mut repository).unwrap().unwrap().id,
        child.get_id().unwrap()
    );
}

#[test]
fn get_focus_候補がなければnoneを返す() {
    let mut repository = TestTaskRepository::new(vec![], fixed_now());

    assert_eq!(get_focus(&mut repository), Ok(None));
}

#[test]
fn get_focus_選択されたtaskが取得できなければnoneを返す() {
    let mut repository = TestTaskRepository::new(vec![], fixed_now());
    repository.set_highest_priority_leaf_task_id(Some(Uuid::new_v4()));

    assert_eq!(get_focus(&mut repository), Ok(None));
}

#[test]
fn create_task_属性を設定してsaveしない() {
    let pending_until = Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap();
    let mut repository = TestTaskRepository::new(vec![], fixed_now());

    let task_id = create_task_with_fresh_factory(
        &mut repository,
        CreateTaskInput {
            name: "新規".to_string(),
            estimated_work_minutes: Some(30),
            pending_until: Some(pending_until),
        },
    )
    .unwrap();

    let task = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(task.get_name().unwrap(), "新規");
    assert_eq!(task.get_priority().unwrap(), 5);
    assert_eq!(task.get_estimated_work_seconds().unwrap(), 30 * 60);
    assert_eq!(task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(task.get_pending_until().unwrap(), pending_until);
    assert_eq!(repository.save_count(), 0);
}

#[test]
fn create_task_operationで固定したidと時刻を使う() {
    let now = fixed_now();
    let expected_id = Uuid::parse_str("00000000-0000-0000-0000-000000000101").unwrap();
    let mut next_id = || expected_id;
    let mut factory = TaskFactory::new(now, &mut next_id);
    let mut repository = TestTaskRepository::new(vec![], now);

    let actual_id = create_task(
        &mut repository,
        CreateTaskInput {
            name: "新規".to_string(),
            estimated_work_minutes: None,
            pending_until: None,
        },
        &mut factory,
    )
    .unwrap();

    let task = repository.get_by_id(actual_id).unwrap().unwrap();
    assert_eq!(actual_id, expected_id);
    assert_eq!(task.get_create_time().unwrap(), now);
    assert_eq!(task.get_start_time().unwrap(), now);
}

#[test]
fn create_task_空の名前を拒否して変更しない() {
    let mut repository = TestTaskRepository::new(vec![], fixed_now());

    let actual = create_task_with_fresh_factory(
        &mut repository,
        CreateTaskInput {
            name: String::new(),
            estimated_work_minutes: None,
            pending_until: None,
        },
    );

    assert!(matches!(
        actual,
        Err(ApplicationError::InvalidInput { field: "name", .. })
    ));
    assert!(repository.projects().is_empty());
}

#[test]
fn create_task_空白名と整数名を拒否して変更しない() {
    for name in [
        "   ",
        "123",
        "  -123  ",
        "+123",
        "999999999999999999999999999999999999999999",
    ] {
        let mut repository = TestTaskRepository::new(vec![], fixed_now());

        let actual = create_task_with_fresh_factory(
            &mut repository,
            CreateTaskInput {
                name: name.to_string(),
                estimated_work_minutes: None,
                pending_until: None,
            },
        );

        assert!(matches!(
            actual,
            Err(ApplicationError::InvalidInput { field: "name", .. })
        ));
        assert!(repository.projects().is_empty());
    }
}

#[test]
fn create_task_負の見積もりを拒否して変更しない() {
    let mut repository = TestTaskRepository::new(vec![], fixed_now());

    let actual = create_task_with_fresh_factory(
        &mut repository,
        CreateTaskInput {
            name: "負の見積もり".to_string(),
            estimated_work_minutes: Some(-1),
            pending_until: None,
        },
    );

    assert!(matches!(
        actual,
        Err(ApplicationError::InvalidInput {
            field: "estimated_work_minutes",
            ..
        })
    ));
    assert!(repository.projects().is_empty());
}

#[test]
fn create_task_秒変換がoverflowする見積もりをerrorにする() {
    let mut repository = TestTaskRepository::new(vec![], fixed_now());

    let actual = catch_unwind(AssertUnwindSafe(|| {
        create_task_with_fresh_factory(
            &mut repository,
            CreateTaskInput {
                name: "巨大な見積もり".to_string(),
                estimated_work_minutes: Some(i64::MAX),
                pending_until: None,
            },
        )
    }));

    assert!(matches!(
        actual,
        Ok(Err(ApplicationError::InvalidInput {
            field: "estimated_work_minutes",
            ..
        }))
    ));
    assert!(repository.projects().is_empty());
}

#[test]
fn breakdown_task_入力順と締切を維持する() {
    let parent = crate::test_support::new_task_handle("親").unwrap();
    let deadline = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
    parent.set_deadline_time_opt(Some(deadline)).unwrap();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

    let child_ids = breakdown_task_with_fresh_factory(
        &mut repository,
        BreakdownTaskInput {
            parent_id: parent.get_id().unwrap(),
            names: vec!["一".to_string(), "二".to_string()],
            pending_until: None,
        },
    )
    .unwrap();

    assert_eq!(
        parent
            .get_children()
            .unwrap()
            .iter()
            .map(|task| task.get_name().unwrap())
            .collect::<Vec<_>>(),
        vec!["一", "二"]
    );
    assert_eq!(child_ids.len(), 2);
    assert!(parent
        .get_children()
        .unwrap()
        .iter()
        .all(|child| child.get_deadline_time_opt().unwrap() == Some(deadline)));
}

#[test]
fn breakdown_task_operationで固定したid列と時刻を使う() {
    let now = fixed_now();
    let expected_ids = [
        Uuid::parse_str("00000000-0000-0000-0000-000000000201").unwrap(),
        Uuid::parse_str("00000000-0000-0000-0000-000000000202").unwrap(),
    ];
    let mut ids = expected_ids.into_iter();
    let mut next_id = || {
        ids.next()
            .expect("task id should be consumed once per child")
    };
    let mut factory = TaskFactory::new(now, &mut next_id);
    let parent = crate::test_support::new_task_handle("親").unwrap();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], now);

    let actual_ids = breakdown_task(
        &mut repository,
        BreakdownTaskInput {
            parent_id: parent.get_id().unwrap(),
            names: vec!["一".to_string(), "二".to_string()],
            pending_until: None,
        },
        &mut factory,
    )
    .unwrap();

    assert_eq!(actual_ids, expected_ids);
    for (child, expected_id) in parent.get_children().unwrap().into_iter().zip(expected_ids) {
        assert_eq!(child.get_id().unwrap(), expected_id);
        assert_eq!(child.get_create_time().unwrap(), now);
        assert_eq!(child.get_start_time().unwrap(), now);
    }
}

#[test]
fn breakdown_task_全ての子を指定時刻までpendingにする() {
    let parent = crate::test_support::new_task_handle("親").unwrap();
    let pending_until = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

    let child_ids = breakdown_task_with_fresh_factory(
        &mut repository,
        BreakdownTaskInput {
            parent_id: parent.get_id().unwrap(),
            names: vec!["一".to_string(), "二".to_string()],
            pending_until: Some(pending_until),
        },
    )
    .unwrap();

    assert_eq!(
        parent
            .get_children()
            .unwrap()
            .iter()
            .map(|task| task.get_name().unwrap())
            .collect::<Vec<_>>(),
        vec!["一", "二"]
    );
    assert_eq!(child_ids.len(), 2);
    assert!(parent.get_children().unwrap().iter().all(|child| {
        child.get_orig_status().unwrap() == Status::Pending
            && child.get_pending_until().unwrap() == pending_until
    }));
}

#[test]
fn breakdown_task_数値名を含む場合は変更しない() {
    let parent = crate::test_support::new_task_handle("親").unwrap();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

    let actual = breakdown_task_with_fresh_factory(
        &mut repository,
        BreakdownTaskInput {
            parent_id: parent.get_id().unwrap(),
            names: vec!["子".to_string(), "10".to_string()],
            pending_until: None,
        },
    );

    assert!(matches!(actual, Err(ApplicationError::InvalidInput { .. })));
    assert!(parent.get_children().unwrap().is_empty());
}

#[test]
fn breakdown_task_空の名前一覧を拒否して変更しない() {
    let parent = crate::test_support::new_task_handle("親").unwrap();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

    let actual = breakdown_task_with_fresh_factory(
        &mut repository,
        BreakdownTaskInput {
            parent_id: parent.get_id().unwrap(),
            names: vec![],
            pending_until: None,
        },
    );

    assert!(matches!(
        actual,
        Err(ApplicationError::InvalidInput { field: "names", .. })
    ));
    assert!(parent.get_children().unwrap().is_empty());
}

#[test]
fn breakdown_task_空白名を含む場合は変更しない() {
    let parent = crate::test_support::new_task_handle("親").unwrap();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

    let actual = breakdown_task_with_fresh_factory(
        &mut repository,
        BreakdownTaskInput {
            parent_id: parent.get_id().unwrap(),
            names: vec!["子".to_string(), "   ".to_string()],
            pending_until: None,
        },
    );

    assert!(matches!(actual, Err(ApplicationError::InvalidInput { .. })));
    assert!(parent.get_children().unwrap().is_empty());
}

#[test]
fn defer_task_絶対時刻までpendingにする() {
    let task = crate::test_support::new_task_handle("延期").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![task], fixed_now());
    let pending_until = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 1).unwrap();

    defer_task(&mut repository, task_id, pending_until).unwrap();

    let task = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(task.get_pending_until().unwrap(), pending_until);
}

#[test]
fn defer_routine_task_親deadlineの有無に応じて次周期へ延期する() {
    let orig_deadline = Local.with_ymd_and_hms(2026, 8, 13, 10, 0, 0).unwrap();
    let orig_start = Local.with_ymd_and_hms(2026, 8, 10, 9, 0, 0).unwrap();
    let expected_start = Local.with_ymd_and_hms(2026, 8, 17, 9, 0, 0).unwrap();

    for (parent_deadline, expected_deadline) in [
        (
            Some(Local.with_ymd_and_hms(2026, 8, 20, 18, 0, 0).unwrap()),
            Local.with_ymd_and_hms(2026, 8, 20, 18, 0, 0).unwrap(),
        ),
        (None, Local.with_ymd_and_hms(2026, 8, 20, 10, 0, 0).unwrap()),
    ] {
        let parent = crate::test_support::new_task_handle("ルーチン").unwrap();
        parent.set_repetition_interval_days_opt(Some(7)).unwrap();
        parent.set_deadline_time_opt(parent_deadline).unwrap();
        let mut child_attr = crate::test_support::new_task_attr("延期対象");
        child_attr.set_deadline_time_opt(Some(orig_deadline));
        child_attr.set_start_time(orig_start);
        child_attr.set_orig_status(Status::Pending);
        child_attr.set_pending_until(fixed_now() + Duration::hours(2));
        let child = parent.create_as_last_child(child_attr);
        let child_id = child.get_id().unwrap();
        let pending_until = child.get_pending_until().unwrap();
        let mut repository = TestTaskRepository::new(vec![parent], fixed_now());

        defer_routine_task(&mut repository, child_id).unwrap();

        assert_eq!(
            child.get_deadline_time_opt().unwrap(),
            Some(expected_deadline)
        );
        assert_eq!(child.get_start_time().unwrap(), expected_start);
        assert_eq!(child.get_orig_status().unwrap(), Status::Todo);
        assert_eq!(child.get_pending_until().unwrap(), pending_until);
    }
}

#[test]
fn defer_routine_task_未知taskを区別して変更しない() {
    let task_id = Uuid::new_v4();
    let mut repository = TestTaskRepository::new(vec![], fixed_now());

    assert_eq!(
        defer_routine_task(&mut repository, task_id),
        Err(ApplicationError::TaskNotFound(task_id))
    );
}

#[test]
fn defer_routine_task_対象条件を満たさなければ理由を区別して変更しない() {
    let deadline = Local.with_ymd_and_hms(2026, 8, 13, 10, 0, 0).unwrap();
    let cases = [
        (false, true, true, "task must have a deadline"),
        (true, false, false, "task must have a parent"),
        (
            true,
            true,
            false,
            "parent task must have a repetition interval",
        ),
    ];

    for (has_deadline, has_parent, has_interval, reason) in cases {
        let task = crate::test_support::new_task_handle("対象").unwrap();
        if has_deadline {
            task.set_deadline_time_opt(Some(deadline)).unwrap();
        }
        let (root, observed) = if has_parent {
            let parent = crate::test_support::new_task_handle("親").unwrap();
            if has_interval {
                parent.set_repetition_interval_days_opt(Some(7)).unwrap();
            }
            let child = parent.create_as_last_child(task.snapshot().unwrap().attr().clone());
            (parent, child)
        } else {
            (task.clone(), task)
        };
        let task_id = observed.get_id().unwrap();
        let snapshot = observed.snapshot().unwrap();
        let revision = observed.get_persistent_mutation_revision().unwrap();
        let mut repository = TestTaskRepository::new(vec![root], fixed_now());

        assert_eq!(
            defer_routine_task(&mut repository, task_id),
            Err(ApplicationError::InvalidInput {
                field: "task_id",
                reason,
            })
        );
        assert_eq!(observed.snapshot().unwrap(), snapshot);
        assert_eq!(
            observed.get_persistent_mutation_revision().unwrap(),
            revision
        );
    }
}

#[test]
fn defer_routine_task_日時計算不能なら変更しない() {
    let orig_deadline = DateTime::<Local>::from_naive_utc_and_offset(
        NaiveDate::MAX.and_hms_opt(6, 0, 0).unwrap(),
        chrono::FixedOffset::east_opt(0).unwrap(),
    );
    let parent = crate::test_support::new_task_handle("ルーチン").unwrap();
    parent.set_repetition_interval_days_opt(Some(7)).unwrap();
    let mut child_attr = crate::test_support::new_task_attr("延期対象");
    child_attr.set_deadline_time_opt(Some(orig_deadline));
    let child = parent.create_as_last_child(child_attr);
    let child_id = child.get_id().unwrap();
    let snapshot = child.snapshot().unwrap();
    let revision = child.get_persistent_mutation_revision().unwrap();
    let mut repository = TestTaskRepository::new(vec![parent], fixed_now());

    assert_eq!(
        defer_routine_task(&mut repository, child_id),
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "defer_routine_deadline",
            datetime: orig_deadline,
        })
    );
    assert_eq!(child.snapshot().unwrap(), snapshot);
    assert_eq!(child.get_persistent_mutation_revision().unwrap(), revision);
}

#[test]
fn complete_task_未完了の子があれば変更しない() {
    let task = crate::test_support::new_task_handle("親").unwrap();
    task.create_as_last_child(crate::test_support::new_task_attr("未完了"));
    let task_id = task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![task], fixed_now());

    let actual = complete_task_with_fresh_factory(
        &mut repository,
        CompleteTaskInput {
            task_id,
            finished_at: fixed_now(),
            additional_actual_work_seconds: 120,
        },
    );

    assert_eq!(actual, Err(ApplicationError::HasUndoneChildren(task_id)));
    let task = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(task.get_status().unwrap(), Status::Todo);
    assert_eq!(task.get_actual_work_seconds().unwrap(), 0);
}

#[test]
fn complete_task_実績を加算して完了する() {
    let task = crate::test_support::new_task_handle("完了").unwrap();
    task.set_actual_work_seconds(60).unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![task], fixed_now());

    let output = complete_task_with_fresh_factory(
        &mut repository,
        CompleteTaskInput {
            task_id,
            finished_at: fixed_now(),
            additional_actual_work_seconds: 120,
        },
    )
    .unwrap();

    let task = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(task.get_status().unwrap(), Status::Done);
    assert_eq!(task.get_end_time_opt().unwrap(), Some(fixed_now()));
    assert_eq!(task.get_actual_work_seconds().unwrap(), 180);
    assert_eq!(output.next_focus_task_id, None);
    assert_eq!(output.next_repetition_task_id, None);
}

#[test]
fn complete_task_反復なしではtask_factoryのidを消費しない() {
    let task = crate::test_support::new_task_handle("単発").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![task], fixed_now());
    let id_call_count = Cell::new(0);
    let mut next_id = || {
        id_call_count.set(id_call_count.get() + 1);
        Uuid::from_u128(0x101)
    };
    let mut factory = TaskFactory::new(fixed_now(), &mut next_id);

    complete_task(
        &mut repository,
        CompleteTaskInput {
            task_id,
            finished_at: fixed_now(),
            additional_actual_work_seconds: 0,
        },
        &mut factory,
    )
    .unwrap();

    assert_eq!(id_call_count.get(), 0);
}

#[test]
fn create_next_repetition_taskは構造化errorを返せるresultを維持する() {
    let task = crate::test_support::new_task_handle("単発").unwrap();
    let mut next_id = Uuid::new_v4;
    let mut factory = TaskFactory::new(fixed_now(), &mut next_id);

    let actual: Result<Option<Uuid>, ApplicationError> =
        create_next_repetition_task(&task, fixed_now(), &mut factory);

    assert_eq!(actual, Ok(None));
}

#[test]
fn complete_task_反復anchorの次業務日計算不能をerrorにして変更しない() {
    let occurrence_anchor = DateTime::<Local>::from_naive_utc_and_offset(
        NaiveDate::MAX.and_hms_opt(6, 0, 0).unwrap(),
        chrono::FixedOffset::east_opt(0).unwrap(),
    );
    let parent =
        TaskHandle::with_identity("ルーチン", Uuid::from_u128(0x201), fixed_now()).unwrap();
    parent.set_repetition_interval_days_opt(Some(7)).unwrap();
    parent.set_estimated_work_seconds(600).unwrap();
    let mut child_attr = TaskAttr::with_identity("今回", Uuid::from_u128(0x202), fixed_now());
    child_attr.set_deadline_time_opt(Some(occurrence_anchor));
    child_attr.set_actual_work_seconds(120);
    let child = parent.create_as_last_child(child_attr);
    let child_id = child.get_id().unwrap();
    let child_status_before = child.get_status().unwrap();
    let child_actual_before = child.get_actual_work_seconds().unwrap();
    let child_end_before = child.get_end_time_opt().unwrap();
    let child_revision_before = child.get_persistent_mutation_revision().unwrap();
    let parent_estimate_before = parent.get_estimated_work_seconds().unwrap();
    let parent_revision_before = parent.get_persistent_mutation_revision().unwrap();
    let child_ids_before = parent
        .get_children()
        .unwrap()
        .into_iter()
        .map(|task| task.get_id().unwrap())
        .collect::<Vec<_>>();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());
    repository.set_highest_priority_leaf_task_id(Some(child_id));
    let focus_before = repository.highest_priority_leaf_task_id();
    let id_call_count = Cell::new(0);
    let mut next_id = || {
        id_call_count.set(id_call_count.get() + 1);
        Uuid::from_u128(0x203)
    };
    let mut factory = TaskFactory::new(fixed_now(), &mut next_id);

    let actual = catch_unwind(AssertUnwindSafe(|| {
        complete_task(
            &mut repository,
            CompleteTaskInput {
                task_id: child_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: 60,
            },
            &mut factory,
        )
    }));

    let actual = actual.expect("complete_task must return an error instead of panicking");
    assert_eq!(
        actual,
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "next_business_day_start",
            datetime: occurrence_anchor,
        })
    );
    assert_eq!(child.get_status().unwrap(), child_status_before);
    assert_eq!(
        child.get_actual_work_seconds().unwrap(),
        child_actual_before
    );
    assert_eq!(child.get_end_time_opt().unwrap(), child_end_before);
    assert_eq!(
        child.get_persistent_mutation_revision().unwrap(),
        child_revision_before
    );
    assert_eq!(
        parent.get_estimated_work_seconds().unwrap(),
        parent_estimate_before
    );
    assert_eq!(
        parent.get_persistent_mutation_revision().unwrap(),
        parent_revision_before
    );
    assert_eq!(
        parent
            .get_children()
            .unwrap()
            .into_iter()
            .map(|task| task.get_id().unwrap())
            .collect::<Vec<_>>(),
        child_ids_before
    );
    assert_eq!(repository.highest_priority_leaf_task_id(), focus_before);
    assert_eq!(id_call_count.get(), 0);
}

#[test]
fn complete_task_反復見積補正のoverflowをerrorにして変更しない() {
    let parent =
        TaskHandle::with_identity("ルーチン", Uuid::from_u128(0x211), fixed_now()).unwrap();
    parent.set_repetition_interval_days_opt(Some(7)).unwrap();
    parent.set_estimated_work_seconds(0).unwrap();
    let child = parent.create_as_last_child(TaskAttr::with_identity(
        "今回",
        Uuid::from_u128(0x212),
        fixed_now(),
    ));
    let child_id = child.get_id().unwrap();
    let child_status_before = child.get_status().unwrap();
    let child_actual_before = child.get_actual_work_seconds().unwrap();
    let child_end_before = child.get_end_time_opt().unwrap();
    let child_revision_before = child.get_persistent_mutation_revision().unwrap();
    let parent_estimate_before = parent.get_estimated_work_seconds().unwrap();
    let parent_revision_before = parent.get_persistent_mutation_revision().unwrap();
    let child_ids_before = parent
        .get_children()
        .unwrap()
        .into_iter()
        .map(|task| task.get_id().unwrap())
        .collect::<Vec<_>>();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());
    let id_call_count = Cell::new(0);
    let mut next_id = || {
        id_call_count.set(id_call_count.get() + 1);
        Uuid::from_u128(0x213)
    };
    let mut factory = TaskFactory::new(fixed_now(), &mut next_id);

    let actual = catch_unwind(AssertUnwindSafe(|| {
        complete_task(
            &mut repository,
            CompleteTaskInput {
                task_id: child_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: i64::MAX,
            },
            &mut factory,
        )
    }));

    let actual = actual.expect("complete_task must return an error instead of panicking");
    assert_eq!(
        actual,
        Err(ApplicationError::InvalidInput {
            field: "estimated_work_seconds",
            reason: "repetition estimate adjustment overflow",
        })
    );
    assert_eq!(child.get_status().unwrap(), child_status_before);
    assert_eq!(
        child.get_actual_work_seconds().unwrap(),
        child_actual_before
    );
    assert_eq!(child.get_end_time_opt().unwrap(), child_end_before);
    assert_eq!(
        child.get_persistent_mutation_revision().unwrap(),
        child_revision_before
    );
    assert_eq!(
        parent.get_estimated_work_seconds().unwrap(),
        parent_estimate_before
    );
    assert_eq!(
        parent.get_persistent_mutation_revision().unwrap(),
        parent_revision_before
    );
    assert_eq!(
        parent
            .get_children()
            .unwrap()
            .into_iter()
            .map(|task| task.get_id().unwrap())
            .collect::<Vec<_>>(),
        child_ids_before
    );
    assert_eq!(id_call_count.get(), 0);
}

#[test]
fn complete_task_focus先読み失敗でtaskと反復親を変更しない() {
    let parent =
        TaskHandle::with_identity("ルーチン", Uuid::from_u128(0x221), fixed_now()).unwrap();
    parent.set_repetition_interval_days_opt(Some(7)).unwrap();
    parent.set_estimated_work_seconds(600).unwrap();
    let mut child_attr = TaskAttr::with_identity("今回", Uuid::from_u128(0x222), fixed_now());
    child_attr.set_actual_work_seconds(120);
    let child = parent.create_as_last_child(child_attr);
    let mut sibling_attr = TaskAttr::with_identity("完了済み", Uuid::from_u128(0x223), fixed_now());
    sibling_attr.set_orig_status(Status::Done);
    let sibling = parent.create_as_last_child(sibling_attr);
    let child_id = child.get_id().unwrap();
    let child_status_before = child.get_status().unwrap();
    let child_actual_before = child.get_actual_work_seconds().unwrap();
    let child_end_before = child.get_end_time_opt().unwrap();
    let child_revision_before = child.get_persistent_mutation_revision().unwrap();
    let parent_estimate_before = parent.get_estimated_work_seconds().unwrap();
    let parent_revision_before = parent.get_persistent_mutation_revision().unwrap();
    let child_ids_before = parent
        .get_children()
        .unwrap()
        .into_iter()
        .map(|task| task.get_id().unwrap())
        .collect::<Vec<_>>();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());
    let id_call_count = Cell::new(0);
    let mut next_id = || {
        id_call_count.set(id_call_count.get() + 1);
        Uuid::from_u128(0x224)
    };
    let mut factory = TaskFactory::new(fixed_now(), &mut next_id);

    let actual = sibling.with_exclusive_data_borrow_for_test(|| {
        complete_task(
            &mut repository,
            CompleteTaskInput {
                task_id: child_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: 60,
            },
            &mut factory,
        )
    });

    assert_eq!(
        actual,
        Err(ApplicationError::TaskTree(TaskTreeError::Borrow))
    );
    assert_eq!(child.get_status().unwrap(), child_status_before);
    assert_eq!(
        child.get_actual_work_seconds().unwrap(),
        child_actual_before
    );
    assert_eq!(child.get_end_time_opt().unwrap(), child_end_before);
    assert_eq!(
        child.get_persistent_mutation_revision().unwrap(),
        child_revision_before
    );
    assert_eq!(
        parent.get_estimated_work_seconds().unwrap(),
        parent_estimate_before
    );
    assert_eq!(
        parent.get_persistent_mutation_revision().unwrap(),
        parent_revision_before
    );
    assert_eq!(
        parent
            .get_children()
            .unwrap()
            .into_iter()
            .map(|task| task.get_id().unwrap())
            .collect::<Vec<_>>(),
        child_ids_before
    );
    assert_eq!(id_call_count.get(), 0);
}

#[test]
fn complete_task_対象以外のsiblingが全てdoneならparentを次focusにする() {
    let parent = crate::test_support::new_task_handle("親").unwrap();
    let child = parent.create_as_last_child(crate::test_support::new_task_attr("対象"));
    let mut sibling_attr = crate::test_support::new_task_attr("完了済み");
    sibling_attr.set_orig_status(Status::Done);
    parent.create_as_last_child(sibling_attr);
    let parent_id = parent.get_id().unwrap();
    let child_id = child.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![parent], fixed_now());

    let output = complete_task_with_fresh_factory(
        &mut repository,
        CompleteTaskInput {
            task_id: child_id,
            finished_at: fixed_now(),
            additional_actual_work_seconds: 0,
        },
    )
    .unwrap();

    assert_eq!(output.next_focus_task_id, Some(parent_id));
}

#[test]
fn complete_task_唯一の反復子から次回taskを生成したらfocusを返さない() {
    let parent = crate::test_support::new_task_handle("ルーチン").unwrap();
    parent.set_repetition_interval_days_opt(Some(7)).unwrap();
    let child = parent.create_as_last_child(crate::test_support::new_task_attr("今回"));
    let child_id = child.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());
    let expected_next_id = Uuid::from_u128(0x232);
    let mut next_id = || expected_next_id;
    let mut factory = TaskFactory::new(fixed_now(), &mut next_id);
    assert_eq!(
        parent
            .get_children()
            .unwrap()
            .into_iter()
            .filter(|task| task.get_status().unwrap() != Status::Done)
            .count(),
        1
    );

    let output = complete_task(
        &mut repository,
        CompleteTaskInput {
            task_id: child_id,
            finished_at: fixed_now(),
            additional_actual_work_seconds: 0,
        },
        &mut factory,
    )
    .unwrap();

    assert_eq!(output.next_focus_task_id, None);
    assert_eq!(output.next_repetition_task_id, Some(expected_next_id));
    let children = parent.get_children().unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(child.get_status().unwrap(), Status::Done);
    assert_eq!(
        children
            .into_iter()
            .filter(|task| task.get_status().unwrap() != Status::Done)
            .count(),
        1
    );
}

#[test]
fn complete_task_todoのsiblingがあれば次focusを返さない() {
    let parent = crate::test_support::new_task_handle("親").unwrap();
    let child = parent.create_as_last_child(crate::test_support::new_task_attr("対象"));
    parent.create_as_last_child(crate::test_support::new_task_attr("未完了"));
    let child_id = child.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![parent], fixed_now());

    let output = complete_task_with_fresh_factory(
        &mut repository,
        CompleteTaskInput {
            task_id: child_id,
            finished_at: fixed_now(),
            additional_actual_work_seconds: 0,
        },
    )
    .unwrap();

    assert_eq!(output.next_focus_task_id, None);
}

#[test]
fn complete_task_同一uuidのtodo_siblingを対象taskと誤認しない() {
    let parent = crate::test_support::new_task_handle("親").unwrap();
    let duplicate_id = Uuid::from_u128(0x231);
    let child =
        parent.create_as_last_child(TaskAttr::with_identity("対象", duplicate_id, fixed_now()));
    parent.create_as_last_child(TaskAttr::with_identity("未完了", duplicate_id, fixed_now()));
    let mut repository = TestTaskRepository::new(vec![parent], fixed_now());

    let output = complete_task_with_fresh_factory(
        &mut repository,
        CompleteTaskInput {
            task_id: child.get_id().unwrap(),
            finished_at: fixed_now(),
            additional_actual_work_seconds: 0,
        },
    )
    .unwrap();

    assert_eq!(output.next_focus_task_id, None);
}

#[test]
fn complete_task_繰り返しtaskを生成して見積もりを補正する() {
    let parent = crate::test_support::new_task_handle("ルーチン").unwrap();
    parent.set_repetition_interval_days_opt(Some(7)).unwrap();
    parent.set_estimated_work_seconds(600).unwrap();
    let child = parent.create_as_last_child(crate::test_support::new_task_attr("今回"));
    child.set_actual_work_seconds(1000).unwrap();
    let child_id = child.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());

    let before_completion = Local::now();
    let mut next_id = Uuid::new_v4;
    let mut factory = TaskFactory::new(Local::now(), &mut next_id);
    let output = complete_task(
        &mut repository,
        CompleteTaskInput {
            task_id: child_id,
            finished_at: fixed_now(),
            additional_actual_work_seconds: 0,
        },
        &mut factory,
    )
    .unwrap();
    let after_completion = Local::now();

    assert_eq!(parent.get_estimated_work_seconds().unwrap(), 900);
    assert!(output.next_repetition_task_id.is_some());
    assert_eq!(parent.get_children().unwrap().len(), 2);
    let next_task = repository
        .get_by_id(output.next_repetition_task_id.unwrap())
        .unwrap()
        .unwrap();
    let create_time = next_task.get_create_time().unwrap();
    assert!(before_completion <= create_time);
    assert!(create_time <= after_completion);
}

#[test]
fn complete_task_反復taskはoperation固定のidentityを使う() {
    let parent = crate::test_support::new_task_handle("ルーチン").unwrap();
    parent.set_repetition_interval_days_opt(Some(7)).unwrap();
    let child = parent.create_as_last_child(crate::test_support::new_task_attr("今回"));
    let child_id = child.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![parent], fixed_now());
    let operation_now = Local.with_ymd_and_hms(2026, 8, 12, 14, 15, 16).unwrap();
    let expected_id = Uuid::from_u128(0x102);
    let id_call_count = Cell::new(0);
    let mut next_id = || {
        id_call_count.set(id_call_count.get() + 1);
        expected_id
    };
    let mut factory = TaskFactory::new(operation_now, &mut next_id);

    let output = complete_task(
        &mut repository,
        CompleteTaskInput {
            task_id: child_id,
            finished_at: fixed_now(),
            additional_actual_work_seconds: 0,
        },
        &mut factory,
    )
    .unwrap();

    assert_eq!(output.next_repetition_task_id, Some(expected_id));
    assert_eq!(id_call_count.get(), 1);
    let next_task = repository.get_by_id(expected_id).unwrap().unwrap();
    assert_eq!(next_task.get_id().unwrap(), expected_id);
    assert_eq!(next_task.get_create_time().unwrap(), operation_now);
}

#[test]
fn complete_task_repetition_anchor_deadlineは元の期限サイクルを維持する() {
    let next_child = next_child_after_finish(
        RepetitionAnchor::Deadline,
        0,
        Local.with_ymd_and_hms(2026, 5, 16, 9, 30, 15).unwrap(),
        Some(Local.with_ymd_and_hms(2026, 5, 16, 23, 59, 59).unwrap()),
        Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap(),
    );

    assert_eq!(
        next_child.get_deadline_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 5, 23, 23, 59, 59).unwrap())
    );
}

#[test]
fn complete_task_repetition_anchor_completionは完了日から次回期限を決める() {
    let next_child = next_child_after_finish(
        RepetitionAnchor::Completion,
        0,
        Local.with_ymd_and_hms(2026, 5, 16, 9, 30, 15).unwrap(),
        Some(Local.with_ymd_and_hms(2026, 5, 16, 23, 59, 59).unwrap()),
        Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap(),
    );

    assert_eq!(
        next_child.get_deadline_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 5, 24, 23, 59, 59).unwrap())
    );
}

#[test]
fn complete_task_days_in_advanceはstart_timeだけ前倒しする() {
    let next_child = next_child_after_finish(
        RepetitionAnchor::Deadline,
        2,
        Local.with_ymd_and_hms(2026, 5, 16, 9, 30, 15).unwrap(),
        Some(Local.with_ymd_and_hms(2026, 5, 16, 23, 59, 59).unwrap()),
        Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap(),
    );

    assert_eq!(
        next_child.get_start_time().unwrap(),
        Local.with_ymd_and_hms(2026, 5, 21, 9, 30, 15).unwrap()
    );
    assert_eq!(
        next_child.get_deadline_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 5, 23, 23, 59, 59).unwrap())
    );
}

#[test]
fn complete_task_deadlineがない場合はcompletionにfallbackする() {
    let next_child = next_child_after_finish(
        RepetitionAnchor::Deadline,
        0,
        Local.with_ymd_and_hms(2026, 5, 16, 9, 30, 15).unwrap(),
        None,
        Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap(),
    );

    assert_eq!(
        next_child.get_deadline_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 5, 24, 23, 59, 59).unwrap())
    );
}

#[test]
fn complete_task_繰り返し親のatomicを次回子タスクに引き継ぐ() {
    let parent_task = crate::test_support::new_task_handle("通勤").unwrap();
    parent_task
        .set_repetition_interval_days_opt(Some(7))
        .unwrap();
    parent_task.set_atomic(true).unwrap();
    parent_task
        .set_start_time(Local.with_ymd_and_hms(2026, 5, 10, 9, 0, 0).unwrap())
        .unwrap();
    parent_task
        .set_deadline_time_opt(Some(Local.with_ymd_and_hms(2026, 5, 10, 10, 0, 0).unwrap()))
        .unwrap();

    let mut child_task_attr = TaskAttr::with_identity("通勤(5/16)", Uuid::new_v4(), fixed_now());
    child_task_attr.set_start_time(Local.with_ymd_and_hms(2026, 5, 16, 9, 0, 0).unwrap());
    child_task_attr
        .set_deadline_time_opt(Some(Local.with_ymd_and_hms(2026, 5, 16, 10, 0, 0).unwrap()));
    let child_task = parent_task.create_as_last_child(child_task_attr);

    let finished_at = Local.with_ymd_and_hms(2026, 5, 16, 10, 0, 0).unwrap();
    let mut repository = TestTaskRepository::new(vec![parent_task.clone()], finished_at);
    complete_task_with_fresh_factory(
        &mut repository,
        CompleteTaskInput {
            task_id: child_task.get_id().unwrap(),
            finished_at,
            additional_actual_work_seconds: 0,
        },
    )
    .unwrap();

    let next_child = parent_task
        .get_children()
        .unwrap()
        .into_iter()
        .find(|task| task.get_status().unwrap() != Status::Done)
        .expect("next repetition child");
    assert!(next_child.get_atomic().unwrap());
}

#[test]
fn complete_task_負の追加実績を拒否して変更しない() {
    let task = crate::test_support::new_task_handle("完了対象").unwrap();
    task.set_actual_work_seconds(120).unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![task], fixed_now());

    let actual = complete_task_with_fresh_factory(
        &mut repository,
        CompleteTaskInput {
            task_id,
            finished_at: fixed_now(),
            additional_actual_work_seconds: -1,
        },
    );

    assert!(matches!(
        actual,
        Err(ApplicationError::InvalidInput {
            field: "additional_actual_work_seconds",
            ..
        })
    ));
    let task = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(task.get_status().unwrap(), Status::Todo);
    assert_eq!(task.get_actual_work_seconds().unwrap(), 120);
    assert_eq!(task.get_end_time_opt().unwrap(), None);
}

#[test]
fn complete_task_実績加算がoverflowする場合はerrorにして変更しない() {
    let task = crate::test_support::new_task_handle("完了対象").unwrap();
    task.set_actual_work_seconds(i64::MAX).unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![task], fixed_now());

    let actual = catch_unwind(AssertUnwindSafe(|| {
        complete_task_with_fresh_factory(
            &mut repository,
            CompleteTaskInput {
                task_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: 1,
            },
        )
    }));

    assert!(matches!(
        actual,
        Ok(Err(ApplicationError::InvalidInput {
            field: "additional_actual_work_seconds",
            ..
        }))
    ));
    let task = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(task.get_status().unwrap(), Status::Todo);
    assert_eq!(task.get_actual_work_seconds().unwrap(), i64::MAX);
    assert_eq!(task.get_end_time_opt().unwrap(), None);
}

#[test]
fn update_use_cases_見積もり締切カテゴリを設定して解除する() {
    let task = crate::test_support::new_task_handle("更新").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![task], fixed_now());
    let deadline = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();

    set_estimate(&mut repository, task_id, 45).unwrap();
    set_deadline(&mut repository, task_id, Some(deadline)).unwrap();
    set_category(&mut repository, task_id, Some(ProjectCategory::Recovery)).unwrap();

    let task = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(task.get_estimated_work_seconds().unwrap(), 45 * 60);
    assert_eq!(task.get_deadline_time_opt().unwrap(), Some(deadline));
    assert_eq!(
        task.get_project_category_opt().unwrap(),
        Some(ProjectCategory::Recovery)
    );

    set_deadline(&mut repository, task_id, None).unwrap();
    set_category(&mut repository, task_id, None).unwrap();
    assert_eq!(task.get_deadline_time_opt().unwrap(), None);
    assert_eq!(task.get_project_category_opt().unwrap(), None);
}

#[test]
fn update_use_cases_未知uuidはtask_not_foundを返す() {
    let mut repository = TestTaskRepository::new(vec![], fixed_now());
    let task_id = Uuid::new_v4();

    assert_eq!(
        set_estimate(&mut repository, task_id, 10),
        Err(ApplicationError::TaskNotFound(task_id))
    );
    assert_eq!(
        breakdown_task_with_fresh_factory(
            &mut repository,
            BreakdownTaskInput {
                parent_id: task_id,
                names: vec!["子".to_string()],
                pending_until: None,
            }
        ),
        Err(ApplicationError::TaskNotFound(task_id))
    );
    assert_eq!(
        defer_task(&mut repository, task_id, fixed_now()),
        Err(ApplicationError::TaskNotFound(task_id))
    );
    assert_eq!(
        complete_task_with_fresh_factory(
            &mut repository,
            CompleteTaskInput {
                task_id,
                finished_at: fixed_now(),
                additional_actual_work_seconds: 0,
            }
        ),
        Err(ApplicationError::TaskNotFound(task_id))
    );
    assert_eq!(
        set_deadline(&mut repository, task_id, None),
        Err(ApplicationError::TaskNotFound(task_id))
    );
    assert_eq!(
        set_category(&mut repository, task_id, None),
        Err(ApplicationError::TaskNotFound(task_id))
    );
}

#[test]
fn set_estimate_負数を拒否して変更しない() {
    let task = crate::test_support::new_task_handle("更新対象").unwrap();
    task.set_estimated_work_seconds(30 * 60).unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![task], fixed_now());

    let negative = set_estimate(&mut repository, task_id, -1);
    assert!(matches!(
        negative,
        Err(ApplicationError::InvalidInput {
            field: "estimated_work_minutes",
            ..
        })
    ));
    assert_eq!(
        repository
            .get_by_id(task_id)
            .unwrap()
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        30 * 60
    );
}

#[test]
fn set_estimate_秒変換がoverflowする場合はerrorにして変更しない() {
    let task = crate::test_support::new_task_handle("更新対象").unwrap();
    task.set_estimated_work_seconds(30 * 60).unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![task], fixed_now());

    let overflow = catch_unwind(AssertUnwindSafe(|| {
        set_estimate(&mut repository, task_id, i64::MAX)
    }));
    assert!(matches!(
        overflow,
        Ok(Err(ApplicationError::InvalidInput {
            field: "estimated_work_minutes",
            ..
        }))
    ));
    assert_eq!(
        repository
            .get_by_id(task_id)
            .unwrap()
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        30 * 60
    );
}

#[test]
fn write_use_cases_repositoryをsaveしない() {
    let root = crate::test_support::new_task_handle("親").unwrap();
    let root_id = root.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![root], fixed_now());

    let created_id = create_task_with_fresh_factory(
        &mut repository,
        CreateTaskInput {
            name: "新規".to_string(),
            estimated_work_minutes: None,
            pending_until: None,
        },
    )
    .unwrap();
    let child_id = breakdown_task_with_fresh_factory(
        &mut repository,
        BreakdownTaskInput {
            parent_id: root_id,
            names: vec!["子".to_string()],
            pending_until: None,
        },
    )
    .unwrap()[0];
    defer_task(&mut repository, child_id, fixed_now()).unwrap();
    set_estimate(&mut repository, child_id, 10).unwrap();
    set_deadline(&mut repository, child_id, Some(fixed_now())).unwrap();
    set_category(&mut repository, child_id, Some(ProjectCategory::Investment)).unwrap();
    complete_task_with_fresh_factory(
        &mut repository,
        CompleteTaskInput {
            task_id: child_id,
            finished_at: fixed_now(),
            additional_actual_work_seconds: 0,
        },
    )
    .unwrap();

    assert!(repository.get_by_id(created_id).unwrap().is_some());
    assert_eq!(repository.save_count(), 0);
}
