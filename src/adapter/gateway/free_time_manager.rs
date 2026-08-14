use crate::application::interface::FreeTimeManagerTrait;
use crate::entity::busy_time_slot::{BusyTimeSlot, DayOfWeekBusyTimeSlots};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Timelike, Weekday};
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use yaml_rust::{Yaml, YamlLoader};

#[cfg(test)]
use chrono::TimeZone;
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::PathBuf;

// Scheduleをどう持つか: 日付をキーとする辞書
pub struct FreeTimeManager {
    free_time_slots_map: HashMap<NaiveDate, Vec<i64>>,
}

impl Default for FreeTimeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FreeTimeManager {
    pub fn new() -> Self {
        let free_time_slots_map = HashMap::new();

        Self {
            free_time_slots_map,
        }
    }

    fn load_busy_time_slots_from_file(
        &mut self,
        busy_time_slots_file_path: &str,
        now: &DateTime<Local>,
    ) {
        let mut file = File::open(busy_time_slots_file_path).unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();

        let day_of_week_map: HashMap<Weekday, DayOfWeekBusyTimeSlots> =
            self.load_busy_time_slots_from_str(&text);

        // 直近1週間は、週次の繰り返しタスクが入っているので正確。そのまま反映する
        // それ以降は、ダミータスクを入れることで繰り返しタスクのぶんの時間を表現する
        for d in 0..70 {
            let dt = *now + Duration::days(d);
            let day_of_week = dt.weekday();
            let day_of_week_busy_time_slots = day_of_week_map.get(&day_of_week).unwrap();

            for busy_time_slot in day_of_week_busy_time_slots.get_busy_time_slots().iter() {
                let hour = busy_time_slot.get_start_time_hour();
                let minute = busy_time_slot.get_start_time_minute();

                let start = dt
                    .with_hour(hour)
                    .expect("invalid hour")
                    .with_minute(minute)
                    .expect("invalid minute")
                    .with_second(0)
                    .expect("invalid second");

                let duration_minutes = &busy_time_slot.get_duration_minutes();
                let end = start + Duration::minutes(*duration_minutes);

                self.register_busy_time_slot(&start, &end);
            }
        }
    }

    fn load_busy_time_slots_from_str(
        &self,
        yaml_str: &str,
    ) -> HashMap<Weekday, DayOfWeekBusyTimeSlots> {
        let mut day_of_week_map: HashMap<Weekday, DayOfWeekBusyTimeSlots> = HashMap::new();

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

                    let end_of_day_hour = day_of_week_yaml["end_of_day_hour"].as_i64().unwrap();
                    let end_of_day_minute = day_of_week_yaml["end_of_day_hour"].as_i64().unwrap();
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

                    let day_of_week_busy_time_slots = DayOfWeekBusyTimeSlots::new(
                        day_of_week,
                        end_of_day_hour,
                        end_of_day_minute,
                        busy_time_slots,
                    );
                    day_of_week_map.insert(day_of_week, day_of_week_busy_time_slots);
                }
            }
        }
        day_of_week_map
    }
}

impl FreeTimeManagerTrait for FreeTimeManager {
    // 簡単のため、日を跨いだ後は全て自由な時間であるとする
    fn get_free_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
        const CAP_RATE: f64 = 1.0;

        let eod = start
            .with_hour(23)
            .expect("invalid hour")
            .with_minute(59)
            .expect("invalid minute");

        if start.date_naive() != end.date_naive() {
            return (end.signed_duration_since(eod).num_minutes() as f64 * CAP_RATE) as i64
                + self.get_free_minutes(start, &eod);
        }

        let date = start.date_naive();
        let free_time_slot = self
            .free_time_slots_map
            .entry(date)
            .or_insert(vec![1; 24 * 60]);

        let start_index = start.hour() * 60 + start.minute();
        let end_index = end.hour() * 60 + end.minute();

        let mut ans = 0;
        for ind in start_index..end_index {
            ans += free_time_slot[ind as usize];
        }

        (ans as f64 * CAP_RATE) as i64
    }

    fn get_busy_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
        let free_minutes = self.get_free_minutes(start, end);

        (*end - *start).num_minutes() - free_minutes
    }

    // 簡単のため、日を跨いだ取得はされない制約とする
    // [start, end)
    // その仕様から、取得できるのは23:59まで。
    // TODO: エラー処理
    fn register_busy_time_slot(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) {
        if start.date_naive() != end.date_naive() {
            panic!("different date between start and end.");
        }

        let date = start.date_naive();
        let free_time_slot = self
            .free_time_slots_map
            .entry(date)
            .or_insert(vec![1; 24 * 60]);

        let start_index = start.hour() * 60 + start.minute();
        let end_index = end.hour() * 60 + end.minute();

        for ind in start_index..end_index {
            free_time_slot[ind as usize] = 0;
        }
    }

    fn load_busy_time_slots_from_file(
        &mut self,
        busy_time_slots_file_path: &str,
        now: &DateTime<Local>,
    ) {
        self.load_busy_time_slots_from_file(busy_time_slots_file_path, now);
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
    let days = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
        .into_iter()
        .map(|day_of_week| {
            format!(
                "  - day_of_week: {day_of_week}\n{legacy_end_of_day_fields}    busy_time_slots:\n      - start_time: \"00:00\"\n        duration_minutes: 60\n        name: sleep\n"
            )
        })
        .collect::<String>();
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
    let now = Local.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap();
    manager.load_busy_time_slots_from_file(path.to_str().unwrap(), &now);

    let start = Local.with_ymd_and_hms(2026, 10, 20, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 10, 20, 1, 0, 0).unwrap();

    assert_eq!(manager.get_free_minutes(&start, &end), 0);
    fs::remove_file(path).unwrap();
}

#[test]
fn get_free_minutes_日跨ぎ照会でも各日の定期slotを差し引く() {
    let path = write_busy_time_slots_yaml(false);
    let mut manager = FreeTimeManager::new();
    let now = Local.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap();
    manager.load_busy_time_slots_from_file(path.to_str().unwrap(), &now);

    let start = Local.with_ymd_and_hms(2026, 8, 10, 23, 30, 0).unwrap();
    let middle = Local.with_ymd_and_hms(2026, 8, 11, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 8, 12, 1, 30, 0).unwrap();

    assert_eq!(manager.get_free_minutes(&start, &end), 24 * 60 + 30);
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
fn get_free_minutes_06時を跨いでも通常の日付境界で定期slotを適用する() {
    let path = write_busy_time_slots_yaml(false);
    let mut manager = FreeTimeManager::new();
    let now = Local.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap();
    manager.load_busy_time_slots_from_file(path.to_str().unwrap(), &now);

    let start = Local.with_ymd_and_hms(2026, 8, 10, 23, 30, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 8, 11, 6, 30, 0).unwrap();

    assert_eq!(manager.get_free_minutes(&start, &end), 6 * 60);
    fs::remove_file(path).unwrap();
}

#[test]
fn load_busy_time_slots_from_file_廃止した日終端fieldを無視する() {
    let path = write_busy_time_slots_yaml(true);
    let mut manager = FreeTimeManager::new();
    let now = Local.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap();
    manager.load_busy_time_slots_from_file(path.to_str().unwrap(), &now);

    let start = Local.with_ymd_and_hms(2026, 8, 10, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 8, 10, 1, 0, 0).unwrap();

    assert_eq!(manager.get_free_minutes(&start, &end), 0);
    fs::remove_file(path).unwrap();
}

#[test]
fn get_free_minutes_明示slotと定期slotが重なっても二重控除しない() {
    let path = write_busy_time_slots_yaml(false);
    let mut manager = FreeTimeManager::new();
    let now = Local.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap();
    manager.load_busy_time_slots_from_file(path.to_str().unwrap(), &now);

    let start = Local.with_ymd_and_hms(2026, 8, 10, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 8, 10, 2, 0, 0).unwrap();
    let explicit_busy_end = Local.with_ymd_and_hms(2026, 8, 10, 1, 30, 0).unwrap();
    manager.register_busy_time_slot(&start, &explicit_busy_end);

    assert_eq!(manager.get_free_minutes(&start, &end), 30);
    fs::remove_file(path).unwrap();
}
