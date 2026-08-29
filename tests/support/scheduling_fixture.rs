use chrono::{DateTime, Duration, Local};
use schronu::entity::task::{Status, TaskAttr, TaskHandle, TaskTreeError};
use uuid::Uuid;

const TYPICAL_SEED: u64 = 0x5c48_524f_4e55_0120;
const STRESS_SEED: u64 = 0x5c48_524f_4e55_0480;
const UUID_NAMESPACE: u128 = 0x5444_3031_3200_0000_0000_0000_0000_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureSize {
    Small,
    Typical,
    Stress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixtureSummary {
    pub projects: usize,
    pub tasks: usize,
    pub leaves: usize,
    pub active_nodes: usize,
    pub active_leaves: usize,
    pub active_projects: usize,
    pub root_statuses: [usize; 3],
    pub leaf_statuses: [usize; 3],
    pub active_atomic: usize,
    pub active_deadline: usize,
    pub active_work_buckets: [usize; 5],
    pub project_depth_percentiles: [usize; 4],
    pub project_size_percentiles: [usize; 4],
}

pub struct SchedulingFixture {
    pub projects: Vec<TaskHandle>,
    pub now: DateTime<Local>,
    pub seed: u64,
}

impl SchedulingFixture {
    pub fn build(size: FixtureSize) -> Result<Self, TaskTreeError> {
        match size {
            FixtureSize::Small => build_small(),
            FixtureSize::Typical => build_profile(1, TYPICAL_SEED),
            FixtureSize::Stress => build_profile(4, STRESS_SEED),
        }
    }

    pub fn summary(&self) -> Result<FixtureSummary, TaskTreeError> {
        summarize_projects(&self.projects)
    }

    pub fn digest(&self) -> Result<u64, TaskTreeError> {
        let mut digest = Fnv1a::new();
        digest.write_u64(self.seed);
        digest.write_i64(self.now.timestamp());
        for project in &self.projects {
            digest_task(project, 0, &mut digest)?;
        }
        Ok(digest.finish())
    }
}

fn fixed_now() -> DateTime<Local> {
    DateTime::parse_from_rfc3339("2030-01-15T09:00:00+09:00")
        .expect("the synthetic fixture time is valid")
        .with_timezone(&Local)
}

fn build_small() -> Result<SchedulingFixture, TaskTreeError> {
    let now = fixed_now();
    let seed = TYPICAL_SEED;
    let mut sequence = 0_u64;
    let root = new_task("fixture-project-0000", &mut sequence, now, Status::Todo)?;

    let mut first = new_attr("fixture-task-0001", &mut sequence, now, Status::Todo);
    first.set_estimated_work_seconds(15 * 60);
    root.create_child(first)?;

    let mut atomic = new_attr("fixture-task-0002", &mut sequence, now, Status::Todo);
    atomic.set_atomic(true);
    atomic.set_estimated_work_seconds(60 * 60);
    atomic.set_deadline_time_opt(Some(now + Duration::days(1)));
    atomic.set_start_time(now + Duration::hours(1));
    atomic.set_pending_until(now + Duration::days(7));
    atomic.set_orig_status(Status::Pending);
    root.create_child(atomic)?;

    let mut pending = new_attr("fixture-task-0003", &mut sequence, now, Status::Pending);
    pending.set_pending_until(now + Duration::days(7));
    root.create_child(pending)?;

    let mut additional = new_attr("fixture-task-0004", &mut sequence, now, Status::Todo);
    additional.set_estimated_work_seconds(15 * 60);
    root.create_child(additional)?;

    let completed = new_task("fixture-project-0001", &mut sequence, now, Status::Done)?;

    Ok(SchedulingFixture {
        projects: vec![root, completed],
        now,
        seed,
    })
}

fn build_profile(scale: usize, seed: u64) -> Result<SchedulingFixture, TaskTreeError> {
    let now = fixed_now();
    let mut projects = Vec::with_capacity(2_213 * scale);
    let mut sequence = 0_u64;
    let mut random = SplitMix64::new(seed);

    for copy in 0..scale {
        let mut active_leaf_remaining = [383_usize, 308];
        let mut active_internal_remaining = 648_usize;
        let mut active_leaf_index = 0_usize;

        for project_index in 0..2_213 {
            let size = project_size(project_index);
            let depth = project_depth(project_index);
            let root_status = root_status(project_index);
            let root = new_task(
                &format!("fixture-project-{copy:02}-{project_index:04}"),
                &mut sequence,
                now,
                root_status,
            )?;
            let project_is_active = project_index >= 1_797;
            let mut current = root.clone();

            for level in 1..=depth {
                let is_leaf = level == depth;
                let status = if !is_leaf && project_is_active && active_internal_remaining > 0 {
                    active_internal_remaining -= 1;
                    Status::Todo
                } else if is_leaf
                    && project_is_active
                    && active_leaf_remaining.iter().sum::<usize>() > 0
                {
                    next_active_leaf_status(&mut active_leaf_remaining)
                } else {
                    Status::Done
                };
                let name = format!("fixture-task-{copy:02}-{project_index:04}-{level:04}");
                let mut attr = new_attr(&name, &mut sequence, now, status);
                if is_leaf && status != Status::Done {
                    configure_active_leaf(&mut attr, active_leaf_index, now, &mut random);
                    if is_leaf {
                        active_leaf_index += 1;
                    }
                }
                current = current.create_child(attr)?;
            }

            let direct_leaves = size.saturating_sub(depth + 1);
            for leaf_offset in 0..direct_leaves {
                let should_be_active =
                    project_is_active && active_leaf_remaining.iter().sum::<usize>() > 0;
                let status = if should_be_active {
                    next_active_leaf_status(&mut active_leaf_remaining)
                } else {
                    Status::Done
                };
                let name = format!(
                    "fixture-task-{copy:02}-{project_index:04}-{:04}",
                    depth + 1 + leaf_offset
                );
                let mut attr = new_attr(&name, &mut sequence, now, status);
                if status != Status::Done {
                    configure_active_leaf(&mut attr, active_leaf_index, now, &mut random);
                    active_leaf_index += 1;
                }
                root.create_child(attr)?;
            }
            projects.push(root);
        }

        assert_eq!(active_leaf_remaining, [0, 0]);
        assert_eq!(active_internal_remaining, 0);
        assert_eq!(active_leaf_index, 691);
    }

    Ok(SchedulingFixture {
        projects,
        now,
        seed,
    })
}

fn project_size(index: usize) -> usize {
    match index {
        0..=1_106 => 1,
        1_107..=1_990 => 2,
        1_991 => 13,
        1_992..=2_019 => 76,
        2_020..=2_189 => 77,
        2_190..=2_211 => 263,
        2_212 => 2_486,
        _ => unreachable!("project index is bounded by the anonymized profile"),
    }
}

fn project_depth(index: usize) -> usize {
    match index {
        0..=1_106 => 0,
        1_107..=1_990 => 1,
        1_991 => 3,
        1_992..=2_043 => 15,
        2_044..=2_189 => 16,
        2_190..=2_211 => 18,
        2_212 => 210,
        _ => unreachable!("project index is bounded by the anonymized profile"),
    }
}

fn root_status(index: usize) -> Status {
    match index {
        1_803..=2_191 => Status::Pending,
        2_192..=2_212 => Status::Todo,
        _ => Status::Done,
    }
}

fn next_active_leaf_status(remaining: &mut [usize; 2]) -> Status {
    if remaining[0] > 0 {
        remaining[0] -= 1;
        Status::Pending
    } else {
        remaining[1] -= 1;
        Status::Todo
    }
}

fn configure_active_leaf(
    attr: &mut TaskAttr,
    active_leaf_index: usize,
    now: DateTime<Local>,
    random: &mut SplitMix64,
) {
    if active_leaf_index < 119 {
        attr.set_atomic(true);
    }
    if active_leaf_index < 343 {
        attr.set_deadline_time_opt(Some(now + Duration::days(1 + (random.next() % 14) as i64)));
    }
    let work_seconds = match active_leaf_index {
        0..=30 => 0,
        31..=379 => 15 * 60,
        380..=602 => 60 * 60,
        603..=643 => 4 * 60 * 60,
        _ => 8 * 60 * 60,
    };
    attr.set_estimated_work_seconds(work_seconds);
    attr.set_start_time(now + Duration::days((random.next() % 7) as i64));
}

fn new_task(
    name: &str,
    sequence: &mut u64,
    now: DateTime<Local>,
    status: Status,
) -> Result<TaskHandle, TaskTreeError> {
    let task = TaskHandle::with_identity(name, next_uuid(sequence), now)?;
    task.set_orig_status(status)?;
    if status == Status::Pending {
        task.set_pending_until(now + Duration::days(30))?;
    }
    Ok(task)
}

fn new_attr(name: &str, sequence: &mut u64, now: DateTime<Local>, status: Status) -> TaskAttr {
    let mut attr = TaskAttr::with_identity(name, next_uuid(sequence), now);
    if status == Status::Pending {
        attr.set_pending_until(now + Duration::days(30));
    }
    attr.set_orig_status(status);
    attr
}

fn next_uuid(sequence: &mut u64) -> Uuid {
    *sequence += 1;
    Uuid::from_u128(UUID_NAMESPACE | u128::from(*sequence))
}

pub fn summarize_projects(projects: &[TaskHandle]) -> Result<FixtureSummary, TaskTreeError> {
    let mut summary = FixtureSummary {
        projects: projects.len(),
        tasks: 0,
        leaves: 0,
        active_nodes: 0,
        active_leaves: 0,
        active_projects: 0,
        root_statuses: [0; 3],
        leaf_statuses: [0; 3],
        active_atomic: 0,
        active_deadline: 0,
        active_work_buckets: [0; 5],
        project_depth_percentiles: [0; 4],
        project_size_percentiles: [0; 4],
    };
    let mut project_depths = Vec::with_capacity(projects.len());
    let mut project_sizes = Vec::with_capacity(projects.len());

    for project in projects {
        summary.root_statuses[status_index(project.get_orig_status()?)] += 1;
        let mut project_size = 0;
        let mut project_depth = 0;
        let mut project_active = false;
        summarize_task(
            project,
            0,
            &mut project_size,
            &mut project_depth,
            &mut project_active,
            &mut summary,
        )?;
        summary.active_projects += usize::from(project_active);
        project_depths.push(project_depth);
        project_sizes.push(project_size);
    }
    project_depths.sort_unstable();
    project_sizes.sort_unstable();
    summary.project_depth_percentiles = percentiles(&project_depths);
    summary.project_size_percentiles = percentiles(&project_sizes);
    Ok(summary)
}

fn summarize_task(
    task: &TaskHandle,
    depth: usize,
    project_size: &mut usize,
    project_depth: &mut usize,
    project_active: &mut bool,
    summary: &mut FixtureSummary,
) -> Result<(), TaskTreeError> {
    *project_size += 1;
    *project_depth = (*project_depth).max(depth);
    summary.tasks += 1;
    let status = task.get_orig_status()?;
    let active = status != Status::Done;
    summary.active_nodes += usize::from(active);
    *project_active |= active;
    let children = task.get_children()?;
    if children.is_empty() {
        summary.leaves += 1;
        summary.leaf_statuses[status_index(status)] += 1;
        if active {
            summary.active_leaves += 1;
            summary.active_atomic += usize::from(task.get_atomic()?);
            summary.active_deadline += usize::from(task.get_deadline_time_opt()?.is_some());
            let seconds = task.get_estimated_work_seconds()?;
            let bucket = match seconds {
                0 => 0,
                1..=900 => 1,
                901..=3_600 => 2,
                3_601..=14_400 => 3,
                _ => 4,
            };
            summary.active_work_buckets[bucket] += 1;
        }
    }
    for child in children {
        summarize_task(
            &child,
            depth + 1,
            project_size,
            project_depth,
            project_active,
            summary,
        )?;
    }
    Ok(())
}

fn status_index(status: Status) -> usize {
    match status {
        Status::Done => 0,
        Status::Todo => 1,
        Status::Pending => 2,
    }
}

fn percentiles(sorted: &[usize]) -> [usize; 4] {
    let nearest_rank = |percent: usize| sorted[(sorted.len() * percent).div_ceil(100) - 1];
    [
        nearest_rank(50),
        nearest_rank(90),
        nearest_rank(99),
        *sorted.last().expect("fixture contains projects"),
    ]
}

fn digest_task(task: &TaskHandle, depth: usize, digest: &mut Fnv1a) -> Result<(), TaskTreeError> {
    digest.write_u64(depth as u64);
    digest.write(task.get_id()?.as_bytes());
    digest.write(task.get_name()?.as_bytes());
    digest.write_u64(status_index(task.get_orig_status()?) as u64);
    digest.write_u64(u64::from(task.get_atomic()?));
    digest.write_i64(task.get_pending_until()?.timestamp());
    digest.write_i64(task.get_start_time()?.timestamp());
    digest.write_i64(
        task.get_deadline_time_opt()?
            .map_or(i64::MIN, |deadline| deadline.timestamp()),
    );
    digest.write_i64(task.get_estimated_work_seconds()?);
    digest.write_i64(task.get_actual_work_seconds()?);
    digest.write_i64(task.get_priority()?);
    let children = task.get_children()?;
    digest.write_u64(children.len() as u64);
    for child in children {
        digest_task(&child, depth + 1, digest)?;
    }
    Ok(())
}

struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}

struct Fnv1a(u64);

impl Fnv1a {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.write(&value.to_le_bytes());
    }

    fn write_i64(&mut self, value: i64) {
        self.write(&value.to_le_bytes());
    }

    fn finish(self) -> u64 {
        self.0
    }
}
