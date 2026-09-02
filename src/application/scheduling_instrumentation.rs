#![allow(dead_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScheduleEvent {
    Candidates(usize),
    Segment,
    OccupiedSlotProbe,
    DependencyCandidateProbe,
    Selection,
    SelectionCandidateProbe,
    ReleaseCandidateProbe,
    AtomicReleaseCacheProbe,
    AtomicReleaseCacheEntries(usize),
    SlackProbes(usize),
    Sort,
    Rebuild,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackEvent {
    Candidates(usize),
    PlacementTrial,
    CursorMinuteAdvance(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FlattenEvent {
    OverloadIteration,
    CandidateTrial,
    OverrideClone(usize),
    FullScheduleScan(usize),
}

#[inline(always)]
pub(crate) fn record_schedule(event: ScheduleEvent) {
    #[cfg(feature = "benchmarking")]
    collector::record_schedule(event);
    #[cfg(not(feature = "benchmarking"))]
    let _ = event;
}

#[inline(always)]
pub(crate) fn record_pack(event: PackEvent) {
    #[cfg(feature = "benchmarking")]
    collector::record_pack(event);
    #[cfg(not(feature = "benchmarking"))]
    let _ = event;
}

#[inline(always)]
pub(crate) fn record_flatten(event: FlattenEvent) {
    #[cfg(feature = "benchmarking")]
    collector::record_flatten(event);
    #[cfg(not(feature = "benchmarking"))]
    let _ = event;
}

#[cfg(feature = "benchmarking")]
pub use collector::{FlattenMetrics, PackMetrics, ScheduleMetrics};

#[cfg(feature = "benchmarking")]
pub(crate) fn capture_schedule_metrics<T>(operation: impl FnOnce() -> T) -> (T, ScheduleMetrics) {
    collector::capture_schedule_metrics(operation)
}

#[cfg(feature = "benchmarking")]
pub(crate) fn capture_pack_metrics<T>(operation: impl FnOnce() -> T) -> (T, PackMetrics) {
    collector::capture_pack_metrics(operation)
}

#[cfg(feature = "benchmarking")]
pub(crate) fn capture_flatten_metrics<T>(operation: impl FnOnce() -> T) -> (T, FlattenMetrics) {
    collector::capture_flatten_metrics(operation)
}

#[cfg(feature = "benchmarking")]
mod collector {
    use super::{FlattenEvent, PackEvent, ScheduleEvent};
    use std::cell::RefCell;

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub struct ScheduleMetrics {
        pub candidate_count: usize,
        pub segment_count: usize,
        pub occupied_slot_probe_count: usize,
        pub dependency_candidate_probe_count: usize,
        pub selection_event_count: usize,
        pub selection_candidate_probe_count: usize,
        pub release_candidate_probe_count: usize,
        pub atomic_release_cache_probe_count: usize,
        pub atomic_release_cache_peak_entry_count: usize,
        pub slack_probe_count: usize,
        pub sort_count: usize,
        pub schedule_rebuild_count: usize,
    }

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub struct PackMetrics {
        pub schedule: ScheduleMetrics,
        pub candidate_count: usize,
        pub placement_trial_count: usize,
        pub cursor_minute_advance_count: usize,
    }

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub struct FlattenMetrics {
        pub schedule: ScheduleMetrics,
        pub overload_iteration_count: usize,
        pub candidate_trial_count: usize,
        pub override_clone_element_count: usize,
        pub full_schedule_scan_element_count: usize,
    }

    enum ActiveMetrics {
        Schedule(ScheduleMetrics),
        Pack(PackMetrics),
        Flatten(FlattenMetrics),
    }

    impl ActiveMetrics {
        fn schedule_mut(&mut self) -> &mut ScheduleMetrics {
            match self {
                Self::Schedule(metrics) => metrics,
                Self::Pack(metrics) => &mut metrics.schedule,
                Self::Flatten(metrics) => &mut metrics.schedule,
            }
        }
    }

    thread_local! {
        static ACTIVE_METRICS: RefCell<Vec<ActiveMetrics>> = const { RefCell::new(Vec::new()) };
    }

    struct SessionGuard {
        active: bool,
    }

    impl SessionGuard {
        fn push(metrics: ActiveMetrics) -> Self {
            ACTIVE_METRICS.with(|active| active.borrow_mut().push(metrics));
            Self { active: true }
        }

        fn finish(mut self) -> ActiveMetrics {
            let metrics = ACTIVE_METRICS.with(|active| {
                active
                    .borrow_mut()
                    .pop()
                    .expect("a metrics session must be active")
            });
            self.active = false;
            metrics
        }
    }

    impl Drop for SessionGuard {
        fn drop(&mut self) {
            if self.active {
                ACTIVE_METRICS.with(|active| {
                    active.borrow_mut().pop();
                });
            }
        }
    }

    pub(super) fn capture_schedule_metrics<T>(
        operation: impl FnOnce() -> T,
    ) -> (T, ScheduleMetrics) {
        let guard = SessionGuard::push(ActiveMetrics::Schedule(ScheduleMetrics::default()));
        let result = operation();
        let ActiveMetrics::Schedule(metrics) = guard.finish() else {
            unreachable!("schedule session must return schedule metrics");
        };
        (result, metrics)
    }

    pub(super) fn capture_pack_metrics<T>(operation: impl FnOnce() -> T) -> (T, PackMetrics) {
        let guard = SessionGuard::push(ActiveMetrics::Pack(PackMetrics::default()));
        let result = operation();
        let ActiveMetrics::Pack(metrics) = guard.finish() else {
            unreachable!("pack session must return pack metrics");
        };
        (result, metrics)
    }

    pub(super) fn capture_flatten_metrics<T>(operation: impl FnOnce() -> T) -> (T, FlattenMetrics) {
        let guard = SessionGuard::push(ActiveMetrics::Flatten(FlattenMetrics::default()));
        let result = operation();
        let ActiveMetrics::Flatten(metrics) = guard.finish() else {
            unreachable!("flatten session must return flatten metrics");
        };
        (result, metrics)
    }

    pub(super) fn record_schedule(event: ScheduleEvent) {
        with_active_metrics(|active| {
            let metrics = active.schedule_mut();
            match event {
                ScheduleEvent::Candidates(count) => metrics.candidate_count += count,
                ScheduleEvent::Segment => metrics.segment_count += 1,
                ScheduleEvent::OccupiedSlotProbe => metrics.occupied_slot_probe_count += 1,
                ScheduleEvent::DependencyCandidateProbe => {
                    metrics.dependency_candidate_probe_count += 1;
                }
                ScheduleEvent::Selection => metrics.selection_event_count += 1,
                ScheduleEvent::SelectionCandidateProbe => {
                    metrics.selection_candidate_probe_count += 1;
                }
                ScheduleEvent::ReleaseCandidateProbe => {
                    metrics.release_candidate_probe_count += 1;
                }
                ScheduleEvent::AtomicReleaseCacheProbe => {
                    metrics.atomic_release_cache_probe_count += 1;
                }
                ScheduleEvent::AtomicReleaseCacheEntries(count) => {
                    metrics.atomic_release_cache_peak_entry_count =
                        metrics.atomic_release_cache_peak_entry_count.max(count);
                }
                ScheduleEvent::SlackProbes(count) => metrics.slack_probe_count += count,
                ScheduleEvent::Sort => metrics.sort_count += 1,
                ScheduleEvent::Rebuild => metrics.schedule_rebuild_count += 1,
            }
        });
    }

    pub(super) fn record_pack(event: PackEvent) {
        with_active_metrics(|active| {
            let ActiveMetrics::Pack(metrics) = active else {
                return;
            };
            match event {
                PackEvent::Candidates(count) => metrics.candidate_count += count,
                PackEvent::PlacementTrial => metrics.placement_trial_count += 1,
                PackEvent::CursorMinuteAdvance(minutes) => {
                    metrics.cursor_minute_advance_count += minutes;
                }
            }
        });
    }

    pub(super) fn record_flatten(event: FlattenEvent) {
        with_active_metrics(|active| {
            let ActiveMetrics::Flatten(metrics) = active else {
                return;
            };
            match event {
                FlattenEvent::OverloadIteration => metrics.overload_iteration_count += 1,
                FlattenEvent::CandidateTrial => metrics.candidate_trial_count += 1,
                FlattenEvent::OverrideClone(elements) => {
                    metrics.override_clone_element_count += elements;
                }
                FlattenEvent::FullScheduleScan(elements) => {
                    metrics.full_schedule_scan_element_count += elements;
                }
            }
        });
    }

    fn with_active_metrics(operation: impl FnOnce(&mut ActiveMetrics)) {
        ACTIVE_METRICS.with(|active| {
            let mut active = active.borrow_mut();
            if let Some(metrics) = active.last_mut() {
                operation(metrics);
            }
        });
    }
}

#[cfg(all(test, feature = "benchmarking"))]
mod tests {
    use super::{
        capture_flatten_metrics, capture_pack_metrics, capture_schedule_metrics, record_flatten,
        record_pack, record_schedule, FlattenEvent, PackEvent, ScheduleEvent,
    };

    #[test]
    fn schedule_sessionはschedule_eventを集約する() {
        let (value, metrics) = capture_schedule_metrics(|| {
            record_schedule(ScheduleEvent::Candidates(3));
            record_schedule(ScheduleEvent::Segment);
            record_schedule(ScheduleEvent::AtomicReleaseCacheEntries(2));
            record_schedule(ScheduleEvent::AtomicReleaseCacheEntries(5));
            record_schedule(ScheduleEvent::SlackProbes(7));
            42
        });

        assert_eq!(value, 42);
        assert_eq!(metrics.candidate_count, 3);
        assert_eq!(metrics.segment_count, 1);
        assert_eq!(metrics.atomic_release_cache_peak_entry_count, 5);
        assert_eq!(metrics.slack_probe_count, 7);
    }

    #[test]
    fn pack_sessionはpack_eventとschedule_eventを集約する() {
        let (_, metrics) = capture_pack_metrics(|| {
            record_schedule(ScheduleEvent::Rebuild);
            record_pack(PackEvent::Candidates(4));
            record_pack(PackEvent::PlacementTrial);
            record_pack(PackEvent::CursorMinuteAdvance(6));
        });

        assert_eq!(metrics.schedule.schedule_rebuild_count, 1);
        assert_eq!(metrics.candidate_count, 4);
        assert_eq!(metrics.placement_trial_count, 1);
        assert_eq!(metrics.cursor_minute_advance_count, 6);
    }

    #[test]
    fn flatten_sessionはflatten_eventとschedule_eventを集約する() {
        let (_, metrics) = capture_flatten_metrics(|| {
            record_schedule(ScheduleEvent::Sort);
            record_flatten(FlattenEvent::OverloadIteration);
            record_flatten(FlattenEvent::CandidateTrial);
            record_flatten(FlattenEvent::OverrideClone(8));
            record_flatten(FlattenEvent::FullScheduleScan(9));
        });

        assert_eq!(metrics.schedule.sort_count, 1);
        assert_eq!(metrics.overload_iteration_count, 1);
        assert_eq!(metrics.candidate_trial_count, 1);
        assert_eq!(metrics.override_clone_element_count, 8);
        assert_eq!(metrics.full_schedule_scan_element_count, 9);
    }
}
