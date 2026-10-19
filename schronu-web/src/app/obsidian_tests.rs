use super::component_runtime::{ComponentAction, ComponentOrchestrator};
use super::obsidian::{open_obsidian, refresh_requests, supports_obsidian};
use super::session_links::SessionLinks;
use super::session_view::{SessionAction, SessionView};
use super::view_test_support::{dispatch_click, rebuild_with_click_listeners};
use crate::client::state::{load_client_state, ActiveTab};
use crate::client::view_projection::project_session_cards;
use crate::client::work_sessions::{KeyValueStorage, StorageError, WORK_SESSIONS_STORAGE_KEY};
use dioxus::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

const START: i64 = 1_800_000_000_000;
const TASK: &str = "00000000-0000-0000-0000-000000000002";
const URI: &str = "obsidian://search?vault=Work&query=00000000-0000-0000-0000-000000000001";

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

fn storage() -> Storage {
    Storage {
        values: RefCell::new(HashMap::from([(
            WORK_SESSIONS_STORAGE_KEY.into(),
            serde_json::json!({"version":1,"work_sessions":[{
                "task_id":TASK,"task_name":"検索起動の検証",
                "started_at_epoch_ms":START,
                "estimated_work_seconds_at_start":900,
                "actual_work_seconds_at_start":300
            }]})
            .to_string(),
        )])),
        ..Storage::default()
    }
}

#[test]
fn macだけを許可しipad互換識別と不明な端末を除外する() {
    for (platform, touches, expected) in [
        (Some("MacIntel"), Some(0), true),
        (Some("MacIntel"), Some(1), true),
        (Some("MacIntel"), Some(5), false),
        (Some("iPad"), Some(5), false),
        (Some("iPhone"), Some(5), false),
        (Some("Win32"), Some(0), false),
        (Some("Linux x86_64"), Some(0), false),
        (Some(""), Some(0), false),
        (None, Some(0), false),
        (Some("MacIntel"), None, false),
        (Some("MacIntel"), Some(-1), false),
    ] {
        assert_eq!(
            supports_obsidian(platform, touches),
            expected,
            "{platform:?}/{touches:?}"
        );
        let storage = storage();
        let state = load_client_state(&storage, START).unwrap();
        let mut links = SessionLinks::obsidian();
        let requests = refresh_requests(
            &mut links,
            Some(&state),
            supports_obsidian(platform, touches),
        );
        assert_eq!(requests.len(), usize::from(expected));
        if expected {
            links.apply_result(TASK, requests[0].1, Ok(Some(URI.into())));
        }
        let mut urls = Vec::new();
        let result = open_obsidian(
            &state,
            &links,
            supports_obsidian(platform, touches),
            false,
            TASK,
            |uri| {
                urls.push(uri.to_owned());
                Ok(())
            },
        );
        assert_eq!(result, expected.then_some(Ok(())));
        assert_eq!(urls.len(), usize::from(expected));
    }
}

#[test]
fn 取得状態とタブ再進入と削除後の遅延応答を独立管理する() {
    let storage = storage();
    let mut state = load_client_state(&storage, START).unwrap();
    let mut links = SessionLinks::obsidian();
    assert!(refresh_requests(&mut links, None, true).is_empty());
    let first = refresh_requests(&mut links, Some(&state), true)[0].1;
    assert!(links.url_for(TASK).is_none());
    assert!(refresh_requests(&mut links, Some(&state), true).is_empty());
    links.apply_result(TASK, first, Err("lookup failure".into()));
    assert_eq!(links.error_messages()[TASK], "lookup failure");
    state.switch_tab(ActiveTab::Load);
    assert!(refresh_requests(&mut links, Some(&state), true).is_empty());
    state.switch_tab(ActiveTab::Session);
    let second = refresh_requests(&mut links, Some(&state), true)[0].1;
    assert!(links.error_messages().is_empty());
    links.apply_result(TASK, first, Ok(Some(URI.into())));
    assert!(links.url_for(TASK).is_none());
    links.apply_result(TASK, second, Ok(Some("https://example.com".into())));
    assert!(links.url_for(TASK).is_none());
    assert!(!links.error_messages().is_empty());
    state.switch_tab(ActiveTab::Load);
    refresh_requests(&mut links, Some(&state), true);
    state.switch_tab(ActiveTab::Session);
    let third = refresh_requests(&mut links, Some(&state), true)[0].1;
    links.apply_result(TASK, third, Ok(Some(URI.into())));
    assert_eq!(links.url_for(TASK), Some(URI));
    state.discard_session(&storage, TASK);
    assert!(refresh_requests(&mut links, Some(&state), true).is_empty());
    links.apply_result(TASK, third, Ok(Some(URI.into())));
    assert!(links.url_for(TASK).is_none());
}

#[test]
fn 起動はロック中も許可し通信中と削除済み対象と未取得を拒否する() {
    let storage = storage();
    let mut state = load_client_state(&storage, START).unwrap();
    let mut links = SessionLinks::obsidian();
    let request = refresh_requests(&mut links, Some(&state), true)[0].1;
    assert_eq!(
        open_obsidian(&state, &links, true, false, TASK, |_| panic!("未取得")),
        None
    );
    links.apply_result(TASK, request, Ok(Some(URI.into())));
    state.enable_carry_lock(&storage);
    let writes = storage.writes.get();
    let before = state.sessions().to_vec();
    let mut urls = Vec::new();
    assert_eq!(
        open_obsidian(&state, &links, true, false, TASK, |uri| {
            urls.push(uri.to_owned());
            Ok(())
        }),
        Some(Ok(()))
    );
    assert_eq!(urls, [URI]);
    assert!(state.carry_lock_locked());
    assert_eq!(storage.writes.get(), writes);
    assert_eq!(state.sessions(), before);
    assert_eq!(state.tick_now_epoch_ms(), START);
    for (supported, busy, task) in [
        (false, false, TASK),
        (true, true, TASK),
        (true, false, "missing"),
    ] {
        assert_eq!(
            open_obsidian(&state, &links, supported, busy, task, |_| panic!(
                "起動不可"
            )),
            None
        );
    }
    assert_eq!(
        open_obsidian(&state, &links, true, false, TASK, |_| Err(
            "launch failure".into()
        )),
        Some(Err("launch failure".into()))
    );
    state.switch_tab(ActiveTab::List);
    assert_eq!(
        open_obsidian(&state, &links, true, false, TASK, |_| panic!("別tab")),
        None
    );
}

#[derive(Clone)]
struct Context {
    client: Rc<RefCell<ComponentOrchestrator>>,
    storage: Rc<Storage>,
    links: Rc<RefCell<SessionLinks>>,
    supported: bool,
    timer_supported: bool,
    fail: Rc<Cell<bool>>,
    urls: Rc<RefCell<Vec<String>>>,
}

fn context(supported: bool, ready: bool, locked: bool) -> Context {
    let storage = Rc::new(storage());
    let mut client = ComponentOrchestrator::new();
    client.mount(storage.as_ref(), START);
    let mut links = SessionLinks::obsidian();
    let requests = refresh_requests(&mut links, client.state(), supported);
    if ready && supported {
        links.apply_result(TASK, requests[0].1, Ok(Some(URI.into())));
    }
    if locked {
        client.action(storage.as_ref(), 0, ComponentAction::EnableCarryLock);
    }
    Context {
        client: Rc::new(RefCell::new(client)),
        storage,
        links: Rc::new(RefCell::new(links)),
        supported,
        timer_supported: false,
        fail: Rc::default(),
        urls: Rc::default(),
    }
}

fn root(ctx: Context) -> Element {
    let mut revision = use_signal(|| 0u64);
    let _revision = revision();
    let client = ctx.client.borrow();
    let state = client.state().unwrap();
    let cards = project_session_cards(state, 0);
    let locked = state.carry_lock_locked();
    let busy = client.server_effect_in_flight();
    drop(client);
    let open_ctx = ctx.clone();
    rsx! { SessionView {
        sessions: cards, global_blocked: false, mutations_locked: locked,
        obsidian_supported: ctx.supported,
        obsidian_ready_task_ids: ctx.links.borrow().linkable_task_ids(),
        obsidian_lookup_errors: ctx.links.borrow().error_messages(),
        obsidian_server_busy: busy,
        timer_shortcut_supported: ctx.timer_supported,
        timer_server_busy: busy,
        on_auto_session: |_| {},
        on_action: move |action: SessionAction| {
            for action in super::component_runtime::component_actions_from_session_action(action, START + 1_000) {
                ctx.client.borrow_mut().action(ctx.storage.as_ref(), 0, action);
            }
            revision += 1;
        },
        on_open_obsidian: move |id: String| {
            let client = open_ctx.client.borrow();
            open_obsidian(client.state().unwrap(), &open_ctx.links.borrow(), open_ctx.supported, client.server_effect_in_flight(), &id, |uri| {
                open_ctx.urls.borrow_mut().push(uri.into());
                if open_ctx.fail.get() { Err("起動要求の検証エラー".into()) } else { Ok(()) }
            })
        },
    } }
}

#[test]
fn obsidianはtask名から独立して残り時間と時計の後に並ぶ() {
    let mut ctx = context(true, true, false);
    ctx.timer_supported = true;
    let mut dom = VirtualDom::new_with_props(root, ctx);
    rebuild_with_click_listeners(&mut dom);
    let html = dioxus::ssr::render(&dom);
    let heading = html.find("class=\"session-card-heading\"").unwrap();
    let countdown = html.find("class=\"session-countdown\"").unwrap();
    let remaining = html.find("class=\"session-remaining\"").unwrap();
    let timer = html.find("class=\"session-timer-shortcut\"").unwrap();
    let obsidian = html.find("class=\"session-obsidian-shortcut\"").unwrap();
    let metadata = html.find("class=\"session-timing-meta\"").unwrap();
    assert!(heading < countdown && countdown < remaining);
    assert!(
        remaining < timer && timer < obsidian && obsidian < metadata,
        "{html}"
    );
    assert!(!html[heading..countdown].contains("session-obsidian-shortcut"));
}

#[test]
fn カードclickはロック中と背景更新中も同期起動して保存と通信を増やさない() {
    let ctx = context(true, true, true);
    let mut dom = VirtualDom::new_with_props(root, ctx.clone());
    let ids = rebuild_with_click_listeners(&mut dom);
    let html = dioxus::ssr::render(&dom);
    assert!(html.contains("検索起動の検証: Obsidianでルートタスクを検索"));
    assert!(html.contains("title=\"Obsidianでルートタスクを検索\""));
    assert!(html.contains("width=\"18\""));
    let writes = ctx.storage.writes.get();
    assert!(ctx.client.borrow().background_refreshing());
    for id in ids {
        dispatch_click(&dom, id);
    }
    assert_eq!(*ctx.urls.borrow(), [URI]);
    assert_eq!(ctx.storage.writes.get(), writes);
    assert!(!ctx.client.borrow().server_effect_in_flight());
    assert!(ctx.client.borrow().state().unwrap().carry_lock_locked());
    dom.render_immediate_to_vec();
    assert!(!dioxus::ssr::render(&dom).contains("session-obsidian-message"));
}

#[test]
fn 非対応端末はボタンを生成せず取得中と失敗時は無効化する() {
    for supported in [false, true] {
        let ctx = context(supported, false, true);
        if supported {
            let requests = ctx.links.borrow().linkable_task_ids();
            assert!(requests.is_empty());
        }
        let mut dom = VirtualDom::new_with_props(root, ctx.clone());
        let ids = rebuild_with_click_listeners(&mut dom);
        let html = dioxus::ssr::render(&dom);
        assert_eq!(html.contains("session-obsidian-shortcut"), supported);
        for id in ids {
            dispatch_click(&dom, id);
        }
        assert!(ctx.urls.borrow().is_empty());
    }
    let ctx = context(true, false, true);
    ctx.links
        .borrow_mut()
        .apply_result(TASK, 1, Err("lookup failure".into()));
    let mut dom = VirtualDom::new_with_props(root, ctx.clone());
    let ids = rebuild_with_click_listeners(&mut dom);
    assert!(dioxus::ssr::render(&dom).contains("セッションタブを開き直してください。"));
    for id in ids {
        dispatch_click(&dom, id);
    }
    assert!(ctx.urls.borrow().is_empty());
}

#[test]
fn 同期起動エラーは再clickと再開で消去できる() {
    let ctx = context(true, true, false);
    ctx.fail.set(true);
    let mut dom = VirtualDom::new_with_props(root, ctx.clone());
    let ids = rebuild_with_click_listeners(&mut dom);
    let obsidian = *ids.last().unwrap();
    dispatch_click(&dom, obsidian);
    dom.render_immediate_to_vec();
    assert!(dioxus::ssr::render(&dom).contains("起動要求の検証エラー"));
    ctx.fail.set(false);
    dispatch_click(&dom, obsidian);
    dom.render_immediate_to_vec();
    assert!(!dioxus::ssr::render(&dom).contains("session-obsidian-message"));
    ctx.fail.set(true);
    dispatch_click(&dom, obsidian);
    dom.render_immediate_to_vec();
    // 子componentのlistenerはDOM順の逆順に生成される。
    dispatch_click(&dom, ids[3]);
    dom.render_immediate_to_vec();
    assert!(!dioxus::ssr::render(&dom).contains("session-obsidian-message"));
    assert_eq!(
        ctx.client.borrow().state().unwrap().sessions()[0].started_at_epoch_ms,
        START + 1_000
    );
}
