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
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};

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

#[cfg(test)]
struct BusyTimeSlotsYamlFile {
    path: PathBuf,
}

#[cfg(test)]
impl BusyTimeSlotsYamlFile {
    fn new(contents: &str) -> Self {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "schronu-busy-time-slots-{}-{}.yaml",
            std::process::id(),
            unique_suffix
        ));
        fs::write(&path, contents).unwrap();

        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
impl Drop for BusyTimeSlotsYamlFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
struct BusyTimeSlotsYamlDirectory {
    path: PathBuf,
}

#[cfg(test)]
impl BusyTimeSlotsYamlDirectory {
    fn new() -> Self {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "schronu-busy-time-slots-directory-{}-{}",
            std::process::id(),
            unique_suffix
        ));
        fs::create_dir(&path).unwrap();

        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
impl Drop for BusyTimeSlotsYamlDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.path);
    }
}

#[cfg(test)]
fn valid_busy_time_slots_yaml() -> String {
    let mut yaml = String::from("days_of_week:\n");
    for day_of_week in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
        yaml.push_str(&format!(
            "  - day_of_week: {day_of_week}\n    end_of_day_hour: 23\n    end_of_day_minute: 59\n    busy_time_slots:\n      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n"
        ));
    }
    yaml
}

#[cfg(test)]
fn assert_load_error_contains(
    manager: &mut FreeTimeManager,
    path: &Path,
    expected_field_path: &str,
) {
    let error = manager
        .load_busy_time_slots_from_file(path.to_str().unwrap())
        .expect_err("不正なbusy_time_slots.yamlは回復可能なエラーになるべきです");
    let message = error.to_string();

    assert!(message.contains(path.to_str().unwrap()));
    assert!(message.contains(expected_field_path));
}

#[test]
fn test_load_busy_time_slots_from_file_存在しないファイルはpathとfield_pathを含むエラーになる() {
    let path = std::env::temp_dir().join(format!(
        "schronu-missing-busy-time-slots-{}-{}.yaml",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, &path, "$");
}

#[test]
fn test_load_busy_time_slots_from_file_存在しないファイルのエラーは構造化情報を保持する() {
    let path = std::env::temp_dir().join(format!(
        "schronu-missing-busy-time-slots-{}-{}.yaml",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let now = Local.with_ymd_and_hms(2000, 1, 3, 0, 0, 0).unwrap();
    let mut manager = FreeTimeManager::new();

    let error = manager
        .load_busy_time_slots_from_file(path.to_str().unwrap(), &now)
        .expect_err("存在しない設定ファイルは回復可能なエラーになるべきです");

    assert_eq!(error.path(), path.as_path());
    assert_eq!(error.field_path(), "$");
    assert_eq!(error.value(), None);
    assert!(error.source().is_some());
}

#[test]
fn test_load_busy_time_slots_from_file_ディレクトリpathはpathとfield_pathを含むエラーになる() {
    let directory = BusyTimeSlotsYamlDirectory::new();
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, directory.path(), "$");
}

#[test]
fn test_load_busy_time_slots_from_file_不正_yamlはpathとfield_pathを含むエラーになる() {
    let file = BusyTimeSlotsYamlFile::new("days_of_week: [\n");
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "$");
}

#[test]
fn test_load_busy_time_slots_from_file_曜日欠落はpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml().replacen("  - day_of_week: Sun\n    end_of_day_hour: 23\n    end_of_day_minute: 59\n    busy_time_slots:\n      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n", "", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "days_of_week");
}

#[test]
fn test_load_busy_time_slots_from_file_days_of_week欠落はpathとfield_pathを含むエラーになる() {
    let file = BusyTimeSlotsYamlFile::new("other: []\n");
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "days_of_week");
}

#[test]
fn test_load_busy_time_slots_from_file_days_of_weekの型違いはpathとfield_pathを含むエラーになる() {
    let file = BusyTimeSlotsYamlFile::new("days_of_week: invalid\n");
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "days_of_week");
}

#[test]
fn test_load_busy_time_slots_from_file_未知曜日はpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml().replacen("day_of_week: Mon", "day_of_week: Holiday", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "days_of_week[0].day_of_week");
}

#[test]
fn test_load_busy_time_slots_from_file_重複曜日はpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml().replacen("day_of_week: Tue", "day_of_week: Mon", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "days_of_week[1].day_of_week");
}

#[test]
fn test_load_busy_time_slots_from_file_end_of_day_hourの型違いはpathとfield_pathを含むエラーになる()
{
    let yaml =
        valid_busy_time_slots_yaml().replacen("end_of_day_hour: 23", "end_of_day_hour: late", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "days_of_week[0].end_of_day_hour");
}

#[test]
fn test_load_busy_time_slots_from_file_end_of_day_hourの範囲外値はpathとfield_pathを含むエラーになる(
) {
    let yaml =
        valid_busy_time_slots_yaml().replacen("end_of_day_hour: 23", "end_of_day_hour: 24", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "days_of_week[0].end_of_day_hour");
}

#[test]
fn test_load_busy_time_slots_from_file_end_of_day_hourの負数は回復可能なエラーになる() {
    let yaml =
        valid_busy_time_slots_yaml().replacen("end_of_day_hour: 23", "end_of_day_hour: -1", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "days_of_week[0].end_of_day_hour");
}

#[test]
fn test_load_busy_time_slots_from_file_end_of_day_hour欠落はpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml().replacen("    end_of_day_hour: 23\n", "", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "days_of_week[0].end_of_day_hour");
}

#[test]
fn test_load_busy_time_slots_from_file_end_of_day_minuteの型違いはpathとfield_pathを含むエラーになる(
) {
    let yaml = valid_busy_time_slots_yaml().replacen(
        "end_of_day_minute: 59",
        "end_of_day_minute: late",
        1,
    );
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].end_of_day_minute",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_end_of_day_minute欠落はpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml().replacen("    end_of_day_minute: 59\n", "", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].end_of_day_minute",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_end_of_day_minuteの範囲外値はpathとfield_pathを含むエラーになる(
) {
    let yaml =
        valid_busy_time_slots_yaml().replacen("end_of_day_minute: 59", "end_of_day_minute: 60", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].end_of_day_minute",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_end_of_day_minuteの負数は回復可能なエラーになる() {
    let yaml = valid_busy_time_slots_yaml().replacen(
        "end_of_day_minute: 59",
        "end_of_day_minute: -1",
        1,
    );
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].end_of_day_minute",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_days_of_week要素がmappingでない場合はpathとfield_pathを含むエラーになる(
) {
    let yaml = valid_busy_time_slots_yaml().replacen(
        "  - day_of_week: Mon\n    end_of_day_hour: 23\n    end_of_day_minute: 59\n    busy_time_slots:\n      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n",
        "  - invalid\n",
        1,
    );
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "days_of_week[0]");
}

#[test]
fn test_load_busy_time_slots_from_file_busy_time_slotsの型違いはpathとfield_pathを含むエラーになる()
{
    let yaml =
        valid_busy_time_slots_yaml().replacen("busy_time_slots:\n", "busy_time_slots: lunch\n", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "days_of_week[0].busy_time_slots");
}

#[test]
fn test_load_busy_time_slots_from_file_busy_time_slots欠落はpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml().replacen(
        "    busy_time_slots:\n      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n",
        "",
        1,
    );
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "days_of_week[0].busy_time_slots");
}

#[test]
fn test_load_busy_time_slots_from_file_busy_time_slots要素がmappingでない場合はpathとfield_pathを含むエラーになる(
) {
    let yaml = valid_busy_time_slots_yaml().replacen(
        "      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n",
        "      - invalid\n",
        1,
    );
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0]",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_空YAMLはpathとfield_pathを含むエラーになる() {
    let file = BusyTimeSlotsYamlFile::new("");
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "$");
}

#[test]
fn test_load_busy_time_slots_from_file_型違いはpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml()
        .replacen("duration_minutes: 60", "duration_minutes: sixty", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0].duration_minutes",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_型違いのエラーは構造化情報を保持する() {
    let yaml =
        valid_busy_time_slots_yaml().replacen("duration_minutes: 60", "duration_minutes: sixty", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let now = Local.with_ymd_and_hms(2000, 1, 3, 0, 0, 0).unwrap();
    let mut manager = FreeTimeManager::new();

    let error = manager
        .load_busy_time_slots_from_file(file.path().to_str().unwrap(), &now)
        .expect_err("型違いの設定値は回復可能なエラーになるべきです");

    assert_eq!(error.path(), file.path());
    assert_eq!(
        error.field_path(),
        "days_of_week[0].busy_time_slots[0].duration_minutes"
    );
    assert!(error
        .value()
        .expect("型違いでは不正値を保持するべきです")
        .contains("sixty"));
    assert!(error.source().is_some());
}

#[test]
fn test_load_busy_time_slots_from_file_start_time欠落はpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml().replacen("        start_time: '13:00'\n", "", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0].start_time",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_start_timeの型違いはpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml().replacen("start_time: '13:00'", "start_time: 1300", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0].start_time",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_start_timeのminute超過はpathとfield_pathを含むエラーになる()
{
    let yaml =
        valid_busy_time_slots_yaml().replacen("start_time: '13:00'", "start_time: '13:60'", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0].start_time",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_start_timeの形式不正はpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml().replacen("start_time: '13:00'", "start_time: '13'", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0].start_time",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_start_timeの数値変換失敗はpathとfield_pathを含むエラーになる(
) {
    let yaml =
        valid_busy_time_slots_yaml().replacen("start_time: '13:00'", "start_time: 'noon:00'", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0].start_time",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_不正時刻はpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml()
        .replacen("start_time: '13:00'", "start_time: '24:00'", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0].start_time",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_duration_minutes欠落はpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml().replacen("        duration_minutes: 60\n", "", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0].duration_minutes",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_負数durationはpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml()
        .replacen("duration_minutes: 60", "duration_minutes: -1", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0].duration_minutes",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_日跨ぎslotはpathとfield_pathを含むエラーになる() {
    let yaml =
        valid_busy_time_slots_yaml().replacen("start_time: '13:00'", "start_time: '23:30'", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0].duration_minutes",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_duration_minutesの最大値はpanicせず原子性を維持する() {
    let valid_file = BusyTimeSlotsYamlFile::new(&valid_busy_time_slots_yaml());
    let invalid_yaml = valid_busy_time_slots_yaml().replacen(
        "duration_minutes: 60",
        &format!("duration_minutes: {}", i64::MAX),
        1,
    );
    let invalid_file = BusyTimeSlotsYamlFile::new(&invalid_yaml);
    let now = Local.with_ymd_and_hms(2000, 1, 3, 0, 0, 0).unwrap();
    let start = Local.with_ymd_and_hms(2000, 1, 3, 13, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2000, 1, 3, 14, 0, 0).unwrap();
    let mut manager = FreeTimeManager::new();

    manager
        .load_busy_time_slots_from_file(valid_file.path().to_str().unwrap(), &now)
        .expect("正常なbusy_time_slots.yamlは読み込めるべきです");
    let before = manager.get_busy_minutes(&start, &end);

    let error = manager
        .load_busy_time_slots_from_file(invalid_file.path().to_str().unwrap(), &now)
        .expect_err("duration_minutesの最大値は回復可能なエラーになるべきです");

    assert_eq!(error.path(), invalid_file.path());
    assert_eq!(
        error.field_path(),
        "days_of_week[0].busy_time_slots[0].duration_minutes"
    );
    assert_eq!(error.value(), Some(i64::MAX.to_string()).as_deref());
    assert_eq!(manager.get_busy_minutes(&start, &end), before);
}

#[test]
fn test_load_busy_time_slots_from_file_23時開始の日跨ぎはdurationの構造化エラーになる() {
    let yaml =
        valid_busy_time_slots_yaml().replacen("start_time: '13:00'", "start_time: '23:00'", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let now = Local.with_ymd_and_hms(2000, 1, 3, 0, 0, 0).unwrap();
    let mut manager = FreeTimeManager::new();

    let error = manager
        .load_busy_time_slots_from_file(file.path().to_str().unwrap(), &now)
        .expect_err("日跨ぎslotはdurationの回復可能なエラーになるべきです");

    assert_eq!(error.path(), file.path());
    assert_eq!(
        error.field_path(),
        "days_of_week[0].busy_time_slots[0].duration_minutes"
    );
    assert_eq!(error.value(), Some("60".to_string()).as_deref());
}

#[test]
fn test_load_busy_time_slots_from_file_nameの型違いはpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml().replacen("name: lunch", "name: 123", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0].name",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_name欠落はpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml().replacen("        name: lunch\n", "", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0].name",
    );
}

#[test]
fn test_register_busy_time_slot_日跨ぎは回復可能なエラーになる() {
    let mut manager = FreeTimeManager::new();
    let start = Local.with_ymd_and_hms(2000, 1, 1, 23, 30, 0).unwrap();
    let end = Local.with_ymd_and_hms(2000, 1, 2, 0, 30, 0).unwrap();

    let error = manager
        .register_busy_time_slot(&start, &end)
        .expect_err("日跨ぎslotはpanicせず登録エラーになるべきです");

    assert!(error.to_string().contains("different date"));
}

#[test]
fn test_register_busy_time_slot_日跨ぎエラー後も既存状態を維持する() {
    let mut manager = FreeTimeManager::new();
    let existing_start = Local.with_ymd_and_hms(2000, 1, 1, 13, 0, 0).unwrap();
    let existing_end = Local.with_ymd_and_hms(2000, 1, 1, 14, 0, 0).unwrap();
    let invalid_start = Local.with_ymd_and_hms(2000, 1, 1, 23, 30, 0).unwrap();
    let invalid_end = Local.with_ymd_and_hms(2000, 1, 2, 0, 30, 0).unwrap();

    manager
        .register_busy_time_slot(&existing_start, &existing_end)
        .expect("同日のslotは登録できるべきです");
    let before = manager.get_busy_minutes(&existing_start, &existing_end);

    manager
        .register_busy_time_slot(&invalid_start, &invalid_end)
        .expect_err("日跨ぎslotは回復可能なエラーになるべきです");
    let after = manager.get_busy_minutes(&existing_start, &existing_end);

    assert_eq!(before, 60);
    assert_eq!(after, before);
}

#[test]
fn test_load_busy_time_slots_from_file_異常読み込み時は既存状態を維持する() {
    let valid_file = BusyTimeSlotsYamlFile::new(&valid_busy_time_slots_yaml());
    let invalid_file = BusyTimeSlotsYamlFile::new("days_of_week: [\n");
    let start = Local.with_ymd_and_hms(2000, 1, 3, 13, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2000, 1, 3, 14, 0, 0).unwrap();
    let mut manager = FreeTimeManager::new();

    manager
        .load_busy_time_slots_from_file(valid_file.path().to_str().unwrap())
        .expect("正常なbusy_time_slots.yamlは読み込めるべきです");
    let before = manager.get_busy_minutes(&start, &end);

    assert_load_error_contains(&mut manager, invalid_file.path(), "$");
    let after = manager.get_busy_minutes(&start, &end);

    assert_eq!(before, 60);
    assert_eq!(after, before);
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

#[test]
fn test_load_busy_time_slots_from_file_後半slotの異常読み込み時も既存状態を維持する() {
    let valid_file = BusyTimeSlotsYamlFile::new(&valid_busy_time_slots_yaml());
    let invalid_yaml = valid_busy_time_slots_yaml().replacen(
        "  - day_of_week: Mon\n    end_of_day_hour: 23\n    end_of_day_minute: 59\n    busy_time_slots:\n      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n",
        "  - day_of_week: Mon\n    end_of_day_hour: 23\n    end_of_day_minute: 59\n    busy_time_slots:\n      - start_time: '09:00'\n        duration_minutes: 30\n        name: morning-meeting\n",
        1,
    ).replacen(
        "  - day_of_week: Sun\n    end_of_day_hour: 23\n    end_of_day_minute: 59\n    busy_time_slots:\n      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n",
        "  - day_of_week: Sun\n    end_of_day_hour: 23\n    end_of_day_minute: 59\n    busy_time_slots:\n      - start_time: '23:30'\n        duration_minutes: 60\n        name: lunch\n",
        1,
    );
    let invalid_file = BusyTimeSlotsYamlFile::new(&invalid_yaml);
    let start = Local.with_ymd_and_hms(2000, 1, 3, 13, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2000, 1, 3, 14, 0, 0).unwrap();
    let candidate_start = Local.with_ymd_and_hms(2000, 1, 3, 9, 0, 0).unwrap();
    let candidate_end = Local.with_ymd_and_hms(2000, 1, 3, 9, 30, 0).unwrap();
    let mut manager = FreeTimeManager::new();

    manager
        .load_busy_time_slots_from_file(valid_file.path().to_str().unwrap())
        .expect("正常なbusy_time_slots.yamlは読み込めるべきです");
    let before = manager.get_busy_minutes(&start, &end);

    assert_load_error_contains(
        &mut manager,
        invalid_file.path(),
        "days_of_week[6].busy_time_slots[0].duration_minutes",
    );
    let after = manager.get_busy_minutes(&start, &end);
    let candidate_free_minutes = manager.get_free_minutes(&candidate_start, &candidate_end);

    assert_eq!(before, 60);
    assert_eq!(after, before);
    assert_eq!(candidate_free_minutes, 30);
}
