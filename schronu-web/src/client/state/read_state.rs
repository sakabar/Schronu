use super::*;
use crate::client::date_buttons::logical_date_buttons;
use crate::{
    DiscardedSessionDay, ListDiscardedSessionsRequest, ListTasksRequest, SessionTask, WebSuccess,
};

pub(super) struct ReadState {
    pub(super) snapshot: Option<ServerSnapshot>,
    pub(super) date_buttons: Vec<LogicalDateButton>,
    pub(super) selected_logical_date: Option<String>,
    pub(super) scheduled_rows: Vec<ScheduledTaskRow>,
    pub(super) has_list: bool,
    pub(super) listed_logical_date: Option<String>,
    pub(super) auto_session_empty: bool,
    pub(super) auto_session_in_flight: bool,
    pub(super) next_request_id: u64,
    pub(super) latest_bootstrap_request_id: Option<u64>,
    pub(super) latest_list_request_id: Option<u64>,
    pub(super) latest_auto_request_id: Option<u64>,
    pub(super) discarded_sessions: DiscardedSessionsViewState,
    pub(super) latest_discarded_request_id: Option<u64>,
}

impl ReadState {
    pub(super) fn new() -> Self {
        Self {
            snapshot: None,
            date_buttons: Vec::new(),
            selected_logical_date: None,
            scheduled_rows: Vec::new(),
            has_list: false,
            listed_logical_date: None,
            auto_session_empty: false,
            auto_session_in_flight: false,
            next_request_id: 1,
            latest_bootstrap_request_id: None,
            latest_list_request_id: None,
            latest_auto_request_id: None,
            discarded_sessions: DiscardedSessionsViewState::Idle,
            latest_discarded_request_id: None,
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

    pub fn request_discarded_sessions(&mut self, logical_date: &str) -> ClientEffect {
        let Some(request_id) = self.allocate_read_request_id() else {
            return ClientEffect::None;
        };
        self.clear_discarded_sessions_read_error();
        self.read.latest_discarded_request_id = Some(request_id);
        self.read.discarded_sessions = DiscardedSessionsViewState::Loading {
            logical_date: logical_date.to_owned(),
        };
        ClientEffect::ListDiscardedSessions {
            request_id,
            request: ListDiscardedSessionsRequest {
                logical_date: logical_date.to_owned(),
            },
        }
    }

    pub fn apply_discarded_sessions_result(
        &mut self,
        request_id: u64,
        requested_date: &str,
        result: Result<WebSuccess<DiscardedSessionDay>, ServerFailure>,
    ) -> ClientEffect {
        let invocation =
            ServerActionInvocation::ListDiscardedSessions(ListDiscardedSessionsRequest {
                logical_date: requested_date.to_owned(),
            });
        if !consume_latest(&mut self.read.latest_discarded_request_id, request_id) {
            self.record_stale_response(invocation, result.is_ok());
            return ClientEffect::None;
        }
        match result {
            Ok(success) => {
                let _ = self.apply_snapshot_metadata(success.snapshot);
                self.read.selected_logical_date = Some(requested_date.to_owned());
                self.read.discarded_sessions = DiscardedSessionsViewState::Loaded(success.data);
                self.record_server(invocation, Outcome::Success, "破棄時間を更新しました。");
            }
            Err(error) => {
                self.record_server_failure(invocation, error);
                let message = self
                    .diagnostics
                    .display_error
                    .as_ref()
                    .map(DisplayError::message)
                    .unwrap_or("破棄時間を取得できませんでした。")
                    .to_owned();
                self.read.discarded_sessions = DiscardedSessionsViewState::Error {
                    logical_date: requested_date.to_owned(),
                    message,
                };
            }
        }
        ClientEffect::None
    }

    pub fn apply_bootstrap_result(
        &mut self,
        request_id: u64,
        result: Result<ServerSnapshot, ServerFailure>,
    ) -> ClientEffect {
        if !consume_latest(&mut self.read.latest_bootstrap_request_id, request_id) {
            self.record_stale_response(ServerActionInvocation::Bootstrap, result.is_ok());
            return ClientEffect::None;
        }
        match result {
            Ok(snapshot) => {
                let cached_logical_date = self
                    .read
                    .has_list
                    .then(|| self.read.selected_logical_date.clone())
                    .flatten();
                if self.apply_snapshot_metadata(snapshot).is_none() {
                    self.record_stale_response(ServerActionInvocation::Bootstrap, true);
                    return ClientEffect::None;
                }
                self.record_server(
                    ServerActionInvocation::Bootstrap,
                    Outcome::Success,
                    "更新しました。",
                );
                if let Some(logical_date) = cached_logical_date {
                    return self.request_list(&logical_date);
                }
            }
            Err(error) => self.record_server_failure(ServerActionInvocation::Bootstrap, error),
        }
        ClientEffect::None
    }

    pub fn apply_list_result(
        &mut self,
        request_id: u64,
        requested_date: &str,
        result: Result<WebSuccess<Vec<ScheduledTaskRow>>, ServerFailure>,
    ) -> ClientEffect {
        self.apply_list_result_with_policy(request_id, requested_date, result, false)
    }

    #[cfg_attr(not(all(feature = "web", target_arch = "wasm32")), allow(dead_code))]
    pub(crate) fn apply_background_list_result(
        &mut self,
        request_id: u64,
        requested_date: &str,
        result: Result<WebSuccess<Vec<ScheduledTaskRow>>, ServerFailure>,
    ) -> ClientEffect {
        self.apply_list_result_with_policy(request_id, requested_date, result, true)
    }

    fn apply_list_result_with_policy(
        &mut self,
        request_id: u64,
        requested_date: &str,
        result: Result<WebSuccess<Vec<ScheduledTaskRow>>, ServerFailure>,
        preserve_across_logical_date_change: bool,
    ) -> ClientEffect {
        let invocation = ServerActionInvocation::ListTasks(ListTasksRequest {
            logical_date: requested_date.to_owned(),
        });
        if !consume_latest(&mut self.read.latest_list_request_id, request_id) {
            self.record_stale_response(invocation, result.is_ok());
            return ClientEffect::None;
        }
        match result {
            Ok(success) => {
                let same_logical_date =
                    self.read.snapshot.as_ref().is_some_and(|current| {
                        current.logical_date == success.snapshot.logical_date
                    });
                let snapshot_result = if preserve_across_logical_date_change {
                    self.apply_snapshot_metadata(success.snapshot)
                } else {
                    self.apply_snapshot(success.snapshot)
                };
                if (preserve_across_logical_date_change && snapshot_result.is_some())
                    || snapshot_result == Some(false)
                    || (snapshot_result.is_none() && same_logical_date)
                {
                    self.read.selected_logical_date = Some(requested_date.to_owned());
                    self.read.listed_logical_date = Some(requested_date.to_owned());
                    self.read.scheduled_rows = success.data;
                    self.read.has_list = true;
                }
                if snapshot_result.is_none() {
                    self.record_stale_response(invocation, true);
                } else {
                    self.record_server(invocation, Outcome::Success, "一覧を更新しました。");
                }
            }
            Err(error) => self.record_server_failure(invocation, error),
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
            self.record_stale_response(ServerActionInvocation::AutoSession, result.is_ok());
            return ClientEffect::None;
        }
        self.read.auto_session_in_flight = false;
        match result {
            Ok(success) => {
                let _ = self.apply_snapshot(success.snapshot);
                self.read.auto_session_empty = success.data.is_none();
                self.record_server(
                    ServerActionInvocation::AutoSession,
                    Outcome::Success,
                    "自動選定が完了しました。",
                );
                if let Some(task) = success.data {
                    self.add_session(storage, &task);
                }
            }
            Err(error) => self.record_server_failure(ServerActionInvocation::AutoSession, error),
        }
        ClientEffect::None
    }

    pub(super) fn apply_snapshot(&mut self, snapshot: ServerSnapshot) -> Option<bool> {
        let changed = self.apply_snapshot_metadata(snapshot)?;
        if changed {
            self.read.selected_logical_date = None;
            self.read.scheduled_rows.clear();
            self.read.has_list = false;
            self.read.listed_logical_date = None;
        }
        Some(changed)
    }

    pub(super) fn apply_mutation_snapshot_and_request_list(
        &mut self,
        snapshot: ServerSnapshot,
    ) -> ClientEffect {
        let _ = self.apply_snapshot_metadata(snapshot);
        self.request_selected_or_current_list()
    }

    pub(super) fn request_selected_or_current_list(&mut self) -> ClientEffect {
        let logical_date = self.read.selected_logical_date.clone().or_else(|| {
            self.read
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.logical_date.clone())
        });
        logical_date.map_or(ClientEffect::None, |date| self.request_list(&date))
    }

    fn apply_snapshot_metadata(&mut self, snapshot: ServerSnapshot) -> Option<bool> {
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

fn consume_latest(latest_request_id: &mut Option<u64>, request_id: u64) -> bool {
    if *latest_request_id != Some(request_id) {
        return false;
    }
    *latest_request_id = None;
    true
}
