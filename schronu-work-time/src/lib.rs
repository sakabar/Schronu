/// Returns the scheduled work remaining for a task with non-negative times.
///
/// Once actual work exceeds the estimate, one more estimate is budgeted. The
/// intermediate value uses `i128` so valid `i64` inputs cannot overflow.
pub fn remaining_work_seconds(estimated_work_seconds: i64, actual_work_seconds: i64) -> i128 {
    let estimated = i128::from(estimated_work_seconds);
    let actual = i128::from(actual_work_seconds);
    let budget = if actual > estimated {
        estimated * 2
    } else {
        estimated
    };
    (budget - actual).max(0)
}

#[cfg(test)]
mod tests {
    use super::remaining_work_seconds;

    #[test]
    fn remaining_work_uses_estimate_then_doubled_estimate_after_overrun() {
        assert_eq!(remaining_work_seconds(1_800, 600), 1_200);
        assert_eq!(remaining_work_seconds(1_800, 2_400), 1_200);
        assert_eq!(remaining_work_seconds(1_800, 3_600), 0);
        assert_eq!(remaining_work_seconds(1_800, 3_601), 0);
        assert_eq!(remaining_work_seconds(0, 0), 0);
    }

    #[test]
    fn remaining_work_uses_wider_intermediate_for_large_values() {
        assert_eq!(
            remaining_work_seconds(i64::MAX - 7, i64::MAX - 6),
            i128::from(i64::MAX - 8)
        );
    }
}
