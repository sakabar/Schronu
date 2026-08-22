use super::daily_capacity::{
    calculate_daily_leeway_seconds,
    calculate_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes,
    try_subjective_date, try_subjective_date_end, try_subjective_date_start,
    END_OF_DAY_OFFSET_MINUTES,
};
use super::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use super::schedule_use_case::{
    get_schedule, get_schedule_with_task_first_available_time, ScheduledTaskView,
};
use super::task_use_case::ApplicationError;
use crate::entity::task::Status;
use chrono::{DateTime, Duration, Local, NaiveDate};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const PACK_TARGET_DAYS: i64 = 7;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackedTask {
    pub task_id: Uuid,
    pub name: String,
    pub priority: i64,
    pub source_date: NaiveDate,
    pub target_date: NaiveDate,
    pub work_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkippedTask {
    pub task_id: Uuid,
    pub name: String,
    pub priority: i64,
    pub required_work_seconds: i64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackResult {
    pub packed_tasks: Vec<PackedTask>,
    pub skipped_tasks: Vec<SkippedTask>,
}

#[derive(Clone)]
struct PackCandidate {
    task_id: Uuid,
    name: String,
    priority: i64,
    planned_start: DateTime<Local>,
    work_seconds: i64,
}

#[derive(Clone, Copy)]
struct PackTargetDay {
    date: NaiveDate,
    end: DateTime<Local>,
}

pub fn pack_tasks(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<PackResult, ApplicationError> {
    pack_tasks_with_end_of_day_offset_minutes(
        repository,
        free_time_manager,
        END_OF_DAY_OFFSET_MINUTES,
    )
}

pub fn pack_tasks_with_end_of_day_offset_minutes(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    end_of_day_offset_minutes: i64,
) -> Result<PackResult, ApplicationError> {
    let now = repository.get_last_synced_time();
    let first_date = try_subjective_date(now)?;
    let target_dates = (0..PACK_TARGET_DAYS)
        .map(|days| {
            first_date.checked_add_signed(Duration::days(days)).ok_or(
                ApplicationError::SubjectiveDateOutOfRange {
                    operation: "pack_target_dates",
                    datetime: now,
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut candidates = collect_candidates(repository, &target_dates)?;
    candidates.sort_by_key(|candidate| {
        (
            Reverse(candidate.priority),
            candidate.planned_start,
            candidate.task_id,
        )
    });

    let mut result = PackResult::default();
    for candidate in candidates {
        let mut packed_task_opt = None;
        let current_planned_start_opt = get_schedule(repository)?
            .into_iter()
            .find(|scheduled| scheduled.task.id == candidate.task_id)
            .map(|scheduled| scheduled.scheduled_start);
        let Some(current_planned_start) = current_planned_start_opt else {
            continue;
        };
        let daily_leeway = calculate_daily_leeway(
            repository,
            free_time_manager,
            &target_dates,
            end_of_day_offset_minutes,
        )?;

        for target_date in &target_dates {
            if try_subjective_date(current_planned_start)? <= *target_date
                || daily_leeway.get(target_date).copied().unwrap_or(0) < candidate.work_seconds
            {
                continue;
            }

            let Some(task) = repository
                .get_by_id(candidate.task_id)
                .map_err(ApplicationError::TaskTree)?
            else {
                continue;
            };
            let target_datetime = try_subjective_date_start(*target_date)?
                .max(task.get_start_time().map_err(ApplicationError::TaskTree)?);
            if try_subjective_date(target_datetime)? != *target_date {
                continue;
            }

            let target_day = PackTargetDay {
                date: *target_date,
                end: try_subjective_date_end(*target_date, end_of_day_offset_minutes)?,
            };
            let placement_start_opt = find_placement_start(
                repository,
                free_time_manager,
                candidate.task_id,
                target_datetime,
                target_day,
                candidate.work_seconds,
                task.get_atomic().map_err(ApplicationError::TaskTree)?,
            )?;

            if let Some(placement_start) =
                placement_start_opt.filter(|start| *start < current_planned_start)
            {
                let source_date = try_subjective_date(current_planned_start)?;
                task.set_pending_until(placement_start)
                    .map_err(ApplicationError::TaskTree)?;
                packed_task_opt = Some(PackedTask {
                    task_id: candidate.task_id,
                    name: candidate.name.clone(),
                    priority: candidate.priority,
                    source_date,
                    target_date: *target_date,
                    work_seconds: candidate.work_seconds,
                });
                break;
            }
        }

        match packed_task_opt {
            Some(packed_task) => result.packed_tasks.push(packed_task),
            None => {
                result.skipped_tasks.push(SkippedTask {
                    task_id: candidate.task_id,
                    name: candidate.name,
                    priority: candidate.priority,
                    required_work_seconds: candidate.work_seconds,
                });
            }
        }
    }

    Ok(result)
}

fn find_placement_start(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    task_id: Uuid,
    first_available_time: DateTime<Local>,
    target_day: PackTargetDay,
    work_seconds: i64,
    atomic: bool,
) -> Result<Option<DateTime<Local>>, ApplicationError> {
    let target_end = target_day.end;
    let mut trial_time = first_available_time.max(repository.get_last_synced_time());

    while trial_time + Duration::seconds(work_seconds) <= target_end {
        if atomic {
            let Some(next_free_time) = find_next_continuous_free_time(
                free_time_manager,
                trial_time,
                target_end,
                work_seconds,
            ) else {
                return Ok(None);
            };
            trial_time = next_free_time;
        }
        let schedule =
            get_schedule_with_task_first_available_time(repository, task_id, trial_time)?;
        let task_segments = schedule
            .iter()
            .filter(|scheduled| scheduled.task.id == task_id)
            .collect::<Vec<_>>();

        if placement_fits_target_day(
            &task_segments,
            target_day,
            work_seconds,
            atomic,
            free_time_manager,
        )? {
            return Ok(task_segments
                .first()
                .map(|scheduled| scheduled.scheduled_start));
        }

        if !atomic {
            return Ok(None);
        }
        trial_time = task_segments
            .first()
            .map_or(trial_time + Duration::minutes(1), |scheduled| {
                (trial_time + Duration::minutes(1))
                    .max(scheduled.scheduled_start + Duration::minutes(1))
            });
    }

    Ok(None)
}

fn find_next_continuous_free_time(
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    mut cursor: DateTime<Local>,
    target_end: DateTime<Local>,
    work_seconds: i64,
) -> Option<DateTime<Local>> {
    let required_minutes = (work_seconds + 59) / 60;
    let check_duration = Duration::minutes(required_minutes);

    while cursor + check_duration <= target_end {
        if free_time_manager.get_free_minutes(&cursor, &(cursor + check_duration))
            >= required_minutes
        {
            return Some(cursor);
        }
        cursor += Duration::minutes(1);
    }

    None
}

fn collect_candidates(
    repository: &dyn TaskRepositoryTrait,
    target_dates: &[NaiveDate],
) -> Result<Vec<PackCandidate>, ApplicationError> {
    let schedule = get_schedule(repository)?;
    let mut seen_ids = HashSet::new();
    let mut candidates = Vec::new();
    for scheduled in schedule {
        if !seen_ids.insert(scheduled.task.id) {
            continue;
        }
        if scheduled.rank != 0
            || scheduled.task.status != Status::Pending
            || scheduled.task.is_on_other_side
            || scheduled.total_work_seconds <= 0
        {
            continue;
        }
        let scheduled_date = try_subjective_date(scheduled.scheduled_start)?;
        let task_start_date = try_subjective_date(scheduled.task.start_time)?;
        if target_dates
            .iter()
            .any(|target_date| *target_date < scheduled_date && task_start_date <= *target_date)
        {
            candidates.push(PackCandidate {
                task_id: scheduled.task.id,
                name: scheduled.task.name,
                priority: scheduled.task.priority,
                planned_start: scheduled.scheduled_start,
                work_seconds: scheduled.total_work_seconds,
            });
        }
    }
    Ok(candidates)
}

fn calculate_daily_leeway(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    target_dates: &[NaiveDate],
    end_of_day_offset_minutes: i64,
) -> Result<HashMap<NaiveDate, i64>, ApplicationError> {
    let mut total_work_seconds = HashMap::<NaiveDate, i64>::new();
    let mut repetitive_work_seconds = HashMap::<NaiveDate, i64>::new();

    for scheduled in get_schedule(repository)? {
        let date = try_subjective_date(scheduled.scheduled_start)?;
        if !target_dates.contains(&date) {
            continue;
        }
        *total_work_seconds.entry(date).or_default() += scheduled.scheduled_work_seconds;
        if repository
            .get_by_id(scheduled.task.id)
            .map_err(ApplicationError::TaskTree)?
            .map(|task| {
                task.get_inherited_repetition_interval_days_opt()
                    .map(|interval| interval.is_some())
            })
            .transpose()
            .map_err(ApplicationError::TaskTree)?
            .unwrap_or(false)
        {
            *repetitive_work_seconds.entry(date).or_default() += scheduled.scheduled_work_seconds;
        }
    }

    target_dates
        .iter()
        .map(|date| {
            let free_time_minutes =
                calculate_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
                    date,
                    repository.get_last_synced_time(),
                    free_time_manager,
                    end_of_day_offset_minutes,
                )?;
            let repetitive = repetitive_work_seconds.get(date).copied().unwrap_or(0);
            let total = total_work_seconds.get(date).copied().unwrap_or(0);
            Ok((
                *date,
                calculate_daily_leeway_seconds(free_time_minutes, repetitive, total),
            ))
        })
        .collect()
}

fn placement_fits_target_day(
    task_segments: &[&ScheduledTaskView],
    target_day: PackTargetDay,
    work_seconds: i64,
    atomic: bool,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<bool, ApplicationError> {
    let target_end = target_day.end;
    let scheduled_dates = task_segments
        .iter()
        .map(|scheduled| try_subjective_date(scheduled.scheduled_start))
        .collect::<Result<Vec<_>, _>>()?;
    let fits_in_day = !task_segments.is_empty()
        && task_segments
            .iter()
            .zip(scheduled_dates)
            .all(|(scheduled, scheduled_date)| {
                scheduled_date == target_day.date && scheduled.scheduled_end <= target_end
            })
        && task_segments
            .iter()
            .map(|scheduled| scheduled.scheduled_work_seconds)
            .sum::<i64>()
            == work_seconds;

    if !fits_in_day || !atomic || task_segments.len() != 1 {
        return Ok(fits_in_day && !atomic);
    }

    let scheduled = task_segments[0];
    let required_minutes = (work_seconds + 59) / 60;
    let free_time_check_end = scheduled.scheduled_start + Duration::minutes(required_minutes);
    Ok(
        free_time_manager.get_free_minutes(&scheduled.scheduled_start, &free_time_check_end)
            >= required_minutes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::task::{Status, TaskHandle};
    use crate::test_support::{TestFreeTimeManager, TestTaskRepository};
    use chrono::{DateTime, Duration, FixedOffset, Local, NaiveDate, TimeZone};
    use uuid::Uuid;

    fn fixed_now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap()
    }

    fn pending_task(
        name: &str,
        now: DateTime<Local>,
        pending_until: DateTime<Local>,
        work_minutes: i64,
        priority: i64,
    ) -> TaskHandle {
        let task = crate::test_support::new_task_handle(name).unwrap();
        task.sync_clock(now).unwrap();
        task.set_start_time(now).unwrap();
        task.set_estimated_work_seconds(work_minutes * 60).unwrap();
        task.set_priority(priority).unwrap();
        task.set_pending_until(pending_until).unwrap();
        task.set_orig_status(Status::Pending).unwrap();
        task
    }

    #[test]
    fn pack_tasksはoperation時刻のsubjective_date計算不能を伝搬しtaskを変更しない() {
        let local_datetime = NaiveDate::MIN.and_hms_opt(5, 59, 0).unwrap();
        let now = DateTime::<Local>::from_naive_utc_and_offset(
            local_datetime,
            FixedOffset::east_opt(0).unwrap(),
        );
        let task = crate::test_support::new_task_handle("対象").unwrap();
        let original_revision = task.get_persistent_mutation_revision().unwrap();
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = pack_tasks_with_end_of_day_offset_minutes(
            &repository,
            &mut free_time_manager,
            END_OF_DAY_OFFSET_MINUTES,
        );

        assert_eq!(
            actual,
            Err(ApplicationError::SubjectiveDateOutOfRange {
                operation: "subjective_date",
                datetime: now,
            })
        );
        assert_eq!(
            task.get_persistent_mutation_revision().unwrap(),
            original_revision
        );
    }

    #[test]
    fn pack_tasks_優先度が高い順に今日の余差へ前倒しする() {
        let now = fixed_now();
        let low = pending_task("低", now, now + Duration::days(10), 30, 1);
        let high = pending_task("高", now, now + Duration::days(10), 30, 9);
        let repository = TestTaskRepository::new(vec![low.clone(), high.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(
            actual
                .packed_tasks
                .iter()
                .map(|packed| packed.task_id)
                .collect::<Vec<_>>(),
            vec![high.get_id().unwrap(), low.get_id().unwrap()]
        );
        assert!(actual
            .packed_tasks
            .iter()
            .all(|packed| packed.target_date == NaiveDate::from_ymd_opt(2026, 8, 11).unwrap()));
        assert_eq!(high.get_start_time().unwrap(), now);
        assert!(high.get_pending_until().unwrap() < now + Duration::days(10));
    }

    #[test]
    fn pack_tasks_同じ優先度では現在の予定日時が早い順に詰める() {
        let now = fixed_now();
        let later = pending_task("後", now, now + Duration::days(11), 30, 5);
        let earlier = pending_task("先", now, now + Duration::days(10), 30, 5);
        let repository = TestTaskRepository::new(vec![later.clone(), earlier.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(
            actual
                .packed_tasks
                .iter()
                .map(|packed| packed.task_id)
                .collect::<Vec<_>>(),
            vec![earlier.get_id().unwrap(), later.get_id().unwrap()]
        );
    }

    #[test]
    fn pack_tasks_優先度と予定日時が同じならuuid昇順に詰める() {
        let now = fixed_now();
        let mut larger_id = pending_task("後", now, now + Duration::days(10), 30, 5);
        larger_id
            .set_id(Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap())
            .unwrap();
        let mut smaller_id = pending_task("先", now, now + Duration::days(10), 30, 5);
        smaller_id
            .set_id(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap())
            .unwrap();
        let repository = TestTaskRepository::new(vec![larger_id.clone(), smaller_id.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(
            actual
                .packed_tasks
                .iter()
                .map(|packed| packed.task_id)
                .collect::<Vec<_>>(),
            vec![smaller_id.get_id().unwrap(), larger_id.get_id().unwrap()]
        );
    }

    #[test]
    fn pack_tasks_配置ごとに余差を再計算して収まらないtaskをスキップする() {
        let now = fixed_now();
        let first = pending_task("1", now, now + Duration::days(1), 30, 3);
        let second = pending_task("2", now, now + Duration::days(1), 30, 2);
        let third = pending_task("3", now, now + Duration::days(1), 30, 1);
        let fourth = pending_task("4", now, now + Duration::days(1), 15, 0);
        let repository = TestTaskRepository::new(
            vec![first.clone(), second.clone(), third.clone(), fourth.clone()],
            now,
        );
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(
            actual
                .packed_tasks
                .iter()
                .map(|packed| packed.task_id)
                .collect::<Vec<_>>(),
            vec![
                first.get_id().unwrap(),
                second.get_id().unwrap(),
                fourth.get_id().unwrap()
            ]
        );
        assert_eq!(actual.skipped_tasks.len(), 1);
        assert_eq!(actual.skipped_tasks[0].task_id, third.get_id().unwrap());
        assert_eq!(third.get_pending_until().unwrap(), now + Duration::days(1));
    }

    #[test]
    fn pack_tasks_先行配置後の最新予定で後続taskの前倒し可否を判定する() {
        let now = Local.with_ymd_and_hms(2026, 8, 11, 6, 0, 0).unwrap();
        let first = pending_task("18時間20分", now, now + Duration::days(1), 18 * 60 + 20, 9);
        let second = pending_task("後続", now, now + Duration::days(1), 30, 8);
        let original_second_pending_until = second.get_pending_until().unwrap();
        let repository = TestTaskRepository::new(vec![first.clone(), second.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(40 * 60);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(actual.packed_tasks[0].task_id, first.get_id().unwrap());
        assert_eq!(actual.skipped_tasks.len(), 1);
        assert_eq!(actual.skipped_tasks[0].task_id, second.get_id().unwrap());
        assert_eq!(
            second.get_pending_until().unwrap(),
            original_second_pending_until
        );
    }

    #[test]
    fn pack_tasks_優先度がi64最小値でも前倒しする() {
        let now = fixed_now();
        let task = pending_task("最小", now, now + Duration::days(10), 30, i64::MIN);
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(actual.packed_tasks[0].task_id, task.get_id().unwrap());
    }

    #[test]
    fn pack_tasks_pending_untilを実際の配置開始時刻へ設定する() {
        let now = fixed_now();
        let blocker = crate::test_support::new_task_handle("先行").unwrap();
        blocker.sync_clock(now).unwrap();
        blocker.set_start_time(now).unwrap();
        blocker.set_estimated_work_seconds(30 * 60).unwrap();
        blocker.set_priority(10).unwrap();
        let candidate = pending_task("対象", now, now + Duration::days(10), 30, 9);
        let repository = TestTaskRepository::new(vec![blocker, candidate.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(180);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(candidate.get_start_time().unwrap(), now);
        assert_eq!(
            candidate.get_pending_until().unwrap(),
            now + Duration::minutes(30)
        );
    }

    #[test]
    fn pack_tasks_最優先候補が収まらなければスキップして低優先度候補を詰める() {
        let now = fixed_now();
        let low = pending_task("低", now, now + Duration::days(10), 30, 1);
        let high = pending_task("高", now, now + Duration::days(10), 60, 9);
        let original_low_pending_until = low.get_pending_until().unwrap();
        let original_high_revision = high.get_persistent_mutation_revision().unwrap();
        let repository = TestTaskRepository::new(vec![low.clone(), high.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(actual.packed_tasks[0].task_id, low.get_id().unwrap());
        assert_eq!(actual.skipped_tasks.len(), 1);
        assert_eq!(actual.skipped_tasks[0].task_id, high.get_id().unwrap());
        assert!(low.get_pending_until().unwrap() < original_low_pending_until);
        assert_eq!(
            high.get_persistent_mutation_revision().unwrap(),
            original_high_revision
        );
    }

    #[test]
    fn pack_tasks_7日合計には収まっても単一日の余差に収まらなければスキップする() {
        let now = fixed_now();
        let task = pending_task("長い", now, now + Duration::days(10), 60, 9);
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert!(actual.packed_tasks.is_empty());
        assert_eq!(actual.skipped_tasks.len(), 1);
        assert_eq!(actual.skipped_tasks[0].task_id, task.get_id().unwrap());
    }

    #[test]
    fn pack_tasks_対象期間は06時区切りの今日から7日間とする() {
        let now = Local.with_ymd_and_hms(2026, 8, 12, 1, 0, 0).unwrap();
        let task = pending_task("対象", now, now + Duration::days(10), 30, 9);
        task.set_start_time(Local.with_ymd_and_hms(2026, 8, 17, 6, 0, 0).unwrap())
            .unwrap();
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(
            actual.packed_tasks[0].target_date,
            NaiveDate::from_ymd_opt(2026, 8, 17).unwrap()
        );
    }

    #[test]
    fn pack_tasks_8日目から着手可能なtaskは対象外にする() {
        let now = Local.with_ymd_and_hms(2026, 8, 12, 1, 0, 0).unwrap();
        let task = pending_task("対象外", now, now + Duration::days(10), 30, 9);
        task.set_start_time(Local.with_ymd_and_hms(2026, 8, 18, 6, 0, 0).unwrap())
            .unwrap();
        let repository = TestTaskRepository::new(vec![task], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert!(actual.packed_tasks.is_empty());
        assert!(actual.skipped_tasks.is_empty());
    }

    #[test]
    fn pack_tasks_締切と依存と反復設定を変更しない() {
        let now = fixed_now();
        let task = pending_task("対象", now, now + Duration::days(10), 30, 9);
        let deadline = now + Duration::days(20);
        task.set_deadline_time_opt(Some(deadline)).unwrap();
        task.set_repetition_interval_days_opt(Some(7)).unwrap();
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(task.get_deadline_time_opt().unwrap(), Some(deadline));
        assert_eq!(task.get_repetition_interval_days_opt().unwrap(), Some(7));
        assert!(task.get_children().unwrap().is_empty());
    }

    #[test]
    fn pack_tasks_atomicは初期予定枠に行動不能時間が重なれば同日後刻へ前倒しする() {
        let now = fixed_now();
        let task = pending_task("atomic", now, now + Duration::days(10), 60, 9);
        task.set_atomic(true).unwrap();
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::with_blocked_interval(
            180,
            now + Duration::minutes(30),
            now + Duration::minutes(90),
        );

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(
            actual.packed_tasks[0].target_date,
            NaiveDate::from_ymd_opt(2026, 8, 11).unwrap()
        );
        assert_eq!(
            task.get_pending_until().unwrap(),
            now + Duration::minutes(90)
        );
    }

    #[test]
    fn pack_tasks_atomicは同日後刻の連続空き枠へ前倒しする() {
        let now = fixed_now();
        let task = pending_task("atomic", now, now + Duration::days(10), 60, 9);
        task.set_atomic(true).unwrap();
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager =
            TestFreeTimeManager::with_blocked_interval(180, now, now + Duration::hours(1));

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(
            actual.packed_tasks[0].target_date,
            NaiveDate::from_ymd_opt(2026, 8, 11).unwrap()
        );
        assert_eq!(task.get_pending_until().unwrap(), now + Duration::hours(1));
    }

    #[test]
    fn pack_tasks_atomicは初日に連続空き枠がなければ翌日へ前倒しする() {
        let now = fixed_now();
        let task = pending_task("atomic", now, now + Duration::days(10), 60, 9);
        task.set_atomic(true).unwrap();
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::with_blocked_interval(
            180,
            now,
            Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap(),
        );

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(
            actual.packed_tasks[0].target_date,
            NaiveDate::from_ymd_opt(2026, 8, 12).unwrap()
        );
    }

    #[test]
    fn pack_tasks_atomicは残作業に秒端数があっても空き枠へ前倒しする() {
        let now = fixed_now();
        let task = pending_task("atomic", now, now + Duration::days(10), 30, 9);
        task.set_actual_work_seconds(1).unwrap();
        task.set_atomic(true).unwrap();
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(actual.packed_tasks[0].work_seconds, 30 * 60 - 1);
        assert_eq!(task.get_pending_until().unwrap(), now);
    }

    #[test]
    fn pack_tasks_atomicに連続空き枠がなければスキップして次のtaskを詰める() {
        let now = fixed_now();
        let atomic = pending_task("atomic", now, now + Duration::days(10), 60, 9);
        atomic.set_atomic(true).unwrap();
        let next = pending_task("次", now, now + Duration::days(10), 30, 8);
        let repository = TestTaskRepository::new(vec![atomic.clone(), next.clone()], now);
        let mut free_time_manager =
            TestFreeTimeManager::with_blocked_interval(180, now, now + Duration::days(7));

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(actual.skipped_tasks.len(), 1);
        assert_eq!(actual.skipped_tasks[0].task_id, atomic.get_id().unwrap());
        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(actual.packed_tasks[0].task_id, next.get_id().unwrap());
    }

    #[test]
    fn pack_tasks_複数taskをスキップして収まる次のtaskを詰める() {
        let now = fixed_now();
        let first = pending_task("1", now, now + Duration::days(10), 60, 9);
        let second = pending_task("2", now, now + Duration::days(10), 60, 8);
        let third = pending_task("3", now, now + Duration::days(10), 30, 7);
        let repository =
            TestTaskRepository::new(vec![first.clone(), second.clone(), third.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(
            actual
                .skipped_tasks
                .iter()
                .map(|skipped| skipped.task_id)
                .collect::<Vec<_>>(),
            vec![first.get_id().unwrap(), second.get_id().unwrap()]
        );
        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(actual.packed_tasks[0].task_id, third.get_id().unwrap());
    }

    #[test]
    fn pack_tasks_相手待ちや着手可能日が期間外のtaskは候補にしない() {
        let now = fixed_now();
        let waiting = pending_task("待ち", now, now + Duration::days(10), 30, 9);
        waiting.set_is_on_other_side(true).unwrap();
        let future = pending_task("未来", now, now + Duration::days(10), 30, 8);
        future.set_start_time(now + Duration::days(8)).unwrap();
        let repository = TestTaskRepository::new(vec![waiting, future], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert!(actual.packed_tasks.is_empty());
        assert!(actual.skipped_tasks.is_empty());
    }

    #[test]
    fn pack_tasks_親taskと完了済みtaskと残作業0のtaskは候補にしない() {
        let now = fixed_now();
        let parent = pending_task("親", now, now + Duration::days(10), 30, 9);
        let child = parent.create_as_last_child(crate::test_support::new_task_attr("子"));
        child.sync_clock(now).unwrap();
        let done = pending_task("完了", now, now + Duration::days(10), 30, 8);
        done.set_orig_status(Status::Done).unwrap();
        let zero = pending_task("ゼロ", now, now + Duration::days(10), 0, 7);
        let repository = TestTaskRepository::new(vec![parent, done, zero], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert!(actual.packed_tasks.is_empty());
        assert!(actual.skipped_tasks.is_empty());
    }

    #[test]
    fn pack_tasks_候補外の親taskの範囲外start_timeは正常な葉taskの配置を妨げない() {
        let now = fixed_now();
        let parent = pending_task("親", now, now + Duration::days(10), 30, 1);
        let out_of_range_start = DateTime::<Local>::from_naive_utc_and_offset(
            NaiveDate::MIN.and_hms_opt(5, 59, 0).unwrap(),
            FixedOffset::east_opt(0).unwrap(),
        );
        parent.set_start_time(out_of_range_start).unwrap();
        let child = parent.create_as_last_child(crate::test_support::new_task_attr("子"));
        child.sync_clock(now).unwrap();
        child.set_start_time(now).unwrap();
        child.set_estimated_work_seconds(30 * 60).unwrap();

        let leaf = pending_task("正常な葉", now, now + Duration::days(10), 30, 9);
        let repository = TestTaskRepository::new(vec![parent, leaf.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(actual.packed_tasks[0].task_id, leaf.get_id().unwrap());
        assert!(leaf.get_pending_until().unwrap() < now + Duration::days(10));
    }
}
