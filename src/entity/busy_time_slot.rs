pub(crate) struct BusyTimeSlot {
    start_time_hour: u32,
    start_time_minute: u32,
    duration_minutes: i64,
}

impl BusyTimeSlot {
    pub(crate) fn new(start_time_hour: u32, start_time_minute: u32, duration_minutes: i64) -> Self {
        Self {
            start_time_hour,
            start_time_minute,
            duration_minutes,
        }
    }

    pub(crate) fn get_start_time_hour(&self) -> u32 {
        self.start_time_hour
    }

    pub(crate) fn get_start_time_minute(&self) -> u32 {
        self.start_time_minute
    }

    pub(crate) fn get_duration_minutes(&self) -> i64 {
        self.duration_minutes
    }
}
