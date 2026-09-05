#[cfg(test)]
mod tests {
    use super::{Clock, EnvironmentWebOperations};
    use crate::{web_error_codes, ListTasksRequest, RecordSessionRequest, WebOperations};
    use chrono::{DateTime, Local, TimeZone};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn 五操作は現在時刻を各1回だけ取得して同じ値をserviceとwireへ渡す() {
        let fixture = Fixture::new();
        let now = Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 59).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut operations = EnvironmentWebOperations::with_environment_and_clock(
            Some(fixture.config.clone().into_os_string()),
            Some(fixture.storage.clone().into_os_string()),
            CountingClock {
                now,
                calls: Arc::clone(&calls),
            },
        );

        let bootstrap = operations.bootstrap().unwrap();
        assert_eq!(bootstrap.observed_at_epoch_ms, now.timestamp_millis());
        let listed = operations
            .list_tasks(ListTasksRequest {
                logical_date: "2026-09-05".to_owned(),
            })
            .unwrap();
        assert_eq!(listed.snapshot.observed_at_epoch_ms, now.timestamp_millis());
        let selected = operations.auto_session().unwrap();
        assert_eq!(
            selected.snapshot.observed_at_epoch_ms,
            now.timestamp_millis()
        );

        let invalid_request = RecordSessionRequest {
            task_id: "invalid".to_owned(),
            started_at_epoch_ms: now.timestamp_millis(),
            expected_actual_work_seconds: 0,
        };
        assert_eq!(
            operations
                .record_session(invalid_request.clone())
                .unwrap_err()
                .code,
            web_error_codes::INVALID_INPUT
        );
        assert_eq!(
            operations
                .complete_session(invalid_request)
                .unwrap_err()
                .code,
            web_error_codes::INVALID_INPUT
        );
        assert_eq!(calls.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn 初期化失敗をcacheせず設定修復後に同じinstanceで再試行できる() {
        let fixture = Fixture::new_without_config();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut operations = EnvironmentWebOperations::with_environment_and_clock(
            Some(fixture.config.clone().into_os_string()),
            Some(fixture.storage.clone().into_os_string()),
            CountingClock {
                now: Local.with_ymd_and_hms(2026, 9, 5, 19, 0, 59).unwrap(),
                calls: Arc::clone(&calls),
            },
        );

        assert_eq!(
            operations.bootstrap().unwrap_err().code,
            web_error_codes::CONFIGURATION_ERROR
        );
        fixture.write_config();
        assert!(operations.bootstrap().is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    struct CountingClock {
        now: DateTime<Local>,
        calls: Arc<AtomicUsize>,
    }

    impl Clock for CountingClock {
        fn now(&mut self) -> DateTime<Local> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.now
        }
    }

    struct Fixture {
        root: PathBuf,
        storage: PathBuf,
        config: PathBuf,
        busy: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let fixture = Self::new_without_config();
            fixture.write_config();
            fixture
        }

        fn new_without_config() -> Self {
            static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "schronu-web-environment-{}-{id}",
                std::process::id()
            ));
            let storage = root.join("storage");
            fs::create_dir_all(&storage).unwrap();
            let busy = root.join("busy.yaml");
            fs::write(&busy, "days_of_week: []\n").unwrap();
            let config = root.join("schronu.yaml");
            Self {
                root,
                storage,
                config,
                busy,
            }
        }

        fn write_config(&self) {
            fs::write(
                &self.config,
                format!(
                    "busy_time_slots_yaml_path: {}\nend_of_day_offset_minutes: 120\n",
                    self.busy.display()
                ),
            )
            .unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
