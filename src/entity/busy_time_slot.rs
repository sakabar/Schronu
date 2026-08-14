pub struct BusyTimeSlot {
    start_time_hour: u32,
    start_time_minute: u32,
    duration_minutes: i64,
    _name: String,
}

impl BusyTimeSlot {
    pub fn new(
        start_time_hour: u32,
        start_time_minute: u32,
        duration_minutes: i64,
        name: String,
    ) -> Self {
        Self {
            start_time_hour,
            start_time_minute,
            duration_minutes,
            _name: name,
        }
    }

    pub fn get_start_time_hour(&self) -> u32 {
        self.start_time_hour
    }

    pub fn get_start_time_minute(&self) -> u32 {
        self.start_time_minute
    }

    pub fn get_duration_minutes(&self) -> i64 {
        self.duration_minutes
    }
}
