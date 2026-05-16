use chrono::Weekday;

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

pub struct DayOfWeekBusyTimeSlots {
    _day_of_week: Weekday,
    _end_of_day_hour: i64,
    _end_of_day_minute: i64,
    busy_time_slots: Vec<BusyTimeSlot>,
}

impl DayOfWeekBusyTimeSlots {
    pub fn new(
        day_of_week: Weekday,
        end_of_day_hour: i64,
        end_of_day_minute: i64,
        busy_time_slots: Vec<BusyTimeSlot>,
    ) -> Self {
        Self {
            _day_of_week: day_of_week,
            _end_of_day_hour: end_of_day_hour,
            _end_of_day_minute: end_of_day_minute,
            busy_time_slots,
        }
    }

    pub fn get_busy_time_slots(&self) -> &Vec<BusyTimeSlot> {
        &self.busy_time_slots
    }
}
