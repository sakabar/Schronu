use super::component_runtime::{ComponentAction, ComponentOrchestrator};
use super::effect_dispatcher::ClientResponse;
use crate::client::state::{ActiveTab, AllTasksStatus, ClientEffect, ListSelection};
use crate::client::view_state::{store_view_state, StoredListView, ViewState};
use crate::client::work_sessions::{KeyValueStorage, StorageError};
use crate::{AllTaskPage, AllTaskRow, ServerSnapshot, SessionTask, WebSuccess};
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Default)]
struct MemoryStorage(RefCell<HashMap<String, String>>);

impl KeyValueStorage for MemoryStorage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        Ok(self.0.borrow().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        self.0.borrow_mut().insert(key.to_owned(), value.to_owned());
        Ok(())
    }
}

fn snapshot(observed_at_epoch_ms: i64) -> ServerSnapshot {
    ServerSnapshot {
        observed_at_epoch_ms,
        logical_date: "2026-09-05".to_owned(),
        buffer_seconds: 0,
    }
}

fn all_row(segment_index: usize, task_id: &str, task_name: &str, is_leaf: bool) -> AllTaskRow {
    AllTaskRow {
        task: SessionTask {
            task_id: task_id.to_owned(),
            task_name: task_name.to_owned(),
            estimated_work_seconds: 900,
            actual_work_seconds: 300,
        },
        segment_index,
        schedule_date: "2026-09-06".to_owned(),
        deadline_epoch_ms: Some(1_000),
        deadline_label: "2026-09-06 07:00".to_owned(),
        misses_deadline: false,
        task_display_kind: crate::TaskDisplayKind::NonRepetitive,
        deadline_display_kind: crate::DeadlineDisplayKind::Future,
        is_leaf,
    }
}

#[test]
fn reloadは日付別viewと共有検索を復元しall選択とdataは復元しない() {
    let storage = MemoryStorage::default();
    store_view_state(
        &storage,
        &ViewState {
            snapshot: snapshot(1),
            list: Some(StoredListView {
                logical_date: "2026-09-06".to_owned(),
                rows: Vec::new(),
            }),
            active_tab: ActiveTab::List,
            task_name_filter: "date filter".to_owned(),
            date_input_text: String::new(),
        },
    )
    .unwrap();

    let mut orchestrator = ComponentOrchestrator::new();
    let bootstrap = orchestrator.mount(&storage, 2);
    assert!(orchestrator.effect_is_background(&bootstrap));
    let state = orchestrator.state().unwrap();
    assert_eq!(state.selected_logical_date(), Some("2026-09-06"));
    assert_eq!(state.list_selection(), ListSelection::Date);
    assert_eq!(state.all_tasks_status(), AllTasksStatus::NotLoaded);
    assert_eq!(orchestrator.task_name_filter(), "date filter");
}

#[test]
fn all取得はoverlay対象外で日付へ移動後もresponse_chainを継続する() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();
    let bootstrap = orchestrator.mount(&storage, 0);
    let bootstrap_id = match bootstrap {
        ClientEffect::Bootstrap { request_id } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    };
    orchestrator.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id: bootstrap_id,
            result: Ok(snapshot(1)),
        },
    );

    let first = orchestrator.action(&storage, 0, ComponentAction::SelectAllTasks);
    assert!(orchestrator.effect_is_background(&first));
    assert!(!orchestrator.server_effect_in_flight());
    let (request_id, request) = match first {
        ClientEffect::ListAllTasks {
            request_id,
            request,
        } => (request_id, request),
        other => panic!("unexpected effect: {other:?}"),
    };

    let date = orchestrator.action(
        &storage,
        0,
        ComponentAction::SelectDate("2026-09-06".to_owned()),
    );
    assert!(matches!(date, ClientEffect::ListTasks { .. }));
    assert_eq!(
        orchestrator.state().unwrap().list_selection(),
        ListSelection::Date
    );

    let follow_up = orchestrator.apply_response(
        &storage,
        ClientResponse::ListAllTasks {
            request_id,
            request,
            result: Ok(WebSuccess {
                snapshot: snapshot(1),
                data: AllTaskPage {
                    rows: Vec::new(),
                    next_cursor: Some("snapshot:500".to_owned()),
                },
            }),
        },
    );
    assert!(matches!(follow_up, ClientEffect::ListAllTasks { .. }));
    assert!(orchestrator.effect_is_background(&follow_up));
    assert_eq!(
        orchestrator.state().unwrap().all_tasks_status(),
        AllTasksStatus::Loading
    );
}

#[test]
fn 検索語は日付別とallで共有しreload後も復元する() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();
    let bootstrap_id = match orchestrator.mount(&storage, 0) {
        ClientEffect::Bootstrap { request_id } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    };
    let _ = orchestrator.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id: bootstrap_id,
            result: Ok(snapshot(1)),
        },
    );

    orchestrator.edit_task_name_filter(&storage, "date filter".to_owned());
    let _ = orchestrator.action(&storage, 0, ComponentAction::SelectAllTasks);
    assert_eq!(orchestrator.task_name_filter(), "date filter");

    orchestrator.edit_task_name_filter(&storage, "all filter".to_owned());
    assert_eq!(orchestrator.task_name_filter(), "all filter");

    let _ = orchestrator.action(
        &storage,
        0,
        ComponentAction::SelectDate("2026-09-06".to_owned()),
    );
    assert_eq!(orchestrator.task_name_filter(), "all filter");

    let _ = orchestrator.action(&storage, 0, ComponentAction::SelectAllTasks);
    assert_eq!(orchestrator.task_name_filter(), "all filter");

    let mut reloaded = ComponentOrchestrator::new();
    let _ = reloaded.mount(&storage, 0);
    assert_eq!(reloaded.task_name_filter(), "all filter");
    let _ = reloaded.action(&storage, 0, ComponentAction::SelectAllTasks);
    assert_eq!(reloaded.task_name_filter(), "all filter");
}

#[test]
fn 検索語変更は選択中の一覧にかかわらずall表示上限を500へ戻す() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();
    let _ = orchestrator.mount(&storage, 0);
    let _ = orchestrator.action(&storage, 0, ComponentAction::SelectAllTasks);

    assert_eq!(orchestrator.all_tasks_visible_limit(), 500);
    orchestrator.show_more_all_tasks();
    assert_eq!(orchestrator.all_tasks_visible_limit(), 1_000);
    orchestrator.edit_task_name_filter(&storage, "設計".to_owned());
    assert_eq!(orchestrator.all_tasks_visible_limit(), 500);

    orchestrator.show_more_all_tasks();
    let _ = orchestrator.action(
        &storage,
        0,
        ComponentAction::SelectDate("2026-09-06".to_owned()),
    );
    orchestrator.edit_task_name_filter(&storage, "実装".to_owned());
    assert_eq!(orchestrator.all_tasks_visible_limit(), 500);
}

#[test]
fn all選択中は製品orchestratorの全日付buttonを非選択にする() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();
    let _ = orchestrator.mount(&storage, 0);
    let _ = orchestrator.action(&storage, 0, ComponentAction::SelectAllTasks);

    assert_eq!(
        orchestrator.state().unwrap().list_selection(),
        ListSelection::All
    );
    assert!(orchestrator
        .date_button_models()
        .iter()
        .all(|date| !date.selected));
}

#[test]
fn allからsession追加成功した時だけ共有検索語をclearする() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();
    let bootstrap_id = match orchestrator.mount(&storage, 0) {
        ClientEffect::Bootstrap { request_id } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    };
    let _ = orchestrator.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id: bootstrap_id,
            result: Ok(snapshot(1)),
        },
    );
    orchestrator.edit_task_name_filter(&storage, "date filter".to_owned());
    let _ = orchestrator.action(&storage, 0, ComponentAction::SelectAllTasks);
    orchestrator.edit_task_name_filter(&storage, "all filter".to_owned());

    let _ = orchestrator.start_session_from_list(
        &storage,
        0,
        SessionTask {
            task_id: "00000000-0000-4000-8000-000000000001".to_owned(),
            task_name: "task".to_owned(),
            estimated_work_seconds: 900,
            actual_work_seconds: 0,
        },
        true,
    );
    assert_eq!(orchestrator.state().unwrap().sessions().len(), 1);
    assert_eq!(orchestrator.task_name_filter(), "");
    assert_eq!(
        orchestrator.state().unwrap().active_tab(),
        ActiveTab::Session
    );

    let _ = orchestrator.action(
        &storage,
        0,
        ComponentAction::SelectDate("2026-09-06".to_owned()),
    );
    assert_eq!(orchestrator.task_name_filter(), "");
}

#[test]
fn allからsession追加が重複で拒否された時は共有検索語を保つ() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();
    let bootstrap_id = match orchestrator.mount(&storage, 0) {
        ClientEffect::Bootstrap { request_id } => request_id,
        other => panic!("unexpected effect: {other:?}"),
    };
    let _ = orchestrator.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id: bootstrap_id,
            result: Ok(snapshot(1)),
        },
    );
    let task = SessionTask {
        task_id: "00000000-0000-4000-8000-000000000001".to_owned(),
        task_name: "task".to_owned(),
        estimated_work_seconds: 900,
        actual_work_seconds: 0,
    };
    let _ = orchestrator.action(
        &storage,
        0,
        ComponentAction::AddSession {
            task: task.clone(),
            is_leaf: true,
        },
    );
    let _ = orchestrator.action(&storage, 0, ComponentAction::SelectAllTasks);
    orchestrator.edit_task_name_filter(&storage, "all filter".to_owned());

    let _ = orchestrator.start_session_from_list(&storage, 0, task, true);

    assert_eq!(orchestrator.state().unwrap().sessions().len(), 1);
    assert_eq!(orchestrator.task_name_filter(), "all filter");
    let _ = orchestrator.action(
        &storage,
        0,
        ComponentAction::SelectDate("2026-09-06".to_owned()),
    );
    assert_eq!(orchestrator.task_name_filter(), "all filter");
}

#[test]
fn all行は日付labelとsegment_index_keyを使い先送りを持たない() {
    use crate::client::view_projection::project_all_task_rows;

    let mut fixed = all_row(12, "same", "設計", true);
    fixed.task_display_kind = crate::TaskDisplayKind::Fixed;
    fixed.deadline_display_kind = crate::DeadlineDisplayKind::Today;
    let mut repetitive = all_row(13, "same", "設計", true);
    repetitive.task_display_kind = crate::TaskDisplayKind::Repetitive;
    repetitive.deadline_display_kind = crate::DeadlineDisplayKind::Future;
    let mut parent = all_row(14, "parent", "親", false);
    parent.misses_deadline = true;
    parent.deadline_display_kind = crate::DeadlineDisplayKind::None;
    let rows = project_all_task_rows(&[fixed, repetitive, parent]);
    assert_eq!(rows[0].row_key, "all:12");
    assert_eq!(
        rows[0].schedule_display,
        crate::client::view_projection::ScheduleDisplayViewModel::AllTasksDate {
            label: "2026/09/06(日)".to_owned(),
        }
    );
    assert!(rows[0].defer_plan.is_none());
    assert_eq!(rows[1].row_key, "all:13");
    assert!(!rows[2].is_leaf);
    assert_eq!(rows[0].task_display_kind, crate::TaskDisplayKind::Fixed);
    assert_eq!(
        rows[0].deadline_display_kind,
        crate::DeadlineDisplayKind::Today
    );
    assert_eq!(
        rows[1].task_display_kind,
        crate::TaskDisplayKind::Repetitive
    );
    assert_eq!(
        rows[1].deadline_display_kind,
        crate::DeadlineDisplayKind::Future
    );
    assert_eq!(
        rows[2].task_display_kind,
        crate::TaskDisplayKind::NonRepetitive
    );
    assert!(rows[2].misses_deadline);
    assert_eq!(
        rows[2].deadline_display_kind,
        crate::DeadlineDisplayKind::None
    );
}

#[test]
fn all行は翌logical_dateの境界と予定のない日数を次のtaskへ付与する() {
    use crate::client::view_projection::project_all_task_rows;

    let mut first = all_row(0, "first", "先頭", true);
    let mut same_day = all_row(1, "same", "同日", true);
    let mut next_day = all_row(2, "next", "翌日", true);
    next_day.schedule_date = "2026-09-07".to_owned();
    let mut after_gap = all_row(3, "gap", "空き後", true);
    after_gap.schedule_date = "2026-09-10".to_owned();
    let mut invalid = all_row(4, "invalid", "不正", true);
    invalid.schedule_date = "not-a-date".to_owned();
    first.schedule_date = "2026-09-06".to_owned();
    same_day.schedule_date = "2026-09-06".to_owned();

    let rows = project_all_task_rows(&[first, same_day, next_day, after_gap, invalid]);
    assert_eq!(rows[0].gap_before, None);
    assert_eq!(rows[1].gap_before, None);
    assert_eq!(rows[2].gap_before, None);
    assert_eq!(rows[3].gap_before.as_deref(), Some("2日間の空き時間"));
    assert_eq!(rows[4].gap_before, None);
    assert!(!rows[0].date_boundary_before);
    assert!(!rows[1].date_boundary_before);
    assert!(rows[2].date_boundary_before);
    assert!(!rows[3].date_boundary_before);
    assert!(!rows[4].date_boundary_before);
}

#[test]
fn all行は非昇順の後も既出task日を空きに数えない() {
    use crate::client::view_projection::project_all_task_rows;

    let mut september_tenth = all_row(0, "tenth", "10日", true);
    september_tenth.schedule_date = "2026-09-10".to_owned();
    let mut september_fifth = all_row(1, "fifth", "5日", true);
    september_fifth.schedule_date = "2026-09-05".to_owned();
    let mut september_twelfth = all_row(2, "twelfth", "12日", true);
    september_twelfth.schedule_date = "2026-09-12".to_owned();

    let rows = project_all_task_rows(&[september_tenth, september_fifth, september_twelfth]);
    assert_eq!(rows[0].gap_before, None);
    assert_eq!(rows[1].gap_before, None);
    assert_eq!(rows[2].gap_before.as_deref(), Some("1日間の空き時間"));
    assert!(rows.iter().all(|row| !row.date_boundary_before));
}

#[test]
fn all検索後は一致taskだけから日付区切りを再計算する() {
    use crate::client::view_projection::project_visible_all_task_rows;

    let first = all_row(10, "first", "検索対象 先頭", true);
    let mut unmatched = all_row(11, "unmatched", "通常task", true);
    unmatched.schedule_date = "2026-09-08".to_owned();
    let mut after_gap = all_row(12, "after-gap", "検索対象 空き後", true);
    after_gap.schedule_date = "2026-09-10".to_owned();

    let projected = project_visible_all_task_rows(&[first, unmatched, after_gap], "検索対象", 500);

    assert_eq!(projected.rows.len(), 2);
    assert!(!projected.has_more);
    assert_eq!(projected.rows[0].row_key, "all:10");
    assert_eq!(projected.rows[1].row_key, "all:12");
    assert_eq!(
        projected.rows[1].gap_before.as_deref(),
        Some("3日間の空き時間")
    );
    assert!(!projected.rows[1].date_boundary_before);

    let mut adjacent = all_row(13, "adjacent", "検索対象 翌日", true);
    adjacent.schedule_date = "2026-09-07".to_owned();
    let projected = project_visible_all_task_rows(
        &[
            all_row(10, "first", "検索対象 先頭", true),
            all_row(11, "unmatched", "通常task", true),
            adjacent,
        ],
        "検索対象",
        500,
    );
    assert!(projected.rows[1].date_boundary_before);
    assert_eq!(projected.rows[1].gap_before, None);
}

#[test]
fn all行の区切りはtask件数上限に含めずさらに表示で先頭から再投影する() {
    use crate::client::view_projection::project_visible_all_task_rows;

    let mut rows = (0..501)
        .map(|index| all_row(index, &format!("task-{index}"), "検索対象", true))
        .collect::<Vec<_>>();
    rows[500].schedule_date = "2026-09-10".to_owned();

    let first_page = project_visible_all_task_rows(&rows, "", 500);
    assert_eq!(first_page.rows.len(), 500);
    assert!(first_page.has_more);
    assert!(first_page.rows.iter().all(|row| row.gap_before.is_none()));
    assert!(first_page.rows.iter().all(|row| !row.date_boundary_before));

    let expanded = project_visible_all_task_rows(&rows, "", 501);
    assert_eq!(expanded.rows.len(), 501);
    assert_eq!(
        expanded.rows[500].gap_before.as_deref(),
        Some("3日間の空き時間")
    );
    assert!(!expanded.rows[500].date_boundary_before);

    let filtered = project_visible_all_task_rows(&rows, "検索", 501);
    assert_eq!(filtered.rows.len(), 501);
    assert_eq!(filtered.rows[500].row_key, "all:500");
    assert_eq!(
        filtered.rows[500].gap_before.as_deref(),
        Some("3日間の空き時間")
    );
}

#[test]
fn all生rowは検索後に表示上限までだけprojectionする() {
    use crate::client::view_projection::project_visible_all_task_rows;

    let rows = (0..20_000)
        .map(|index| {
            all_row(
                index,
                &format!("task-{index}"),
                if index % 2 == 0 {
                    "検索対象 TASK"
                } else {
                    "通常タスク"
                },
                true,
            )
        })
        .collect::<Vec<_>>();

    let projected = project_visible_all_task_rows(&rows, "  検索対象 task  ", 500);
    assert_eq!(projected.rows.len(), 500);
    assert!(projected.has_more);
    assert_eq!(projected.rows[0].row_key, "all:0");
    assert_eq!(projected.rows[499].row_key, "all:998");
}
