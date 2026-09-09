use super::{ServerFailure, WebError};

pub(super) fn completion_conflict_actual(error: &ServerFailure) -> Option<i64> {
    match error {
        ServerFailure::Operation(WebError {
            code,
            current_actual_work_seconds: Some(current_actual_work_seconds),
            ..
        }) if code == crate::web_error_codes::ACTUAL_WORK_CONFLICT
            && *current_actual_work_seconds >= 0 =>
        {
            Some(*current_actual_work_seconds)
        }
        _ => None,
    }
}

pub(super) fn keeps_safety_marker(error: &ServerFailure) -> bool {
    matches!(error, ServerFailure::Transport(_))
        || matches!(
            error,
            ServerFailure::Operation(WebError { code, .. })
                if code == crate::web_error_codes::REPOSITORY_STATE_UNCERTAIN
        )
}
