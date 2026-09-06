use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::long_press_controller::{
    LongPressController, LongPressScheduler, LongPressSource, LONG_PRESS_MILLIS,
};

type ScheduledCallback = Box<dyn FnOnce()>;

#[derive(Clone, Default)]
struct FakeScheduler {
    scheduled: Rc<RefCell<Vec<(u32, ScheduledCallback)>>>,
}

impl FakeScheduler {
    fn run_next(&self) -> bool {
        let task = self.scheduled.borrow_mut().pop();
        if let Some((_, callback)) = task {
            callback();
            true
        } else {
            false
        }
    }
}

impl LongPressScheduler for FakeScheduler {
    fn schedule(&self, delay_millis: u32, callback: ScheduledCallback) {
        self.scheduled.borrow_mut().push((delay_millis, callback));
    }
}

fn controller(
    scheduler: FakeScheduler,
    holding: Rc<RefCell<Vec<bool>>>,
    arm_count: Rc<Cell<u8>>,
) -> LongPressController<FakeScheduler> {
    LongPressController::new(
        scheduler,
        move |active| holding.borrow_mut().push(active),
        move || arm_count.set(arm_count.get().saturating_add(1)),
    )
}

#[test]
fn componentが使うcontrollerは1200ms後に一度だけarmする() {
    let scheduler = FakeScheduler::default();
    let holding = Rc::new(RefCell::new(Vec::new()));
    let arm_count = Rc::new(Cell::new(0));
    let controller = controller(
        scheduler.clone(),
        Rc::clone(&holding),
        Rc::clone(&arm_count),
    );

    controller.start(LongPressSource::Pointer);
    assert_eq!(holding.borrow().as_slice(), [true]);
    assert_eq!(scheduler.scheduled.borrow()[0].0, LONG_PRESS_MILLIS);
    assert_eq!(arm_count.get(), 0);

    assert!(scheduler.run_next());
    assert_eq!(holding.borrow().as_slice(), [true, false]);
    assert_eq!(arm_count.get(), 1);
    assert!(!scheduler.run_next());
    assert_eq!(arm_count.get(), 1);
}

#[test]
fn pointer_cancelとscroll_cancelはstale_timerのarmを拒否する() {
    for cancel in [
        LongPressController::cancel_pointer,
        LongPressController::cancel_for_scroll,
    ] {
        let scheduler = FakeScheduler::default();
        let holding = Rc::new(RefCell::new(Vec::new()));
        let arm_count = Rc::new(Cell::new(0));
        let controller = controller(
            scheduler.clone(),
            Rc::clone(&holding),
            Rc::clone(&arm_count),
        );

        controller.start(LongPressSource::Pointer);
        cancel(&controller);
        assert_eq!(holding.borrow().as_slice(), [true, false]);
        assert!(scheduler.run_next());
        assert_eq!(arm_count.get(), 0);
    }
}
