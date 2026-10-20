use super::*;
use crate::app::effect_dispatcher::apply_response;
use crate::client::state::{ClientState, ListMode};

pub(super) fn session_state(storage: &MemoryStorage) -> ClientState {
    let (mut state, _) = initialize_client(storage, 1_000);
    let ClientEffect::LoadBand { request_id } =
        state.apply_bootstrap_result(1, Ok(load_snapshot(1_000)))
    else {
        panic!("expected initial load")
    };
    state.apply_load_band_result(
        request_id,
        Ok(WebSuccess {
            snapshot: load_snapshot(1_000),
            data: load_data(Vec::new()),
        }),
    );
    reduce_component_action_at(
        &mut state,
        storage,
        1_000,
        ComponentAction::AddSession {
            task: task(RECORD_ID),
            is_leaf: true,
        },
    );
    state
}

pub(super) fn list_response(effect: ClientEffect, fail: bool) -> ClientResponse {
    match effect {
        ClientEffect::ListTasks {
            request_id,
            request,
        } => ClientResponse::ListTasks {
            request_id,
            requested_date: request.logical_date,
            result: if fail {
                Err(ServerFailure::Transport("offline".to_owned()))
            } else {
                Ok(WebSuccess {
                    snapshot: load_snapshot(1_200),
                    data: Vec::new(),
                })
            },
        },
        ClientEffect::ListCompletedTasks {
            request_id,
            request,
        } => ClientResponse::ListCompletedTasks {
            request_id,
            requested_date: request.logical_date,
            result: if fail {
                Err(ServerFailure::Transport("offline".to_owned()))
            } else {
                Ok(WebSuccess {
                    snapshot: load_snapshot(1_200),
                    data: crate::CompletedTaskReport {
                        rows: Vec::new(),
                        in_progress_actual_work_seconds: None,
                        total_actual_work_seconds: 0,
                        available_seconds: 0,
                        recorded_percentage: None,
                    },
                })
            },
        },
        other => panic!("expected active list request: {other:?}"),
    }
}

#[test]
fn 完了成功後は表示tabと一覧modeにかかわらず一覧に続けて負荷を一度取得する() {
    for tab in [
        ActiveTab::Session,
        ActiveTab::List,
        ActiveTab::Load,
        ActiveTab::Create,
    ] {
        for mode in [ListMode::Scheduled, ListMode::Completed] {
            for record in [true, false] {
                for fail_list in [false, true] {
                    let storage = MemoryStorage::default();
                    let mut state = session_state(&storage);
                    state.switch_tab(tab);
                    state.switch_list_mode(mode);
                    let effect = if record {
                        state.begin_complete_session(&storage, RECORD_ID)
                    } else {
                        state.begin_complete_session_without_recording(&storage, RECORD_ID)
                    };
                    let ClientEffect::CompleteSession { request_id, .. } = effect else {
                        panic!("expected completion request: {effect:?}")
                    };
                    let response = || ClientResponse::CompleteSession {
                        request_id,
                        result: Ok(crate::WebSuccess {
                            snapshot: load_snapshot(1_100),
                            data: None,
                        }),
                    };
                    let list = apply_response(&mut state, &storage, response(), false);
                    assert_eq!(state.active_tab(), tab);
                    assert_eq!(state.list_mode(), mode);
                    assert!(!state.band_loading(), "一覧取得中に負荷取得を開始しない");
                    assert_eq!(
                        apply_response(&mut state, &storage, response(), false),
                        ClientEffect::None
                    );
                    let response = || list_response(list.clone(), fail_list);
                    let load = apply_response(&mut state, &storage, response(), false);
                    let ClientEffect::LoadBand { request_id } = load else {
                        panic!("expected workload request: {load:?}")
                    };
                    assert_eq!(
                        apply_response(&mut state, &storage, response(), false),
                        ClientEffect::None
                    );
                    assert_eq!(
                        apply_response(
                            &mut state,
                            &storage,
                            ClientResponse::LoadBand {
                                request_id,
                                result: Ok(WebSuccess {
                                    snapshot: load_snapshot(1_300),
                                    data: load_data(vec![band_day("2026-09-27", 2)]),
                                }),
                            },
                            false
                        ),
                        ClientEffect::None
                    );
                    assert_eq!(state.band_rows()[0].durations.unavailable_seconds, 2);
                }
            }
        }
    }
}

#[test]
fn 完了失敗では負荷を取得せず競合再送が成功した時だけ一覧の後で取得する() {
    for conflict in [false, true] {
        let storage = MemoryStorage::default();
        let mut state = session_state(&storage);
        let ClientEffect::CompleteSession { request_id, .. } =
            state.begin_complete_session(&storage, RECORD_ID)
        else {
            panic!("expected completion request")
        };
        let error = if conflict {
            ServerFailure::Operation(WebError {
                code: web_error_codes::ACTUAL_WORK_CONFLICT.to_owned(),
                message: "actual work changed".to_owned(),
                retry_advice: RetryAdvice::ManualCheck,
                current_actual_work_seconds: Some(250),
            })
        } else {
            ServerFailure::Transport("offline".to_owned())
        };
        assert_eq!(
            apply_response(
                &mut state,
                &storage,
                ClientResponse::CompleteSession {
                    request_id,
                    result: Err(error),
                },
                false
            ),
            ClientEffect::None
        );
        let unrelated_list = state.request_list("2026-09-27");
        let ClientEffect::LoadBand { request_id } = apply_response(
            &mut state,
            &storage,
            list_response(unrelated_list, false),
            false,
        ) else {
            panic!("expected list-linked load")
        };
        state.apply_load_band_result(
            request_id,
            Ok(WebSuccess {
                snapshot: load_snapshot(1_200),
                data: load_data(Vec::new()),
            }),
        );
        if conflict {
            let ClientEffect::CompleteSession { request_id, .. } =
                state.confirm_completion_conflict(&storage, RECORD_ID)
            else {
                panic!("expected conflict retry")
            };
            let list = apply_response(
                &mut state,
                &storage,
                ClientResponse::CompleteSession {
                    request_id,
                    result: Ok(crate::WebSuccess {
                        snapshot: load_snapshot(1_300),
                        data: None,
                    }),
                },
                false,
            );
            assert!(matches!(
                apply_response(&mut state, &storage, list_response(list, false), false),
                ClientEffect::LoadBand { .. }
            ));
        }
    }
}

#[test]
fn 記録解除と作業区間完了は一覧に続けて負荷を取得する() {
    for work_slice in [false, true] {
        let storage = MemoryStorage::default();
        let mut state = session_state(&storage);
        let effect = if work_slice {
            state.begin_complete_work_slice(&storage, RECORD_ID)
        } else {
            state.begin_record_session(&storage, RECORD_ID)
        };
        let result = Ok(WebSuccess {
            snapshot: load_snapshot(1_100),
            data: RecordSessionResult {
                actual_work_seconds: 100,
            },
        });
        let response = match effect {
            ClientEffect::RecordSession { request_id, .. } => {
                ClientResponse::RecordSession { request_id, result }
            }
            ClientEffect::CompleteWorkSlice { request_id, .. } => {
                ClientResponse::CompleteWorkSlice { request_id, result }
            }
            other => panic!("expected recording request: {other:?}"),
        };
        let list = apply_response(&mut state, &storage, response, false);
        assert!(matches!(
            apply_response(&mut state, &storage, list_response(list, false), false),
            ClientEffect::LoadBand { .. }
        ));
    }
}

#[test]
fn 全tabの復元でbootstrapと保存一覧の更新後に一度だけ負荷を背景取得する() {
    for tab in [
        ActiveTab::Session,
        ActiveTab::List,
        ActiveTab::Load,
        ActiveTab::Create,
    ] {
        for mode in [ListMode::Scheduled, ListMode::Completed] {
            for has_list in [false, true] {
                for fail_list in [false, true] {
                    let storage = MemoryStorage::default();
                    let list = has_list.then(|| match mode {
                        ListMode::Scheduled => StoredActiveList::Scheduled {
                            logical_date: "2026-09-28".to_owned(),
                            rows: Vec::new(),
                        },
                        ListMode::Completed => StoredActiveList::Completed {
                            logical_date: "2026-09-28".to_owned(),
                            rows: Vec::new(),
                            in_progress_actual_work_seconds: None,
                            total_actual_work_seconds: 0,
                            available_seconds: 0,
                            recorded_percentage: None,
                        },
                    });
                    store_view_state(
                        &storage,
                        &ViewState {
                            snapshot: load_snapshot(900),
                            list_mode: mode,
                            list,
                            active_tab: tab,
                            task_name_filter: String::new(),
                            date_input_text: String::new(),
                        },
                    )
                    .unwrap();
                    let mut client = ComponentOrchestrator::new();
                    let ClientEffect::Bootstrap { request_id } = client.mount(&storage, 1_000)
                    else {
                        panic!("expected bootstrap")
                    };
                    let mut effect = client.apply_response(
                        &storage,
                        ClientResponse::Bootstrap {
                            request_id,
                            result: Ok(load_snapshot(1_000)),
                        },
                    );
                    assert_eq!(client.state().unwrap().active_tab(), tab);
                    if has_list {
                        assert!(!client.state().unwrap().band_loading());
                        assert!(client.effect_is_background(&effect));
                        effect = client.apply_response(&storage, list_response(effect, fail_list));
                    }
                    assert!(matches!(effect, ClientEffect::LoadBand { .. }));
                    assert!(client.effect_is_background(&effect));
                    assert_eq!(client.refresh_failed(), has_list && fail_list);
                    let ClientEffect::LoadBand { request_id } = effect else {
                        unreachable!()
                    };
                    assert_eq!(
                        client.apply_response(
                            &storage,
                            ClientResponse::LoadBand {
                                request_id,
                                result: Ok(WebSuccess {
                                    snapshot: load_snapshot(1_300),
                                    data: load_data(Vec::new())
                                })
                            }
                        ),
                        ClientEffect::None
                    );
                    assert_eq!(client.refresh_failed(), has_list && fail_list);
                }
            }
        }
    }
}

#[test]
fn 完了後の負荷取得失敗は表示を維持し手動更新で再試行できる() {
    let storage = MemoryStorage::default();
    let mut state = session_state(&storage);
    let ClientEffect::LoadBand { request_id } = state.request_load_band() else {
        panic!("expected load")
    };
    state.apply_load_band_result(
        request_id,
        Ok(WebSuccess {
            snapshot: load_snapshot(1_050),
            data: load_data(vec![band_day("2026-09-27", 2)]),
        }),
    );
    let ClientEffect::CompleteSession { request_id, .. } =
        state.begin_complete_session(&storage, RECORD_ID)
    else {
        panic!("expected completion")
    };
    let list = apply_response(
        &mut state,
        &storage,
        ClientResponse::CompleteSession {
            request_id,
            result: Ok(crate::WebSuccess {
                snapshot: load_snapshot(1_100),
                data: None,
            }),
        },
        false,
    );
    let ClientEffect::LoadBand { request_id } =
        apply_response(&mut state, &storage, list_response(list, false), false)
    else {
        panic!("expected load")
    };
    apply_response(
        &mut state,
        &storage,
        ClientResponse::LoadBand {
            request_id,
            result: Err(ServerFailure::Transport("offline".to_owned())),
        },
        false,
    );
    assert_eq!(state.band_rows()[0].durations.unavailable_seconds, 2);
    assert_eq!(state.band_observed_at_epoch_ms(), Some(1_050));
    assert!(state.band_error().is_some());
    assert!(!state.band_loading());
    assert!(matches!(
        reduce_component_action_at(&mut state, &storage, 1_500, ComponentAction::RefreshLoad),
        ClientEffect::LoadBand { .. }
    ));
}

#[test]
fn 先送り成功は一覧に続けて負荷を取得する() {
    let storage = MemoryStorage::default();
    let (mut state, _) = initialize_client(&storage, 1_000);
    let ClientEffect::LoadBand { request_id } =
        state.apply_bootstrap_result(1, Ok(load_snapshot(1_000)))
    else {
        panic!("expected initial load")
    };
    state.apply_load_band_result(
        request_id,
        Ok(WebSuccess {
            snapshot: load_snapshot(1_000),
            data: load_data(Vec::new()),
        }),
    );
    let ClientEffect::DeferTask { request_id, .. } = state.request_defer_task(
        &storage,
        RECORD_ID,
        "2026-09-27",
        crate::DeferPlan {
            mode: crate::DeferMode::Normal,
            requested_pending_until_epoch_ms: 2_000,
            effective_pending_until_epoch_ms: None,
            repetition_interval_days: None,
        },
    ) else {
        panic!("expected defer")
    };
    let list = apply_response(
        &mut state,
        &storage,
        ClientResponse::DeferTask {
            request_id,
            result: Ok(load_snapshot(1_100)),
        },
        false,
    );
    assert!(matches!(
        apply_response(&mut state, &storage, list_response(list, false), false),
        ClientEffect::LoadBand { .. }
    ));
}
