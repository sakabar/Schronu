use chrono::{Datelike, NaiveDate};
use schronu_web::client::date_buttons::logical_date_buttons_for_mode;
use schronu_web::client::date_input::{resolve_date_input_for_mode, DateInputError};
use schronu_web::client::state::{ActiveTab, ClientEffect, ListMode, ServerFailure};
use schronu_web::{CompletedTaskReport, CompletedTaskRow, WebSuccess};

mod client_state_support;
use client_state_support::*;

fn completed_row(task_id: &str, task_name: &str) -> CompletedTaskRow {
    CompletedTaskRow {
        task_id: task_id.to_owned(),
        task_name: task_name.to_owned(),
        project_name: "Schronu".to_owned(),
        completed_at_epoch_ms: 1_789_551_723_000,
        actual_work_seconds: 120,
        estimated_work_seconds: 60,
    }
}

fn completed_report(task_id: &str, task_name: &str) -> CompletedTaskReport {
    CompletedTaskReport {
        rows: vec![completed_row(task_id, task_name)],
        total_actual_work_seconds: 120,
        available_seconds: 600,
        recorded_percentage: Some(20),
    }
}

fn completed_effect(effect: ClientEffect) -> (u64, schronu_web::ListCompletedTasksRequest) {
    match effect {
        ClientEffect::ListCompletedTasks {
            request_id,
            request,
        } => (request_id, request),
        other => panic!("completed effect expected: {other:?}"),
    }
}

#[test]
fn list_modeは予定をdefaultにして切替先endpointを1回選ぶ() {
    let storage = FakeStorage::default();
    let mut state = schronu_web::client::state::load_client_state(&storage, 0).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-16", 1)));
    assert_eq!(state.list_mode(), ListMode::Scheduled);

    let (completed_id, request) = completed_effect(state.switch_list_mode(ListMode::Completed));
    assert_eq!(request.logical_date, "2026-09-16");
    assert_eq!(state.list_mode(), ListMode::Completed);
    state.apply_completed_list_result(
        completed_id,
        &request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-16", 2),
            data: completed_report(TASK_ID, "完了task"),
        }),
    );
    assert_eq!(state.completed_rows().len(), 1);
    assert_eq!(
        state.completed_report().unwrap().recorded_percentage,
        Some(20)
    );

    let (_, scheduled_request) = list_effect(state.switch_list_mode(ListMode::Scheduled));
    assert_eq!(scheduled_request.logical_date, "2026-09-16");
}

#[test]
fn 別tabから一覧へ戻ると選択日を維持して予定modeを取得する() {
    for other_tab in [ActiveTab::Session, ActiveTab::Load, ActiveTab::History] {
        let storage = FakeStorage::default();
        let mut state = schronu_web::client::state::load_client_state(&storage, 0).unwrap();
        let bootstrap_id = bootstrap_effect(state.request_bootstrap());
        state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-16", 1)));
        let _ = state.switch_tab(ActiveTab::List);
        let (list_id, request) = list_effect(state.select_logical_date("2026-09-15"));
        state.apply_list_result(
            list_id,
            &request.logical_date,
            Ok(WebSuccess {
                snapshot: snapshot("2026-09-16", 2),
                data: Vec::new(),
            }),
        );
        let _ = state.switch_list_mode(ListMode::Completed);

        let _ = state.switch_tab(other_tab);
        let (_, request) = list_effect(state.switch_tab(ActiveTab::List));

        assert_eq!(state.list_mode(), ListMode::Scheduled, "{other_tab:?}");
        assert_eq!(request.logical_date, "2026-09-15", "{other_tab:?}");
    }
}

#[test]
fn 一覧tabの再選択は完了modeを維持する() {
    let storage = FakeStorage::default();
    let mut state = schronu_web::client::state::load_client_state(&storage, 0).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-16", 1)));
    let _ = state.switch_tab(ActiveTab::List);
    let _ = state.switch_list_mode(ListMode::Completed);

    assert_eq!(state.switch_tab(ActiveTab::List), ClientEffect::None);
    assert_eq!(state.list_mode(), ListMode::Completed);
}

#[test]
fn 全てから完了へ切替えるとsnapshotの現在日へ戻る() {
    let storage = FakeStorage::default();
    let mut state = schronu_web::client::state::load_client_state(&storage, 0).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-16", 1)));
    let _ = state.select_logical_date("2026-09-20");
    let _ = state.select_all_tasks();

    let (_, request) = completed_effect(state.switch_list_mode(ListMode::Completed));
    assert_eq!(request.logical_date, "2026-09-16");
}

#[test]
fn 異なるmodeと古い完了一覧responseは表示を上書きしない() {
    let storage = FakeStorage::default();
    let mut state = schronu_web::client::state::load_client_state(&storage, 0).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-16", 1)));
    let (completed_id, request) = completed_effect(state.switch_list_mode(ListMode::Completed));
    let _ = state.switch_list_mode(ListMode::Scheduled);

    state.apply_completed_list_result(
        completed_id,
        &request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-16", 2),
            data: completed_report(TASK_ID, "古い"),
        }),
    );

    assert_eq!(state.list_mode(), ListMode::Scheduled);
    assert!(state.completed_rows().is_empty());
    assert!(state.completed_report().is_none());
}

#[test]
fn 古い完了一覧responseは新しいrowsとsummaryをatomicに保持する() {
    let storage = FakeStorage::default();
    let mut state = schronu_web::client::state::load_client_state(&storage, 0).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-16", 1)));
    let (old_id, old_request) = completed_effect(state.switch_list_mode(ListMode::Completed));
    let (new_id, new_request) = completed_effect(state.request_completed_list("2026-09-16"));
    let mut current = completed_report(TASK_ID, "新しい");
    current.total_actual_work_seconds = 300;
    current.rows[0].actual_work_seconds = 300;
    current.recorded_percentage = Some(50);
    state.apply_completed_list_result(
        new_id,
        &new_request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-16", 3),
            data: current,
        }),
    );

    state.apply_completed_list_result(
        old_id,
        &old_request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-16", 2),
            data: completed_report(OTHER_TASK_ID, "古い"),
        }),
    );

    assert_eq!(state.completed_rows()[0].task_name, "新しい");
    assert_eq!(
        state.completed_report().unwrap().total_actual_work_seconds,
        300
    );
    assert_eq!(
        state.completed_report().unwrap().recorded_percentage,
        Some(50)
    );
}

#[test]
fn 完了modeではmutation成功後も完了一覧を再取得する() {
    let storage = FakeStorage::default();
    let mut state = state_with_sessions(&storage, &[TASK_ID]);
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-16", 1)));
    let _ = state.switch_list_mode(ListMode::Completed);
    let (request_id, _) = complete_effect(state.begin_complete_session(&storage, TASK_ID));

    let (_, request) = completed_effect(state.apply_complete_result(
        &storage,
        request_id,
        Ok(snapshot("2026-09-16", 2)),
    ));
    assert_eq!(request.logical_date, "2026-09-16");
}

#[test]
fn 完了一覧errorは既存rowを保持して履歴へ記録する() {
    let storage = FakeStorage::default();
    let mut state = schronu_web::client::state::load_client_state(&storage, 0).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-16", 1)));
    let (first_id, first) = completed_effect(state.switch_list_mode(ListMode::Completed));
    state.apply_completed_list_result(
        first_id,
        &first.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-16", 2),
            data: completed_report(TASK_ID, "保持"),
        }),
    );
    let (retry_id, retry) = completed_effect(state.request_completed_list("2026-09-16"));
    state.apply_completed_list_result(
        retry_id,
        &retry.logical_date,
        Err(ServerFailure::Transport("secret".to_owned())),
    );

    assert_eq!(state.completed_rows()[0].task_name, "保持");
    assert!(state.has_completed_list());
    assert!(state.display_error().unwrap().retryable());
    assert!(state
        .history()
        .back()
        .unwrap()
        .invocation
        .to_string()
        .contains("list_completed_tasks(logical_date: \"2026-09-16\")"));
}

#[test]
fn mode切替先の完了一覧errorは保持rowをactive一覧にしない() {
    let storage = FakeStorage::default();
    let mut state = schronu_web::client::state::load_client_state(&storage, 0).unwrap();
    let bootstrap_id = bootstrap_effect(state.request_bootstrap());
    state.apply_bootstrap_result(bootstrap_id, Ok(snapshot("2026-09-16", 1)));

    let (completed_id, completed_request) =
        completed_effect(state.switch_list_mode(ListMode::Completed));
    state.apply_completed_list_result(
        completed_id,
        &completed_request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-16", 2),
            data: completed_report(TASK_ID, "A日の完了"),
        }),
    );
    let _ = state.switch_list_mode(ListMode::Scheduled);
    let (scheduled_id, scheduled_request) = list_effect(state.select_logical_date("2026-09-17"));
    state.apply_list_result(
        scheduled_id,
        &scheduled_request.logical_date,
        Ok(WebSuccess {
            snapshot: snapshot("2026-09-16", 3),
            data: vec![row(OTHER_TASK_ID, 0)],
        }),
    );

    let (failed_id, failed_request) = completed_effect(state.switch_list_mode(ListMode::Completed));
    state.apply_completed_list_result(
        failed_id,
        &failed_request.logical_date,
        Err(ServerFailure::Transport("secret".to_owned())),
    );

    assert_eq!(state.selected_logical_date(), Some("2026-09-17"));
    assert_eq!(state.completed_rows()[0].task_name, "A日の完了");
    assert!(!state.has_completed_list());
    assert!(state.display_error().is_some());
}

#[test]
fn 完了modeの日付buttonは今日から過去七日を降順表示する() {
    let buttons = logical_date_buttons_for_mode("2026-01-03", ListMode::Completed).unwrap();
    assert_eq!(buttons.len(), 8);
    assert_eq!(buttons[0].logical_date, "2026-01-03");
    assert_eq!(buttons[0].label, "土 今日");
    assert_eq!(buttons[1].logical_date, "2026-01-02");
    assert_eq!(buttons[1].label, "金 昨日");
    assert_eq!(buttons[7].logical_date, "2025-12-27");
}

#[test]
fn 完了modeの月日は現在日以前の直近有効日へ解決する() {
    assert_eq!(
        resolve_date_input_for_mode("9/16", "2026-09-17", ListMode::Completed)
            .unwrap()
            .logical_date,
        "2026-09-16"
    );
    assert_eq!(
        resolve_date_input_for_mode("9/18", "2026-09-17", ListMode::Completed)
            .unwrap()
            .logical_date,
        "2025-09-18"
    );
    assert_eq!(
        resolve_date_input_for_mode("2/29", "2027-03-01", ListMode::Completed)
            .unwrap()
            .logical_date,
        "2024-02-29"
    );
    assert_eq!(
        resolve_date_input_for_mode("2099/1/1", "2026-09-17", ListMode::Completed)
            .unwrap()
            .logical_date,
        "2099-01-01"
    );
    let chrono_min = NaiveDate::MIN;
    let next_day = chrono_min.succ_opt().unwrap();
    let current = chrono_min.format("%Y-%m-%d").to_string();
    let next_month_day = format!("{}/{}", next_day.month(), next_day.day());
    assert_eq!(
        resolve_date_input_for_mode(&next_month_day, &current, ListMode::Completed),
        Err(DateInputError::DateOverflow)
    );
}
