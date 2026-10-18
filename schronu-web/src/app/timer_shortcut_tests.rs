use super::component_runtime::{ComponentAction, ComponentOrchestrator};
use super::session_view::{SessionAction, SessionView};
use super::timer_shortcut::{open_timer_shortcut, supports_timer_shortcut, timer_enabled};
use super::view_test_support::{dispatch_click, rebuild_with_click_listeners};
use crate::client::state::{load_client_state, ActiveTab};
use crate::client::view_projection::{project_session_cards, CompletionConflictViewModel};
use crate::client::work_sessions::{
    KeyValueStorage, StorageError, WorkSession, WORK_SESSIONS_STORAGE_KEY,
};
use dioxus::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

const START: i64 = 1_800_000_000_000;
const TASK: &str = "550e8400-e29b-41d4-a716-446655440000";

#[derive(Default)]
struct Storage {
    values: RefCell<HashMap<String, String>>,
    writes: Cell<usize>,
}

impl KeyValueStorage for Storage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        Ok(self.values.borrow().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), StorageError> {
        self.writes.set(self.writes.get() + 1);
        self.values.borrow_mut().insert(key.into(), value.into());
        Ok(())
    }
}

fn storage(estimated: i64, actual: i64) -> Storage {
    let session = WorkSession {
        task_id: TASK.into(),
        task_name: "残り時間の検証".into(),
        started_at_epoch_ms: START,
        estimated_work_seconds_at_start: estimated,
        actual_work_seconds_at_start: actual,
    };
    Storage {
        values: RefCell::new(HashMap::from([(
            WORK_SESSIONS_STORAGE_KEY.into(),
            serde_json::json!({"version":1,"work_sessions":[session]}).to_string(),
        )])),
        ..Storage::default()
    }
}

#[test]
fn 端末判定はmacとipadを許可しiphoneと不明な端末を除外する() {
    for (platform, expected) in [
        (Some("MacIntel"), true),
        (Some("MacPPC"), true),
        (Some("iPad"), true),
        (Some("iPhone"), false),
        (Some("iPod"), false),
        (Some("Win32"), false),
        (Some("Linux armv8l"), false),
        (Some(""), false),
        (None, false),
    ] {
        assert_eq!(supports_timer_shortcut(platform), expected, "{platform:?}");
    }
}

#[test]
fn クリック時刻で補正済み残り秒を計算し正値だけを渡す() {
    for (estimated, actual, elapsed, expected) in [
        (900, 300, 4_999, Some(596)),
        (1800, 2400, 4_999, Some(1196)),
        (900, 300, 599_999, Some(1)),
        (900, 300, 600_000, None),
        (900, 300, 601_000, None),
        (0, 0, 0, None),
    ] {
        let storage = storage(estimated, actual);
        let mut state = load_client_state(&storage, START).unwrap();
        let before = state.sessions().to_vec();
        let mut urls = Vec::new();
        let result =
            open_timer_shortcut(&mut state, true, false, TASK, START + elapsed, 0, |url| {
                urls.push(url.to_owned());
                Ok(())
            });
        assert_eq!(result, expected.map(|_| Ok(())));
        assert_eq!(
            urls,
            expected
                .map(|seconds| format!(
                    "shortcuts://run-shortcut?name=TimerForSchronu&input=text&text={seconds}"
                ))
                .into_iter()
                .collect::<Vec<_>>()
        );
        assert_eq!(state.sessions(), before);
        assert_eq!(state.tick_now_epoch_ms(), START);
        assert_eq!(storage.writes.get(), 0);
    }
}

#[test]
fn ハンドラーは端末と通信とロック期限と削除済み対象を再確認する() {
    let storage = storage(900, 300);
    let mut state = load_client_state(&storage, START).unwrap();
    let never_open = |_: &str| -> Result<(), String> {
        panic!("起動してはいけない");
    };
    assert_eq!(
        open_timer_shortcut(&mut state, false, false, TASK, START, 0, never_open),
        None
    );
    assert_eq!(
        open_timer_shortcut(&mut state, true, true, TASK, START, 0, never_open),
        None
    );
    assert_eq!(
        open_timer_shortcut(&mut state, true, false, "missing", START, 0, never_open),
        None
    );
    state.enable_carry_lock(&storage);
    assert_eq!(
        open_timer_shortcut(&mut state, true, false, TASK, START, 0, never_open),
        None
    );
    state.arm_carry_lock(0);
    assert_eq!(
        open_timer_shortcut(&mut state, true, false, TASK, START, 15_000, never_open),
        None
    );
    state.arm_carry_lock(20_000);
    assert_eq!(
        open_timer_shortcut(&mut state, true, false, TASK, START, 19_999, never_open),
        None
    );
    state.disable_carry_lock(&storage);
    state.switch_tab(ActiveTab::Load);
    assert_eq!(
        open_timer_shortcut(&mut state, true, false, TASK, START, 20_000, never_open),
        None
    );
}

#[test]
fn 終了処理中のセッションは直接要求しても起動しない() {
    let storage = storage(900, 300);
    let mut state = load_client_state(&storage, START + 1_000).unwrap();
    state.begin_record_session(&storage, TASK);
    assert!(state.is_session_in_flight(TASK));
    assert_eq!(
        open_timer_shortcut(&mut state, true, false, TASK, START + 2_000, 0, |_| panic!(
            "終了処理中"
        )),
        None
    );
}

#[test]
fn 競合と手動確認と結果不明でもハンドラーが起動を拒否する() {
    use crate::client::state::{ClientEffect, ServerFailure};
    use crate::{web_error_codes, RetryAdvice, WebError};
    for (code, actual) in [
        (web_error_codes::ACTUAL_WORK_CONFLICT, Some(301)),
        (web_error_codes::ACTUAL_WORK_CONFLICT, None),
        (web_error_codes::REPOSITORY_STATE_UNCERTAIN, None),
    ] {
        let storage = storage(900, 300);
        let mut state = load_client_state(&storage, START + 1_000).unwrap();
        let ClientEffect::CompleteSession { request_id, .. } =
            state.begin_complete_session(&storage, TASK)
        else {
            panic!("完了要求が必要");
        };
        state.apply_complete_result(
            &storage,
            request_id,
            Err(ServerFailure::Operation(WebError {
                code: code.into(),
                message: "終了結果の検証".into(),
                retry_advice: RetryAdvice::ManualCheck,
                current_actual_work_seconds: actual,
            })),
        );
        let card = project_session_cards(&state, 0).remove(0);
        assert!(
            card.completion_conflict.is_some()
                || card.manual_check_blocked
                || state.mutation_globally_blocked()
        );
        assert_eq!(
            open_timer_shortcut(&mut state, true, false, TASK, START + 2_000, 0, |_| panic!(
                "結果確認中"
            )),
            None
        );
    }
}

#[test]
fn 競合と手動確認と確定済みを含む全停止条件でボタンを無効化する() {
    let storage = storage(900, 300);
    let state = load_client_state(&storage, START).unwrap();
    let card = project_session_cards(&state, 0).remove(0);
    assert!(timer_enabled(&card, false, false, false));
    for (global, lock, busy) in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        assert!(!timer_enabled(&card, global, lock, busy));
    }
    for kind in 0..6 {
        let mut blocked = card.clone();
        match kind {
            0 => blocked.in_flight = true,
            1 => blocked.manual_check_blocked = true,
            2 => blocked.server_committed = true,
            3 => {
                blocked.completion_conflict = Some(CompletionConflictViewModel {
                    current_actual_work_seconds: 301,
                    measured_elapsed_seconds: 1,
                    record_elapsed_seconds: true,
                })
            }
            4 => blocked.remaining_seconds = 0,
            _ => blocked.remaining_seconds = -1,
        }
        assert!(!timer_enabled(&blocked, false, false, false));
    }
}

#[derive(Clone)]
struct Context {
    client: Rc<RefCell<ComponentOrchestrator>>,
    storage: Rc<Storage>,
    now: Rc<Cell<i64>>,
    supported: Rc<Cell<bool>>,
    fail: Rc<Cell<bool>>,
    urls: Rc<RefCell<Vec<String>>>,
}

fn context(supported: bool) -> Context {
    let storage = Rc::new(storage(900, 300));
    let mut client = ComponentOrchestrator::new();
    client.mount(storage.as_ref(), START);
    Context {
        client: Rc::new(RefCell::new(client)),
        storage,
        now: Rc::new(Cell::new(START)),
        supported: Rc::new(Cell::new(supported)),
        fail: Rc::new(Cell::new(false)),
        urls: Rc::default(),
    }
}

fn root(ctx: Context) -> Element {
    let mut revision = use_signal(|| 0u64);
    let _revision = revision();
    let (cards, global_blocked, locked, busy) = {
        let client = ctx.client.borrow();
        let state = client.state().unwrap();
        (
            project_session_cards(state, 0),
            state.mutation_globally_blocked(),
            state.carry_lock_locked(),
            client.server_effect_in_flight(),
        )
    };
    let open_ctx = ctx.clone();
    rsx! { SessionView {
        sessions: cards, global_blocked, mutations_locked: locked,
        timer_shortcut_supported: ctx.supported.get(), timer_server_busy: busy,
        on_auto_session: |_| {},
        on_action: move |action: SessionAction| {
            for action in super::component_runtime::component_actions_from_session_action(action, ctx.now.get()) {
                ctx.client.borrow_mut().action(ctx.storage.as_ref(), 0, action);
            }
            revision += 1;
        },
        on_open_timer: move |id: String| {
            open_ctx.client.borrow_mut().open_timer_shortcut(&id,open_ctx.supported.get(),open_ctx.now.get(),0,|url| {
                open_ctx.urls.borrow_mut().push(url.into());
                if open_ctx.fail.get() { Err("起動要求の検証エラー".into()) } else { Ok(()) }
            })
        },
    } }
}

#[test]
fn 時計clickは通常表示を追加せず最新秒数を渡し保存と通信を増やさない() {
    let ctx = context(true);
    let mut dom = VirtualDom::new_with_props(root, ctx.clone());
    let mut ids = rebuild_with_click_listeners(&mut dom);
    // 条件付き時計buttonのlistenerは既存の静的操作buttonの後に生成される。
    ids.rotate_right(1);
    let html = dioxus::ssr::render(&dom);
    assert!(html.contains("session-timer-shortcut"));
    assert!(html.contains("残り時間の検証: 残り時間でタイマーを開く"));
    assert!(html.contains("width=\"18\""));
    assert!(ctx.client.borrow().background_refreshing());
    let writes = ctx.storage.writes.get();
    ctx.now.set(START + 4_999);
    dispatch_click(&dom, ids[0]);
    dom.render_immediate_to_vec();
    assert!(
        !dioxus::ssr::render(&dom).contains("session-timer-message"),
        "urls={:?}, html={}",
        ctx.urls.borrow(),
        dioxus::ssr::render(&dom)
    );
    ctx.now.set(START + 6_000);
    dispatch_click(&dom, ids[0]);
    assert_eq!(
        *ctx.urls.borrow(),
        [
            "shortcuts://run-shortcut?name=TimerForSchronu&input=text&text=596",
            "shortcuts://run-shortcut?name=TimerForSchronu&input=text&text=594",
        ]
    );
    assert_eq!(ctx.storage.writes.get(), writes);
    assert!(!ctx.client.borrow().server_effect_in_flight());
    assert_eq!(ctx.client.borrow().state().unwrap().sessions().len(), 1);
    assert_eq!(
        ctx.client.borrow().state().unwrap().sessions()[0].actual_work_seconds_at_start,
        300
    );
}

#[test]
fn 非対応端末では時計を生成せず起動要求しない() {
    let ctx = context(false);
    let mut dom = VirtualDom::new_with_props(root, ctx.clone());
    rebuild_with_click_listeners(&mut dom);
    assert!(!dioxus::ssr::render(&dom).contains("session-timer-shortcut"));
    assert!(ctx.urls.borrow().is_empty());
}

#[test]
fn ipadはどちらのplatform表現でも時計clickから起動できる() {
    for platform in ["MacIntel", "iPad"] {
        let ctx = context(supports_timer_shortcut(Some(platform)));
        let mut dom = VirtualDom::new_with_props(root, ctx.clone());
        let ids = rebuild_with_click_listeners(&mut dom);
        assert!(dioxus::ssr::render(&dom).contains("session-timer-shortcut"));
        dispatch_click(&dom, *ids.last().unwrap());
        assert_eq!(
            *ctx.urls.borrow(),
            ["shortcuts://run-shortcut?name=TimerForSchronu&input=text&text=600"]
        );
    }
}

#[test]
fn 同期エラーから再試行でき再開操作でメッセージを消去する() {
    let ctx = context(true);
    ctx.fail.set(true);
    let mut dom = VirtualDom::new_with_props(root, ctx.clone());
    let mut ids = rebuild_with_click_listeners(&mut dom);
    // 条件付き時計buttonのlistenerは既存の静的操作buttonの後に生成される。
    ids.rotate_right(1);
    dispatch_click(&dom, ids[0]);
    dom.render_immediate_to_vec();
    assert!(
        dioxus::ssr::render(&dom).contains("起動要求の検証エラー"),
        "urls={:?}, html={}",
        ctx.urls.borrow(),
        dioxus::ssr::render(&dom)
    );
    ctx.fail.set(false);
    dispatch_click(&dom, ids[0]);
    dom.render_immediate_to_vec();
    let html = dioxus::ssr::render(&dom);
    assert!(!html.contains("起動要求の検証エラー"));
    assert!(!html.contains("session-timer-message"));
    ctx.fail.set(true);
    dispatch_click(&dom, ids[0]);
    dom.render_immediate_to_vec();
    assert!(dioxus::ssr::render(&dom).contains("起動要求の検証エラー"));
    // 子componentのlistenerはDOM順の逆順に生成される。
    ctx.now.set(START + 1_000);
    dispatch_click(&dom, ids[4]);
    dom.render_immediate_to_vec();
    assert!(!dioxus::ssr::render(&dom).contains("session-timer-message"));
    assert_eq!(
        ctx.client.borrow().state().unwrap().sessions()[0].started_at_epoch_ms,
        START + 1_000
    );
}

#[test]
fn 描画後に端末や通信状態が変わってもハンドラーが拒否する() {
    let ctx = context(true);
    let mut dom = VirtualDom::new_with_props(root, ctx.clone());
    let mut ids = rebuild_with_click_listeners(&mut dom);
    // 条件付き時計buttonのlistenerは既存の静的操作buttonの後に生成される。
    ids.rotate_right(1);
    ctx.supported.set(false);
    dispatch_click(&dom, ids[0]);
    ctx.supported.set(true);
    ctx.client.borrow_mut().begin_server_effect();
    dispatch_click(&dom, ids[0]);
    assert!(ctx.urls.borrow().is_empty());
    ctx.client.borrow_mut().finish_server_effect();
    ctx.client
        .borrow_mut()
        .action(ctx.storage.as_ref(), 0, ComponentAction::EnableCarryLock);
    dispatch_click(&dom, ids[0]);
    assert!(ctx.urls.borrow().is_empty());
}
