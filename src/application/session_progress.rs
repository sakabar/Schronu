use std::fmt::{Display, Formatter};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionProgress {
    pub total_work_seconds: i64,
    pub progress_percent: Option<i64>,
    pub remaining_work_seconds_at_start: i64,
    pub remaining_work_seconds: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionProgressCalculationError {
    NegativeSeconds {
        field: &'static str,
        value: i64,
    },
    TotalWorkSecondsOverflow {
        actual_work_seconds_at_start: i64,
        elapsed_seconds: i64,
    },
    ProgressPercentOverflow {
        total_work_seconds: i64,
        estimated_work_seconds: i64,
    },
}

impl Display for SessionProgressCalculationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NegativeSeconds { field, value } => {
                write!(formatter, "{field} must be non-negative: {value}")
            }
            Self::TotalWorkSecondsOverflow {
                actual_work_seconds_at_start,
                elapsed_seconds,
            } => write!(
                formatter,
                "total work seconds overflow: actual_work_seconds_at_start={actual_work_seconds_at_start}, elapsed_seconds={elapsed_seconds}"
            ),
            Self::ProgressPercentOverflow {
                total_work_seconds,
                estimated_work_seconds,
            } => write!(
                formatter,
                "progress percent overflow: total_work_seconds={total_work_seconds}, estimated_work_seconds={estimated_work_seconds}"
            ),
        }
    }
}

impl std::error::Error for SessionProgressCalculationError {}

pub fn calculate_session_progress(
    estimated_work_seconds: i64,
    actual_work_seconds_at_start: i64,
    elapsed_seconds: i64,
) -> Result<SessionProgress, SessionProgressCalculationError> {
    validate_non_negative("estimated_work_seconds", estimated_work_seconds)?;
    validate_non_negative("actual_work_seconds_at_start", actual_work_seconds_at_start)?;
    validate_non_negative("elapsed_seconds", elapsed_seconds)?;

    let total_work_seconds = actual_work_seconds_at_start
        .checked_add(elapsed_seconds)
        .ok_or(SessionProgressCalculationError::TotalWorkSecondsOverflow {
            actual_work_seconds_at_start,
            elapsed_seconds,
        })?;
    let progress_percent = if estimated_work_seconds == 0 {
        None
    } else {
        let percentage = i128::from(total_work_seconds) * 100 / i128::from(estimated_work_seconds);
        Some(i64::try_from(percentage).map_err(|_| {
            SessionProgressCalculationError::ProgressPercentOverflow {
                total_work_seconds,
                estimated_work_seconds,
            }
        })?)
    };
    let remaining_work_seconds_at_start = if estimated_work_seconds > actual_work_seconds_at_start {
        estimated_work_seconds - actual_work_seconds_at_start
    } else {
        0
    };
    let remaining_work_seconds = remaining_work_seconds_at_start - elapsed_seconds;

    Ok(SessionProgress {
        total_work_seconds,
        progress_percent,
        remaining_work_seconds_at_start,
        remaining_work_seconds,
    })
}

fn validate_non_negative(
    field: &'static str,
    value: i64,
) -> Result<(), SessionProgressCalculationError> {
    if value < 0 {
        Err(SessionProgressCalculationError::NegativeSeconds { field, value })
    } else {
        Ok(())
    }
}
