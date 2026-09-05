use crate::{
    web_error_codes, CompleteSessionResponse, ListTasksRequest, RecordSessionRequest,
    RecordSessionResult, RetryAdvice, ScheduledTaskRow, ServerSnapshot, SessionTask, WebError,
    WebOperations, WebSuccess, WebWorkerHandle,
};
use chrono::{DateTime, Local, NaiveDate};
use schronu::adapter::controller::{
    resolve_project_storage_directory, RecordSessionRequest as CoreRecordSessionRequest,
    ScheduledTaskRowDto, ServerSnapshot as CoreServerSnapshot, SessionTaskDto, WebService,
    WebSuccess as CoreWebSuccess,
};
use schronu::adapter::gateway::schronu_config::load_schronu_config;
use std::env;
use std::ffi::OsString;

trait Clock: 'static {
    fn now(&mut self) -> DateTime<Local>;
}

struct SystemClock;

impl Clock for SystemClock {
    fn now(&mut self) -> DateTime<Local> {
        Local::now()
    }
}

pub fn web_worker_from_environment() -> WebWorkerHandle {
    WebWorkerHandle::spawn(EnvironmentWebOperations::new)
}

struct EnvironmentWebOperations<C = SystemClock> {
    config_path: Option<OsString>,
    storage_directory: Option<OsString>,
    clock: C,
    service: Option<WebService>,
}

impl EnvironmentWebOperations<SystemClock> {
    fn new() -> Self {
        Self::with_environment_and_clock(
            env::var_os("SCHRONU_CONFIG_PATH"),
            env::var_os("SCHRONU_STORAGE_DIR"),
            SystemClock,
        )
    }
}

impl<C: Clock> EnvironmentWebOperations<C> {
    fn with_environment_and_clock(
        config_path: Option<OsString>,
        storage_directory: Option<OsString>,
        clock: C,
    ) -> Self {
        Self {
            config_path,
            storage_directory,
            clock,
            service: None,
        }
    }

    fn service(&mut self) -> Result<&mut WebService, WebError> {
        if self.service.is_none() {
            let config =
                load_schronu_config(self.config_path.clone()).map_err(|_| configuration_error())?;
            let directory = resolve_project_storage_directory(self.storage_directory.clone())
                .map_err(|_| configuration_error())?;
            self.service = Some(WebService::new(directory, config));
        }
        self.service.as_mut().ok_or_else(configuration_error)
    }
}

impl<C: Clock> WebOperations for EnvironmentWebOperations<C> {
    fn bootstrap(&mut self) -> Result<ServerSnapshot, WebError> {
        let operation_now = self.clock.now();
        self.service()?
            .bootstrap_at(operation_now)
            .map(Into::into)
            .map_err(Into::into)
    }

    fn list_tasks(
        &mut self,
        request: ListTasksRequest,
    ) -> Result<WebSuccess<Vec<ScheduledTaskRow>>, WebError> {
        let operation_now = self.clock.now();
        let logical_date = NaiveDate::parse_from_str(&request.logical_date, "%Y-%m-%d")
            .map_err(|_| invalid_input_error())?;
        self.service()?
            .list_tasks_at(operation_now, logical_date)
            .map(convert_success)
            .map_err(Into::into)
    }

    fn auto_session(&mut self) -> Result<WebSuccess<Option<SessionTask>>, WebError> {
        let operation_now = self.clock.now();
        self.service()?
            .auto_session_at(operation_now)
            .map(convert_success)
            .map_err(Into::into)
    }

    fn record_session(
        &mut self,
        request: RecordSessionRequest,
    ) -> Result<WebSuccess<RecordSessionResult>, WebError> {
        let operation_now = self.clock.now();
        self.service()?
            .record_session_at(operation_now, request.into())
            .map(convert_success)
            .map_err(Into::into)
    }

    fn complete_session(
        &mut self,
        request: RecordSessionRequest,
    ) -> Result<CompleteSessionResponse, WebError> {
        let operation_now = self.clock.now();
        self.service()?
            .complete_session_at(operation_now, request.into())
            .map(Into::into)
            .map_err(Into::into)
    }
}

impl From<CoreServerSnapshot> for ServerSnapshot {
    fn from(snapshot: CoreServerSnapshot) -> Self {
        Self {
            observed_at_epoch_ms: snapshot.observed_at_epoch_ms,
            logical_date: snapshot.logical_date,
            buffer_seconds: snapshot.buffer_seconds,
        }
    }
}

impl From<SessionTaskDto> for SessionTask {
    fn from(task: SessionTaskDto) -> Self {
        Self {
            task_id: task.task_id,
            task_name: task.task_name,
            estimated_work_seconds: task.estimated_work_seconds,
            actual_work_seconds: task.actual_work_seconds,
        }
    }
}

impl From<ScheduledTaskRowDto> for ScheduledTaskRow {
    fn from(row: ScheduledTaskRowDto) -> Self {
        Self {
            task: row.task.into(),
            schedule_start_epoch_ms: row.schedule_start_epoch_ms,
            schedule_end_epoch_ms: row.schedule_end_epoch_ms,
            deadline_epoch_ms: row.deadline_epoch_ms,
            is_leaf: row.is_leaf,
        }
    }
}

impl From<RecordSessionRequest> for CoreRecordSessionRequest {
    fn from(request: RecordSessionRequest) -> Self {
        Self {
            task_id: request.task_id,
            started_at_epoch_ms: request.started_at_epoch_ms,
            expected_actual_work_seconds: request.expected_actual_work_seconds,
        }
    }
}

trait ConvertData {
    type Output;
    fn convert(self) -> Self::Output;
}

impl ConvertData for Vec<ScheduledTaskRowDto> {
    type Output = Vec<ScheduledTaskRow>;

    fn convert(self) -> Self::Output {
        self.into_iter().map(Into::into).collect()
    }
}

impl ConvertData for Option<SessionTaskDto> {
    type Output = Option<SessionTask>;

    fn convert(self) -> Self::Output {
        self.map(Into::into)
    }
}

impl ConvertData for schronu::adapter::controller::RecordSessionResult {
    type Output = RecordSessionResult;

    fn convert(self) -> Self::Output {
        RecordSessionResult {
            actual_work_seconds: self.actual_work_seconds,
        }
    }
}

fn convert_success<T: ConvertData>(success: CoreWebSuccess<T>) -> WebSuccess<T::Output> {
    WebSuccess {
        snapshot: success.snapshot.into(),
        data: success.data.convert(),
    }
}

fn invalid_input_error() -> WebError {
    WebError {
        code: web_error_codes::INVALID_INPUT.to_owned(),
        message: "logical_dateはYYYY-MM-DD形式で指定してください。".to_owned(),
        retry_advice: RetryAdvice::ManualCheck,
    }
}

fn configuration_error() -> WebError {
    WebError {
        code: web_error_codes::CONFIGURATION_ERROR.to_owned(),
        message: "Schronuの設定を読み込めませんでした。".to_owned(),
        retry_advice: RetryAdvice::ManualCheck,
    }
}

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
            let mut busy_yaml = String::from("days_of_week:\n");
            for day in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
                busy_yaml.push_str(&format!(
                    "  - day_of_week: {day}\n    busy_time_slots: []\n"
                ));
            }
            fs::write(&busy, busy_yaml).unwrap();
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
