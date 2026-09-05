use crate::{web_error_codes, RetryAdvice, WebError};
use schronu::adapter::controller::WebReadError;
use schronu::application::task_use_case::ApplicationError;

impl From<WebReadError> for WebError {
    fn from(error: WebReadError) -> Self {
        match error {
            WebReadError::InvalidInput(_) => manual(
                web_error_codes::INVALID_INPUT,
                "入力内容を確認してください。",
            ),
            WebReadError::Application(error) => map_application_error(error),
            WebReadError::Overflow(_) => manual(
                web_error_codes::ARITHMETIC_OVERFLOW,
                "時間の計算結果が範囲を超えました。",
            ),
            WebReadError::BusyTimeSlots(_) | WebReadError::PathEncoding(_) => manual(
                web_error_codes::CONFIGURATION_ERROR,
                "Schronuの設定を確認してください。",
            ),
            WebReadError::Lock(_) | WebReadError::Repository(_) => retry(
                web_error_codes::REPOSITORY_UNAVAILABLE,
                "データを読み込めませんでした。時間をおいて再試行してください。",
            ),
            WebReadError::RepositorySaveFailed(_) => retry(
                web_error_codes::REPOSITORY_SAVE_FAILED,
                "データを保存できませんでした。再試行してください。",
            ),
            WebReadError::RepositoryStateUncertain(_) | WebReadError::RepositoryPoisoned => manual(
                web_error_codes::REPOSITORY_STATE_UNCERTAIN,
                "保存結果を確認できません。Schronuの状態を確認してください。",
            ),
        }
    }
}

fn map_application_error(error: ApplicationError) -> WebError {
    match error {
        ApplicationError::TaskNotFound(_) => manual(
            web_error_codes::TASK_NOT_FOUND,
            "対象のタスクが見つかりません。",
        ),
        ApplicationError::TaskAlreadyCompleted(_) => manual(
            web_error_codes::TASK_ALREADY_COMPLETED,
            "対象のタスクはすでに完了しています。",
        ),
        ApplicationError::ActualWorkConflict { .. } => manual(
            web_error_codes::ACTUAL_WORK_CONFLICT,
            "タスクの実績時間が更新されています。再読み込みして確認してください。",
        ),
        ApplicationError::InvalidInput { .. }
        | ApplicationError::AmbiguousLocalDateTime { .. }
        | ApplicationError::NonexistentLocalDateTime { .. }
        | ApplicationError::LogicalDateOutOfRange { .. }
        | ApplicationError::LogicalDateStartOutOfRange { .. }
        | ApplicationError::LogicalDateEndOutOfRange { .. } => manual(
            web_error_codes::INVALID_INPUT,
            "入力した日時または値を確認してください。",
        ),
        ApplicationError::HasUndoneChildren(_) => manual(
            web_error_codes::TASK_NOT_COMPLETABLE,
            "未完了の子タスクがあるため完了できません。",
        ),
        ApplicationError::ScheduleTimeOutOfRange { .. }
        | ApplicationError::RemainingWorkCalculationOverflow { .. } => manual(
            web_error_codes::ARITHMETIC_OVERFLOW,
            "タスク時間の計算結果が範囲を超えました。",
        ),
        ApplicationError::TaskTree(_) | ApplicationError::ProjectRegistration(_) => manual(
            web_error_codes::OPERATION_FAILED,
            "操作を完了できませんでした。Schronuの状態を確認してください。",
        ),
    }
}

fn retry(code: &str, message: &str) -> WebError {
    WebError {
        code: code.to_owned(),
        message: message.to_owned(),
        retry_advice: RetryAdvice::Retry,
    }
}

fn manual(code: &str, message: &str) -> WebError {
    WebError {
        code: code.to_owned(),
        message: message.to_owned(),
        retry_advice: RetryAdvice::ManualCheck,
    }
}

#[cfg(test)]
mod tests {
    use crate::{web_error_codes, RetryAdvice};
    use schronu::adapter::controller::{WebReadError, WebSessionInputError};
    use schronu::application::interface::{TaskRepositoryError, TaskRepositoryOperation};
    use schronu::application::task_use_case::ApplicationError;
    use std::path::PathBuf;

    fn assert_mapping(error: WebReadError, expected_code: &str, expected_advice: RetryAdvice) {
        let mapped = crate::WebError::from(error);
        assert_eq!(mapped.code, expected_code);
        assert_eq!(mapped.retry_advice, expected_advice);
        assert!(!mapped.message.contains("private-detail"));
        assert!(!mapped.message.contains("/secret/path"));
    }

    #[test]
    fn 利用者が修正できる入力とtask状態はmanual_checkへ分類する() {
        let task_id = "00000000-0000-4000-8000-000000000001"
            .parse()
            .expect("fixture UUID must be valid");
        let cases = [
            (
                WebReadError::InvalidInput(WebSessionInputError::InvalidTaskId {
                    task_id: "private-detail".to_owned(),
                    reason: "private-detail".to_owned(),
                }),
                web_error_codes::INVALID_INPUT,
            ),
            (
                WebReadError::Application(ApplicationError::TaskNotFound(task_id)),
                web_error_codes::TASK_NOT_FOUND,
            ),
            (
                WebReadError::Application(ApplicationError::TaskAlreadyCompleted(task_id)),
                web_error_codes::TASK_ALREADY_COMPLETED,
            ),
            (
                WebReadError::Application(ApplicationError::ActualWorkConflict {
                    task_id,
                    expected_actual_work_seconds: 1,
                    actual_work_seconds: 2,
                }),
                web_error_codes::ACTUAL_WORK_CONFLICT,
            ),
            (
                WebReadError::Application(ApplicationError::HasUndoneChildren(task_id)),
                web_error_codes::TASK_NOT_COMPLETABLE,
            ),
        ];

        for (error, expected_code) in cases {
            assert_mapping(error, expected_code, RetryAdvice::ManualCheck);
        }
    }

    #[test]
    fn repositoryの確定状態に応じてretry可否を分類する() {
        let load_error = TaskRepositoryError::new(
            TaskRepositoryOperation::Load,
            std::io::Error::other("private-detail"),
        );
        assert_mapping(
            WebReadError::Repository(load_error),
            web_error_codes::REPOSITORY_UNAVAILABLE,
            RetryAdvice::Retry,
        );

        let retryable_save =
            TaskRepositoryError::retryable_save(std::io::Error::other("private-detail"));
        assert_mapping(
            WebReadError::RepositorySaveFailed(retryable_save),
            web_error_codes::REPOSITORY_SAVE_FAILED,
            RetryAdvice::Retry,
        );

        let uncertain_save = TaskRepositoryError::new(
            TaskRepositoryOperation::Save,
            std::io::Error::other("private-detail"),
        );
        assert_mapping(
            WebReadError::RepositoryStateUncertain(uncertain_save),
            web_error_codes::REPOSITORY_STATE_UNCERTAIN,
            RetryAdvice::ManualCheck,
        );
        assert_mapping(
            WebReadError::RepositoryPoisoned,
            web_error_codes::REPOSITORY_STATE_UNCERTAIN,
            RetryAdvice::ManualCheck,
        );
    }

    #[test]
    fn 設定と未知のoperation失敗をmanual_checkへ分類して内部情報を隠す() {
        assert_mapping(
            WebReadError::PathEncoding(PathBuf::from("/secret/path")),
            web_error_codes::CONFIGURATION_ERROR,
            RetryAdvice::ManualCheck,
        );
        assert_mapping(
            WebReadError::Application(ApplicationError::InvalidInput {
                field: "private-detail",
                reason: "private-detail",
            }),
            web_error_codes::INVALID_INPUT,
            RetryAdvice::ManualCheck,
        );
    }
}
