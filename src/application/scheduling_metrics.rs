#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct FlattenMetrics {
    pub overload_iteration_count: usize,
    pub candidate_trial_count: usize,
    pub override_clone_element_count: usize,
    pub full_schedule_scan_element_count: usize,
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
