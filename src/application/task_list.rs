use crate::application::interface::TaskRepositoryTrait;
use crate::application::schedule_use_case::get_schedule;
use crate::application::task_use_case::ApplicationError;
use crate::application::task_view::TaskView;
use crate::entity::task::{ProjectCategory, Status, TaskHandle, TaskTreeError};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

pub const LIST_TASKS_MAX_PAGE_SIZE: usize = 500;
const CURSOR_VERSION: u8 = 1;
const CURSOR_PREFIX: &str = "schronu-list-tasks.";

#[derive(Clone, Debug, PartialEq)]
pub struct ListTasksPageRequest {
    pub filter: ListTasksFilter,
    pub query: Option<String>,
    pub root_task_id: Option<Uuid>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ListTasksPage {
    pub tasks: Vec<TaskView>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ListTasksCursor {
    version: u8,
    repository_revision: Option<Uuid>,
    filter_fingerprint: String,
    resume_after_position: Vec<usize>,
    previous_task_id: Uuid,
}

#[derive(Clone, Debug, Serialize)]
struct CanonicalListTasksFilter {
    period: Option<CanonicalTaskPeriodFilter>,
    statuses: Vec<u8>,
    categories: Vec<i8>,
    query: Option<String>,
    root_task_id: Option<Uuid>,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct CanonicalTaskPeriodFilter {
    field: u8,
    from_seconds: i64,
    from_nanoseconds: u32,
    until_seconds: i64,
    until_nanoseconds: u32,
}

#[derive(Default)]
struct ListTasksTraversalMetrics {
    #[cfg(feature = "benchmarking")]
    visited_task_count: usize,
    #[cfg(feature = "benchmarking")]
    peak_retained_task_count: usize,
}

impl ListTasksTraversalMetrics {
    fn record_visit(&mut self) {
        #[cfg(feature = "benchmarking")]
        {
            self.visited_task_count += 1;
        }
    }

    fn record_retained(&mut self, count: usize) {
        #[cfg(feature = "benchmarking")]
        {
            self.peak_retained_task_count = self.peak_retained_task_count.max(count);
        }
        #[cfg(not(feature = "benchmarking"))]
        let _ = count;
    }
}

struct TraversalItem {
    task: TaskHandle,
    position: Vec<usize>,
}

struct PreOrderTraversal {
    stack: Vec<TraversalItem>,
    construction_peak_retained_task_count: usize,
}

impl PreOrderTraversal {
    fn new(roots: Vec<TaskHandle>) -> Self {
        let construction_peak_retained_task_count = roots.len();
        let stack = roots
            .into_iter()
            .enumerate()
            .rev()
            .map(|(index, task)| TraversalItem {
                task,
                position: vec![index],
            })
            .collect();
        Self {
            stack,
            construction_peak_retained_task_count,
        }
    }

    fn resume_after(
        roots: Vec<TaskHandle>,
        position: &[usize],
        previous_task_id: Uuid,
    ) -> Result<Self, ApplicationError> {
        if position.is_empty() {
            return Err(cursor_error("resume position mismatch"));
        }

        let mut levels = Vec::with_capacity(position.len());
        let mut siblings = roots;
        let mut parent_position = Vec::new();
        let mut retained_count = 0;
        let mut construction_peak_retained_task_count = 0;
        let mut selected_task = None;

        for (depth, selected_index) in position.iter().copied().enumerate() {
            retained_count += siblings.len();
            construction_peak_retained_task_count =
                construction_peak_retained_task_count.max(retained_count);
            let current_siblings = std::mem::take(&mut siblings);
            let Some(task) = current_siblings.get(selected_index).cloned() else {
                return Err(cursor_error("resume position mismatch"));
            };
            levels.push((current_siblings, selected_index, parent_position.clone()));
            parent_position.push(selected_index);
            selected_task = Some(task.clone());
            if depth + 1 < position.len() {
                siblings = task.get_children().map_err(ApplicationError::TaskTree)?;
            }
        }

        let selected_task =
            selected_task.ok_or_else(|| cursor_error("resume position mismatch"))?;
        if selected_task.get_id().map_err(ApplicationError::TaskTree)? != previous_task_id {
            return Err(cursor_error("resume position mismatch"));
        }

        let mut stack = Vec::new();
        for (siblings, selected_index, parent_position) in levels {
            for sibling_index in (selected_index + 1..siblings.len()).rev() {
                let mut sibling_position = parent_position.clone();
                sibling_position.push(sibling_index);
                stack.push(TraversalItem {
                    task: siblings[sibling_index].clone(),
                    position: sibling_position,
                });
            }
        }
        let children = selected_task
            .get_children()
            .map_err(ApplicationError::TaskTree)?;
        construction_peak_retained_task_count = construction_peak_retained_task_count
            .max(retained_count + children.len())
            .max(stack.len() + children.len());
        for (child_index, child) in children.into_iter().enumerate().rev() {
            let mut child_position = position.to_vec();
            child_position.push(child_index);
            stack.push(TraversalItem {
                task: child,
                position: child_position,
            });
        }

        Ok(Self {
            stack,
            construction_peak_retained_task_count,
        })
    }

    fn next(&mut self) -> Result<Option<TraversalItem>, ApplicationError> {
        let Some(item) = self.stack.pop() else {
            return Ok(None);
        };
        let children = item
            .task
            .get_children()
            .map_err(ApplicationError::TaskTree)?;
        for (child_index, child) in children.into_iter().enumerate().rev() {
            let mut child_position = item.position.clone();
            child_position.push(child_index);
            self.stack.push(TraversalItem {
                task: child,
                position: child_position,
            });
        }
        Ok(Some(item))
    }

    fn retained_task_count(&self) -> usize {
        self.stack.len()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskPeriodField {
    ScheduledStart,
    CreatedAt,
    Deadline,
    CompletedAt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskPeriodFilter {
    pub field: TaskPeriodField,
    pub from: DateTime<Local>,
    pub until: DateTime<Local>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListTasksFilter {
    pub period: Option<TaskPeriodFilter>,
    pub statuses: Vec<Status>,
    pub categories: Vec<Option<ProjectCategory>>,
}

pub fn list_tasks(
    repository: &dyn TaskRepositoryTrait,
    filter: ListTasksFilter,
) -> Result<Vec<TaskView>, ApplicationError> {
    if filter
        .period
        .as_ref()
        .is_some_and(|period| period.from >= period.until)
    {
        return Err(ApplicationError::InvalidInput {
            field: "period",
            reason: "from must be earlier than until",
        });
    }

    let scheduled_task_ids = filter
        .period
        .as_ref()
        .filter(|period| period.field == TaskPeriodField::ScheduledStart)
        .map(|period| {
            Ok(get_schedule(repository)?
                .into_iter()
                .filter(|entry| {
                    period.from <= entry.scheduled_start && entry.scheduled_start < period.until
                })
                .map(|entry| entry.task.id)
                .collect::<HashSet<_>>())
        })
        .transpose()?;

    let mut tasks = Vec::new();
    for root in repository.get_all_projects() {
        collect_tasks_pre_order(root, &mut tasks).map_err(ApplicationError::TaskTree)?;
    }

    Ok(tasks
        .into_iter()
        .map(|task| TaskView::try_from(&task).map_err(ApplicationError::TaskTree))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|task| filter.statuses.is_empty() || filter.statuses.contains(&task.status))
        .filter(|task| {
            filter.categories.is_empty() || filter.categories.contains(&task.project_category)
        })
        .filter(|task| {
            filter
                .period
                .as_ref()
                .is_none_or(|period| match period.field {
                    TaskPeriodField::ScheduledStart => scheduled_task_ids
                        .as_ref()
                        .is_some_and(|task_ids| task_ids.contains(&task.id)),
                    TaskPeriodField::CreatedAt => {
                        is_in_period(task.create_time, period.from, period.until)
                    }
                    TaskPeriodField::Deadline => task
                        .deadline_time
                        .is_some_and(|time| is_in_period(time, period.from, period.until)),
                    TaskPeriodField::CompletedAt => task
                        .end_time
                        .is_some_and(|time| is_in_period(time, period.from, period.until)),
                })
        })
        .collect::<Vec<_>>())
}

pub fn list_tasks_page(
    repository: &dyn TaskRepositoryTrait,
    request: ListTasksPageRequest,
) -> Result<ListTasksPage, ApplicationError> {
    #[cfg(feature = "benchmarking")]
    {
        list_tasks_page_with_metrics(repository, request).map(|(page, _metrics)| page)
    }
    #[cfg(not(feature = "benchmarking"))]
    {
        let mut metrics = ListTasksTraversalMetrics::default();
        list_tasks_page_internal(repository, request, &mut metrics)
    }
}

#[cfg(feature = "benchmarking")]
fn list_tasks_page_with_metrics(
    repository: &dyn TaskRepositoryTrait,
    request: ListTasksPageRequest,
) -> Result<(ListTasksPage, ListTasksTraversalMetrics), ApplicationError> {
    let mut metrics = ListTasksTraversalMetrics::default();
    let page = list_tasks_page_internal(repository, request, &mut metrics)?;
    Ok((page, metrics))
}

fn list_tasks_page_internal(
    repository: &dyn TaskRepositoryTrait,
    request: ListTasksPageRequest,
    metrics: &mut ListTasksTraversalMetrics,
) -> Result<ListTasksPage, ApplicationError> {
    if request
        .limit
        .is_some_and(|limit| !(1..=LIST_TASKS_MAX_PAGE_SIZE).contains(&limit))
    {
        return Err(ApplicationError::InvalidInput {
            field: "limit",
            reason: "must be between 1 and 500",
        });
    }
    validate_period(&request.filter)?;

    let canonical_query = request
        .query
        .filter(|query| !query.is_empty())
        .map(|query| query.to_lowercase());
    let filter_fingerprint = canonical_filter_fingerprint(
        &request.filter,
        canonical_query.as_deref(),
        request.root_task_id,
    );
    let roots = traversal_roots(repository, request.root_task_id)?;
    let scheduled_task_ids = scheduled_task_ids(repository, &request.filter)?;

    let mut traversal = if let Some(cursor) = request.cursor.as_deref() {
        let cursor = decode_cursor(cursor)?;
        if cursor.version != CURSOR_VERSION {
            return Err(cursor_error("version mismatch"));
        }
        if cursor.filter_fingerprint != filter_fingerprint {
            return Err(cursor_error("filter mismatch"));
        }
        if cursor.repository_revision != repository.repository_revision() {
            return Err(cursor_error("revision mismatch"));
        }
        PreOrderTraversal::resume_after(
            roots,
            &cursor.resume_after_position,
            cursor.previous_task_id,
        )?
    } else {
        PreOrderTraversal::new(roots)
    };
    metrics.record_retained(traversal.construction_peak_retained_task_count);

    let mut tasks = Vec::with_capacity(request.limit.unwrap_or(0));
    let mut last_returned = None;
    while let Some(item) = traversal.next()? {
        metrics.record_visit();
        metrics.record_retained(traversal.retained_task_count() + 1);
        let task = TaskView::try_from(&item.task).map_err(ApplicationError::TaskTree)?;
        if !task_matches(&task, &request.filter, scheduled_task_ids.as_ref())
            || canonical_query
                .as_ref()
                .is_some_and(|query| !task.name.to_lowercase().contains(query))
        {
            continue;
        }

        if request.limit.is_some_and(|limit| tasks.len() == limit) {
            let (resume_after_position, previous_task_id) =
                last_returned.expect("a full non-empty page has a previous task");
            let cursor = ListTasksCursor {
                version: CURSOR_VERSION,
                repository_revision: repository.repository_revision(),
                filter_fingerprint,
                resume_after_position,
                previous_task_id,
            };
            return Ok(ListTasksPage {
                tasks,
                next_cursor: Some(encode_cursor(&cursor)),
            });
        }

        last_returned = Some((item.position, task.id));
        tasks.push(task);
    }

    Ok(ListTasksPage {
        tasks,
        next_cursor: None,
    })
}

fn traversal_roots(
    repository: &dyn TaskRepositoryTrait,
    root_task_id: Option<Uuid>,
) -> Result<Vec<TaskHandle>, ApplicationError> {
    match root_task_id {
        Some(root_task_id) => repository
            .get_by_id(root_task_id)
            .map_err(ApplicationError::TaskTree)?
            .map(|root| vec![root])
            .ok_or(ApplicationError::TaskNotFound(root_task_id)),
        None => Ok(repository.get_all_projects().into_iter().cloned().collect()),
    }
}

fn canonical_filter_fingerprint(
    filter: &ListTasksFilter,
    query: Option<&str>,
    root_task_id: Option<Uuid>,
) -> String {
    let mut statuses = filter.statuses.iter().map(status_code).collect::<Vec<_>>();
    statuses.sort_unstable();
    statuses.dedup();
    let mut categories = filter
        .categories
        .iter()
        .map(category_code)
        .collect::<Vec<_>>();
    categories.sort_unstable();
    categories.dedup();
    let canonical = CanonicalListTasksFilter {
        period: filter
            .period
            .as_ref()
            .map(|period| CanonicalTaskPeriodFilter {
                field: period_field_code(period.field),
                from_seconds: period.from.timestamp(),
                from_nanoseconds: period.from.timestamp_subsec_nanos(),
                until_seconds: period.until.timestamp(),
                until_nanoseconds: period.until.timestamp_subsec_nanos(),
            }),
        statuses,
        categories,
        query: query.map(str::to_owned),
        root_task_id,
    };
    serde_json::to_string(&canonical).expect("canonical list-tasks filter is serializable")
}

fn status_code(status: &Status) -> u8 {
    match status {
        Status::Todo => 0,
        Status::Pending => 1,
        Status::Done => 2,
    }
}

fn category_code(category: &Option<ProjectCategory>) -> i8 {
    match category {
        None => -1,
        Some(ProjectCategory::Earning) => 0,
        Some(ProjectCategory::Sustaining) => 1,
        Some(ProjectCategory::Recovery) => 2,
        Some(ProjectCategory::Investment) => 3,
        Some(ProjectCategory::Consumption) => 4,
    }
}

fn period_field_code(field: TaskPeriodField) -> u8 {
    match field {
        TaskPeriodField::ScheduledStart => 0,
        TaskPeriodField::CreatedAt => 1,
        TaskPeriodField::Deadline => 2,
        TaskPeriodField::CompletedAt => 3,
    }
}

fn encode_cursor(cursor: &ListTasksCursor) -> String {
    let bytes = serde_json::to_vec(cursor).expect("list-tasks cursor is serializable");
    let mut encoded = String::with_capacity(CURSOR_PREFIX.len() + bytes.len() * 2);
    encoded.push_str(CURSOR_PREFIX);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(encoded, "{byte:02x}").expect("writing to String is infallible");
    }
    encoded
}

fn decode_cursor(encoded: &str) -> Result<ListTasksCursor, ApplicationError> {
    let hex = encoded
        .strip_prefix(CURSOR_PREFIX)
        .ok_or_else(|| cursor_error("malformed cursor"))?;
    if hex.len() % 2 != 0 {
        return Err(cursor_error("malformed cursor"));
    }
    let bytes = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|digits| {
            let digits =
                std::str::from_utf8(digits).map_err(|_| cursor_error("malformed cursor"))?;
            u8::from_str_radix(digits, 16).map_err(|_| cursor_error("malformed cursor"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    serde_json::from_slice(&bytes).map_err(|_| cursor_error("malformed cursor"))
}

fn cursor_error(reason: &'static str) -> ApplicationError {
    ApplicationError::InvalidInput {
        field: "cursor",
        reason,
    }
}

fn validate_period(filter: &ListTasksFilter) -> Result<(), ApplicationError> {
    if filter
        .period
        .as_ref()
        .is_some_and(|period| period.from >= period.until)
    {
        return Err(ApplicationError::InvalidInput {
            field: "period",
            reason: "from must be earlier than until",
        });
    }
    Ok(())
}

fn scheduled_task_ids(
    repository: &dyn TaskRepositoryTrait,
    filter: &ListTasksFilter,
) -> Result<Option<HashSet<Uuid>>, ApplicationError> {
    filter
        .period
        .as_ref()
        .filter(|period| period.field == TaskPeriodField::ScheduledStart)
        .map(|period| {
            Ok(get_schedule(repository)?
                .into_iter()
                .filter(|entry| {
                    period.from <= entry.scheduled_start && entry.scheduled_start < period.until
                })
                .map(|entry| entry.task.id)
                .collect())
        })
        .transpose()
}

fn task_matches(
    task: &TaskView,
    filter: &ListTasksFilter,
    scheduled_task_ids: Option<&HashSet<Uuid>>,
) -> bool {
    (filter.statuses.is_empty() || filter.statuses.contains(&task.status))
        && (filter.categories.is_empty() || filter.categories.contains(&task.project_category))
        && filter
            .period
            .as_ref()
            .is_none_or(|period| match period.field {
                TaskPeriodField::ScheduledStart => {
                    scheduled_task_ids.is_some_and(|task_ids| task_ids.contains(&task.id))
                }
                TaskPeriodField::CreatedAt => {
                    is_in_period(task.create_time, period.from, period.until)
                }
                TaskPeriodField::Deadline => task
                    .deadline_time
                    .is_some_and(|time| is_in_period(time, period.from, period.until)),
                TaskPeriodField::CompletedAt => task
                    .end_time
                    .is_some_and(|time| is_in_period(time, period.from, period.until)),
            })
}

fn collect_tasks_pre_order(
    task: &TaskHandle,
    tasks: &mut Vec<TaskHandle>,
) -> Result<(), TaskTreeError> {
    tasks.push(task.clone());
    for child in task.get_children()? {
        collect_tasks_pre_order(&child, tasks)?;
    }
    Ok(())
}

fn is_in_period(time: DateTime<Local>, from: DateTime<Local>, until: DateTime<Local>) -> bool {
    from <= time && time < until
}

#[cfg(test)]
#[path = "list_tasks_contract_tests.rs"]
mod contract_tests;

#[cfg(test)]
#[path = "list_tasks_pagination_tests.rs"]
mod pagination_contract_tests;
