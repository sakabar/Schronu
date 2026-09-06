use super::test_support::{
    assert_tool_result_content_matches_structured, initialized_server, new_task_attr,
    new_task_handle, tool_call_request, RecordingRepository, Uuid,
};
use serde_json::{json, Value};
use std::collections::HashSet;

fn call_list_tasks(
    server: &mut super::McpServer<RecordingRepository>,
    id: &str,
    arguments: Value,
) -> Value {
    server
        .handle_request(tool_call_request(id, "list_tasks", arguments))
        .unwrap()
}

fn task_names(response: &Value) -> Vec<&str> {
    response["result"]["structuredContent"]["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["name"].as_str().unwrap())
        .collect()
}

#[test]
fn list_tasksはcardinality境界ごとに全pageを重複欠損なく連結する() {
    let cases = [
        (0, vec![0]),
        (1, vec![1]),
        (100, vec![100]),
        (101, vec![100, 1]),
        (500, vec![100, 100, 100, 100, 100]),
    ];

    for (count, expected_page_lengths) in cases {
        let projects = (0..count)
            .map(|index| new_task_handle(&format!("boundary-task-{index:03}")).unwrap())
            .collect();
        let repository = RecordingRepository::new(projects);
        let mut server = initialized_server(repository);
        let mut cursor = None;
        let mut actual_page_lengths = Vec::new();
        let mut actual_names = Vec::new();

        loop {
            let arguments = cursor
                .as_ref()
                .map_or_else(|| json!({}), |cursor| json!({"cursor": cursor}));
            let response = call_list_tasks(&mut server, "boundary-page", arguments);
            let content = &response["result"]["structuredContent"];
            let tasks = content["tasks"].as_array().unwrap();
            actual_page_lengths.push(tasks.len());
            actual_names.extend(
                tasks
                    .iter()
                    .map(|task| task["name"].as_str().unwrap().to_string()),
            );
            cursor = content["next_cursor"].as_str().map(str::to_string);
            if cursor.is_none() {
                assert_eq!(content["next_cursor"], Value::Null);
                break;
            }
        }

        let expected_names = (0..count)
            .map(|index| format!("boundary-task-{index:03}"))
            .collect::<Vec<_>>();
        assert_eq!(actual_page_lengths, expected_page_lengths, "count={count}");
        assert_eq!(actual_names, expected_names, "count={count}");
        assert_eq!(
            actual_names.iter().collect::<HashSet<_>>().len(),
            count,
            "count={count}"
        );
    }
}

#[test]
fn list_tasksは既定100件を返しcursorだけを引き継いでlimitを変更できる() {
    let projects = (0..103)
        .map(|index| new_task_handle(&format!("task-{index:03}")).unwrap())
        .collect();
    let repository = RecordingRepository::new(projects);
    let mut server = initialized_server(repository);

    let first = call_list_tasks(&mut server, "first", json!({}));
    assert_eq!(first["result"]["isError"], false);
    assert_tool_result_content_matches_structured(&first);
    assert_eq!(task_names(&first).len(), 100);
    assert_eq!(task_names(&first).first(), Some(&"task-000"));
    assert_eq!(task_names(&first).last(), Some(&"task-099"));
    let cursor = first["result"]["structuredContent"]["next_cursor"]
        .as_str()
        .unwrap();

    let second = call_list_tasks(&mut server, "second", json!({"limit": 5, "cursor": cursor}));
    assert_eq!(
        task_names(&second),
        vec!["task-100", "task-101", "task-102"]
    );
    assert_eq!(
        second["result"]["structuredContent"]["next_cursor"],
        Value::Null
    );
}

#[test]
fn list_tasksはqueryとroot_subtreeを組み合わせunboundedだけが全件を返す() {
    let root = new_task_handle("root").unwrap();
    let root_id = root.get_id().unwrap();
    root.create_as_last_child(new_task_attr("Keep child"));
    root.create_as_last_child(new_task_attr("skip child"));
    let outside = new_task_handle("Keep outside").unwrap();
    let repository = RecordingRepository::new(vec![root, outside]);
    let mut server = initialized_server(repository);

    let filtered = call_list_tasks(
        &mut server,
        "filtered",
        json!({
            "query": "KEEP",
            "root_task_id": root_id.to_string(),
            "unbounded": true
        }),
    );

    assert_eq!(task_names(&filtered), vec!["Keep child"]);
    assert_eq!(
        filtered["result"]["structuredContent"]["next_cursor"],
        Value::Null
    );
}

#[test]
fn list_tasksはunbounded競合とcursor不正を既存invalid_inputへ写像する() {
    let repository = RecordingRepository::new(vec![new_task_handle("task").unwrap()]);
    let mut server = initialized_server(repository);
    let cases = [
        (
            "unbounded-limit",
            json!({"unbounded": true, "limit": 1}),
            "unbounded",
        ),
        (
            "unbounded-cursor",
            json!({"unbounded": true, "cursor": "opaque"}),
            "unbounded",
        ),
        ("malformed-cursor", json!({"cursor": "opaque"}), "cursor"),
    ];

    for (id, arguments, field) in cases {
        let response = call_list_tasks(&mut server, id, arguments);
        assert_eq!(response["result"]["isError"], true);
        assert_tool_result_content_matches_structured(&response);
        let error = &response["result"]["structuredContent"]["error"];
        assert_eq!(error["code"], "invalid_input");
        assert_eq!(error["field"], field);
    }
}

#[test]
fn list_tasksは不存在rootを既存task_not_foundへ写像する() {
    let missing_id = Uuid::new_v4();
    let repository = RecordingRepository::new(vec![]);
    let mut server = initialized_server(repository);

    let response = call_list_tasks(
        &mut server,
        "missing-root",
        json!({"root_task_id": missing_id.to_string()}),
    );

    assert_eq!(response["result"]["isError"], true);
    assert_tool_result_content_matches_structured(&response);
    let error = &response["result"]["structuredContent"]["error"];
    assert_eq!(error["code"], "task_not_found");
    assert_eq!(error["field"], "root_task_id");
    assert_eq!(error["task_id"], missing_id.to_string());
}
