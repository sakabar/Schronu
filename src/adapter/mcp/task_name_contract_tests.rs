use super::test_support::{
    initialized_server, new_task_handle, tool_call_request, RecordingRepository,
};
use crate::application::interface::TaskRepositoryTrait;
use serde_json::{json, Value};
use std::rc::Rc;

const ALLOWED_NAME: &str = "  日本語  'single' \"double\" C:\\path  ";
const INVALID_NAMES: &[(&str, &str)] = &[
    (" \u{3000} ", "must not be blank"),
    (" -42 ", "must not be an integer-only name"),
    ("tab\tname", "must not contain control characters"),
    ("line\nname", "must not contain control characters"),
    ("escape\u{1b}name", "must not contain control characters"),
    ("c1\u{85}name", "must not contain control characters"),
];

#[test]
fn mcpのcreateとbreakdownは許可名をapplicationまで原文で渡す() {
    let parent = new_task_handle("parent").unwrap();
    let parent_id = parent.get_id().unwrap();
    let repository = RecordingRepository::new(vec![parent]);
    let mut server = initialized_server(repository);

    let create_response = server
        .handle_request(tool_call_request(
            "create-allowed",
            "create_task",
            json!({"name": ALLOWED_NAME}),
        ))
        .unwrap();
    assert_eq!(create_response["result"]["isError"], false);

    let breakdown_response = server
        .handle_request(tool_call_request(
            "breakdown-allowed",
            "breakdown_task",
            json!({"parent_id": parent_id.to_string(), "names": [ALLOWED_NAME]}),
        ))
        .unwrap();
    assert_eq!(breakdown_response["result"]["isError"], false);

    let project_names = server
        .repository
        .get_all_projects()
        .into_iter()
        .map(|project| project.get_name().unwrap())
        .collect::<Vec<_>>();
    assert!(project_names.iter().any(|name| name == ALLOWED_NAME));

    let parent = server.repository.get_by_id(parent_id).unwrap().unwrap();
    let child_names = parent
        .get_children()
        .unwrap()
        .into_iter()
        .map(|child| child.get_name().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(child_names, vec![ALLOWED_NAME]);
}

#[test]
fn mcpのcreateとbreakdownはcanonical_errorを保持して変更しない() {
    for (case_index, (name, reason)) in INVALID_NAMES.iter().enumerate() {
        assert_create_rejected(case_index, name, reason);
        assert_breakdown_rejected(case_index, name, reason);
    }
}

fn assert_create_rejected(case_index: usize, name: &str, reason: &str) {
    let repository = RecordingRepository::new(vec![]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut server = initialized_server(repository);
    let response = server
        .handle_request(tool_call_request(
            &format!("create-invalid-{case_index}"),
            "create_task",
            json!({"name": name}),
        ))
        .unwrap();

    assert_tool_error(&response, "name", reason);
    assert_eq!(save_count.get(), 0, "name={name:?}");
    assert_eq!(mutation_count.get(), 0, "name={name:?}");
    assert!(server.repository.get_all_projects().is_empty());
}

fn assert_breakdown_rejected(case_index: usize, name: &str, reason: &str) {
    let parent = new_task_handle("parent").unwrap();
    let parent_id = parent.get_id().unwrap();
    let repository = RecordingRepository::new(vec![parent]);
    let save_count = Rc::clone(&repository.save_count);
    let mutation_count = Rc::clone(&repository.mutation_count);
    let mut server = initialized_server(repository);
    let response = server
        .handle_request(tool_call_request(
            &format!("breakdown-invalid-{case_index}"),
            "breakdown_task",
            json!({"parent_id": parent_id.to_string(), "names": [name]}),
        ))
        .unwrap();

    assert_tool_error(&response, "names", reason);
    assert_eq!(save_count.get(), 0, "name={name:?}");
    assert_eq!(mutation_count.get(), 0, "name={name:?}");
    let parent = server.repository.get_by_id(parent_id).unwrap().unwrap();
    assert!(parent.get_children().unwrap().is_empty(), "name={name:?}");
}

fn assert_tool_error(response: &Value, field: &str, reason: &str) {
    assert_eq!(response["result"]["isError"], true);
    let error = &response["result"]["structuredContent"]["error"];
    assert_eq!(error["code"], "invalid_input");
    assert_eq!(error["field"], field);
    assert_eq!(error["message"], reason);
    assert_eq!(
        response["result"]["content"][0]["text"],
        response["result"]["structuredContent"].to_string()
    );
}
