use crate::application::interface::FreeTimeManagerTrait;
use crate::entity::busy_time_slot::BusyTimeSlot;
use chrono::{DateTime, Datelike, Local, NaiveDate, Timelike, Weekday};
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use yaml_rust::{Yaml, YamlLoader};

use chrono::TimeZone;
#[cfg(test)]
use std::fmt::Write as _;
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::PathBuf;

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

    fn load_busy_time_slots_from_file(&mut self, busy_time_slots_file_path: &str) {
        let mut file = File::open(busy_time_slots_file_path).unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();

        self.weekly_busy_time_slots = self.load_busy_time_slots_from_str(&text);
    }

    fn load_busy_time_slots_from_str(&self, yaml_str: &str) -> HashMap<Weekday, Vec<BusyTimeSlot>> {
        let mut day_of_week_map: HashMap<Weekday, Vec<BusyTimeSlot>> = HashMap::new();

        match YamlLoader::load_from_str(yaml_str) {
            Err(_) => {
                panic!("Error occured in {:?}", yaml_str);
            }
            Ok(docs) => {
                let days_of_week_yaml: &Yaml = &docs[0]["days_of_week"];

                for day_of_week_yaml in days_of_week_yaml.as_vec().unwrap_or(&vec![]).iter() {
                    // Todo: parse()する
                    // https://docs.rs/chrono/latest/chrono/enum.Weekday.html
                    let day_of_week = match day_of_week_yaml["day_of_week"].as_str().unwrap_or("") {
                        "Mon" => Weekday::Mon,
                        "Tue" => Weekday::Tue,
                        "Wed" => Weekday::Wed,
                        "Thu" => Weekday::Thu,
                        "Fri" => Weekday::Fri,
                        "Sat" => Weekday::Sat,
                        "Sun" => Weekday::Sun,
                        s => panic!("Unknown day_of_week: {}", s),
                    };

                    let busy_time_slots_yaml =
                        day_of_week_yaml["busy_time_slots"].as_vec().unwrap();

                    let mut busy_time_slots: Vec<BusyTimeSlot> = vec![];

                    for busy_time_slot_yaml in busy_time_slots_yaml.iter() {
                        let start_time_str = busy_time_slot_yaml["start_time"]
                            .as_str()
                            .unwrap()
                            .to_string();

                        let cols: Vec<&str> = start_time_str.split(':').collect();
                        if cols.len() != 2 {
                            panic!("{:?}", cols);
                        }

                        let start_time_hour: u32 =
                            cols[0].to_string().parse().expect("invalid hour");
                        let start_time_minute: u32 =
                            cols[1].to_string().parse().expect("invalid minute");

                        let duration_minutes =
                            busy_time_slot_yaml["duration_minutes"].as_i64().unwrap();
                        let name = busy_time_slot_yaml["name"].as_str().unwrap().to_string();

                        let busy_time_slot = BusyTimeSlot::new(
                            start_time_hour,
                            start_time_minute,
                            duration_minutes,
                            name,
                        );
                        busy_time_slots.push(busy_time_slot);
                    }

                    day_of_week_map.insert(day_of_week, busy_time_slots);
                }
            }
        }
        day_of_week_map
    }

    fn get_free_time_slot(&self, date: NaiveDate) -> Vec<i64> {
        let mut free_time_slot = vec![1; 24 * 60];

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

fn mark_busy_time_slot(free_time_slot: &mut [i64], busy_time_slot: &BusyTimeSlot) {
    let start_index =
        (busy_time_slot.get_start_time_hour() * 60 + busy_time_slot.get_start_time_minute()) as i64;
    let end_index = (start_index + busy_time_slot.get_duration_minutes()).clamp(0, 24 * 60);

    for index in start_index.clamp(0, 24 * 60)..end_index {
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
                24 * 60
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

    // [start, end)
    // TODO: エラー処理
    fn register_busy_time_slot(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) {
        if start.date_naive() != end.date_naive() {
            panic!("different date between start and end.");
        }

        let date = start.date_naive();
        let free_time_slot = self
            .registered_busy_time_slots_map
            .entry(date)
            .or_insert(vec![1; 24 * 60]);

        let start_index = start.hour() * 60 + start.minute();
        let end_index = end.hour() * 60 + end.minute();

        for ind in start_index..end_index {
            free_time_slot[ind as usize] = 0;
        }
    }

    fn load_busy_time_slots_from_file(&mut self, busy_time_slots_file_path: &str) {
        self.load_busy_time_slots_from_file(busy_time_slots_file_path);
    }
}

#[test]
fn test_get_free_minutes_簡単なケース1() {
    let mut ft_mng = FreeTimeManager::new();

    let start = Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2000, 1, 1, 0, 2, 3).unwrap();
    let actual = ft_mng.get_free_minutes(&start, &end);

    assert_eq!(actual, 2);
}

#[test]
fn test_get_free_minutes_丸1日のケース() {
    let mut ft_mng = FreeTimeManager::new();

    let start = Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2000, 1, 1, 23, 59, 59).unwrap();
    let actual = ft_mng.get_free_minutes(&start, &end);

    assert_eq!(actual, 60 * 23 + 59);
}

#[test]
fn test_register_busy_time_slot_簡単なケース() {
    let mut ft_mng = FreeTimeManager::new();

    let start_busy = Local.with_ymd_and_hms(2000, 1, 1, 13, 0, 0).unwrap();
    let end_busy = Local.with_ymd_and_hms(2000, 1, 1, 14, 0, 0).unwrap();

    ft_mng.register_busy_time_slot(&start_busy, &end_busy);

    let start = Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2000, 1, 1, 23, 59, 59).unwrap();
    let actual = ft_mng.get_free_minutes(&start, &end);

    assert_eq!(actual, 60 * 23 + 59 - 60);
}

#[test]
fn test_get_busy_minutes_簡単なケース() {
    let mut ft_mng = FreeTimeManager::new();

    let start_busy = Local.with_ymd_and_hms(2000, 1, 1, 13, 0, 0).unwrap();
    let end_busy = Local.with_ymd_and_hms(2000, 1, 1, 14, 0, 0).unwrap();

    ft_mng.register_busy_time_slot(&start_busy, &end_busy);

    let start = Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2000, 1, 1, 23, 59, 59).unwrap();
    let actual = ft_mng.get_busy_minutes(&start, &end);

    assert_eq!(actual, 60);
}

#[cfg(test)]
fn busy_time_slots_yaml_with_daily_slot(include_legacy_end_of_day_fields: bool) -> String {
    let legacy_end_of_day_fields = if include_legacy_end_of_day_fields {
        "    end_of_day_hour: ignored\n    end_of_day_minute: ignored\n"
    } else {
        "    end_of_day_hour: 0\n    end_of_day_minute: 0\n"
    };
    let mut days = String::new();
    for day_of_week in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
        write!(
            days,
            "  - day_of_week: {day_of_week}\n{legacy_end_of_day_fields}    busy_time_slots:\n      - start_time: \"00:00\"\n        duration_minutes: 60\n        name: sleep\n"
        )
        .unwrap();
    }
    format!("days_of_week:\n{days}")
}

#[cfg(test)]
fn write_busy_time_slots_yaml(include_legacy_end_of_day_fields: bool) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "schronu-free-time-manager-{}.yaml",
        uuid::Uuid::new_v4()
    ));
    fs::write(
        &path,
        busy_time_slots_yaml_with_daily_slot(include_legacy_end_of_day_fields),
    )
    .unwrap();
    path
}

#[test]
fn load_busy_time_slots_from_file_70日を超える将来日にも毎週定期slotを適用する() {
    let path = write_busy_time_slots_yaml(false);
    let mut manager = FreeTimeManager::new();
    manager.load_busy_time_slots_from_file(path.to_str().unwrap());

    let start = Local.with_ymd_and_hms(2026, 10, 20, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 10, 20, 1, 0, 0).unwrap();

    assert_eq!(manager.get_free_minutes(&start, &end), 0);
    fs::remove_file(path).unwrap();
}

#[test]
fn get_free_minutes_日跨ぎ照会でも各日の定期slotを差し引く() {
    let path = write_busy_time_slots_yaml(false);
    let mut manager = FreeTimeManager::new();
    manager.load_busy_time_slots_from_file(path.to_str().unwrap());

    let start = Local.with_ymd_and_hms(2026, 8, 10, 23, 30, 0).unwrap();
    let middle = Local.with_ymd_and_hms(2026, 8, 11, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 8, 12, 1, 30, 0).unwrap();

    assert_eq!(manager.get_free_minutes(&start, &end), 24 * 60);
    assert_eq!(
        manager.get_free_minutes(&start, &end),
        manager.get_free_minutes(&start, &middle) + manager.get_free_minutes(&middle, &end)
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn get_free_minutes_23時59分から翌日00時00分を1分として扱う() {
    let mut manager = FreeTimeManager::new();
    let start = Local.with_ymd_and_hms(2026, 8, 10, 23, 59, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 8, 11, 0, 0, 0).unwrap();

    assert_eq!(manager.get_free_minutes(&start, &end), 1);
}

#[test]
fn get_free_minutes_終了が開始以前なら0分を返す() {
    let mut manager = FreeTimeManager::new();
    let start = Local.with_ymd_and_hms(2026, 8, 11, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 8, 10, 23, 59, 0).unwrap();

    assert_eq!(manager.get_free_minutes(&start, &end), 0);
}

#[test]
fn get_free_minutes_06時を跨いでも通常の日付境界で定期slotを適用する() {
    let path = write_busy_time_slots_yaml(false);
    let mut manager = FreeTimeManager::new();
    manager.load_busy_time_slots_from_file(path.to_str().unwrap());

    let start = Local.with_ymd_and_hms(2026, 8, 10, 23, 30, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 8, 11, 6, 30, 0).unwrap();

    assert_eq!(manager.get_free_minutes(&start, &end), 6 * 60);
    fs::remove_file(path).unwrap();
}

#[test]
fn load_busy_time_slots_from_file_廃止した日終端fieldを無視する() {
    let path = write_busy_time_slots_yaml(true);
    let mut manager = FreeTimeManager::new();
    manager.load_busy_time_slots_from_file(path.to_str().unwrap());

    let start = Local.with_ymd_and_hms(2026, 8, 10, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 8, 10, 1, 0, 0).unwrap();

    assert_eq!(manager.get_free_minutes(&start, &end), 0);
    fs::remove_file(path).unwrap();
}

#[test]
fn load_busy_time_slots_from_file_再読込後も明示slotを維持する() {
    let path = write_busy_time_slots_yaml(false);
    let mut manager = FreeTimeManager::new();
    manager.load_busy_time_slots_from_file(path.to_str().unwrap());

    let explicit_start = Local.with_ymd_and_hms(2026, 8, 10, 2, 0, 0).unwrap();
    let explicit_end = Local.with_ymd_and_hms(2026, 8, 10, 3, 0, 0).unwrap();
    manager.register_busy_time_slot(&explicit_start, &explicit_end);
    manager.load_busy_time_slots_from_file(path.to_str().unwrap());

    assert_eq!(manager.get_free_minutes(&explicit_start, &explicit_end), 0);
    fs::remove_file(path).unwrap();
}

#[test]
fn get_free_minutes_明示slotと定期slotが重なっても二重控除しない() {
    let path = write_busy_time_slots_yaml(false);
    let mut manager = FreeTimeManager::new();
    manager.load_busy_time_slots_from_file(path.to_str().unwrap());

    let start = Local.with_ymd_and_hms(2026, 8, 10, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 8, 10, 2, 0, 0).unwrap();
    let explicit_busy_end = Local.with_ymd_and_hms(2026, 8, 10, 1, 30, 0).unwrap();
    manager.register_busy_time_slot(&start, &explicit_busy_end);

    assert_eq!(manager.get_free_minutes(&start, &end), 30);
    fs::remove_file(path).unwrap();
}
