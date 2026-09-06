use crate::client::carry_lock::CarryLockMode;
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
#[cfg(all(feature = "web", target_arch = "wasm32"))]
use std::rc::Rc;
#[cfg(all(feature = "web", target_arch = "wasm32"))]
use wasm_bindgen::{closure::Closure, JsCast};

#[cfg_attr(not(all(feature = "web", target_arch = "wasm32")), allow(dead_code))]
pub(crate) const LONG_PRESS_MILLIS: u32 = 1_200;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CarryLockViewModel {
    pub mode: CarryLockMode,
    pub remaining_seconds: u64,
}

impl CarryLockViewModel {
    pub fn new(mode: CarryLockMode, monotonic_now_ms: u64) -> Self {
        let remaining_seconds = match mode {
            CarryLockMode::ArmedUntil(deadline) => {
                deadline
                    .saturating_sub(monotonic_now_ms)
                    .saturating_add(999)
                    / 1_000
            }
            CarryLockMode::Normal | CarryLockMode::Locked => 0,
        };
        Self {
            mode,
            remaining_seconds,
        }
    }

    #[cfg_attr(not(all(feature = "web", target_arch = "wasm32")), allow(dead_code))]
    pub fn mutations_locked(self) -> bool {
        self.mode == CarryLockMode::Locked
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LongPressSource {
    Pointer,
    Keyboard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ActiveLongPress {
    token: u64,
    source: LongPressSource,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct LongPressTracker {
    next_token: u64,
    active: Option<ActiveLongPress>,
}

impl LongPressTracker {
    pub fn begin(&mut self, source: LongPressSource) -> u64 {
        self.next_token = self.next_token.wrapping_add(1);
        let token = self.next_token;
        self.active = Some(ActiveLongPress { token, source });
        token
    }

    pub fn begin_keyboard(&mut self, key: &str, auto_repeating: bool) -> Option<u64> {
        if auto_repeating || !matches!(key, " " | "Enter") || self.active.is_some() {
            return None;
        }
        Some(self.begin(LongPressSource::Keyboard))
    }

    pub fn cancel(&mut self, source: LongPressSource) {
        if self.active.is_some_and(|active| active.source == source) {
            self.active = None;
        }
    }

    pub fn cancel_all(&mut self) -> bool {
        self.active.take().is_some()
    }

    pub fn complete(&mut self, token: u64) -> bool {
        if self.active.is_some_and(|active| active.token == token) {
            self.active = None;
            true
        } else {
            false
        }
    }
}

#[component]
pub(crate) fn CarryLockBar(
    model: CarryLockViewModel,
    on_enable: EventHandler<()>,
    on_arm: EventHandler<()>,
    on_disable: EventHandler<()>,
) -> Element {
    let class = match model.mode {
        CarryLockMode::Normal => "carry-lock-bar is-normal",
        CarryLockMode::Locked => "carry-lock-bar is-locked",
        CarryLockMode::ArmedUntil(_) => "carry-lock-bar is-armed",
    };
    let announcement = match model.mode {
        CarryLockMode::Normal => "通常モード",
        CarryLockMode::Locked => "操作ロック中",
        CarryLockMode::ArmedUntil(_) => "1操作可能",
    };
    let mut tracker = use_signal(LongPressTracker::default);
    let mut pressing = use_signal(|| false);
    #[cfg(all(feature = "web", target_arch = "wasm32"))]
    let _window_scroll_listener = use_hook(|| {
        Rc::new(ScrollCancellationGuard::attach(
            BrowserWindowScrollSource,
            move || {
                let cancelled = tracker.write().cancel_all();
                if cancelled {
                    pressing.set(false);
                }
            },
        ))
    });

    rsx! {
        aside { class, aria_label: "持ち歩きロック状態",
            span {
                class: "carry-lock-live-status",
                aria_live: "polite",
                aria_atomic: "true",
                "{announcement}"
            }
            match model.mode {
                CarryLockMode::Normal => rsx! {
                    button {
                        class: "carry-lock-enable",
                        r#type: "button",
                        aria_pressed: "false",
                        onclick: move |_| on_enable.call(()),
                        "持ち歩きロック"
                    }
                },
                CarryLockMode::Locked => rsx! {
                    div { class: "carry-lock-status",
                        strong { "操作ロック中" }
                        span { "1.2秒長押しで1操作許可" }
                    }
                    button {
                        class: if pressing() { "carry-lock-hold is-pressing" } else { "carry-lock-hold" },
                        style: "touch-action: pan-y;",
                        r#type: "button",
                        aria_label: "1.2秒長押しで1操作許可",
                        onpointerdown: move |event: PointerEvent| {
                            if accepts_long_press_pointer(
                                &event.pointer_type(),
                                event.is_primary(),
                                event.trigger_button(),
                            ) {
                                begin_long_press(
                                    tracker,
                                    pressing,
                                    LongPressSource::Pointer,
                                    on_arm,
                                );
                            }
                        },
                        onpointerup: move |_| cancel_long_press(tracker, pressing, LongPressSource::Pointer),
                        onpointerleave: move |_| cancel_long_press(tracker, pressing, LongPressSource::Pointer),
                        onpointercancel: move |_| cancel_long_press(tracker, pressing, LongPressSource::Pointer),
                        onkeydown: move |event: KeyboardEvent| {
                            let key = event.key().to_string();
                            let token = {
                                tracker
                                    .write()
                                    .begin_keyboard(&key, event.is_auto_repeating())
                            };
                            if let Some(token) = token {
                                event.prevent_default();
                                pressing.set(true);
                                complete_long_press_after_delay(tracker, pressing, token, on_arm);
                            }
                        },
                        onkeyup: move |_| cancel_long_press(tracker, pressing, LongPressSource::Keyboard),
                        onblur: move |_| {
                            cancel_long_press(tracker, pressing, LongPressSource::Pointer);
                            cancel_long_press(tracker, pressing, LongPressSource::Keyboard);
                        },
                        "長押しして操作を許可"
                    }
                    DisableCarryLockDetails { on_disable }
                },
                CarryLockMode::ArmedUntil(_) => rsx! {
                    div { class: "carry-lock-status",
                        strong { "1操作可能" }
                        span { "残り{model.remaining_seconds}秒" }
                    }
                    DisableCarryLockDetails { on_disable }
                },
            }
        }
    }
}

#[component]
fn DisableCarryLockDetails(on_disable: EventHandler<()>) -> Element {
    rsx! {
        details { class: "carry-lock-details",
            summary { "通常モードへ戻す" }
            div { class: "carry-lock-disable-confirmation",
                p { "持ち歩きロックを解除しますか?" }
                button {
                    r#type: "button",
                    onclick: move |_| on_disable.call(()),
                    "確認して解除"
                }
            }
        }
    }
}

pub(super) fn accepts_long_press_pointer(
    pointer_type: &str,
    is_primary: bool,
    trigger_button: Option<MouseButton>,
) -> bool {
    if !is_primary {
        return false;
    }
    match pointer_type {
        "mouse" => trigger_button == Some(MouseButton::Primary),
        "touch" | "pen" => true,
        _ => false,
    }
}

fn begin_long_press(
    mut tracker: Signal<LongPressTracker>,
    mut pressing: Signal<bool>,
    source: LongPressSource,
    on_arm: EventHandler<()>,
) {
    let token = tracker.write().begin(source);
    pressing.set(true);
    complete_long_press_after_delay(tracker, pressing, token, on_arm);
}

fn cancel_long_press(
    mut tracker: Signal<LongPressTracker>,
    mut pressing: Signal<bool>,
    source: LongPressSource,
) {
    tracker.write().cancel(source);
    pressing.set(false);
}

trait ScrollEventSource {
    type Subscription;

    fn subscribe_capture(self, on_scroll: Box<dyn FnMut()>) -> Self::Subscription;
}

struct ScrollCancellationGuard<Subscription> {
    _subscription: Subscription,
}

impl<Subscription> ScrollCancellationGuard<Subscription> {
    fn attach<Source>(source: Source, on_scroll: impl FnMut() + 'static) -> Self
    where
        Source: ScrollEventSource<Subscription = Subscription>,
    {
        Self {
            _subscription: source.subscribe_capture(Box::new(on_scroll)),
        }
    }
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
struct BrowserWindowScrollSource;

#[cfg(all(feature = "web", target_arch = "wasm32"))]
const CAPTURE_SCROLL_EVENTS: bool = true;

#[cfg(all(feature = "web", target_arch = "wasm32"))]
struct BrowserWindowScrollSubscription {
    window: web_sys::Window,
    callback: Closure<dyn FnMut(web_sys::Event)>,
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
impl ScrollEventSource for BrowserWindowScrollSource {
    type Subscription = BrowserWindowScrollSubscription;

    fn subscribe_capture(self, mut on_scroll: Box<dyn FnMut()>) -> Self::Subscription {
        let window = web_sys::window().expect("browser Window API must be available");
        let callback = Closure::wrap(Box::new(move |_event: web_sys::Event| {
            on_scroll();
        }) as Box<dyn FnMut(web_sys::Event)>);
        window
            .add_event_listener_with_callback_and_bool(
                "scroll",
                callback.as_ref().unchecked_ref(),
                CAPTURE_SCROLL_EVENTS,
            )
            .expect("window scroll listener must be registered");
        BrowserWindowScrollSubscription { window, callback }
    }
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
impl Drop for BrowserWindowScrollSubscription {
    fn drop(&mut self) {
        let _ = self.window.remove_event_listener_with_callback_and_bool(
            "scroll",
            self.callback.as_ref().unchecked_ref(),
            CAPTURE_SCROLL_EVENTS,
        );
    }
}

#[cfg(all(feature = "web", target_arch = "wasm32"))]
fn complete_long_press_after_delay(
    mut tracker: Signal<LongPressTracker>,
    mut pressing: Signal<bool>,
    token: u64,
    on_arm: EventHandler<()>,
) {
    spawn(async move {
        gloo_timers::future::TimeoutFuture::new(LONG_PRESS_MILLIS).await;
        if tracker.write().complete(token) {
            pressing.set(false);
            on_arm.call(());
        }
    });
}

#[cfg(not(all(feature = "web", target_arch = "wasm32")))]
fn complete_long_press_after_delay(
    _tracker: Signal<LongPressTracker>,
    _pressing: Signal<bool>,
    _token: u64,
    _on_arm: EventHandler<()>,
) {
}

#[cfg(test)]
mod scroll_listener_tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    type SharedCallback = Rc<RefCell<Option<Box<dyn FnMut()>>>>;

    #[derive(Clone, Default)]
    struct FakeScrollSource {
        callback: SharedCallback,
        subscribe_count: Rc<Cell<usize>>,
        unsubscribe_count: Rc<Cell<usize>>,
    }

    impl FakeScrollSource {
        fn fire(&self) {
            if let Some(callback) = self.callback.borrow_mut().as_mut() {
                callback();
            }
        }
    }

    struct FakeScrollSubscription {
        callback: SharedCallback,
        unsubscribe_count: Rc<Cell<usize>>,
    }

    impl Drop for FakeScrollSubscription {
        fn drop(&mut self) {
            self.callback.borrow_mut().take();
            self.unsubscribe_count
                .set(self.unsubscribe_count.get().saturating_add(1));
        }
    }

    impl ScrollEventSource for FakeScrollSource {
        type Subscription = FakeScrollSubscription;

        fn subscribe_capture(self, on_scroll: Box<dyn FnMut()>) -> Self::Subscription {
            self.subscribe_count
                .set(self.subscribe_count.get().saturating_add(1));
            self.callback.borrow_mut().replace(on_scroll);
            FakeScrollSubscription {
                callback: self.callback,
                unsubscribe_count: self.unsubscribe_count,
            }
        }
    }

    #[test]
    fn guardはscrollを購読しcallbackで長押しをcancelしてdrop時に解除する() {
        let source = FakeScrollSource::default();
        let tracker = Rc::new(RefCell::new(LongPressTracker::default()));
        let stale_timer = tracker.borrow_mut().begin(LongPressSource::Pointer);
        let cancellation_count = Rc::new(Cell::new(0_u8));
        let callback_tracker = Rc::clone(&tracker);
        let callback_count = Rc::clone(&cancellation_count);

        let guard = ScrollCancellationGuard::attach(source.clone(), move || {
            if callback_tracker.borrow_mut().cancel_all() {
                callback_count.set(callback_count.get().saturating_add(1));
            }
        });
        assert_eq!(source.subscribe_count.get(), 1);

        source.fire();
        assert_eq!(cancellation_count.get(), 1);
        assert!(!tracker.borrow_mut().complete(stale_timer));

        drop(guard);
        assert_eq!(source.unsubscribe_count.get(), 1);
        source.fire();
        assert_eq!(cancellation_count.get(), 1);
    }
}
