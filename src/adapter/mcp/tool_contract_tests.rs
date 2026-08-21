use super::test_support::*;
use super::McpServer;

#[test]
fn 初期化済みtools_callは検証結果によらずdispatch直前にrepository_clockを同期してloadする() {
    let cases = [
        ("valid", "list_tasks", json!({})),
        ("invalid-arguments", "get_task", json!({})),
        ("unknown-tool", "unknown_tool", json!({})),
    ];

    for (id, tool_name, arguments) in cases {
        let repository = RecordingRepository::new(vec![]);
        let load_count = Rc::clone(&repository.load_count);
        let reload_if_changed_count = Rc::clone(&repository.reload_if_changed_count);
        let operation_order = Rc::clone(&repository.operation_order);
        let sync_clock_times = Rc::clone(&repository.sync_clock_times);
        let mut server = initialized_server(repository);
        let before = Local::now();

        server
            .handle_request(tool_call_request(id, tool_name, arguments))
            .unwrap();

        let after = Local::now();
        let sync_clock_times = sync_clock_times.borrow();
        assert_eq!(sync_clock_times.len(), 1, "case: {id}");
        assert_eq!(load_count.get(), 1, "case: {id}");
        assert_eq!(reload_if_changed_count.get(), 1, "case: {id}");
        assert_eq!(
            *operation_order.borrow(),
            vec!["sync_clock", "load"],
            "case: {id}"
        );
        assert!(
            before <= sync_clock_times[0] && sync_clock_times[0] <= after,
            "case: {id}, actual: {}",
            sync_clock_times[0]
        );
    }
}

#[test]
fn 同一mcp_processの連続read_toolはreload_if_changed経路を使う() {
    let repository = RecordingRepository::new(vec![]);
    let reload_if_changed_count = Rc::clone(&repository.reload_if_changed_count);
    let mut server = initialized_server(repository);

    server
        .handle_request(tool_call_request("first", "list_tasks", json!({})))
        .unwrap();
    server
        .handle_request(tool_call_request("second", "list_tasks", json!({})))
        .unwrap();

    assert_eq!(reload_if_changed_count.get(), 2);
}

#[test]
fn get_scheduleは借用競合を既存internal_error形式で返す() {
    let task = TaskHandle::new("借用競合").unwrap();
    let server = McpServer::new(RecordingRepository::new(vec![task.clone()]));

    let response = task.with_exclusive_data_borrow_for_test(|| {
        super::handler::call_get_schedule(
            &server.repository,
            json!("borrow"),
            super::input::GetScheduleInput {
                from: super::input::OptionalValue::Missing,
                until: super::input::OptionalValue::Missing,
            },
        )
    });

    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        "internal_error"
    );
    assert_eq!(
        response["result"]["structuredContent"]["error"]["message"],
        "task tree operation failed: cannot borrow task tree data"
    );
}

#[test]
fn 同一mcp_processの2回目のread_toolは実repositoryのcacheを使う() {
    let storage = McpCacheTestStorage::new();
    let storage_path = storage.path.to_str().unwrap();
    let now = fixed_now();
    let mut source = TaskRepository::new(storage_path);
    source.sync_clock(now).unwrap();
    source
        .start_new_project(TaskHandle::new("MCP cache対象").unwrap())
        .unwrap();
    source.save().unwrap();
    let project_yaml_path = storage
        .path
        .join("20260811-MCP cache対象")
        .join("project.yaml");
    let repository = TaskRepository::new(storage_path);
    let mut server = McpServer::with_storage_directory(repository, &storage.path);
    server.handle_request(initialize_request()).unwrap();
    server.handle_request(json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }));

    let first = server
        .handle_request(tool_call_request("first", "list_tasks", json!({})))
        .unwrap();
    assert_eq!(first["result"]["isError"], false);
    fs::write(project_yaml_path, "project: [").unwrap();

    let second = server
        .handle_request(tool_call_request("second", "list_tasks", json!({})))
        .unwrap();

    assert_eq!(second["result"]["isError"], false);
    assert_eq!(
        second["result"]["structuredContent"]["tasks"][0]["name"],
        "MCP cache対象"
    );
}

#[test]
fn repository_load失敗はtaskを作成せずstructured_errorを返し同一sessionの次回callで再試行する() {
    let repository = RecordingRepository::new(vec![]).with_load_failure_once();
    let load_count = Rc::clone(&repository.load_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let operation_order = Rc::clone(&repository.operation_order);
    let project_count = Rc::clone(&repository.project_count);
    let save_count = Rc::clone(&repository.save_count);
    let sync_clock_times = Rc::clone(&repository.sync_clock_times);
    let mut server = initialized_server(repository);

    let failed = server
        .handle_request(tool_call_request(
            "load-failure",
            "create_task",
            json!({"name": "must not be created before load"}),
        ))
        .unwrap();

    assert_eq!(failed["jsonrpc"], "2.0");
    assert_eq!(failed["id"], "load-failure");
    assert_eq!(failed["result"]["isError"], true);
    assert_tool_result_content_matches_structured(&failed);
    let error = &failed["result"]["structuredContent"]["error"];
    assert_eq!(error["code"], "repository_load_failed");
    assert_eq!(error["recovery"], "repair_repository");
    assert!(!error["message"].as_str().unwrap().is_empty());
    assert_eq!(sync_clock_times.borrow().len(), 1);
    assert_eq!(load_count.get(), 1);
    assert_eq!(mutation_count.get(), 0);
    assert_eq!(project_count.get(), 0);
    assert_eq!(save_count.get(), 0);
    assert_eq!(*operation_order.borrow(), vec!["sync_clock", "load"]);

    let retried = server
        .handle_request(tool_call_request(
            "load-retry",
            "create_task",
            json!({"name": "created after successful retry"}),
        ))
        .unwrap();

    assert_eq!(retried["jsonrpc"], "2.0");
    assert_eq!(retried["id"], "load-retry");
    assert_eq!(retried["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&retried);
    assert_eq!(sync_clock_times.borrow().len(), 2);
    assert_eq!(load_count.get(), 2);
    assert_eq!(mutation_count.get(), 1);
    assert_eq!(project_count.get(), 1);
    assert_eq!(save_count.get(), 1);
    assert_eq!(
        *operation_order.borrow(),
        vec![
            "sync_clock",
            "load",
            "sync_clock",
            "load",
            "mutation",
            "save"
        ]
    );
}

#[test]
#[allow(non_snake_case)]
fn get_taskはTaskViewをstructured_contentで返してsaveしない() {
    let pending_until = Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap();
    let create_time = Local.with_ymd_and_hms(2026, 8, 1, 9, 0, 0).unwrap();
    let start_time = Local.with_ymd_and_hms(2026, 8, 10, 10, 0, 0).unwrap();
    let deadline_time = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
    let root = TaskHandle::new("MCP task").unwrap();
    root.set_orig_status(Status::Pending).unwrap();
    root.set_pending_until(pending_until).unwrap();
    root.set_priority(7).unwrap();
    root.set_create_time(create_time).unwrap();
    root.set_start_time(start_time).unwrap();
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
    root.sync_clock(fixed_now()).unwrap();
    let child = root.create_as_last_child(TaskAttr::new("child"));
    let task_id = root.get_id().unwrap();
    let child_id = child.get_id().unwrap();
    let repository = RecordingRepository::new(vec![root]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "get-task",
            "get_task",
            json!({"task_id": task_id.to_string()}),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "get-task");
    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    let task = &response["result"]["structuredContent"]["task"];
    assert_eq!(
        sorted_object_keys(task),
        vec![
            "actual_work_seconds",
            "atomic",
            "child_ids",
            "create_time",
            "days_in_advance",
            "deadline_time",
            "end_time",
            "estimated_work_seconds",
            "id",
            "is_on_other_side",
            "name",
            "original_status",
            "parent_id",
            "pending_until",
            "priority",
            "project_category",
            "repetition_anchor",
            "repetition_interval_days",
            "root_id",
            "start_time",
            "status"
        ]
    );
    assert_eq!(
        task,
        &json_fixture(
            include_str!("../../../tests/fixtures/mcp/task-view.json"),
            &[
                ("{{task_id}}", &task_id.to_string()),
                ("{{child_id}}", &child_id.to_string()),
                ("{{create_time}}", &create_time.to_rfc3339()),
                ("{{deadline_time}}", &deadline_time.to_rfc3339()),
                ("{{pending_until}}", &pending_until.to_rfc3339()),
                ("{{start_time}}", &start_time.to_rfc3339()),
            ],
        )
    );
    assert_eq!(task["id"], task_id.to_string());
    assert_eq!(task["root_id"], task_id.to_string());
    assert_eq!(task["parent_id"], serde_json::Value::Null);
    assert_eq!(task["child_ids"], json!([child_id.to_string()]));
    assert_eq!(task["name"], "MCP task");
    assert_eq!(task["status"], "pending");
    assert_eq!(task["original_status"], "pending");
    assert_eq!(task["is_on_other_side"], true);
    assert_eq!(task["atomic"], true);
    assert_eq!(task["pending_until"], pending_until.to_rfc3339());
    assert_eq!(task["priority"], 7);
    assert_eq!(task["create_time"], create_time.to_rfc3339());
    assert_eq!(task["start_time"], start_time.to_rfc3339());
    assert_eq!(task["end_time"], serde_json::Value::Null);
    assert_eq!(task["deadline_time"], deadline_time.to_rfc3339());
    assert_eq!(task["estimated_work_seconds"], 1_800);
    assert_eq!(task["actual_work_seconds"], 900);
    assert_eq!(task["repetition_interval_days"], 7);
    assert_eq!(task["repetition_anchor"], "completion");
    assert_eq!(task["days_in_advance"], 2);
    assert_eq!(task["project_category"], "recovery");

    let child_response = server
        .handle_request(tool_call_request(
            "get-child-task",
            "get_task",
            json!({"task_id": child_id.to_string()}),
        ))
        .unwrap();
    assert_eq!(child_response["jsonrpc"], "2.0");
    assert_eq!(child_response["id"], "get-child-task");
    assert_tool_result_content_matches_structured(&child_response);
    let child_task = &child_response["result"]["structuredContent"]["task"];
    assert_eq!(child_task["id"], child_id.to_string());
    assert_eq!(child_task["root_id"], task_id.to_string());
    assert_eq!(child_task["parent_id"], task_id.to_string());
    assert_eq!(child_task["child_ids"], json!([]));
    assert_eq!(child_task["pending_until"], serde_json::Value::Null);
    assert_eq!(child_task["end_time"], serde_json::Value::Null);
    assert_eq!(child_task["deadline_time"], serde_json::Value::Null);
    assert_eq!(
        child_task["repetition_interval_days"],
        serde_json::Value::Null
    );
    assert_eq!(child_task["project_category"], "recovery");
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn get_task_不正uuidをstructured_errorで返す() {
    let repository = RecordingRepository::new(vec![]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "invalid-task-id",
            "get_task",
            json!({"task_id": "not-a-uuid"}),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "invalid-task-id");
    assert_eq!(response["result"]["isError"], true);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        "invalid_input"
    );
    assert_eq!(
        response["result"]["structuredContent"]["error"]["field"],
        "task_id"
    );
    assert!(!response["result"]["structuredContent"]["error"]["message"]
        .as_str()
        .unwrap()
        .is_empty());
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn get_task_schema違反をinvalid_paramsで返す() {
    let task_id = Uuid::new_v4().to_string();
    let cases = [
        (
            "extra-field",
            json!({"task_id": task_id, "extra": 1}),
            "arguments.extra",
        ),
        ("missing-task-id", json!({}), "task_id"),
        ("wrong-task-id-type", json!({"task_id": 1}), "task_id"),
        ("non-object-arguments", serde_json::Value::Null, "arguments"),
    ];

    for (id, arguments, expected_field) in cases {
        let repository = RecordingRepository::new(vec![]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);

        let response = server
            .handle_request(tool_call_request(id, "get_task", arguments))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        assert_eq!(response["error"]["code"], -32602);
        assert_eq!(response["error"]["message"], "Invalid params");
        assert_eq!(response["error"]["data"]["code"], "invalid_input");
        assert_eq!(response["error"]["data"]["field"], expected_field);
        assert!(!response["error"]["data"]["reason"]
            .as_str()
            .unwrap()
            .is_empty());
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }
}

#[test]
fn get_task_未知uuidをstructured_errorで返す() {
    let task_id = Uuid::new_v4();
    let repository = RecordingRepository::new(vec![]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "missing-task",
            "get_task",
            json!({"task_id": task_id.to_string()}),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "missing-task");
    assert_eq!(response["result"]["isError"], true);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        "task_not_found"
    );
    assert_eq!(
        response["result"]["structuredContent"]["error"]["task_id"],
        task_id.to_string()
    );
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn get_focus_選択taskを返してrepositoryを変更しない() {
    let non_focused_task = TaskHandle::new("not focused").unwrap();
    let focused_task = TaskHandle::new("focused task").unwrap();
    let task_id = focused_task.get_id().unwrap();
    let repository =
        RecordingRepository::new(vec![non_focused_task, focused_task]).with_focus_task_id(task_id);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request("get-focus", "get_focus", json!({})))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "get-focus");
    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"]["task"]["id"],
        task_id.to_string()
    );
    assert_eq!(
        response["result"]["structuredContent"]["task"]["name"],
        "focused task"
    );
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn get_focus_候補なしをtask_nullで返す() {
    let repository = RecordingRepository::new(vec![]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(json!({
            "jsonrpc": "2.0",
            "id": "no-focus",
            "method": "tools/call",
            "params": {"name": "get_focus"}
        }))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "no-focus");
    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"]["task"],
        serde_json::Value::Null
    );
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn get_focus_extra_fieldをinvalid_paramsで返す() {
    let repository = RecordingRepository::new(vec![]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "get-focus-extra",
            "get_focus",
            json!({"extra": true}),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "get-focus-extra");
    assert_eq!(response["error"]["code"], -32602);
    assert_eq!(response["error"]["message"], "Invalid params");
    assert_eq!(response["error"]["data"]["code"], "invalid_input");
    assert_eq!(response["error"]["data"]["field"], "arguments.extra");
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn list_tasks_期間status_categoryで絞ってrepositoryを変更しない() {
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
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "list-filtered",
            "list_tasks",
            json!({
                "period": {
                    "field": "created_at",
                    "from": "2026-08-10T00:00:00+09:00",
                    "until": "2026-08-11T00:00:00+09:00"
                },
                "statuses": ["pending"],
                "categories": ["recovery"]
            }),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "list-filtered");
    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    let tasks = response["result"]["structuredContent"]["tasks"]
        .as_array()
        .unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["id"], matching_id.to_string());
    assert_eq!(tasks[0]["name"], "matching");
    assert_eq!(tasks[0]["status"], "pending");
    assert_eq!(tasks[0]["project_category"], "recovery");
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn list_tasks_arguments省略で全taskを返す() {
    let first = TaskHandle::new("first").unwrap();
    let first_id = first.get_id().unwrap();
    let child = first.create_as_last_child(TaskAttr::new("child"));
    let child_id = child.get_id().unwrap();
    let second = TaskHandle::new("second").unwrap();
    let second_id = second.get_id().unwrap();
    let repository = RecordingRepository::new(vec![first, second]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(json!({
            "jsonrpc": "2.0",
            "id": "list-all",
            "method": "tools/call",
            "params": {"name": "list_tasks"}
        }))
        .unwrap();

    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    let ids = response["result"]["structuredContent"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            first_id.to_string(),
            child_id.to_string(),
            second_id.to_string()
        ]
    );
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn list_tasks_null_categoryで未分類taskを絞る() {
    let uncategorized = TaskHandle::new("uncategorized").unwrap();
    let uncategorized_id = uncategorized.get_id().unwrap();
    let categorized = TaskHandle::new("categorized").unwrap();
    categorized
        .set_project_category_opt(Some(ProjectCategory::Recovery))
        .unwrap();
    let repository = RecordingRepository::new(vec![uncategorized, categorized]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "list-uncategorized",
            "list_tasks",
            json!({"categories": [null]}),
        ))
        .unwrap();

    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    let tasks = response["result"]["structuredContent"]["tasks"]
        .as_array()
        .unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["id"], uncategorized_id.to_string());
    assert_eq!(tasks[0]["project_category"], serde_json::Value::Null);
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn list_tasks_schema違反をinvalid_paramsで返す() {
    let cases = [
        ("statuses-type", json!({"statuses": "pending"}), "statuses"),
        (
            "status-value",
            json!({"statuses": ["invalid"]}),
            "statuses[0]",
        ),
        (
            "category-value",
            json!({"categories": ["invalid"]}),
            "categories[0]",
        ),
        (
            "period-required",
            json!({
                "period": {
                    "field": "created_at",
                    "from": "2026-08-10T00:00:00+09:00"
                }
            }),
            "period.until",
        ),
        ("extra", json!({"extra": true}), "arguments.extra"),
    ];

    for (id, arguments, field) in cases {
        let repository = RecordingRepository::new(vec![]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);
        let response = server
            .handle_request(tool_call_request(id, "list_tasks", arguments))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        assert_eq!(response["error"]["code"], -32602);
        assert_eq!(response["error"]["message"], "Invalid params");
        assert_eq!(response["error"]["data"]["code"], "invalid_input");
        assert_eq!(response["error"]["data"]["field"], field);
        assert!(!response["error"]["data"]["reason"]
            .as_str()
            .unwrap()
            .is_empty());
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }
}

#[test]
fn list_tasks_不正日時と逆転期間をstructured_errorで返す() {
    let cases = [
        (
            "invalid-date",
            json!({
                "period": {
                    "field": "created_at",
                    "from": "not-a-date",
                    "until": "2026-08-11T00:00:00+09:00"
                }
            }),
            "period.from",
        ),
        (
            "reversed-period",
            json!({
                "period": {
                    "field": "created_at",
                    "from": "2026-08-11T00:00:00+09:00",
                    "until": "2026-08-10T00:00:00+09:00"
                }
            }),
            "period",
        ),
    ];

    for (id, arguments, field) in cases {
        let repository = RecordingRepository::new(vec![]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);
        let response = server
            .handle_request(tool_call_request(id, "list_tasks", arguments))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        assert_eq!(response["result"]["isError"], true);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            "invalid_input"
        );
        assert_eq!(
            response["result"]["structuredContent"]["error"]["field"],
            field
        );
        assert!(!response["result"]["structuredContent"]["error"]["message"]
            .as_str()
            .unwrap()
            .is_empty());
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }
}

#[test]
#[allow(non_snake_case)]
fn get_scheduleは予定をScheduledTaskViewの全field付きで返しrepositoryを変更しない() {
    let task = TaskHandle::new("scheduled task").unwrap();
    let task_id = task.get_id().unwrap();
    let create_time = Local.with_ymd_and_hms(2026, 8, 1, 9, 0, 0).unwrap();
    task.set_create_time(create_time).unwrap();
    task.set_start_time(fixed_now()).unwrap();
    task.set_estimated_work_seconds(15 * 60).unwrap();
    task.set_priority(5).unwrap();
    task.sync_clock(fixed_now()).unwrap();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let sync_clock_times = Rc::clone(&repository.sync_clock_times);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(json!({
            "jsonrpc": "2.0",
            "id": "get-schedule",
            "method": "tools/call",
            "params": {"name": "get_schedule"}
        }))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "get-schedule");
    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    let schedule = response["result"]["structuredContent"]["schedule"]
        .as_array()
        .unwrap();
    assert_eq!(schedule.len(), 1);
    assert_eq!(
        sorted_object_keys(&schedule[0]),
        vec![
            "first_available_time",
            "rank",
            "scheduled_end",
            "scheduled_start",
            "scheduled_work_seconds",
            "task",
            "total_work_seconds"
        ]
    );
    let synced_now = sync_clock_times.borrow()[0];
    assert_eq!(
        schedule[0],
        json_fixture(
            include_str!("../../../tests/fixtures/mcp/scheduled-task-view.json"),
            &[
                ("{{task_id}}", &task_id.to_string()),
                ("{{create_time}}", &create_time.to_rfc3339()),
                ("{{start_time}}", &fixed_now().to_rfc3339()),
                ("{{scheduled_start}}", &synced_now.to_rfc3339()),
                (
                    "{{scheduled_end}}",
                    &(synced_now + Duration::minutes(15)).to_rfc3339(),
                ),
            ],
        )
    );
    assert_eq!(schedule[0]["task"]["id"], task_id.to_string());
    assert_eq!(schedule[0]["task"]["name"], "scheduled task");
    assert_eq!(schedule[0]["first_available_time"], synced_now.to_rfc3339());
    assert_eq!(schedule[0]["scheduled_start"], synced_now.to_rfc3339());
    assert_eq!(
        schedule[0]["scheduled_end"],
        (synced_now + Duration::minutes(15)).to_rfc3339()
    );
    assert_eq!(schedule[0]["scheduled_work_seconds"], 15 * 60);
    assert_eq!(schedule[0]["total_work_seconds"], 15 * 60);
    assert_eq!(schedule[0]["rank"], 0);
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn get_scheduleは引数なしで現在から次の業務日境界までの予定だけを返す() {
    let now = Local::now();
    let current_task = TaskHandle::new("current task").unwrap();
    current_task.set_start_time(now).unwrap();
    current_task.set_estimated_work_seconds(15 * 60).unwrap();

    let future_task = TaskHandle::new("future task").unwrap();
    future_task
        .set_start_time(get_next_morning_datetime(now) + Duration::hours(1))
        .unwrap();
    future_task.set_estimated_work_seconds(15 * 60).unwrap();

    let repository = RecordingRepository::new(vec![current_task, future_task]);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "default-range",
            "get_schedule",
            json!({}),
        ))
        .unwrap();

    let schedule = response["result"]["structuredContent"]["schedule"]
        .as_array()
        .unwrap();
    assert_eq!(schedule.len(), 1);
    assert_eq!(schedule[0]["task"]["name"], "current task");
}

#[test]
fn get_scheduleは日付範囲と片方の日付指定で重なる予定を返す() {
    let now = Local::now();
    let from_boundary = get_next_morning_datetime(now);
    let until_boundary = get_next_morning_datetime(from_boundary);

    let crossing_task = TaskHandle::new("crossing task").unwrap();
    crossing_task
        .set_start_time(from_boundary - Duration::minutes(30))
        .unwrap();
    crossing_task
        .set_estimated_work_seconds(2 * 60 * 60)
        .unwrap();

    let inside_task = TaskHandle::new("inside task").unwrap();
    inside_task
        .set_start_time(from_boundary + Duration::hours(3))
        .unwrap();
    inside_task.set_estimated_work_seconds(15 * 60).unwrap();

    let later_task = TaskHandle::new("later task").unwrap();
    later_task
        .set_start_time(until_boundary + Duration::hours(1))
        .unwrap();
    later_task.set_estimated_work_seconds(15 * 60).unwrap();

    let repository = RecordingRepository::new(vec![crossing_task, inside_task, later_task]);
    let mut server = initialized_server(repository);
    let from = from_boundary.format("%F").to_string();
    let until = until_boundary.format("%F").to_string();

    for (id, arguments) in [
        (
            "range",
            json!({"from": from.clone(), "until": until.clone()}),
        ),
        ("from-only", json!({"from": from.clone()})),
        ("until-only", json!({"until": until.clone()})),
    ] {
        let response = server
            .handle_request(tool_call_request(id, "get_schedule", arguments))
            .unwrap();
        let schedule = response["result"]["structuredContent"]["schedule"]
            .as_array()
            .unwrap();
        let names = schedule
            .iter()
            .map(|scheduled| scheduled["task"]["name"].as_str().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["crossing task", "inside task"]);
        assert_eq!(
            schedule[0]["scheduled_start"],
            (from_boundary - Duration::minutes(30)).to_rfc3339()
        );
    }
}

#[test]
fn get_schedule_予定なしを空配列で返す() {
    let repository = RecordingRepository::new(vec![]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "empty-schedule",
            "get_schedule",
            json!({}),
        ))
        .unwrap();

    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"]["schedule"],
        json!([])
    );
    assert_eq!(save_count.get(), 0);
    assert_eq!(mutation_count.get(), 0);
}

#[test]
fn get_schedule_schema違反をinvalid_paramsで返す() {
    let cases = [
        ("extra", json!({"extra": true}), "arguments.extra"),
        ("null", serde_json::Value::Null, "arguments"),
        ("invalid-from-type", json!({"from": true}), "from"),
    ];

    for (id, arguments, field) in cases {
        let repository = RecordingRepository::new(vec![]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);
        let response = server
            .handle_request(tool_call_request(id, "get_schedule", arguments))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        assert_eq!(response["error"]["code"], -32602);
        assert_eq!(response["error"]["message"], "Invalid params");
        assert_eq!(response["error"]["data"]["code"], "invalid_input");
        assert_eq!(response["error"]["data"]["field"], field);
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }
}

#[test]
fn get_scheduleは不正な日付範囲をinvalid_inputで返す() {
    let cases = [
        ("invalid-date", json!({"from": "2026-02-30"}), "from"),
        (
            "reversed-range",
            json!({"from": "2026-08-13", "until": "2026-08-12"}),
            "until",
        ),
    ];

    for (id, arguments, field) in cases {
        let repository = RecordingRepository::new(vec![]);
        let mut server = initialized_server(repository);
        let response = server
            .handle_request(tool_call_request(id, "get_schedule", arguments))
            .unwrap();

        assert_eq!(response["result"]["isError"], true);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            "invalid_input"
        );
        assert_eq!(
            response["result"]["structuredContent"]["error"]["field"],
            field
        );
    }
}

#[test]
fn create_task_作成して成功時に1回saveする() {
    let pending_until = fixed_now() + Duration::hours(18);
    let repository = RecordingRepository::new(vec![]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "create-task",
            "create_task",
            json!({
                "name": "created by MCP",
                "estimated_work_minutes": 30,
                "pending_until": pending_until.to_rfc3339()
            }),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "create-task");
    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    let task_id = response["result"]["structuredContent"]["task_id"]
        .as_str()
        .unwrap();
    assert!(Uuid::parse_str(task_id).is_ok());
    assert_eq!(save_count.get(), 1);
    assert_eq!(mutation_count.get(), 1);

    let created = server
        .handle_request(tool_call_request(
            "created-task",
            "get_task",
            json!({"task_id": task_id}),
        ))
        .unwrap();
    let task = &created["result"]["structuredContent"]["task"];
    assert_eq!(task["name"], "created by MCP");
    assert_eq!(task["estimated_work_seconds"], 30 * 60);
    assert_eq!(task["original_status"], "pending");
    assert_eq!(task["pending_until"], pending_until.to_rfc3339());
    assert_eq!(save_count.get(), 1);
    assert_eq!(mutation_count.get(), 1);
}

#[test]
fn create_task_schema違反ではtaskを作成せずsaveもしない() {
    let cases = [
        ("missing-name", json!({}), "name"),
        ("name-type", json!({"name": 1}), "name"),
        (
            "negative-estimate",
            json!({"name": "task", "estimated_work_minutes": -1}),
            "estimated_work_minutes",
        ),
        (
            "extra",
            json!({"name": "task", "extra": true}),
            "arguments.extra",
        ),
    ];

    for (id, arguments, field) in cases {
        let repository = RecordingRepository::new(vec![]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);
        let response = server
            .handle_request(tool_call_request(id, "create_task", arguments))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        assert_eq!(response["error"]["code"], -32602);
        assert_eq!(response["error"]["message"], "Invalid params");
        assert_eq!(response["error"]["data"]["code"], "invalid_input");
        assert_eq!(response["error"]["data"]["field"], field);
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }
}

#[test]
fn create_task_意味的不正では作成もsaveもしない() {
    let cases = [
        ("empty-name", json!({"name": "  "}), "name"),
        (
            "invalid-pending",
            json!({"name": "task", "pending_until": "not-a-date"}),
            "pending_until",
        ),
        (
            "estimate-overflow",
            json!({"name": "task", "estimated_work_minutes": i64::MAX}),
            "estimated_work_minutes",
        ),
        (
            "estimate-out-of-range",
            json!({"name": "task", "estimated_work_minutes": u64::MAX}),
            "estimated_work_minutes",
        ),
    ];

    for (id, arguments, field) in cases {
        let repository = RecordingRepository::new(vec![]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);
        let response = server
            .handle_request(tool_call_request(id, "create_task", arguments))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        assert_eq!(response["result"]["isError"], true);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            "invalid_input"
        );
        assert_eq!(
            response["result"]["structuredContent"]["error"]["field"],
            field
        );
        assert!(!response["result"]["structuredContent"]["error"]["message"]
            .as_str()
            .unwrap()
            .is_empty());
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }
}

#[test]
fn create_task_save失敗を成功扱いしない() {
    let repository = RecordingRepository::new(vec![]).with_save_failure();
    let load_count = Rc::clone(&repository.load_count);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let sync_clock_times = Rc::clone(&repository.sync_clock_times);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "save-failure",
            "create_task",
            json!({"name": "not persisted"}),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "save-failure");
    assert_eq!(response["result"]["isError"], true);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        "repository_save_failed"
    );
    assert!(!response["result"]["structuredContent"]["error"]["message"]
        .as_str()
        .unwrap()
        .is_empty());
    assert_eq!(save_count.get(), 1);
    assert_eq!(mutation_count.get(), 1);
    assert_eq!(sync_clock_times.borrow().len(), 1);
    assert_eq!(load_count.get(), 1);

    let poisoned_calls = [
        tool_call_request("read-after-save-failure", "list_tasks", json!({})),
        tool_call_request(
            "write-after-save-failure",
            "create_task",
            json!({"name": "must not be created"}),
        ),
        tool_call_request("unknown-after-save-failure", "unknown_tool", json!({})),
        tool_call_request("invalid-after-save-failure", "get_task", json!({})),
    ];
    for request in poisoned_calls {
        let expected_id = request["id"].clone();
        let response = server.handle_request(request).unwrap();
        assert_repository_state_uncertain_response(&response, &expected_id);
    }

    assert_eq!(save_count.get(), 1);
    assert_eq!(mutation_count.get(), 1);
    assert_eq!(sync_clock_times.borrow().len(), 1);
    assert_eq!(load_count.get(), 1);
}

#[test]
fn breakdown_task_子を入力順に追加して1回saveする() {
    let pending_until = fixed_now() + Duration::hours(18);
    let parent = TaskHandle::new("parent").unwrap();
    let parent_id = parent.get_id().unwrap();
    let repository = RecordingRepository::new(vec![parent]);
    let save_count = Rc::clone(&repository.save_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "breakdown",
            "breakdown_task",
            json!({
                "parent_id": parent_id.to_string(),
                "names": ["first child", "second child"],
                "pending_until": pending_until.to_rfc3339()
            }),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "breakdown");
    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    let child_ids = response["result"]["structuredContent"]["child_ids"]
        .as_array()
        .unwrap();
    assert_eq!(child_ids.len(), 2);
    assert!(child_ids
        .iter()
        .all(|id| Uuid::parse_str(id.as_str().unwrap()).is_ok()));
    assert_eq!(save_count.get(), 1);

    let parent_response = server
        .handle_request(tool_call_request(
            "parent-after-breakdown",
            "get_task",
            json!({"task_id": parent_id.to_string()}),
        ))
        .unwrap();
    assert_eq!(
        parent_response["result"]["structuredContent"]["task"]["child_ids"],
        serde_json::Value::Array(child_ids.clone())
    );
    for (index, expected_name) in ["first child", "second child"].iter().enumerate() {
        let child_response = server
            .handle_request(tool_call_request(
                "child-after-breakdown",
                "get_task",
                json!({"task_id": child_ids[index].as_str().unwrap()}),
            ))
            .unwrap();
        let child = &child_response["result"]["structuredContent"]["task"];
        assert_eq!(child["name"], *expected_name);
        assert_eq!(child["original_status"], "pending");
        assert_eq!(child["pending_until"], pending_until.to_rfc3339());
    }
    assert_eq!(save_count.get(), 1);
}

#[test]
fn breakdown_task_schema違反では親を変更しない() {
    let cases = [
        ("missing-parent", json!({"names": ["child"]}), "parent_id"),
        (
            "missing-names",
            json!({"parent_id": Uuid::new_v4().to_string()}),
            "names",
        ),
        (
            "empty-names",
            json!({"parent_id": Uuid::new_v4().to_string(), "names": []}),
            "names",
        ),
        (
            "empty-name",
            json!({"parent_id": Uuid::new_v4().to_string(), "names": [""]}),
            "names[0]",
        ),
        (
            "name-type",
            json!({"parent_id": Uuid::new_v4().to_string(), "names": [1]}),
            "names[0]",
        ),
    ];

    for (id, arguments, field) in cases {
        let repository = RecordingRepository::new(vec![]);
        let save_count = Rc::clone(&repository.save_count);
        let mutation_count = Rc::clone(&repository.mutation_count);
        let mut server = initialized_server(repository);
        let response = server
            .handle_request(tool_call_request(id, "breakdown_task", arguments))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        assert_eq!(response["error"]["code"], -32602);
        assert_eq!(response["error"]["message"], "Invalid params");
        assert_eq!(response["error"]["data"]["code"], "invalid_input");
        assert_eq!(response["error"]["data"]["field"], field);
        assert_eq!(save_count.get(), 0);
        assert_eq!(mutation_count.get(), 0);
    }
}

#[test]
fn breakdown_task_意味的不正と未知parentでは変更もsaveもしない() {
    let parent = TaskHandle::new("parent").unwrap();
    let parent_id = parent.get_id().unwrap();
    let cases = [
        (
            "invalid-parent-id",
            json!({"parent_id": "not-a-uuid", "names": ["child"]}),
            "invalid_input",
            "parent_id",
        ),
        (
            "invalid-pending",
            json!({
                "parent_id": parent_id.to_string(),
                "names": ["child"],
                "pending_until": "not-a-date"
            }),
            "invalid_input",
            "pending_until",
        ),
        (
            "invalid-name",
            json!({"parent_id": parent_id.to_string(), "names": ["  "]}),
            "invalid_input",
            "names",
        ),
        (
            "missing-parent",
            json!({"parent_id": Uuid::new_v4().to_string(), "names": ["child"]}),
            "task_not_found",
            "parent_id",
        ),
    ];

    for (id, arguments, code, field) in cases {
        let repository = RecordingRepository::new(vec![parent.clone()]);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);
        let response = server
            .handle_request(tool_call_request(id, "breakdown_task", arguments))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        assert_eq!(response["result"]["isError"], true);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            code
        );
        assert_eq!(
            response["result"]["structuredContent"]["error"]["field"],
            field
        );
        assert!(!response["result"]["structuredContent"]["error"]["message"]
            .as_str()
            .unwrap()
            .is_empty());
        assert_eq!(save_count.get(), 0);

        let parent_response = server
            .handle_request(tool_call_request(
                "parent-after-error",
                "get_task",
                json!({"task_id": parent_id.to_string()}),
            ))
            .unwrap();
        assert_eq!(
            parent_response["result"]["structuredContent"]["task"]["child_ids"],
            json!([])
        );
    }
}

#[test]
fn breakdown_task_save失敗を成功扱いしない() {
    let parent = TaskHandle::new("parent").unwrap();
    let parent_id = parent.get_id().unwrap();
    let parent_observer = parent.clone();
    let repository = RecordingRepository::new(vec![parent]).with_save_failure();
    let save_count = Rc::clone(&repository.save_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "breakdown-save-failure",
            "breakdown_task",
            json!({"parent_id": parent_id.to_string(), "names": ["child"]}),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "breakdown-save-failure");
    assert_eq!(response["result"]["isError"], true);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        "repository_save_failed"
    );
    assert!(!response["result"]["structuredContent"]["error"]["message"]
        .as_str()
        .unwrap()
        .is_empty());
    assert_eq!(save_count.get(), 1);
    assert_eq!(parent_observer.get_children().unwrap().len(), 1);

    let parent_response = server
        .handle_request(tool_call_request(
            "parent-after-save-failure",
            "get_task",
            json!({"task_id": parent_id.to_string()}),
        ))
        .unwrap();
    assert_repository_state_uncertain_response(
        &parent_response,
        &json!("parent-after-save-failure"),
    );
}

#[test]
fn defer_task_絶対時刻まで延期して1回saveする() {
    let pending_until = fixed_now() + Duration::hours(18);
    let task = TaskHandle::new("deferred task").unwrap();
    let task_id = task.get_id().unwrap();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "defer-task",
            "defer_task",
            json!({
                "task_id": task_id.to_string(),
                "pending_until": pending_until.to_rfc3339()
            }),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "defer-task");
    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"],
        json!({"task_id": task_id.to_string()})
    );
    assert_eq!(save_count.get(), 1);

    let task_response = server
        .handle_request(tool_call_request(
            "deferred-task",
            "get_task",
            json!({"task_id": task_id.to_string()}),
        ))
        .unwrap();
    let deferred = &task_response["result"]["structuredContent"]["task"];
    assert_eq!(deferred["original_status"], "pending");
    assert_eq!(deferred["pending_until"], pending_until.to_rfc3339());
    assert_eq!(save_count.get(), 1);
}

#[test]
fn defer_task_入力不正と未知taskでは変更もsaveもしない() {
    let task = TaskHandle::new("unchanged task").unwrap();
    let task_id = task.get_id().unwrap();
    let cases = [
        (
            "missing-task-id",
            json!({"pending_until": fixed_now().to_rfc3339()}),
            Some(-32602),
            "invalid_input",
            "task_id",
        ),
        (
            "missing-pending",
            json!({"task_id": task_id.to_string()}),
            Some(-32602),
            "invalid_input",
            "pending_until",
        ),
        (
            "task-id-type",
            json!({"task_id": 1, "pending_until": fixed_now().to_rfc3339()}),
            Some(-32602),
            "invalid_input",
            "task_id",
        ),
        (
            "pending-type",
            json!({"task_id": task_id.to_string(), "pending_until": 1}),
            Some(-32602),
            "invalid_input",
            "pending_until",
        ),
        (
            "extra",
            json!({
                "task_id": task_id.to_string(),
                "pending_until": fixed_now().to_rfc3339(),
                "extra": true
            }),
            Some(-32602),
            "invalid_input",
            "arguments.extra",
        ),
        (
            "invalid-task-id",
            json!({"task_id": "invalid", "pending_until": fixed_now().to_rfc3339()}),
            None,
            "invalid_input",
            "task_id",
        ),
        (
            "invalid-pending",
            json!({"task_id": task_id.to_string(), "pending_until": "invalid"}),
            None,
            "invalid_input",
            "pending_until",
        ),
        (
            "missing-task",
            json!({"task_id": Uuid::new_v4().to_string(), "pending_until": fixed_now().to_rfc3339()}),
            None,
            "task_not_found",
            "task_id",
        ),
    ];

    for (id, arguments, protocol_code, tool_code, field) in cases {
        let repository = RecordingRepository::new(vec![task.clone()]);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);
        let response = server
            .handle_request(tool_call_request(id, "defer_task", arguments))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        if let Some(protocol_code) = protocol_code {
            assert_eq!(response["error"]["code"], protocol_code);
            assert_eq!(response["error"]["message"], "Invalid params");
            assert_eq!(response["error"]["data"]["code"], tool_code);
            assert_eq!(response["error"]["data"]["field"], field);
        } else {
            assert_eq!(response["result"]["isError"], true);
            assert_tool_result_content_matches_structured(&response);
            assert_eq!(
                response["result"]["structuredContent"]["error"]["code"],
                tool_code
            );
            assert_eq!(
                response["result"]["structuredContent"]["error"]["field"],
                field
            );
            assert!(!response["result"]["structuredContent"]["error"]["message"]
                .as_str()
                .unwrap()
                .is_empty());
        }
        assert_eq!(save_count.get(), 0);

        let task_response = server
            .handle_request(tool_call_request(
                "task-after-defer-error",
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();
        let unchanged = &task_response["result"]["structuredContent"]["task"];
        assert_eq!(unchanged["original_status"], "todo");
        assert_eq!(unchanged["pending_until"], serde_json::Value::Null);
    }
}

#[test]
fn defer_task_save失敗を成功扱いしない() {
    let pending_until = fixed_now() + Duration::hours(18);
    let task = TaskHandle::new("deferred task").unwrap();
    let task_id = task.get_id().unwrap();
    let task_observer = task.clone();
    let repository = RecordingRepository::new(vec![task]).with_save_failure();
    let save_count = Rc::clone(&repository.save_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "defer-save-failure",
            "defer_task",
            json!({
                "task_id": task_id.to_string(),
                "pending_until": pending_until.to_rfc3339()
            }),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "defer-save-failure");
    assert_eq!(response["result"]["isError"], true);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        "repository_save_failed"
    );
    assert!(!response["result"]["structuredContent"]["error"]["message"]
        .as_str()
        .unwrap()
        .is_empty());
    assert_eq!(save_count.get(), 1);
    assert_eq!(task_observer.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(task_observer.get_pending_until().unwrap(), pending_until);

    let task_response = server
        .handle_request(tool_call_request(
            "deferred-after-save-failure",
            "get_task",
            json!({"task_id": task_id.to_string()}),
        ))
        .unwrap();
    assert_repository_state_uncertain_response(
        &task_response,
        &json!("deferred-after-save-failure"),
    );
}

#[test]
fn complete_task_完了と実績を反映して1回saveする() {
    let finished_at = fixed_now() + Duration::hours(1);
    let task = TaskHandle::new("completed task").unwrap();
    task.set_actual_work_seconds(60).unwrap();
    let task_id = task.get_id().unwrap();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "complete-task",
            "complete_task",
            json!({
                "task_id": task_id.to_string(),
                "finished_at": finished_at.to_rfc3339(),
                "additional_actual_work_seconds": 120
            }),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "complete-task");
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
    assert_eq!(save_count.get(), 1);

    let task_response = server
        .handle_request(tool_call_request(
            "completed-task",
            "get_task",
            json!({"task_id": task_id.to_string()}),
        ))
        .unwrap();
    let completed = &task_response["result"]["structuredContent"]["task"];
    assert_eq!(completed["original_status"], "done");
    assert_eq!(completed["end_time"], finished_at.to_rfc3339());
    assert_eq!(completed["actual_work_seconds"], 180);
    assert_eq!(save_count.get(), 1);
}

#[test]
fn complete_task_optional省略時は現在時刻と追加実績0を使う() {
    let task = TaskHandle::new("completed with defaults").unwrap();
    task.set_actual_work_seconds(60).unwrap();
    let task_id = task.get_id().unwrap();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mut server = initialized_server(repository);
    let before = Local::now();

    let response = server
        .handle_request(tool_call_request(
            "complete-defaults",
            "complete_task",
            json!({"task_id": task_id.to_string()}),
        ))
        .unwrap();
    let after = Local::now();

    assert_eq!(response["result"]["isError"], false);
    assert_eq!(save_count.get(), 1);
    let task_response = server
        .handle_request(tool_call_request(
            "completed-default-task",
            "get_task",
            json!({"task_id": task_id.to_string()}),
        ))
        .unwrap();
    let completed = &task_response["result"]["structuredContent"]["task"];
    let end_time = DateTime::parse_from_rfc3339(completed["end_time"].as_str().unwrap())
        .unwrap()
        .with_timezone(&Local);
    assert!(before <= end_time && end_time <= after);
    assert_eq!(completed["actual_work_seconds"], 60);
}

#[test]
fn complete_task_next_focusとnext_repetitionのuuidを返す() {
    let parent = TaskHandle::new("parent").unwrap();
    let child = parent.create_as_last_child(TaskAttr::new("only child"));
    let parent_id = parent.get_id().unwrap();
    let child_id = child.get_id().unwrap();
    let mut focus_server = initialized_server(RecordingRepository::new(vec![parent]));

    let focus_response = focus_server
        .handle_request(tool_call_request(
            "complete-for-focus",
            "complete_task",
            json!({"task_id": child_id.to_string(), "finished_at": fixed_now().to_rfc3339()}),
        ))
        .unwrap();

    assert_eq!(
        focus_response["result"]["structuredContent"]["next_focus_task_id"],
        parent_id.to_string()
    );
    assert_eq!(
        focus_response["result"]["structuredContent"]["next_repetition_task_id"],
        serde_json::Value::Null
    );

    let repetition_parent = TaskHandle::new("weekly").unwrap();
    repetition_parent
        .set_repetition_interval_days_opt(Some(7))
        .unwrap();
    let repetition_child =
        repetition_parent.create_as_last_child(TaskAttr::new("weekly occurrence"));
    let repetition_parent_id = repetition_parent.get_id().unwrap();
    let repetition_child_id = repetition_child.get_id().unwrap();
    let mut repetition_server =
        initialized_server(RecordingRepository::new(vec![repetition_parent]));

    let repetition_response = repetition_server
        .handle_request(tool_call_request(
            "complete-for-repetition",
            "complete_task",
            json!({
                "task_id": repetition_child_id.to_string(),
                "finished_at": fixed_now().to_rfc3339()
            }),
        ))
        .unwrap();

    let next_repetition_id = repetition_response["result"]["structuredContent"]
        ["next_repetition_task_id"]
        .as_str()
        .unwrap();
    assert!(Uuid::parse_str(next_repetition_id).is_ok());
    let parent_response = repetition_server
        .handle_request(tool_call_request(
            "repetition-parent",
            "get_task",
            json!({"task_id": repetition_parent_id.to_string()}),
        ))
        .unwrap();
    assert!(
        parent_response["result"]["structuredContent"]["task"]["child_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| id == next_repetition_id)
    );
}

#[test]
fn complete_task_未完了childと未知taskを区別してsaveしない() {
    let parent = TaskHandle::new("parent").unwrap();
    parent.create_as_last_child(TaskAttr::new("undone child"));
    let parent_id = parent.get_id().unwrap();
    let cases = [
        ("undone-child", parent_id, "has_undone_children", "task_id"),
        ("missing-task", Uuid::new_v4(), "task_not_found", "task_id"),
    ];

    for (id, task_id, code, field) in cases {
        let repository = RecordingRepository::new(vec![parent.clone()]);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);
        let response = server
            .handle_request(tool_call_request(
                id,
                "complete_task",
                json!({
                    "task_id": task_id.to_string(),
                    "finished_at": fixed_now().to_rfc3339()
                }),
            ))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        assert_eq!(response["result"]["isError"], true);
        assert_tool_result_content_matches_structured(&response);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            code
        );
        assert_eq!(
            response["result"]["structuredContent"]["error"]["field"],
            field
        );
        assert_eq!(
            response["result"]["structuredContent"]["error"]["task_id"],
            task_id.to_string()
        );
        assert!(!response["result"]["structuredContent"]["error"]["message"]
            .as_str()
            .unwrap()
            .is_empty());
        assert_eq!(save_count.get(), 0);
    }
}

#[test]
fn complete_task_入力不正では変更もsaveもしない() {
    let task = TaskHandle::new("unchanged task").unwrap();
    let task_id = task.get_id().unwrap();
    let cases = [
        ("missing-task-id", json!({}), Some(-32602), "task_id"),
        (
            "negative-work",
            json!({"task_id": task_id.to_string(), "additional_actual_work_seconds": -1}),
            Some(-32602),
            "additional_actual_work_seconds",
        ),
        (
            "invalid-task-id",
            json!({"task_id": "invalid"}),
            None,
            "task_id",
        ),
        (
            "invalid-finished-at",
            json!({"task_id": task_id.to_string(), "finished_at": "invalid"}),
            None,
            "finished_at",
        ),
        (
            "work-out-of-range",
            json!({"task_id": task_id.to_string(), "additional_actual_work_seconds": u64::MAX}),
            None,
            "additional_actual_work_seconds",
        ),
        (
            "extra",
            json!({"task_id": task_id.to_string(), "extra": true}),
            Some(-32602),
            "arguments.extra",
        ),
    ];

    for (id, arguments, protocol_code, field) in cases {
        let repository = RecordingRepository::new(vec![task.clone()]);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);
        let response = server
            .handle_request(tool_call_request(id, "complete_task", arguments))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        if let Some(protocol_code) = protocol_code {
            assert_eq!(response["error"]["code"], protocol_code);
            assert_eq!(response["error"]["message"], "Invalid params");
            assert_eq!(response["error"]["data"]["code"], "invalid_input");
            assert_eq!(response["error"]["data"]["field"], field);
        } else {
            assert_eq!(response["result"]["isError"], true);
            assert_tool_result_content_matches_structured(&response);
            assert_eq!(
                response["result"]["structuredContent"]["error"]["code"],
                "invalid_input"
            );
            assert_eq!(
                response["result"]["structuredContent"]["error"]["field"],
                field
            );
            assert!(!response["result"]["structuredContent"]["error"]["message"]
                .as_str()
                .unwrap()
                .is_empty());
        }
        assert_eq!(save_count.get(), 0);
        let task_response = server
            .handle_request(tool_call_request(
                &format!("{id}-unchanged"),
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();
        let unchanged = &task_response["result"]["structuredContent"]["task"];
        assert_eq!(unchanged["original_status"], "todo");
        assert_eq!(unchanged["end_time"], serde_json::Value::Null);
        assert_eq!(unchanged["actual_work_seconds"], 0);
    }
}

#[test]
fn complete_task_save失敗を成功扱いしない() {
    let task = TaskHandle::new("completed task").unwrap();
    let task_id = task.get_id().unwrap();
    let task_observer = task.clone();
    let repository = RecordingRepository::new(vec![task]).with_save_failure();
    let save_count = Rc::clone(&repository.save_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "complete-save-failure",
            "complete_task",
            json!({"task_id": task_id.to_string(), "finished_at": fixed_now().to_rfc3339()}),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "complete-save-failure");
    assert_eq!(response["result"]["isError"], true);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        "repository_save_failed"
    );
    assert!(!response["result"]["structuredContent"]["error"]["message"]
        .as_str()
        .unwrap()
        .is_empty());
    assert_eq!(save_count.get(), 1);
    assert_eq!(task_observer.get_orig_status().unwrap(), Status::Done);
    let task_response = server
        .handle_request(tool_call_request(
            "completed-after-save-failure",
            "get_task",
            json!({"task_id": task_id.to_string()}),
        ))
        .unwrap();
    assert_repository_state_uncertain_response(
        &task_response,
        &json!("completed-after-save-failure"),
    );
}

#[test]
fn update_task_指定fieldをまとめて更新して1回saveする() {
    let deadline = fixed_now() + Duration::days(10);
    let task = TaskHandle::new("updated task").unwrap();
    let task_id = task.get_id().unwrap();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "update-all-fields",
            "update_task",
            json!({
                "task_id": task_id.to_string(),
                "estimated_work_minutes": 45,
                "deadline_time": deadline.to_rfc3339(),
                "category": "recovery"
            }),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "update-all-fields");
    assert_eq!(response["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"],
        json!({"task_id": task_id.to_string()})
    );
    assert_eq!(save_count.get(), 1);

    let task_response = server
        .handle_request(tool_call_request(
            "updated-task",
            "get_task",
            json!({"task_id": task_id.to_string()}),
        ))
        .unwrap();
    let updated = &task_response["result"]["structuredContent"]["task"];
    assert_eq!(updated["estimated_work_seconds"], 45 * 60);
    assert_eq!(updated["deadline_time"], deadline.to_rfc3339());
    assert_eq!(updated["project_category"], "recovery");
    assert_eq!(save_count.get(), 1);
}

#[test]
fn update_task_同じ値ならsaveしない() {
    let task = TaskHandle::new("unchanged task").unwrap();
    let task_id = task.get_id().unwrap();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "update-with-same-value",
            "update_task",
            json!({
                "task_id": task_id.to_string(),
                "estimated_work_minutes": 15
            }),
        ))
        .unwrap();

    assert_eq!(response["result"]["isError"], false);
    assert_eq!(save_count.get(), 0);
}

#[test]
fn update_task_nullでdeadlineとcategoryを解除する() {
    let task = TaskHandle::new("cleared task").unwrap();
    task.set_deadline_time_opt(Some(fixed_now() + Duration::days(10)))
        .unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Investment))
        .unwrap();
    let task_id = task.get_id().unwrap();
    let repository = RecordingRepository::new(vec![task]);
    let save_count = Rc::clone(&repository.save_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "clear-fields",
            "update_task",
            json!({
                "task_id": task_id.to_string(),
                "deadline_time": null,
                "category": null
            }),
        ))
        .unwrap();

    assert_eq!(response["result"]["isError"], false);
    assert_eq!(save_count.get(), 1);
    let task_response = server
        .handle_request(tool_call_request(
            "cleared-task",
            "get_task",
            json!({"task_id": task_id.to_string()}),
        ))
        .unwrap();
    let cleared = &task_response["result"]["structuredContent"]["task"];
    assert_eq!(cleared["deadline_time"], serde_json::Value::Null);
    assert_eq!(cleared["project_category"], serde_json::Value::Null);
}

#[test]
fn update_task_schemaで公開した全categoryを設定できる() {
    for category in [
        "recovery",
        "investment",
        "consumption",
        "earning",
        "sustaining",
    ] {
        let task = TaskHandle::new("categorized task").unwrap();
        let task_id = task.get_id().unwrap();
        let mut server = initialized_server(RecordingRepository::new(vec![task]));

        let response = server
            .handle_request(tool_call_request(
                &format!("set-{category}"),
                "update_task",
                json!({"task_id": task_id.to_string(), "category": category}),
            ))
            .unwrap();

        assert_eq!(response["result"]["isError"], false);
        let task_response = server
            .handle_request(tool_call_request(
                &format!("get-{category}"),
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();
        assert_eq!(
            task_response["result"]["structuredContent"]["task"]["project_category"],
            category
        );
    }
}

#[test]
fn update_task_入力不正では変更もsaveもしない() {
    let deadline = fixed_now() + Duration::days(10);
    let task = TaskHandle::new("unchanged update task").unwrap();
    task.set_estimated_work_seconds(30 * 60).unwrap();
    task.set_deadline_time_opt(Some(deadline)).unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Consumption))
        .unwrap();
    let task_id = task.get_id().unwrap();
    let cases = [
        (
            "no-update-field",
            json!({"task_id": task_id.to_string()}),
            Some(-32602),
            "arguments",
        ),
        (
            "missing-task-id",
            json!({"category": null}),
            Some(-32602),
            "task_id",
        ),
        (
            "negative-estimate",
            json!({"task_id": task_id.to_string(), "estimated_work_minutes": -1}),
            Some(-32602),
            "estimated_work_minutes",
        ),
        (
            "invalid-category",
            json!({"task_id": task_id.to_string(), "category": "unknown"}),
            Some(-32602),
            "category",
        ),
        (
            "extra",
            json!({"task_id": task_id.to_string(), "category": null, "extra": true}),
            Some(-32602),
            "arguments.extra",
        ),
        (
            "invalid-task-id",
            json!({"task_id": "invalid", "category": null}),
            None,
            "task_id",
        ),
        (
            "invalid-deadline",
            json!({"task_id": task_id.to_string(), "deadline_time": "invalid"}),
            None,
            "deadline_time",
        ),
        (
            "estimate-out-of-range",
            json!({"task_id": task_id.to_string(), "estimated_work_minutes": u64::MAX}),
            None,
            "estimated_work_minutes",
        ),
    ];

    for (id, arguments, protocol_code, field) in cases {
        let repository = RecordingRepository::new(vec![task.clone()]);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);
        let response = server
            .handle_request(tool_call_request(id, "update_task", arguments))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        if let Some(protocol_code) = protocol_code {
            assert_eq!(response["error"]["code"], protocol_code);
            assert_eq!(response["error"]["message"], "Invalid params");
            assert_eq!(response["error"]["data"]["code"], "invalid_input");
            assert_eq!(response["error"]["data"]["field"], field);
        } else {
            assert_eq!(response["result"]["isError"], true);
            assert_tool_result_content_matches_structured(&response);
            assert_eq!(
                response["result"]["structuredContent"]["error"]["code"],
                "invalid_input"
            );
            assert_eq!(
                response["result"]["structuredContent"]["error"]["field"],
                field
            );
            assert!(!response["result"]["structuredContent"]["error"]["message"]
                .as_str()
                .unwrap()
                .is_empty());
        }
        assert_eq!(save_count.get(), 0);
        let task_response = server
            .handle_request(tool_call_request(
                &format!("{id}-unchanged"),
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();
        let unchanged = &task_response["result"]["structuredContent"]["task"];
        assert_eq!(unchanged["estimated_work_seconds"], 30 * 60);
        assert_eq!(unchanged["deadline_time"], deadline.to_rfc3339());
        assert_eq!(unchanged["project_category"], "consumption");
    }
}

#[test]
fn update_task_application_errorと未知taskでは変更もsaveもしない() {
    let original_deadline = fixed_now() + Duration::days(10);
    let requested_deadline = fixed_now() + Duration::days(20);
    let task = TaskHandle::new("unchanged application task").unwrap();
    task.set_estimated_work_seconds(30 * 60).unwrap();
    task.set_deadline_time_opt(Some(original_deadline)).unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Consumption))
        .unwrap();
    let task_id = task.get_id().unwrap();
    let missing_task_id = Uuid::new_v4();
    let cases = [
        (
            "estimate-overflow",
            task_id,
            json!({
                "task_id": task_id.to_string(),
                "estimated_work_minutes": i64::MAX,
                "deadline_time": requested_deadline.to_rfc3339(),
                "category": "investment"
            }),
            "invalid_input",
            "estimated_work_minutes",
        ),
        (
            "missing-task",
            missing_task_id,
            json!({"task_id": missing_task_id.to_string(), "category": "investment"}),
            "task_not_found",
            "task_id",
        ),
    ];

    for (id, requested_task_id, arguments, code, field) in cases {
        let repository = RecordingRepository::new(vec![task.clone()]);
        let save_count = Rc::clone(&repository.save_count);
        let mut server = initialized_server(repository);
        let response = server
            .handle_request(tool_call_request(id, "update_task", arguments))
            .unwrap();

        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id);
        assert_eq!(response["result"]["isError"], true);
        assert_tool_result_content_matches_structured(&response);
        let error = &response["result"]["structuredContent"]["error"];
        assert_eq!(error["code"], code);
        assert_eq!(error["field"], field);
        assert!(!error["message"].as_str().unwrap().is_empty());
        if code == "task_not_found" {
            assert_eq!(error["task_id"], requested_task_id.to_string());
        }
        assert_eq!(save_count.get(), 0);
        let task_response = server
            .handle_request(tool_call_request(
                &format!("{id}-unchanged"),
                "get_task",
                json!({"task_id": task_id.to_string()}),
            ))
            .unwrap();
        let unchanged = &task_response["result"]["structuredContent"]["task"];
        assert_eq!(unchanged["estimated_work_seconds"], 30 * 60);
        assert_eq!(unchanged["deadline_time"], original_deadline.to_rfc3339());
        assert_eq!(unchanged["project_category"], "consumption");
    }
}

#[test]
fn update_task_save失敗を成功扱いしない() {
    let task = TaskHandle::new("updated before save failure").unwrap();
    let task_id = task.get_id().unwrap();
    let task_observer = task.clone();
    let repository = RecordingRepository::new(vec![task]).with_save_failure();
    let save_count = Rc::clone(&repository.save_count);
    let mut server = initialized_server(repository);

    let response = server
        .handle_request(tool_call_request(
            "update-save-failure",
            "update_task",
            json!({"task_id": task_id.to_string(), "estimated_work_minutes": 20}),
        ))
        .unwrap();

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], "update-save-failure");
    assert_eq!(response["result"]["isError"], true);
    assert_tool_result_content_matches_structured(&response);
    assert_eq!(
        response["result"]["structuredContent"]["error"]["code"],
        "repository_save_failed"
    );
    assert!(!response["result"]["structuredContent"]["error"]["message"]
        .as_str()
        .unwrap()
        .is_empty());
    assert_eq!(save_count.get(), 1);
    assert_eq!(task_observer.get_estimated_work_seconds().unwrap(), 20 * 60);
    let task_response = server
        .handle_request(tool_call_request(
            "updated-after-save-failure",
            "get_task",
            json!({"task_id": task_id.to_string()}),
        ))
        .unwrap();
    assert_repository_state_uncertain_response(
        &task_response,
        &json!("updated-after-save-failure"),
    );
}
