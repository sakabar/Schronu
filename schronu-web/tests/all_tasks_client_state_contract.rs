use schronu_web::client::state::{
    load_client_state, AllTasksStatus, ClientEffect, ListSelection, Outcome,
    ServerActionInvocation, ServerFailure,
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
        },
        segment_index,
        schedule_date: "2026-09-05".to_owned(),
        deadline_epoch_ms: None,
        deadline_label: "____/__/__".to_owned(),
        misses_deadline: false,
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

    assert_eq!(
        state.apply_all_tasks_result(second_id, second_request, Ok(page(vec![all_row(2)], None)),),
        ClientEffect::None
    );
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
    assert_eq!(state.history().back().unwrap().outcome, Outcome::Failure);

    let _ = state.select_logical_date("2026-09-06");
    assert_eq!(state.list_selection(), ListSelection::Date);
    assert_eq!(state.display_error(), None);

    let (_, retry) = all_effect(state.retry_all_tasks());
    assert_eq!(retry.cursor, None);
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Loading);
}

#[test]
fn 日付選択中もall取得を継続し古いresponseは履歴だけに残す() {
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
    state.apply_all_tasks_result(
        request_id,
        request.clone(),
        Ok(page(vec![all_row(1)], None)),
    );
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Invalidated);
    let history = state.history().back().unwrap();
    assert_eq!(
        history.invocation,
        ServerActionInvocation::ListAllTasks(request)
    );
    assert_eq!(history.outcome, Outcome::Success);
    assert!(history.summary.contains("古い応答"));
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

    state.apply_all_tasks_result(request_id, request, Ok(page(vec![all_row(0)], None)));
    assert_eq!(state.all_tasks_status(), AllTasksStatus::Invalidated);
    assert_eq!(state.all_task_rows(), None);

    let (_, retry) = all_effect(state.select_all_tasks());
    assert_eq!(retry.cursor, None);
}
