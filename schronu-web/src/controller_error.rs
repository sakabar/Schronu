#[cfg(test)]
mod tests {
    use super::map_controller_error;
    use crate::{web_error_codes, RetryAdvice};
    use schronu::adapter::controller::{WebReadError, WebSessionInputError};
    use schronu::application::interface::{TaskRepositoryError, TaskRepositoryOperation};
    use schronu::application::task_use_case::ApplicationError;
    use std::path::PathBuf;

    fn assert_mapping(error: WebReadError, expected_code: &str, expected_advice: RetryAdvice) {
        let mapped = map_controller_error(error);
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
