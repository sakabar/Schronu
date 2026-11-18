mod persistence;

use super::component::{BrowserApp, RestoringShell};
use super::offline_view::OfflineView;
use crate::client::work_sessions::{BrowserLocalStorage, KeyValueStorage, WorkSession};
use dioxus::prelude::*;
use persistence::{Saved, MODE_NOTICE};
use std::rc::Rc;
use wasm_bindgen::{closure::Closure, JsCast};
use web_sys::{Event, Window};

#[derive(Clone)]
enum Screen {
    Restoring,
    Online,
    Offline(Rc<Saved>),
    RestoreFailed,
}

#[derive(Clone, Copy)]
struct Host {
    screen: Signal<Screen>,
    busy: Signal<bool>,
    message: Signal<Option<String>>,
    error: Signal<Option<String>>,
}

impl Host {
    fn start(&mut self) -> bool {
        if *self.busy.peek() {
            return false;
        }
        self.busy.set(true);
        self.error.set(None);
        self.message.set(None);
        true
    }

    async fn restore(mut self) {
        match persistence::read().await {
            Ok(Some(saved)) if saved.data.returning => match saved.handoff().await {
                Ok(()) => self.screen.set(Screen::Online),
                Err(error) => {
                    self.error.set(Some(error));
                    self.screen.set(Screen::RestoreFailed);
                }
            },
            Ok(Some(saved)) => self.screen.set(Screen::Offline(Rc::new(saved))),
            Ok(None) => self.screen.set(Screen::Online),
            Err(error) => {
                // Ordinary online use still works in browsers without IndexedDB.
                // A known prepared trip must be recovered before issuing new writes.
                let prepared = BrowserLocalStorage
                    .get(MODE_NOTICE)
                    .ok()
                    .flatten()
                    .is_some_and(|mode| mode == "offline");
                self.error.set(Some(error));
                self.screen.set(if prepared {
                    Screen::RestoreFailed
                } else {
                    Screen::Online
                });
            }
        }
        self.busy.set(false);
    }

    fn prepare(mut self, sessions: Vec<WorkSession>) {
        if !self.start() {
            return;
        }
        self.message.set(Some(
            "予定7日分・負荷・オフライン画面を保存しています…".into(),
        ));
        spawn(async move {
            match persistence::prepare(sessions).await {
                Ok(saved) => {
                    self.screen.set(Screen::Offline(Rc::new(saved)));
                    self.message
                        .set(Some("オフラインの準備が完了しました。".into()));
                    self.busy.set(false);
                }
                Err(error) => self.recover(error).await,
            }
        });
    }

    async fn recover(mut self, error: String) {
        self.message.set(None);
        self.error.set(Some(error));
        // Reload the winner after a competing tab's compare-and-set or a failed
        // checkpoint. In-memory optimistic edits never become the source of truth.
        self.restore().await;
    }

    fn edit(mut self, previous: &Saved, candidate: crate::client::offline::OfflineState) {
        if !self.start() {
            return;
        }
        let mut saved = previous.clone();
        spawn(async move {
            match saved.update(candidate).await {
                Ok(()) => {
                    self.screen.set(Screen::Offline(Rc::new(saved)));
                    self.busy.set(false);
                }
                Err(error) => self.recover(error).await,
            }
        });
    }

    fn sync(mut self, previous: &Saved) {
        if !self.start() {
            return;
        }
        self.message.set(Some("サーバーと同期しています…".into()));
        let mut saved = previous.clone();
        spawn(async move {
            match saved.sync().await {
                Ok(count) => {
                    let conflicts = saved.data.state.conflicts.len();
                    self.message.set(Some(format!(
                        "同期済み {count}件・要確認 {conflicts}タスク"
                    )));
                    self.screen.set(if saved.data.returning {
                        Screen::Online
                    } else {
                        Screen::Offline(Rc::new(saved))
                    });
                    self.busy.set(false);
                }
                Err(error) => self.recover(error).await,
            }
        });
    }
}

struct ModeListener(Window, Closure<dyn FnMut(Event)>);

impl ModeListener {
    fn attach(mut host: Host) -> Option<Self> {
        let window = web_sys::window()?;
        let callback = Closure::new(move |event: Event| {
            let key = js_sys::Reflect::get(event.as_ref(), &"key".into()).ok();
            if key.and_then(|key| key.as_string()).as_deref() == Some(MODE_NOTICE) && host.start() {
                spawn(host.restore());
            }
        });
        window
            .add_event_listener_with_callback("storage", callback.as_ref().unchecked_ref())
            .ok()?;
        Some(Self(window, callback))
    }
}

impl Drop for ModeListener {
    fn drop(&mut self) {
        let _ = self
            .0
            .remove_event_listener_with_callback("storage", self.1.as_ref().unchecked_ref());
    }
}

#[component]
pub(super) fn OfflineHost() -> Element {
    let mut host = Host {
        screen: use_signal(|| Screen::Restoring),
        busy: use_signal(|| true),
        message: use_signal(|| None),
        error: use_signal(|| None),
    };
    use_hook(move || ModeListener::attach(host).map(Rc::new));
    use_future(move || host.restore());
    let screen = host.screen.read().clone();
    // The first browser render must hydrate the same tree AppBody emitted in SSR.
    if matches!(screen, Screen::Restoring) {
        return rsx! { RestoringShell {} };
    }
    let busy = (host.busy)();
    rsx! {
        if let Some(message) = (host.message)() {
            p { class: "offline-notice", role: "status", "{message}" }
        }
        if let Some(error) = (host.error)() {
            p { class: "error", role: "alert", "{error}" }
        }
        match screen {
            Screen::Restoring => unreachable!(),
            Screen::RestoreFailed => rsx! {
                button {
                    r#type: "button", disabled: busy,
                    onclick: move |_| { if host.start() { spawn(host.restore()); } },
                    "保存状態を再確認"
                }
            },
            Screen::Online => rsx! {
                div {
                    inert: busy.then_some("true"), aria_busy: busy,
                    BrowserApp { on_prepare: move |sessions| host.prepare(sessions) }
                }
            },
            Screen::Offline(saved) => {
                let previous = saved.clone();
                rsx! { OfflineView {
                    state: saved.data.state.clone(),
                    busy,
                    on_change: move |candidate| host.edit(&previous, candidate),
                    on_sync: move |_| host.sync(&saved),
                } }
            }
        }
    }
}
