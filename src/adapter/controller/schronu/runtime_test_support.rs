#[cfg(test)]
use super::handler::{decide_finish_time_values, decide_time_values, pack_display};

#[cfg(test)]
use super::interactive::{
    backward_width, get_byte_offset_for_deletion, get_byte_offset_for_insert, get_forward_width,
    get_width_for_rerender, idle_refresh_deadline, idle_wait_duration,
};

#[cfg(test)]
use chrono::{FixedOffset, TimeZone, Timelike};

#[cfg(test)]
use schronu::application::interface::{
    BusyTimeSlotRegistrationError, ProjectRegistrationError, RepositoryReloadOutcome,
    TaskRepositoryOperation,
};

#[cfg(test)]
use std::cell::{Cell, RefCell};

#[cfg(test)]
use std::collections::VecDeque;

#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
use std::rc::Rc;

#[cfg(test)]
use std::time::Instant;

#[cfg(test)]
const DEFAULT_LOWEST_PRIORITY_RECENT_DAYS: i64 = 0;

#[cfg(test)]
trait TaskHandleTestExt {
    fn create_as_last_child(&self, task_attr: TaskAttr) -> TaskHandle;
}

#[cfg(test)]
impl TaskHandleTestExt for TaskHandle {
    fn create_as_last_child(&self, task_attr: TaskAttr) -> TaskHandle {
        self.create_child(task_attr)
            .expect("test hierarchy child creation must succeed")
    }
}

#[cfg(test)]
fn next_test_task_id() -> Uuid {
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    Uuid::from_u128(u128::from(SEQUENCE.fetch_add(1, Ordering::Relaxed)))
}

#[cfg(test)]
fn test_task_time() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 8, 19, 0, 0, 0).unwrap()
}

#[cfg(test)]
fn maximum_local_datetime() -> DateTime<Local> {
    DateTime::<Local>::from_naive_utc_and_offset(
        NaiveDate::MAX.and_hms_opt(12, 0, 0).unwrap(),
        FixedOffset::east_opt(0).unwrap(),
    )
}

#[cfg(test)]
fn new_test_task_attr(name: &str) -> TaskAttr {
    TaskAttr::with_identity(name, next_test_task_id(), test_task_time())
}

#[cfg(test)]
fn new_test_task_handle(name: &str) -> Result<TaskHandle, TaskTreeError> {
    TaskHandle::with_identity(name, next_test_task_id(), test_task_time())
}

#[cfg(test)]
fn complete_task_tree_snapshot(root: &TaskHandle) -> Vec<String> {
    fn append(task: &TaskHandle, path: &str, rows: &mut Vec<String>) {
        let attr = task.get_attr().unwrap();
        let children = task.get_children().unwrap();
        rows.push(format!(
            "{path}|id={:?}|name={:?}|orig_status={:?}|status={:?}|other_side={:?}|atomic={:?}|fixed_start={:?}|pending_until={:?}|last_synced={:?}|priority={:?}|create={:?}|start={:?}|end={:?}|deadline={:?}|estimated={:?}|actual={:?}|repetition_interval={:?}|repetition_anchor={:?}|days_in_advance={:?}|category={:?}|children={}",
            attr.get_id(),
            attr.get_name(),
            attr.get_orig_status(),
            attr.get_status(),
            attr.get_is_on_other_side(),
            attr.get_atomic(),
            attr.get_fixed_start(),
            attr.get_pending_until(),
            attr.get_last_synced_time(),
            attr.get_priority(),
            attr.get_create_time(),
            attr.get_start_time(),
            attr.get_end_time_opt(),
            attr.get_deadline_time_opt(),
            attr.get_estimated_work_seconds(),
            attr.get_actual_work_seconds(),
            attr.get_repetition_interval_days_opt(),
            attr.get_repetition_anchor(),
            attr.get_days_in_advance(),
            attr.get_project_category_opt(),
            children.len(),
        ));
        for (index, child) in children.iter().enumerate() {
            append(child, &format!("{path}.{index}"), rows);
        }
    }

    let mut rows = Vec::new();
    append(root, "root", &mut rows);
    rows
}

#[cfg(test)]
fn report_command_result(stdout: &mut dyn SchronuWriter, result: Result<(), CommandError>) {
    if let Err(error) = result {
        let _output_error = render_display_model(stdout, &error_display_model(&error))
            .map_err(CommandError::Output);
    }
}

#[cfg(test)]
fn focus_selection_mode_from_command(command: &Command) -> Option<FocusSelectionMode> {
    super::handler::handle(command)
        .and_then(|outcome| match outcome.focus_change {
            FocusChange::SelectionMode(selection) => {
                Some(focus_selection_mode_from_selection(selection))
            }
            FocusChange::Keep | FocusChange::Clear | FocusChange::Set(_) => None,
        })
}

#[cfg(test)]
fn make_obsidian_search_url(query: &str) -> String {
    make_obsidian_search_url_with_vault(query, &active_config().obsidian_vault_name)
}

#[cfg(test)]
fn make_obsidian_root_task_search_url(focused_task: &TaskHandle) -> String {
    make_obsidian_root_task_search_url_with_vault(
        focused_task,
        &active_config().obsidian_vault_name,
    )
    .expect("fixture root task must be readable")
}

#[cfg(test)]
fn execute_set_priority(
    focused_task_opt: &Option<TaskHandle>,
    priority_str: &str,
) -> Result<(), ApplicationError> {
    if let Ok(priority) = priority_str.parse::<i64>() {
        set_focused_task_priority(focused_task_opt, priority)?;
    }
    Ok(())
}

#[cfg(test)]
fn decide_time(tokens: &[&str], now: &DateTime<Local>) -> Option<DateTime<Local>> {
    let values = tokens
        .iter()
        .skip(1)
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    decide_time_values(&values, now).expect("test datetime input must resolve")
}

#[cfg(test)]
fn decide_finish_time(tokens: &Vec<&str>, now: &DateTime<Local>) -> Option<DateTime<Local>> {
    let values = tokens
        .iter()
        .skip(1)
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
    decide_finish_time_values(&values, now).expect("test finish datetime input must resolve")
}

#[cfg(test)]
struct TestWriter {
    buffer: Vec<u8>,
    supports_ansi_color: bool,
    newline_prefix: &'static str,
}

#[cfg(test)]
struct TestStorageDir {
    path: PathBuf,
}

#[cfg(test)]
impl TestStorageDir {
    fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(format!("schronu-controller-{}", Uuid::new_v4())),
        }
    }
}

#[cfg(test)]
impl Drop for TestStorageDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
fn seed_clean_task_revision_observer(
    storage_directory: &std::path::Path,
    task: &TaskHandle,
    now: DateTime<Local>,
) -> TaskRepository {
    let mut observer = TaskRepository::new(storage_directory.to_str().unwrap());
    observer.sync_clock(now).unwrap();
    observer.start_new_project(task.clone()).unwrap();
    observer.save().unwrap();
    assert!(!observer.has_pending_changes().unwrap());

    let original_priority = task.get_priority().unwrap();
    let probe_priority = if original_priority == i64::MAX {
        original_priority - 1
    } else {
        original_priority + 1
    };
    task.set_priority(probe_priority).unwrap();
    task.set_priority(original_priority).unwrap();
    assert!(observer.has_pending_changes().unwrap());
    observer.save().unwrap();
    assert!(!observer.has_pending_changes().unwrap());
    observer
}

#[cfg(test)]
impl TestWriter {
    fn new() -> Self {
        Self {
            buffer: vec![],
            supports_ansi_color: true,
            newline_prefix: "",
        }
    }

    fn new_for_pipe() -> Self {
        Self {
            buffer: vec![],
            supports_ansi_color: false,
            newline_prefix: "",
        }
    }

    fn new_with_newline_prefix(newline_prefix: &'static str) -> Self {
        Self {
            buffer: vec![],
            supports_ansi_color: true,
            newline_prefix,
        }
    }

    fn into_string(self) -> String {
        String::from_utf8(self.buffer).expect("test output must be UTF-8")
    }
}

#[cfg(test)]
impl Write for TestWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
impl SchronuWriter for TestWriter {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        let newline_prefix = self.newline_prefix;
        writeln!(self, "{newline_prefix}{message}")
    }

    fn supports_ansi_color(&self) -> bool {
        self.supports_ansi_color
    }
}

#[cfg(test)]
struct ScriptedInteractiveInput {
    inputs: VecDeque<interactive::ReceivedInput>,
}

#[cfg(test)]
impl ScriptedInteractiveInput {
    fn command(command: &str) -> Self {
        let mut inputs = command
            .chars()
            .map(|character| interactive::ReceivedInput::Key(termion::event::Key::Char(character)))
            .collect::<VecDeque<_>>();
        inputs.push_back(interactive::ReceivedInput::Key(termion::event::Key::Char(
            '\n',
        )));
        Self { inputs }
    }

    fn empty() -> Self {
        Self {
            inputs: VecDeque::new(),
        }
    }
}

#[cfg(test)]
impl interactive::InputSource for ScriptedInteractiveInput {
    fn receive(&mut self, _wait_duration: StdDuration) -> interactive::ReceivedInput {
        self.inputs
            .pop_front()
            .unwrap_or(interactive::ReceivedInput::Disconnected)
    }
}

#[cfg(test)]
struct SignaledFailureWriter {
    fail_output: Rc<Cell<bool>>,
    error_kind: std::io::ErrorKind,
}

#[cfg(test)]
impl Write for SignaledFailureWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if self.fail_output.get() {
            Err(std::io::Error::new(
                self.error_kind,
                "test interactive output failure",
            ))
        } else {
            Ok(buffer.len())
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.fail_output.get() {
            Err(std::io::Error::new(
                self.error_kind,
                "test interactive output failure",
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
impl SchronuWriter for SignaledFailureWriter {
    fn writeln_newline(&mut self, message: &str) -> std::io::Result<()> {
        writeln!(self, "{message}")
    }

    fn supports_ansi_color(&self) -> bool {
        false
    }
}

#[cfg(test)]
struct SignaledFailureTerminalFactory {
    fail_output: Rc<Cell<bool>>,
    error_kind: std::io::ErrorKind,
}

#[cfg(test)]
impl interactive::TerminalFactory for SignaledFailureTerminalFactory {
    fn open_terminal(&mut self) -> std::io::Result<Box<dyn SchronuWriter>> {
        Ok(Box::new(SignaledFailureWriter {
            fail_output: Rc::clone(&self.fail_output),
            error_kind: self.error_kind,
        }))
    }
}

#[cfg(test)]
struct SharedSignaledFailureWriter {
    fail_output: Rc<Cell<bool>>,
    output: Rc<RefCell<Vec<u8>>>,
    drop_count: Rc<Cell<usize>>,
    error_kind: std::io::ErrorKind,
    fail_after_output_marker: Option<String>,
}

#[cfg(test)]
impl Write for SharedSignaledFailureWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if self.fail_output.get() {
            Err(std::io::Error::new(
                self.error_kind,
                "test interactive output failure",
            ))
        } else {
            self.output.borrow_mut().extend_from_slice(buffer);
            if let Some(marker) = &self.fail_after_output_marker {
                let output = self.output.borrow();
                if output
                    .windows(marker.len())
                    .any(|window| window == marker.as_bytes())
                {
                    self.fail_output.set(true);
                }
            }
            Ok(buffer.len())
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.fail_output.get() {
            Err(std::io::Error::new(
                self.error_kind,
                "test interactive output failure",
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
impl SchronuWriter for SharedSignaledFailureWriter {
    fn writeln_newline(&mut self, message: &str) -> std::io::Result<()> {
        writeln!(self, "{message}")
    }

    fn supports_ansi_color(&self) -> bool {
        false
    }
}

#[cfg(test)]
impl Drop for SharedSignaledFailureWriter {
    fn drop(&mut self) {
        self.drop_count.set(self.drop_count.get() + 1);
    }
}

#[cfg(test)]
struct SharedSignaledFailureTerminalFactory {
    fail_output: Rc<Cell<bool>>,
    output: Rc<RefCell<Vec<u8>>>,
    drop_count: Rc<Cell<usize>>,
    error_kind: std::io::ErrorKind,
    fail_after_output_marker: Option<String>,
}

#[cfg(test)]
impl interactive::TerminalFactory for SharedSignaledFailureTerminalFactory {
    fn open_terminal(&mut self) -> std::io::Result<Box<dyn SchronuWriter>> {
        Ok(Box::new(SharedSignaledFailureWriter {
            fail_output: Rc::clone(&self.fail_output),
            output: Rc::clone(&self.output),
            drop_count: Rc::clone(&self.drop_count),
            error_kind: self.error_kind,
            fail_after_output_marker: self.fail_after_output_marker.clone(),
        }))
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn run_interactive_runtime_for_test(
    initial_now: DateTime<Local>,
    terminal_factory: &mut dyn interactive::TerminalFactory,
    input: &mut dyn interactive::InputSource,
    repository: &mut TestTaskRepository,
    free_time_manager: &mut TestFreeTimeManager,
    focused_task_id_opt: &mut Option<Uuid>,
    last_focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &mut DateTime<Local>,
    focus_selection_mode: &mut FocusSelectionMode,
) -> Result<(), interactive::DriverRunError<RunError>> {
    interactive::run_with_terminal_factory(
        initial_now,
        terminal_factory,
        input,
        |stdout, event| {
            handle_interactive_driver_event(
                stdout,
                InteractiveDriverState {
                    task_repository: repository,
                    free_time_manager,
                    focused_task_id_opt,
                    last_focused_task_id_opt,
                    focus_started_datetime,
                    focus_selection_mode,
                },
                event,
            )
        },
    )
}

#[cfg(test)]
struct RawModeFailureTerminalFactory;

#[cfg(test)]
impl interactive::TerminalFactory for RawModeFailureTerminalFactory {
    fn open_terminal(&mut self) -> std::io::Result<Box<dyn SchronuWriter>> {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "test raw mode failure",
        ))
    }
}

#[cfg(test)]
struct FailingNewlineWriter {
    buffer: Vec<u8>,
    failures_remaining: usize,
    newline_call_count: usize,
    error_kind: std::io::ErrorKind,
}

#[cfg(test)]
impl FailingNewlineWriter {
    fn fail_once() -> Self {
        Self {
            buffer: vec![],
            failures_remaining: 1,
            newline_call_count: 0,
            error_kind: std::io::ErrorKind::Other,
        }
    }

    fn always_failing(error_kind: std::io::ErrorKind) -> Self {
        Self {
            buffer: vec![],
            failures_remaining: usize::MAX,
            newline_call_count: 0,
            error_kind,
        }
    }
}

#[cfg(test)]
impl Write for FailingNewlineWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
impl SchronuWriter for FailingNewlineWriter {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        self.newline_call_count += 1;
        if self.failures_remaining > 0 {
            self.failures_remaining -= 1;
            return Err(std::io::Error::new(
                self.error_kind,
                "newline write failure",
            ));
        }
        writeln!(self, "<reset>{message}")
    }
}

#[cfg(test)]
struct FlushTrackingWriter {
    buffer: Vec<u8>,
    flush_count: usize,
    flush_buffer_lengths: Vec<usize>,
    flush_error: Option<(usize, std::io::ErrorKind)>,
    supports_ansi_color: bool,
}

#[cfg(test)]
impl FlushTrackingWriter {
    fn successful(supports_ansi_color: bool) -> Self {
        Self {
            buffer: vec![],
            flush_count: 0,
            flush_buffer_lengths: vec![],
            flush_error: None,
            supports_ansi_color,
        }
    }

    fn failing(error_kind: std::io::ErrorKind) -> Self {
        Self {
            buffer: vec![],
            flush_count: 0,
            flush_buffer_lengths: vec![],
            flush_error: Some((1, error_kind)),
            supports_ansi_color: true,
        }
    }

    fn failing_on_nth_flush(flush_count: usize, error_kind: std::io::ErrorKind) -> Self {
        Self {
            buffer: vec![],
            flush_count: 0,
            flush_buffer_lengths: vec![],
            flush_error: Some((flush_count, error_kind)),
            supports_ansi_color: true,
        }
    }
}

#[cfg(test)]
impl Write for FlushTrackingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flush_count += 1;
        self.flush_buffer_lengths.push(self.buffer.len());
        match self.flush_error {
            Some((failure_count, kind)) if self.flush_count == failure_count => {
                Err(std::io::Error::new(kind, "flush failure"))
            }
            Some(_) | None => Ok(()),
        }
    }
}

#[cfg(test)]
impl SchronuWriter for FlushTrackingWriter {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        writeln!(self, "{message}")
    }

    fn supports_ansi_color(&self) -> bool {
        self.supports_ansi_color
    }
}

#[cfg(test)]
fn strip_ansi_escape_sequences(value: &str) -> String {
    Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]")
        .unwrap()
        .replace_all(value, "")
        .into_owned()
}

#[cfg(test)]
struct TestTaskRepository {
    task: TaskHandle,
    storage_directory: String,
    last_synced_time: DateTime<Local>,
    highest_priority_leaf_task_id_opt: Option<Uuid>,
    defer_candidate_leaf_task_id_opt: Option<Uuid>,
    last_defer_candidate_recent_threshold_opt: Option<DateTime<Local>>,
    load_should_fail: bool,
    load_attempt_count: Cell<usize>,
    reload_if_changed_attempt_count: Cell<usize>,
    get_by_id_attempt_count: Cell<usize>,
    save_failures_remaining: Cell<usize>,
    save_attempt_count: Cell<usize>,
    save_attempt_signal_opt: Option<Rc<Cell<bool>>>,
    has_pending_changes: Cell<bool>,
    operation_trace: RefCell<Vec<&'static str>>,
}

#[cfg(test)]
struct CommandTestResult {
    task: TaskHandle,
    focused_task_id_opt: Option<Uuid>,
    output: String,
}

#[cfg(test)]
fn execute_command_for_test(
    task: TaskHandle,
    now: DateTime<Local>,
    focused_task_id_opt: Option<Uuid>,
    command: &str,
) -> CommandTestResult {
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = focused_task_id_opt;
    let mut stdout = TestWriter::new();

    if let Err(error) = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        command,
    ) {
        let _output_error = render_display_model(&mut stdout, &error_display_model(&error))
            .map_err(CommandError::Output);
    }

    CommandTestResult {
        task: task_repository.task,
        focused_task_id_opt,
        output: stdout.into_string(),
    }
}

#[cfg(test)]
impl TestTaskRepository {
    fn new(task: TaskHandle, last_synced_time: DateTime<Local>) -> Self {
        let task_id = task.get_id().unwrap();
        Self {
            task,
            storage_directory: String::new(),
            last_synced_time,
            highest_priority_leaf_task_id_opt: Some(task_id),
            defer_candidate_leaf_task_id_opt: Some(task_id),
            last_defer_candidate_recent_threshold_opt: None,
            load_should_fail: false,
            load_attempt_count: Cell::new(0),
            reload_if_changed_attempt_count: Cell::new(0),
            get_by_id_attempt_count: Cell::new(0),
            save_failures_remaining: Cell::new(0),
            save_attempt_count: Cell::new(0),
            save_attempt_signal_opt: None,
            has_pending_changes: Cell::new(true),
            operation_trace: RefCell::new(Vec::new()),
        }
    }

    fn with_storage_directory(mut self, storage_directory: &std::path::Path) -> Self {
        self.storage_directory = storage_directory.to_str().unwrap().to_string();
        self
    }

    fn with_save_attempt_signal(mut self, signal: Rc<Cell<bool>>) -> Self {
        self.save_attempt_signal_opt = Some(signal);
        self
    }

    fn with_pending_changes(self, has_pending_changes: bool) -> Self {
        self.has_pending_changes.set(has_pending_changes);
        self
    }

    fn operation_trace(&self) -> Vec<&'static str> {
        self.operation_trace.borrow().clone()
    }
}

#[cfg(test)]
impl TaskRepositoryTrait for TestTaskRepository {
    fn get_project_storage_dir_name(&self) -> &str {
        &self.storage_directory
    }

    fn get_all_projects(&self) -> Vec<&TaskHandle> {
        vec![&self.task]
    }

    fn load(&mut self) -> Result<(), schronu::application::interface::TaskRepositoryError> {
        self.operation_trace.borrow_mut().push("load");
        self.load_attempt_count
            .set(self.load_attempt_count.get() + 1);
        if self.load_should_fail {
            Err(TaskRepositoryError::new(
                TaskRepositoryOperation::Load,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "ParseProject failed for /test/project.yaml: test load failure",
                ),
            ))
        } else {
            Ok(())
        }
    }

    fn reload_if_changed(
        &mut self,
        now: DateTime<Local>,
    ) -> Result<RepositoryReloadOutcome, TaskRepositoryError> {
        self.operation_trace
            .borrow_mut()
            .push("reload_if_changed");
        self.reload_if_changed_attempt_count
            .set(self.reload_if_changed_attempt_count.get() + 1);
        self.sync_clock(now)
            .map_err(|error| TaskRepositoryError::new(TaskRepositoryOperation::Load, error))?;
        self.load()?;
        Ok(RepositoryReloadOutcome::Reloaded)
    }

    fn save(&self) -> Result<(), schronu::application::interface::TaskRepositoryError> {
        self.operation_trace.borrow_mut().push("save");
        self.save_attempt_count
            .set(self.save_attempt_count.get() + 1);
        if let Some(signal) = &self.save_attempt_signal_opt {
            signal.set(true);
        }
        let failures_remaining = self.save_failures_remaining.get();
        if failures_remaining > 0 {
            self.save_failures_remaining.set(failures_remaining - 1);
            Err(TaskRepositoryError::new(
                TaskRepositoryOperation::Save,
                std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "WriteFile failed for /test/project.yaml: test save failure",
                ),
            ))
        } else {
            Ok(())
        }
    }

    fn has_pending_changes(&self) -> Result<bool, TaskTreeError> {
        self.operation_trace
            .borrow_mut()
            .push("has_pending_changes");
        Ok(self.has_pending_changes.get())
    }

    fn sync_clock(&mut self, now: DateTime<Local>) -> Result<(), TaskTreeError> {
        self.last_synced_time = now;
        Ok(())
    }

    fn get_last_synced_time(&self) -> DateTime<Local> {
        self.last_synced_time
    }

    fn get_highest_priority_project(&mut self) -> Option<&TaskHandle> {
        Some(&self.task)
    }

    fn get_highest_priority_leaf_task_id(
        &mut self,
        excluded_task_ids: &[Uuid],
    ) -> Result<Option<Uuid>, TaskTreeError> {
        if self
            .highest_priority_leaf_task_id_opt
            .is_some_and(|id| excluded_task_ids.contains(&id))
        {
            return Ok(None);
        }
        Ok(self.highest_priority_leaf_task_id_opt)
    }

    fn get_defer_candidate_leaf_task_id(
        &mut self,
        recent_threshold: DateTime<Local>,
        excluded_task_ids: &[Uuid],
    ) -> Result<Option<Uuid>, TaskTreeError> {
        self.last_defer_candidate_recent_threshold_opt = Some(recent_threshold);
        if self
            .defer_candidate_leaf_task_id_opt
            .is_some_and(|id| excluded_task_ids.contains(&id))
        {
            return Ok(None);
        }
        Ok(self.defer_candidate_leaf_task_id_opt)
    }

    fn get_by_id(&self, id: Uuid) -> Result<Option<TaskHandle>, TaskTreeError> {
        self.get_by_id_attempt_count
            .set(self.get_by_id_attempt_count.get() + 1);
        self.task.get_by_id(id)
    }

    fn start_new_project(
        &mut self,
        root_task: TaskHandle,
    ) -> Result<(), ProjectRegistrationError> {
        self.task = root_task;
        Ok(())
    }
}

#[cfg(test)]
#[derive(Default)]
struct TestFreeTimeManager {
    free_minutes: i64,
}

#[cfg(test)]
impl TestFreeTimeManager {
    fn with_free_minutes(free_minutes: i64) -> Self {
        Self { free_minutes }
    }
}

#[cfg(test)]
trait FixtureTaskOptionExt {
    fn get_pending_until(&self) -> Result<DateTime<Local>, TaskTreeError>;
    fn get_estimated_work_seconds(&self) -> Result<i64, TaskTreeError>;
    fn get_name(&self) -> Result<String, TaskTreeError>;
    fn set_estimated_work_seconds(&self, estimated_work_seconds: i64) -> Result<(), TaskTreeError>;
}

#[cfg(test)]
impl FixtureTaskOptionExt for Option<TaskHandle> {
    fn get_pending_until(&self) -> Result<DateTime<Local>, TaskTreeError> {
        self.as_ref()
            .ok_or(TaskTreeError::MissingDummyRootChild)?
            .get_pending_until()
    }

    fn get_estimated_work_seconds(&self) -> Result<i64, TaskTreeError> {
        self.as_ref()
            .ok_or(TaskTreeError::MissingDummyRootChild)?
            .get_estimated_work_seconds()
    }

    fn get_name(&self) -> Result<String, TaskTreeError> {
        self.as_ref()
            .ok_or(TaskTreeError::MissingDummyRootChild)?
            .get_name()
    }

    fn set_estimated_work_seconds(&self, estimated_work_seconds: i64) -> Result<(), TaskTreeError> {
        self.as_ref()
            .ok_or(TaskTreeError::MissingDummyRootChild)?
            .set_estimated_work_seconds(estimated_work_seconds)
    }
}

#[cfg(test)]
impl FreeTimeManagerTrait for TestFreeTimeManager {
    fn get_free_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        self.free_minutes
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

#[cfg(test)]
#[derive(Default)]
struct TestFreeTimeManagerWithLoadError {
    loaded_path: RefCell<Option<PathBuf>>,
}

#[cfg(test)]
impl TestFreeTimeManagerWithLoadError {
    fn loaded_path(&self) -> Option<PathBuf> {
        self.loaded_path.borrow().clone()
    }
}

#[cfg(test)]
impl FreeTimeManagerTrait for TestFreeTimeManagerWithLoadError {
    fn get_free_minutes(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        0
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
        busy_time_slots_file_path: &str,
    ) -> Result<(), BusyTimeSlotLoadError> {
        let path = PathBuf::from(busy_time_slots_file_path);
        self.loaded_path.replace(Some(path.clone()));
        Err(BusyTimeSlotLoadError::new(
            path,
            "$",
            None,
            std::io::Error::new(std::io::ErrorKind::InvalidData, "test load error"),
        ))
    }
}

#[cfg(test)]
struct TestFreeTimeManagerForBand;

#[cfg(test)]
impl FreeTimeManagerTrait for TestFreeTimeManagerForBand {
    fn get_free_minutes(&mut self, start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        if start.hour() == 6 {
            990
        } else {
            190
        }
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

#[cfg(test)]
struct TestFreeTimeManagerByDate {
    free_minutes_by_date: HashMap<NaiveDate, i64>,
}

#[cfg(test)]
impl FreeTimeManagerTrait for TestFreeTimeManagerByDate {
    fn get_free_minutes(&mut self, start: &DateTime<Local>, _end: &DateTime<Local>) -> i64 {
        self.free_minutes_by_date
            .get(&start.date_naive())
            .copied()
            .unwrap_or(0)
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

#[cfg(test)]
fn execute_sequential_command(command: &str) -> (TaskHandle, Option<Uuid>) {
    let now = Local.with_ymd_and_hms(2026, 7, 26, 12, 0, 0).unwrap();
    let task = new_test_task_handle("親タスク").unwrap();
    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    let _ = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        command,
    );

    (task, focused_task_id_opt)
}

#[cfg(test)]
fn execute_arrange_command(command: &str) -> TaskHandle {
    let now = Local.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap();
    let task = new_test_task_handle("ルーチン").unwrap();
    let _ = task.set_repetition_interval_days_opt(Some(7));

    let mut estimated_child_attr = new_test_task_attr("見積もりあり");
    estimated_child_attr.set_estimated_work_seconds(5 * 60);
    task.create_as_last_child(estimated_child_attr);

    let mut zero_estimate_child_attr = new_test_task_attr("見積もり0");
    zero_estimate_child_attr.set_estimated_work_seconds(0);
    task.create_as_last_child(zero_estimate_child_attr);

    let mut done_child_attr = new_test_task_attr("完了済み");
    done_child_attr.set_estimated_work_seconds(10 * 60);
    done_child_attr.set_orig_status(Status::Done);
    task.create_as_last_child(done_child_attr);

    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    let _ = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        command,
    );

    task
}

#[cfg(test)]
fn assert_show_all_spreadsheet_formatter_contract() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("夕食  の 準備").unwrap();
    let _ = task.set_estimated_work_seconds(40 * 60);
    let _ = task.set_start_time(now);
    let _ = task.set_priority(1);
    let _ = task.set_project_category_opt(Some(ProjectCategory::Investment));
    let _ = task.sync_clock(now);
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "全");
    let task_row = result
        .output
        .lines()
        .find(|line| line.contains(&task_id.to_string()))
        .expect("ShowAll task row");

    assert_eq!(
        task_row,
        format!("0000 {task_id} A ____/__/__ 08/11(火)-12:00~12:40 0 40 01 資 夕食  の 準備")
    );
}

#[cfg(test)]
fn execute_pack(
    stdout: &mut dyn SchronuWriter,
    task_repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) {
    let result = pack_tasks_with_end_of_day_offset_minutes(
        task_repository,
        free_time_manager,
        active_config().end_of_day_offset_minutes,
    )
    .unwrap();
    render_display_model(stdout, &DisplayModel::Pack(pack_display(result))).unwrap();
}

#[cfg(test)]
fn execute(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &DateTime<Local>,
    untrimmed_line: &str,
) -> Result<(), CommandError> {
    let parsed_command = parse_command(untrimmed_line, ParseMode::NonInteractive)
        .map_err(map_command_parse_error)?;
    execute_parsed(
        stdout,
        task_repository,
        free_time_manager,
        focused_task_id_opt,
        focus_started_datetime,
        &parsed_command,
        OutcomeApplicationMode::Flushed,
    )
}

#[cfg(test)]
fn execute_show_all_command_for_test(
    command: &str,
    now: DateTime<Local>,
    task: TaskHandle,
) -> String {
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = None;
    let mut stdout = TestWriter::new();

    let _ = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        command,
    );

    String::from_utf8(stdout.buffer).unwrap()
}

#[cfg(test)]
fn execute_calendar_command_for_test(
    command: &str,
    now: DateTime<Local>,
    task: TaskHandle,
    free_minutes: i64,
) -> String {
    execute_calendar_command_with_ansi_color_for_test(command, now, task, free_minutes, true)
}

#[cfg(test)]
fn rendered_focus_messages_for_test(
    focused_task: &TaskHandle,
    focus_started_datetime: &DateTime<Local>,
    now: &DateTime<Local>,
) -> [String; 2] {
    let display = build_focus_timing_display(focused_task, focus_started_datetime, now).unwrap();
    let mut writer = TestWriter::new_for_pipe();
    render_display_model(&mut writer, &DisplayModel::Focus(display)).unwrap();
    let output = writer.into_string();
    let mut lines = output.lines().rev();
    let progress = lines.next().unwrap().to_string();
    let summary = lines.next().unwrap().to_string();
    [summary, progress]
}

#[cfg(test)]
fn execute_calendar_command_with_ansi_color_for_test(
    command: &str,
    now: DateTime<Local>,
    task: TaskHandle,
    free_minutes: i64,
    supports_ansi_color: bool,
) -> String {
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager::with_free_minutes(free_minutes);
    let mut focused_task_id_opt = None;
    let mut stdout = if supports_ansi_color {
        TestWriter::new()
    } else {
        TestWriter::new_for_pipe()
    };

    let _ = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        command,
    );

    String::from_utf8(stdout.buffer).unwrap()
}

#[cfg(test)]
fn execute_band_command_with_elapsed_for_test(
    command: &str,
    now: DateTime<Local>,
    task: TaskHandle,
) -> String {
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerForBand;
    let mut focused_task_id_opt = None;
    let mut stdout = TestWriter::new();

    let _ = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        command,
    );

    String::from_utf8(stdout.buffer).unwrap()
}

#[cfg(test)]
fn add_scheduled_child_for_test(
    root: &TaskHandle,
    name: &str,
    start_time: DateTime<Local>,
    estimated_work_minutes: i64,
) -> TaskHandle {
    let child = root.create_as_last_child(new_test_task_attr(name));
    let _ = child.set_estimated_work_seconds(estimated_work_minutes * 60);
    let _ = child.set_start_time(start_time);
    let _ = child.set_pending_until(start_time);
    let _ = child.set_orig_status(Status::Pending);
    child
}

#[cfg(test)]
fn execute_flatten_command_for_test(
    command: &str,
    now: DateTime<Local>,
    task: TaskHandle,
    free_minutes_by_date: HashMap<NaiveDate, i64>,
) -> CommandTestResult {
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerByDate {
        free_minutes_by_date,
    };
    let mut focused_task_id_opt = None;
    let mut stdout = TestWriter::new();

    let _ = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        command,
    );

    CommandTestResult {
        task: task_repository.task,
        focused_task_id_opt,
        output: stdout.into_string(),
    }
}

#[cfg(test)]
impl TaskListDisplayRow {
    // sort・give-up判定に必要な属性を呼び出し側で確定し、表示専用値は製品型で補う。
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn new_task(
        scheduled_start: DateTime<Local>,
        logical_naive_date: NaiveDate,
        priority_rank: usize,
        id: Uuid,
        priority: i64,
        work_seconds: i64,
        project_category_opt: Option<ProjectCategory>,
        task_name: String,
    ) -> Self {
        TaskListDisplayRow {
            scheduled_start,
            logical_naive_date_opt: Some(logical_naive_date),
            rank: priority_rank,
            id,
            priority,
            work_seconds,
            project_category_opt,
            is_real_task: true,
            give_up_candidate: false,
            display_row: super::renderer::TaskListRow::Task(
                super::renderer::TaskListTaskRow {
                    rank: 0,
                    task_id: id,
                    icon: "/".to_string(),
                    remaining_time: "____/__/__".to_string(),
                    scheduled_start,
                    scheduled_end: scheduled_start + Duration::seconds(work_seconds),
                    priority_rank,
                    estimated_minutes: schronu::entity::task::round_up_sec_as_minute(work_seconds),
                    project_number_priority: priority,
                    project_category: project_category_opt,
                    task_name,
                    give_up_candidate: false,
                },
            ),
        }
    }
}
