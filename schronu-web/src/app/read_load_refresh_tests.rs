use super::load_refresh_tests::{list_response, session_state};
use super::*;
use crate::app::effect_dispatcher::apply_response;
use crate::client::state::ListMode;

#[test]
fn 初回bootstrapの成功失敗とも負荷を背景取得し重複応答を無視する() {
    for fail in [false, true] {
        let storage = MemoryStorage::default();
        let mut client = ComponentOrchestrator::new();
        let ClientEffect::Bootstrap { request_id } = client.mount(&storage, 1_000) else {
            panic!("expected bootstrap")
        };
        let response = || ClientResponse::Bootstrap {
            request_id,
            result: if fail {
                Err(ServerFailure::Transport("offline".to_owned()))
            } else {
                Ok(load_snapshot(1_000))
            },
        };
        let load = client.apply_response(&storage, response());
        assert!(matches!(load, ClientEffect::LoadBand { .. }));
        assert!(client.effect_is_background(&load));
        assert_eq!(client.refresh_failed(), fail);
        assert_eq!(
            client.apply_response(&storage, response()),
            ClientEffect::None
        );
        let ClientEffect::LoadBand { request_id } = load else {
            unreachable!()
        };
        client.apply_response(
            &storage,
            ClientResponse::LoadBand {
                request_id,
                result: Ok(WebSuccess {
                    snapshot: load_snapshot(1_200),
                    data: load_data(Vec::new()),
                }),
            },
        );
        assert_eq!(client.refresh_failed(), fail);
        assert_eq!(client.state().unwrap().display_error().is_some(), fail);
    }
}

#[test]
fn 通常一覧の成功失敗とも負荷を一度取得する() {
    for mode in [ListMode::Scheduled, ListMode::Completed] {
        for fail in [false, true] {
            let storage = MemoryStorage::default();
            let mut state = session_state(&storage);
            state.switch_list_mode(mode);
            let list = state.request_active_list("2026-09-27");
            let load = apply_response(
                &mut state,
                &storage,
                list_response(list.clone(), fail),
                false,
            );
            assert!(matches!(load, ClientEffect::LoadBand { .. }));
            assert_eq!(
                apply_response(&mut state, &storage, list_response(list, fail), false),
                ClientEffect::None
            );
        }
    }
}

#[test]
fn 自動セッションは候補ありなし失敗とも負荷を一度取得する() {
    for minutes in [None, Some(30)] {
        for outcome in 0..3 {
            let storage = MemoryStorage::default();
            let mut state = session_state(&storage);
            let ClientEffect::AutoSession { request_id, .. } =
                state.request_auto_session_with_minutes(minutes)
            else {
                panic!("expected auto session")
            };
            let response = || ClientResponse::AutoSession {
                request_id,
                result: if outcome == 2 {
                    Err(ServerFailure::Transport("offline".to_owned()))
                } else {
                    Ok(WebSuccess {
                        snapshot: load_snapshot(1_100),
                        data: (outcome == 0).then(|| task(COMPLETE_ID)),
                    })
                },
            };
            assert!(matches!(
                apply_response(&mut state, &storage, response(), false),
                ClientEffect::LoadBand { .. }
            ));
            assert_eq!(
                apply_response(&mut state, &storage, response(), false),
                ClientEffect::None
            );
        }
    }
}

#[test]
fn 全件取得は最終pageまたは途中失敗後だけ負荷を背景取得する() {
    for fail in [false, true] {
        let storage = MemoryStorage::default();
        let mut client = ComponentOrchestrator::new();
        let ClientEffect::Bootstrap { request_id } = client.mount(&storage, 1_000) else {
            panic!("expected bootstrap")
        };
        let initial = client.apply_response(
            &storage,
            ClientResponse::Bootstrap {
                request_id,
                result: Ok(load_snapshot(1_000)),
            },
        );
        if let ClientEffect::LoadBand { request_id } = initial {
            client.apply_response(
                &storage,
                ClientResponse::LoadBand {
                    request_id,
                    result: Ok(WebSuccess {
                        snapshot: load_snapshot(1_000),
                        data: load_data(Vec::new()),
                    }),
                },
            );
        }
        for _ in 0..2 {
            let first = client.action(
                &storage,
                1_000,
                if client.state().unwrap().all_tasks_failure().is_some() {
                    ComponentAction::RetryAllTasks
                } else {
                    ComponentAction::SelectAllTasks
                },
            );
            let ClientEffect::ListAllTasks {
                request_id,
                request,
            } = first
            else {
                panic!("expected first page")
            };
            let next = client.apply_response(
                &storage,
                ClientResponse::ListAllTasks {
                    request_id,
                    request,
                    result: Ok(WebSuccess {
                        snapshot: load_snapshot(1_100),
                        data: crate::AllTaskPage {
                            rows: Vec::new(),
                            next_cursor: Some("next".to_owned()),
                        },
                    }),
                },
            );
            let ClientEffect::ListAllTasks {
                request_id,
                request,
            } = next
            else {
                panic!("expected next page")
            };
            assert!(!client.state().unwrap().band_loading());
            let response = || ClientResponse::ListAllTasks {
                request_id,
                request: request.clone(),
                result: if fail {
                    Err(ServerFailure::Transport("offline".to_owned()))
                } else {
                    Ok(WebSuccess {
                        snapshot: load_snapshot(1_200),
                        data: crate::AllTaskPage {
                            rows: Vec::new(),
                            next_cursor: None,
                        },
                    })
                },
            };
            let load = client.apply_response(&storage, response());
            assert!(matches!(load, ClientEffect::LoadBand { .. }));
            assert!(client.effect_is_background(&load));
            assert_eq!(
                client.apply_response(&storage, response()),
                ClientEffect::None
            );
            let ClientEffect::LoadBand { request_id } = load else {
                unreachable!()
            };
            client.apply_response(
                &storage,
                ClientResponse::LoadBand {
                    request_id,
                    result: Ok(WebSuccess {
                        snapshot: load_snapshot(1_200),
                        data: load_data(Vec::new()),
                    }),
                },
            );
            if !fail {
                break;
            }
        }
    }
}

#[test]
fn 利用者起点の一覧と自動セッションに連動する負荷は全面待機の対象になる() {
    for auto in [false, true] {
        let storage = MemoryStorage::default();
        let mut client = ComponentOrchestrator::new();
        let ClientEffect::Bootstrap { request_id } = client.mount(&storage, 1_000) else {
            panic!("expected bootstrap")
        };
        client.apply_response(
            &storage,
            ClientResponse::Bootstrap {
                request_id,
                result: Ok(load_snapshot(1_000)),
            },
        );
        let effect = client.action(
            &storage,
            1_000,
            if auto {
                ComponentAction::AutoSession
            } else {
                ComponentAction::SelectDate("2026-09-27".to_owned())
            },
        );
        let response = if let ClientEffect::AutoSession { request_id, .. } = effect {
            ClientResponse::AutoSession {
                request_id,
                result: Ok(WebSuccess {
                    snapshot: load_snapshot(1_100),
                    data: None,
                }),
            }
        } else {
            list_response(effect, false)
        };
        let load = client.apply_response(&storage, response);
        assert!(matches!(load, ClientEffect::LoadBand { .. }));
        assert!(!client.effect_is_background(&load));
    }
}

#[test]
fn 連動した負荷の成功失敗は先行する読込errorを上書きしない() {
    for source in 0..4 {
        for fail_load in [false, true] {
            let storage = MemoryStorage::default();
            let mut state = session_state(&storage);
            let response = if source == 0 {
                let ClientEffect::Bootstrap { request_id } = state.request_bootstrap() else {
                    panic!("expected bootstrap")
                };
                ClientResponse::Bootstrap {
                    request_id,
                    result: Err(ServerFailure::Transport("offline".to_owned())),
                }
            } else if source >= 2 {
                let mode = if source == 2 {
                    ListMode::Scheduled
                } else {
                    ListMode::Completed
                };
                state.switch_list_mode(mode);
                let list = state.request_active_list("2026-09-27");
                list_response(list, true)
            } else {
                let ClientEffect::AutoSession { request_id, .. } = state.request_auto_session()
                else {
                    panic!("expected auto session")
                };
                ClientResponse::AutoSession {
                    request_id,
                    result: Err(ServerFailure::Transport("offline".to_owned())),
                }
            };
            let ClientEffect::LoadBand { request_id } =
                apply_response(&mut state, &storage, response, false)
            else {
                panic!("expected load")
            };
            let original = state.display_error().cloned();
            apply_response(
                &mut state,
                &storage,
                ClientResponse::LoadBand {
                    request_id,
                    result: if fail_load {
                        Err(ServerFailure::Transport("offline".to_owned()))
                    } else {
                        Ok(WebSuccess {
                            snapshot: load_snapshot(1_300),
                            data: load_data(Vec::new()),
                        })
                    },
                },
                false,
            );
            assert!(original.is_some());
            assert_eq!(state.display_error(), original.as_ref());
            assert_eq!(state.band_error().is_some(), fail_load);
        }
    }
}

#[test]
fn 作成成功後は一覧への遷移と取得に続けて負荷を一度取得する() {
    let storage = MemoryStorage::default();
    let mut client = ComponentOrchestrator::new();
    let ClientEffect::Bootstrap { request_id } = client.mount(&storage, 1_000) else {
        panic!("expected bootstrap")
    };
    client.apply_response(
        &storage,
        ClientResponse::Bootstrap {
            request_id,
            result: Ok(load_snapshot(1_000)),
        },
    );
    assert_eq!(
        client.action(
            &storage,
            1_000,
            ComponentAction::SwitchTab(ActiveTab::Create)
        ),
        ClientEffect::None
    );
    client.edit_create_name("contract task".to_owned());
    let ClientEffect::CreateTask { request_id, .. } = client.submit_create_task(&storage, 1_000)
    else {
        panic!("expected creation")
    };
    assert_eq!(
        client.apply_response(
            &storage,
            ClientResponse::CreateTask {
                request_id,
                result: Ok(WebSuccess {
                    snapshot: load_snapshot(1_100),
                    data: CreateTaskResult {
                        task_id: COMPLETE_ID.to_owned(),
                        pending_until_epoch_ms: 2_000
                    },
                })
            }
        ),
        ClientEffect::None
    );
    let list = client.action(&storage, 1_200, ComponentAction::SwitchTab(ActiveTab::List));
    assert!(matches!(list, ClientEffect::ListTasks { .. }));
    let response = || list_response(list.clone(), false);
    let load = client.apply_response(&storage, response());
    assert!(matches!(load, ClientEffect::LoadBand { .. }));
    assert!(!client.effect_is_background(&load));
    assert_eq!(
        client.apply_response(&storage, response()),
        ClientEffect::None
    );
}
