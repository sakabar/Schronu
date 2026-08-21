use super::output::{scheduled_task_view_json, task_view_json};
use super::test_support::json_fixture;
use crate::application::schedule_use_case::ScheduledTaskView;
use crate::application::task_use_case::TaskView;
use crate::entity::task::{ProjectCategory, RepetitionAnchor, Status};
use chrono::{Local, TimeZone};
use serde_json::json;
use uuid::Uuid;

#[test]
fn task_viewのserde表現は既存mcp_json契約と一致する() {
    let task_id = Uuid::parse_str("44ecb80c-5107-4b1c-b8e2-88db0d39ed56").unwrap();
    let root_id = Uuid::parse_str("63039331-0cbb-4426-ac85-76ce34f0be7f").unwrap();
    let child_id = Uuid::parse_str("6201d826-9bd7-4b5f-93cc-f3f4e545c881").unwrap();
    let pending_until = Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap();
    let create_time = Local.with_ymd_and_hms(2026, 8, 1, 9, 0, 0).unwrap();
    let start_time = Local.with_ymd_and_hms(2026, 8, 10, 10, 0, 0).unwrap();
    let deadline_time = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
    let task = TaskView {
        id: task_id,
        root_id,
        parent_id: None,
        child_ids: vec![child_id],
        name: "MCP task".to_string(),
        status: Status::Done,
        original_status: Status::Pending,
        is_on_other_side: true,
        atomic: true,
        pending_until: Some(pending_until),
        priority: 7,
        create_time,
        start_time,
        end_time: None,
        deadline_time: Some(deadline_time),
        estimated_work_seconds: 1_800,
        actual_work_seconds: 900,
        repetition_interval_days: Some(7),
        repetition_anchor: RepetitionAnchor::Completion,
        days_in_advance: 2,
        project_category: Some(ProjectCategory::Recovery),
    };
    let mut expected = json_fixture(
        include_str!("../../../tests/fixtures/mcp/task-view.json"),
        &[
            ("{{task_id}}", &task_id.to_string()),
            ("{{child_id}}", &child_id.to_string()),
            ("{{pending_until}}", &pending_until.to_rfc3339()),
            ("{{create_time}}", &create_time.to_rfc3339()),
            ("{{start_time}}", &start_time.to_rfc3339()),
            ("{{deadline_time}}", &deadline_time.to_rfc3339()),
        ],
    );
    expected["root_id"] = json!(root_id.to_string());
    expected["status"] = json!("done");

    let serialized = serde_json::to_value(&task).unwrap();

    assert_eq!(serialized, task_view_json(&task));
    assert_eq!(serialized, expected);
}

#[test]
fn scheduled_task_viewのserde表現はnested_taskを含む既存mcp_json契約と一致する() {
    let task_id = Uuid::parse_str("13d302b4-9660-4783-afef-77181ff690f5").unwrap();
    let root_id = Uuid::parse_str("33258548-4f9d-441d-91fa-0302a3343035").unwrap();
    let create_time = Local.with_ymd_and_hms(2026, 8, 1, 9, 0, 0).unwrap();
    let start_time = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let scheduled_start = Local.with_ymd_and_hms(2026, 8, 11, 13, 0, 0).unwrap();
    let first_available_time = Local.with_ymd_and_hms(2026, 8, 11, 12, 30, 0).unwrap();
    let scheduled_end = Local.with_ymd_and_hms(2026, 8, 11, 13, 15, 0).unwrap();
    let scheduled = ScheduledTaskView {
        task: TaskView {
            id: task_id,
            root_id,
            parent_id: None,
            child_ids: vec![],
            name: "scheduled task".to_string(),
            status: Status::Pending,
            original_status: Status::Todo,
            is_on_other_side: false,
            atomic: false,
            pending_until: None,
            priority: 5,
            create_time,
            start_time,
            end_time: None,
            deadline_time: None,
            estimated_work_seconds: 900,
            actual_work_seconds: 0,
            repetition_interval_days: None,
            repetition_anchor: RepetitionAnchor::Deadline,
            days_in_advance: 0,
            project_category: None,
        },
        first_available_time,
        scheduled_start,
        scheduled_end,
        scheduled_work_seconds: 900,
        total_work_seconds: 1_800,
        rank: 0,
    };
    let mut expected = json_fixture(
        include_str!("../../../tests/fixtures/mcp/scheduled-task-view.json"),
        &[
            ("{{task_id}}", &task_id.to_string()),
            ("{{create_time}}", &create_time.to_rfc3339()),
            ("{{start_time}}", &start_time.to_rfc3339()),
            ("{{scheduled_start}}", &scheduled_start.to_rfc3339()),
            ("{{scheduled_end}}", &scheduled_end.to_rfc3339()),
        ],
    );
    expected["first_available_time"] = json!(first_available_time.to_rfc3339());
    expected["total_work_seconds"] = json!(1_800);
    expected["task"]["root_id"] = json!(root_id.to_string());
    expected["task"]["status"] = json!("pending");

    let serialized = serde_json::to_value(&scheduled).unwrap();

    assert_eq!(serialized, scheduled_task_view_json(&scheduled));
    assert_eq!(serialized, expected);
}

#[test]
fn viewで公開するenumのserde表現は全variantでlowercaseになる() {
    assert_eq!(
        serde_json::to_value([Status::Todo, Status::Pending, Status::Done]).unwrap(),
        json!(["todo", "pending", "done"])
    );
    assert_eq!(
        serde_json::to_value([RepetitionAnchor::Deadline, RepetitionAnchor::Completion]).unwrap(),
        json!(["deadline", "completion"])
    );
    assert_eq!(
        serde_json::to_value([
            ProjectCategory::Earning,
            ProjectCategory::Sustaining,
            ProjectCategory::Recovery,
            ProjectCategory::Investment,
            ProjectCategory::Consumption,
        ])
        .unwrap(),
        json!([
            "earning",
            "sustaining",
            "recovery",
            "investment",
            "consumption"
        ])
    );
}
