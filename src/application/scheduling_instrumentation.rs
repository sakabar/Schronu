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
