use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[test]
fn test_get_free_minutes_簡単なケース1() {
    let mut ft_mng = FreeTimeManager::new();

    let start = Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2000, 1, 1, 0, 2, 3).unwrap();
    let actual = ft_mng.get_free_minutes(&start, &end);

    assert_eq!(actual, 2);
}

#[test]
fn get_free_secondsは開始終了の端数秒を保持する() {
    let mut manager = FreeTimeManager::new();
    let start = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 59).unwrap();
    let end = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 1).unwrap();

    assert_eq!(manager.get_free_seconds(&start, &end), 2);
}

#[test]
fn get_free_secondsはcurrent_minuteのbusy部分だけを除外する() {
    let mut manager = FreeTimeManager::new();
    let busy_start = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 0).unwrap();
    let busy_end = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 0).unwrap();
    manager
        .register_busy_time_slot(&busy_start, &busy_end)
        .unwrap();
    let start = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 59).unwrap();
    let end = Local.with_ymd_and_hms(2026, 9, 5, 19, 1, 1).unwrap();

    assert_eq!(manager.get_free_seconds(&start, &end), 1);
}

#[test]
fn get_free_secondsは日跨ぎの半開区間を秒精度で合計する() {
    let mut manager = FreeTimeManager::new();
    let start = Local.with_ymd_and_hms(2026, 9, 5, 23, 59, 59).unwrap();
    let end = Local.with_ymd_and_hms(2026, 9, 6, 0, 0, 1).unwrap();

    assert_eq!(manager.get_free_seconds(&start, &end), 2);
    assert_eq!(manager.get_free_seconds(&end, &start), 0);
}

#[test]
fn free_time_traitの秒api既定実装は既存の分値を秒へ変換する() {
    struct MinuteOnlyManager;

    impl FreeTimeManagerTrait for MinuteOnlyManager {
        fn get_free_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
            7
        }

        fn get_busy_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
            0
        }

        fn register_busy_time_slot(
            &mut self,
            _start: &DateTime<Local>,
            _end: &DateTime<Local>,
        ) -> Result<(), BusyTimeSlotRegistrationError> {
            Ok(())
        }

        fn load_busy_time_slots_from_file(
            &mut self,
            _busy_time_slots_file_path: &str,
        ) -> Result<(), BusyTimeSlotLoadError> {
            Ok(())
        }
    }

    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    assert_eq!(MinuteOnlyManager.get_free_seconds(&now, &now), 420);
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

    ft_mng
        .register_busy_time_slot(&start_busy, &end_busy)
        .expect("同日内のbusy slotは登録できるべきです");

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

    ft_mng
        .register_busy_time_slot(&start_busy, &end_busy)
        .expect("同日内のbusy slotは登録できるべきです");

    let start = Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2000, 1, 1, 23, 59, 59).unwrap();
    let actual = ft_mng.get_busy_minutes(&start, &end);

    assert_eq!(actual, 60);
}

#[cfg(test)]
fn busy_time_slots_yaml_with_daily_slot() -> String {
    let mut days = String::new();
    for day_of_week in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
        write!(
            days,
            "  - day_of_week: {day_of_week}\n    busy_time_slots:\n      - start_time: \"00:00\"\n        duration_minutes: 60\n        name: sleep\n"
        )
        .unwrap();
    }
    format!("days_of_week:\n{days}")
}

#[cfg(test)]
fn write_busy_time_slots_yaml() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "schronu-free-time-manager-{}.yaml",
        uuid::Uuid::new_v4()
    ));
    fs::write(&path, busy_time_slots_yaml_with_daily_slot()).unwrap();
    path
}

#[test]
fn load_busy_time_slots_from_file_70日を超える将来日にも毎週定期slotを適用する() {
    let path = write_busy_time_slots_yaml();
    let mut manager = FreeTimeManager::new();
    manager
        .load_busy_time_slots_from_file(path.to_str().unwrap())
        .expect("正常なbusy_time_slots.yamlは読み込めるべきです");

    let start = Local.with_ymd_and_hms(2026, 10, 20, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 10, 20, 1, 0, 0).unwrap();

    assert_eq!(manager.get_free_minutes(&start, &end), 0);
    fs::remove_file(path).unwrap();
}

#[test]
fn get_free_minutes_日跨ぎ照会でも各日の定期slotを差し引く() {
    let path = write_busy_time_slots_yaml();
    let mut manager = FreeTimeManager::new();
    manager
        .load_busy_time_slots_from_file(path.to_str().unwrap())
        .expect("正常なbusy_time_slots.yamlは読み込めるべきです");

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
    let path = write_busy_time_slots_yaml();
    let mut manager = FreeTimeManager::new();
    manager
        .load_busy_time_slots_from_file(path.to_str().unwrap())
        .expect("正常なbusy_time_slots.yamlは読み込めるべきです");

    let start = Local.with_ymd_and_hms(2026, 8, 10, 23, 30, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 8, 11, 6, 30, 0).unwrap();

    assert_eq!(manager.get_free_minutes(&start, &end), 6 * 60);
    fs::remove_file(path).unwrap();
}

#[test]
fn load_busy_time_slots_from_file_週次slotを読み込む() {
    let path = write_busy_time_slots_yaml();
    let mut manager = FreeTimeManager::new();
    manager
        .load_busy_time_slots_from_file(path.to_str().unwrap())
        .expect("正常なbusy_time_slots.yamlは読み込めるべきです");

    let start = Local.with_ymd_and_hms(2026, 8, 10, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 8, 10, 1, 0, 0).unwrap();

    assert_eq!(manager.get_free_minutes(&start, &end), 0);
    fs::remove_file(path).unwrap();
}

#[test]
fn load_busy_time_slots_from_file_再読込後も明示slotを維持する() {
    let path = write_busy_time_slots_yaml();
    let mut manager = FreeTimeManager::new();
    manager
        .load_busy_time_slots_from_file(path.to_str().unwrap())
        .expect("正常なbusy_time_slots.yamlは読み込めるべきです");

    let explicit_start = Local.with_ymd_and_hms(2026, 8, 10, 2, 0, 0).unwrap();
    let explicit_end = Local.with_ymd_and_hms(2026, 8, 10, 3, 0, 0).unwrap();
    manager
        .register_busy_time_slot(&explicit_start, &explicit_end)
        .expect("同日内の明示busy slotは登録できるべきです");
    manager
        .load_busy_time_slots_from_file(path.to_str().unwrap())
        .expect("正常なbusy_time_slots.yamlは再読込できるべきです");

    assert_eq!(manager.get_free_minutes(&explicit_start, &explicit_end), 0);
    fs::remove_file(path).unwrap();
}

#[cfg(test)]
struct BusyTimeSlotsYamlFile {
    path: PathBuf,
}

#[cfg(test)]
static TEST_FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
fn unique_test_fixture_path(prefix: &str, suffix: &str) -> PathBuf {
    let sequence = TEST_FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);

    std::env::temp_dir().join(format!(
        "schronu-{prefix}-{}-{sequence}{suffix}",
        std::process::id(),
    ))
}

#[cfg(test)]
impl BusyTimeSlotsYamlFile {
    fn new(contents: &str) -> Self {
        let path = unique_test_fixture_path("busy-time-slots", ".yaml");
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
        let path = unique_test_fixture_path("busy-time-slots-directory", "");
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
            "  - day_of_week: {day_of_week}\n    busy_time_slots:\n      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n"
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

#[cfg(test)]
fn assert_load_error_value(
    manager: &mut FreeTimeManager,
    path: &Path,
    expected_field_path: &str,
    expected_value: &str,
) {
    let error = manager
        .load_busy_time_slots_from_file(path.to_str().unwrap())
        .expect_err("不正なbusy_time_slots.yamlは回復可能なエラーになるべきです");

    assert_eq!(error.path(), path);
    assert_eq!(error.field_path(), expected_field_path);
    assert!(error
        .value()
        .expect("型違いでは不正値を保持するべきです")
        .contains(expected_value));
    assert!(error.source().is_some());
}

#[cfg(test)]
fn assert_load_error_has_no_value(
    manager: &mut FreeTimeManager,
    path: &Path,
    expected_field_path: &str,
) {
    let error = manager
        .load_busy_time_slots_from_file(path.to_str().unwrap())
        .expect_err("不正なbusy_time_slots.yamlは回復可能なエラーになるべきです");

    assert_eq!(error.path(), path);
    assert_eq!(error.field_path(), expected_field_path);
    assert_eq!(error.value(), None);
    assert!(error.source().is_some());
}

#[test]
fn test_load_busy_time_slots_from_file_存在しないファイルはpathとfield_pathを含むエラーになる() {
    let path = unique_test_fixture_path("missing-busy-time-slots", ".yaml");
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, &path, "$");
}

#[test]
fn test_load_busy_time_slots_from_file_存在しないファイルのエラーは構造化情報を保持する() {
    let path = unique_test_fixture_path("missing-busy-time-slots", ".yaml");
    let mut manager = FreeTimeManager::new();

    let error = manager
        .load_busy_time_slots_from_file(path.to_str().unwrap())
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
    let yaml = valid_busy_time_slots_yaml().replacen("  - day_of_week: Sun\n    busy_time_slots:\n      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n", "", 1);
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
fn test_load_busy_time_slots_from_file_days_of_weekの型違いは不正値を保持する() {
    let file = BusyTimeSlotsYamlFile::new("days_of_week: invalid\n");
    let mut manager = FreeTimeManager::new();

    assert_load_error_value(&mut manager, file.path(), "days_of_week", "invalid");
}

#[test]
fn test_load_busy_time_slots_from_file_day_of_weekの型違いは不正値を保持する() {
    let yaml = valid_busy_time_slots_yaml().replacen("day_of_week: Mon", "day_of_week: 123", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_value(
        &mut manager,
        file.path(),
        "days_of_week[0].day_of_week",
        "123",
    );
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
fn test_load_busy_time_slots_from_file_days_of_week要素がmappingでない場合はpathとfield_pathを含むエラーになる(
) {
    let yaml = valid_busy_time_slots_yaml().replacen(
        "  - day_of_week: Mon\n    busy_time_slots:\n      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n",
        "  - invalid\n",
        1,
    );
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_value(&mut manager, file.path(), "days_of_week[0]", "invalid");
}

#[test]
fn test_load_busy_time_slots_from_file_day_of_week欠落は不正値なしで報告する() {
    let yaml = valid_busy_time_slots_yaml().replacen("  - day_of_week: Mon\n", "  -\n", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_has_no_value(&mut manager, file.path(), "days_of_week[0].day_of_week");
}

#[test]
fn test_load_busy_time_slots_from_file_busy_time_slotsの型違いはpathとfield_pathを含むエラーになる()
{
    let yaml = valid_busy_time_slots_yaml().replacen(
        "    busy_time_slots:\n      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n",
        "    busy_time_slots: lunch\n",
        1,
    );
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "days_of_week[0].busy_time_slots");
}

#[test]
fn test_load_busy_time_slots_from_file_busy_time_slotsの型違いは不正値を保持する() {
    let yaml = valid_busy_time_slots_yaml().replacen(
        "    busy_time_slots:\n      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n",
        "    busy_time_slots: invalid\n",
        1,
    );
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_value(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots",
        "invalid",
    );
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

    assert_load_error_value(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0]",
        "invalid",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_空_yamlはpathとfield_pathを含むエラーになる() {
    let file = BusyTimeSlotsYamlFile::new("");
    let mut manager = FreeTimeManager::new();

    assert_load_error_contains(&mut manager, file.path(), "$");
}

#[test]
fn test_load_busy_time_slots_from_file_型違いはpathとfield_pathを含むエラーになる() {
    let yaml =
        valid_busy_time_slots_yaml().replacen("duration_minutes: 60", "duration_minutes: sixty", 1);
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
    let mut manager = FreeTimeManager::new();

    let error = manager
        .load_busy_time_slots_from_file(file.path().to_str().unwrap())
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
fn test_load_busy_time_slots_from_file_nameの型違いは不正値を保持する() {
    let yaml = valid_busy_time_slots_yaml().replacen("name: lunch", "name: 123", 1);
    let file = BusyTimeSlotsYamlFile::new(&yaml);
    let mut manager = FreeTimeManager::new();

    assert_load_error_value(
        &mut manager,
        file.path(),
        "days_of_week[0].busy_time_slots[0].name",
        "123",
    );
}

#[test]
fn test_load_busy_time_slots_from_file_start_time欠落はpathとfield_pathを含むエラーになる() {
    let yaml = valid_busy_time_slots_yaml().replacen(
        "      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n",
        "      - duration_minutes: 60\n        name: lunch\n",
        1,
    );
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

    let error = manager
        .load_busy_time_slots_from_file(file.path().to_str().unwrap())
        .expect_err("start_timeの型違いは回復可能なエラーになるべきです");

    assert_eq!(error.path(), file.path());
    assert_eq!(
        error.field_path(),
        "days_of_week[0].busy_time_slots[0].start_time"
    );
    assert!(error
        .value()
        .expect("型違いでは不正値を保持するべきです")
        .contains("1300"));
    assert!(error.source().is_some());
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
    let yaml =
        valid_busy_time_slots_yaml().replacen("start_time: '13:00'", "start_time: '24:00'", 1);
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
    let yaml =
        valid_busy_time_slots_yaml().replacen("duration_minutes: 60", "duration_minutes: -1", 1);
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
    let start = Local.with_ymd_and_hms(2000, 1, 3, 13, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2000, 1, 3, 14, 0, 0).unwrap();
    let mut manager = FreeTimeManager::new();

    manager
        .load_busy_time_slots_from_file(valid_file.path().to_str().unwrap())
        .expect("正常なbusy_time_slots.yamlは読み込めるべきです");
    let before = manager.get_busy_minutes(&start, &end);

    let error = manager
        .load_busy_time_slots_from_file(invalid_file.path().to_str().unwrap())
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
    let mut manager = FreeTimeManager::new();

    let error = manager
        .load_busy_time_slots_from_file(file.path().to_str().unwrap())
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

    assert_load_error_has_no_value(
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
    let path = write_busy_time_slots_yaml();
    let mut manager = FreeTimeManager::new();
    manager
        .load_busy_time_slots_from_file(path.to_str().unwrap())
        .expect("正常なbusy_time_slots.yamlは読み込めるべきです");

    let start = Local.with_ymd_and_hms(2026, 8, 10, 0, 0, 0).unwrap();
    let end = Local.with_ymd_and_hms(2026, 8, 10, 2, 0, 0).unwrap();
    let explicit_busy_end = Local.with_ymd_and_hms(2026, 8, 10, 1, 30, 0).unwrap();
    manager
        .register_busy_time_slot(&start, &explicit_busy_end)
        .expect("同日内の明示busy slotは登録できるべきです");

    assert_eq!(manager.get_free_minutes(&start, &end), 30);
    fs::remove_file(path).unwrap();
}

#[test]
fn test_load_busy_time_slots_from_file_後半slotの異常読み込み時も既存状態を維持する() {
    let valid_file = BusyTimeSlotsYamlFile::new(&valid_busy_time_slots_yaml());
    let invalid_yaml = valid_busy_time_slots_yaml().replacen(
        "  - day_of_week: Mon\n    busy_time_slots:\n      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n",
        "  - day_of_week: Mon\n    busy_time_slots:\n      - start_time: '09:00'\n        duration_minutes: 30\n        name: morning-meeting\n",
        1,
    ).replacen(
        "  - day_of_week: Sun\n    busy_time_slots:\n      - start_time: '13:00'\n        duration_minutes: 60\n        name: lunch\n",
        "  - day_of_week: Sun\n    busy_time_slots:\n      - start_time: '23:30'\n        duration_minutes: 60\n        name: lunch\n",
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
