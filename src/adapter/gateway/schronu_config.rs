#[cfg(test)]
mod tests {
    use chrono::{NaiveTime, Weekday};
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
        assert_eq!(actual.end_of_day_offset_minutes, 30);
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
            "obsidian_vault_name: Work\nbusy_time_slots_yaml_path: schedules/busy.yaml\nend_of_day_offset_minutes: -120\ncalendar_blank_line_weekday: Fri\nextrude_skip_weekdays: [Sat, Sun]\ndefault_deadline_time: '19:00'\n",
        );

        let actual = load_schronu_config(Some(path.into_os_string())).unwrap();

        assert_eq!(actual.obsidian_vault_name, "Work");
        assert_eq!(
            actual.busy_time_slots_yaml_path,
            directory.join("schedules/busy.yaml")
        );
        assert_eq!(actual.end_of_day_offset_minutes, -120);
        assert_eq!(actual.calendar_blank_line_weekday, Weekday::Fri);
        assert_eq!(
            actual.extrude_skip_weekdays,
            vec![Weekday::Sat, Weekday::Sun]
        );
        assert_eq!(
            actual.default_deadline_time,
            NaiveTime::from_hms_opt(19, 0, 0).unwrap()
        );
    }

    #[test]
    fn configサンプルは有効な全項目設定である() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config/schronu.sample.yaml");

        let actual = load_schronu_config(Some(path.clone().into_os_string())).unwrap();

        assert_eq!(actual.obsidian_vault_name, "Obsidian-Moica");
        assert_eq!(
            actual.busy_time_slots_yaml_path,
            path.parent().unwrap().join("busy_time_slots.yaml")
        );
        assert_eq!(actual.end_of_day_offset_minutes, -120);
        assert_eq!(actual.calendar_blank_line_weekday, Weekday::Mon);
        assert_eq!(
            actual.extrude_skip_weekdays,
            vec![Weekday::Sat, Weekday::Sun]
        );
        assert_eq!(
            actual.default_deadline_time,
            NaiveTime::from_hms_opt(19, 0, 0).unwrap()
        );
    }

    #[test]
    fn config未知キーはerrorにする() {
        let directory = test_directory();
        let path = write_config(&directory, "unknown: value\n");

        let error = load_schronu_config(Some(path.into_os_string())).unwrap_err();

        assert!(error.contains("unknown"));
    }

    #[test]
    fn config不正値と重複曜日はerrorにする() {
        for contents in [
            "calendar_blank_line_weekday: Monday\n",
            "end_of_day_duration: '00:30'\n",
            "end_of_day_offset_minutes: '30'\n",
            "end_of_day_offset_minutes: -1080\n",
            "end_of_day_offset_minutes: 1440\n",
            "default_deadline_time: '25:00'\n",
            "extrude_skip_weekdays: [Sat, Sat]\n",
            "extrude_skip_weekdays: [Mon, Tue, Wed, Thu, Fri, Sat, Sun]\n",
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
use chrono::{NaiveTime, Weekday};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use yaml_rust::{Yaml, YamlLoader};

const DEFAULT_OBSIDIAN_VAULT_NAME: &str = "Obsidian-Moica";
const DEFAULT_BUSY_TIME_SLOTS_YAML_PATH: &str = "../Schronu-private/busy_time_slots.yaml";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchronuConfig {
    pub obsidian_vault_name: String,
    pub busy_time_slots_yaml_path: PathBuf,
    pub end_of_day_offset_minutes: i64,
    pub calendar_blank_line_weekday: Weekday,
    pub extrude_skip_weekdays: Vec<Weekday>,
    pub default_deadline_time: NaiveTime,
}

impl Default for SchronuConfig {
    fn default() -> Self {
        Self {
            obsidian_vault_name: DEFAULT_OBSIDIAN_VAULT_NAME.to_string(),
            busy_time_slots_yaml_path: PathBuf::from(DEFAULT_BUSY_TIME_SLOTS_YAML_PATH),
            end_of_day_offset_minutes: 30,
            calendar_blank_line_weekday: Weekday::Mon,
            extrude_skip_weekdays: vec![],
            default_deadline_time: NaiveTime::from_hms_opt(23, 59, 59).unwrap(),
        }
    }
}

pub fn load_schronu_config(configured_path: Option<OsString>) -> Result<SchronuConfig, String> {
    let Some(path) = configured_path else {
        return Ok(SchronuConfig::default());
    };
    let path = PathBuf::from(path);
    let path_text = path
        .to_str()
        .ok_or_else(|| "config path must be valid UTF-8".to_string())?;
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read config {path_text}: {error}"))?;
    parse_schronu_config(&contents, path.parent().unwrap_or_else(|| Path::new(".")))
}

fn parse_schronu_config(contents: &str, config_directory: &Path) -> Result<SchronuConfig, String> {
    let documents = YamlLoader::load_from_str(contents)
        .map_err(|error| format!("failed to parse config YAML: {error}"))?;
    if documents.len() != 1 {
        return Err("config YAML must contain exactly one document".to_string());
    }
    let yaml = &documents[0];
    let mapping = yaml
        .as_hash()
        .ok_or_else(|| "config YAML root must be a mapping".to_string())?;
    let known_keys = [
        "obsidian_vault_name",
        "busy_time_slots_yaml_path",
        "end_of_day_offset_minutes",
        "calendar_blank_line_weekday",
        "extrude_skip_weekdays",
        "default_deadline_time",
    ];
    for key in mapping.keys() {
        let key = key
            .as_str()
            .ok_or_else(|| "config keys must be strings".to_string())?;
        if !known_keys.contains(&key) {
            return Err(format!("unknown config key: {key}"));
        }
    }

    let mut config = SchronuConfig::default();
    if let Some(value) = optional_string(yaml, "obsidian_vault_name")? {
        if value.is_empty() {
            return Err("obsidian_vault_name must not be empty".to_string());
        }
        config.obsidian_vault_name = value.to_string();
    }
    if let Some(value) = optional_string(yaml, "busy_time_slots_yaml_path")? {
        if value.is_empty() {
            return Err("busy_time_slots_yaml_path must not be empty".to_string());
        }
        let path = PathBuf::from(value);
        config.busy_time_slots_yaml_path = if path.is_absolute() {
            path
        } else {
            config_directory.join(path)
        };
    }
    if !matches!(yaml["end_of_day_offset_minutes"], Yaml::BadValue) {
        config.end_of_day_offset_minutes = yaml["end_of_day_offset_minutes"]
            .as_i64()
            .ok_or_else(|| "end_of_day_offset_minutes must be an integer".to_string())?;
        if !(-1079..=1439).contains(&config.end_of_day_offset_minutes) {
            return Err("end_of_day_offset_minutes must be between -1079 and 1439".to_string());
        }
    }
    if let Some(value) = optional_string(yaml, "calendar_blank_line_weekday")? {
        config.calendar_blank_line_weekday = parse_weekday(value)?;
    }
    if let Some(value) = yaml["extrude_skip_weekdays"].as_vec() {
        let mut weekdays = Vec::with_capacity(value.len());
        let mut seen = HashSet::new();
        for weekday in value {
            let weekday = weekday
                .as_str()
                .ok_or_else(|| "extrude_skip_weekdays must contain weekday strings".to_string())?;
            let weekday = parse_weekday(weekday)?;
            if !seen.insert(weekday) {
                return Err(format!(
                    "extrude_skip_weekdays contains duplicate weekday: {weekday:?}"
                ));
            }
            weekdays.push(weekday);
        }
        config.extrude_skip_weekdays = weekdays;
    } else if !matches!(yaml["extrude_skip_weekdays"], Yaml::BadValue) {
        return Err("extrude_skip_weekdays must be an array".to_string());
    }
    if config.extrude_skip_weekdays.len() == 7 {
        return Err("extrude_skip_weekdays must leave at least one weekday".to_string());
    }
    if let Some(value) = optional_string(yaml, "default_deadline_time")? {
        config.default_deadline_time = parse_deadline_time(value)?;
    }
    Ok(config)
}

fn optional_string<'a>(yaml: &'a Yaml, key: &str) -> Result<Option<&'a str>, String> {
    match &yaml[key] {
        Yaml::BadValue => Ok(None),
        value => value
            .as_str()
            .map(Some)
            .ok_or_else(|| format!("{key} must be a string")),
    }
}

fn parse_deadline_time(value: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(value, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(value, "%H:%M"))
        .map_err(|_| "default_deadline_time must use HH:MM or HH:MM:SS".to_string())
}

fn parse_weekday(value: &str) -> Result<Weekday, String> {
    match value {
        "Mon" => Ok(Weekday::Mon),
        "Tue" => Ok(Weekday::Tue),
        "Wed" => Ok(Weekday::Wed),
        "Thu" => Ok(Weekday::Thu),
        "Fri" => Ok(Weekday::Fri),
        "Sat" => Ok(Weekday::Sat),
        "Sun" => Ok(Weekday::Sun),
        _ => Err(format!("invalid weekday: {value}")),
    }
}
