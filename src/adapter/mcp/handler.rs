use super::input::{
    decode_input, BreakdownTaskInput, CompleteTaskInput, CreateTaskInput, DeferTaskInput,
    GetFocusInput, GetScheduleInput, GetTaskInput, ListTasksInput, ToolInputError, UpdateTaskInput,
};
use super::internal_error_response;
use super::output::{scheduled_task_view_json, task_view_json};
use super::protocol::{error_response, invalid_params_response, tool_result_response};
use crate::application::interface::TaskRepositoryTrait;
use crate::application::schedule_use_case::get_schedule;
use crate::application::task_use_case::{
    breakdown_task as breakdown_task_use_case, complete_task as complete_task_use_case,
    create_task as create_task_use_case, defer_task as defer_task_use_case, get_focus, get_task,
    list_tasks, set_category, set_deadline, set_estimate, ApplicationError,
};
use serde_json::{json, Value};
use uuid::Uuid;

pub(super) fn call_tool<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    request: &Value,
) -> Value {
    let params = &request["params"];
    match params["name"].as_str() {
        Some("get_focus") => {
            let empty_arguments = json!({});
            let input = match decode_input::<GetFocusInput>(
                params.get("arguments").unwrap_or(&empty_arguments),
            ) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_get_focus(repository, id, input)
        }
        Some("get_task") => {
            let input = match decode_input::<GetTaskInput>(&params["arguments"]) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_get_task(repository, id, input)
        }
        Some("list_tasks") => {
            let empty_arguments = json!({});
            let input = match decode_input::<ListTasksInput>(
                params.get("arguments").unwrap_or(&empty_arguments),
            ) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_list_tasks(repository, id, input)
        }
        Some("get_schedule") => {
            let empty_arguments = json!({});
            let input = match decode_input::<GetScheduleInput>(
                params.get("arguments").unwrap_or(&empty_arguments),
            ) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_get_schedule(repository, id, input)
        }
        Some("create_task") => {
            let input = match decode_input::<CreateTaskInput>(&params["arguments"]) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_create_task(repository, id, input)
        }
        Some("breakdown_task") => {
            let input = match decode_input::<BreakdownTaskInput>(&params["arguments"]) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_breakdown_task(repository, id, input)
        }
        Some("defer_task") => {
            let input = match decode_input::<DeferTaskInput>(&params["arguments"]) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_defer_task(repository, id, input)
        }
        Some("complete_task") => {
            let input = match decode_input::<CompleteTaskInput>(&params["arguments"]) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_complete_task(repository, id, input)
        }
        Some("update_task") => {
            let input = match decode_input::<UpdateTaskInput>(&params["arguments"]) {
                Ok(input) => input,
                Err(error) => return tool_input_error_response(id, error),
            };
            call_update_task(repository, id, input)
        }
        _ => error_response(id, -32602, "Unknown tool"),
    }
}

fn call_get_focus<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    _input: GetFocusInput,
) -> Value {
    match get_focus(repository) {
        Ok(task) => {
            let task = task.as_ref().map(task_view_json).unwrap_or(Value::Null);
            tool_result_response(id, json!({"task": task}), false)
        }
        Err(error) => internal_error_response(id, &error.to_string()),
    }
}

fn call_get_task<R: TaskRepositoryTrait>(repository: &R, id: Value, input: GetTaskInput) -> Value {
    let task_id = input.task_id.0;

    match get_task(repository, task_id) {
        Ok(Some(task)) => tool_result_response(id, json!({"task": task_view_json(&task)}), false),
        Ok(None) => task_not_found_response(id, task_id, None),
        Err(error) => internal_error_response(id, &error.to_string()),
    }
}

fn call_list_tasks<R: TaskRepositoryTrait>(
    repository: &R,
    id: Value,
    input: ListTasksInput,
) -> Value {
    match list_tasks(repository, input.into_filter()) {
        Ok(tasks) => tool_result_response(
            id,
            json!({
                "tasks": tasks.iter().map(task_view_json).collect::<Vec<_>>()
            }),
            false,
        ),
        Err(ApplicationError::InvalidInput { field, reason }) => {
            invalid_input_response(id, field, reason)
        }
        Err(error) => internal_error_response(id, &error.to_string()),
    }
}

pub(super) fn call_get_schedule<R: TaskRepositoryTrait>(
    repository: &R,
    id: Value,
    input: GetScheduleInput,
) -> Value {
    let (from, until) = match input.into_period(repository.get_last_synced_time()) {
        Ok(period) => period,
        Err(ToolInputError::Schema(error)) => return invalid_params_response(id, error),
        Err(ToolInputError::Semantic { field, message }) => {
            return invalid_input_response(id, &field, message)
        }
    };

    match get_schedule(repository) {
        Ok(schedule) => tool_result_response(
            id,
            json!({
                "schedule": schedule
                    .iter()
                    .filter(|scheduled| scheduled.scheduled_start < until && scheduled.scheduled_end > from)
                    .map(scheduled_task_view_json)
                    .collect::<Vec<_>>()
            }),
            false,
        ),
        Err(error) => internal_error_response(id, &error.to_string()),
    }
}

fn call_create_task<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    input: CreateTaskInput,
) -> Value {
    let input = input.into_application();

    let task_id = match create_task_use_case(repository, input) {
        Ok(task_id) => task_id,
        Err(ApplicationError::InvalidInput { field, reason }) => {
            return invalid_input_response(id, field, reason)
        }
        Err(error) => return internal_error_response(id, &error.to_string()),
    };

    tool_result_response(id, json!({"task_id": task_id.to_string()}), false)
}

fn call_breakdown_task<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    input: BreakdownTaskInput,
) -> Value {
    let input = input.into_application();
    let child_ids = match breakdown_task_use_case(repository, input) {
        Ok(child_ids) => child_ids,
        Err(ApplicationError::TaskNotFound(task_id)) => {
            return task_not_found_response(id, task_id, Some("parent_id"))
        }
        Err(ApplicationError::InvalidInput { field, reason }) => {
            return invalid_input_response(id, field, reason)
        }
        Err(error) => return internal_error_response(id, &error.to_string()),
    };

    tool_result_response(
        id,
        json!({
            "child_ids": child_ids.iter().map(Uuid::to_string).collect::<Vec<_>>()
        }),
        false,
    )
}

fn call_defer_task<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    input: DeferTaskInput,
) -> Value {
    let (task_id, pending_until) = input.into_parts();

    match defer_task_use_case(repository, task_id, pending_until) {
        Ok(()) => {}
        Err(ApplicationError::TaskNotFound(task_id)) => {
            return task_not_found_response(id, task_id, Some("task_id"))
        }
        Err(ApplicationError::InvalidInput { field, reason }) => {
            return invalid_input_response(id, field, reason)
        }
        Err(error) => return internal_error_response(id, &error.to_string()),
    }

    tool_result_response(id, json!({"task_id": task_id.to_string()}), false)
}

fn call_complete_task<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    input: CompleteTaskInput,
) -> Value {
    let input = input.into_application();
    let task_id = input.task_id;
    let output = match complete_task_use_case(repository, input) {
        Ok(output) => output,
        Err(ApplicationError::TaskNotFound(task_id)) => {
            return task_not_found_response(id, task_id, Some("task_id"))
        }
        Err(ApplicationError::HasUndoneChildren(task_id)) => {
            return has_undone_children_response(id, task_id)
        }
        Err(ApplicationError::InvalidInput { field, reason }) => {
            return invalid_input_response(id, field, reason)
        }
        Err(ApplicationError::TaskTree(error)) => {
            return internal_error_response(id, &error.to_string())
        }
    };

    tool_result_response(
        id,
        json!({
            "task_id": task_id.to_string(),
            "next_focus_task_id": output.next_focus_task_id.map(|task_id| task_id.to_string()),
            "next_repetition_task_id": output.next_repetition_task_id.map(|task_id| task_id.to_string())
        }),
        false,
    )
}

fn call_update_task<R: TaskRepositoryTrait>(
    repository: &mut R,
    id: Value,
    input: UpdateTaskInput,
) -> Value {
    let input = input.into_changes();

    if let Some(estimated_work_minutes) = input.estimated_work_minutes {
        if let Err(error) = set_estimate(repository, input.task_id, estimated_work_minutes) {
            return update_task_application_error_response(id, error);
        }
    }
    if let Some(deadline_time) = input.deadline_time {
        if let Err(error) = set_deadline(repository, input.task_id, deadline_time) {
            return update_task_application_error_response(id, error);
        }
    }
    if let Some(category) = input.category {
        if let Err(error) = set_category(repository, input.task_id, category) {
            return update_task_application_error_response(id, error);
        }
    }

    tool_result_response(id, json!({"task_id": input.task_id.to_string()}), false)
}

pub(super) fn tool_call_succeeded_with_mutation(request: &Value, response: &Value) -> bool {
    matches!(
        request["params"]["name"].as_str(),
        Some("create_task" | "breakdown_task" | "defer_task" | "complete_task" | "update_task")
    ) && response.get("error").is_none()
        && response["result"]["isError"] != Value::Bool(true)
}

fn invalid_input_response(id: Value, field: &str, message: &str) -> Value {
    tool_result_response(
        id,
        json!({
            "error": {
                "code": "invalid_input",
                "message": message,
                "field": field
            }
        }),
        true,
    )
}

fn tool_input_error_response(id: Value, error: ToolInputError) -> Value {
    match error {
        ToolInputError::Schema(error) => invalid_params_response(id, error),
        ToolInputError::Semantic { field, message } => invalid_input_response(id, &field, message),
    }
}

fn task_not_found_response(id: Value, task_id: Uuid, field: Option<&str>) -> Value {
    let mut error = json!({
        "code": "task_not_found",
        "message": format!("task not found: {task_id}"),
        "task_id": task_id.to_string()
    });
    if let Some(field) = field {
        error["field"] = Value::String(field.to_string());
    }
    tool_result_response(id, json!({"error": error}), true)
}

fn has_undone_children_response(id: Value, task_id: Uuid) -> Value {
    tool_result_response(
        id,
        json!({
            "error": {
                "code": "has_undone_children",
                "message": format!("task has undone children: {task_id}"),
                "task_id": task_id.to_string(),
                "field": "task_id"
            }
        }),
        true,
    )
}

fn update_task_application_error_response(id: Value, error: ApplicationError) -> Value {
    match error {
        ApplicationError::TaskNotFound(task_id) => {
            task_not_found_response(id, task_id, Some("task_id"))
        }
        ApplicationError::InvalidInput { field, reason } => {
            invalid_input_response(id, field, reason)
        }
        error => internal_error_response(id, &error.to_string()),
    }
}

#[cfg(test)]
mod typed_handler_contract_tests {
    use super::{
        call_breakdown_task, call_complete_task, call_create_task, call_defer_task, call_get_focus,
        call_get_schedule, call_get_task, call_list_tasks, call_update_task,
    };
    use crate::adapter::mcp::input::{
        BreakdownTaskInput, CompleteTaskInput, CreateTaskInput, DeferTaskInput, GetFocusInput,
        GetScheduleInput, GetTaskInput, IsoDate, ListTasksInput, NonEmptyString, NonEmptyVec,
        NonNegativeI64, NullablePatch, OptionalValue, ProjectCategoryValue, Rfc3339DateTime,
        StatusValue, TaskPeriodFieldValue, TaskPeriodInput, UpdateTaskInput, UuidValue,
    };
    use crate::adapter::mcp::test_support::{
        assert_tool_result_content_matches_structured, fixed_now, get_next_morning_datetime,
        task_for_list, Duration, Local, ProjectCategory, RecordingRepository, Status, TaskHandle,
        TimeZone,
    };
    use serde_json::json;
    use std::rc::Rc;
    use uuid::Uuid;

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
        let focused_task = TaskHandle::new("typed focus").unwrap();
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
        let task = TaskHandle::new("typed task").unwrap();
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

        let created = TaskHandle::new("created").unwrap();
        created.set_orig_status(Status::Done).unwrap();
        created
            .set_create_time(from + Duration::minutes(10))
            .unwrap();
        created.set_deadline_time_opt(Some(outside)).unwrap();
        created.set_end_time_opt(Some(outside)).unwrap();

        let deadline = TaskHandle::new("deadline").unwrap();
        deadline.set_create_time(outside).unwrap();
        deadline
            .set_deadline_time_opt(Some(from + Duration::minutes(20)))
            .unwrap();
        deadline.set_orig_status(Status::Done).unwrap();
        deadline.set_end_time_opt(Some(outside)).unwrap();

        let completed = TaskHandle::new("completed").unwrap();
        completed.set_orig_status(Status::Done).unwrap();
        completed.set_create_time(outside).unwrap();
        completed.set_deadline_time_opt(Some(outside)).unwrap();
        completed
            .set_end_time_opt(Some(from + Duration::minutes(30)))
            .unwrap();

        let scheduled = TaskHandle::new("scheduled").unwrap();
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
        let uncategorized = TaskHandle::new("uncategorized").unwrap();
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
        let inside = TaskHandle::new("inside range").unwrap();
        inside.set_start_time(from + Duration::hours(1)).unwrap();
        inside.set_estimated_work_seconds(15 * 60).unwrap();
        let outside = TaskHandle::new("outside range").unwrap();
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
    fn get_schedule_handlerはtyped_missingで現在から次の業務日境界までを既定とする() {
        let past = TaskHandle::new("past task").unwrap();
        past.set_start_time(fixed_now() - Duration::hours(1))
            .unwrap();
        past.set_estimated_work_seconds(15 * 60).unwrap();
        past.set_actual_work_seconds(15 * 60).unwrap();
        let current = TaskHandle::new("current task").unwrap();
        current.set_start_time(fixed_now()).unwrap();
        current.set_estimated_work_seconds(15 * 60).unwrap();
        let future = TaskHandle::new("future task").unwrap();
        future
            .set_start_time(get_next_morning_datetime(fixed_now()) + Duration::hours(1))
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
        let parent = TaskHandle::new("typed parent").unwrap();
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
        let parent = TaskHandle::new("typed parent").unwrap();
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
        let task = TaskHandle::new("typed deferred task").unwrap();
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
    fn complete_task_handlerはtyped完了時刻と追加実績をapplication入力へ変換する() {
        let finished_at = fixed_now() + Duration::hours(1);
        let task = TaskHandle::new("typed completed task").unwrap();
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
    fn complete_task_handlerはfinished_at_missingを現在時刻に変換する() {
        let task = TaskHandle::new("typed completed with defaults").unwrap();
        task.set_actual_work_seconds(60).unwrap();
        let task_id = task.get_id().unwrap();
        let task_observer = task.clone();
        let repository = RecordingRepository::new(vec![task]);
        let save_count = Rc::clone(&repository.save_count);
        let mut repository = repository;
        let before = Local::now();

        let response = call_complete_task(
            &mut repository,
            json!("typed-complete-defaults"),
            CompleteTaskInput {
                task_id: UuidValue(task_id),
                finished_at: OptionalValue::Missing,
                additional_actual_work_seconds: NonNegativeI64(0),
            },
        );
        let after = Local::now();

        assert_eq!(response["id"], "typed-complete-defaults");
        assert_eq!(response["result"]["isError"], false);
        let end_time = task_observer.get_end_time_opt().unwrap().unwrap();
        assert!(before <= end_time && end_time <= after);
        assert_eq!(task_observer.get_actual_work_seconds().unwrap(), 60);
        assert_eq!(save_count.get(), 0);
    }

    #[test]
    fn update_task_handlerはtyped_valueを公開field順に適用する() {
        let deadline = fixed_now() + Duration::days(10);
        let task = TaskHandle::new("typed updated task").unwrap();
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
        let task = TaskHandle::new("typed preserved task").unwrap();
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
        let task = TaskHandle::new("typed cleared task").unwrap();
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
        let task = TaskHandle::new("typed unchanged update task").unwrap();
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
}
