use schronu_web::{
    web_error_codes, CompleteSessionResponse, ListTasksRequest, RecordSessionRequest,
    RecordSessionResult, RetryAdvice, ScheduledTaskRow, ServerSnapshot, SessionTask, WebError,
    WebSuccess,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;

#[test]
fn five_operationsのrequestとsuccessは仕様どおりのjson形式を持つ() {
    let snapshot = ServerSnapshot {
        observed_at_epoch_ms: 1_788_565_500_123,
        logical_date: "2026-09-05".to_owned(),
        buffer_seconds: -61,
    };
    let task = SessionTask {
        task_id: "00000000-0000-0000-0000-000000000001".to_owned(),
        task_name: "wire task".to_owned(),
        estimated_work_seconds: 900,
        actual_work_seconds: 300,
    };
    let row = ScheduledTaskRow {
        task: task.clone(),
        schedule_start_epoch_ms: 1_788_565_500_000,
        schedule_end_epoch_ms: 1_788_566_100_000,
        deadline_epoch_ms: Some(1_788_566_400_000),
        is_leaf: true,
    };

    assert_json_round_trip(
        &snapshot,
        json!({
            "observed_at_epoch_ms": 1_788_565_500_123_i64,
            "logical_date": "2026-09-05",
            "buffer_seconds": -61
        }),
    );
    assert_json_round_trip(
        &ListTasksRequest {
            logical_date: "2026-09-05".to_owned(),
        },
        json!({"logical_date": "2026-09-05"}),
    );
    assert_json_round_trip(
        &WebSuccess {
            snapshot: snapshot.clone(),
            data: vec![row],
        },
        json!({
            "snapshot": {
                "observed_at_epoch_ms": 1_788_565_500_123_i64,
                "logical_date": "2026-09-05",
                "buffer_seconds": -61
            },
            "data": [{
                "task": {
                    "task_id": "00000000-0000-0000-0000-000000000001",
                    "task_name": "wire task",
                    "estimated_work_seconds": 900,
                    "actual_work_seconds": 300
                },
                "schedule_start_epoch_ms": 1_788_565_500_000_i64,
                "schedule_end_epoch_ms": 1_788_566_100_000_i64,
                "deadline_epoch_ms": 1_788_566_400_000_i64,
                "is_leaf": true
            }]
        }),
    );
    assert_json_round_trip(
        &WebSuccess {
            snapshot: snapshot.clone(),
            data: Some(task),
        },
        json!({
            "snapshot": {
                "observed_at_epoch_ms": 1_788_565_500_123_i64,
                "logical_date": "2026-09-05",
                "buffer_seconds": -61
            },
            "data": {
                "task_id": "00000000-0000-0000-0000-000000000001",
                "task_name": "wire task",
                "estimated_work_seconds": 900,
                "actual_work_seconds": 300
            }
        }),
    );

    let mutation_request = RecordSessionRequest {
        task_id: "00000000-0000-0000-0000-000000000001".to_owned(),
        started_at_epoch_ms: 1_788_565_500_000,
        expected_actual_work_seconds: 300,
    };
    assert_json_round_trip(
        &mutation_request,
        json!({
            "task_id": "00000000-0000-0000-0000-000000000001",
            "started_at_epoch_ms": 1_788_565_500_000_i64,
            "expected_actual_work_seconds": 300
        }),
    );
    assert_json_round_trip(
        &WebSuccess {
            snapshot: snapshot.clone(),
            data: RecordSessionResult {
                actual_work_seconds: 361,
            },
        },
        json!({
            "snapshot": {
                "observed_at_epoch_ms": 1_788_565_500_123_i64,
                "logical_date": "2026-09-05",
                "buffer_seconds": -61
            },
            "data": {"actual_work_seconds": 361}
        }),
    );

    let complete_response: CompleteSessionResponse = snapshot;
    assert_json_round_trip(
        &complete_response,
        json!({
            "observed_at_epoch_ms": 1_788_565_500_123_i64,
            "logical_date": "2026-09-05",
            "buffer_seconds": -61
        }),
    );
}

#[test]
fn error_codeとretry_adviceはsnake_case文字列として往復する() {
    let cases = [
        web_error_codes::INVALID_INPUT,
        web_error_codes::TASK_NOT_FOUND,
        web_error_codes::TASK_ALREADY_COMPLETED,
        web_error_codes::ACTUAL_WORK_CONFLICT,
        web_error_codes::ARITHMETIC_OVERFLOW,
        web_error_codes::TASK_NOT_COMPLETABLE,
        web_error_codes::CONFIGURATION_ERROR,
        web_error_codes::REPOSITORY_UNAVAILABLE,
        web_error_codes::OPERATION_FAILED,
        web_error_codes::WORKER_UNAVAILABLE,
        web_error_codes::REPOSITORY_SAVE_FAILED,
        web_error_codes::REPOSITORY_STATE_UNCERTAIN,
    ];

    for code in cases {
        assert_json_round_trip(&code.to_owned(), json!(code));
    }
    assert_json_round_trip(&RetryAdvice::Retry, json!("retry"));
    assert_json_round_trip(&RetryAdvice::ManualCheck, json!("manual_check"));

    assert_json_round_trip(
        &WebError {
            code: web_error_codes::WORKER_UNAVAILABLE.to_owned(),
            message: "Web worker is unavailable".to_owned(),
            retry_advice: RetryAdvice::Retry,
        },
        json!({
            "code": "worker_unavailable",
            "message": "Web worker is unavailable",
            "retry_advice": "retry"
        }),
    );
}

#[test]
fn 未知のweb_error_codeもerror全体を壊さず保持する() {
    let unknown_error = json!({
        "code": "future_server_error",
        "message": "A newer server reported an unknown error",
        "retry_advice": "manual_check"
    });

    let decoded: WebError = serde_json::from_value(unknown_error.clone()).unwrap();

    assert_eq!(serde_json::to_value(&decoded).unwrap(), unknown_error);
    assert_eq!(decoded.message, "A newer server reported an unknown error");
    assert_eq!(decoded.retry_advice, RetryAdvice::ManualCheck);
}

fn assert_json_round_trip<T>(value: &T, expected_json: serde_json::Value)
where
    T: Clone + std::fmt::Debug + Eq + Serialize + DeserializeOwned,
{
    let encoded = serde_json::to_value(value).unwrap();
    assert_eq!(encoded, expected_json);
    let decoded = serde_json::from_value(encoded).unwrap();
    assert_eq!(*value, decoded);
}
