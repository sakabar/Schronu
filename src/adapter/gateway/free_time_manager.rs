use crate::application::interface::FreeTimeManagerTrait;
use chrono::{DateTime, Local, NaiveDate, Timelike};
use std::collections::HashMap;

#[cfg(test)]
use chrono::TimeZone;

// Scheduleをどう持つか: 日付をキーとする辞書
pub struct FreeTimeManager {
    free_time_slots_map: HashMap<NaiveDate, Vec<i64>>,
}

impl FreeTimeManager {
    pub fn new() -> Self {
        let free_time_slots_map = HashMap::new();

        Self {
            free_time_slots_map,
        }
    }
}

impl FreeTimeManagerTrait for FreeTimeManager {
    // 簡単のため、日を跨いだ後は全て自由な時間であるとする
    fn get_free_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
        let eod = start
            .with_hour(23)
            .expect("invalid hour")
            .with_minute(59)
            .expect("invalid minute");

        if start.date_naive() != end.date_naive() {
            return end.signed_duration_since(eod).num_minutes()
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
        ans
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
