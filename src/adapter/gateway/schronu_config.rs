#[cfg(test)]
mod tests {
    use chrono::{Duration, NaiveTime, Weekday};
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    use super::{load_schronu_config, SchronuConfig};

    fn write_config(directory: &Path, contents: &str) -> PathBuf {
        let path = directory.join("schronu.yaml");
        fs::write(&path, contents).unwrap();
        path
    }

    fn test_directory() -> PathBuf {
        let directory = env::temp_dir().join(format!("schronu-config-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn config_path未指定時は既存の既定値を返す() {
        let actual = load_schronu_config(None).unwrap();

        assert_eq!(actual.obsidian_vault_name, "Obsidian-Moica");
        assert_eq!(
            actual.busy_time_slots_yaml_path,
            PathBuf::from("../Schronu-private/busy_time_slots.yaml")
        );
        assert_eq!(actual.end_of_day_duration, Duration::minutes(30));
        assert_eq!(actual.calendar_blank_line_weekday, Weekday::Mon);
        assert!(actual.extrude_skip_weekdays.is_empty());
        assert_eq!(
            actual.default_deadline_time,
            NaiveTime::from_hms_opt(23, 59, 59).unwrap()
        );
    }

    #[test]
    fn config全項目を読み込み相対busy_yaml_pathは設定ファイル基準で解決する() {
        let directory = test_directory();
        let path = write_config(
            &directory,
            "obsidian_vault_name: Work\\nbusy_time_slots_yaml_path: schedules/busy.yaml\\nend_of_day_duration: '01:15'\\ncalendar_blank_line_weekday: Fri\\nextrude_skip_weekdays: [Sat, Sun]\\ndefault_deadline_time: '19:00'\\n",
        );

        let actual = load_schronu_config(Some(path.into_os_string())).unwrap();

        assert_eq!(actual.obsidian_vault_name, "Work");
        assert_eq!(
            actual.busy_time_slots_yaml_path,
            directory.join("schedules/busy.yaml")
        );
        assert_eq!(actual.end_of_day_duration, Duration::minutes(75));
        assert_eq!(actual.calendar_blank_line_weekday, Weekday::Fri);
        assert_eq!(actual.extrude_skip_weekdays, vec![Weekday::Sat, Weekday::Sun]);
        assert_eq!(
            actual.default_deadline_time,
            NaiveTime::from_hms_opt(19, 0, 0).unwrap()
        );
    }

    #[test]
    fn config未知キーはerrorにする() {
        let directory = test_directory();
        let path = write_config(&directory, "unknown: value\\n");

        let error = load_schronu_config(Some(path.into_os_string())).unwrap_err();

        assert!(error.contains("unknown"));
    }

    #[test]
    fn config不正値と重複曜日はerrorにする() {
        for contents in [
            "calendar_blank_line_weekday: Monday\\n",
            "end_of_day_duration: '24:00'\\n",
            "default_deadline_time: '25:00'\\n",
            "extrude_skip_weekdays: [Sat, Sat]\\n",
        ] {
            let directory = test_directory();
            let path = write_config(&directory, contents);

            assert!(load_schronu_config(Some(path.into_os_string())).is_err());
        }
    }

    #[test]
    fn config読込不能なpathは理由付きerrorにする() {
        let path = env::temp_dir().join(format!("missing-schronu-config-{}.yaml", Uuid::new_v4()));

        let error = load_schronu_config(Some(path.into_os_string())).unwrap_err();

        assert!(error.contains("config"));
    }

    #[test]
    fn config型は公開される() {
        let _: Option<SchronuConfig> = None;
    }
}
