use super::session_progress::{
    calculate_session_progress, SessionProgress, SessionProgressCalculationError,
};

#[test]
fn 見積と経過を秒単位で計算する() {
    let actual = calculate_session_progress(273, 0, 120).unwrap();

    assert_eq!(
        actual,
        SessionProgress {
            total_work_seconds: 120,
            progress_percent: Some(43),
            remaining_work_seconds_at_start: 273,
            remaining_work_seconds: 153,
        }
    );
}

#[test]
fn 開始時の既存実績を進捗へ含める() {
    let actual = calculate_session_progress(900, 300, 0).unwrap();

    assert_eq!(actual.total_work_seconds, 300);
    assert_eq!(actual.progress_percent, Some(33));
    assert_eq!(actual.remaining_work_seconds_at_start, 600);
    assert_eq!(actual.remaining_work_seconds, 600);
}

#[test]
fn 進捗率は100パーセントを超えて保持する() {
    let actual = calculate_session_progress(100, 114, 2).unwrap();

    assert_eq!(actual.total_work_seconds, 116);
    assert_eq!(actual.progress_percent, Some(116));
    assert_eq!(actual.remaining_work_seconds_at_start, 0);
    assert_eq!(actual.remaining_work_seconds, -2);
}

#[test]
fn 見積0は進捗率を未算定として表す() {
    let actual = calculate_session_progress(0, 600, 60).unwrap();

    assert_eq!(actual.total_work_seconds, 660);
    assert_eq!(actual.progress_percent, None);
    assert_eq!(actual.remaining_work_seconds_at_start, 0);
    assert_eq!(actual.remaining_work_seconds, -60);
}

#[test]
fn 負の入力はfieldと値を保持したerrorにする() {
    for (estimated, actual_at_start, elapsed, field) in [
        (-1, 0, 0, "estimated_work_seconds"),
        (0, -1, 0, "actual_work_seconds_at_start"),
        (0, 0, -1, "elapsed_seconds"),
    ] {
        let actual = calculate_session_progress(estimated, actual_at_start, elapsed);

        assert_eq!(
            actual,
            Err(SessionProgressCalculationError::NegativeSeconds { field, value: -1 })
        );
    }
}

#[test]
fn 総作業秒はi64を超えても正確に保持する() {
    let actual = calculate_session_progress(i64::MAX, i64::MAX, 1).unwrap();

    assert_eq!(actual.total_work_seconds, i128::from(i64::MAX) + 1);
    assert_eq!(actual.progress_percent, Some(100));
}

#[test]
fn 有効なi64入力の巨大な進捗率を正確に保持する() {
    let actual = calculate_session_progress(1, i64::MAX, 0).unwrap();

    assert_eq!(actual.progress_percent, Some(i128::from(i64::MAX) * 100));
}
