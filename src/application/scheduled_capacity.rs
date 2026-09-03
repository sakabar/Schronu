use super::daily_capacity::{try_logical_date, try_next_logical_date_start};
use super::task_use_case::ApplicationError;
use chrono::{DateTime, Local, NaiveDate};

/// schedule segmentが日次容量を占有する秒数を返す。
///
/// fixedは作業残秒ではなく予約window全体を占有する。flexibleは分割された実作業秒を
/// 使う。この区別によりwork conservationと予約容量を混同せず、重複fixedも各予約を
/// 個別加算して過負荷として可視化できる。
pub(crate) fn scheduled_capacity_seconds(
    fixed_start: bool,
    scheduled_start: DateTime<Local>,
    scheduled_end: DateTime<Local>,
    scheduled_work_seconds: i64,
) -> i64 {
    if fixed_start {
        (scheduled_end - scheduled_start).num_seconds().max(0)
    } else {
        scheduled_work_seconds.max(0)
    }
}

/// schedule segmentの容量を、交差する論理日へ時系列順に配賦する。
#[allow(dead_code)] // packとflattenを後続commitで順次移行するまでの段階的導入。
pub(crate) fn scheduled_capacity_seconds_by_logical_date(
    fixed_start: bool,
    scheduled_start: DateTime<Local>,
    scheduled_end: DateTime<Local>,
    scheduled_work_seconds: i64,
) -> Result<Vec<(NaiveDate, i64)>, ApplicationError> {
    if scheduled_end <= scheduled_start {
        return Ok(Vec::new());
    }

    let capacity_seconds = scheduled_capacity_seconds(
        fixed_start,
        scheduled_start,
        scheduled_end,
        scheduled_work_seconds,
    );
    if capacity_seconds == 0 {
        return Ok(Vec::new());
    }

    let mut intersections = Vec::<(NaiveDate, i64)>::new();
    let mut cursor = scheduled_start;
    while cursor < scheduled_end {
        let date = try_logical_date(cursor)?;
        let next_boundary = try_next_logical_date_start(cursor)?;
        let intersection_end = scheduled_end.min(next_boundary);
        let intersection_seconds = (intersection_end - cursor).num_seconds().max(0);
        intersections.push((date, intersection_seconds));
        cursor = intersection_end;
    }

    if fixed_start {
        let mut allocated_seconds = 0;
        let last_index = intersections.len() - 1;
        for (index, (_, seconds)) in intersections.iter_mut().enumerate() {
            if index == last_index {
                *seconds = capacity_seconds - allocated_seconds;
            } else {
                allocated_seconds += *seconds;
            }
        }
        return Ok(intersections);
    }

    let segment_seconds = (scheduled_end - scheduled_start).num_seconds();
    if segment_seconds <= 0 {
        return Ok(vec![(try_logical_date(scheduled_start)?, capacity_seconds)]);
    }

    let mut allocated_seconds = 0;
    let last_index = intersections.len() - 1;
    for (index, (_, seconds)) in intersections.iter_mut().enumerate() {
        if index == last_index {
            *seconds = capacity_seconds - allocated_seconds;
        } else {
            let proportional_seconds = (i128::from(capacity_seconds) * i128::from(*seconds)
                / i128::from(segment_seconds)) as i64;
            *seconds = proportional_seconds;
            allocated_seconds += proportional_seconds;
        }
    }

    Ok(intersections)
}

#[cfg(test)]
#[path = "scheduled_capacity_tests.rs"]
mod tests;
