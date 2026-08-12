#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::interface::{
        FreeTimeManagerTrait, TaskRepositoryError, TaskRepositoryTrait,
    };
    use crate::entity::task::{Status, Task};
    use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone};
    use std::cell::Cell;
    use uuid::Uuid;

    struct TestTaskRepository {
        projects: Vec<Task>,
        now: DateTime<Local>,
        save_count: Cell<usize>,
    }

    impl TestTaskRepository {
        fn new(projects: Vec<Task>, now: DateTime<Local>) -> Self {
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

        fn get_all_projects(&self) -> Vec<&Task> {
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

        fn get_highest_priority_project(&mut self) -> Option<&Task> {
            self.projects.first()
        }

        fn get_highest_priority_leaf_task_id(&mut self) -> Option<Uuid> {
            None
        }

        fn get_defer_candidate_leaf_task_id(&mut self, _recent_days: i64) -> Option<Uuid> {
            None
        }

        fn get_by_id(&self, id: Uuid) -> Option<Task> {
            self.projects.iter().find_map(|task| task.get_by_id(id))
        }

        fn start_new_project(&mut self, root_task: Task) {
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

        fn register_busy_time_slot(&mut self, _start: &DateTime<Local>, _end: &DateTime<Local>) {}

        fn load_busy_time_slots_from_file(
            &mut self,
            _busy_time_slots_file_path: &str,
            _now: &DateTime<Local>,
        ) {
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
    ) -> Task {
        let task = Task::new(name);
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
    fn pack_tasks_最優先候補がどの日にも収まらなければ低優先度候補を詰めず終了する() {
        let now = fixed_now();
        let low = pending_task("低", now, now + Duration::days(10), 30, 1);
        let high = pending_task("高", now, now + Duration::days(10), 60, 9);
        let original_low_pending_until = low.get_pending_until();
        let repository = TestTaskRepository::new(vec![low.clone(), high.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert!(actual.packed_tasks.is_empty());
        assert_eq!(actual.stopped.unwrap().task_id, high.get_id());
        assert_eq!(low.get_pending_until(), original_low_pending_until);
    }

    #[test]
    fn pack_tasks_7日合計には収まっても単一日の余差に収まらなければ終了する() {
        let now = fixed_now();
        let task = pending_task("長い", now, now + Duration::days(10), 60, 9);
        let repository = TestTaskRepository::new(vec![task.clone()], now);
        let mut free_time_manager = TestFreeTimeManager::new(60);

        let actual = pack_tasks(&repository, &mut free_time_manager);

        assert!(actual.packed_tasks.is_empty());
        assert_eq!(actual.stopped.unwrap().task_id, task.get_id());
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
    fn pack_tasks_atomicは予定枠に行動不能時間が重なる場合は前倒ししない() {
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

        assert!(actual.packed_tasks.is_empty());
        assert_eq!(actual.stopped.unwrap().task_id, task.get_id());
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
        assert!(actual.stopped.is_none());
    }
}
