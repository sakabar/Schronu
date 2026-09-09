
#[test]
fn component_actionはrank非0の手動session追加を拒否する() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);

    assert_eq!(
        reduce_component_action_at(
            &mut state,
            &storage,
            1_000,
            ComponentAction::AddSession {
                task: task(RECORD_ID),
                is_leaf: false,
            },
        ),
        ClientEffect::None
    );

    assert!(state.sessions().is_empty());
    assert!(state.history().is_empty());
}

#[test]
fn reloadは前回一覧と入力を復元しbackground更新中もlocal追加を許可する() {
    let storage = MemoryStorage::default();
    let cached_row = ScheduledTaskRow {
        task: task(RECORD_ID),
        schedule_start_epoch_ms: 1_789_000_000_000,
        schedule_end_epoch_ms: 1_789_000_600_000,
        deadline_epoch_ms: None,
        deadline_label: "____/__/__".to_owned(),
        misses_deadline: false,
        is_leaf: true,
    };
    store_view_state(
        &storage,
        &ViewState {
            snapshot: ServerSnapshot {
                observed_at_epoch_ms: 1_789_000_000_000,
                logical_date: "2026-09-09".to_owned(),
                buffer_seconds: 60,
            },
            list: Some(StoredListView {
                logical_date: "2026-09-12".to_owned(),
                rows: vec![cached_row.clone()],
            }),
            active_tab: ActiveTab::List,
            task_name_filter: "設計".to_owned(),
            date_input_text: "2026/9/12".to_owned(),
        },
    )
    .unwrap();

    let mut orchestrator = ComponentOrchestrator::new();
    assert!(matches!(
        orchestrator.mount(&storage, 1_789_000_100_000),
        ClientEffect::Bootstrap { request_id: 1 }
    ));
    let state = orchestrator.state().unwrap();
    assert_eq!(state.active_tab(), ActiveTab::List);
    assert_eq!(state.selected_logical_date(), Some("2026-09-12"));
    assert_eq!(state.scheduled_rows(), [cached_row.clone()]);
    assert_eq!(orchestrator.task_name_filter(), "設計");
    assert_eq!(orchestrator.date_input().text(), "2026/9/12");
    assert!(orchestrator.background_refreshing());
    assert!(orchestrator.server_actions_blocked());
    assert!(!orchestrator.server_effect_in_flight());

    assert_eq!(
        orchestrator.action(
            &storage,
            1_000,
            ComponentAction::AddSession {
                task: cached_row.task,
                is_leaf: true,
            },
        ),
        ClientEffect::None
    );
    assert_eq!(orchestrator.state().unwrap().sessions().len(), 1);

    for blocked in [
        ComponentAction::SelectDate("2026-09-13".to_owned()),
        ComponentAction::AutoSession,
        ComponentAction::DiscardSession(RECORD_ID.to_owned()),
        ComponentAction::RecordSession(RECORD_ID.to_owned()),
        ComponentAction::CompleteSession(RECORD_ID.to_owned()),
        ComponentAction::CompleteSessionWithoutRecording(RECORD_ID.to_owned()),
        ComponentAction::ConfirmCompletionConflict(RECORD_ID.to_owned()),
    ] {
        assert_eq!(
            orchestrator.action(&storage, 1_001, blocked),
            ClientEffect::None
        );
    }
    assert_eq!(orchestrator.state().unwrap().sessions().len(), 1);
}

#[test]
fn local画面変更はview_stateへ保存して次のmountで復元する() {
    let storage = MemoryStorage::default();
    let mut orchestrator = mounted_orchestrator(&storage);

    orchestrator.edit_task_name_filter(&storage, "実装".to_owned());
    orchestrator.edit_date_input(&storage, "9/20".to_owned());
    orchestrator.action(
        &storage,
        2_000,
        ComponentAction::SwitchTab(ActiveTab::History),
    );

    let mut restored = ComponentOrchestrator::new();
    restored.mount(&storage, 3_000);
    assert_eq!(restored.state().unwrap().active_tab(), ActiveTab::History);
    assert_eq!(restored.task_name_filter(), "実装");
    assert_eq!(restored.date_input().text(), "9/20");
}

#[test]
fn bootstrap後は保存日付を再取得し成功時だけ一覧をatomic置換する() {
    let storage = MemoryStorage::default();
    let cached_row = ScheduledTaskRow {
        task: task(RECORD_ID),
        schedule_start_epoch_ms: 1_789_000_000_000,
        schedule_end_epoch_ms: 1_789_000_600_000,
        deadline_epoch_ms: None,
        deadline_label: "____/__/__".to_owned(),
        misses_deadline: false,
        is_leaf: true,
    };
    store_view_state(
        &storage,
        &ViewState {
            snapshot: snapshot(1_789_000_000_000),
            list: Some(StoredListView {
                logical_date: "2026-09-12".to_owned(),
                rows: vec![cached_row.clone()],
            }),
            active_tab: ActiveTab::List,
            task_name_filter: String::new(),
            date_input_text: String::new(),
        },
    )
    .unwrap();
    let mut orchestrator = ComponentOrchestrator::new();
    orchestrator.mount(&storage, 1_789_000_100_000);

    let follow_up = orchestrator.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id: 1,
            result: Ok(ServerSnapshot {
                observed_at_epoch_ms: 1_789_100_000_000,
                logical_date: "2026-09-10".to_owned(),
                buffer_seconds: 30,
            }),
        },
    );
    assert!(matches!(
        follow_up,
        ClientEffect::ListTasks {
            request_id: 2,
            request: ref list_request,
        } if list_request.logical_date == "2026-09-12"
    ));
    assert_eq!(orchestrator.state().unwrap().scheduled_rows(), [cached_row]);
    assert!(orchestrator.background_refreshing());

    orchestrator.apply_response(
        &storage,
        ClientResponse::ListTasks {
            request_id: 2,
            requested_date: "2026-09-12".to_owned(),
            result: Err(ServerFailure::Transport("offline".to_owned())),
        },
    );
    assert_eq!(orchestrator.state().unwrap().scheduled_rows().len(), 1);
    assert!(orchestrator.refresh_failed());

    assert!(matches!(
        orchestrator.action(&storage, 1_000, ComponentAction::RetryRefresh),
        ClientEffect::Bootstrap { request_id: 3 }
    ));
    let list_effect = orchestrator.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id: 3,
            result: Ok(ServerSnapshot {
                observed_at_epoch_ms: 1_789_200_000_000,
                logical_date: "2026-09-11".to_owned(),
                buffer_seconds: 20,
            }),
        },
    );
    assert!(matches!(list_effect, ClientEffect::ListTasks { request_id: 4, .. }));
    orchestrator.apply_response(
        &storage,
        ClientResponse::ListTasks {
            request_id: 4,
            requested_date: "2026-09-12".to_owned(),
            result: Ok(WebSuccess {
                snapshot: ServerSnapshot {
                    observed_at_epoch_ms: 1_789_200_000_000,
                    logical_date: "2026-09-11".to_owned(),
                    buffer_seconds: 20,
                },
                data: Vec::new(),
            }),
        },
    );
    assert!(orchestrator.state().unwrap().scheduled_rows().is_empty());
    assert!(orchestrator.state().unwrap().has_scheduled_list());
    assert!(!orchestrator.background_refreshing());
    assert!(!orchestrator.server_actions_blocked());
    let stored = load_view_state(&storage).into_state().unwrap();
    assert_eq!(stored.list.unwrap().rows, Vec::new());
}

#[test]
fn bootstrap失敗でも前回一覧を維持し再試行を提供する() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();
    orchestrator.mount(&storage, 1_000);
    orchestrator.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id: 1,
            result: Err(ServerFailure::Transport("offline".to_owned())),
        },
    );

    assert!(orchestrator.refresh_failed());
    assert!(matches!(
        orchestrator.action(&storage, 2_000, ComponentAction::RetryRefresh),
        ClientEffect::Bootstrap { request_id: 2 }
    ));
    assert!(orchestrator.background_refreshing());
}

#[test]
fn native_ssrはbrowser_storageへ触れずloading_shellだけを描画する() {
    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);

    assert!(!html.contains("Schronu"), "{html}");
    assert!(html.contains("通信中…"), "{html}");
    assert!(html.contains("loading-overlay"), "{html}");
    assert!(html.contains("loading-spinner"), "{html}");
    assert!(html.contains("role=\"status\""), "{html}");
    assert!(html.contains("aria-live=\"polite\""), "{html}");
    assert!(html.contains("aria-busy=\"true\""), "{html}");
    assert!(html.contains("id=\"schronu-web-loading\""), "{html}");
    assert!(!html.contains("schronu-web-ready"), "{html}");
    assert!(!html.contains("schronu-buffer-ready"), "{html}");
    assert!(!html.contains("BUFFER"), "{html}");
    assert!(!html.contains("--:--:--"), "{html}");
    assert!(!html.contains("schronu 今"), "{html}");
    assert!(!html.contains(">更新<"), "{html}");
}

fn failed_initial_load() -> Element {
    rsx! {
        InitialLoadView {
            in_flight: false,
            error: Some("初期データを取得できませんでした".to_owned()),
        }
    }
}

fn ready_buffer() -> Element {
    rsx! {
        InteractiveShell { blocked: false,
            BufferPanel { value: 3_661 }
        }
    }
}

#[test]
fn 初回取得失敗と成功後のbufferは別idと別domを持つ() {
    let mut failed_dom = VirtualDom::new(failed_initial_load);
    failed_dom.rebuild_in_place();
    let failed_html = dioxus::ssr::render(&failed_dom);
    assert!(failed_html.contains("id=\"schronu-web-load-error\""), "{failed_html}");
    assert!(failed_html.contains("初期データを取得できませんでした"), "{failed_html}");
    assert!(!failed_html.contains("schronu-buffer-ready"), "{failed_html}");
    assert!(!failed_html.contains("BUFFER"), "{failed_html}");

    let mut ready_dom = VirtualDom::new(ready_buffer);
    ready_dom.rebuild_in_place();
    let ready_html = dioxus::ssr::render(&ready_dom);
    assert!(ready_html.contains("id=\"schronu-web-ready\""), "{ready_html}");
    assert!(ready_html.contains("id=\"schronu-buffer-ready\""), "{ready_html}");
    assert!(ready_html.contains("01:01:01"), "{ready_html}");
    assert!(!ready_html.contains("--:--:--"), "{ready_html}");
}

#[test]
fn 初回画面はsnapshotとerrorと通信状態からloading_error_readyを区別する() {
    assert_eq!(
        initial_load_phase(false, false, None),
        InitialLoadPhase::Loading,
        "mount直後の通信開始前もloadingを維持する"
    );
    assert_eq!(
        initial_load_phase(false, true, None),
        InitialLoadPhase::Loading
    );
    assert_eq!(
        initial_load_phase(false, false, Some("失敗")),
        InitialLoadPhase::Error
    );
    assert_eq!(
        initial_load_phase(true, true, Some("古いerror")),
        InitialLoadPhase::Ready,
        "snapshot取得後の通常通信ではready DOMを維持する"
    );
}

fn ready_buffer_during_follow_up_load() -> Element {
    rsx! {
        InteractiveShell { blocked: true,
            BufferPanel { value: 60 }
        }
        LoadingOverlay {}
    }
}

#[test]
fn 初回取得後の通常通信中は確定済みbufferを維持する() {
    let mut dom = VirtualDom::new(ready_buffer_during_follow_up_load);
    dom.rebuild_in_place();
    let html = dioxus::ssr::render(&dom);

    assert!(html.contains("id=\"schronu-web-ready\""), "{html}");
    assert!(html.contains("id=\"schronu-buffer-ready\""), "{html}");
    assert!(html.contains("00:01:00"), "{html}");
    assert!(html.contains("loading-overlay"), "{html}");
}

#[test]
fn 共通chromeはsessionだけに表示する() {
    fn chrome_root(active_tab: ActiveTab) -> Element {
        rsx! {
            SessionChrome { active_tab,
                aside { class: "carry-lock-bar", "lock" }
                section { class: "buffer-panel", "buffer" }
            }
        }
    }

    for (active_tab, visible) in [
        (ActiveTab::Session, true),
        (ActiveTab::List, false),
        (ActiveTab::History, false),
    ] {
        let mut dom = VirtualDom::new_with_props(chrome_root, active_tab);
        dom.rebuild_in_place();
        let html = dioxus::ssr::render(&dom);
        assert_eq!(html.contains("carry-lock-bar"), visible, "{html}");
        assert_eq!(html.contains("buffer-panel"), visible, "{html}");
    }
}

fn interactive_shell_transition() -> Element {
    let mut blocked = use_signal(|| true);
    rsx! {
        InteractiveShell { blocked: blocked(),
            button { onclick: move |_| blocked.set(false), "response受理" }
        }
    }
}

#[test]
fn 通信完了時は製品mainからinert属性自体を除去する() {
    let mut dom = VirtualDom::new(interactive_shell_transition);
    let click_ids = rebuild_with_click_listeners(&mut dom);
    assert_eq!(click_ids.len(), 1);
    assert!(dioxus::ssr::render(&dom).contains(" inert"));

    dispatch_click(&dom, click_ids[0]);
    let mutations = dom.render_immediate_to_vec().edits;

    assert!(
        mutations.iter().any(|mutation| matches!(
            mutation,
            Mutation::SetAttribute {
                name: "inert",
                value: AttributeValue::None,
                ..
            }
        )),
        "{mutations:?}"
    );
    assert!(!dioxus::ssr::render(&dom).contains(" inert"));
}

#[test]
fn 製品orchestratorは全server_effectの完了まで通信中を保持する() {
    let mut orchestrator = ComponentOrchestrator::new();

    assert!(!orchestrator.server_effect_in_flight());
    orchestrator.begin_server_effect();
    assert!(orchestrator.server_effect_in_flight());
    orchestrator.begin_server_effect();
    orchestrator.finish_server_effect();
    assert!(orchestrator.server_effect_in_flight());
    orchestrator.finish_server_effect();
    assert!(!orchestrator.server_effect_in_flight());
}

#[test]
fn 製品orchestratorはmountを一度に制限しresponseとtickを同じstateへ適用する() {
    let storage = MemoryStorage::default();
    let mut orchestrator = ComponentOrchestrator::new();

    let first = orchestrator.mount(&storage, 1_000);
    assert!(matches!(first, ClientEffect::Bootstrap { request_id: 1 }));
    assert_eq!(orchestrator.mount(&storage, 2_000), ClientEffect::None);

    orchestrator.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id: 1,
            result: Ok(ServerSnapshot {
                observed_at_epoch_ms: 1_000,
                logical_date: "2026-09-05".to_owned(),
                buffer_seconds: 60,
            }),
        },
    );
    assert_eq!(
        orchestrator
            .state()
            .unwrap()
            .snapshot()
            .unwrap()
            .buffer_seconds,
        60
    );
    assert_eq!(
        orchestrator.action(
            &storage,
            3_000,
            ComponentAction::Tick {
                wall_now_epoch_ms: 3_000,
            }
        ),
        ClientEffect::None
    );
    assert_eq!(orchestrator.state().unwrap().tick_now_epoch_ms(), 3_000);
}

#[test]
fn 最後のsessionを即時削除した時だけsessionから一覧tabへ移る() {
    let storage = MemoryStorage::default();
    let mut orchestrator = mounted_orchestrator(&storage);
    add_session(&mut orchestrator, &storage, RECORD_ID);
    add_session(&mut orchestrator, &storage, COMPLETE_ID);

    assert!(matches!(
        orchestrator.action(
            &storage,
            2_000,
            ComponentAction::DiscardSession(RECORD_ID.to_owned()),
        ),
        ClientEffect::ListTasks { .. }
    ));
    assert_eq!(
        orchestrator.state().unwrap().active_tab(),
        ActiveTab::Session,
        "sessionが残る場合は遷移しない"
    );

    assert!(matches!(
        orchestrator.action(
            &storage,
            2_001,
            ComponentAction::DiscardSession(COMPLETE_ID.to_owned()),
        ),
        ClientEffect::ListTasks { .. }
    ));
    assert_eq!(orchestrator.state().unwrap().active_tab(), ActiveTab::List);

    let other_storage = MemoryStorage::default();
    let mut other = mounted_orchestrator(&other_storage);
    add_session(&mut other, &other_storage, RECORD_ID);
    other.action(
        &other_storage,
        2_000,
        ComponentAction::SwitchTab(ActiveTab::History),
    );
    other.action(
        &other_storage,
        2_001,
        ComponentAction::DiscardSession(RECORD_ID.to_owned()),
    );
    assert_eq!(other.state().unwrap().active_tab(), ActiveTab::History);
}

#[test]
fn server応答で最後のsessionが消えた時だけ一覧tabへ移る() {
    let multiple_storage = MemoryStorage::default();
    let mut multiple = mounted_orchestrator(&multiple_storage);
    add_session(&mut multiple, &multiple_storage, RECORD_ID);
    add_session(&mut multiple, &multiple_storage, COMPLETE_ID);
    let request_id = mutation_request_id(&multiple.action(
        &multiple_storage,
        2_000,
        ComponentAction::RecordSession(RECORD_ID.to_owned()),
    ));
    assert!(matches!(
        multiple.apply_response(
            &multiple_storage,
            ClientResponse::RecordSession {
                request_id,
                result: Ok(WebSuccess {
                    snapshot: snapshot(2_000),
                    data: RecordSessionResult {
                        actual_work_seconds: 1,
                    },
                }),
            },
        ),
        ClientEffect::ListTasks { .. }
    ));
    assert_eq!(multiple.state().unwrap().sessions().len(), 1);
    assert_eq!(multiple.state().unwrap().active_tab(), ActiveTab::Session);

    for complete in [false, true] {
        let storage = MemoryStorage::default();
        let mut orchestrator = mounted_orchestrator(&storage);
        add_session(&mut orchestrator, &storage, RECORD_ID);
        let effect = orchestrator.action(
            &storage,
            2_000,
            if complete {
                ComponentAction::CompleteSession(RECORD_ID.to_owned())
            } else {
                ComponentAction::RecordSession(RECORD_ID.to_owned())
            },
        );
        let request_id = mutation_request_id(&effect);
        let response = if complete {
            ClientResponse::CompleteSession {
                request_id,
                result: Ok(snapshot(2_000)),
            }
        } else {
            ClientResponse::RecordSession {
                request_id,
                result: Ok(WebSuccess {
                    snapshot: snapshot(2_000),
                    data: RecordSessionResult {
                        actual_work_seconds: 1,
                    },
                }),
            }
        };

        assert!(matches!(
            orchestrator.apply_response(&storage, response),
            ClientEffect::ListTasks { .. }
        ));
        assert!(orchestrator.state().unwrap().sessions().is_empty());
        assert_eq!(orchestrator.state().unwrap().active_tab(), ActiveTab::List);
    }
}

#[test]
fn session削除失敗と他tabでは一覧へ強制遷移しない() {
    let failure_storage = MemoryStorage::default();
    let mut failure = mounted_orchestrator(&failure_storage);
    add_session(&mut failure, &failure_storage, RECORD_ID);
    let request_id = mutation_request_id(&failure.action(
        &failure_storage,
        2_000,
        ComponentAction::RecordSession(RECORD_ID.to_owned()),
    ));
    assert_eq!(
        failure.apply_response(
            &failure_storage,
            ClientResponse::RecordSession {
                request_id,
                result: Err(ServerFailure::Transport("切断".to_owned())),
            },
        ),
        ClientEffect::None
    );
    assert_eq!(failure.state().unwrap().active_tab(), ActiveTab::Session);
    assert_eq!(failure.state().unwrap().sessions().len(), 1);

    let storage_failure = MemoryStorage::default();
    let mut blocked = mounted_orchestrator(&storage_failure);
    add_session(&mut blocked, &storage_failure, RECORD_ID);
    let request_id = mutation_request_id(&blocked.action(
        &storage_failure,
        2_000,
        ComponentAction::RecordSession(RECORD_ID.to_owned()),
    ));
    storage_failure.set_fail_writes(true);
    assert!(matches!(
        blocked.apply_response(
            &storage_failure,
            ClientResponse::RecordSession {
                request_id,
                result: Ok(WebSuccess {
                    snapshot: snapshot(2_000),
                    data: RecordSessionResult {
                        actual_work_seconds: 1,
                    },
                }),
            },
        ),
        ClientEffect::ListTasks { .. }
    ));
    assert_eq!(blocked.state().unwrap().active_tab(), ActiveTab::Session);
    assert_eq!(blocked.state().unwrap().sessions().len(), 1);

    storage_failure.set_fail_writes(false);
    assert_eq!(
        blocked.action(
            &storage_failure,
            2_001,
            ComponentAction::ConfirmRepositoryChecked,
        ),
        ClientEffect::None
    );
    assert!(blocked.state().unwrap().sessions().is_empty());
    assert_eq!(blocked.state().unwrap().active_tab(), ActiveTab::List);

    let conflict_storage = MemoryStorage::default();
    let mut conflict = mounted_orchestrator(&conflict_storage);
    add_session(&mut conflict, &conflict_storage, COMPLETE_ID);
    let request_id = mutation_request_id(&conflict.action(
        &conflict_storage,
        2_000,
        ComponentAction::CompleteSession(COMPLETE_ID.to_owned()),
    ));
    assert_eq!(
        conflict.apply_response(
            &conflict_storage,
            ClientResponse::CompleteSession {
                request_id,
                result: Err(ServerFailure::Operation(WebError {
                    code: web_error_codes::ACTUAL_WORK_CONFLICT.to_owned(),
                    message: "競合".to_owned(),
                    retry_advice: RetryAdvice::ManualCheck,
                    current_actual_work_seconds: Some(1),
                })),
            },
        ),
        ClientEffect::None
    );
    assert_eq!(conflict.state().unwrap().active_tab(), ActiveTab::Session);
    assert_eq!(conflict.state().unwrap().sessions().len(), 1);
    assert_eq!(
        conflict.action(
            &conflict_storage,
            2_500,
            ComponentAction::ResumeCompletionConflict(COMPLETE_ID.to_owned()),
        ),
        ClientEffect::None
    );
    assert_eq!(conflict.state().unwrap().active_tab(), ActiveTab::Session);
    assert_eq!(conflict.state().unwrap().sessions().len(), 1);

    let other_storage = MemoryStorage::default();
    let mut other = mounted_orchestrator(&other_storage);
    add_session(&mut other, &other_storage, COMPLETE_ID);
    let request_id = mutation_request_id(&other.action(
        &other_storage,
        2_000,
        ComponentAction::CompleteSession(COMPLETE_ID.to_owned()),
    ));
    other.action(
        &other_storage,
        2_001,
        ComponentAction::SwitchTab(ActiveTab::History),
    );
    assert!(matches!(
        other.apply_response(
            &other_storage,
            ClientResponse::CompleteSession {
                request_id,
                result: Ok(snapshot(2_000)),
            },
        ),
        ClientEffect::ListTasks { .. }
    ));
    assert!(other.state().unwrap().sessions().is_empty());
    assert_eq!(other.state().unwrap().active_tab(), ActiveTab::History);
}

fn mounted_orchestrator(storage: &MemoryStorage) -> ComponentOrchestrator {
    let mut orchestrator = ComponentOrchestrator::new();
    assert!(matches!(
        orchestrator.mount(storage, 1_000),
        ClientEffect::Bootstrap { request_id: 1 }
    ));
    orchestrator.apply_response(
        storage,
        ClientResponse::Bootstrap {
            request_id: 1,
            result: Ok(snapshot(1_000)),
        },
    );
    orchestrator
}

fn add_session(orchestrator: &mut ComponentOrchestrator, storage: &MemoryStorage, task_id: &str) {
    assert_eq!(
        orchestrator.action(
            storage,
            1_500,
            ComponentAction::AddSession {
                task: task(task_id),
                is_leaf: true,
            },
        ),
        ClientEffect::None
    );
}

fn mutation_request_id(effect: &ClientEffect) -> u64 {
    match effect {
        ClientEffect::RecordSession { request_id, .. }
        | ClientEffect::CompleteSession { request_id, .. } => *request_id,
        effect => panic!("mutation effectを期待しました: {effect:?}"),
    }
}

fn snapshot(observed_at_epoch_ms: i64) -> ServerSnapshot {
    ServerSnapshot {
        observed_at_epoch_ms,
        logical_date: "2026-09-05".to_owned(),
        buffer_seconds: 60,
    }
}

fn task(task_id: &str) -> SessionTask {
    SessionTask {
        task_id: task_id.to_owned(),
        task_name: format!("task {task_id}"),
        estimated_work_seconds: 600,
        actual_work_seconds: 0,
    }
}

const RECORD_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
const COMPLETE_ID: &str = "123e4567-e89b-12d3-a456-426614174001";

#[derive(Default)]
struct MemoryStorage {
    values: RefCell<HashMap<String, String>>,
    fail_reads: bool,
    fail_writes: Cell<bool>,
    fail_carry_lock_reads: bool,
}

impl MemoryStorage {
    fn failing_reads() -> Self {
        Self {
            values: RefCell::new(HashMap::new()),
            fail_reads: true,
            fail_writes: Cell::new(false),
            fail_carry_lock_reads: false,
        }
    }

    fn failing_writes() -> Self {
        Self {
            values: RefCell::new(HashMap::new()),
            fail_reads: false,
            fail_writes: Cell::new(true),
            fail_carry_lock_reads: false,
        }
    }

    #[cfg(feature = "web")]
    fn failing_carry_lock_reads() -> Self {
        Self {
            values: RefCell::new(HashMap::new()),
            fail_reads: false,
            fail_writes: Cell::new(false),
            fail_carry_lock_reads: true,
        }
    }

    fn set_fail_writes(&self, fail_writes: bool) {
        self.fail_writes.set(fail_writes);
    }
}

impl KeyValueStorage for MemoryStorage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        if self.fail_reads {
            return Err(StorageError::ReadFailed);
        }
        if key == "schronu_web.carry_lock.v1" && self.fail_carry_lock_reads {
            return Err(StorageError::ReadFailed);
        }
        Ok(self.values.borrow().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        if self.fail_writes.get() {
            return Err(StorageError::WriteFailed);
        }
        self.values
            .borrow_mut()
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }
}
