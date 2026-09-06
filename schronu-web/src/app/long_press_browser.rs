use super::long_press_controller::LongPressScheduler;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BrowserLongPressScheduler;

impl LongPressScheduler for BrowserLongPressScheduler {
    fn schedule(&self, delay_millis: u32, callback: Box<dyn FnOnce()>) {
        dioxus::prelude::spawn(async move {
            gloo_timers::future::TimeoutFuture::new(delay_millis).await;
            callback();
        });
    }
}
