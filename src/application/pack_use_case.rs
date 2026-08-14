use super::daily_capacity::{
    calculate_daily_leeway_seconds,
    calculate_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes,
    subjective_date, subjective_date_end, subjective_date_start, END_OF_DAY_OFFSET_MINUTES,
};
use super::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use super::schedule_use_case::{
    get_schedule, get_schedule_with_task_first_available_time, ScheduledTaskView,
};
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
) -> PackResult {
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
) -> PackResult {
    let now = repository.get_last_synced_time();
    let first_date = subjective_date(now);
    let target_dates = (0..PACK_TARGET_DAYS)
        .map(|days| first_date + Duration::days(days))
        .collect::<Vec<_>>();
    let mut candidates = collect_candidates(repository, &target_dates);
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
        let current_planned_start_opt = get_schedule(repository)
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
        );

        for target_date in &target_dates {
            if subjective_date(current_planned_start) <= *target_date
                || daily_leeway.get(target_date).copied().unwrap_or(0) < candidate.work_seconds
            {
                continue;
            }

            let Some(task) = repository.get_by_id(candidate.task_id) else {
                continue;
            };
            let target_datetime = subjective_date_start(*target_date).max(task.get_start_time());
            if subjective_date(target_datetime) != *target_date {
                continue;
            }

            let target_day = PackTargetDay {
                date: *target_date,
                end: subjective_date_end(*target_date, end_of_day_offset_minutes),
            };
            let placement_start_opt = find_placement_start(
                repository,
                free_time_manager,
                candidate.task_id,
                target_datetime,
                target_day,
                candidate.work_seconds,
                task.get_atomic(),
            );

            if let Some(placement_start) =
                placement_start_opt.filter(|start| *start < current_planned_start)
            {
                task.set_pending_until(placement_start);
                packed_task_opt = Some(PackedTask {
                    task_id: candidate.task_id,
                    name: candidate.name.clone(),
                    priority: candidate.priority,
                    source_date: subjective_date(current_planned_start),
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

    result
}

fn find_placement_start(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    task_id: Uuid,
    first_available_time: DateTime<Local>,
    target_day: PackTargetDay,
    work_seconds: i64,
    atomic: bool,
) -> Option<DateTime<Local>> {
    let target_end = target_day.end;
    let mut trial_time = first_available_time.max(repository.get_last_synced_time());

    while trial_time + Duration::seconds(work_seconds) <= target_end {
        if atomic {
            trial_time = find_next_continuous_free_time(
                free_time_manager,
                trial_time,
                target_end,
                work_seconds,
            )?;
        }
        let schedule = get_schedule_with_task_first_available_time(repository, task_id, trial_time);
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
        ) {
            return task_segments
                .first()
                .map(|scheduled| scheduled.scheduled_start);
        }

        if !atomic {
            return None;
        }
        trial_time = task_segments
            .first()
            .map_or(trial_time + Duration::minutes(1), |scheduled| {
                (trial_time + Duration::minutes(1))
                    .max(scheduled.scheduled_start + Duration::minutes(1))
            });
    }

    None
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
) -> Vec<PackCandidate> {
    let schedule = get_schedule(repository);
    let mut seen_ids = HashSet::new();
    schedule
        .into_iter()
        .filter(|scheduled| seen_ids.insert(scheduled.task.id))
        .filter(|scheduled| {
            scheduled.rank == 0
                && scheduled.task.status == Status::Pending
                && !scheduled.task.is_on_other_side
                && scheduled.total_work_seconds > 0
                && target_dates.iter().any(|target_date| {
                    *target_date < subjective_date(scheduled.scheduled_start)
                        && subjective_date(scheduled.task.start_time) <= *target_date
                })
        })
        .map(|scheduled| PackCandidate {
            task_id: scheduled.task.id,
            name: scheduled.task.name,
            priority: scheduled.task.priority,
            planned_start: scheduled.scheduled_start,
            work_seconds: scheduled.total_work_seconds,
        })
        .collect()
}

fn calculate_daily_leeway(
    repository: &dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    target_dates: &[NaiveDate],
    end_of_day_offset_minutes: i64,
) -> HashMap<NaiveDate, i64> {
    let mut total_work_seconds = HashMap::<NaiveDate, i64>::new();
    let mut repetitive_work_seconds = HashMap::<NaiveDate, i64>::new();

    for scheduled in get_schedule(repository) {
        let date = subjective_date(scheduled.scheduled_start);
        if !target_dates.contains(&date) {
            continue;
        }
        *total_work_seconds.entry(date).or_default() += scheduled.scheduled_work_seconds;
        if repository
            .get_by_id(scheduled.task.id)
            .is_some_and(|task| task.get_inherited_repetition_interval_days_opt().is_some())
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
                );
            let repetitive = repetitive_work_seconds.get(date).copied().unwrap_or(0);
            let total = total_work_seconds.get(date).copied().unwrap_or(0);
            (
                *date,
                calculate_daily_leeway_seconds(free_time_minutes, repetitive, total),
            )
        })
        .collect()
}

fn placement_fits_target_day(
    task_segments: &[&ScheduledTaskView],
    target_day: PackTargetDay,
    work_seconds: i64,
    atomic: bool,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> bool {
    let target_end = target_day.end;
    let fits_in_day = !task_segments.is_empty()
        && task_segments.iter().all(|scheduled| {
            subjective_date(scheduled.scheduled_start) == target_day.date
                && scheduled.scheduled_end <= target_end
        })
        && task_segments
            .iter()
            .map(|scheduled| scheduled.scheduled_work_seconds)
            .sum::<i64>()
            == work_seconds;

    if !fits_in_day || !atomic || task_segments.len() != 1 {
        return fits_in_day && !atomic;
    }

    let scheduled = task_segments[0];
    let required_minutes = (work_seconds + 59) / 60;
    let free_time_check_end = scheduled.scheduled_start + Duration::minutes(required_minutes);
    free_time_manager.get_free_minutes(&scheduled.scheduled_start, &free_time_check_end)
        >= required_minutes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::interface::{
        FreeTimeManagerTrait, TaskRepositoryError, TaskRepositoryTrait,
    };
    use crate::entity::task::{Status, TaskHandle};
    use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone};
    use std::cell::Cell;
    use uuid::Uuid;

    struct TestTaskRepository {
        projects: Vec<TaskHandle>,
        now: DateTime<Local>,
        save_count: Cell<usize>,
    }

    impl TestTaskRepository {
        fn new(projects: Vec<TaskHandle>, now: DateTime<Local>) -> Self {
            Self {
                projects,
                now,
                save_count: Cell::new(0),
            }
        }
    }

    impl TaskRepositoryTrait for TestTaskRepository {
        fn get_project_storage_dir_name(&self) -> &str {
            "unused"
        }

        fn get_all_projects(&self) -> Vec<&TaskHandle> {
            self.projects.iter().collect()
        }

        fn load(&mut self) -> Result<(), TaskRepositoryError> {
            Ok(())
        }

        fn save(&self) -> Result<(), TaskRepositoryError> {
            self.save_count.set(self.save_count.get() + 1);
            Ok(())
        }

        fn sync_clock(&mut self, now: DateTime<Local>) {
            self.now = now;
            for project in &self.projects {
                project.sync_clock(now);
            }
        }

        fn get_last_synced_time(&self) -> DateTime<Local> {
            self.now
        }

        fn get_highest_priority_project(&mut self) -> Option<&TaskHandle> {
            self.projects.first()
        }

        fn get_highest_priority_leaf_task_id(&mut self) -> Option<Uuid> {
            None
        }

        fn get_defer_candidate_leaf_task_id(&mut self, _recent_days: i64) -> Option<Uuid> {
            None
        }

        fn get_by_id(&self, id: Uuid) -> Option<TaskHandle> {
            self.projects.iter().find_map(|task| task.get_by_id(id))
        }

        fn start_new_project(&mut self, root_task: TaskHandle) {
            self.projects.push(root_task);
        }
    }

    struct TestFreeTimeManager {
        daily_free_minutes: i64,
        blocked_interval: Option<(DateTime<Local>, DateTime<Local>)>,
    }

    impl TestFreeTimeManager {
        fn new(daily_free_minutes: i64) -> Self {
            Self {
                daily_free_minutes,
                blocked_interval: None,
            }
        }

        fn with_blocked_interval(
            daily_free_minutes: i64,
            start: DateTime<Local>,
            end: DateTime<Local>,
        ) -> Self {
            Self {
                daily_free_minutes,
                blocked_interval: Some((start, end)),
            }
        }
    }

    impl FreeTimeManagerTrait for TestFreeTimeManager {
        fn get_free_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
            let duration_minutes = (*end - *start).num_minutes().max(0);
            if duration_minutes >= 12 * 60 {
                return self.daily_free_minutes;
            }

            let blocked_minutes = self
                .blocked_interval
                .map(|(blocked_start, blocked_end)| {
                    let overlap_start = (*start).max(blocked_start);
                    let overlap_end = (*end).min(blocked_end);
                    (overlap_end - overlap_start).num_minutes().max(0)
                })
                .unwrap_or(0);
            duration_minutes - blocked_minutes
        }

        fn get_busy_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
            (*end - *start).num_minutes() - self.get_free_minutes(start, end)
        }

        fn register_busy_time_slot(
            &mut self,
            _start: &DateTime<Local>,
            _end: &DateTime<Local>,
        ) -> Result<(), crate::application::interface::BusyTimeSlotRegistrationError> {
            Ok(())
        }

        fn load_busy_time_slots_from_file(
            &mut self,
            _busy_time_slots_file_path: &str,
        ) -> Result<(), crate::application::interface::BusyTimeSlotLoadError> {
            Ok(())
        }
    }

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
        let task = TaskHandle::new(name);
        task.sync_clock(now);
        task.set_start_time(now);
        task.set_estimated_work_seconds(work_minutes * 60);
        task.set_priority(priority);
        task.set_pending_until(pending_until);
        task.set_orig_status(Status::Pending);
        task
    }

    #[test]
    fn pack_tasks_優先度が高い順に今日の余差へ前倒しする() {
        let now = fixed_now();
        let low = pending_task("低", now, now + Duration::days(10), 30, 1);
        let high = pending_task("高", now, now + Duration::days(10), 30, 9);
        let repository = TestTaskRepository::new(vec![low.clone(), high.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert_eq!(
            actual
                .packed_tasks
                .iter()
                .map(|packed| packed.task_id)
                .collect::<Vec<_>>(),
            vec![high.get_id(), low.get_id()]
        );
        assert!(actual
            .packed_tasks
            .iter()
            .all(|packed| packed.target_date == NaiveDate::from_ymd_opt(2026, 8, 11).unwrap()));
        assert_eq!(high.get_start_time(), now);
        assert!(high.get_pending_until() < now + Duration::days(10));
    }

    #[test]
    fn pack_tasks_同じ優先度では現在の予定日時が早い順に詰める() {
        let now = fixed_now();
        let later = pending_task("後", now, now + Duration::days(11), 30, 5);
        let earlier = pending_task("先", now, now + Duration::days(10), 30, 5);
        let repository = TestTaskRepository::new(vec![later.clone(), earlier.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert_eq!(
            actual
                .packed_tasks
                .iter()
                .map(|packed| packed.task_id)
                .collect::<Vec<_>>(),
            vec![earlier.get_id(), later.get_id()]
        );
    }

    #[test]
    fn pack_tasks_優先度と予定日時が同じならuuid昇順に詰める() {
        let now = fixed_now();
        let mut larger_id = pending_task("後", now, now + Duration::days(10), 30, 5);
        larger_id.set_id(Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap());
        let mut smaller_id = pending_task("先", now, now + Duration::days(10), 30, 5);
        smaller_id.set_id(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap());
        let repository = TestTaskRepository::new(vec![larger_id.clone(), smaller_id.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert_eq!(
            actual
                .packed_tasks
                .iter()
                .map(|packed| packed.task_id)
                .collect::<Vec<_>>(),
            vec![smaller_id.get_id(), larger_id.get_id()]
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

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert_eq!(
            actual
                .packed_tasks
                .iter()
                .map(|packed| packed.task_id)
                .collect::<Vec<_>>(),
            vec![first.get_id(), second.get_id(), fourth.get_id()]
        );
        assert_eq!(actual.skipped_tasks.len(), 1);
        assert_eq!(actual.skipped_tasks[0].task_id, third.get_id());
        assert_eq!(third.get_pending_until(), now + Duration::days(1));
    }

    #[test]
    fn pack_tasks_先行配置後の最新予定で後続taskの前倒し可否を判定する() {
        let now = Local.with_ymd_and_hms(2026, 8, 11, 6, 0, 0).unwrap();
        let first = pending_task("18時間20分", now, now + Duration::days(1), 18 * 60 + 20, 9);
        let second = pending_task("後続", now, now + Duration::days(1), 30, 8);
        let original_second_pending_until = second.get_pending_until();
        let repository = TestTaskRepository::new(vec![first.clone(), second.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(40 * 60);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(actual.packed_tasks[0].task_id, first.get_id());
        assert_eq!(actual.skipped_tasks.len(), 1);
        assert_eq!(actual.skipped_tasks[0].task_id, second.get_id());
        assert_eq!(second.get_pending_until(), original_second_pending_until);
    }

    #[test]
    fn pack_tasks_優先度がi64最小値でも前倒しする() {
        let now = fixed_now();
        let task = pending_task("最小", now, now + Duration::days(10), 30, i64::MIN);
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(actual.packed_tasks[0].task_id, task.get_id());
    }

    #[test]
    fn pack_tasks_pending_untilを実際の配置開始時刻へ設定する() {
        let now = fixed_now();
        let blocker = TaskHandle::new("先行");
        blocker.sync_clock(now);
        blocker.set_start_time(now);
        blocker.set_estimated_work_seconds(30 * 60);
        blocker.set_priority(10);
        let candidate = pending_task("対象", now, now + Duration::days(10), 30, 9);
        let repository = TestTaskRepository::new(vec![blocker, candidate.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(180);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(candidate.get_start_time(), now);
        assert_eq!(candidate.get_pending_until(), now + Duration::minutes(30));
    }

    #[test]
    fn pack_tasks_最優先候補が収まらなければスキップして低優先度候補を詰める() {
        let now = fixed_now();
        let low = pending_task("低", now, now + Duration::days(10), 30, 1);
        let high = pending_task("高", now, now + Duration::days(10), 60, 9);
        let original_low_pending_until = low.get_pending_until();
        let original_high_revision = high.get_persistent_mutation_revision();
        let repository = TestTaskRepository::new(vec![low.clone(), high.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(actual.packed_tasks[0].task_id, low.get_id());
        assert_eq!(actual.skipped_tasks.len(), 1);
        assert_eq!(actual.skipped_tasks[0].task_id, high.get_id());
        assert!(low.get_pending_until() < original_low_pending_until);
        assert_eq!(
            high.get_persistent_mutation_revision(),
            original_high_revision
        );
    }

    #[test]
    fn pack_tasks_7日合計には収まっても単一日の余差に収まらなければスキップする() {
        let now = fixed_now();
        let task = pending_task("長い", now, now + Duration::days(10), 60, 9);
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert!(actual.packed_tasks.is_empty());
        assert_eq!(actual.skipped_tasks.len(), 1);
        assert_eq!(actual.skipped_tasks[0].task_id, task.get_id());
    }

    #[test]
    fn pack_tasks_対象期間は06時区切りの今日から7日間とする() {
        let now = Local.with_ymd_and_hms(2026, 8, 12, 1, 0, 0).unwrap();
        let task = pending_task("対象", now, now + Duration::days(10), 30, 9);
        task.set_start_time(Local.with_ymd_and_hms(2026, 8, 17, 6, 0, 0).unwrap());
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = pack_tasks(&repository, &mut free_time_manager);

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
        task.set_start_time(Local.with_ymd_and_hms(2026, 8, 18, 6, 0, 0).unwrap());
        let repository = TestTaskRepository::new(vec![task], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert!(actual.packed_tasks.is_empty());
        assert!(actual.skipped_tasks.is_empty());
    }

    #[test]
    fn pack_tasks_締切と依存と反復設定を変更しない() {
        let now = fixed_now();
        let task = pending_task("対象", now, now + Duration::days(10), 30, 9);
        let deadline = now + Duration::days(20);
        task.set_deadline_time_opt(Some(deadline));
        task.set_repetition_interval_days_opt(Some(7));
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(task.get_deadline_time_opt(), Some(deadline));
        assert_eq!(task.get_repetition_interval_days_opt(), Some(7));
        assert!(task.get_children().is_empty());
    }

    #[test]
    fn pack_tasks_atomicは初期予定枠に行動不能時間が重なれば同日後刻へ前倒しする() {
        let now = fixed_now();
        let task = pending_task("atomic", now, now + Duration::days(10), 60, 9);
        task.set_atomic(true);
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::with_blocked_interval(
            180,
            now + Duration::minutes(30),
            now + Duration::minutes(90),
        );

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(
            actual.packed_tasks[0].target_date,
            NaiveDate::from_ymd_opt(2026, 8, 11).unwrap()
        );
        assert_eq!(task.get_pending_until(), now + Duration::minutes(90));
    }

    #[test]
    fn pack_tasks_atomicは同日後刻の連続空き枠へ前倒しする() {
        let now = fixed_now();
        let task = pending_task("atomic", now, now + Duration::days(10), 60, 9);
        task.set_atomic(true);
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager =
            TestFreeTimeManager::with_blocked_interval(180, now, now + Duration::hours(1));

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(
            actual.packed_tasks[0].target_date,
            NaiveDate::from_ymd_opt(2026, 8, 11).unwrap()
        );
        assert_eq!(task.get_pending_until(), now + Duration::hours(1));
    }

    #[test]
    fn pack_tasks_atomicは初日に連続空き枠がなければ翌日へ前倒しする() {
        let now = fixed_now();
        let task = pending_task("atomic", now, now + Duration::days(10), 60, 9);
        task.set_atomic(true);
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::with_blocked_interval(
            180,
            now,
            Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap(),
        );

        let actual = pack_tasks(&repository, &mut free_time_manager);

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
        task.set_actual_work_seconds(1);
        task.set_atomic(true);
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(actual.packed_tasks[0].work_seconds, 30 * 60 - 1);
        assert_eq!(task.get_pending_until(), now);
    }

    #[test]
    fn pack_tasks_atomicに連続空き枠がなければスキップして次のtaskを詰める() {
        let now = fixed_now();
        let atomic = pending_task("atomic", now, now + Duration::days(10), 60, 9);
        atomic.set_atomic(true);
        let next = pending_task("次", now, now + Duration::days(10), 30, 8);
        let repository = TestTaskRepository::new(vec![atomic.clone(), next.clone()], now);
        let mut free_time_manager =
            TestFreeTimeManager::with_blocked_interval(180, now, now + Duration::days(7));

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert_eq!(actual.skipped_tasks.len(), 1);
        assert_eq!(actual.skipped_tasks[0].task_id, atomic.get_id());
        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(actual.packed_tasks[0].task_id, next.get_id());
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

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert_eq!(
            actual
                .skipped_tasks
                .iter()
                .map(|skipped| skipped.task_id)
                .collect::<Vec<_>>(),
            vec![first.get_id(), second.get_id()]
        );
        assert_eq!(actual.packed_tasks.len(), 1);
        assert_eq!(actual.packed_tasks[0].task_id, third.get_id());
    }

    #[test]
    fn pack_tasks_相手待ちや着手可能日が期間外のtaskは候補にしない() {
        let now = fixed_now();
        let waiting = pending_task("待ち", now, now + Duration::days(10), 30, 9);
        waiting.set_is_on_other_side(true);
        let future = pending_task("未来", now, now + Duration::days(10), 30, 8);
        future.set_start_time(now + Duration::days(8));
        let repository = TestTaskRepository::new(vec![waiting, future], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert!(actual.packed_tasks.is_empty());
        assert!(actual.skipped_tasks.is_empty());
    }

    #[test]
    fn pack_tasks_親taskと完了済みtaskと残作業0のtaskは候補にしない() {
        let now = fixed_now();
        let parent = pending_task("親", now, now + Duration::days(10), 30, 9);
        let child = parent.create_as_last_child(crate::entity::task::TaskAttr::new("子"));
        child.sync_clock(now);
        let done = pending_task("完了", now, now + Duration::days(10), 30, 8);
        done.set_orig_status(Status::Done);
        let zero = pending_task("ゼロ", now, now + Duration::days(10), 0, 7);
        let repository = TestTaskRepository::new(vec![parent, done, zero], now);
        let mut free_time_manager = TestFreeTimeManager::new(120);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert!(actual.packed_tasks.is_empty());
        assert!(actual.skipped_tasks.is_empty());
    }
}
