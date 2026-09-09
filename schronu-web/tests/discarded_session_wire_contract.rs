use schronu_web::{DiscardSessionRequest, DiscardedSessionDay, DiscardedSessionEvent, DiscardedSessionTaskTotal, ListDiscardedSessionsRequest, WebSuccess, ServerSnapshot};

#[test]
fn discard_writeと日次readのwire_shapeを固定する() {
    let request = DiscardSessionRequest {
        event_id: "00000000-0000-0000-0000-000000000001".to_owned(),
        task_id: "00000000-0000-0000-0000-000000000002".to_owned(),
        task_name_at_start: "task".to_owned(),
        started_at_epoch_ms: 1_000,
        ended_at_epoch_ms: 61_000,
    };
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["event_id"], request.event_id);
    assert_eq!(json["task_name_at_start"], "task");
    assert_eq!(serde_json::to_value(ListDiscardedSessionsRequest { logical_date: "2026-09-10".to_owned() }).unwrap()["logical_date"], "2026-09-10");

    let response = WebSuccess {
        snapshot: ServerSnapshot { observed_at_epoch_ms: 61_000, logical_date: "2026-09-10".to_owned(), buffer_seconds: 0 },
        data: DiscardedSessionDay {
            logical_date: "2026-09-10".to_owned(), total_seconds: 60,
            task_totals: vec![DiscardedSessionTaskTotal { task_id: request.task_id.clone(), task_name: "task".to_owned(), total_seconds: 60 }],
            events: vec![DiscardedSessionEvent { event_id: request.event_id, task_id: request.task_id, task_name_at_start: "task".to_owned(), started_at_epoch_ms: 1_000, ended_at_epoch_ms: 61_000, elapsed_seconds: 60, source: "web".to_owned(), reason: "web_discard_release".to_owned() }],
        },
    };
    let json = serde_json::to_value(response).unwrap();
    assert_eq!(json["data"]["total_seconds"], 60);
    assert_eq!(json["data"]["events"][0]["elapsed_seconds"], 60);
}
