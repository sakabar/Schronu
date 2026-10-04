use schronu_web::{
    web_error_codes, AllTaskPage, AllTaskRow, BandDay, BandDurations, CompleteSessionRequest,
    CompleteSessionResponse, CompletedTaskReport, CompletedTaskRow, DeadlineDisplayKind, DeferMode,
    DeferPlan, DeferTaskRequest, ListAllTasksRequest, ListCompletedTasksRequest, ListTasksRequest,
    LoadData, RecordSessionRequest, RecordSessionResult, RetryAdvice, RoutineLoadReport,
    RoutineLoadRow, ScheduleOccurrence, ScheduledTaskRow, ServerSnapshot, SessionTask,
    TaskDisplayKind, WebError, WebSuccess,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;

#[test]
fn seven_operationsのrequestとsuccessは仕様どおりのjson形式を持つ() {
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
        task: task.clone().into(),
        occurrence: ScheduleOccurrence::Actual {
            task_id: task.task_id.clone(),
        },
        schedule_start_epoch_ms: 1_788_565_500_000,
        schedule_end_epoch_ms: 1_788_566_100_000,
        deadline_epoch_ms: Some(1_788_566_400_000),
        deadline_label: "____-00:05".to_owned(),
        misses_deadline: false,
        task_display_kind: TaskDisplayKind::Fixed,
        deadline_display_kind: DeadlineDisplayKind::Future,
        is_leaf: true,
        defer_plan: Some(DeferPlan {
            mode: DeferMode::DeadlineLimited,
            requested_pending_until_epoch_ms: 1_788_650_400_000,
            effective_pending_until_epoch_ms: Some(1_788_566_100_000),
            repetition_interval_days: None,
        }),
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
        &DeferTaskRequest {
            task_id: "00000000-0000-0000-0000-000000000001".to_owned(),
            selected_logical_date: "2026-09-05".to_owned(),
            expected_plan: DeferPlan {
                mode: DeferMode::DeadlineLimited,
                requested_pending_until_epoch_ms: 1_788_600_000_000_i64,
                effective_pending_until_epoch_ms: Some(1_788_599_000_000_i64),
                repetition_interval_days: None,
            },
        },
        json!({
            "task_id": "00000000-0000-0000-0000-000000000001",
            "selected_logical_date": "2026-09-05",
            "expected_plan": {
                "mode": "deadline_limited",
                "requested_pending_until_epoch_ms": 1_788_600_000_000_i64,
                "effective_pending_until_epoch_ms": 1_788_599_000_000_i64
            }
        }),
    );
    assert_json_round_trip(
        &ListTasksRequest {
            logical_date: "2026-09-05".to_owned(),
        },
        json!({"logical_date": "2026-09-05"}),
    );
    assert_json_round_trip(
        &ListAllTasksRequest {
            cursor: Some("00000000-0000-4000-8000-000000000001:500".to_owned()),
        },
        json!({"cursor": "00000000-0000-4000-8000-000000000001:500"}),
    );
    assert_json_round_trip(
        &WebSuccess {
            snapshot: snapshot.clone(),
            data: AllTaskPage {
                rows: vec![AllTaskRow {
                    task: task.clone().into(),
                    occurrence: ScheduleOccurrence::Actual {
                        task_id: task.task_id.clone(),
                    },
                    segment_index: 0,
                    schedule_date: "2026-09-05".to_owned(),
                    deadline_epoch_ms: None,
                    deadline_label: "____/__/__".to_owned(),
                    misses_deadline: false,
                    task_display_kind: TaskDisplayKind::NonRepetitive,
                    deadline_display_kind: DeadlineDisplayKind::None,
                    is_leaf: true,
                }],
                next_cursor: None,
            },
        },
        json!({
            "snapshot": {
                "observed_at_epoch_ms": 1_788_565_500_123_i64,
                "logical_date": "2026-09-05",
                "buffer_seconds": -61
            },
            "data": {
                "rows": [{
                    "task": {
                        "task_id": "00000000-0000-0000-0000-000000000001",
                        "task_name": "wire task",
                        "estimated_work_seconds": 900,
                        "actual_work_seconds": 300
                    },
                    "occurrence": {
                        "kind": "actual",
                        "task_id": "00000000-0000-0000-0000-000000000001"
                    },
                    "segment_index": 0,
                    "schedule_date": "2026-09-05",
                    "deadline_epoch_ms": null,
                    "deadline_label": "____/__/__",
                    "misses_deadline": false,
                    "task_display_kind": "non_repetitive",
                    "deadline_display_kind": "none",
                    "is_leaf": true
                }],
                "next_cursor": null
            }
        }),
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
                "occurrence": {
                    "kind": "actual",
                    "task_id": "00000000-0000-0000-0000-000000000001"
                },
                "schedule_start_epoch_ms": 1_788_565_500_000_i64,
                "schedule_end_epoch_ms": 1_788_566_100_000_i64,
                "deadline_epoch_ms": 1_788_566_400_000_i64,
                "deadline_label": "____-00:05",
                "misses_deadline": false,
                "task_display_kind": "fixed",
                "deadline_display_kind": "future",
                "is_leaf": true,
                "defer_plan": {
                    "mode": "deadline_limited",
                    "requested_pending_until_epoch_ms": 1_788_650_400_000_i64,
                    "effective_pending_until_epoch_ms": 1_788_566_100_000_i64
                }
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
        ended_at_epoch_ms: Some(1_788_565_560_000),
        expected_actual_work_seconds: 300,
    };
    assert_json_round_trip(
        &mutation_request,
        json!({
            "task_id": "00000000-0000-0000-0000-000000000001",
            "started_at_epoch_ms": 1_788_565_500_000_i64,
            "ended_at_epoch_ms": 1_788_565_560_000_i64,
            "expected_actual_work_seconds": 300
        }),
    );
    assert_json_round_trip(
        &CompleteSessionRequest {
            task_id: mutation_request.task_id.clone(),
            started_at_epoch_ms: mutation_request.started_at_epoch_ms,
            ended_at_epoch_ms: mutation_request.ended_at_epoch_ms,
            expected_actual_work_seconds: mutation_request.expected_actual_work_seconds,
            record_elapsed_seconds: false,
        },
        json!({
            "task_id": "00000000-0000-0000-0000-000000000001",
            "started_at_epoch_ms": 1_788_565_500_000_i64,
            "ended_at_epoch_ms": 1_788_565_560_000_i64,
            "expected_actual_work_seconds": 300,
            "record_elapsed_seconds": false
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
fn completed_reportのrequestとsuccessは公開json_shapeを保持する() {
    assert_json_round_trip(
        &ListCompletedTasksRequest {
            logical_date: "2026-09-05".to_owned(),
        },
        json!({"logical_date": "2026-09-05"}),
    );

    assert_json_round_trip(
        &WebSuccess {
            snapshot: ServerSnapshot {
                observed_at_epoch_ms: 1_788_565_500_123,
                logical_date: "2026-09-05".to_owned(),
                buffer_seconds: -61,
            },
            data: CompletedTaskReport {
                rows: vec![CompletedTaskRow {
                    task_id: "00000000-0000-0000-0000-000000000001".to_owned(),
                    task_name: "wire task".to_owned(),
                    project_name: "wire project".to_owned(),
                    completed_at_epoch_ms: 1_788_565_499_999,
                    actual_work_seconds: 901,
                    estimated_work_seconds: 900,
                    task_display_kind: Default::default(),
                }],
                total_actual_work_seconds: 901,
                available_seconds: 43_200,
                recorded_percentage: Some(2),
            },
        },
        json!({
            "snapshot": {
                "observed_at_epoch_ms": 1_788_565_500_123_i64,
                "logical_date": "2026-09-05",
                "buffer_seconds": -61
            },
            "data": {
                "rows": [{
                    "task_id": "00000000-0000-0000-0000-000000000001",
                    "task_name": "wire task",
                    "project_name": "wire project",
                    "completed_at_epoch_ms": 1_788_565_499_999_i64,
                    "actual_work_seconds": 901,
                    "estimated_work_seconds": 900,
                    "task_display_kind": "non_repetitive"
                }],
                "total_actual_work_seconds": 901,
                "available_seconds": 43_200,
                "recorded_percentage": 2
            }
        }),
    );
}

#[test]
fn 旧completed_row_payloadはtask種別を単発として補完する() {
    let row: CompletedTaskRow = serde_json::from_value(json!({
        "task_id": "00000000-0000-0000-0000-000000000001",
        "task_name": "legacy task",
        "project_name": "legacy project",
        "completed_at_epoch_ms": 1_788_565_499_999_i64,
        "actual_work_seconds": 901,
        "estimated_work_seconds": 900
    }))
    .unwrap();

    assert_eq!(
        serde_json::to_value(row).unwrap()["task_display_kind"],
        json!("non_repetitive")
    );
}

#[test]
fn band_dayは日付と累積差分と区分秒数をjsonで保持する() {
    let day = BandDay {
        logical_date: "2026-09-05".to_owned(),
        accumulated_rho_diff_seconds: 60,
        accumulated_free_diff_seconds: -120,
        durations: BandDurations {
            unavailable_seconds: 1,
            elapsed_seconds: 2,
            repetitive_seconds: 3,
            non_repetitive_seconds: 4,
            rho_leeway_seconds: 5,
        },
    };

    assert_json_round_trip(
        &day,
        json!({
            "logical_date": "2026-09-05",
            "accumulated_rho_diff_seconds": 60,
            "accumulated_free_diff_seconds": -120,
            "durations": {
                "unavailable_seconds": 1,
                "elapsed_seconds": 2,
                "repetitive_seconds": 3,
                "non_repetitive_seconds": 4,
                "rho_leeway_seconds": 5
            }
        }),
    );
}

#[test]
fn load_dataは7日帯と28日繰返負荷を同じpayloadで保持する() {
    let data = LoadData {
        band_days: Vec::new(),
        routine_load: RoutineLoadReport {
            start_date: "2026-10-03".to_owned(),
            end_date: "2026-10-30".to_owned(),
            horizon_day_count: 28,
            rows: vec![RoutineLoadRow {
                project_task_id: "project-id".to_owned(),
                project_name: "生活".to_owned(),
                routine_task_id: "routine-id".to_owned(),
                routine_name: "週次家事".to_owned(),
                repetition_interval_days: 7,
                total_work_seconds: 15_600,
                occurrence_day_count: 4,
            }],
        },
    };

    assert_json_round_trip(
        &data,
        json!({
            "band_days": [],
            "routine_load": {
                "start_date": "2026-10-03",
                "end_date": "2026-10-30",
                "horizon_day_count": 28,
                "rows": [{
                    "project_task_id": "project-id",
                    "project_name": "生活",
                    "routine_task_id": "routine-id",
                    "routine_name": "週次家事",
                    "repetition_interval_days": 7,
                    "total_work_seconds": 15_600,
                    "occurrence_day_count": 4
                }]
            }
        }),
    );
}

#[test]
fn 旧一覧payloadは表示分類fieldがなくてもdeserializeできる() {
    let scheduled: ScheduledTaskRow = serde_json::from_value(json!({
        "task": {
            "task_id": "00000000-0000-0000-0000-000000000001",
            "task_name": "legacy",
            "estimated_work_seconds": 1,
            "actual_work_seconds": 0
        },
        "schedule_start_epoch_ms": 1,
        "schedule_end_epoch_ms": 2,
        "deadline_epoch_ms": 1,
        "deadline_label": "+00:01____",
        "misses_deadline": true,
        "is_leaf": true,
        "defer_plan": {
            "mode": "normal",
            "requested_pending_until_epoch_ms": 3
        }
    }))
    .unwrap();
    assert_eq!(scheduled.task_display_kind, TaskDisplayKind::NonRepetitive);
    assert_eq!(scheduled.deadline_display_kind, DeadlineDisplayKind::None);
    assert!(scheduled.misses_deadline);
    assert_eq!(scheduled.occurrence, ScheduleOccurrence::LegacyActual);
    assert_eq!(
        scheduled.task.task_id.as_deref(),
        Some("00000000-0000-0000-0000-000000000001")
    );
}

#[test]
fn projected一覧payloadはactionable_task_idを持たず安定identityを保持する() {
    let projected: ScheduledTaskRow = serde_json::from_value(json!({
        "task": {
            "task_name": "筋トレ(9/8)",
            "estimated_work_seconds": 900,
            "actual_work_seconds": 0
        },
        "occurrence": {
            "kind": "projected",
            "occurrence_key": "parent:1788876000000",
            "source_task_id": "00000000-0000-0000-0000-000000000010"
        },
        "schedule_start_epoch_ms": 1_788_876_000_000_i64,
        "schedule_end_epoch_ms": 1_788_876_900_000_i64,
        "deadline_epoch_ms": 1_788_879_600_000_i64,
        "deadline_label": "____-00:45",
        "misses_deadline": false,
        "task_display_kind": "repetitive",
        "deadline_display_kind": "today",
        "is_leaf": true
    }))
    .unwrap();

    assert!(projected.task.task_id.is_none());
    assert!(projected.task.actionable_task().is_none());
    assert!(projected.defer_plan.is_none());
    assert_eq!(
        projected.occurrence.occurrence_key(),
        Some("parent:1788876000000")
    );
}

#[test]
fn 終了時刻がない旧mutation_requestをdeserializeできる() {
    let record: RecordSessionRequest = serde_json::from_value(json!({
        "task_id": "00000000-0000-0000-0000-000000000001",
        "started_at_epoch_ms": 1_788_565_500_000_i64,
        "expected_actual_work_seconds": 300
    }))
    .unwrap();
    assert_eq!(record.ended_at_epoch_ms, None);

    let complete: CompleteSessionRequest = serde_json::from_value(json!({
        "task_id": "00000000-0000-0000-0000-000000000001",
        "started_at_epoch_ms": 1_788_565_500_000_i64,
        "expected_actual_work_seconds": 300,
        "record_elapsed_seconds": true
    }))
    .unwrap();
    assert_eq!(complete.ended_at_epoch_ms, None);
}

#[test]
fn error_codeとretry_adviceはsnake_case文字列として往復する() {
    let cases = [
        web_error_codes::INVALID_CURSOR,
        web_error_codes::INVALID_INPUT,
        web_error_codes::TASK_NOT_FOUND,
        web_error_codes::TASK_ALREADY_COMPLETED,
        web_error_codes::ACTUAL_WORK_CONFLICT,
        web_error_codes::DEFER_PLAN_CHANGED,
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
            current_actual_work_seconds: None,
        },
        json!({
            "code": "worker_unavailable",
            "message": "Web worker is unavailable",
            "retry_advice": "retry"
        }),
    );
}

#[test]
fn 実績競合errorだけが現在の実績時間をjsonへ公開する() {
    assert_json_round_trip(
        &WebError {
            code: web_error_codes::ACTUAL_WORK_CONFLICT.to_owned(),
            message: "セッションカードで実績時間を確認してください。".to_owned(),
            retry_advice: RetryAdvice::ManualCheck,
            current_actual_work_seconds: Some(420),
        },
        json!({
            "code": "actual_work_conflict",
            "message": "セッションカードで実績時間を確認してください。",
            "retry_advice": "manual_check",
            "current_actual_work_seconds": 420
        }),
    );
}

#[test]
fn 現在実績のない旧error_payloadをdeserializeできる() {
    let legacy_error = json!({
        "code": "worker_unavailable",
        "message": "Web worker is unavailable",
        "retry_advice": "retry"
    });

    let decoded: WebError = serde_json::from_value(legacy_error.clone()).unwrap();

    assert_eq!(decoded.current_actual_work_seconds, None);
    assert_eq!(serde_json::to_value(&decoded).unwrap(), legacy_error);
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
