use super::*;
use crate::application::interface::TaskRepositorySaveFailureDisposition;
use crate::test_support::TestTaskRepository;
use chrono::{Local, TimeZone};

fn transaction_error(
    disposition: TaskRepositorySaveFailureDisposition,
) -> RepositoryTransactionError<(), ()> {
    let now = Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let mut repository = TestTaskRepository::new(Vec::new(), now);
    repository.set_save_failure_disposition(disposition);
    run_repository_transaction(
        &mut repository,
        now,
        || Ok::<_, ()>(()),
        |_| Ok::<_, ()>(((), true)),
    )
    .unwrap_err()
}

#[test]
fn retryableなsave失敗をsave_failedとして返す() {
    assert!(matches!(
        transaction_error(TaskRepositorySaveFailureDisposition::Retryable),
        RepositoryTransactionError::SaveFailed(_)
    ));
}

#[test]
fn commit状態不確実なsave失敗をstate_uncertainとして返す() {
    assert!(matches!(
        transaction_error(TaskRepositorySaveFailureDisposition::StateUncertain),
        RepositoryTransactionError::StateUncertain(_)
    ));
}
