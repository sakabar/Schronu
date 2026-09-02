use chrono::{DateTime, Local};

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
