use crate::client::carry_lock::CarryLockMode;
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
#[cfg(all(feature = "web", target_arch = "wasm32"))]
use std::rc::Rc;
#[cfg(all(feature = "web", target_arch = "wasm32"))]
use wasm_bindgen::{closure::Closure, JsCast};

use super::long_press_controller::{
    LongPressController, LongPressSchedulerHandle, LongPressSource,
};

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

#[component]
pub(crate) fn CarryLockBar(
    model: CarryLockViewModel,
    scheduler: LongPressSchedulerHandle,
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
    let pressing = use_signal(|| false);
    let controller = use_hook(move || {
        LongPressController::new(
            scheduler,
            move |active| {
                let mut pressing = pressing;
                pressing.set(active);
            },
            move || on_arm.call(()),
        )
    });
    #[cfg(all(feature = "web", target_arch = "wasm32"))]
    let _window_scroll_listener = use_hook(|| {
        let controller = controller.clone();
        Rc::new(ScrollCancellationGuard::attach(
            BrowserWindowScrollSource,
            move || {
                controller.cancel_for_scroll();
            },
        ))
    });

    let pointer_down_controller = controller.clone();
    let pointer_up_controller = controller.clone();
    let pointer_leave_controller = controller.clone();
    let pointer_cancel_controller = controller.clone();
    let key_down_controller = controller.clone();
    let key_up_controller = controller.clone();
    let blur_controller = controller;

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
                                pointer_down_controller.start(LongPressSource::Pointer);
                            }
                        },
                        onpointerup: move |_| pointer_up_controller.cancel_pointer(),
                        onpointerleave: move |_| pointer_leave_controller.cancel_pointer(),
                        onpointercancel: move |_| pointer_cancel_controller.cancel_pointer(),
                        onkeydown: move |event: KeyboardEvent| {
                            let key = event.key().to_string();
                            if key_down_controller.start_keyboard(&key, event.is_auto_repeating()) {
                                event.prevent_default();
                            }
                        },
                        onkeyup: move |_| key_up_controller.cancel_keyboard(),
                        onblur: move |_| {
                            blur_controller.cancel_pointer();
                            blur_controller.cancel_keyboard();
                        },
                        strong { "操作ロック中" }
                        span { "1.2秒長押しで1操作許可" }
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
    fn guardはscrollを購読しcallbackを呼びdrop時に解除する() {
        let source = FakeScrollSource::default();
        let cancellation_count = Rc::new(Cell::new(0_u8));
        let callback_count = Rc::clone(&cancellation_count);

        let guard = ScrollCancellationGuard::attach(source.clone(), move || {
            callback_count.set(callback_count.get().saturating_add(1));
        });
        assert_eq!(source.subscribe_count.get(), 1);

        source.fire();
        assert_eq!(cancellation_count.get(), 1);

        drop(guard);
        assert_eq!(source.unsubscribe_count.get(), 1);
        source.fire();
        assert_eq!(cancellation_count.get(), 1);
    }
}
