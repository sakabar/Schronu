#[cfg(test)]
use super::renderer::format_task_category_summary;
#[cfg(test)]
pub(super) use super::renderer::project_category_symbol;
use super::renderer::{
    format_task_list_columns, task_list_columns, weekday_jp, AncestorTreeRow, BandDayRow,
    BandDisplay, BandDurations, CalendarAlerts, CalendarDayRow, CalendarDisplay, CalendarSummary,
    DebugTreeRow, DisplayModel, FocusDisplay, LeafTreeRow, MessageLevel, TaskCategoryWorkSeconds,
    TaskListDisplay, TaskListIconMode, TaskListMetricsDisplay, TaskListRow, TaskListTaskRow,
    TreeDisplay, BAND_SECONDS_PER_DAY,
};
use crate::adapter::gateway::schronu_config::SchronuConfig;
use crate::application::daily_capacity::{
    calculate_daily_rho_diff_hours,
    calculate_free_time_minutes_for_logical_date_with_end_of_day_offset_minutes,
    calculate_full_day_free_time_minutes_for_logical_date_with_end_of_day_offset_minutes,
    try_logical_date, try_logical_date_end, try_next_logical_date_start, RHO_GOAL,
};
use crate::application::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use crate::application::schedule_use_case::get_schedule;
use crate::application::task_use_case::ApplicationError;
use crate::entity::task::{
    extract_leaf_tasks_from_project, round_up_sec_as_minute, ProjectCategory, TaskHandle,
};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate};
use regex::Regex;
use std::cmp::{max, min};
use std::collections::HashMap;
use unicode_width::UnicodeWidthChar;
use uuid::Uuid;

const DAILY_SUMMARY_HORIZON_DAYS: usize = 28;
const TASK_NAME_DISPLAY_WIDTH_LIMIT: usize = 70;

fn unreached_daily_summary_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2037, 12, 31).expect("daily summary fallback date must be valid")
}

fn unobserved_daily_summary_metric_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(1900, 1, 1).expect("daily summary fallback date must be valid")
}

fn task_list_horizon_days(pattern: &str) -> Option<i64> {
    match pattern {
        "今" => Some(0),
        "明" | "近" => Some(1),
        "暦" | "帯" => Some(DAILY_SUMMARY_HORIZON_DAYS as i64),
        _ => None,
    }
}

pub(super) fn get_weekday_jp(date: &NaiveDate) -> &str {
    weekday_jp(date.weekday())
}

pub(super) fn task_list_search_text(row: &TaskListTaskRow) -> String {
    format_task_list_columns(&task_list_columns(row, TaskListIconMode::Original))
}

pub(super) fn get_adjustable_prefix_label(
    task: &TaskHandle,
    dt: DateTime<Local>,
    rank: usize,
    last_synced_time: DateTime<Local>,
) -> Result<String, ApplicationError> {
    if rank != 0
        || task
            .get_is_on_other_side()
            .map_err(ApplicationError::TaskTree)?
    {
        return Ok("".to_string());
    }

    let planned_date = try_logical_date(dt)?;
    let available_datetime = max(
        task.get_start_time().map_err(ApplicationError::TaskTree)?,
        last_synced_time,
    );
    let available_date = try_logical_date(available_datetime)?;
    let advance_days = (planned_date - available_date).num_days();

    if advance_days > 0 {
        Ok(format!("【前{}】", advance_days))
    } else {
        Ok("".to_string())
    }
}

pub(super) struct RhoMetrics {
    pub(super) _total_work_hours: f64,
    pub(super) repetitive_work_hours: f64,
    pub(super) non_repetitive_work_hours: f64,
    pub(super) _available_hours: f64,
    pub(super) free_hours: f64,
    pub(super) rho: f64,
    pub(super) non_repetitive_rho: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TaskListDisplayOrder {
    ScheduledStartDesc,
    LowPriorityTail,
}

struct DailySummaryRow {
    calendar_row: CalendarDayRow,
    band_row: Option<BandDayRow>,
}

pub(super) fn calculate_daily_band_durations(
    is_today: bool,
    full_day_free_minutes: i64,
    remaining_free_minutes: i64,
    total_work_seconds: i64,
    repetitive_work_seconds: i64,
    diff_to_goal_hours: f64,
) -> BandDurations {
    BandDurations {
        fixed_seconds: (BAND_SECONDS_PER_DAY - full_day_free_minutes.max(0) * 60).max(0),
        elapsed_seconds: if is_today {
            (full_day_free_minutes - remaining_free_minutes).max(0) * 60
        } else {
            0
        },
        repetitive_seconds: repetitive_work_seconds.max(0),
        non_repetitive_seconds: (total_work_seconds - repetitive_work_seconds).max(0),
        rho_leeway_seconds: (-diff_to_goal_hours * 3600.0).max(0.0).round() as i64,
    }
}

#[derive(Clone)]
pub(super) struct TaskListDisplayRow {
    pub(super) scheduled_start: DateTime<Local>,
    pub(super) logical_naive_date_opt: Option<NaiveDate>,
    pub(super) rank: usize,
    pub(super) id: Uuid,
    pub(super) priority: i64,
    pub(super) work_seconds: i64,
    pub(super) project_category_opt: Option<ProjectCategory>,
    pub(super) is_real_task: bool,
    pub(super) give_up_candidate: bool,
    pub(super) display_row: TaskListRow,
}

impl TaskListDisplayRow {
    pub(super) fn new_message(
        scheduled_start: DateTime<Local>,
        rank: usize,
        id: Uuid,
        priority: i64,
        message: String,
    ) -> Self {
        TaskListDisplayRow {
            scheduled_start,
            logical_naive_date_opt: None,
            rank,
            id,
            priority,
            work_seconds: 0,
            project_category_opt: None,
            is_real_task: false,
            give_up_candidate: false,
            display_row: TaskListRow::Message { text: message },
        }
    }

    pub(super) fn new_gap(
        scheduled_start: DateTime<Local>,
        rank: usize,
        id: Uuid,
        priority: i64,
        minutes: i64,
    ) -> Self {
        let mut row = Self::new_message(scheduled_start, rank, id, priority, String::new());
        row.display_row = TaskListRow::Gap { minutes };
        row
    }

    #[allow(clippy::too_many_arguments)]
    fn new_spreadsheet_task(
        scheduled_start: DateTime<Local>,
        logical_naive_date: NaiveDate,
        rank: usize,
        id: Uuid,
        priority: i64,
        work_seconds: i64,
        project_category_opt: Option<ProjectCategory>,
        task_row: TaskListTaskRow,
    ) -> Self {
        TaskListDisplayRow {
            scheduled_start,
            logical_naive_date_opt: Some(logical_naive_date),
            rank,
            id,
            priority,
            work_seconds,
            project_category_opt,
            is_real_task: true,
            give_up_candidate: false,
            display_row: TaskListRow::Task(task_row),
        }
    }

    pub(super) fn into_display_row(mut self) -> TaskListRow {
        if let TaskListRow::Task(task_row) = &mut self.display_row {
            task_row.give_up_candidate = self.give_up_candidate;
        }
        self.display_row
    }
}

pub(super) const PROJECT_CATEGORY_SUMMARY_LEN: usize = 6;

fn project_category_summary_index(project_category_opt: Option<ProjectCategory>) -> usize {
    match project_category_opt {
        Some(ProjectCategory::Earning) => 0,
        Some(ProjectCategory::Sustaining) => 1,
        Some(ProjectCategory::Recovery) => 2,
        Some(ProjectCategory::Investment) => 3,
        Some(ProjectCategory::Consumption) => 4,
        None => 5,
    }
}

pub(super) fn summarize_scheduled_work_seconds_by_project_category(
    rows: &[TaskListDisplayRow],
) -> [i64; PROJECT_CATEGORY_SUMMARY_LEN] {
    let mut summary = [0; PROJECT_CATEGORY_SUMMARY_LEN];

    for row in rows.iter().filter(|row| row.is_real_task) {
        let index = project_category_summary_index(row.project_category_opt);
        summary[index] += row.work_seconds;
    }

    summary
}

#[cfg(test)]
pub(super) fn format_scheduled_work_seconds_by_project_category(
    summary: &[i64; PROJECT_CATEGORY_SUMMARY_LEN],
    denominator_seconds: i64,
) -> String {
    format_task_category_summary(&task_category_work_seconds(*summary), denominator_seconds)
}

fn task_category_work_seconds(
    summary: [i64; PROJECT_CATEGORY_SUMMARY_LEN],
) -> Vec<TaskCategoryWorkSeconds> {
    [
        Some(ProjectCategory::Earning),
        Some(ProjectCategory::Sustaining),
        Some(ProjectCategory::Recovery),
        Some(ProjectCategory::Investment),
        Some(ProjectCategory::Consumption),
        None,
    ]
    .into_iter()
    .zip(summary)
    .map(|(project_category, seconds)| TaskCategoryWorkSeconds {
        project_category,
        seconds,
    })
    .collect()
}

fn calculate_project_category_denominator_seconds(
    rows: &[TaskListDisplayRow],
    last_synced_time: DateTime<Local>,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    end_of_day_offset_minutes: i64,
) -> Result<i64, ApplicationError> {
    let mut dates = rows
        .iter()
        .filter(|row| row.is_real_task)
        .filter_map(|row| row.logical_naive_date_opt)
        .collect::<Vec<_>>();
    dates.sort();
    dates.dedup();

    dates.iter().try_fold(0, |total, date| {
        calculate_free_time_minutes_for_logical_date_with_end_of_day_offset_minutes(
            date,
            last_synced_time,
            free_time_manager,
            end_of_day_offset_minutes,
        )
        .map(|minutes| total + minutes * 60)
    })
}

pub(super) fn advance_display_datetime_cursor(
    current_datetime_cursor: DateTime<Local>,
    end_datetime: DateTime<Local>,
) -> DateTime<Local> {
    max(current_datetime_cursor, end_datetime)
}

pub(super) fn sort_task_list_display_rows(
    rows: &mut [TaskListDisplayRow],
    display_order: TaskListDisplayOrder,
) {
    match display_order {
        TaskListDisplayOrder::ScheduledStartDesc => {
            rows.reverse();
        }
        TaskListDisplayOrder::LowPriorityTail => {
            rows.sort_by(|a, b| {
                b.priority
                    .cmp(&a.priority)
                    .then_with(|| a.scheduled_start.cmp(&b.scheduled_start))
                    .then_with(|| a.rank.cmp(&b.rank))
                    .then_with(|| a.id.cmp(&b.id))
            });
        }
    }
}

pub(super) fn mark_give_up_candidate_rows(
    rows: &mut [TaskListDisplayRow],
    shortage_seconds: i64,
    target_date: NaiveDate,
) {
    if shortage_seconds <= 0 {
        return;
    }

    let mut candidate_indices: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            if row.is_real_task
                && row.work_seconds > 0
                && row.logical_naive_date_opt == Some(target_date)
            {
                Some(index)
            } else {
                None
            }
        })
        .collect();

    candidate_indices.sort_by(|a, b| {
        rows[*a]
            .priority
            .cmp(&rows[*b].priority)
            .then_with(|| rows[*b].scheduled_start.cmp(&rows[*a].scheduled_start))
            .then_with(|| rows[*b].rank.cmp(&rows[*a].rank))
            .then_with(|| rows[*b].id.cmp(&rows[*a].id))
    });

    let mut accumulated_seconds = 0;
    for index in candidate_indices {
        rows[index].give_up_candidate = true;
        accumulated_seconds += rows[index].work_seconds;

        if accumulated_seconds >= shortage_seconds {
            break;
        }
    }
}

pub(super) fn mark_give_up_candidate_rows_by_date(
    rows: &mut [TaskListDisplayRow],
    shortage_duration_by_date: &HashMap<NaiveDate, Duration>,
) {
    let mut dates_and_shortages = shortage_duration_by_date
        .iter()
        .filter_map(|(date, shortage_duration)| {
            if *shortage_duration > Duration::seconds(0) {
                Some((*date, shortage_duration.num_seconds()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    dates_and_shortages.sort_by_key(|a| a.0);

    for (date, shortage_seconds) in dates_and_shortages {
        mark_give_up_candidate_rows(rows, shortage_seconds, date);
    }
}

pub(super) fn calculate_rho_metrics(
    total_work_seconds: i64,
    repetitive_work_seconds: i64,
    available_minutes: i64,
) -> RhoMetrics {
    let total_work_hours = total_work_seconds as f64 / 3600.0;
    let repetitive_work_hours = repetitive_work_seconds as f64 / 3600.0;
    let non_repetitive_work_hours = (total_work_seconds - repetitive_work_seconds) as f64 / 3600.0;
    let available_hours = available_minutes as f64 / 60.0;
    let free_hours = available_hours - total_work_hours;

    let rho = if available_minutes > 0 {
        total_work_seconds as f64 / (available_minutes * 60) as f64
    } else {
        f64::INFINITY
    };

    let non_repetitive_available_hours = available_hours - repetitive_work_hours;
    let non_repetitive_rho = if non_repetitive_available_hours > 0.0 {
        non_repetitive_work_hours / non_repetitive_available_hours
    } else {
        f64::INFINITY
    };

    RhoMetrics {
        _total_work_hours: total_work_hours,
        repetitive_work_hours,
        non_repetitive_work_hours,
        _available_hours: available_hours,
        free_hours,
        rho,
        non_repetitive_rho,
    }
}

pub(super) fn calculate_lq_opt(rho: f64) -> Option<f64> {
    if rho < 1.0 {
        Some(rho / (1.0 - rho))
    } else {
        None
    }
}

pub(super) fn build_tree_display(
    focused_task_opt: &Option<TaskHandle>,
) -> Result<TreeDisplay, ApplicationError> {
    let mut rows = Vec::new();
    if let Some(focused_task) = focused_task_opt.as_ref() {
        let s = focused_task
            .tree_debug_pretty_print()
            .map_err(ApplicationError::TaskTree)?;
        for line in s.split('\n') {
            // Done([+])のタスクは表示しない
            // 恒久的には、tree_debug_pretty_print()に似た関数を自分で実装してカスタマイズする
            if line.contains("[ ]") || line.contains("[-]") {
                rows.push(DebugTreeRow {
                    debug: line.to_string(),
                });
            }
        }
    }
    Ok(TreeDisplay::Debug { rows })
}

pub(super) fn build_ancestor_tree_display(
    focused_task_opt: &Option<TaskHandle>,
) -> Result<TreeDisplay, ApplicationError> {
    // まずは葉タスクから根に向かいながら後ろに追加していき、
    // 最後に逆順にして表示する
    let mut ancestors: Vec<(DateTime<Local>, TaskHandle)> = vec![];

    if let Some(task) = focused_task_opt {
        ancestors = task
            .list_all_parent_tasks_with_first_available_time()
            .map_err(ApplicationError::TaskTree)?;
    }

    ancestors.reverse();

    let mut rows = Vec::with_capacity(ancestors.len());
    for (level, (first_available_datetime, task)) in ancestors.iter().enumerate() {
        let id = task.get_id().map_err(ApplicationError::TaskTree)?;
        let name = task.get_name().map_err(ApplicationError::TaskTree)?;
        let estimated_work_minutes = (task
            .get_estimated_work_seconds()
            .map_err(ApplicationError::TaskTree)? as f64
            / 60.0)
            .ceil() as i64;
        rows.push(AncestorTreeRow {
            level,
            task_id: id,
            first_available_date: first_available_datetime.date_naive(),
            estimated_minutes: estimated_work_minutes,
            name,
        });
    }
    Ok(TreeDisplay::Ancestors { rows })
}

pub(super) fn build_focus_header_display(
    focused_task: &TaskHandle,
) -> Result<FocusDisplay, ApplicationError> {
    Ok(FocusDisplay::Header {
        project_category: focused_task
            .get_project_category_opt()
            .map_err(ApplicationError::TaskTree)?,
        project_priority: focused_task
            .get_priority()
            .map_err(ApplicationError::TaskTree)?,
        task_attr: focused_task.get_attr(),
    })
}

pub(super) fn build_focus_timing_display(
    focused_task: &TaskHandle,
    focus_started_datetime: &DateTime<Local>,
    now: &DateTime<Local>,
) -> Result<FocusDisplay, ApplicationError> {
    Ok(FocusDisplay::Timing {
        estimated_work_seconds: focused_task
            .get_estimated_work_seconds()
            .map_err(ApplicationError::TaskTree)?,
        actual_work_seconds: focused_task
            .get_actual_work_seconds()
            .map_err(ApplicationError::TaskTree)?,
        focus_started_at: *focus_started_datetime,
        now: *now,
    })
}

pub(super) trait FocusDisplaySource {
    fn build_ancestors(&self) -> Result<DisplayModel, ApplicationError>;
    fn build_header(&self) -> Option<Result<FocusDisplay, ApplicationError>>;
    fn build_timing(&self) -> Option<Result<FocusDisplay, ApplicationError>>;
}

pub(super) struct TaskFocusDisplaySource<'a> {
    pub(super) focused_task_opt: Option<&'a TaskHandle>,
    pub(super) focus_started_datetime: &'a DateTime<Local>,
    pub(super) now: DateTime<Local>,
}

impl FocusDisplaySource for TaskFocusDisplaySource<'_> {
    fn build_ancestors(&self) -> Result<DisplayModel, ApplicationError> {
        build_ancestor_tree_display(&self.focused_task_opt.cloned()).map(DisplayModel::Tree)
    }

    fn build_header(&self) -> Option<Result<FocusDisplay, ApplicationError>> {
        self.focused_task_opt.map(build_focus_header_display)
    }

    fn build_timing(&self) -> Option<Result<FocusDisplay, ApplicationError>> {
        self.focused_task_opt.map(|focused_task| {
            build_focus_timing_display(focused_task, self.focus_started_datetime, &self.now)
        })
    }
}

pub(super) fn build_leaf_tree_display(
    task_repository: &mut dyn TaskRepositoryTrait,
) -> Result<TreeDisplay, ApplicationError> {
    let mut ans_tpls = vec![];

    for project_root_task in task_repository.get_all_projects().iter() {
        let project_name = project_root_task
            .get_name()
            .map_err(ApplicationError::TaskTree)?;

        // 優先度が高いタスクほど下に表示されるようにし、フォーカスが当たるタスクは末尾に表示されるようにする。
        let leaf_tasks = extract_leaf_tasks_from_project(project_root_task)
            .map_err(ApplicationError::TaskTree)?;
        for leaf_task in leaf_tasks.iter() {
            let deadline_time_opt = leaf_task
                .get_deadline_time_opt()
                .map_err(ApplicationError::TaskTree)?;
            let neg_priority = !leaf_task
                .get_priority()
                .map_err(ApplicationError::TaskTree)?;
            let id = leaf_task.get_id().map_err(ApplicationError::TaskTree)?;
            let task_debug = format!(
                "{:?}",
                leaf_task.get_attr().map_err(ApplicationError::TaskTree)?
            );

            let tpl = (
                deadline_time_opt.is_none(),
                neg_priority,
                deadline_time_opt,
                id,
                project_name.clone(),
                task_debug,
            );
            ans_tpls.push(tpl);
        }
    }

    ans_tpls.sort();
    ans_tpls.reverse();

    let row_count = ans_tpls.len();
    let rows = ans_tpls
        .into_iter()
        .enumerate()
        .map(
            |(index, (_, _, _, _, project_name, task_debug))| LeafTreeRow {
                remaining_count: row_count - index,
                project_name,
                task_debug,
            },
        )
        .collect();
    Ok(TreeDisplay::Leaves { rows })
}

// 集計用タプルはこの関数内だけで使用し、意味を持つ公開型を増やさない。
#[allow(clippy::type_complexity)]
pub(super) fn build_show_all_tasks_display_with_config(
    focused_task_id_opt: &mut Option<Uuid>,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    pattern_opt: &Option<String>,
    display_order: TaskListDisplayOrder,
    config: &SchronuConfig,
) -> Result<DisplayModel, ApplicationError> {
    let yyyymmdd_reg = Regex::new(r"^(\d{4})/(\d{2})/(\d{2})$").unwrap();
    let yyyymmdd_pattern_date = pattern_opt
        .as_ref()
        .and_then(|pattern| yyyymmdd_reg.captures(pattern))
        .map(|captures| {
            let invalid_calendar_date = || ApplicationError::InvalidInput {
                field: "pattern",
                reason: "invalid calendar date",
            };
            let year = captures[1]
                .parse::<i32>()
                .map_err(|_| invalid_calendar_date())?;
            let month = captures[2]
                .parse::<u32>()
                .map_err(|_| invalid_calendar_date())?;
            let day = captures[3]
                .parse::<u32>()
                .map_err(|_| invalid_calendar_date())?;
            NaiveDate::from_ymd_opt(year, month, day).ok_or_else(invalid_calendar_date)
        })
        .transpose()?;
    let scheduled_tasks = get_schedule(task_repository)?;
    let mut task_list_display_rows: Vec<TaskListDisplayRow> = vec![];
    let mut available_biggest_row_opt: Option<TaskListDisplayRow> = None;
    let mut available_biggest_task_estimate_work_seconds = 0;

    // ここからρ計算用
    let last_synced_time = task_repository.get_last_synced_time();
    let last_synced_logical_date = try_logical_date(last_synced_time)?;
    let next_logical_date_start = try_next_logical_date_start(last_synced_time)?;

    let eod = try_logical_date_end(last_synced_logical_date, config.end_of_day_offset_minutes)?;
    // ここまでρ計算用

    let is_calendar_func = pattern_opt
        .as_ref()
        .is_some_and(|pattern| pattern == "暦" || pattern == "calendar" || pattern == "cal");

    let is_band_func = pattern_opt
        .as_ref()
        .is_some_and(|pattern| pattern == "帯" || pattern == "band");

    let is_today_func = pattern_opt.as_ref().is_some_and(|pattern| pattern == "今");

    let is_daily_summary_func = is_calendar_func || is_band_func;

    // 日付ごとのタスク数を集計する
    let mut counter: HashMap<NaiveDate, usize> = HashMap::new();
    let mut total_estimated_work_seconds_of_the_date_counter: HashMap<NaiveDate, i64> =
        HashMap::new();
    let mut deadline_estimated_work_seconds_map: HashMap<NaiveDate, i64> = HashMap::new();

    let mut repetitive_task_estimated_work_seconds_map: HashMap<NaiveDate, i64> = HashMap::new();

    // 日ごとの、前倒し可能なtaskの見積もりの和
    let mut adjustable_estimated_work_seconds_map: HashMap<NaiveDate, i64> = HashMap::new();

    // タスク一覧で、どのタスクをいつやる見込みかを表示するために、「現在時刻」をズラして見ていく
    let mut current_datetime_cursor = task_repository.get_last_synced_time();
    let integer_reg = Regex::new(r"^\d+$").unwrap();
    let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];

    for (ind, scheduled_task) in scheduled_tasks.iter().enumerate() {
        let dt = &scheduled_task.first_available_time;
        let scheduled_start = &scheduled_task.scheduled_start;
        let scheduled_end = &scheduled_task.scheduled_end;
        let scheduled_work_seconds = scheduled_task.scheduled_work_seconds;
        let total_work_seconds = scheduled_task.total_work_seconds;
        let rank = &scheduled_task.rank;
        let deadline_time_opt = &scheduled_task.task.deadline_time;
        let id = &scheduled_task.task.id;
        let logical_naive_date = try_logical_date(*scheduled_start)?;
        let needs_scheduled_boundary = pattern_opt.as_ref().is_some_and(|pattern| {
            pattern == "今"
                || pattern == "明"
                || pattern == "近"
                || pattern == "暦"
                || pattern == "帯"
                || pattern == "週"
                || pattern == "末"
                || pattern == "翌"
                || days_of_week.contains(&pattern.as_str())
        });
        let scheduled_next_logical_date_start = needs_scheduled_boundary
            .then(|| try_next_logical_date_start(*scheduled_start))
            .transpose()?;

        // 表示期間を過ぎた未来task以降は一覧へ含めない
        if let Some(valid_days) = pattern_opt.as_deref().and_then(task_list_horizon_days) {
            if let Some(scheduled_boundary) = scheduled_next_logical_date_start {
                if scheduled_boundary - next_logical_date_start > Duration::days(valid_days) {
                    break;
                }
            }
        }

        counter
            .entry(logical_naive_date)
            .and_modify(|cnt| *cnt += 1)
            .or_insert(1);

        let task_opt = task_repository
            .get_by_id(*id)
            .map_err(ApplicationError::TaskTree)?;
        if let Some(task) = task_opt {
            let inherited_repetition_interval_days_opt = task
                .get_inherited_repetition_interval_days_opt()
                .map_err(ApplicationError::TaskTree)?;
            let mut repetition_prefix_label = "".to_string();

            if let Some(repetition_interval_days) = inherited_repetition_interval_days_opt {
                repetition_prefix_label = format!(
                    "{}【繰】({})",
                    repetition_prefix_label, repetition_interval_days
                );
            }

            let is_on_other_side = task
                .get_is_on_other_side()
                .map_err(ApplicationError::TaskTree)?;
            if is_on_other_side {
                repetition_prefix_label = format!("{}【待ち】", repetition_prefix_label);
            }

            // 前倒し可能なタスクの見積もり時間をカウントする
            let adjustable_prefix_label =
                get_adjustable_prefix_label(&task, *dt, *rank, last_synced_time)?;
            let task_estimated_work_seconds = task
                .get_estimated_work_seconds()
                .map_err(ApplicationError::TaskTree)?;
            let task_deadline_time_opt = task
                .get_deadline_time_opt()
                .map_err(ApplicationError::TaskTree)?;
            let task_priority = task.get_priority().map_err(ApplicationError::TaskTree)?;
            let task_project_category_opt = task
                .get_project_category_opt()
                .map_err(ApplicationError::TaskTree)?;

            if !adjustable_prefix_label.is_empty() {
                adjustable_estimated_work_seconds_map
                    .entry(logical_naive_date)
                    .and_modify(|estimated_work_seconds_val| {
                        *estimated_work_seconds_val += task_estimated_work_seconds
                    })
                    .or_insert(task_estimated_work_seconds);
            }

            let name = format!(
                "{}{}{}",
                adjustable_prefix_label,
                repetition_prefix_label,
                task.get_name().map_err(ApplicationError::TaskTree)?
            );
            let chars_vec: Vec<char> = name.chars().collect();

            let chars_width_acc: Vec<usize> = chars_vec
                .iter()
                .map(|&ch| UnicodeWidthChar::width(ch).unwrap_or(0))
                .scan(0, |acc, x| {
                    *acc += x;
                    Some(*acc)
                })
                .collect();

            let latest_index_opt =
                chars_width_acc
                    .iter()
                    .enumerate()
                    .find_map(|(index, &value)| {
                        if value > TASK_NAME_DISPLAY_WIDTH_LIMIT {
                            Some(index)
                        } else {
                            None
                        }
                    });

            let mut shorten_name: String = if let Some(latest_index) = latest_index_opt {
                format!(
                    "{}...",
                    chars_vec.iter().take(latest_index + 1).collect::<String>()
                )
            } else {
                name.to_string()
            };
            if total_work_seconds > scheduled_work_seconds {
                shorten_name = format!(
                    "<{}/{}>{}",
                    round_up_sec_as_minute(scheduled_work_seconds),
                    round_up_sec_as_minute(total_work_seconds),
                    shorten_name
                );
            }

            // 元々見積もり時間から作業済時間を引いたのが残りの見積もり時間
            // ただし、作業時間が元々の見積もり時間をオーバーしている時には既に想定外の事態になっているため、
            // 残りの見積もりを0とはせず、安全に倒して元々の見積もりの2倍として扱う
            let estimated_work_seconds = scheduled_work_seconds;
            if let Some(deadline_time) = deadline_time_opt {
                let deadline_naive_date = try_logical_date(*deadline_time)?;

                deadline_estimated_work_seconds_map
                    .entry(deadline_naive_date)
                    .and_modify(|deadline_estimated_work_seconds| {
                        *deadline_estimated_work_seconds += estimated_work_seconds
                    })
                    .or_insert(estimated_work_seconds);
            }

            if inherited_repetition_interval_days_opt.is_some() {
                repetitive_task_estimated_work_seconds_map
                    .entry(logical_naive_date)
                    .and_modify(|repetitive_task_estimated_work_seconds| {
                        *repetitive_task_estimated_work_seconds += estimated_work_seconds
                    })
                    .or_insert(estimated_work_seconds);
            }

            let current_datetime_cursor_clone = &current_datetime_cursor.clone();
            let start_datetime = scheduled_start;

            // 「今」か「明」か「近」の時のみ、日時カーソルが飛んだ場合には、その間の時間を表示する
            if (*scheduled_start - *current_datetime_cursor_clone).num_minutes() > 0 {
                let blank_duration = *scheduled_start - *current_datetime_cursor_clone;
                let tmp_id = Uuid::new_v4();

                if let Some(pattern) = pattern_opt {
                    if (pattern == "今" && *scheduled_start < next_logical_date_start)
                        || (pattern == "明"
                            && *current_datetime_cursor_clone >= next_logical_date_start
                            && (*scheduled_start - next_logical_date_start) < Duration::days(1))
                        || (pattern == "近"
                            && (*scheduled_start - next_logical_date_start) < Duration::days(1))
                    {
                        task_list_display_rows.push(TaskListDisplayRow::new_gap(
                            *current_datetime_cursor_clone,
                            0,
                            tmp_id,
                            0,
                            blank_duration.num_minutes(),
                        ));
                    }
                }
            }

            let end_datetime = *scheduled_end;
            current_datetime_cursor =
                advance_display_datetime_cursor(current_datetime_cursor, end_datetime);

            total_estimated_work_seconds_of_the_date_counter
                .entry(logical_naive_date)
                .and_modify(|estimated_work_seconds_val| {
                    *estimated_work_seconds_val += estimated_work_seconds
                })
                .or_insert(estimated_work_seconds);

            // ! : 今日中が締切。締切注意の意
            let deadline_icon: String = "!".to_string();

            // v : もっと着手を手前(下)にせよの意
            let breaking_deadline_icon: String = "v".to_string();

            // / : 今日着手する予定の葉タスク。/という記号自体に強い意味合いはない。
            let today_leaf_icon: String = "/".to_string();

            let icon = if task_deadline_time_opt.is_some()
                && task_deadline_time_opt.unwrap() < next_logical_date_start
                && task_deadline_time_opt.unwrap() < end_datetime
            {
                &breaking_deadline_icon
            } else if task_deadline_time_opt.is_some()
                && task_deadline_time_opt.unwrap() < next_logical_date_start
            {
                &deadline_icon
            } else if rank == &0 && scheduled_start < &eod {
                &today_leaf_icon
            } else {
                // - : 特に無しだが、空白にすると列数が乱れるので目立たない記号を入れる
                "-"
            };

            let deadline_string = if let Some(deadline_time) = deadline_time_opt {
                if *deadline_time < next_logical_date_start {
                    let breaking_minutes = (end_datetime - deadline_time).num_minutes().abs();
                    let breaking_hh = breaking_minutes / 60;
                    let breaking_mm = breaking_minutes % 60;

                    if *deadline_time < last_synced_time {
                        format!("+{:02}:{:02}ASAP", breaking_hh, breaking_mm)
                    } else if *deadline_time < end_datetime {
                        format!("+{:02}:{:02}____", breaking_hh, breaking_mm)
                    } else {
                        format!("____-{:02}:{:02}", breaking_hh, breaking_mm)
                    }
                } else {
                    let deadline_leeway_days = (*deadline_time - end_datetime).num_days().abs();

                    if deadline_leeway_days == 0 {
                        "________0D".to_string()
                    } else if *deadline_time > end_datetime {
                        format!("_____-{:03}D", deadline_leeway_days)
                    } else {
                        format!("_____+{:03}D", deadline_leeway_days)
                    }
                }
            } else {
                "____/__/__".to_string()
            };

            let task_row = TaskListTaskRow {
                rank: ind,
                task_id: *id,
                icon: icon.to_string(),
                remaining_time: deadline_string,
                scheduled_start: *start_datetime,
                scheduled_end: end_datetime,
                priority_rank: *rank,
                estimated_minutes: round_up_sec_as_minute(estimated_work_seconds),
                project_number_priority: task_priority,
                project_category: task_project_category_opt,
                task_name: shorten_name,
                give_up_candidate: false,
            };
            let task_search_text = task_list_search_text(&task_row);
            let task_list_display_row = TaskListDisplayRow::new_spreadsheet_task(
                *scheduled_start,
                logical_naive_date,
                *rank,
                *id,
                task_priority,
                estimated_work_seconds,
                task_project_category_opt,
                task_row,
            );
            let has_deadline_icon = icon == deadline_icon || icon == breaking_deadline_icon;
            let has_task_list_icon = has_deadline_icon || icon == today_leaf_icon;

            match pattern_opt {
                Some(pattern) => {
                    if pattern == "葉" {
                        if rank == &0
                            || task_deadline_time_opt.is_some()
                                && task_deadline_time_opt.unwrap() < next_logical_date_start
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "枝" {
                        if rank > &0 {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "印" {
                        if has_task_list_icon {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "〆" {
                        if has_deadline_icon {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if is_daily_summary_func {
                        // カレンダー表示機能を使う時には、タスク一覧は表示しない。
                    } else if pattern == "今" {
                        if scheduled_next_logical_date_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary == next_logical_date_start
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "明" {
                        if scheduled_next_logical_date_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary - next_logical_date_start == Duration::days(1)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "近" {
                        if scheduled_next_logical_date_start.is_some_and(|scheduled_boundary| {
                            let diff = scheduled_boundary - next_logical_date_start;
                            diff == Duration::zero() || diff == Duration::days(1)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "単" {
                        // 「単」はtask名ではなく、継承済みの繰り返し間隔の有無で単発taskを判定する
                        if inherited_repetition_interval_days_opt.is_none() {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if days_of_week.contains(&pattern.as_str()) {
                        // 月 火 水 木 金 土 日 が指定された時は、明日以降で、直近のその曜日のタスクを表示する
                        let now_weekday_jp = get_weekday_jp(&last_synced_logical_date);

                        let now_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == now_weekday_jp)
                            .unwrap();
                        let target_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == pattern.as_str())
                            .unwrap();

                        let ind_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                        // 今日のデータについては「全 今」で表示できるので、その代わりに、1週間後の同じ曜日の情報を表示するようにする
                        let days: i64 = if ind_diff == 0 { 7 } else { ind_diff as i64 };

                        if scheduled_next_logical_date_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary - next_logical_date_start == Duration::days(days)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "週" {
                        // 今日を含む直近1週間のタスクを表示する
                        if scheduled_next_logical_date_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary - next_logical_date_start < Duration::days(7)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "末" {
                        // 週末までのタスクを表示する
                        let now_weekday_jp = get_weekday_jp(&last_synced_logical_date);

                        let now_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == now_weekday_jp)
                            .unwrap();
                        let target_days_of_week_ind =
                            days_of_week.iter().position(|&x| x == "日").unwrap();

                        let days_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                        if scheduled_next_logical_date_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary - next_logical_date_start
                                <= Duration::days(days_diff as i64)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "翌" {
                        // 翌週末までのタスクを表示する
                        let now_weekday_jp = get_weekday_jp(&last_synced_logical_date);

                        let now_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == now_weekday_jp)
                            .unwrap();
                        let target_days_of_week_ind =
                            days_of_week.iter().position(|&x| x == "日").unwrap();

                        let days_diff =
                            ((7 + target_days_of_week_ind - now_days_of_week_ind) % 7) as i64;

                        if scheduled_next_logical_date_start.is_some_and(|scheduled_boundary| {
                            let diff = scheduled_boundary - next_logical_date_start;
                            Duration::days(days_diff) < diff
                                && diff <= Duration::days(days_diff + 7)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if let Some(pattern_date) = yyyymmdd_pattern_date {
                        if pattern_date == logical_naive_date {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if integer_reg.is_match(pattern) {
                        let caps = integer_reg.captures(pattern).unwrap();
                        let input_minute: i64 = caps[0].parse().unwrap();
                        let target_free_time_seconds = input_minute * 60;

                        if *scheduled_start > next_logical_date_start
                            || last_synced_time
                                < task.get_start_time().map_err(ApplicationError::TaskTree)?
                        {
                            continue;
                        }

                        if *rank == 0
                            && !is_on_other_side
                            && estimated_work_seconds < target_free_time_seconds
                            && estimated_work_seconds > available_biggest_task_estimate_work_seconds
                        {
                            available_biggest_task_estimate_work_seconds = estimated_work_seconds;

                            available_biggest_row_opt = Some(task_list_display_row.clone());
                        }
                    } else if name.to_lowercase().contains(&pattern.to_lowercase())
                        || task_search_text.contains(pattern)
                    {
                        task_list_display_rows.push(task_list_display_row.clone());
                    }
                }
                None => {
                    task_list_display_rows.push(task_list_display_row.clone());
                }
            }
        }
    }

    // 着手可能な最大のタスクを実施するモード
    if let Some(row) = available_biggest_row_opt {
        task_list_display_rows.push(row);
    }

    // 1日の残りの時間から稼働率ρを計算する
    let busy_minutes = max(
        0,
        free_time_manager.get_busy_minutes(&last_synced_time, &eod),
    );
    let naive_dt_today = last_synced_logical_date;
    let today_total_deadline_estimated_work_seconds =
        *total_estimated_work_seconds_of_the_date_counter
            .get(&naive_dt_today)
            .unwrap_or(&0);
    let today_total_deadline_estimated_work_minutes =
        (today_total_deadline_estimated_work_seconds as f64 / 60.0).ceil() as i64;
    let lambda_minutes = today_total_deadline_estimated_work_minutes + busy_minutes;
    let estimated_finish_at = last_synced_time + Duration::minutes(lambda_minutes);

    let mu_minutes = max(0, (eod - last_synced_time).num_minutes());
    let today_total_repetitive_estimated_work_seconds = *repetitive_task_estimated_work_seconds_map
        .get(&naive_dt_today)
        .unwrap_or(&0);
    let available_minutes = mu_minutes - busy_minutes;
    let rho_metrics = calculate_rho_metrics(
        today_total_deadline_estimated_work_seconds,
        today_total_repetitive_estimated_work_seconds,
        available_minutes,
    );
    let lq = calculate_lq_opt(rho_metrics.rho);
    let non_repetitive_lq_opt = calculate_lq_opt(rho_metrics.non_repetitive_rho);

    // 日付の小さい順にソートする
    let mut counter_arr: Vec<(&NaiveDate, &usize)> = counter.iter().collect();
    counter_arr.sort_by(|a, b| a.0.cmp(b.0));

    let mut daily_summary_rows: Vec<DailySummaryRow> = vec![];
    let mut shortage_duration_by_date: HashMap<NaiveDate, Duration> = HashMap::new();

    // 順調フラグ
    let mut has_today_deadline_leeway = true;
    let mut has_today_freetime_leeway = true;
    let mut has_today_new_task_leeway = true;
    let mut has_tomorrow_deadline_leeway = true;
    let mut has_tomorrow_freetime_leeway = true;
    let mut has_weekly_deadline_leeway = true;
    let mut has_weekly_freetime_leeway = true;

    // 「それぞれの日の rho (0.7) との差」の累積和。
    // どれくらい突発を吸収できるかの指標となる。
    // 元々は単に0.7との差で計算していたが、それだと0.7<rho<1.0でその日のタスクがなんとかなっているのに
    // 0.7との差の累積和が肥大化して使いものにならなかったため、以下の定義で計算するようにした。
    // ただし、特定の日にタスクを寄せて無理矢理rho<0.7の日を作るほうが良く見えてしまうので注意が必要。
    // rho < 0.7 : 累積和はそのぶん減る
    // 0.7<= rho <=1.0 : ノーカウント。その日のうちに吸収できる
    // 1.0 < rho : 累積和はそのぶん増える
    let mut accumulate_duration_diff_to_goal_rho = Duration::minutes(0);

    // 「それぞれの日の自由時間との差」の累積和
    let mut accumulate_duration_diff_to_limit = Duration::minutes(0);

    let mut first_caught_up_date = unreached_daily_summary_date();

    let mut first_leeway_date = unreached_daily_summary_date();
    let mut first_leeway_duration = Duration::seconds(0);

    let mut max_accumulate_duration_diff_to_limit = -Duration::hours(24);
    let mut max_accumulate_duration_diff_to_limit_date = unobserved_daily_summary_metric_date();

    let mut max_accumulated_rho_diff: f64 = -1.0;
    let mut max_accumulated_rho_diff_date = unobserved_daily_summary_metric_date();

    let max_counter_days = min(counter_arr.len(), DAILY_SUMMARY_HORIZON_DAYS);

    for (date, _cnt) in &counter_arr[0..max_counter_days] {
        let total_estimated_work_seconds_of_the_date: i64 =
            *total_estimated_work_seconds_of_the_date_counter
                .get(date)
                .unwrap_or(&0);
        let total_estimated_work_hours_of_the_date =
            total_estimated_work_seconds_of_the_date as f64 / 3600.0;

        let total_repetitive_task_work_seconds_of_the_date =
            *repetitive_task_estimated_work_seconds_map
                .get(date)
                .unwrap_or(&0);
        let total_repetitive_task_work_hours_of_the_date =
            total_repetitive_task_work_seconds_of_the_date as f64 / 3600.0;

        let cnt_of_the_date = *counter.get(date).unwrap_or(&0);

        let free_time_minutes =
            calculate_free_time_minutes_for_logical_date_with_end_of_day_offset_minutes(
                date,
                last_synced_time,
                free_time_manager,
                config.end_of_day_offset_minutes,
            )?;
        let full_day_free_time_minutes_opt = if is_band_func {
            Some(
                calculate_full_day_free_time_minutes_for_logical_date_with_end_of_day_offset_minutes(
                    date,
                    free_time_manager,
                    config.end_of_day_offset_minutes,
                )?,
            )
        } else {
            None
        };

        let free_time_hours = free_time_minutes as f64 / 60.0;
        let rho_in_date = total_estimated_work_hours_of_the_date / free_time_hours;
        let non_repetitive_rho_in_date =
            if free_time_hours - total_repetitive_task_work_hours_of_the_date > 0.0 {
                (total_estimated_work_hours_of_the_date
                    - total_repetitive_task_work_hours_of_the_date)
                    / (free_time_hours - total_repetitive_task_work_hours_of_the_date)
            } else {
                f64::INFINITY
            };

        let diff_to_goal = calculate_daily_rho_diff_hours(
            free_time_minutes,
            total_repetitive_task_work_seconds_of_the_date,
            total_estimated_work_seconds_of_the_date,
        );
        let diff_to_goal_sign: char = if diff_to_goal > 0.0 { ' ' } else { '-' };
        let diff_to_goal_hour = diff_to_goal.abs().floor();
        let diff_to_goal_minute = (diff_to_goal.abs() - diff_to_goal_hour) * 60.0;

        let over_time_hours_f = total_estimated_work_hours_of_the_date - free_time_hours;
        let over_time_hours = over_time_hours_f.abs().floor() as i64;
        let over_time_minutes = (over_time_hours_f.abs() * 60.0) as i64 % 60;

        let adjustable_estimated_work_seconds: i64 = *adjustable_estimated_work_seconds_map
            .get(date)
            .unwrap_or(&0);
        let adjustable_estimated_work_duration =
            Duration::seconds(adjustable_estimated_work_seconds);

        // これまでにどれだけ累積でマイナス(余裕)だったとしても、前倒しできるタスクの量でキャップされる
        if accumulate_duration_diff_to_limit < -adjustable_estimated_work_duration {
            accumulate_duration_diff_to_limit = -adjustable_estimated_work_duration
        }

        let over_time_duration = if over_time_hours_f > 0.0 {
            Duration::hours(over_time_hours) + Duration::minutes(over_time_minutes)
        } else {
            -Duration::hours(over_time_hours) - Duration::minutes(over_time_minutes)
        };
        accumulate_duration_diff_to_limit += over_time_duration;

        if accumulate_duration_diff_to_limit > max_accumulate_duration_diff_to_limit {
            max_accumulate_duration_diff_to_limit = accumulate_duration_diff_to_limit;
            max_accumulate_duration_diff_to_limit_date = **date;
        }
        shortage_duration_by_date.insert(**date, accumulate_duration_diff_to_limit);

        if !daily_summary_rows.is_empty()
            && accumulate_duration_diff_to_limit < Duration::seconds(0)
            && **date < first_caught_up_date
        {
            first_caught_up_date = **date;
        }

        let diff_to_limit_sign: char = if accumulate_duration_diff_to_limit > Duration::minutes(0) {
            ' '
        } else {
            '-'
        };

        let repetitive_task_estimated_work_seconds = *repetitive_task_estimated_work_seconds_map
            .get(date)
            .unwrap_or(&0);
        let repetitive_task_estimated_work_hours =
            repetitive_task_estimated_work_seconds as f64 / 3600.0;

        let non_repetitive_free_time_hours = free_time_hours - repetitive_task_estimated_work_hours;
        let accumulated_rho_diff = if free_time_hours - repetitive_task_estimated_work_hours > 0.0 {
            accumulate_duration_diff_to_limit.num_minutes() as f64
                / 60.0
                / non_repetitive_free_time_hours
        } else {
            f64::INFINITY
        };

        accumulate_duration_diff_to_goal_rho = if accumulated_rho_diff >= 0.0 {
            // タスクが捌けていない場合はそれがそのまま積み残される
            accumulate_duration_diff_to_limit
        } else if accumulated_rho_diff < RHO_GOAL - 1.0 && non_repetitive_rho_in_date < RHO_GOAL {
            // タスクが捌けてかなり余裕がある場合
            accumulate_duration_diff_to_goal_rho
                - Duration::hours(diff_to_goal_hour as i64)
                - Duration::minutes(diff_to_goal_minute as i64)
        } else if accumulated_rho_diff < 0.0 {
            // なんとかその日のうちに捌けている状態。積む余裕は無い
            Duration::minutes(0)
        } else {
            accumulate_duration_diff_to_goal_rho
        };

        if accumulate_duration_diff_to_goal_rho < Duration::minutes(0) && **date < first_leeway_date
        {
            first_leeway_date = **date;
            first_leeway_duration = accumulate_duration_diff_to_goal_rho;
        }

        let diff_to_limit_in_day_sign: char =
            if total_estimated_work_hours_of_the_date > free_time_hours {
                ' '
            } else {
                '-'
            };
        if !daily_summary_rows.is_empty()
            && accumulated_rho_diff.is_finite()
            && accumulated_rho_diff > max_accumulated_rho_diff
        {
            max_accumulated_rho_diff = accumulated_rho_diff;
            max_accumulated_rho_diff_date = **date;
        }

        let deadline_rest_duration_seconds: i64 =
            deadline_estimated_work_seconds_map.get(date).unwrap_or(&0)
                - (free_time_hours * 3600.0).floor() as i64;
        let deadline_rest_sign: char = if deadline_rest_duration_seconds > 0 {
            ' '
        } else {
            '-'
        };

        // 順調フラグ確認
        if daily_summary_rows.is_empty() {
            has_today_deadline_leeway = deadline_rest_sign == '-';
            has_today_freetime_leeway = diff_to_limit_in_day_sign == '-';
            has_today_new_task_leeway = diff_to_goal_sign == '-';
        }

        if daily_summary_rows.len() == 1 {
            has_tomorrow_deadline_leeway = deadline_rest_sign == '-';
            has_tomorrow_freetime_leeway = diff_to_limit_in_day_sign == '-';
        }

        // 一度フラグが折れていたら復活させない
        // 今日と明日については個別にアラートを出すので、判定はそれ以降について行う。
        if 2 <= daily_summary_rows.len()
            && daily_summary_rows.len() < 7
            && has_weekly_deadline_leeway
        {
            has_weekly_deadline_leeway = deadline_rest_sign == '-';
        }

        if 2 <= daily_summary_rows.len()
            && daily_summary_rows.len() < 7
            && has_weekly_freetime_leeway
        {
            has_weekly_freetime_leeway = diff_to_limit_sign == '-';
        }

        // 今日より前には前倒せないため
        let adjustable_estimated_work_hours = if daily_summary_rows.is_empty() {
            0.0
        } else {
            *adjustable_estimated_work_seconds_map
                .get(date)
                .unwrap_or(&0) as f64
                / 3600.0
        };

        let calendar_row = CalendarDayRow {
            date: **date,
            free_time_minutes,
            free_time_diff_minutes: over_time_duration.num_minutes(),
            adjustable_work_seconds: (adjustable_estimated_work_hours * 3600.0).round() as i64,
            rho_diff: rho_in_date - 1.0,
            rho_goal_diff_hours: diff_to_goal,
            accumulated_rho_goal_diff_minutes: accumulate_duration_diff_to_goal_rho.num_minutes(),
            deadline_diff_seconds: deadline_rest_duration_seconds,
            deadline_ratio: deadline_rest_duration_seconds as f64 / (free_time_hours * 60.0 * 60.0),
            accumulated_free_diff_minutes: accumulate_duration_diff_to_limit.num_minutes(),
            non_repetitive_free_minutes: (non_repetitive_free_time_hours * 60.0) as i64,
            accumulated_rho_diff,
            task_count: cnt_of_the_date,
        };

        let band_row = full_day_free_time_minutes_opt.map(|full_minutes| BandDayRow {
            date: **date,
            accumulated_rho_diff_seconds: accumulate_duration_diff_to_goal_rho.num_seconds(),
            accumulated_free_diff_seconds: accumulate_duration_diff_to_limit.num_seconds(),
            durations: calculate_daily_band_durations(
                **date == naive_dt_today,
                full_minutes,
                free_time_minutes,
                total_estimated_work_seconds_of_the_date,
                total_repetitive_task_work_seconds_of_the_date,
                diff_to_goal,
            ),
        });

        daily_summary_rows.push(DailySummaryRow {
            calendar_row,
            band_row,
        });
    }

    if !is_daily_summary_func {
        mark_give_up_candidate_rows_by_date(
            &mut task_list_display_rows,
            &shortage_duration_by_date,
        );
    }

    sort_task_list_display_rows(&mut task_list_display_rows, display_order);

    let task_list_display = if !is_daily_summary_func {
        for row in task_list_display_rows.iter() {
            *focused_task_id_opt = Some(row.id);
        }
        let project_category_summary =
            summarize_scheduled_work_seconds_by_project_category(&task_list_display_rows);
        let project_category_denominator_seconds = calculate_project_category_denominator_seconds(
            &task_list_display_rows,
            last_synced_time,
            free_time_manager,
            config.end_of_day_offset_minutes,
        )?;
        let category_work_seconds = task_category_work_seconds(project_category_summary);
        Some(DisplayModel::TaskList(TaskListDisplay {
            rows: task_list_display_rows
                .into_iter()
                .map(TaskListDisplayRow::into_display_row)
                .collect(),
            category_work_seconds,
            category_denominator_seconds: project_category_denominator_seconds,
        }))
    } else {
        None
    };

    let summary = CalendarSummary {
        last_synced_date: last_synced_time.date_naive(),
        first_caught_up_date,
        first_leeway_date,
        first_leeway_minutes: first_leeway_duration.num_minutes(),
        max_accumulated_free_diff_minutes: max_accumulate_duration_diff_to_limit.num_minutes(),
        max_accumulated_free_diff_date: max_accumulate_duration_diff_to_limit_date,
        max_accumulated_rho_diff,
        max_accumulated_rho_diff_date,
    };
    let alerts = CalendarAlerts {
        has_today_deadline_leeway,
        has_today_freetime_leeway,
        has_today_new_task_leeway,
        has_tomorrow_deadline_leeway,
        has_tomorrow_freetime_leeway,
        has_weekly_deadline_leeway,
        has_weekly_freetime_leeway,
    };
    let calendar_display = is_calendar_func.then(|| {
        DisplayModel::Calendar(CalendarDisplay {
            rows: daily_summary_rows
                .iter()
                .map(|row| row.calendar_row.clone())
                .collect(),
            blank_line_weekday: config.calendar_blank_line_weekday,
            summary: summary.clone(),
            alerts,
        })
    });

    let band_display = is_band_func.then(|| {
        DisplayModel::Band(BandDisplay {
            rows: daily_summary_rows
                .iter()
                .filter_map(|row| row.band_row.clone())
                .collect(),
            summary,
            alerts,
        })
    });

    let primary_display = task_list_display
        .or(calendar_display)
        .or(band_display)
        .expect("show-all view always builds one primary display");
    let trailing_display = if is_today_func || is_calendar_func || is_band_func {
        DisplayModel::TaskListMetrics(TaskListMetricsDisplay {
            busy_minutes,
            lambda_minutes,
            estimated_finish_at,
            non_repetitive_work_hours: rho_metrics.non_repetitive_work_hours,
            repetitive_work_hours: rho_metrics.repetitive_work_hours,
            free_hours: rho_metrics.free_hours,
            rho: rho_metrics.rho,
            non_repetitive_rho: rho_metrics.non_repetitive_rho,
            lq,
            non_repetitive_lq: non_repetitive_lq_opt,
        })
    } else {
        DisplayModel::Message {
            level: MessageLevel::Plain,
            text: String::new(),
        }
    };
    Ok(DisplayModel::Sequence(vec![
        primary_display,
        trailing_display,
    ]))
}
