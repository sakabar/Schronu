use super::diagnostics::READ_TRANSPORT_ERROR_MESSAGE;
use super::*;
use crate::{AllTaskPage, AllTaskRow, ListAllTasksRequest, WebSuccess};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListSelection {
    Date,
    All,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AllTasksStatus {
    NotLoaded,
    Loading,
    Loaded,
    Failed,
    Invalidated,
}

struct LoadingAllTasks {
    rows: Vec<AllTaskRow>,
    expected_request_id: u64,
    expected_cursor: Option<String>,
}

enum AllTasksLoadState {
    NotLoaded,
    Loading(LoadingAllTasks),
    Loaded(Vec<AllTaskRow>),
    Failed(ServerFailure),
    Invalidated,
}

pub(super) struct AllTasksState {
    selection: ListSelection,
    load: AllTasksLoadState,
}

impl AllTasksState {
    pub(super) fn new() -> Self {
        Self {
            selection: ListSelection::Date,
            load: AllTasksLoadState::NotLoaded,
        }
    }
}

impl ClientState {
    pub fn list_selection(&self) -> ListSelection {
        self.all_tasks.selection
    }

    pub fn all_tasks_status(&self) -> AllTasksStatus {
        match self.all_tasks.load {
            AllTasksLoadState::NotLoaded => AllTasksStatus::NotLoaded,
            AllTasksLoadState::Loading(_) => AllTasksStatus::Loading,
            AllTasksLoadState::Loaded(_) => AllTasksStatus::Loaded,
            AllTasksLoadState::Failed(_) => AllTasksStatus::Failed,
            AllTasksLoadState::Invalidated => AllTasksStatus::Invalidated,
        }
    }

    pub fn all_task_rows(&self) -> Option<&[AllTaskRow]> {
        match &self.all_tasks.load {
            AllTasksLoadState::Loaded(rows) => Some(rows),
            _ => None,
        }
    }

    pub fn all_tasks_failure(&self) -> Option<&ServerFailure> {
        match &self.all_tasks.load {
            AllTasksLoadState::Failed(error) => Some(error),
            _ => None,
        }
    }

    pub fn all_tasks_failure_message(&self) -> Option<&str> {
        match &self.all_tasks.load {
            AllTasksLoadState::Failed(ServerFailure::Operation(error)) => Some(&error.message),
            AllTasksLoadState::Failed(ServerFailure::Transport(_)) => {
                Some(READ_TRANSPORT_ERROR_MESSAGE)
            }
            _ => None,
        }
    }

    pub fn select_logical_date(&mut self, logical_date: &str) -> ClientEffect {
        self.all_tasks.selection = ListSelection::Date;
        self.request_list(logical_date)
    }

    pub fn select_all_tasks(&mut self) -> ClientEffect {
        self.all_tasks.selection = ListSelection::All;
        if matches!(
            self.all_tasks.load,
            AllTasksLoadState::NotLoaded | AllTasksLoadState::Invalidated
        ) {
            self.start_all_tasks_request()
        } else {
            ClientEffect::None
        }
    }

    pub fn retry_all_tasks(&mut self) -> ClientEffect {
        if matches!(self.all_tasks.load, AllTasksLoadState::Failed(_)) {
            self.all_tasks.selection = ListSelection::All;
            self.start_all_tasks_request()
        } else {
            ClientEffect::None
        }
    }

    fn start_all_tasks_request(&mut self) -> ClientEffect {
        let Some(request_id) = self.allocate_read_request_id() else {
            return ClientEffect::None;
        };
        self.all_tasks.load = AllTasksLoadState::Loading(LoadingAllTasks {
            rows: Vec::new(),
            expected_request_id: request_id,
            expected_cursor: None,
        });
        ClientEffect::ListAllTasks {
            request_id,
            request: ListAllTasksRequest { cursor: None },
        }
    }

    pub fn apply_all_tasks_result(
        &mut self,
        request_id: u64,
        request: ListAllTasksRequest,
        result: Result<WebSuccess<AllTaskPage>, ServerFailure>,
    ) -> ClientEffect {
        let invocation = ServerActionInvocation::ListAllTasks(request.clone());
        let current = std::mem::replace(&mut self.all_tasks.load, AllTasksLoadState::NotLoaded);
        let AllTasksLoadState::Loading(mut loading) = current else {
            self.all_tasks.load = current;
            self.record_stale_response(invocation, result.is_ok());
            return ClientEffect::None;
        };
        if loading.expected_request_id != request_id || loading.expected_cursor != request.cursor {
            self.all_tasks.load = AllTasksLoadState::Loading(loading);
            self.record_stale_response(invocation, result.is_ok());
            return ClientEffect::None;
        }

        match result {
            Ok(success) => {
                let _ = self.apply_snapshot_metadata(success.snapshot);
                loading.rows.extend(success.data.rows);
                self.record_server(invocation, Outcome::Success, "全件一覧を取得しました。");
                let Some(next_cursor) = success.data.next_cursor else {
                    self.all_tasks.load = AllTasksLoadState::Loaded(loading.rows);
                    return ClientEffect::None;
                };
                let Some(next_request_id) = self.allocate_read_request_id() else {
                    self.all_tasks.load = AllTasksLoadState::Failed(ServerFailure::Transport(
                        "request id exhausted".to_owned(),
                    ));
                    return ClientEffect::None;
                };
                let request = ListAllTasksRequest {
                    cursor: Some(next_cursor.clone()),
                };
                self.all_tasks.load = AllTasksLoadState::Loading(LoadingAllTasks {
                    rows: loading.rows,
                    expected_request_id: next_request_id,
                    expected_cursor: Some(next_cursor),
                });
                ClientEffect::ListAllTasks {
                    request_id: next_request_id,
                    request,
                }
            }
            Err(error) => {
                self.record_server(
                    invocation,
                    Outcome::Failure,
                    "全件一覧の取得に失敗しました。",
                );
                self.all_tasks.load = AllTasksLoadState::Failed(error);
                ClientEffect::None
            }
        }
    }

    pub(super) fn invalidate_all_tasks(&mut self) {
        if matches!(
            self.all_tasks.load,
            AllTasksLoadState::Loading(_)
                | AllTasksLoadState::Loaded(_)
                | AllTasksLoadState::Failed(_)
        ) {
            self.all_tasks.load = AllTasksLoadState::Invalidated;
        }
    }
}
