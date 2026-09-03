#[path = "support/scheduling_harness.rs"]
#[allow(dead_code)]
mod scheduling_harness;

use chrono::{DateTime, Local, NaiveDate, TimeZone};
use scheduling_harness::SchedulingRepository;
use schronu::application::flatten_use_case::{flatten_tasks, UnresolvedReason};
use schronu::application::interface::{
    BusyTimeSlotLoadError, BusyTimeSlotRegistrationError, FreeTimeManagerTrait,
};
use schronu::application::pack_use_case::pack_tasks;
use schronu::application::schedule_use_case::get_schedule;
use schronu::entity::task::{Status, TaskHandle};
use uuid::Uuid;

const MINUTE_SECONDS: i64 = 60;
const FIXED_RESERVATION_MINUTES: i64 = 380;
const ACTUAL_WORK_MINUTES: i64 = 60;

#[derive(Clone, Copy)]
struct FixedCapacityFixture {
    operation_datetime: DateTime<Local>,
    busy_start: DateTime<Local>,
    busy_end: DateTime<Local>,
    fixed_id: Uuid,
}

impl FixedCapacityFixture {
    fn new() -> Self {
        let operation_datetime = datetime(2026, 8, 12, 0, 10);
        Self {
            operation_datetime,
            busy_start: operation_datetime,
            busy_end: datetime(2026, 8, 12, 0, 30),
            fixed_id: Uuid::from_u128(1),
        }
    }

    fn repository(&self, candidate_minutes: Option<i64>) -> SchedulingRepository {
        let mut projects = vec![task(
            "fixed-crossing-boundary",
            self.fixed_id,
            self.operation_datetime,
            self.operation_datetime,
            FIXED_RESERVATION_MINUTES,
            10,
            Status::Todo,
        )];
        projects[0].set_fixed_start(true).unwrap();
        projects[0]
            .set_actual_work_seconds(ACTUAL_WORK_MINUTES * MINUTE_SECONDS)
            .unwrap();

        if let Some(candidate_minutes) = candidate_minutes {
            projects.push(task(
                "pack-candidate",
                Uuid::from_u128(100 + candidate_minutes as u128),
                self.operation_datetime,
                datetime(2026, 8, 12, 6, 0),
                candidate_minutes,
                1,
                Status::Pending,
            ));
            projects[1]
                .set_pending_until(datetime(2026, 8, 13, 6, 0))
                .unwrap();
        }

        SchedulingRepository::new(projects, self.operation_datetime)
    }

    fn free_time_manager(&self) -> RecordingFreeTimeManager {
        self.free_time_manager_with_daily_free_minutes(60)
    }

    fn free_time_manager_with_daily_free_minutes(
        &self,
        daily_free_minutes: i64,
    ) -> RecordingFreeTimeManager {
        RecordingFreeTimeManager::new(daily_free_minutes, self.busy_start, self.busy_end)
    }
}

struct RecordingFreeTimeManager {
    daily_free_minutes: i64,
    blocked_interval: (DateTime<Local>, DateTime<Local>),
    queries: Vec<(DateTime<Local>, DateTime<Local>)>,
}

impl RecordingFreeTimeManager {
    fn new(
        daily_free_minutes: i64,
        blocked_start: DateTime<Local>,
        blocked_end: DateTime<Local>,
    ) -> Self {
        Self {
            daily_free_minutes,
            blocked_interval: (blocked_start, blocked_end),
            queries: Vec::new(),
        }
    }

    fn queried(&self, expected_start: DateTime<Local>, expected_end: DateTime<Local>) -> bool {
        self.queries.contains(&(expected_start, expected_end))
    }
}

impl FreeTimeManagerTrait for RecordingFreeTimeManager {
    fn get_free_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
        self.queries.push((*start, *end));
        let duration_minutes = (*end - *start).num_minutes().max(0);
        if duration_minutes >= 12 * 60 {
            return self.daily_free_minutes;
        }

        let (blocked_start, blocked_end) = self.blocked_interval;
        let overlap_start = (*start).max(blocked_start);
        let overlap_end = (*end).min(blocked_end);
        let blocked_minutes = (overlap_end - overlap_start).num_minutes().max(0);
        duration_minutes - blocked_minutes
    }

    fn get_busy_minutes(&mut self, start: &DateTime<Local>, end: &DateTime<Local>) -> i64 {
        (*end - *start).num_minutes() - self.get_free_minutes(start, end)
    }

    fn register_busy_time_slot(
        &mut self,
        _start: &DateTime<Local>,
        _end: &DateTime<Local>,
    ) -> Result<(), BusyTimeSlotRegistrationError> {
        Ok(())
    }

    fn load_busy_time_slots_from_file(
        &mut self,
        _busy_time_slots_file_path: &str,
    ) -> Result<(), BusyTimeSlotLoadError> {
        Ok(())
    }
}

#[test]
fn fixed容量はbusy控除と論理日配賦をpackとflattenで一致させる() {
    let fixture = FixedCapacityFixture::new();

    let schedule_repository = fixture.repository(None);
    let fixed_segment = get_schedule(&schedule_repository)
        .unwrap()
        .into_iter()
        .find(|segment| segment.task.id == fixture.fixed_id)
        .unwrap();
    assert_eq!(fixed_segment.scheduled_start, fixture.operation_datetime);
    assert_eq!(fixed_segment.scheduled_end, datetime(2026, 8, 12, 6, 30));
    assert_eq!(
        (fixed_segment.scheduled_end - fixed_segment.scheduled_start).num_minutes(),
        FIXED_RESERVATION_MINUTES
    );
    assert_eq!(
        fixed_segment.scheduled_work_seconds,
        (FIXED_RESERVATION_MINUTES - ACTUAL_WORK_MINUTES) * MINUTE_SECONDS
    );

    for (candidate_minutes, should_pack) in [(12, true), (13, false)] {
        let repository = fixture.repository(Some(candidate_minutes));
        let candidate_id = Uuid::from_u128(100 + candidate_minutes as u128);
        let mut free_time_manager = fixture.free_time_manager();

        let result = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(
            result
                .packed_tasks
                .iter()
                .any(|packed| packed.task_id == candidate_id),
            should_pack,
            "candidate_minutes={candidate_minutes}"
        );
        assert_eq!(
            result
                .skipped_tasks
                .iter()
                .any(|skipped| skipped.task_id == candidate_id),
            !should_pack,
            "candidate_minutes={candidate_minutes}"
        );
        assert!(free_time_manager.queried(fixture.busy_start, fixture.busy_end));
    }

    let flatten_repository = fixture.repository(None);
    let mut flatten_free_time_manager = fixture.free_time_manager_with_daily_free_minutes(59);
    let flatten_result =
        flatten_tasks(&flatten_repository, &mut flatten_free_time_manager).unwrap();
    let start_date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();

    assert!(flatten_result.had_overload);
    assert_eq!(flatten_result.unresolved_overloads.len(), 1);
    assert_eq!(flatten_result.unresolved_overloads[0].date, start_date);
    assert_eq!(
        flatten_result.unresolved_overloads[0].excess_work_seconds,
        350 * MINUTE_SECONDS
    );
    assert_eq!(
        flatten_result.unresolved_overloads[0].reasons[0].reason,
        UnresolvedReason::FixedStart
    );
    assert!(flatten_free_time_manager.queried(fixture.busy_start, fixture.busy_end));
}

fn task(
    name: &str,
    id: Uuid,
    now: DateTime<Local>,
    start: DateTime<Local>,
    work_minutes: i64,
    priority: i64,
    status: Status,
) -> TaskHandle {
    let task = TaskHandle::with_identity(name, id, now).unwrap();
    task.sync_clock(now).unwrap();
    task.set_start_time(start).unwrap();
    task.set_estimated_work_seconds(work_minutes * MINUTE_SECONDS)
        .unwrap();
    task.set_priority(priority).unwrap();
    task.set_orig_status(status).unwrap();
    task
}

fn datetime(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
    Local
        .with_ymd_and_hms(year, month, day, hour, minute, 0)
        .unwrap()
}
