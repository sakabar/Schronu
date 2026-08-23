pub(super) use super::renderer::project_category_symbol;
#[cfg(test)]
use super::renderer::{format_task_category_summary, format_task_list_row};
use super::renderer::{
    format_task_list_task_row, writeln_newline, AncestorTreeRow, DebugTreeRow, LeafTreeRow,
    SchronuWriter, TaskCategoryWorkSeconds, TaskListDisplay, TaskListRow, TaskListTaskRow,
    TreeDisplay,
};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Weekday};
use regex::Regex;
use schronu::adapter::gateway::schronu_config::SchronuConfig;
use schronu::application::daily_capacity::{
    calculate_daily_rho_diff_hours,
    calculate_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes,
    calculate_full_day_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes,
    try_next_business_day_start, try_subjective_date, try_subjective_date_end, RHO_GOAL,
};
use schronu::application::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use schronu::application::schedule_use_case::get_schedule;
use schronu::application::task_use_case::ApplicationError;
use schronu::entity::task::{
    extract_leaf_tasks_from_project, round_up_sec_as_minute, ProjectCategory, TaskHandle,
};
use std::cmp::{max, min};
use std::collections::HashMap;
use termion::color;
use unicode_width::UnicodeWidthChar;
use uuid::Uuid;

const FOCUS_PROGRESS_BAR_SEGMENTS: usize = 100;

pub(super) fn get_weekday_jp(date: &NaiveDate) -> &str {
    get_weekday_jp_from_weekday(date.weekday())
}

fn get_weekday_jp_from_weekday(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "月",
        Weekday::Tue => "火",
        Weekday::Wed => "水",
        Weekday::Thu => "木",
        Weekday::Fri => "金",
        Weekday::Sat => "土",
        Weekday::Sun => "日",
    }
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

    let planned_date = try_subjective_date(dt)?;
    let available_datetime = max(
        task.get_start_time().map_err(ApplicationError::TaskTree)?,
        last_synced_time,
    );
    let available_date = try_subjective_date(available_datetime)?;
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

const DAILY_BAND_SECONDS_PER_SEGMENT: i64 = 15 * 60;
pub(super) const DAILY_BAND_SEGMENTS: usize = 24 * 4;
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

struct DailySummaryRow {
    date: NaiveDate,
    calendar_message: String,
    band_message: String,
}

pub(super) struct DailyBandDurations {
    pub(super) fixed_seconds: i64,
    pub(super) elapsed_seconds: i64,
    pub(super) repetitive_seconds: i64,
    pub(super) non_repetitive_seconds: i64,
    pub(super) rho_leeway_seconds: i64,
}

pub(super) fn calculate_daily_band_durations(
    is_today: bool,
    full_day_free_minutes: i64,
    remaining_free_minutes: i64,
    total_work_seconds: i64,
    repetitive_work_seconds: i64,
    diff_to_goal_hours: f64,
) -> DailyBandDurations {
    DailyBandDurations {
        fixed_seconds: (SECONDS_PER_DAY - full_day_free_minutes.max(0) * 60).max(0),
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

fn round_daily_band_segment_count(seconds: i64) -> usize {
    let non_negative_seconds = seconds.max(0);
    ((non_negative_seconds.saturating_add(DAILY_BAND_SECONDS_PER_SEGMENT / 2))
        / DAILY_BAND_SECONDS_PER_SEGMENT) as usize
}

pub(super) fn format_signed_hours_minutes(duration: Duration) -> String {
    let sign = if duration >= Duration::zero() {
        '+'
    } else {
        '-'
    };
    let absolute_minutes = duration.num_seconds().unsigned_abs() / 60;

    format!(
        "{}{:02}:{:02}",
        sign,
        absolute_minutes / 60,
        absolute_minutes % 60
    )
}

fn format_daily_band_segment(symbol: char, count: usize, supports_ansi_color: bool) -> String {
    if count == 0 {
        return String::new();
    }
    if !supports_ansi_color {
        return symbol.to_string().repeat(count);
    }
    let color_value = match symbol {
        '#' => 110,
        'x' => 244,
        '=' => 33,
        '-' => 208,
        ':' => 28,
        '.' => 34,
        '>' => 196,
        _ => return symbol.to_string().repeat(count),
    };
    let symbols = symbol.to_string().repeat(count);
    format!(
        "{}{}{}",
        color::Fg(color::AnsiValue(color_value)),
        symbols,
        color::Fg(color::Reset)
    )
}

fn format_daily_band_legend(supports_ansi_color: bool) -> String {
    format!(
        "凡例: {} 固定  {} 経過済み  {} 繰返  {} 単発  {} 余差  {} 空き  {} 超過  (1文字=15分)",
        format_daily_band_segment('#', 1, supports_ansi_color),
        format_daily_band_segment('x', 1, supports_ansi_color),
        format_daily_band_segment('=', 1, supports_ansi_color),
        format_daily_band_segment('-', 1, supports_ansi_color),
        format_daily_band_segment(':', 1, supports_ansi_color),
        format_daily_band_segment('.', 1, supports_ansi_color),
        format_daily_band_segment('>', 1, supports_ansi_color),
    )
}

pub(super) fn format_daily_band(
    date: NaiveDate,
    weekday_jp: &str,
    accumulated_free_diff: Duration,
    accumulated_rho_diff: Duration,
    durations: &DailyBandDurations,
    supports_ansi_color: bool,
) -> String {
    let categories = [
        ('#', durations.fixed_seconds.max(0)),
        ('x', durations.elapsed_seconds.max(0)),
        ('=', durations.repetitive_seconds.max(0)),
        ('-', durations.non_repetitive_seconds.max(0)),
        (':', durations.rho_leeway_seconds.max(0)),
    ];
    let used_seconds = categories
        .iter()
        .fold(0_i64, |sum, (_, seconds)| sum.saturating_add(*seconds));
    let empty_seconds = SECONDS_PER_DAY.saturating_sub(used_seconds);
    let overflow_seconds = used_seconds.saturating_sub(SECONDS_PER_DAY);

    let mut bar = String::with_capacity(DAILY_BAND_SEGMENTS);
    let mut cumulative_seconds = 0_i64;
    let mut previous_boundary = 0_usize;

    for (symbol, seconds) in categories.into_iter().chain([('.', empty_seconds)]) {
        cumulative_seconds = cumulative_seconds.saturating_add(seconds);
        let boundary = round_daily_band_segment_count(cumulative_seconds.min(SECONDS_PER_DAY))
            .min(DAILY_BAND_SEGMENTS);
        bar.push_str(&format_daily_band_segment(
            symbol,
            boundary - previous_boundary,
            supports_ansi_color,
        ));
        previous_boundary = boundary;
    }

    let overflow = format_daily_band_segment(
        '>',
        round_daily_band_segment_count(overflow_seconds),
        supports_ansi_color,
    );
    format!(
        "{}({}) {} {} [{}]{}",
        date,
        weekday_jp,
        format_signed_hours_minutes(accumulated_rho_diff),
        format_signed_hours_minutes(accumulated_free_diff),
        bar,
        overflow
    )
}

#[derive(Clone)]
pub(super) struct TaskListDisplayRow {
    pub(super) scheduled_start: DateTime<Local>,
    pub(super) subjective_naive_date_opt: Option<NaiveDate>,
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
            subjective_naive_date_opt: None,
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
        subjective_naive_date: NaiveDate,
        rank: usize,
        id: Uuid,
        priority: i64,
        work_seconds: i64,
        project_category_opt: Option<ProjectCategory>,
        task_row: TaskListTaskRow,
    ) -> Self {
        TaskListDisplayRow {
            scheduled_start,
            subjective_naive_date_opt: Some(subjective_naive_date),
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

    #[cfg(test)]
    pub(super) fn render_message(&self) -> String {
        let mut display_row = self.display_row.clone();
        if let TaskListRow::Task(task_row) = &mut display_row {
            task_row.give_up_candidate = self.give_up_candidate;
        } else if self.is_real_task && self.give_up_candidate {
            if let TaskListRow::Message { text } = &mut display_row {
                *text = replace_task_list_icon(text, "A");
            }
        }
        format_task_list_row(&display_row)
    }

    fn into_display_row(mut self) -> TaskListRow {
        if let TaskListRow::Task(task_row) = &mut self.display_row {
            task_row.give_up_candidate = self.give_up_candidate;
        }
        self.display_row
    }
}

#[cfg(test)]
pub(super) fn replace_task_list_icon(message_prefix: &str, icon: &str) -> String {
    let mut token_ranges = message_prefix
        .char_indices()
        .filter(|(_, character)| !character.is_whitespace())
        .map(|(index, _)| index)
        .scan(None, |previous_index, index| {
            let starts_token = previous_index.is_none_or(|previous_index| {
                message_prefix[previous_index..index]
                    .chars()
                    .any(char::is_whitespace)
            });
            *previous_index = Some(index);
            Some(starts_token.then_some(index))
        })
        .flatten();
    let Some(icon_start) = token_ranges.nth(2) else {
        return message_prefix.to_string();
    };
    let icon_end = message_prefix[icon_start..]
        .find(char::is_whitespace)
        .map_or(message_prefix.len(), |offset| icon_start + offset);

    format!(
        "{}{}{}",
        &message_prefix[..icon_start],
        icon,
        &message_prefix[icon_end..]
    )
}

pub(super) const PROJECT_CATEGORY_SUMMARY_LEN: usize = 6;

pub(super) fn format_focused_task_header(project_category_opt: Option<ProjectCategory>) -> String {
    format!(
        "focused task is: project_category={}",
        project_category_symbol(project_category_opt)
    )
}

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
        .filter_map(|row| row.subjective_naive_date_opt)
        .collect::<Vec<_>>();
    dates.sort();
    dates.dedup();

    dates.iter().try_fold(0, |total, date| {
        calculate_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
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
                && row.subjective_naive_date_opt == Some(target_date)
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

pub(super) fn make_messages_about_focus(
    focused_task: &TaskHandle,
    focus_started_datetime: &DateTime<Local>,
    now: &DateTime<Local>,
) -> Result<[String; 2], ApplicationError> {
    let estimated_work_seconds = focused_task
        .get_estimated_work_seconds()
        .map_err(ApplicationError::TaskTree)?;
    let actual_work_seconds = focused_task
        .get_actual_work_seconds()
        .map_err(ApplicationError::TaskTree)?;
    let estimated_finish_datetime =
        *focus_started_datetime + Duration::seconds(estimated_work_seconds - actual_work_seconds);

    let left_duration = estimated_finish_datetime - *now;
    let for_duration = *now - *focus_started_datetime;
    let focusing_minutes = for_duration.num_minutes() + 1;
    let progress = format_focus_progress(
        estimated_work_seconds,
        actual_work_seconds,
        for_duration.num_seconds(),
    );

    let summary = format!(
        "{} (since {} until {}) focusing for {} minutes",
        if left_duration >= Duration::minutes(1) {
            format!("{} minutes left", left_duration.num_minutes())
        } else if left_duration >= Duration::seconds(0) {
            format!("{} seconds left", left_duration.num_seconds())
        } else {
            format!("{} minutes over", -left_duration.num_minutes() + 1)
        },
        focus_started_datetime.format("%H:%M:%S"),
        estimated_finish_datetime.format("%H:%M:%S"),
        focusing_minutes,
    );

    Ok([summary, progress])
}

pub(super) fn format_focus_progress(
    estimated_work_seconds: i64,
    actual_work_seconds: i64,
    focusing_seconds: i64,
) -> String {
    if estimated_work_seconds <= 0 {
        return format!("[{}] --%", "-".repeat(FOCUS_PROGRESS_BAR_SEGMENTS));
    }

    let total_work_seconds =
        (i128::from(actual_work_seconds) + i128::from(focusing_seconds)).max(0);
    let percentage = total_work_seconds * 100 / i128::from(estimated_work_seconds);
    let filled_segments = percentage.min(FOCUS_PROGRESS_BAR_SEGMENTS as i128) as usize;
    let overflow_segments = (percentage - 100).max(0) as usize;

    format!(
        "[{}{}]{} {}%",
        // U+2588 FULL BLOCK
        "█".repeat(filled_segments),
        // U+2591 LIGHT SHADE
        "░".repeat(FOCUS_PROGRESS_BAR_SEGMENTS - filled_segments),
        ">".repeat(overflow_segments),
        percentage
    )
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
pub(super) fn execute_show_all_tasks_with_config(
    stdout: &mut dyn SchronuWriter,
    focused_task_id_opt: &mut Option<Uuid>,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    pattern_opt: &Option<String>,
    display_order: TaskListDisplayOrder,
    config: &SchronuConfig,
) -> Result<Option<TaskListDisplay>, ApplicationError> {
    let supports_ansi_color = stdout.supports_ansi_color();
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
    let last_synced_subjective_date = try_subjective_date(last_synced_time)?;
    let next_business_day_start = try_next_business_day_start(last_synced_time)?;

    let eod = try_subjective_date_end(
        last_synced_subjective_date,
        config.end_of_day_offset_minutes,
    )?;
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

    // 日ごとの、前倒し可能なタスクの見積もりの和
    // 前倒し可能という決め方だと、何日まで前倒しできるのか曖昧性が発生する?
    let mut adjustable_estimated_work_seconds_map: HashMap<NaiveDate, i64> = HashMap::new();

    // 「暦」コマンドで、未来のサマリは見ても仕方ないので、直近の28日ぶん(配列の末尾)に絞る
    const SUMMARY_DAYS: usize = 28;

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
        let subjective_naive_date = try_subjective_date(*scheduled_start)?;
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
        let scheduled_next_business_day_start = needs_scheduled_boundary
            .then(|| try_next_business_day_start(*scheduled_start))
            .transpose()?;

        // 「今」「明」コマンドの場合は未来の情報には興味がないので、スキップする
        if let Some(pattern) = pattern_opt {
            if pattern == "今"
                || pattern == "明"
                || pattern == "近"
                || pattern == "暦"
                || pattern == "帯"
            {
                let valid_days = if pattern == "今" {
                    0
                } else if pattern == "明" || pattern == "近" {
                    1
                } else if pattern == "暦" || pattern == "帯" {
                    SUMMARY_DAYS as i64
                } else {
                    // 事前にif文で囲ってあるので、通常はこのケースに入ることはない
                    9999
                };

                if let Some(scheduled_boundary) = scheduled_next_business_day_start {
                    if scheduled_boundary - next_business_day_start > Duration::days(valid_days) {
                        break;
                    }
                }
            }
        }

        counter
            .entry(subjective_naive_date)
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
                // FIXME 【繰】というマジックナンバーが2ヶ所に登場していて危ない
                repetition_prefix_label = format!(
                    "{}【繰】({})",
                    repetition_prefix_label, repetition_interval_days
                );
            }

            if task
                .get_is_on_other_side()
                .map_err(ApplicationError::TaskTree)?
            {
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
                    .entry(subjective_naive_date)
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
            let max_len: usize = 70;

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
                    .find_map(
                        |(index, &value)| {
                            if value > max_len {
                                Some(index)
                            } else {
                                None
                            }
                        },
                    );

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
                let deadline_naive_date = try_subjective_date(*deadline_time)?;

                deadline_estimated_work_seconds_map
                    .entry(deadline_naive_date)
                    .and_modify(|deadline_estimated_work_seconds| {
                        *deadline_estimated_work_seconds += estimated_work_seconds
                    })
                    .or_insert(estimated_work_seconds);
            }

            if inherited_repetition_interval_days_opt.is_some() {
                repetitive_task_estimated_work_seconds_map
                    .entry(subjective_naive_date)
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
                    if (pattern == "今" && *scheduled_start < next_business_day_start)
                        || (pattern == "明"
                            && *current_datetime_cursor_clone >= next_business_day_start
                            && (*scheduled_start - next_business_day_start) < Duration::days(1))
                        || (pattern == "近"
                            && (*scheduled_start - next_business_day_start) < Duration::days(1))
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
                .entry(subjective_naive_date)
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
                && task_deadline_time_opt.unwrap() < next_business_day_start
                && task_deadline_time_opt.unwrap() < end_datetime
            {
                &breaking_deadline_icon
            } else if task_deadline_time_opt.is_some()
                && task_deadline_time_opt.unwrap() < next_business_day_start
            {
                &deadline_icon
            } else if rank == &0 && scheduled_start < &eod {
                &today_leaf_icon
            } else {
                // - : 特に無しだが、空白にすると列数が乱れるので目立たない記号を入れる
                "-"
            };

            let deadline_string = if let Some(deadline_time) = deadline_time_opt {
                if *deadline_time < next_business_day_start {
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
            let task_list_display_row = TaskListDisplayRow::new_spreadsheet_task(
                *scheduled_start,
                subjective_naive_date,
                *rank,
                *id,
                task_priority,
                estimated_work_seconds,
                task_project_category_opt,
                task_row,
            );
            let msg = match &task_list_display_row.display_row {
                TaskListRow::Task(task_row) => format_task_list_task_row(task_row),
                _ => unreachable!("spreadsheet task constructor must create a task row"),
            };
            let has_deadline_icon = icon == deadline_icon || icon == breaking_deadline_icon;
            let has_task_list_icon = has_deadline_icon || icon == today_leaf_icon;

            match pattern_opt {
                Some(pattern) => {
                    // FIXME 文字列マッチの絞り込み機能とその他の属性による絞り込みを機能を分ける
                    if pattern == "葉" {
                        if rank == &0
                            || task_deadline_time_opt.is_some()
                                && task_deadline_time_opt.unwrap() < next_business_day_start
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
                        if scheduled_next_business_day_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary == next_business_day_start
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "明" {
                        if scheduled_next_business_day_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary - next_business_day_start == Duration::days(1)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "近" {
                        if scheduled_next_business_day_start.is_some_and(|scheduled_boundary| {
                            let diff = scheduled_boundary - next_business_day_start;
                            diff == Duration::zero() || diff == Duration::days(1)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "単" {
                        // non_repetitive (単発) のタスクのみを表示する
                        // FIXME 【繰】が2ヶ所に登場していて危ない
                        if !msg.contains("【繰】") {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if days_of_week.contains(&pattern.as_str()) {
                        // 月 火 水 木 金 土 日 が指定された時は、明日以降で、直近のその曜日のタスクを表示する
                        let now_weekday_jp = get_weekday_jp(&last_synced_subjective_date);

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

                        if scheduled_next_business_day_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary - next_business_day_start == Duration::days(days)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "週" {
                        // 今日を含む直近1週間のタスクを表示する
                        if scheduled_next_business_day_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary - next_business_day_start < Duration::days(7)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "末" {
                        // 週末までのタスクを表示する
                        let now_weekday_jp = get_weekday_jp(&last_synced_subjective_date);

                        let now_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == now_weekday_jp)
                            .unwrap();
                        let target_days_of_week_ind =
                            days_of_week.iter().position(|&x| x == "日").unwrap();

                        let days_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                        if scheduled_next_business_day_start.is_some_and(|scheduled_boundary| {
                            scheduled_boundary - next_business_day_start
                                <= Duration::days(days_diff as i64)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "翌" {
                        // 翌週末までのタスクを表示する
                        let now_weekday_jp = get_weekday_jp(&last_synced_subjective_date);

                        let now_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == now_weekday_jp)
                            .unwrap();
                        let target_days_of_week_ind =
                            days_of_week.iter().position(|&x| x == "日").unwrap();

                        let days_diff =
                            ((7 + target_days_of_week_ind - now_days_of_week_ind) % 7) as i64;

                        if scheduled_next_business_day_start.is_some_and(|scheduled_boundary| {
                            let diff = scheduled_boundary - next_business_day_start;
                            Duration::days(days_diff) < diff
                                && diff <= Duration::days(days_diff + 7)
                        }) {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if let Some(pattern_date) = yyyymmdd_pattern_date {
                        if pattern_date == subjective_naive_date {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if integer_reg.is_match(pattern) {
                        let caps = integer_reg.captures(pattern).unwrap();
                        let input_minute: i64 = caps[0].parse().unwrap();
                        let target_free_time_seconds = input_minute * 60;

                        if *scheduled_start > next_business_day_start
                            || last_synced_time
                                < task.get_start_time().map_err(ApplicationError::TaskTree)?
                        {
                            continue;
                        }

                        // 【待ち】がマジックナンバーなのがちょっとよくない
                        if *rank == 0
                            && !msg.contains("【待ち】")
                            && estimated_work_seconds < target_free_time_seconds
                            && estimated_work_seconds > available_biggest_task_estimate_work_seconds
                        {
                            available_biggest_task_estimate_work_seconds = estimated_work_seconds;

                            available_biggest_row_opt = Some(task_list_display_row.clone());
                        }
                    } else if name.to_lowercase().contains(&pattern.to_lowercase())
                        || msg.contains(pattern)
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
    let busy_hours = busy_minutes as f64 / 60.0;
    let busy_s = format!("残り拘束時間は{:.1}時間です", busy_hours);

    let naive_dt_today = last_synced_subjective_date;
    let today_total_deadline_estimated_work_seconds =
        *total_estimated_work_seconds_of_the_date_counter
            .get(&naive_dt_today)
            .unwrap_or(&0);
    let today_total_deadline_estimated_work_minutes =
        (today_total_deadline_estimated_work_seconds as f64 / 60.0).ceil() as i64;
    let lambda_minutes = today_total_deadline_estimated_work_minutes + busy_minutes;
    let lambda_hours = lambda_minutes as f64 / 60.0;

    let estimated_finish_dt = last_synced_time + Duration::minutes(lambda_minutes);
    let s = format!(
        "完了見込み日時は{:.1}時間後の{}です",
        lambda_hours,
        estimated_finish_dt.format("%Y/%m/%d %H:%M:%S")
    );

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
    let lq1_opt = calculate_lq_opt(rho_metrics.rho);
    let non_repetitive_lq_opt = calculate_lq_opt(rho_metrics.non_repetitive_rho);

    let free_hours = rho_metrics.free_hours;
    let free_hours_sign = if free_hours >= 0.0 { '+' } else { '-' };
    let free_hours_hour: i64 = free_hours.abs().floor() as i64;
    let free_hours_minute: i64 = ((free_hours.abs() - free_hours_hour as f64) * 60.0) as i64;

    let non_repetitive_rho_msg = format!(
        "one ρ = ({:.2} + 0.00) / ({:.2} + 0.00 {} {} {} {}/60) = {:4.2}",
        rho_metrics.non_repetitive_work_hours,
        rho_metrics.non_repetitive_work_hours,
        free_hours_sign,
        free_hours_hour,
        free_hours_sign,
        free_hours_minute,
        rho_metrics.non_repetitive_rho,
    );
    let non_repetitive_lq_msg = match non_repetitive_lq_opt {
        Some(non_repetitive_lq) => format!("Lq = {:.1}", non_repetitive_lq),
        None => "Lq = inf".to_string(),
    };

    let s_for_non_repetitive_rho = format!("{}, {}", non_repetitive_rho_msg, non_repetitive_lq_msg);

    let rho1_msg = format!(
        "rep ρ = ({:.2} + {:.2}) / ({:.2} + {:.2} {} {} {} {}/60) = {:4.2}",
        rho_metrics.non_repetitive_work_hours,
        rho_metrics.repetitive_work_hours,
        rho_metrics.non_repetitive_work_hours,
        rho_metrics.repetitive_work_hours,
        free_hours_sign,
        free_hours_hour,
        free_hours_sign,
        free_hours_minute,
        rho_metrics.rho,
    );

    let lq_msg = match lq1_opt {
        Some(lq1) => format!("Lq = {:.1}", lq1),
        None => "Lq = inf".to_string(),
    };

    let s_for_rho1 = format!("{}, {}", rho1_msg, lq_msg);

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

    let mut first_caught_up_date = NaiveDate::from_ymd_opt(2037, 12, 31).unwrap();

    let mut first_leeway_date = NaiveDate::from_ymd_opt(2037, 12, 31).unwrap();
    let mut first_leeway_duration = Duration::seconds(0);

    let mut max_accumulate_duration_diff_to_limit = -Duration::hours(24);
    let mut max_accumulate_duration_diff_to_limit_date =
        NaiveDate::from_ymd_opt(1900, 1, 1).unwrap();

    let mut max_accumulated_rho_diff: f64 = -1.0;
    let mut max_accumulated_rho_diff_date = NaiveDate::from_ymd_opt(1900, 1, 1).unwrap();

    let max_counter_days = min(counter_arr.len(), SUMMARY_DAYS);

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

        let weekday_jp = get_weekday_jp(date);

        let free_time_minutes =
            calculate_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
                date,
                last_synced_time,
                free_time_manager,
                config.end_of_day_offset_minutes,
            )?;
        let full_day_free_time_minutes_opt = if is_band_func {
            Some(
                calculate_full_day_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
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

        let acc_diff_to_goal_sign: char =
            if accumulate_duration_diff_to_goal_rho > Duration::minutes(0) {
                ' '
            } else {
                '-'
            };

        let diff_to_limit_in_day_sign: char =
            if total_estimated_work_hours_of_the_date > free_time_hours {
                ' '
            } else {
                '-'
            };
        let diff_to_limit_hours_in_day: i64 = (total_estimated_work_hours_of_the_date
            - free_time_hours)
            .abs()
            .floor() as i64;
        let diff_to_limit_minutes_in_day: i64 =
            (((total_estimated_work_hours_of_the_date - free_time_hours).abs()
                - diff_to_limit_hours_in_day as f64)
                * 60.0)
                .floor() as i64;

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
        let deadline_rest_hours = deadline_rest_duration_seconds.abs() / 3600;
        let deadline_rest_minutes =
            deadline_rest_duration_seconds.abs() / 60 - deadline_rest_hours * 60;
        let deadline_rest_sign: char = if deadline_rest_duration_seconds > 0 {
            ' '
        } else {
            '-'
        };

        let indicator_about_deadline = format!(
            "{}{:.0}時間{:02.0}分\t{:5.2}",
            deadline_rest_sign,
            deadline_rest_hours,
            deadline_rest_minutes,
            deadline_rest_duration_seconds as f64 / (free_time_hours * 60.0 * 60.0),
        );

        let non_repetitive_free_time_sign = if non_repetitive_free_time_hours >= 0.0 {
            ' '
        } else {
            '-'
        };
        let indicator_about_diff_to_limit = format!(
            "{}{:02}時間{:02}分\t{}{:02}時間{:02}分\t{:5.2}",
            diff_to_limit_sign,
            accumulate_duration_diff_to_limit.num_hours().abs(),
            accumulate_duration_diff_to_limit.num_minutes().abs() % 60,
            non_repetitive_free_time_sign,
            non_repetitive_free_time_hours.abs().floor(),
            (non_repetitive_free_time_hours.abs() * 60.0) as i64 % 60,
            accumulated_rho_diff,
        );

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

        let adjustable_estimated_work_rate = adjustable_estimated_work_hours / free_time_hours;

        let adjustable_estimated_work_hours_str = if adjustable_estimated_work_hours == 0.0 {
            // "({:02.0}%)"と同じ幅になるようにする
            "     ".to_string()
        } else {
            format!("({:02.0}%)", adjustable_estimated_work_rate * 100.0)
        };

        let s = format!(
            "{}({})\t{:4.1}時間\t{}{:.0}時間{:02.0}分{}\t{:5.2}\t{}{:.0}時間{:02.0}分\t{}{:02}時間{:02}分\t{}\t{}\t{:02}[タスク]",
            date,
            weekday_jp,

            free_time_hours,

            diff_to_limit_in_day_sign,
            diff_to_limit_hours_in_day,
            diff_to_limit_minutes_in_day,
            adjustable_estimated_work_hours_str,

            rho_in_date - 1.0,

            diff_to_goal_sign,
            diff_to_goal_hour,
            diff_to_goal_minute,

            acc_diff_to_goal_sign,
            accumulate_duration_diff_to_goal_rho.num_hours().abs(),
            accumulate_duration_diff_to_goal_rho.num_minutes().abs() % 60,

            indicator_about_deadline,
            indicator_about_diff_to_limit,

            cnt_of_the_date,
        );

        let band_message =
            full_day_free_time_minutes_opt.map_or_else(String::new, |full_minutes| {
                format_daily_band(
                    **date,
                    weekday_jp,
                    accumulate_duration_diff_to_limit,
                    accumulate_duration_diff_to_goal_rho,
                    &calculate_daily_band_durations(
                        **date == naive_dt_today,
                        full_minutes,
                        free_time_minutes,
                        total_estimated_work_seconds_of_the_date,
                        total_repetitive_task_work_seconds_of_the_date,
                        diff_to_goal,
                    ),
                    supports_ansi_color,
                )
            });

        daily_summary_rows.push(DailySummaryRow {
            date: **date,
            calendar_message: s,
            band_message,
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
        Some(TaskListDisplay {
            rows: task_list_display_rows
                .into_iter()
                .map(TaskListDisplayRow::into_display_row)
                .collect(),
            category_work_seconds,
            category_denominator_seconds: project_category_denominator_seconds,
        })
    } else {
        None
    };

    // 逆順にして、下側に直近の日付があるようにする
    daily_summary_rows.reverse();

    let write_daily_summary = |stdout: &mut dyn SchronuWriter| {
        let clear_date_info = format!(
            "今のタスクが片付く日付: {}日後の{}",
            (first_caught_up_date - last_synced_time.date_naive()).num_days(),
            first_caught_up_date
        );

        let first_leeway_date_info = format!(
            "次にタスクを積める日付: {}日後の{} (-{}時間{:02}分)",
            (first_leeway_date - last_synced_time.date_naive()).num_days(),
            first_leeway_date,
            first_leeway_duration.num_hours().abs(),
            first_leeway_duration.num_minutes().abs() % 60,
        );

        let max_hours_sign = if max_accumulate_duration_diff_to_limit >= Duration::seconds(0) {
            ' '
        } else {
            '-'
        };
        let max_hours = max_accumulate_duration_diff_to_limit.num_hours().abs();
        let max_minutes = max_accumulate_duration_diff_to_limit.num_minutes().abs() % 60;
        let max_info = format!(
            "最大の累積時間: {}{:02}時間{:02}分 ({}), 最大のrhoの差: {:.2} ({}), {}",
            max_hours_sign,
            max_hours,
            max_minutes,
            max_accumulate_duration_diff_to_limit_date,
            max_accumulated_rho_diff,
            max_accumulated_rho_diff_date,
            first_leeway_date_info,
        );

        writeln_newline(stdout, &clear_date_info).unwrap();
        writeln_newline(stdout, &max_info).unwrap();
        writeln_newline(stdout, "").unwrap();

        let mut is_all_favorable = true;

        if !has_today_deadline_leeway {
            writeln_newline(stdout, "[Crit] 【今日の】〆切に間に合いません。【ただちに】〆切をリスケする調整をしてください。").unwrap();
            is_all_favorable = false;
        }

        if has_today_freetime_leeway {
            if !has_today_new_task_leeway {
                writeln_newline(stdout, "[Warn] 脇道に逸れずに予定の遂行をしてください。見積もりを間違えたり突発タスクが発生したりした場合に終了予定時刻に間に合わなくなる可能性があります。").unwrap();
                is_all_favorable = false;
            }
        } else {
            writeln_newline(stdout, "[Crit] 【今日の】終了予定時刻に間に合いません。【ただちに】どれかの予定を諦めて明日以降に延期してください。").unwrap();
            is_all_favorable = false;
        }

        if !has_tomorrow_deadline_leeway {
            writeln_newline(stdout, "[Warn] 【明日の】〆切に間に合いません。〆切をあさって以降にリスケする調整を【今日中に】してください。").unwrap();
            is_all_favorable = false;
        }

        if !has_tomorrow_freetime_leeway {
            writeln_newline(stdout, "[Warn] 【明日の】終了予定時刻に間に合いません。【今日中に】どれかの予定を諦めてあさって以降に延期してください。").unwrap();
            is_all_favorable = false;
        }

        if !has_weekly_deadline_leeway {
            writeln_newline(stdout, "[Warn] 【1週間以内の】〆切に間に合いません。【近々】どれかの予定を諦めて来週以降に延期してください。").unwrap();
            is_all_favorable = false;
        }

        if !has_weekly_freetime_leeway {
            writeln_newline(stdout, "[Warn] 【1週間以内の】終了予定時刻に間に合いません。【近々】どれかの予定を諦めて来週以降に延期してください。").unwrap();
            is_all_favorable = false;
        }

        if is_all_favorable {
            writeln_newline(
                stdout,
                "[Info] 順調です。突発タスクに対応したり1日の終わり際にタスクを新しく積んだりする余裕があります。ひとまずは脇道に逸れずに予定の遂行をしてください。",
            )
            .unwrap();
        }

        writeln_newline(stdout, "").unwrap();
    };

    if is_calendar_func {
        for (cal_ind, row) in daily_summary_rows.iter().enumerate() {
            writeln_newline(stdout, &row.calendar_message).unwrap();

            if row.calendar_message.contains(&format!(
                "({})",
                get_weekday_jp_from_weekday(config.calendar_blank_line_weekday)
            )) && cal_ind != daily_summary_rows.len() - 1
            {
                writeln_newline(stdout, "").unwrap();
            }
        }
        // フッター
        let footer: String = [
            "日          ",
            "空          ",
            "空差      ",
            "空差比",
            "余差    ",
            "余差累    ",
            "〆差      ",
            "〆差比",
            "空差累    ",
            "単発余暇",
            "空差累比",
            "タスク数",
        ]
        .join("\t");
        writeln_newline(stdout, &footer).unwrap();
        writeln_newline(stdout, "").unwrap();
        write_daily_summary(stdout);
    } else if is_band_func {
        writeln_newline(stdout, &format_daily_band_legend(supports_ansi_color)).unwrap();
        writeln_newline(stdout, "").unwrap();

        for (band_ind, row) in daily_summary_rows.iter().enumerate() {
            writeln_newline(stdout, &row.band_message).unwrap();

            if row.date.weekday() == Weekday::Mon && band_ind != daily_summary_rows.len() - 1 {
                writeln_newline(stdout, "").unwrap();
            }
        }
        writeln_newline(stdout, "").unwrap();
        write_daily_summary(stdout);
    }

    if is_today_func || is_calendar_func || is_band_func {
        writeln_newline(stdout, &busy_s).unwrap();
        writeln_newline(stdout, &s).unwrap();
        writeln_newline(stdout, &s_for_rho1).unwrap();
        writeln_newline(stdout, &s_for_non_repetitive_rho).unwrap();
    }

    writeln_newline(stdout, "").unwrap();
    Ok(task_list_display)
}
