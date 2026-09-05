use crate::application::interface::{
    BusyTimeSlotLoadError, BusyTimeSlotRegistrationError, FreeTimeManagerTrait,
};
use crate::entity::busy_time_slot::BusyTimeSlot;
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Timelike, Weekday};
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use yaml_rust::{Yaml, YamlLoader};

use chrono::TimeZone;

const MINUTES_PER_DAY: i64 = 24 * 60;

// Scheduleをどう持つか: 日付をキーとする辞書
pub struct FreeTimeManager {
    weekly_busy_time_slots: HashMap<Weekday, Vec<BusyTimeSlot>>,
    registered_busy_time_slots_map: HashMap<NaiveDate, Vec<i64>>,
}

impl Default for FreeTimeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FreeTimeManager {
    pub fn new() -> Self {
        Self {
            weekly_busy_time_slots: HashMap::new(),
            registered_busy_time_slots_map: HashMap::new(),
        }
    }

    fn load_busy_time_slots_from_file(
        &mut self,
        busy_time_slots_file_path: &str,
    ) -> Result<(), BusyTimeSlotLoadError> {
        let path = Path::new(busy_time_slots_file_path);
        let mut file =
            File::open(path).map_err(|error| BusyTimeSlotLoadError::new(path, "$", None, error))?;
        let mut text = String::new();
        file.read_to_string(&mut text)
            .map_err(|error| BusyTimeSlotLoadError::new(path, "$", None, error))?;

        let weekly_busy_time_slots = self.load_busy_time_slots_from_str(path, &text)?;
        self.weekly_busy_time_slots = weekly_busy_time_slots;
        Ok(())
    }

    fn load_busy_time_slots_from_str(
        &self,
        path: &Path,
        yaml_str: &str,
    ) -> Result<HashMap<Weekday, Vec<BusyTimeSlot>>, BusyTimeSlotLoadError> {
        let mut day_of_week_map: HashMap<Weekday, Vec<BusyTimeSlot>> = HashMap::new();
        let docs = YamlLoader::load_from_str(yaml_str)
            .map_err(|e| BusyTimeSlotLoadError::new(path, "$", None, e))?;
        let document = docs
            .first()
            .ok_or_else(|| invalid(path, "$", None, "empty YAML document"))?;
        let days_yaml = &document["days_of_week"];
        let days = days_yaml.as_vec().ok_or_else(|| {
            invalid(
                path,
                "days_of_week",
                yaml_error_value(days_yaml),
                "must be an array",
            )
        })?;
        for (day_index, day) in days.iter().enumerate() {
            let day_path = format!("days_of_week[{day_index}]");
            if day.as_hash().is_none() {
                return Err(invalid(
                    path,
                    &day_path,
                    yaml_error_value(day),
                    "must be a mapping",
                ));
            }
            let day_name_yaml = &day["day_of_week"];
            let day_name = day_name_yaml.as_str().ok_or_else(|| {
                invalid(
                    path,
                    &format!("{day_path}.day_of_week"),
                    yaml_error_value(day_name_yaml),
                    "must be a string",
                )
            })?;
            let day_of_week = match day_name {
                "Mon" => Weekday::Mon,
                "Tue" => Weekday::Tue,
                "Wed" => Weekday::Wed,
                "Thu" => Weekday::Thu,
                "Fri" => Weekday::Fri,
                "Sat" => Weekday::Sat,
                "Sun" => Weekday::Sun,
                _ => {
                    return Err(invalid(
                        path,
                        &format!("{day_path}.day_of_week"),
                        Some(day_name.into()),
                        "unknown weekday",
                    ))
                }
            };
            if day_of_week_map.contains_key(&day_of_week) {
                return Err(invalid(
                    path,
                    &format!("{day_path}.day_of_week"),
                    Some(day_name.into()),
                    "duplicate weekday",
                ));
            }
            let slots_yaml = &day["busy_time_slots"];
            let slots = slots_yaml.as_vec().ok_or_else(|| {
                invalid(
                    path,
                    &format!("{day_path}.busy_time_slots"),
                    yaml_error_value(slots_yaml),
                    "must be an array",
                )
            })?;
            let mut busy_time_slots = vec![];
            for (slot_index, slot) in slots.iter().enumerate() {
                let slot_path = format!("{day_path}.busy_time_slots[{slot_index}]");
                if slot.as_hash().is_none() {
                    return Err(invalid(
                        path,
                        &slot_path,
                        yaml_error_value(slot),
                        "must be a mapping",
                    ));
                }
                let start_yaml = &slot["start_time"];
                let start = start_yaml.as_str().ok_or_else(|| {
                    invalid(
                        path,
                        &format!("{slot_path}.start_time"),
                        yaml_error_value(start_yaml),
                        "must be a string",
                    )
                })?;
                let (hour, minute) = start.split_once(':').ok_or_else(|| {
                    invalid(
                        path,
                        &format!("{slot_path}.start_time"),
                        Some(start.into()),
                        "invalid time",
                    )
                })?;
                let hour: u32 = hour.parse().map_err(|e| {
                    BusyTimeSlotLoadError::new(
                        path,
                        format!("{slot_path}.start_time"),
                        Some(start.into()),
                        e,
                    )
                })?;
                let minute: u32 = minute.parse().map_err(|e| {
                    BusyTimeSlotLoadError::new(
                        path,
                        format!("{slot_path}.start_time"),
                        Some(start.into()),
                        e,
                    )
                })?;
                let duration_yaml = &slot["duration_minutes"];
                let duration = duration_yaml.as_i64().ok_or_else(|| {
                    invalid(
                        path,
                        &format!("{slot_path}.duration_minutes"),
                        yaml_error_value(duration_yaml),
                        "must be an integer",
                    )
                })?;
                let name_yaml = &slot["name"];
                let _ = name_yaml.as_str().ok_or_else(|| {
                    invalid(
                        path,
                        &format!("{slot_path}.name"),
                        yaml_error_value(name_yaml),
                        "must be a string",
                    )
                })?;
                if hour >= 24 || minute >= 60 {
                    return Err(invalid(
                        path,
                        &format!("{slot_path}.start_time"),
                        Some(start.into()),
                        "time out of range",
                    ));
                }
                let end_minutes = i64::from(hour)
                    .checked_mul(60)
                    .and_then(|minutes| minutes.checked_add(i64::from(minute)))
                    .and_then(|minutes| minutes.checked_add(duration));
                if duration < 0 || end_minutes.is_none_or(|minutes| minutes >= MINUTES_PER_DAY) {
                    return Err(invalid(
                        path,
                        &format!("{slot_path}.duration_minutes"),
                        Some(duration.to_string()),
                        "invalid slot range",
                    ));
                }
                busy_time_slots.push(BusyTimeSlot::new(hour, minute, duration));
            }
            day_of_week_map.insert(day_of_week, busy_time_slots);
        }
        if day_of_week_map.len() != 7 {
            return Err(invalid(
                path,
                "days_of_week",
                None,
                "all weekdays are required",
            ));
        }
        Ok(day_of_week_map)
    }

    fn get_free_time_slot(&self, date: NaiveDate) -> Vec<i64> {
        let mut free_time_slot = vec![1; MINUTES_PER_DAY as usize];

        if let Some(busy_time_slots) = self.weekly_busy_time_slots.get(&date.weekday()) {
            for busy_time_slot in busy_time_slots {
                mark_busy_time_slot(&mut free_time_slot, busy_time_slot);
            }
        }

        if let Some(registered_free_time_slot) = self.registered_busy_time_slots_map.get(&date) {
            for (index, free) in registered_free_time_slot.iter().enumerate() {
                if *free == 0 {
                    free_time_slot[index] = 0;
                }
            }
        }

        free_time_slot
    }
}

fn invalid(
    path: &Path,
    field_path: &str,
    value: Option<String>,
    message: &str,
) -> BusyTimeSlotLoadError {
    BusyTimeSlotLoadError::new(
        path,
        field_path,
        value,
        std::io::Error::new(std::io::ErrorKind::InvalidData, message),
    )
}

fn yaml_error_value(value: &Yaml) -> Option<String> {
    (!matches!(value, Yaml::BadValue)).then(|| format!("{value:?}"))
}

fn mark_busy_time_slot(free_time_slot: &mut [i64], busy_time_slot: &BusyTimeSlot) {
    let start_index =
        (busy_time_slot.get_start_time_hour() * 60 + busy_time_slot.get_start_time_minute()) as i64;
    let end_index = (start_index + busy_time_slot.get_duration_minutes()).clamp(0, MINUTES_PER_DAY);

    for index in start_index.clamp(0, MINUTES_PER_DAY)..end_index {
        free_time_slot[index as usize] = 0;
    }
}

impl FreeTimeManagerTrait for FreeTimeManager {
    fn get_free_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
        const CAP_RATE: f64 = 1.0;

        if start >= end {
            return 0;
        }

        let mut current = *start;
        let mut free_minutes = 0;
        while current < *end {
            let next_date = current.date_naive().succ_opt().expect("date overflow");
            let next_midnight = Local
                .with_ymd_and_hms(
                    next_date.year(),
                    next_date.month(),
                    next_date.day(),
                    0,
                    0,
                    0,
                )
                .single()
                .expect("invalid local midnight");
            let segment_end = (*end).min(next_midnight);
            let free_time_slot = self.get_free_time_slot(current.date_naive());

            let start_index = current.hour() * 60 + current.minute();
            let end_index = if current.date_naive() == segment_end.date_naive() {
                segment_end.hour() * 60 + segment_end.minute()
            } else {
                MINUTES_PER_DAY as u32
            };

            for index in start_index..end_index {
                free_minutes += free_time_slot[index as usize];
            }
            current = segment_end;
        }

        (free_minutes as f64 * CAP_RATE) as i64
    }

    fn get_busy_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
        let free_minutes = self.get_free_minutes(start, end);

        (*end - *start).num_minutes() - free_minutes
    }

    fn get_free_seconds(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
        if start >= end {
            return 0;
        }

        let mut current = *start;
        let mut free_duration = Duration::zero();
        while current < *end {
            let minute_start = current
                .with_second(0)
                .and_then(|datetime| datetime.with_nanosecond(0))
                .unwrap_or(current);
            let next_minute = minute_start
                .checked_add_signed(Duration::minutes(1))
                .unwrap_or(*end);
            let segment_end = (*end).min(next_minute);
            let free_time_slot = self.get_free_time_slot(current.date_naive());
            let minute_index = (current.hour() * 60 + current.minute()) as usize;

            if free_time_slot[minute_index] != 0 {
                let segment_duration = segment_end.signed_duration_since(current);
                free_duration = free_duration
                    .checked_add(&segment_duration)
                    .unwrap_or_else(|| end.signed_duration_since(*start));
            }
            current = segment_end;
        }

        free_duration.num_seconds()
    }

    // 同日内の半開区間[start, end)だけを登録する。
    fn register_busy_time_slot(
        &mut self,
        start: &DateTime<Local>,
        end: &DateTime<Local>,
    ) -> Result<(), BusyTimeSlotRegistrationError> {
        if start.date_naive() != end.date_naive() {
            return Err(BusyTimeSlotRegistrationError);
        }

        let date = start.date_naive();
        let free_time_slot = self
            .registered_busy_time_slots_map
            .entry(date)
            .or_insert(vec![1; MINUTES_PER_DAY as usize]);

        let start_index = start.hour() * 60 + start.minute();
        let end_index = end.hour() * 60 + end.minute();

        for ind in start_index..end_index {
            free_time_slot[ind as usize] = 0;
        }
        Ok(())
    }

    fn load_busy_time_slots_from_file(
        &mut self,
        busy_time_slots_file_path: &str,
    ) -> Result<(), BusyTimeSlotLoadError> {
        FreeTimeManager::load_busy_time_slots_from_file(self, busy_time_slots_file_path)
    }
}

#[cfg(test)]
include!("free_time_manager_tests.rs");
