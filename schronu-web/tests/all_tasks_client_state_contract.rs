use schronu_web::client::state::{
    load_client_state, AllTasksStatus, ClientEffect, ListSelection, ServerFailure,
};
use schronu_web::{
    AllTaskPage, AllTaskRow, DeferMode, DeferPlan, ListAllTasksRequest, SessionTask, WebSuccess,
};

mod client_state_support;
use client_state_support::{row, snapshot, FakeStorage, TASK_ID};

fn all_row(segment_index: usize) -> AllTaskRow {
    AllTaskRow {
        task: SessionTask {
            task_id: TASK_ID.to_owned(),
            task_name: format!("task {segment_index}"),
            estimated_work_seconds: 900,
            actual_work_seconds: 0,
        }
        .into(),
        display_task_name: None,
        occurrence: schronu_web::ScheduleOccurrence::Actual {
            task_id: TASK_ID.to_owned(),
        },
        segment_index,
        schedule_date: "2026-09-05".to_owned(),
        deadline_epoch_ms: None,
        deadline_label: "____/__/__".to_owned(),
        misses_deadline: false,
        task_display_kind: schronu_web::TaskDisplayKind::NonRepetitive,
        deadline_display_kind: schronu_web::DeadlineDisplayKind::None,
        is_leaf: true,
    }
}

fn all_effect(effect: ClientEffect) -> (u64, ListAllTasksRequest) {
    match effect {
        ClientEffect::ListAllTasks {
            request_id,
            request,
        } => (request_id, request),
        other => panic!("unexpected effect: {other:?}"),
    }
}

fn page(rows: Vec<AllTaskRow>, next_cursor: Option<&str>) -> WebSuccess<AllTaskPage> {
    WebSuccess {
        snapshot: snapshot("2026-09-05", 1),
        data: AllTaskPage {
            rows,
            next_cursor: next_cursor.map(str::to_owned),
        },
    }
}

#[test]
fn all一覧は全page成功まで行を公開せず順序どおりatomicに公開する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();

    assert_eq!(state.list_selection(), ListSelection::Date);
    assert_eq!(state.all_tasks_status(), AllTasksStatus::NotLoaded);
    assert_eq!(state.all_task_rows(), None);

    let (first_id, first_request) = all_effect(state.select_all_tasks());
    assert_eq!(state.list_selection(), ListSelection::All);
    assert_eq!(first_request.cursor, None);
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Loading);
    assert_eq!(state.all_task_rows(), None);
    assert_eq!(state.select_all_tasks(), ClientEffect::None);

    let follow_up = state.apply_all_tasks_result(
        first_id,
        first_request,
        Ok(page(vec![all_row(0), all_row(1)], Some("snapshot:2"))),
    );
    let (second_id, second_request) = all_effect(follow_up);
    assert_eq!(second_request.cursor.as_deref(), Some("snapshot:2"));
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Loading);
    assert_eq!(state.all_task_rows(), None);

    assert!(matches!(
        state.apply_all_tasks_result(second_id, second_request, Ok(page(vec![all_row(2)], None))),
        ClientEffect::LoadBand { .. }
    ));
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Loaded);
    assert_eq!(
        state
            .all_task_rows()
            .unwrap()
            .iter()
            .map(|row| row.segment_index)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(state.select_all_tasks(), ClientEffect::None);
}

#[test]
fn all一覧は失敗時に途中行を破棄しretryを先頭から開始する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    let (first_id, first_request) = all_effect(state.select_all_tasks());
    let (second_id, second_request) = all_effect(state.apply_all_tasks_result(
        first_id,
        first_request,
        Ok(page(vec![all_row(0)], Some("snapshot:1"))),
    ));

    state.apply_all_tasks_result(
        second_id,
        second_request,
        Err(ServerFailure::Transport("offline".to_owned())),
    );
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Failed);
    assert_eq!(state.all_task_rows(), None);
    assert_eq!(state.display_error(), None);
    assert_eq!(
        state.all_tasks_failure_message(),
        Some("通信に失敗しました。時間をおいて再試行してください。")
    );

    let _ = state.select_logical_date("2026-09-06");
    assert_eq!(state.list_selection(), ListSelection::Date);
    assert_eq!(state.display_error(), None);

    let (_, retry) = all_effect(state.retry_all_tasks());
    assert_eq!(retry.cursor, None);
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Loading);
}

#[test]
fn 日付選択中もall取得を継続し古いresponseは無視する() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    let (request_id, request) = all_effect(state.select_all_tasks());

    let date_effect = state.select_logical_date("2026-09-06");
    assert!(matches!(date_effect, ClientEffect::ListTasks { .. }));
    assert_eq!(state.list_selection(), ListSelection::Date);
    state.apply_all_tasks_result(
        request_id,
        request.clone(),
        Ok(page(vec![all_row(0)], None)),
    );
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Loaded);
    assert_eq!(state.list_selection(), ListSelection::Date);
    assert_eq!(state.select_all_tasks(), ClientEffect::None);

    state.select_logical_date("2026-09-06");
    let defer = state.request_defer_task(
        &storage,
        TASK_ID,
        "2026-09-06",
        DeferPlan {
            mode: DeferMode::Normal,
            requested_pending_until_epoch_ms: 1_000,
            effective_pending_until_epoch_ms: None,
            repetition_interval_days: None,
        },
    );
    let defer_id = match defer {
        ClientEffect::DeferTask { request_id, .. } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    };
    let follow_up =
        state.apply_defer_task_result(&storage, defer_id, Ok(snapshot("2026-09-05", 2)));
    let (list_id, list_request) = client_state_support::list_effect(follow_up);
    assert!(matches!(
        state.apply_list_result(
            list_id,
            &list_request.logical_date,
            Ok(WebSuccess {
                snapshot: snapshot("2026-09-05", 3),
                data: Vec::new(),
            })
        ),
        ClientEffect::LoadBand { .. }
    ));
    state.apply_all_tasks_result(
        request_id,
        request.clone(),
        Ok(page(vec![all_row(1)], None)),
    );
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Invalidated);
}

#[test]
fn local_session操作は取得済みall一覧を無効化しない() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    let (request_id, request) = all_effect(state.select_all_tasks());
    state.apply_all_tasks_result(request_id, request, Ok(page(vec![all_row(0)], None)));

    state.add_session_from_row(&storage, &row(TASK_ID, 0));
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Loaded);
    state.tick(1_000);
    state.restart_session_without_recording(&storage, TASK_ID);
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Loaded);
    state.discard_session(&storage, TASK_ID);
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Loaded);
    assert_eq!(state.all_task_rows().unwrap(), [all_row(0)]);
}

#[test]
fn mutation成功は取得中allを無効化して遅延responseを適用しない() {
    let storage = FakeStorage::default();
    let mut state = load_client_state(&storage, 0).unwrap();
    let (request_id, request) = all_effect(state.select_all_tasks());
    let defer = state.request_defer_task(
        &storage,
        TASK_ID,
        "2026-09-06",
        DeferPlan {
            mode: DeferMode::Normal,
            requested_pending_until_epoch_ms: 1_000,
            effective_pending_until_epoch_ms: None,
            repetition_interval_days: None,
        },
    );
    let defer_id = match defer {
        ClientEffect::DeferTask { request_id, .. } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    };
    state.apply_defer_task_result(&storage, defer_id, Ok(snapshot("2026-09-05", 2)));
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Invalidated);

    assert_eq!(
        state.apply_all_tasks_result(request_id, request, Ok(page(vec![all_row(0)], None))),
        ClientEffect::None
    );
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Invalidated);
    assert_eq!(state.all_task_rows(), None);

    let (_, retry) = all_effect(state.select_all_tasks());
    assert_eq!(retry.cursor, None);
}

#[test]
fn mutation後は日付別取得の成否にかかわらず選択中allを自動再取得する() {
    use client_state_support::{
        complete_effect, defer_effect, list_effect, record_effect, state_with_sessions,
    };
    use schronu_web::client::state::{ActiveTab, DisplayError, Operation};
    use schronu_web::RecordSessionResult;

    for operation in ["record", "complete", "complete_without_recording", "defer"] {
        for fail_list in [false, true] {
            let storage = FakeStorage::default();
            let mut state = if operation == "defer" {
                load_client_state(&storage, 0).unwrap()
            } else {
                state_with_sessions(&storage, &[TASK_ID])
            };
            let (old_id, old_request) = all_effect(state.select_all_tasks());
            state.apply_all_tasks_result(
                old_id,
                old_request.clone(),
                Ok(page(vec![all_row(0)], None)),
            );
            state.switch_tab(ActiveTab::Session);
            let follow_up = match operation {
                "record" => {
                    let (id, _) = record_effect(state.begin_record_session(&storage, TASK_ID));
                    state.apply_record_result(
                        &storage,
                        id,
                        Ok(WebSuccess {
                            snapshot: snapshot("2026-09-05", 2),
                            data: RecordSessionResult {
                                actual_work_seconds: 100,
                            },
                        }),
                    )
                }
                "defer" => {
                    let (id, _) = defer_effect(state.request_defer_task(
                        &storage,
                        TASK_ID,
                        "2026-09-05",
                        DeferPlan {
                            mode: DeferMode::Normal,
                            requested_pending_until_epoch_ms: 1_000,
                            effective_pending_until_epoch_ms: None,
                            repetition_interval_days: None,
                        },
                    ));
                    state.apply_defer_task_result(&storage, id, Ok(snapshot("2026-09-05", 2)))
                }
                _ => {
                    let effect = if operation == "complete" {
                        state.begin_complete_session(&storage, TASK_ID)
                    } else {
                        state.begin_complete_session_without_recording(&storage, TASK_ID)
                    };
                    let (id, _) = complete_effect(effect);
                    state.apply_complete_result(&storage, id, Ok(snapshot("2026-09-05", 2)))
                }
            };
            assert_eq!(state.all_tasks_status(), AllTasksStatus::Invalidated);
            let (list_id, list_request) = list_effect(follow_up);
            let result = if fail_list {
                Err(ServerFailure::Transport("offline".to_owned()))
            } else {
                Ok(WebSuccess {
                    snapshot: snapshot("2026-09-05", 3),
                    data: Vec::new(),
                })
            };
            let (first_id, first_request) =
                all_effect(state.apply_list_result(list_id, &list_request.logical_date, result));
            assert_eq!(first_request.cursor, None);
            assert_eq!(state.active_tab(), ActiveTab::Session);
            assert_eq!(state.all_tasks_status(), AllTasksStatus::Loading);
            assert_eq!(
                state.apply_list_result(
                    list_id,
                    &list_request.logical_date,
                    Err(ServerFailure::Transport("duplicate".to_owned()))
                ),
                ClientEffect::None
            );
            assert_eq!(
                state.apply_all_tasks_result(old_id, old_request, Ok(page(vec![all_row(9)], None))),
                ClientEffect::None
            );
            let (second_id, second_request) = all_effect(state.apply_all_tasks_result(
                first_id,
                first_request,
                Ok(page(vec![all_row(1)], Some("new:1"))),
            ));
            assert_eq!(state.all_task_rows(), None);
            assert!(matches!(
                state.apply_all_tasks_result(
                    second_id,
                    second_request,
                    Ok(page(vec![all_row(2)], None))
                ),
                ClientEffect::LoadBand { .. }
            ));
            assert_eq!(state.all_task_rows().unwrap(), [all_row(1), all_row(2)]);
            assert_eq!(state.list_selection(), ListSelection::All);
            assert_eq!(state.display_error().is_some(), fail_list);
            if fail_list {
                assert!(matches!(
                    state.display_error(),
                    Some(DisplayError::Transport {
                        operation: Operation::ListTasks,
                        ..
                    })
                ));
            }
        }
    }
}
