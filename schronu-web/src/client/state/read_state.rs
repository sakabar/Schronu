use super::*;
use crate::client::date_buttons::logical_date_buttons;
use crate::{ListTasksRequest, SessionTask, WebSuccess};
use std::ops::Range;

pub(super) struct ReadState {
    pub(super) snapshot: Option<ServerSnapshot>,
    pub(super) buffer_tracking_started_at_epoch_ms: Option<i64>,
    pub(super) committed_session_intervals: Vec<Range<i64>>,
    pub(super) date_buttons: Vec<LogicalDateButton>,
    pub(super) selected_logical_date: Option<String>,
    pub(super) scheduled_rows: Vec<ScheduledTaskRow>,
    pub(super) auto_session_empty: bool,
    pub(super) auto_session_in_flight: bool,
    pub(super) next_request_id: u64,
    pub(super) latest_bootstrap_request_id: Option<u64>,
    pub(super) latest_list_request_id: Option<u64>,
    pub(super) latest_auto_request_id: Option<u64>,
}

impl ReadState {
    pub(super) fn new() -> Self {
        Self {
            snapshot: None,
            buffer_tracking_started_at_epoch_ms: None,
            committed_session_intervals: Vec::new(),
            date_buttons: Vec::new(),
            selected_logical_date: None,
            scheduled_rows: Vec::new(),
            auto_session_empty: false,
            auto_session_in_flight: false,
            next_request_id: 1,
            latest_bootstrap_request_id: None,
            latest_list_request_id: None,
            latest_auto_request_id: None,
        }
    }
}

impl ClientState {
    pub fn request_bootstrap(&mut self) -> ClientEffect {
        let Some(request_id) = self.allocate_read_request_id() else {
            return ClientEffect::None;
        };
        self.read.latest_bootstrap_request_id = Some(request_id);
        ClientEffect::Bootstrap { request_id }
    }

    pub fn request_list(&mut self, logical_date: &str) -> ClientEffect {
        let Some(request_id) = self.allocate_read_request_id() else {
            return ClientEffect::None;
        };
        self.read.latest_list_request_id = Some(request_id);
        ClientEffect::ListTasks {
            request_id,
            request: ListTasksRequest {
                logical_date: logical_date.to_owned(),
            },
        }
    }

    pub fn request_auto_session(&mut self) -> ClientEffect {
        if self.read.auto_session_in_flight {
            return ClientEffect::None;
        }
        let Some(request_id) = self.allocate_read_request_id() else {
            return ClientEffect::None;
        };
        self.read.latest_auto_request_id = Some(request_id);
        self.read.auto_session_in_flight = true;
        ClientEffect::AutoSession { request_id }
    }

    pub fn apply_bootstrap_result(
        &mut self,
        request_id: u64,
        result: Result<ServerSnapshot, ServerFailure>,
    ) -> ClientEffect {
        if !consume_latest(&mut self.read.latest_bootstrap_request_id, request_id) {
            self.record_stale_response(Operation::Bootstrap, result.is_ok());
            return ClientEffect::None;
        }
        match result {
            Ok(snapshot) => {
                if self.apply_snapshot(snapshot).is_none() {
                    self.record_stale_response(Operation::Bootstrap, true);
                    return ClientEffect::None;
                }
                self.record_server(
                    Operation::Bootstrap,
                    None,
                    Outcome::Success,
                    "更新しました。",
                );
            }
            Err(error) => self.record_server_failure(Operation::Bootstrap, None, error),
        }
        ClientEffect::None
    }

    pub fn apply_list_result(
        &mut self,
        request_id: u64,
        requested_date: &str,
        result: Result<WebSuccess<Vec<ScheduledTaskRow>>, ServerFailure>,
    ) -> ClientEffect {
        if !consume_latest(&mut self.read.latest_list_request_id, request_id) {
            self.record_stale_response(Operation::ListTasks, result.is_ok());
            return ClientEffect::None;
        }
        match result {
            Ok(success) => {
                let same_logical_date =
                    self.read.snapshot.as_ref().is_some_and(|current| {
                        current.logical_date == success.snapshot.logical_date
                    });
                let snapshot_result = self.apply_snapshot(success.snapshot);
                if snapshot_result == Some(false)
                    || (snapshot_result.is_none() && same_logical_date)
                {
                    self.read.selected_logical_date = Some(requested_date.to_owned());
                    self.read.scheduled_rows = success.data;
                }
                if snapshot_result.is_none() {
                    self.record_stale_response(Operation::ListTasks, true);
                } else {
                    self.record_server(
                        Operation::ListTasks,
                        None,
                        Outcome::Success,
                        "一覧を更新しました。",
                    );
                }
            }
            Err(error) => self.record_server_failure(Operation::ListTasks, None, error),
        }
        ClientEffect::None
    }

    pub fn apply_auto_session_result<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        request_id: u64,
        result: Result<WebSuccess<Option<SessionTask>>, ServerFailure>,
    ) -> ClientEffect {
        if !consume_latest(&mut self.read.latest_auto_request_id, request_id) {
            self.record_stale_response(Operation::AutoSession, result.is_ok());
            return ClientEffect::None;
        }
        self.read.auto_session_in_flight = false;
        match result {
            Ok(success) => {
                let _ = self.apply_snapshot(success.snapshot);
                self.read.auto_session_empty = success.data.is_none();
                self.record_server(
                    Operation::AutoSession,
                    None,
                    Outcome::Success,
                    "自動選定が完了しました。",
                );
                if let Some(task) = success.data {
                    self.add_session(storage, &task);
                }
            }
            Err(error) => self.record_server_failure(Operation::AutoSession, None, error),
        }
        ClientEffect::None
    }

    pub(super) fn apply_snapshot(&mut self, snapshot: ServerSnapshot) -> Option<bool> {
        if self
            .read
            .snapshot
            .as_ref()
            .is_some_and(|current| current.observed_at_epoch_ms > snapshot.observed_at_epoch_ms)
        {
            return None;
        }
        let changed = self
            .read
            .snapshot
            .as_ref()
            .is_none_or(|current| current.logical_date != snapshot.logical_date);
        if changed {
            self.read.date_buttons =
                logical_date_buttons(&snapshot.logical_date).unwrap_or_default();
            self.read.selected_logical_date = None;
            self.read.scheduled_rows.clear();
            self.read.buffer_tracking_started_at_epoch_ms = Some(snapshot.observed_at_epoch_ms);
            self.read.committed_session_intervals.clear();
        }
        self.read.snapshot = Some(snapshot);
        Some(changed)
    }

    fn allocate_read_request_id(&mut self) -> Option<u64> {
        let request_id = self.read.next_request_id;
        self.read.next_request_id = request_id.checked_add(1)?;
        Some(request_id)
    }
}

impl ReadState {
    pub(super) fn record_committed_session_interval(&mut self, interval: Range<i64>) {
        if interval.start >= interval.end {
            return;
        }
        let mut intervals = std::mem::take(&mut self.committed_session_intervals);
        intervals.push(interval);
        intervals.sort_unstable_by_key(|current| current.start);

        for current in intervals {
            if let Some(previous) = self.committed_session_intervals.last_mut() {
                if current.start <= previous.end {
                    previous.end = previous.end.max(current.end);
                    continue;
                }
            }
            self.committed_session_intervals.push(current);
        }
    }
}

fn consume_latest(latest_request_id: &mut Option<u64>, request_id: u64) -> bool {
    if *latest_request_id != Some(request_id) {
        return false;
    }
    *latest_request_id = None;
    true
}
