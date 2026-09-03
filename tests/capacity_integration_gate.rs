#[path = "support/scheduling_harness.rs"]
#[allow(dead_code)]
mod scheduling_harness;

use chrono::{DateTime, Local, NaiveDate, TimeZone};
use scheduling_harness::SchedulingRepository;
use schronu::application::flatten_use_case::{flatten_tasks, UnresolvedReason};
use schronu::application::interface::{
    BusyTimeSlotLoadError, BusyTimeSlotRegistrationError, FreeTimeManagerTrait, TaskRepositoryTrait,
};
use schronu::application::pack_use_case::{pack_tasks, pack_tasks_with_end_of_day_offset_minutes};
use schronu::application::schedule_use_case::get_schedule;
use schronu::entity::task::{Status, TaskHandle};
use uuid::Uuid;

const MINUTE_SECONDS: i64 = 60;
const FIXED_RESERVATION_MINUTES: i64 = 380;
const FLEXIBLE_WORK_MINUTES: i64 = 380;
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
        self.repository_with_candidate(
            candidate_minutes.map(|minutes| (minutes, 1, datetime(2026, 8, 12, 6, 0))),
        )
    }

    fn repository_with_candidate(
        &self,
        candidate: Option<(i64, i64, DateTime<Local>)>,
    ) -> SchedulingRepository {
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

        if let Some((candidate_minutes, candidate_priority, candidate_start)) = candidate {
            projects.push(task(
                "pack-candidate",
                Uuid::from_u128(100 + candidate_minutes as u128),
                self.operation_datetime,
                candidate_start,
                candidate_minutes,
                candidate_priority,
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

#[derive(Clone, Copy)]
struct FlexibleCapacityFixture {
    operation_datetime: DateTime<Local>,
    busy_start: DateTime<Local>,
    busy_end: DateTime<Local>,
    flexible_id: Uuid,
}

impl FlexibleCapacityFixture {
    fn new() -> Self {
        let operation_datetime = datetime(2026, 8, 12, 0, 10);
        Self {
            operation_datetime,
            busy_start: operation_datetime,
            busy_end: datetime(2026, 8, 12, 0, 30),
            flexible_id: Uuid::from_u128(2),
        }
    }

    fn repository(&self, candidate_minutes: Option<i64>) -> SchedulingRepository {
        self.repository_with_candidate(
            candidate_minutes.map(|minutes| (minutes, 1, datetime(2026, 8, 12, 6, 0))),
        )
    }

    fn repository_with_candidate(
        &self,
        candidate: Option<(i64, i64, DateTime<Local>)>,
    ) -> SchedulingRepository {
        let mut projects = vec![task(
            "flexible-crossing-boundary",
            self.flexible_id,
            self.operation_datetime,
            self.operation_datetime,
            FLEXIBLE_WORK_MINUTES,
            10,
            Status::Todo,
        )];

        if let Some((candidate_minutes, candidate_priority, candidate_start)) = candidate {
            projects.push(task(
                "pack-candidate",
                Uuid::from_u128(200 + candidate_minutes as u128),
                self.operation_datetime,
                candidate_start,
                candidate_minutes,
                candidate_priority,
                Status::Pending,
            ));
            projects[1]
                .set_pending_until(datetime(2026, 8, 13, 6, 0))
                .unwrap();
        }

        SchedulingRepository::new(projects, self.operation_datetime)
    }
}

struct RecordingFreeTimeManager {
    daily_free_minutes: i64,
    short_interval_free_minutes: Option<i64>,
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
            short_interval_free_minutes: None,
            blocked_interval: (blocked_start, blocked_end),
            queries: Vec::new(),
        }
    }

    fn with_short_interval_free_minutes(
        daily_free_minutes: i64,
        short_interval_free_minutes: i64,
        blocked_start: DateTime<Local>,
        blocked_end: DateTime<Local>,
    ) -> Self {
        Self {
            daily_free_minutes,
            short_interval_free_minutes: Some(short_interval_free_minutes),
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
        if let Some(free_minutes) = self.short_interval_free_minutes {
            return free_minutes;
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
    let start_date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
    let next_date = NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();
    let busy_probe_fixed_id = Uuid::from_u128(301);
    let busy_probe_candidate_id = Uuid::from_u128(302);
    let busy_probe_repository = fixed_busy_probe_repository(
        fixture.operation_datetime,
        busy_probe_fixed_id,
        busy_probe_candidate_id,
    );
    let busy_probe_fixed_segment = get_schedule(&busy_probe_repository)
        .unwrap()
        .into_iter()
        .find(|segment| segment.task.id == busy_probe_fixed_id)
        .unwrap();
    assert_eq!(
        busy_probe_fixed_segment.scheduled_start,
        datetime(2026, 8, 12, 0, 20)
    );
    assert_eq!(
        busy_probe_fixed_segment.scheduled_end,
        datetime(2026, 8, 12, 6, 20)
    );
    assert!(busy_probe_fixed_segment.scheduled_start < fixture.busy_end);
    assert!(busy_probe_fixed_segment.scheduled_end > datetime(2026, 8, 12, 6, 0));
    assert_crossing_segment_busy_changes_pack_result(
        fixed_busy_probe_repository(
            fixture.operation_datetime,
            busy_probe_fixed_id,
            busy_probe_candidate_id,
        ),
        fixed_busy_probe_repository(
            fixture.operation_datetime,
            busy_probe_fixed_id,
            busy_probe_candidate_id,
        ),
        busy_probe_candidate_id,
        1,
        fixture.operation_datetime,
        fixture.busy_start,
        fixture.busy_end,
        datetime(2026, 8, 12, 8, 36),
        516,
    );

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

    let mut maximum_packed_candidate_minutes = 0;
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
        if should_pack {
            let packed = result
                .packed_tasks
                .iter()
                .find(|packed| packed.task_id == candidate_id)
                .unwrap();
            assert_eq!(
                packed.source_date,
                NaiveDate::from_ymd_opt(2026, 8, 13).unwrap()
            );
            assert_eq!(
                packed.target_date,
                NaiveDate::from_ymd_opt(2026, 8, 12).unwrap()
            );
            assert_eq!(packed.work_seconds, 12 * MINUTE_SECONDS);
            maximum_packed_candidate_minutes = packed.work_seconds / MINUTE_SECONDS;
        }
        assert!(free_time_manager.queried(fixture.busy_start, fixture.busy_end));
    }
    assert_eq!(maximum_packed_candidate_minutes, 12);
    let next_day_rho_limit_minutes = 42;
    let pack_next_day_capacity_minutes =
        next_day_rho_limit_minutes - maximum_packed_candidate_minutes;
    let start_capacity_probe_id = Uuid::from_u128(101);
    for (current_free_minutes, should_probe_placement) in [(500, false), (502, true)] {
        let start_capacity_probe_repository =
            fixture.repository_with_candidate(Some((1, 20, fixture.operation_datetime)));
        start_capacity_probe_repository
            .get_by_id(start_capacity_probe_id)
            .unwrap()
            .unwrap()
            .set_atomic(true)
            .unwrap();
        let mut start_capacity_probe_free_time =
            RecordingFreeTimeManager::with_short_interval_free_minutes(
                0,
                current_free_minutes,
                fixture.busy_start,
                fixture.busy_end,
            );
        let start_capacity_probe_result = pack_tasks_with_end_of_day_offset_minutes(
            &start_capacity_probe_repository,
            &mut start_capacity_probe_free_time,
            720,
        )
        .unwrap();

        assert!(start_capacity_probe_result.packed_tasks.is_empty());
        assert!(start_capacity_probe_result
            .skipped_tasks
            .iter()
            .any(|skipped| skipped.task_id == start_capacity_probe_id));
        assert_eq!(
            start_capacity_probe_free_time
                .queried(fixture.operation_datetime, datetime(2026, 8, 12, 0, 11),),
            should_probe_placement,
            "current_free_minutes={current_free_minutes}"
        );
    }
    let pack_start_day_capacity_minutes = 350;
    let pack_observed_capacity = [
        (start_date, pack_start_day_capacity_minutes),
        (next_date, pack_next_day_capacity_minutes),
    ];
    assert_eq!(pack_observed_capacity, [(start_date, 350), (next_date, 30)]);
    assert_eq!(
        pack_observed_capacity
            .iter()
            .map(|(_, minutes)| minutes)
            .sum::<i64>(),
        FIXED_RESERVATION_MINUTES
    );

    let flatten_repository = fixture.repository(None);
    let mut flatten_free_time_manager = fixture.free_time_manager_with_daily_free_minutes(30);
    let flatten_result =
        flatten_tasks(&flatten_repository, &mut flatten_free_time_manager).unwrap();

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

    let below_next_day_allocation_repository = fixture.repository(None);
    let mut below_next_day_allocation_free_time =
        fixture.free_time_manager_with_daily_free_minutes(29);
    let below_next_day_allocation_result = flatten_tasks(
        &below_next_day_allocation_repository,
        &mut below_next_day_allocation_free_time,
    )
    .unwrap();
    let next_day_overload = below_next_day_allocation_result
        .unresolved_overloads
        .iter()
        .find(|overload| overload.date == next_date)
        .unwrap();
    assert_eq!(next_day_overload.excess_work_seconds, MINUTE_SECONDS);

    let flatten_start_day_capacity_minutes =
        flatten_result.unresolved_overloads[0].excess_work_seconds / MINUTE_SECONDS;
    let below_boundary_free_minutes = 29;
    let flatten_next_day_capacity_minutes =
        below_boundary_free_minutes + next_day_overload.excess_work_seconds / MINUTE_SECONDS;
    let flatten_observed_capacity = [
        (start_date, flatten_start_day_capacity_minutes),
        (next_date, flatten_next_day_capacity_minutes),
    ];
    assert_eq!(
        flatten_observed_capacity,
        [(start_date, 350), (next_date, 30)]
    );
    assert_eq!(
        flatten_observed_capacity
            .iter()
            .map(|(_, minutes)| minutes)
            .sum::<i64>(),
        FIXED_RESERVATION_MINUTES
    );
    assert_eq!(pack_observed_capacity, flatten_observed_capacity);
}

#[test]
fn flexible容量はbusy控除と論理日配賦をpackとflattenで一致させる() {
    let fixture = FlexibleCapacityFixture::new();
    let start_date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
    let next_date = NaiveDate::from_ymd_opt(2026, 8, 12).unwrap();

    let busy_probe_candidate_minutes = 134;
    let busy_probe_candidate_id = Uuid::from_u128(200 + busy_probe_candidate_minutes as u128);
    assert_crossing_segment_busy_changes_pack_result(
        fixture.repository_with_candidate(Some((
            busy_probe_candidate_minutes,
            20,
            fixture.operation_datetime,
        ))),
        fixture.repository_with_candidate(Some((
            busy_probe_candidate_minutes,
            20,
            fixture.operation_datetime,
        ))),
        busy_probe_candidate_id,
        busy_probe_candidate_minutes,
        fixture.operation_datetime,
        fixture.busy_start,
        fixture.busy_end,
        datetime(2026, 8, 12, 12, 0),
        720,
    );

    let schedule_repository = fixture.repository(None);
    let flexible_segments = get_schedule(&schedule_repository)
        .unwrap()
        .into_iter()
        .filter(|segment| segment.task.id == fixture.flexible_id)
        .collect::<Vec<_>>();
    assert_eq!(flexible_segments.len(), 1);
    let flexible_segment = &flexible_segments[0];
    assert!(!flexible_segment.task.fixed_start);
    assert_eq!(flexible_segment.scheduled_start, fixture.operation_datetime);
    assert_eq!(flexible_segment.scheduled_end, datetime(2026, 8, 12, 6, 30));
    assert_eq!(
        flexible_segment.scheduled_work_seconds,
        FLEXIBLE_WORK_MINUTES * MINUTE_SECONDS
    );
    assert_eq!(
        flexible_segment.total_work_seconds,
        FLEXIBLE_WORK_MINUTES * MINUTE_SECONDS
    );
    assert_eq!(
        (flexible_segment.scheduled_end - flexible_segment.scheduled_start).num_minutes(),
        FLEXIBLE_WORK_MINUTES
    );

    let mut maximum_packed_candidate_minutes = 0;
    for (candidate_minutes, should_pack) in [(12, true), (13, false)] {
        let repository = fixture.repository(Some(candidate_minutes));
        let candidate_id = Uuid::from_u128(200 + candidate_minutes as u128);
        let mut free_time_manager =
            RecordingFreeTimeManager::new(60, fixture.busy_start, fixture.busy_end);

        let result = pack_tasks(&repository, &mut free_time_manager).unwrap();

        assert_eq!(result.packed_tasks.len(), usize::from(should_pack));
        assert_eq!(result.skipped_tasks.len(), usize::from(!should_pack));
        if should_pack {
            let packed = &result.packed_tasks[0];
            assert_eq!(packed.task_id, candidate_id);
            assert_eq!(packed.name, "pack-candidate");
            assert_eq!(packed.priority, 1);
            assert_eq!(
                packed.source_date,
                NaiveDate::from_ymd_opt(2026, 8, 13).unwrap()
            );
            assert_eq!(packed.target_date, next_date);
            assert_eq!(packed.work_seconds, 12 * MINUTE_SECONDS);
            maximum_packed_candidate_minutes = packed.work_seconds / MINUTE_SECONDS;
        } else {
            let skipped = &result.skipped_tasks[0];
            assert_eq!(skipped.task_id, candidate_id);
            assert_eq!(skipped.name, "pack-candidate");
            assert_eq!(skipped.priority, 1);
            assert_eq!(skipped.required_work_seconds, 13 * MINUTE_SECONDS);
        }
        assert!(free_time_manager.queried(fixture.busy_start, fixture.busy_end));
    }
    assert_eq!(maximum_packed_candidate_minutes, 12);
    let next_day_rho_limit_minutes = 42;
    let pack_next_day_capacity_minutes =
        next_day_rho_limit_minutes - maximum_packed_candidate_minutes;
    let start_capacity_probe_id = Uuid::from_u128(201);
    for (current_free_minutes, should_pack) in [(500, false), (502, true)] {
        let start_capacity_probe_repository =
            fixture.repository_with_candidate(Some((1, 20, fixture.operation_datetime)));
        let mut start_capacity_probe_free_time =
            RecordingFreeTimeManager::with_short_interval_free_minutes(
                0,
                current_free_minutes,
                fixture.busy_start,
                fixture.busy_end,
            );
        let start_capacity_probe_result = pack_tasks(
            &start_capacity_probe_repository,
            &mut start_capacity_probe_free_time,
        )
        .unwrap();

        assert_eq!(
            start_capacity_probe_result
                .packed_tasks
                .iter()
                .any(|packed| {
                    packed.task_id == start_capacity_probe_id
                        && packed.target_date == start_date
                        && packed.work_seconds == MINUTE_SECONDS
                }),
            should_pack,
            "current_free_minutes={current_free_minutes}"
        );
        assert_eq!(
            start_capacity_probe_result
                .skipped_tasks
                .iter()
                .any(|skipped| skipped.task_id == start_capacity_probe_id),
            !should_pack,
            "current_free_minutes={current_free_minutes}"
        );
    }
    let pack_start_day_capacity_minutes = 350;
    let pack_observed_capacity = [
        (start_date, pack_start_day_capacity_minutes),
        (next_date, pack_next_day_capacity_minutes),
    ];
    assert_eq!(pack_observed_capacity, [(start_date, 350), (next_date, 30)]);
    assert_eq!(
        pack_observed_capacity
            .iter()
            .map(|(_, minutes)| minutes)
            .sum::<i64>(),
        FLEXIBLE_WORK_MINUTES
    );

    let capacity_boundary_repository = fixture.repository(None);
    let mut capacity_boundary_free_time =
        RecordingFreeTimeManager::new(30, fixture.busy_start, fixture.busy_end);
    let capacity_boundary_result = flatten_tasks(
        &capacity_boundary_repository,
        &mut capacity_boundary_free_time,
    )
    .unwrap();

    assert!(capacity_boundary_result.had_overload);
    assert_eq!(capacity_boundary_result.unresolved_overloads.len(), 1);
    let start_day_overload = &capacity_boundary_result.unresolved_overloads[0];
    assert_eq!(start_day_overload.date, start_date);
    assert_eq!(start_day_overload.excess_work_seconds, 350 * MINUTE_SECONDS);
    assert_eq!(
        start_day_overload.reasons[0].reason,
        UnresolvedReason::CrossesLogicalDate
    );
    assert!(capacity_boundary_result
        .unresolved_overloads
        .iter()
        .all(|overload| overload.date != next_date));
    assert!(capacity_boundary_free_time.queried(fixture.busy_start, fixture.busy_end));

    let below_capacity_boundary_repository = fixture.repository(None);
    let mut below_capacity_boundary_free_time =
        RecordingFreeTimeManager::new(29, fixture.busy_start, fixture.busy_end);
    let below_capacity_boundary_result = flatten_tasks(
        &below_capacity_boundary_repository,
        &mut below_capacity_boundary_free_time,
    )
    .unwrap();
    let below_boundary_start_overload = below_capacity_boundary_result
        .unresolved_overloads
        .iter()
        .find(|overload| overload.date == start_date)
        .unwrap();
    assert_eq!(
        below_boundary_start_overload.excess_work_seconds,
        350 * MINUTE_SECONDS
    );
    let next_day_overload = below_capacity_boundary_result
        .unresolved_overloads
        .iter()
        .find(|overload| overload.date == next_date)
        .unwrap();
    assert_eq!(next_day_overload.excess_work_seconds, MINUTE_SECONDS);
    assert!(below_capacity_boundary_free_time.queried(fixture.busy_start, fixture.busy_end));

    let flatten_start_day_capacity_minutes =
        below_boundary_start_overload.excess_work_seconds / MINUTE_SECONDS;
    let below_boundary_free_minutes = 29;
    let flatten_next_day_capacity_minutes =
        below_boundary_free_minutes + next_day_overload.excess_work_seconds / MINUTE_SECONDS;
    let flatten_observed_capacity = [
        (start_date, flatten_start_day_capacity_minutes),
        (next_date, flatten_next_day_capacity_minutes),
    ];
    assert_eq!(
        flatten_observed_capacity,
        [(start_date, 350), (next_date, 30)]
    );
    assert_eq!(
        flatten_observed_capacity
            .iter()
            .map(|(_, minutes)| minutes)
            .sum::<i64>(),
        FLEXIBLE_WORK_MINUTES
    );
    assert_eq!(pack_observed_capacity, flatten_observed_capacity);
}

#[allow(clippy::too_many_arguments)]
fn assert_crossing_segment_busy_changes_pack_result(
    busy_repository: SchedulingRepository,
    no_busy_repository: SchedulingRepository,
    candidate_id: Uuid,
    candidate_minutes: i64,
    operation_datetime: DateTime<Local>,
    busy_start: DateTime<Local>,
    busy_end: DateTime<Local>,
    expected_query_end: DateTime<Local>,
    end_of_day_offset_minutes: i64,
) {
    let mut busy_free_time_manager = RecordingFreeTimeManager::new(0, busy_start, busy_end);
    let busy_result = pack_tasks_with_end_of_day_offset_minutes(
        &busy_repository,
        &mut busy_free_time_manager,
        end_of_day_offset_minutes,
    )
    .unwrap();

    assert!(busy_result.packed_tasks.is_empty());
    assert_eq!(busy_result.skipped_tasks.len(), 1);
    assert_eq!(busy_result.skipped_tasks[0].task_id, candidate_id);
    assert_eq!(busy_result.skipped_tasks[0].name, "pack-candidate");
    assert_eq!(busy_result.skipped_tasks[0].priority, 20);
    assert_eq!(
        busy_result.skipped_tasks[0].required_work_seconds,
        candidate_minutes * MINUTE_SECONDS
    );
    assert!(busy_free_time_manager.queried(operation_datetime, expected_query_end));

    let mut no_busy_free_time_manager = RecordingFreeTimeManager::new(0, busy_start, busy_start);
    let no_busy_result = pack_tasks_with_end_of_day_offset_minutes(
        &no_busy_repository,
        &mut no_busy_free_time_manager,
        end_of_day_offset_minutes,
    )
    .unwrap();
    let packed = no_busy_result
        .packed_tasks
        .iter()
        .find(|packed| packed.task_id == candidate_id)
        .unwrap();

    assert_eq!(
        packed.target_date,
        NaiveDate::from_ymd_opt(2026, 8, 11).unwrap()
    );
    assert_eq!(packed.work_seconds, candidate_minutes * MINUTE_SECONDS);
    assert!(no_busy_result.skipped_tasks.is_empty());
}

fn fixed_busy_probe_repository(
    operation_datetime: DateTime<Local>,
    fixed_id: Uuid,
    candidate_id: Uuid,
) -> SchedulingRepository {
    let fixed = task(
        "fixed-busy-crossing-boundary",
        fixed_id,
        operation_datetime,
        datetime(2026, 8, 12, 0, 20),
        360,
        10,
        Status::Todo,
    );
    fixed.set_fixed_start(true).unwrap();
    let candidate = task(
        "pack-candidate",
        candidate_id,
        operation_datetime,
        operation_datetime,
        1,
        20,
        Status::Pending,
    );
    candidate
        .set_pending_until(datetime(2026, 8, 13, 6, 0))
        .unwrap();
    SchedulingRepository::new(vec![fixed, candidate], operation_datetime)
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
