#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ScheduleMetrics {
    pub candidate_count: usize,
    pub segment_count: usize,
    pub occupied_slot_probe_count: usize,
    pub dependency_candidate_probe_count: usize,
    pub selection_event_count: usize,
    pub selection_candidate_probe_count: usize,
    pub release_candidate_probe_count: usize,
    pub frontier_clone_element_count: usize,
    pub slack_probe_count: usize,
    pub sort_count: usize,
    pub schedule_rebuild_count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PackMetrics {
    pub schedule: ScheduleMetrics,
    pub candidate_count: usize,
    pub placement_trial_count: usize,
    pub cursor_minute_advance_count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct FlattenMetrics {
    pub schedule: ScheduleMetrics,
    pub overload_iteration_count: usize,
    pub candidate_trial_count: usize,
    pub override_clone_element_count: usize,
    pub full_schedule_scan_element_count: usize,
}

impl ScheduleMetrics {
    #[inline(always)]
    pub fn record_candidates(&mut self, count: usize) {
        #[cfg(feature = "benchmarking")]
        {
            self.candidate_count += count;
        }
        #[cfg(not(feature = "benchmarking"))]
        let _ = count;
    }

    #[inline(always)]
    pub fn record_segment(&mut self) {
        #[cfg(feature = "benchmarking")]
        {
            self.segment_count += 1;
        }
    }

    #[inline(always)]
    pub fn record_occupied_slot_probe(&mut self) {
        #[cfg(feature = "benchmarking")]
        {
            self.occupied_slot_probe_count += 1;
        }
    }

    #[inline(always)]
    pub fn record_dependency_candidate_probe(&mut self) {
        #[cfg(feature = "benchmarking")]
        {
            self.dependency_candidate_probe_count += 1;
        }
    }

    #[inline(always)]
    pub fn record_selection_event(&mut self) {
        #[cfg(feature = "benchmarking")]
        {
            self.selection_event_count += 1;
        }
    }

    #[inline(always)]
    pub fn record_selection_candidate_probe(&mut self) {
        #[cfg(feature = "benchmarking")]
        {
            self.selection_candidate_probe_count += 1;
        }
    }

    #[inline(always)]
    pub fn record_release_candidate_probe(&mut self) {
        #[cfg(feature = "benchmarking")]
        {
            self.release_candidate_probe_count += 1;
        }
    }

    #[inline(always)]
    pub fn record_frontier_clone_elements(&mut self, count: usize) {
        #[cfg(feature = "benchmarking")]
        {
            self.frontier_clone_element_count += count;
        }
        #[cfg(not(feature = "benchmarking"))]
        let _ = count;
    }

    #[inline(always)]
    pub fn record_slack_probes(&mut self, count: usize) {
        #[cfg(feature = "benchmarking")]
        {
            self.slack_probe_count += count;
        }
        #[cfg(not(feature = "benchmarking"))]
        let _ = count;
    }

    #[inline(always)]
    pub fn record_sort(&mut self) {
        #[cfg(feature = "benchmarking")]
        {
            self.sort_count += 1;
        }
    }

    #[inline(always)]
    pub fn record_rebuild(&mut self) {
        #[cfg(feature = "benchmarking")]
        {
            self.schedule_rebuild_count += 1;
        }
    }
}

impl PackMetrics {
    #[inline(always)]
    pub fn record_candidate_count(&mut self, count: usize) {
        #[cfg(feature = "benchmarking")]
        {
            self.candidate_count += count;
        }
        #[cfg(not(feature = "benchmarking"))]
        let _ = count;
    }

    #[inline(always)]
    pub fn record_placement_trial(&mut self) {
        #[cfg(feature = "benchmarking")]
        {
            self.placement_trial_count += 1;
        }
    }

    #[inline(always)]
    pub fn record_cursor_minute_advance(&mut self, minutes: usize) {
        #[cfg(feature = "benchmarking")]
        {
            self.cursor_minute_advance_count += minutes;
        }
        #[cfg(not(feature = "benchmarking"))]
        let _ = minutes;
    }
}

impl FlattenMetrics {
    #[inline(always)]
    pub fn record_overload_iteration(&mut self) {
        #[cfg(feature = "benchmarking")]
        {
            self.overload_iteration_count += 1;
        }
    }

    #[inline(always)]
    pub fn record_candidate_trial(&mut self) {
        #[cfg(feature = "benchmarking")]
        {
            self.candidate_trial_count += 1;
        }
    }

    #[inline(always)]
    pub fn record_override_clone(&mut self, elements: usize) {
        #[cfg(feature = "benchmarking")]
        {
            self.override_clone_element_count += elements;
        }
        #[cfg(not(feature = "benchmarking"))]
        let _ = elements;
    }

    #[inline(always)]
    pub fn record_full_schedule_scan(&mut self, elements: usize) {
        #[cfg(feature = "benchmarking")]
        {
            self.full_schedule_scan_element_count += elements;
        }
        #[cfg(not(feature = "benchmarking"))]
        let _ = elements;
    }
}
