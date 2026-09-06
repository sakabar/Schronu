use std::cell::RefCell;
use std::rc::Rc;

pub(crate) const LONG_PRESS_MILLIS: u32 = 1_200;

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
    pub(crate) fn begin(&mut self, source: LongPressSource) -> u64 {
        self.next_token = self.next_token.wrapping_add(1);
        let token = self.next_token;
        self.active = Some(ActiveLongPress { token, source });
        token
    }

    pub(crate) fn begin_keyboard(&mut self, key: &str, auto_repeating: bool) -> Option<u64> {
        if auto_repeating || !matches!(key, " " | "Enter") || self.active.is_some() {
            return None;
        }
        Some(self.begin(LongPressSource::Keyboard))
    }

    pub(crate) fn cancel(&mut self, source: LongPressSource) -> bool {
        if self.active.is_some_and(|active| active.source == source) {
            self.active = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn cancel_all(&mut self) -> bool {
        self.active.take().is_some()
    }

    pub(crate) fn complete(&mut self, token: u64) -> bool {
        if self.active.is_some_and(|active| active.token == token) {
            self.active = None;
            true
        } else {
            false
        }
    }
}

pub(crate) trait LongPressScheduler: 'static {
    fn schedule(&self, delay_millis: u32, callback: Box<dyn FnOnce()>);
}

#[derive(Clone)]
pub(crate) struct LongPressSchedulerHandle(Rc<dyn LongPressScheduler>);

impl LongPressSchedulerHandle {
    pub(crate) fn new(scheduler: impl LongPressScheduler) -> Self {
        Self(Rc::new(scheduler))
    }
}

impl PartialEq for LongPressSchedulerHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl LongPressScheduler for LongPressSchedulerHandle {
    fn schedule(&self, delay_millis: u32, callback: Box<dyn FnOnce()>) {
        self.0.schedule(delay_millis, callback);
    }
}

pub(crate) struct LongPressController<S> {
    scheduler: S,
    tracker: Rc<RefCell<LongPressTracker>>,
    on_holding: Rc<dyn Fn(bool)>,
    on_arm: Rc<dyn Fn()>,
}

impl<S: Clone> Clone for LongPressController<S> {
    fn clone(&self) -> Self {
        Self {
            scheduler: self.scheduler.clone(),
            tracker: Rc::clone(&self.tracker),
            on_holding: Rc::clone(&self.on_holding),
            on_arm: Rc::clone(&self.on_arm),
        }
    }
}

impl<S: LongPressScheduler + Clone> LongPressController<S> {
    pub(crate) fn new(
        scheduler: S,
        on_holding: impl Fn(bool) + 'static,
        on_arm: impl Fn() + 'static,
    ) -> Self {
        Self {
            scheduler,
            tracker: Rc::new(RefCell::new(LongPressTracker::default())),
            on_holding: Rc::new(on_holding),
            on_arm: Rc::new(on_arm),
        }
    }

    pub(crate) fn start(&self, source: LongPressSource) {
        let token = self.tracker.borrow_mut().begin(source);
        (self.on_holding)(true);
        self.schedule_completion(token);
    }

    pub(crate) fn start_keyboard(&self, key: &str, auto_repeating: bool) -> bool {
        let token = self
            .tracker
            .borrow_mut()
            .begin_keyboard(key, auto_repeating);
        let Some(token) = token else {
            return false;
        };
        (self.on_holding)(true);
        self.schedule_completion(token);
        true
    }

    fn schedule_completion(&self, token: u64) {
        let tracker = Rc::clone(&self.tracker);
        let on_holding = Rc::clone(&self.on_holding);
        let on_arm = Rc::clone(&self.on_arm);
        self.scheduler.schedule(
            LONG_PRESS_MILLIS,
            Box::new(move || {
                if tracker.borrow_mut().complete(token) {
                    on_holding(false);
                    on_arm();
                }
            }),
        );
    }

    pub(crate) fn cancel_pointer(&self) {
        self.cancel(LongPressSource::Pointer);
    }

    pub(crate) fn cancel_keyboard(&self) {
        self.cancel(LongPressSource::Keyboard);
    }

    pub(crate) fn cancel_for_scroll(&self) {
        if self.tracker.borrow_mut().cancel_all() {
            (self.on_holding)(false);
        }
    }

    fn cancel(&self, source: LongPressSource) {
        if self.tracker.borrow_mut().cancel(source) {
            (self.on_holding)(false);
        }
    }
}
