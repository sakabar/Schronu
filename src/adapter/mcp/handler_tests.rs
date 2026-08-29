use super::{
    call_breakdown_task as call_breakdown_task_with_factory,
    call_complete_task as call_complete_task_with_factory,
    call_create_task as call_create_task_with_factory, call_defer_routine_task, call_defer_task,
    call_get_focus, call_get_schedule, call_get_task, call_list_tasks, call_update_task,
    tool_input_error_response,
};
use crate::adapter::mcp::input::{
    BreakdownTaskInput, CompleteTaskInput, CreateTaskInput, DeferRoutineTaskInput, DeferTaskInput,
    GetFocusInput, GetScheduleInput, GetTaskInput, IsoDate, ListTasksInput, NonEmptyString,
    NonEmptyVec, NonNegativeI64, NullablePatch, OptionalValue, ProjectCategoryValue,
    Rfc3339DateTime, StatusValue, TaskPeriodFieldValue, TaskPeriodInput, ToolInputError,
    UpdateTaskInput, UuidValue,
};
use crate::adapter::mcp::test_support::{
    assert_tool_result_content_matches_structured, fixed_now, new_task_handle, task_for_list,
    try_next_logical_date_start, Duration, Local, ProjectCategory, RecordingRepository, Status,
    TimeZone,
};
use crate::application::task_use_case::{ApplicationError, TaskFactory};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use serde_json::{json, Value};
use std::rc::Rc;
use uuid::Uuid;

fn call_create_task(
    repository: &mut RecordingRepository,
    id: serde_json::Value,
    input: CreateTaskInput,
) -> serde_json::Value {
    let mut next_id = Uuid::new_v4;
    let mut factory = TaskFactory::new(fixed_now(), &mut next_id);
    call_create_task_with_factory(repository, id, input, &mut factory)
}

fn call_breakdown_task(
    repository: &mut RecordingRepository,
    id: serde_json::Value,
    input: BreakdownTaskInput,
) -> serde_json::Value {
    let mut next_id = Uuid::new_v4;
    let mut factory = TaskFactory::new(fixed_now(), &mut next_id);
    call_breakdown_task_with_factory(repository, id, input, &mut factory)
}

fn call_complete_task(
    repository: &mut RecordingRepository,
    id: serde_json::Value,
    input: CompleteTaskInput,
) -> serde_json::Value {
    let operation_now = fixed_now();
    let mut next_id = Uuid::new_v4;
    let mut factory = TaskFactory::new(operation_now, &mut next_id);
    call_complete_task_with_factory(repository, id, input, operation_now, &mut factory)
}

fn assert_application_datetime_error_metadata(expected: ApplicationError) {
    let response = tool_input_error_response(
        Value::String("datetime-error".to_string()),
        ToolInputError::Application(expected.clone()),
    );

    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        "internal_error"
    );
    assert_eq!(
        response["result"]["structuredContent"]["error"]["message"],
        expected.to_string()
    );
}

#[test]
fn mcp日時error境界は存在しないlocal日時を保持する() {
    let local_datetime = NaiveDateTime::new(
        NaiveDate::from_ymd_opt(2026, 3, 29).unwrap(),
        NaiveTime::from_hms_opt(2, 30, 0).unwrap(),
    );

    assert_application_datetime_error_metadata(ApplicationError::NonexistentLocalDateTime {
        local_datetime,
    });
}

#[test]
fn mcp日時error境界は曖昧なlocal日時と両候補を保持する() {
    let local_datetime = NaiveDateTime::new(
        NaiveDate::from_ymd_opt(2026, 10, 25).unwrap(),
        NaiveTime::from_hms_opt(2, 30, 0).unwrap(),
    );
    let earlier = DateTime::<Local>::from_naive_utc_and_offset(
        local_datetime,
        FixedOffset::east_opt(2 * 60 * 60).unwrap(),
    );
    let later = DateTime::<Local>::from_naive_utc_and_offset(
        local_datetime,
        FixedOffset::east_opt(60 * 60).unwrap(),
    );

    assert_application_datetime_error_metadata(ApplicationError::AmbiguousLocalDateTime {
        local_datetime,
        earlier,
        later,
    });
}

fn call_typed_list_tasks(
    repository: &RecordingRepository,
    id: &str,
    input: ListTasksInput,
) -> serde_json::Value {
    call_list_tasks(repository, json!(id), input)
}

fn response_task_ids(response: &serde_json::Value) -> Vec<Uuid> {
    response["result"]["structuredContent"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| Uuid::parse_str(task["id"].as_str().unwrap()).unwrap())
        .collect()
}

#[test]
fn get_focus_handlerはtyped_inputを受け取りrepositoryを変更しない() {
    let focused_task = new_task_handle("typed focus").unwrap();
    let task_id = focused_task.get_id().unwrap();
    let repository = RecordingRepository::new(vec![focused_task]).with_focus_task_id(task_id);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut repository = repository;

    let response = call_get_focus(&mut repository, json!("typed-focus"), GetFocusInput {});

    assert_eq!(response["result"]["isError"], false);
    assert_eq!(
        response["result"]["structuredContent"]["task"]["id"],
        task_id.to_string()
    );
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn get_task_handlerはtyped_inputを受け取りrepositoryを変更しない() {
    let task = new_task_handle("typed task").unwrap();
    let task_id = task.get_id().unwrap();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);

    let response = call_get_task(
        &repository,
        json!("typed-task"),
        GetTaskInput {
            task_id: UuidValue(task_id),
        },
    );

    assert_eq!(response["result"]["isError"], false);
    assert_eq!(
        response["result"]["structuredContent"]["task"]["id"],
        task_id.to_string()
    );
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn list_tasks_handlerはtyped_filterをapplication入力へ変換しrepositoryを変更しない() {
    let matching = task_for_list(
        "matching",
        Status::Pending,
        ProjectCategory::Recovery,
        Local.with_ymd_and_hms(2026, 8, 10, 9, 0, 0).unwrap(),
    );
    let matching_id = matching.get_id().unwrap();
    let wrong_status = task_for_list(
        "wrong status",
        Status::Todo,
        ProjectCategory::Recovery,
        Local.with_ymd_and_hms(2026, 8, 10, 10, 0, 0).unwrap(),
    );
    let wrong_category = task_for_list(
        "wrong category",
        Status::Pending,
        ProjectCategory::Investment,
        Local.with_ymd_and_hms(2026, 8, 10, 11, 0, 0).unwrap(),
    );
    let outside_period = task_for_list(
        "outside period",
        Status::Pending,
        ProjectCategory::Recovery,
        Local.with_ymd_and_hms(2026, 8, 9, 23, 59, 59).unwrap(),
    );
    let repository =
        RecordingRepository::new(vec![matching, wrong_status, wrong_category, outside_period]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);

    let response = call_typed_list_tasks(
        &repository,
        "typed-list",
        ListTasksInput {
            period: OptionalValue::Value(TaskPeriodInput {
                field: TaskPeriodFieldValue::CreatedAt,
                from: Rfc3339DateTime(Local.with_ymd_and_hms(2026, 8, 10, 0, 0, 0).unwrap()),
                until: Rfc3339DateTime(Local.with_ymd_and_hms(2026, 8, 11, 0, 0, 0).unwrap()),
            }),
            statuses: OptionalValue::Value(vec![StatusValue::Pending]),
            categories: OptionalValue::Value(vec![Some(ProjectCategoryValue::Recovery)]),
        },
    );

    assert_eq!(response["result"]["isError"], false);
    let tasks = response["result"]["structuredContent"]["tasks"]
        .as_array()
        .unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["id"], matching_id.to_string());
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn list_tasks_handlerはperiod_fieldの全4値をapplication入力へ変換する() {
    let from = fixed_now();
    let until = from + Duration::hours(1);
    let outside = from - Duration::hours(1);

    let created = new_task_handle("created").unwrap();
    created.set_orig_status(Status::Done).unwrap();
    created
        .set_create_time(from + Duration::minutes(10))
        .unwrap();
    created.set_deadline_time_opt(Some(outside)).unwrap();
    created.set_end_time_opt(Some(outside)).unwrap();

    let deadline = new_task_handle("deadline").unwrap();
    deadline.set_create_time(outside).unwrap();
    deadline
        .set_deadline_time_opt(Some(from + Duration::minutes(20)))
        .unwrap();
    deadline.set_orig_status(Status::Done).unwrap();
    deadline.set_end_time_opt(Some(outside)).unwrap();

    let completed = new_task_handle("completed").unwrap();
    completed.set_orig_status(Status::Done).unwrap();
    completed.set_create_time(outside).unwrap();
    completed.set_deadline_time_opt(Some(outside)).unwrap();
    completed
        .set_end_time_opt(Some(from + Duration::minutes(30)))
        .unwrap();

    let scheduled = new_task_handle("scheduled").unwrap();
    scheduled.set_create_time(outside).unwrap();
    scheduled
        .set_start_time(from + Duration::minutes(40))
        .unwrap();
    scheduled.set_estimated_work_seconds(15 * 60).unwrap();

    let expected = [
        (
            TaskPeriodFieldValue::ScheduledStart,
            scheduled.get_id().unwrap(),
        ),
        (TaskPeriodFieldValue::CreatedAt, created.get_id().unwrap()),
        (TaskPeriodFieldValue::Deadline, deadline.get_id().unwrap()),
        (
            TaskPeriodFieldValue::CompletedAt,
            completed.get_id().unwrap(),
        ),
    ];
    let repository = RecordingRepository::new(vec![created, deadline, completed, scheduled]);

    for (field, expected_id) in expected {
        let response = call_typed_list_tasks(
            &repository,
            "typed-period-field",
            ListTasksInput {
                period: OptionalValue::Value(TaskPeriodInput {
                    field,
                    from: Rfc3339DateTime(from),
                    until: Rfc3339DateTime(until),
                }),
                statuses: OptionalValue::Missing,
                categories: OptionalValue::Missing,
            },
        );

        assert_eq!(response_task_ids(&response), vec![expected_id]);
    }
}

#[test]
fn list_tasks_handlerはstatusの全3値をapplication入力へ変換する() {
    let todo = task_for_list("todo", Status::Todo, ProjectCategory::Recovery, fixed_now());
    let pending = task_for_list(
        "pending",
        Status::Pending,
        ProjectCategory::Recovery,
        fixed_now(),
    );
    let done = task_for_list("done", Status::Done, ProjectCategory::Recovery, fixed_now());
    let expected = [
        (StatusValue::Todo, todo.get_id().unwrap()),
        (StatusValue::Pending, pending.get_id().unwrap()),
        (StatusValue::Done, done.get_id().unwrap()),
    ];
    let repository = RecordingRepository::new(vec![todo, pending, done]);

    for (status, expected_id) in expected {
        let response = call_typed_list_tasks(
            &repository,
            "typed-status",
            ListTasksInput {
                period: OptionalValue::Missing,
                statuses: OptionalValue::Value(vec![status]),
                categories: OptionalValue::Missing,
            },
        );

        assert_eq!(response_task_ids(&response), vec![expected_id]);
    }
}

#[test]
fn list_tasks_handlerはcategoryの全5値とnullをapplication入力へ変換する() {
    let earning = task_for_list(
        "earning",
        Status::Todo,
        ProjectCategory::Earning,
        fixed_now(),
    );
    let sustaining = task_for_list(
        "sustaining",
        Status::Todo,
        ProjectCategory::Sustaining,
        fixed_now(),
    );
    let recovery = task_for_list(
        "recovery",
        Status::Todo,
        ProjectCategory::Recovery,
        fixed_now(),
    );
    let investment = task_for_list(
        "investment",
        Status::Todo,
        ProjectCategory::Investment,
        fixed_now(),
    );
    let consumption = task_for_list(
        "consumption",
        Status::Todo,
        ProjectCategory::Consumption,
        fixed_now(),
    );
    let uncategorized = new_task_handle("uncategorized").unwrap();
    let expected = [
        (
            Some(ProjectCategoryValue::Earning),
            earning.get_id().unwrap(),
        ),
        (
            Some(ProjectCategoryValue::Sustaining),
            sustaining.get_id().unwrap(),
        ),
        (
            Some(ProjectCategoryValue::Recovery),
            recovery.get_id().unwrap(),
        ),
        (
            Some(ProjectCategoryValue::Investment),
            investment.get_id().unwrap(),
        ),
        (
            Some(ProjectCategoryValue::Consumption),
            consumption.get_id().unwrap(),
        ),
        (None, uncategorized.get_id().unwrap()),
    ];
    let repository = RecordingRepository::new(vec![
        earning,
        sustaining,
        recovery,
        investment,
        consumption,
        uncategorized,
    ]);

    for (category, expected_id) in expected {
        let response = call_typed_list_tasks(
            &repository,
            "typed-category",
            ListTasksInput {
                period: OptionalValue::Missing,
                statuses: OptionalValue::Missing,
                categories: OptionalValue::Value(vec![category]),
            },
        );

        assert_eq!(response_task_ids(&response), vec![expected_id]);
    }
}

#[test]
fn get_schedule_handlerはtyped日付範囲で予定を絞りrepositoryを変更しない() {
    let from = Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap();
    let until = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let inside = new_task_handle("inside range").unwrap();
    inside.set_start_time(from + Duration::hours(1)).unwrap();
    inside.set_estimated_work_seconds(15 * 60).unwrap();
    let outside = new_task_handle("outside range").unwrap();
    outside.set_start_time(until + Duration::hours(1)).unwrap();
    outside.set_estimated_work_seconds(15 * 60).unwrap();
    let repository = RecordingRepository::new(vec![inside, outside]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);

    let response = call_get_schedule(
        &repository,
        json!("typed-schedule-range"),
        GetScheduleInput {
            from: OptionalValue::Value(IsoDate(from.date_naive())),
            until: OptionalValue::Value(IsoDate(until.date_naive())),
        },
    );

    assert_eq!(response["result"]["isError"], false);
    let schedule = response["result"]["structuredContent"]["schedule"]
        .as_array()
        .unwrap();
    assert_eq!(schedule.len(), 1);
    assert_eq!(schedule[0]["task"]["name"], "inside range");
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn get_schedule_handlerはtyped_missingで現在から次の論理日境界までを既定とする() {
    let past = new_task_handle("past task").unwrap();
    past.set_start_time(fixed_now() - Duration::hours(1))
        .unwrap();
    past.set_estimated_work_seconds(15 * 60).unwrap();
    past.set_actual_work_seconds(15 * 60).unwrap();
    let current = new_task_handle("current task").unwrap();
    current.set_start_time(fixed_now()).unwrap();
    current.set_estimated_work_seconds(15 * 60).unwrap();
    let future = new_task_handle("future task").unwrap();
    future
        .set_start_time(try_next_logical_date_start(fixed_now()).unwrap() + Duration::hours(1))
        .unwrap();
    future.set_estimated_work_seconds(15 * 60).unwrap();
    let repository = RecordingRepository::new(vec![past, current, future]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);

    let response = call_get_schedule(
        &repository,
        json!("typed-schedule-default"),
        GetScheduleInput {
            from: OptionalValue::Missing,
            until: OptionalValue::Missing,
        },
    );

    assert_eq!(response["result"]["isError"], false);
    let schedule = response["result"]["structuredContent"]["schedule"]
        .as_array()
        .unwrap();
    assert_eq!(schedule.len(), 1);
    assert_eq!(schedule[0]["task"]["name"], "current task");
    assert!(schedule
        .iter()
        .all(|scheduled| scheduled["task"]["name"] != "past task"));
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn create_task_handlerはtyped_inputをapplication入力へ変換する() {
    let pending_until = fixed_now() + Duration::hours(18);
    let repository = RecordingRepository::new(vec![]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut repository = repository;

    let response = call_create_task(
        &mut repository,
        json!("typed-create"),
        CreateTaskInput {
            name: NonEmptyString("created by typed input".to_string()),
            estimated_work_minutes: OptionalValue::Value(NonNegativeI64(30)),
            pending_until: OptionalValue::Value(Rfc3339DateTime(pending_until)),
        },
    );

    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    let task_id = Uuid::parse_str(
        response["result"]["structuredContent"]["task_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let created = call_get_task(
        &repository,
        json!("typed-created-task"),
        GetTaskInput {
            task_id: UuidValue(task_id),
        },
    );
    let task = &created["result"]["structuredContent"]["task"];
    assert_eq!(task["name"], "created by typed input");
    assert_eq!(task["estimated_work_seconds"], 30 * 60);
    assert_eq!(task["original_status"], "pending");
    assert_eq!(task["pending_until"], pending_until.to_rfc3339());
    assert_eq!(mutation_count.get(), 1);
    assert_eq!(save_count.get(), 0);
}

#[test]
fn breakdown_task_handlerはtyped_inputの順序と時刻をapplication入力へ変換する() {
    let pending_until = fixed_now() + Duration::hours(18);
    let parent = new_task_handle("typed parent").unwrap();
    let parent_id = parent.get_id().unwrap();
    let repository = RecordingRepository::new(vec![parent]);
    let save_count = Rc::clone(&repository.save_count);
    let mut repository = repository;

    let response = call_breakdown_task(
        &mut repository,
        json!("typed-breakdown"),
        BreakdownTaskInput {
            parent_id: UuidValue(parent_id),
            names: NonEmptyVec(vec![
                NonEmptyString("first typed child".to_string()),
                NonEmptyString("second typed child".to_string()),
            ]),
            pending_until: OptionalValue::Value(Rfc3339DateTime(pending_until)),
        },
    );

    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    let child_ids = response["result"]["structuredContent"]["child_ids"]
        .as_array()
        .unwrap();
    assert_eq!(child_ids.len(), 2);
    for (index, expected_name) in ["first typed child", "second typed child"]
        .iter()
        .enumerate()
    {
        let child_id = Uuid::parse_str(child_ids[index].as_str().unwrap()).unwrap();
        let child = call_get_task(
            &repository,
            json!("typed-child"),
            GetTaskInput {
                task_id: UuidValue(child_id),
            },
        );
        let task = &child["result"]["structuredContent"]["task"];
        assert_eq!(task["name"], *expected_name);
        assert_eq!(task["original_status"], "pending");
        assert_eq!(task["pending_until"], pending_until.to_rfc3339());
    }
    assert_eq!(save_count.get(), 0);
}

#[test]
fn create_task_handlerはtyped_inputの空白名をapplicationへそのまま渡す() {
    let repository = RecordingRepository::new(vec![]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut repository = repository;

    let response = call_create_task(
        &mut repository,
        json!("typed-create-blank"),
        CreateTaskInput {
            name: NonEmptyString("   ".to_string()),
            estimated_work_minutes: OptionalValue::Missing,
            pending_until: OptionalValue::Missing,
        },
    );

    assert_eq!(response["result"]["isError"], true);
    assert_tool_result_content_matches_structured(&response);
    let error = &response["result"]["structuredContent"]["error"];
    assert_eq!(error["code"], "invalid_input");
    assert_eq!(error["field"], "name");
    assert_eq!(error["message"], "must not be blank");
    assert_eq!(mutation_count.get(), 0);
    assert_eq!(save_count.get(), 0);
}

#[test]
fn breakdown_task_handlerはtyped_inputの空白名をapplicationへそのまま渡す() {
    let parent = new_task_handle("typed parent").unwrap();
    let parent_id = parent.get_id().unwrap();
    let parent_observer = parent.clone();
    let repository = RecordingRepository::new(vec![parent]);
    let save_count = Rc::clone(&repository.save_count);
    let mut repository = repository;

    let response = call_breakdown_task(
        &mut repository,
        json!("typed-breakdown-blank"),
        BreakdownTaskInput {
            parent_id: UuidValue(parent_id),
            names: NonEmptyVec(vec![
                NonEmptyString("valid child".to_string()),
                NonEmptyString("   ".to_string()),
            ]),
            pending_until: OptionalValue::Missing,
        },
    );

    assert_eq!(response["result"]["isError"], true);
    assert_tool_result_content_matches_structured(&response);
    let error = &response["result"]["structuredContent"]["error"];
    assert_eq!(error["code"], "invalid_input");
    assert_eq!(error["field"], "names");
    assert_eq!(error["message"], "must not be blank");
    assert!(parent_observer.get_children().unwrap().is_empty());
    assert_eq!(save_count.get(), 0);
}

#[test]
fn create_task_handlerはtyped_i64上限の秒変換overflowをtool_errorにする() {
    let repository = RecordingRepository::new(vec![]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut repository = repository;

    let response = call_create_task(
        &mut repository,
        json!("typed-create-overflow"),
        CreateTaskInput {
            name: NonEmptyString("overflow".to_string()),
            estimated_work_minutes: OptionalValue::Value(NonNegativeI64(i64::MAX)),
            pending_until: OptionalValue::Missing,
        },
    );

    assert_eq!(response["result"]["isError"], true);
    assert_tool_result_content_matches_structured(&response);
    let error = &response["result"]["structuredContent"]["error"];
    assert_eq!(error["code"], "invalid_input");
    assert_eq!(error["field"], "estimated_work_minutes");
    assert_eq!(error["message"], "seconds conversion overflow");
    assert_eq!(mutation_count.get(), 0);
    assert_eq!(save_count.get(), 0);
}

#[test]
fn defer_task_handlerはtyped_uuidと時刻をapplication入力へ変換する() {
    let pending_until = fixed_now() + Duration::hours(18);
    let task = new_task_handle("typed deferred task").unwrap();
    let task_id = task.get_id().unwrap();
    let task_observer = task.clone();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mut repository = repository;

    let response = call_defer_task(
        &mut repository,
        json!("typed-defer"),
        DeferTaskInput {
            task_id: UuidValue(task_id),
            pending_until: Rfc3339DateTime(pending_until),
        },
    );

    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"],
        json!({"task_id": task_id.to_string()})
    );
    assert_eq!(task_observer.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(task_observer.get_pending_until().unwrap(), pending_until);
    assert_eq!(save_count.get(), 0);
}

#[test]
fn defer_routine_task_handlerはtyped_uuidで次周期へ延期する() {
    let deadline = fixed_now() + Duration::hours(2);
    let parent = new_task_handle("routine parent").unwrap();
    parent.set_repetition_interval_days_opt(Some(7)).unwrap();
    let mut child_attr = crate::test_support::new_task_attr("routine child");
    child_attr.set_deadline_time_opt(Some(deadline));
    child_attr.set_orig_status(Status::Pending);
    let child = parent.create_as_last_child(child_attr);
    let task_id = child.get_id().unwrap();
    let repository = RecordingRepository::new(vec![parent]);
    let save_count = Rc::clone(&repository.save_count);
    let mut repository = repository;

    let response = call_defer_routine_task(
        &mut repository,
        json!("typed-defer-routine"),
        DeferRoutineTaskInput {
            task_id: UuidValue(task_id),
        },
    );

    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"],
        json!({"task_id": task_id.to_string()})
    );
    assert_eq!(
        child.get_deadline_time_opt().unwrap(),
        Some(deadline + Duration::days(7))
    );
    assert_eq!(child.get_orig_status().unwrap(), Status::Todo);
    assert_eq!(save_count.get(), 0);
}

#[test]
fn defer_routine_task_handlerは未知taskと対象不成立をstructured_errorにする() {
    let missing_id = Uuid::new_v4();
    let mut repository = RecordingRepository::new(vec![]);
    let missing = call_defer_routine_task(
        &mut repository,
        json!("missing-routine"),
        DeferRoutineTaskInput {
            task_id: UuidValue(missing_id),
        },
    );
    let missing_error = &missing["result"]["structuredContent"]["error"];
    assert_eq!(missing_error["code"], "task_not_found");
    assert_eq!(missing_error["field"], "task_id");
    assert_eq!(missing_error["task_id"], missing_id.to_string());

    let cases = [
        (false, false, "task must have a deadline"),
        (true, false, "task must have a parent"),
    ];
    for (has_deadline, has_parent, reason) in cases {
        let task = new_task_handle("not routine").unwrap();
        if has_deadline {
            task.set_deadline_time_opt(Some(fixed_now() + Duration::days(1)))
                .unwrap();
        }
        let (root, observed) = if has_parent {
            let parent = new_task_handle("parent without interval").unwrap();
            let child = parent.create_as_last_child(task.snapshot().unwrap().attr().clone());
            (parent, child)
        } else {
            (task.clone(), task)
        };
        let task_id = observed.get_id().unwrap();
        let snapshot = observed.snapshot().unwrap();
        let mut repository = RecordingRepository::new(vec![root]);
        let invalid = call_defer_routine_task(
            &mut repository,
            json!("invalid-routine"),
            DeferRoutineTaskInput {
                task_id: UuidValue(task_id),
            },
        );
        let invalid_error = &invalid["result"]["structuredContent"]["error"];
        assert_eq!(invalid_error["code"], "invalid_input");
        assert_eq!(invalid_error["field"], "task_id");
        assert_eq!(invalid_error["message"], reason);
        assert_eq!(observed.snapshot().unwrap(), snapshot);
    }

    let parent = new_task_handle("parent without interval").unwrap();
    let mut child_attr = crate::test_support::new_task_attr("routine child");
    child_attr.set_deadline_time_opt(Some(fixed_now() + Duration::days(1)));
    let child = parent.create_as_last_child(child_attr);
    let task_id = child.get_id().unwrap();
    let snapshot = child.snapshot().unwrap();
    let mut repository = RecordingRepository::new(vec![parent]);
    let invalid = call_defer_routine_task(
        &mut repository,
        json!("missing-interval"),
        DeferRoutineTaskInput {
            task_id: UuidValue(task_id),
        },
    );
    let invalid_error = &invalid["result"]["structuredContent"]["error"];
    assert_eq!(invalid_error["code"], "invalid_input");
    assert_eq!(invalid_error["field"], "task_id");
    assert_eq!(
        invalid_error["message"],
        "parent task must have a repetition interval"
    );
    assert_eq!(child.snapshot().unwrap(), snapshot);
}

#[test]
fn defer_routine_task_handlerは日時error情報を保持して変更しない() {
    let deadline = DateTime::<Local>::from_naive_utc_and_offset(
        NaiveDate::MAX.and_hms_opt(6, 0, 0).unwrap(),
        FixedOffset::east_opt(0).unwrap(),
    );
    let parent = new_task_handle("routine parent").unwrap();
    parent.set_repetition_interval_days_opt(Some(7)).unwrap();
    let mut child_attr = crate::test_support::new_task_attr("routine child");
    child_attr.set_deadline_time_opt(Some(deadline));
    let child = parent.create_as_last_child(child_attr);
    let task_id = child.get_id().unwrap();
    let snapshot = child.snapshot().unwrap();
    let mut repository = RecordingRepository::new(vec![parent]);

    let response = call_defer_routine_task(
        &mut repository,
        json!("datetime-error"),
        DeferRoutineTaskInput {
            task_id: UuidValue(task_id),
        },
    );

    let expected = ApplicationError::LogicalDateOutOfRange {
        operation: "defer_routine_deadline",
        datetime: deadline,
    };
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        "internal_error"
    );
    assert_eq!(
        response["result"]["structuredContent"]["error"]["message"],
        expected.to_string()
    );
    assert_eq!(child.snapshot().unwrap(), snapshot);
}

#[test]
fn complete_task_handlerはtyped完了時刻と追加実績をapplication入力へ変換する() {
    let finished_at = fixed_now() + Duration::hours(1);
    let task = new_task_handle("typed completed task").unwrap();
    task.set_actual_work_seconds(60).unwrap();
    let task_id = task.get_id().unwrap();
    let task_observer = task.clone();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mut repository = repository;

    let response = call_complete_task(
        &mut repository,
        json!("typed-complete"),
        CompleteTaskInput {
            task_id: UuidValue(task_id),
            finished_at: OptionalValue::Value(Rfc3339DateTime(finished_at)),
            additional_actual_work_seconds: NonNegativeI64(120),
        },
    );

    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"],
        json!({
            "task_id": task_id.to_string(),
            "next_focus_task_id": null,
            "next_repetition_task_id": null
        })
    );
    assert_eq!(task_observer.get_orig_status().unwrap(), Status::Done);
    assert_eq!(task_observer.get_end_time_opt().unwrap(), Some(finished_at));
    assert_eq!(task_observer.get_actual_work_seconds().unwrap(), 180);
    assert_eq!(save_count.get(), 0);
}

#[test]
fn complete_task_handlerはfinished_at_missingをoperation時刻に変換する() {
    let task = new_task_handle("typed completed with defaults").unwrap();
    task.set_actual_work_seconds(60).unwrap();
    let task_id = task.get_id().unwrap();
    let task_observer = task.clone();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mut repository = repository;
    let response = call_complete_task(
        &mut repository,
        json!("typed-complete-defaults"),
        CompleteTaskInput {
            task_id: UuidValue(task_id),
            finished_at: OptionalValue::Missing,
            additional_actual_work_seconds: NonNegativeI64(0),
        },
    );
    assert_eq!(response["id"], "typed-complete-defaults");
    assert_eq!(response["result"]["isError"], false);
    let end_time = task_observer.get_end_time_opt().unwrap().unwrap();
    assert_eq!(end_time, fixed_now());
    assert_eq!(task_observer.get_actual_work_seconds().unwrap(), 60);
    assert_eq!(save_count.get(), 0);
}

#[test]
fn update_task_handlerはtyped_valueを公開field順に適用する() {
    let deadline = fixed_now() + Duration::days(10);
    let task = new_task_handle("typed updated task").unwrap();
    let task_id = task.get_id().unwrap();
    let task_observer = task.clone();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mut repository = repository;

    let response = call_update_task(
        &mut repository,
        json!("typed-update-values"),
        UpdateTaskInput {
            task_id: UuidValue(task_id),
            estimated_work_minutes: OptionalValue::Value(NonNegativeI64(45)),
            deadline_time: NullablePatch::Value(Rfc3339DateTime(deadline)),
            category: NullablePatch::Value(ProjectCategoryValue::Recovery),
        },
    );

    assert_eq!(response["id"], "typed-update-values");
    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"],
        json!({"task_id": task_id.to_string()})
    );
    assert_eq!(task_observer.get_estimated_work_seconds().unwrap(), 45 * 60);
    assert_eq!(
        task_observer.get_deadline_time_opt().unwrap(),
        Some(deadline)
    );
    assert_eq!(
        task_observer.get_project_category_opt().unwrap(),
        Some(ProjectCategory::Recovery)
    );
    assert_eq!(save_count.get(), 0);
}

#[test]
fn update_task_handlerはtyped_missingのnullable_patchを変更しない() {
    let deadline = fixed_now() + Duration::days(10);
    let task = new_task_handle("typed preserved task").unwrap();
    task.set_estimated_work_seconds(30 * 60).unwrap();
    task.set_deadline_time_opt(Some(deadline)).unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Investment))
        .unwrap();
    let task_id = task.get_id().unwrap();
    let task_observer = task.clone();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mut repository = repository;

    let response = call_update_task(
        &mut repository,
        json!("typed-update-missing-patches"),
        UpdateTaskInput {
            task_id: UuidValue(task_id),
            estimated_work_minutes: OptionalValue::Value(NonNegativeI64(45)),
            deadline_time: NullablePatch::Missing,
            category: NullablePatch::Missing,
        },
    );

    assert_eq!(response["id"], "typed-update-missing-patches");
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(task_observer.get_estimated_work_seconds().unwrap(), 45 * 60);
    assert_eq!(
        task_observer.get_deadline_time_opt().unwrap(),
        Some(deadline)
    );
    assert_eq!(
        task_observer.get_project_category_opt().unwrap(),
        Some(ProjectCategory::Investment)
    );
    assert_eq!(save_count.get(), 0);
}

#[test]
fn update_task_handlerはtyped_missingを変更せずnullを解除に変換する() {
    let deadline = fixed_now() + Duration::days(10);
    let task = new_task_handle("typed cleared task").unwrap();
    task.set_estimated_work_seconds(30 * 60).unwrap();
    task.set_deadline_time_opt(Some(deadline)).unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Investment))
        .unwrap();
    let task_id = task.get_id().unwrap();
    let task_observer = task.clone();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mut repository = repository;

    let response = call_update_task(
        &mut repository,
        json!("typed-update-clear"),
        UpdateTaskInput {
            task_id: UuidValue(task_id),
            estimated_work_minutes: OptionalValue::Missing,
            deadline_time: NullablePatch::Null,
            category: NullablePatch::Null,
        },
    );

    assert_eq!(response["id"], "typed-update-clear");
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(task_observer.get_estimated_work_seconds().unwrap(), 30 * 60);
    assert_eq!(task_observer.get_deadline_time_opt().unwrap(), None);
    assert_eq!(task_observer.get_project_category_opt().unwrap(), None);
    assert_eq!(save_count.get(), 0);
}

#[test]
fn update_task_handlerは先頭field失敗時に後続fieldを適用しない() {
    let original_deadline = fixed_now() + Duration::days(10);
    let requested_deadline = fixed_now() + Duration::days(20);
    let task = new_task_handle("typed unchanged update task").unwrap();
    task.set_estimated_work_seconds(30 * 60).unwrap();
    task.set_deadline_time_opt(Some(original_deadline)).unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Consumption))
        .unwrap();
    let task_id = task.get_id().unwrap();
    let task_observer = task.clone();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mut repository = repository;

    let response = call_update_task(
        &mut repository,
        json!("typed-update-first-field-error"),
        UpdateTaskInput {
            task_id: UuidValue(task_id),
            estimated_work_minutes: OptionalValue::Value(NonNegativeI64(i64::MAX)),
            deadline_time: NullablePatch::Value(Rfc3339DateTime(requested_deadline)),
            category: NullablePatch::Value(ProjectCategoryValue::Investment),
        },
    );

    assert_eq!(response["id"], "typed-update-first-field-error");
    assert_eq!(response["result"]["isError"], true);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"]["error"],
        json!({
            "code": "invalid_input",
            "message": "seconds conversion overflow",
            "field": "estimated_work_minutes"
        })
    );
    assert_eq!(task_observer.get_estimated_work_seconds().unwrap(), 30 * 60);
    assert_eq!(
        task_observer.get_deadline_time_opt().unwrap(),
        Some(original_deadline)
    );
    assert_eq!(
        task_observer.get_project_category_opt().unwrap(),
        Some(ProjectCategory::Consumption)
    );
    assert_eq!(save_count.get(), 0);
}
