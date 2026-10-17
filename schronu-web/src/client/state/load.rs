use super::*;
use crate::{BandDay, LoadData, RoutineLoadReport, WebSuccess};

pub(super) struct LoadState {
    pub(super) rows: Vec<BandDay>,
    pub(super) routine_load_report: Option<RoutineLoadReport>,
    pub(super) observed_at_epoch_ms: Option<i64>,
    pub(super) loading: bool,
    pub(super) error: Option<String>,
    pub(super) latest_request_id: Option<u64>,
}

impl LoadState {
    pub(super) fn new() -> Self {
        Self {
            rows: Vec::new(),
            routine_load_report: None,
            observed_at_epoch_ms: None,
            loading: false,
            error: None,
            latest_request_id: None,
        }
    }
}

impl ClientState {
    pub fn band_rows(&self) -> &[BandDay] {
        &self.load.rows
    }

    pub fn band_observed_at_epoch_ms(&self) -> Option<i64> {
        self.load.observed_at_epoch_ms
    }

    pub fn routine_load_report(&self) -> Option<&RoutineLoadReport> {
        self.load.routine_load_report.as_ref()
    }

    pub fn band_loading(&self) -> bool {
        self.load.loading
    }

    pub fn band_error(&self) -> Option<&str> {
        self.load.error.as_deref()
    }

    pub fn request_load_band(&mut self) -> ClientEffect {
        let Some(request_id) = self.allocate_read_request_id() else {
            return ClientEffect::None;
        };
        self.load.latest_request_id = Some(request_id);
        self.load.loading = true;
        self.load.error = None;
        ClientEffect::LoadBand { request_id }
    }

    pub fn apply_load_band_result(
        &mut self,
        request_id: u64,
        result: Result<WebSuccess<LoadData>, ServerFailure>,
    ) -> ClientEffect {
        let invocation = ServerActionInvocation::LoadBand;
        if self.load.latest_request_id != Some(request_id) {
            return ClientEffect::None;
        }
        self.load.latest_request_id = None;
        self.load.loading = false;
        match result {
            Ok(success) => {
                let observed_at = success.snapshot.observed_at_epoch_ms;
                if self.apply_snapshot_metadata(success.snapshot).is_none() {
                    return ClientEffect::None;
                }
                self.load.rows = success.data.band_days;
                self.load.routine_load_report = Some(success.data.routine_load);
                self.load.observed_at_epoch_ms = Some(observed_at);
                self.load.error = None;
                self.record_server_success(invocation);
            }
            Err(error) => {
                self.load.error = Some(match &error {
                    ServerFailure::Operation(error) => error.message.clone(),
                    ServerFailure::Transport(_) => {
                        "負荷を取得できませんでした。再試行してください。".to_owned()
                    }
                });
                if self.diagnostics.display_error.as_ref().is_none_or(|error| {
                    matches!(
                        error,
                        DisplayError::Operation {
                            operation: Operation::LoadBand,
                            ..
                        } | DisplayError::Transport {
                            operation: Operation::LoadBand,
                            ..
                        }
                    )
                }) {
                    self.record_server_failure(invocation, error);
                }
            }
        }
        ClientEffect::None
    }
}
