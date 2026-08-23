use chrono::{DateTime, Datelike, Local, NaiveDate, Weekday};
use schronu::entity::task::ProjectCategory;
use std::io::{IsTerminal, Stdout, Write};
use termion::color;
use termion::raw::RawTerminal;
use uuid::Uuid;

pub(super) const MAX_COL: u16 = 999;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SpreadsheetTaskRow<'a> {
    pub(super) rank: &'a str,
    pub(super) task_id: &'a str,
    pub(super) icon: &'a str,
    pub(super) remaining_time: &'a str,
    pub(super) scheduled_time: &'a str,
    pub(super) priority: &'a str,
    pub(super) estimated_minutes: &'a str,
    pub(super) project_number: &'a str,
    pub(super) category: &'a str,
    pub(super) task_name: &'a str,
}

pub(super) fn format_spreadsheet_task_row(row: &SpreadsheetTaskRow<'_>) -> String {
    format!(
        "{} {} {} {} {} {} {} {} {} {}",
        row.rank,
        row.task_id,
        row.icon,
        row.remaining_time,
        row.scheduled_time,
        row.priority,
        row.estimated_minutes,
        row.project_number,
        row.category,
        row.task_name,
    )
}

pub(super) trait SchronuWriter: Write {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error>;

    fn supports_ansi_color(&self) -> bool {
        true
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum DisplayFragment {
    Raw(Vec<u8>),
    Newline(String),
    Flush,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MessageLevel {
    Plain,
    #[allow(dead_code)] // Adopted by later display-model migrations.
    Info,
    #[allow(dead_code)] // Adopted by later display-model migrations.
    Warn,
    #[allow(dead_code)] // Adopted by later display-model migrations.
    Critical,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DebugTreeRow {
    pub(super) debug: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AncestorTreeRow {
    pub(super) level: usize,
    pub(super) task_id: Uuid,
    pub(super) first_available_date: NaiveDate,
    pub(super) estimated_minutes: i64,
    pub(super) name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LeafTreeRow {
    pub(super) remaining_count: usize,
    pub(super) project_name: String,
    pub(super) task_debug: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum TreeDisplay {
    Debug { rows: Vec<DebugTreeRow> },
    Ancestors { rows: Vec<AncestorTreeRow> },
    Leaves { rows: Vec<LeafTreeRow> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TaskListTaskRow {
    pub(super) rank: usize,
    pub(super) task_id: Uuid,
    pub(super) icon: String,
    pub(super) remaining_time: String,
    pub(super) scheduled_start: DateTime<Local>,
    pub(super) scheduled_end: DateTime<Local>,
    pub(super) priority_rank: usize,
    pub(super) estimated_minutes: i64,
    pub(super) project_number_priority: i64,
    pub(super) project_category: Option<ProjectCategory>,
    pub(super) task_name: String,
    pub(super) give_up_candidate: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TaskListIconMode {
    Original,
    ApplyGiveUpCandidate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TaskListColumns {
    rank: String,
    task_id: String,
    icon: String,
    remaining_time: String,
    scheduled_time: String,
    priority: String,
    estimated_minutes: String,
    project_number: String,
    category: String,
    task_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum TaskListRow {
    Task(TaskListTaskRow),
    Gap { minutes: i64 },
    Message { text: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TaskCategoryWorkSeconds {
    pub(super) project_category: Option<ProjectCategory>,
    pub(super) seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TaskListDisplay {
    pub(super) rows: Vec<TaskListRow>,
    pub(super) category_work_seconds: Vec<TaskCategoryWorkSeconds>,
    pub(super) category_denominator_seconds: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CalendarDayRow {
    pub(super) date: NaiveDate,
    pub(super) free_time_minutes: i64,
    pub(super) free_time_diff_minutes: i64,
    pub(super) adjustable_work_seconds: i64,
    pub(super) rho_diff: f64,
    pub(super) rho_goal_diff_hours: f64,
    pub(super) accumulated_rho_goal_diff_minutes: i64,
    pub(super) deadline_diff_seconds: i64,
    pub(super) deadline_ratio: f64,
    pub(super) accumulated_free_diff_minutes: i64,
    pub(super) non_repetitive_free_minutes: i64,
    pub(super) accumulated_rho_diff: f64,
    pub(super) task_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CalendarSummary {
    pub(super) last_synced_date: NaiveDate,
    pub(super) first_caught_up_date: NaiveDate,
    pub(super) first_leeway_date: NaiveDate,
    pub(super) first_leeway_minutes: i64,
    pub(super) max_accumulated_free_diff_minutes: i64,
    pub(super) max_accumulated_free_diff_date: NaiveDate,
    pub(super) max_accumulated_rho_diff: f64,
    pub(super) max_accumulated_rho_diff_date: NaiveDate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CalendarAlerts {
    pub(super) has_today_deadline_leeway: bool,
    pub(super) has_today_freetime_leeway: bool,
    pub(super) has_today_new_task_leeway: bool,
    pub(super) has_tomorrow_deadline_leeway: bool,
    pub(super) has_tomorrow_freetime_leeway: bool,
    pub(super) has_weekly_deadline_leeway: bool,
    pub(super) has_weekly_freetime_leeway: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CalendarDisplay {
    pub(super) rows: Vec<CalendarDayRow>,
    pub(super) blank_line_weekday: Weekday,
    pub(super) summary: CalendarSummary,
    pub(super) alerts: CalendarAlerts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BandDurations {
    pub(super) fixed_seconds: i64,
    pub(super) elapsed_seconds: i64,
    pub(super) repetitive_seconds: i64,
    pub(super) non_repetitive_seconds: i64,
    pub(super) rho_leeway_seconds: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BandDayRow {
    pub(super) date: NaiveDate,
    pub(super) accumulated_rho_diff_seconds: i64,
    pub(super) accumulated_free_diff_seconds: i64,
    pub(super) durations: BandDurations,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct BandDisplay {
    pub(super) rows: Vec<BandDayRow>,
    pub(super) summary: CalendarSummary,
    pub(super) alerts: CalendarAlerts,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PackRow {
    pub(super) source_date: NaiveDate,
    pub(super) target_date: NaiveDate,
    pub(super) work_seconds: i64,
    pub(super) priority: i64,
    pub(super) task_id: Uuid,
    pub(super) name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PackDisplay {
    pub(super) rows: Vec<PackRow>,
    pub(super) skipped_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum DisplayModel {
    Legacy {
        fragments: Vec<DisplayFragment>,
    },
    Message {
        level: MessageLevel,
        text: String,
    },
    Tree(TreeDisplay),
    TaskList(TaskListDisplay),
    Calendar(CalendarDisplay),
    Band(BandDisplay),
    Pack(PackDisplay),
    #[allow(dead_code)] // Composition boundary for later typed display models.
    Sequence(Vec<DisplayModel>),
}

impl Default for DisplayModel {
    fn default() -> Self {
        Self::Legacy {
            fragments: Vec::new(),
        }
    }
}

impl DisplayModel {
    #[allow(dead_code)] // Legacy callers remain covered until their dedicated migration commits.
    pub(super) fn newline(message: impl Into<String>) -> Self {
        Self::Legacy {
            fragments: vec![DisplayFragment::Newline(message.into())],
        }
    }

    pub(super) fn flush() -> Self {
        Self::Legacy {
            fragments: vec![DisplayFragment::Flush],
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        match self {
            Self::Legacy { fragments } => fragments.is_empty(),
            Self::Message { .. } => false,
            Self::Tree(_)
            | Self::TaskList(_)
            | Self::Calendar(_)
            | Self::Band(_)
            | Self::Pack(_) => false,
            Self::Sequence(models) => models.iter().all(Self::is_empty),
        }
    }

    #[allow(dead_code)] // DisplayRecorder compatibility is retained during incremental migration.
    pub(super) fn fragments(&self) -> &[DisplayFragment] {
        match self {
            Self::Legacy { fragments } => fragments,
            Self::Message { .. }
            | Self::Tree(_)
            | Self::TaskList(_)
            | Self::Calendar(_)
            | Self::Band(_)
            | Self::Pack(_)
            | Self::Sequence(_) => {
                unreachable!("semantic display models do not expose legacy fragments")
            }
        }
    }

    fn legacy_fragments_mut(&mut self) -> &mut Vec<DisplayFragment> {
        match self {
            Self::Legacy { fragments } => fragments,
            Self::Message { .. }
            | Self::Tree(_)
            | Self::TaskList(_)
            | Self::Calendar(_)
            | Self::Band(_)
            | Self::Pack(_)
            | Self::Sequence(_) => {
                unreachable!("DisplayRecorder always owns a legacy display model")
            }
        }
    }
}

pub(super) struct DisplayRecorder {
    model: DisplayModel,
    supports_ansi_color: bool,
}

impl Default for DisplayRecorder {
    fn default() -> Self {
        Self {
            model: DisplayModel::default(),
            supports_ansi_color: true,
        }
    }
}

impl DisplayRecorder {
    pub(super) fn with_ansi_color(supports_ansi_color: bool) -> Self {
        Self {
            model: DisplayModel::default(),
            supports_ansi_color,
        }
    }

    pub(super) fn model(&self) -> &DisplayModel {
        &self.model
    }
}

impl Write for DisplayRecorder {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.model
            .legacy_fragments_mut()
            .push(DisplayFragment::Raw(buffer.to_vec()));
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.model
            .legacy_fragments_mut()
            .push(DisplayFragment::Flush);
        Ok(())
    }
}

impl SchronuWriter for DisplayRecorder {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        self.model
            .legacy_fragments_mut()
            .push(DisplayFragment::Newline(message.to_string()));
        Ok(())
    }

    fn supports_ansi_color(&self) -> bool {
        self.supports_ansi_color
    }
}

pub(super) fn render_display_model(
    writer: &mut dyn SchronuWriter,
    model: &DisplayModel,
) -> Result<(), std::io::Error> {
    match model {
        DisplayModel::Legacy { fragments } => {
            for fragment in fragments {
                match fragment {
                    DisplayFragment::Raw(buffer) => writer.write_all(buffer)?,
                    DisplayFragment::Newline(message) => writer.writeln_newline(message)?,
                    DisplayFragment::Flush => writer.flush()?,
                }
            }
        }
        DisplayModel::Message { level, text } => {
            let prefix = match level {
                MessageLevel::Plain => "",
                MessageLevel::Info => "[Info] ",
                MessageLevel::Warn => "[Warn] ",
                MessageLevel::Critical => "[Crit] ",
                MessageLevel::Error => "[Error] ",
            };
            writer.writeln_newline(&format!("{prefix}{text}"))?;
        }
        DisplayModel::Tree(tree) => render_tree_display(writer, tree)?,
        DisplayModel::TaskList(task_list) => render_task_list_display(writer, task_list)?,
        DisplayModel::Calendar(calendar) => render_calendar_display(writer, calendar)?,
        DisplayModel::Band(band) => render_band_display(writer, band)?,
        DisplayModel::Pack(pack) => render_pack_display(writer, pack)?,
        DisplayModel::Sequence(models) => {
            for model in models {
                render_display_model(writer, model)?;
            }
        }
    }
    Ok(())
}

fn render_task_list_display(
    writer: &mut dyn SchronuWriter,
    display: &TaskListDisplay,
) -> Result<(), std::io::Error> {
    for row in &display.rows {
        writer.writeln_newline(&format_task_list_row(row))?;
    }
    writer.writeln_newline("")?;
    writer.writeln_newline(&format_task_category_summary(
        &display.category_work_seconds,
        display.category_denominator_seconds,
    ))?;
    writer.writeln_newline("")?;
    Ok(())
}

fn render_calendar_display(
    writer: &mut dyn SchronuWriter,
    display: &CalendarDisplay,
) -> Result<(), std::io::Error> {
    for (index, row) in display.rows.iter().rev().enumerate() {
        writer.writeln_newline(&format_calendar_day_row(row))?;
        if row.date.weekday() == display.blank_line_weekday && index + 1 < display.rows.len() {
            writer.writeln_newline("")?;
        }
    }
    writer.writeln_newline(
        "日          \t空          \t空差      \t空差比\t余差    \t余差累    \t〆差      \t〆差比\t空差累    \t単発余暇\t空差累比\tタスク数",
    )?;
    writer.writeln_newline("")?;
    render_calendar_summary(writer, &display.summary)?;
    render_calendar_alerts(writer, display.alerts)?;
    Ok(())
}

const BAND_SECONDS_PER_SEGMENT: i64 = 15 * 60;
pub(super) const BAND_SEGMENTS: usize = 24 * 4;
pub(super) const BAND_SECONDS_PER_DAY: i64 = BAND_SEGMENTS as i64 * BAND_SECONDS_PER_SEGMENT;

fn render_band_display(
    writer: &mut dyn SchronuWriter,
    display: &BandDisplay,
) -> Result<(), std::io::Error> {
    let supports_ansi_color = writer.supports_ansi_color();
    writer.writeln_newline(&format_band_legend(supports_ansi_color))?;
    writer.writeln_newline("")?;
    for (index, row) in display.rows.iter().rev().enumerate() {
        writer.writeln_newline(&format_band_day_row(row, supports_ansi_color))?;
        if row.date.weekday() == Weekday::Mon && index + 1 < display.rows.len() {
            writer.writeln_newline("")?;
        }
    }
    writer.writeln_newline("")?;
    render_calendar_summary(writer, &display.summary)?;
    render_calendar_alerts(writer, display.alerts)?;
    Ok(())
}

fn round_band_segment_count(seconds: i64) -> usize {
    let non_negative_seconds = seconds.max(0);
    ((non_negative_seconds.saturating_add(BAND_SECONDS_PER_SEGMENT / 2)) / BAND_SECONDS_PER_SEGMENT)
        as usize
}

fn format_band_segment(symbol: char, count: usize, supports_ansi_color: bool) -> String {
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
    format!(
        "{}{}{}",
        color::Fg(color::AnsiValue(color_value)),
        symbol.to_string().repeat(count),
        color::Fg(color::Reset)
    )
}

fn format_band_legend(supports_ansi_color: bool) -> String {
    format!(
        "凡例: {} 固定  {} 経過済み  {} 繰返  {} 単発  {} 余差  {} 空き  {} 超過  (1文字=15分)",
        format_band_segment('#', 1, supports_ansi_color),
        format_band_segment('x', 1, supports_ansi_color),
        format_band_segment('=', 1, supports_ansi_color),
        format_band_segment('-', 1, supports_ansi_color),
        format_band_segment(':', 1, supports_ansi_color),
        format_band_segment('.', 1, supports_ansi_color),
        format_band_segment('>', 1, supports_ansi_color),
    )
}

pub(super) fn format_band_day_row(row: &BandDayRow, supports_ansi_color: bool) -> String {
    let durations = row.durations;
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
    let empty_seconds = BAND_SECONDS_PER_DAY.saturating_sub(used_seconds);
    let overflow_seconds = used_seconds.saturating_sub(BAND_SECONDS_PER_DAY);
    let mut bar = String::with_capacity(BAND_SEGMENTS);
    let mut cumulative_seconds = 0_i64;
    let mut previous_boundary = 0_usize;
    for (symbol, seconds) in categories.into_iter().chain([('.', empty_seconds)]) {
        cumulative_seconds = cumulative_seconds.saturating_add(seconds);
        let boundary = round_band_segment_count(cumulative_seconds.min(BAND_SECONDS_PER_DAY))
            .min(BAND_SEGMENTS);
        bar.push_str(&format_band_segment(
            symbol,
            boundary - previous_boundary,
            supports_ansi_color,
        ));
        previous_boundary = boundary;
    }
    let overflow = format_band_segment(
        '>',
        round_band_segment_count(overflow_seconds),
        supports_ansi_color,
    );
    format!(
        "{}({}) {} {} [{}]{}",
        row.date,
        weekday_jp(row.date.weekday()),
        format_signed_seconds(row.accumulated_rho_diff_seconds),
        format_signed_seconds(row.accumulated_free_diff_seconds),
        bar,
        overflow,
    )
}

pub(super) fn format_signed_seconds(seconds: i64) -> String {
    let sign = if seconds >= 0 { '+' } else { '-' };
    let absolute_minutes = seconds.unsigned_abs() / 60;
    format!(
        "{sign}{:02}:{:02}",
        absolute_minutes / 60,
        absolute_minutes % 60
    )
}

fn render_pack_display(
    writer: &mut dyn SchronuWriter,
    display: &PackDisplay,
) -> Result<(), std::io::Error> {
    let total_work_seconds = display.rows.iter().map(|row| row.work_seconds).sum::<i64>();
    for row in &display.rows {
        writer.writeln_newline(&format!(
            "詰\t{}\t{}\t{}\t優先度{}\t{}\t{}",
            row.source_date,
            row.target_date,
            format_work_seconds_as_hours_minutes(row.work_seconds),
            row.priority,
            row.task_id,
            row.name,
        ))?;
    }
    if display.rows.is_empty() && display.skipped_count == 0 {
        writer.writeln_newline("[Info] 詰められるタスクはありません。")?;
    } else {
        writer.writeln_newline(&format!(
            "詰: {}件 {} (スキップ{}件)",
            display.rows.len(),
            format_work_seconds_as_hours_minutes(total_work_seconds),
            display.skipped_count,
        ))?;
    }
    Ok(())
}

pub(super) fn format_work_seconds_as_hours_minutes(work_seconds: i64) -> String {
    let total_minutes = work_seconds.max(0) / 60;
    format!("{:02}:{:02}", total_minutes / 60, total_minutes % 60)
}

fn format_calendar_day_row(row: &CalendarDayRow) -> String {
    let adjustable_rate = if row.adjustable_work_seconds == 0 {
        "     ".to_string()
    } else {
        format!(
            "({:02.0}%)",
            row.adjustable_work_seconds as f64 / (row.free_time_minutes as f64 * 60.0) * 100.0
        )
    };
    format!(
        "{}({})\t{:4.1}時間\t{}{}\t{:5.2}\t{}\t{}\t{}\t{:5.2}\t{}\t{}\t{:5.2}\t{:02}[タスク]",
        row.date,
        weekday_jp(row.date.weekday()),
        row.free_time_minutes as f64 / 60.0,
        format_signed_duration(row.free_time_diff_minutes, false),
        adjustable_rate,
        row.rho_diff,
        format_rho_goal_diff(row.rho_goal_diff_hours),
        format_signed_duration(row.accumulated_rho_goal_diff_minutes, true),
        format_signed_duration(row.deadline_diff_seconds / 60, false),
        row.deadline_ratio,
        format_signed_duration(row.accumulated_free_diff_minutes, true),
        format_non_repetitive_duration(row.non_repetitive_free_minutes),
        row.accumulated_rho_diff,
        row.task_count,
    )
}

fn format_rho_goal_diff(hours: f64) -> String {
    let sign = if hours > 0.0 { ' ' } else { '-' };
    let absolute_hours = hours.abs();
    let whole_hours = absolute_hours.floor();
    let minutes = (absolute_hours - whole_hours) * 60.0;
    format!("{sign}{whole_hours:.0}時間{minutes:02.0}分")
}

fn format_non_repetitive_duration(minutes: i64) -> String {
    let sign = if minutes >= 0 { ' ' } else { '-' };
    let absolute_minutes = minutes.abs();
    format!(
        "{sign}{:02}時間{:02}分",
        absolute_minutes / 60,
        absolute_minutes % 60
    )
}

fn format_signed_duration(minutes: i64, pad_hours: bool) -> String {
    let sign = if minutes > 0 { ' ' } else { '-' };
    let absolute_minutes = minutes.abs();
    let hours = absolute_minutes / 60;
    let minutes = absolute_minutes % 60;
    if pad_hours {
        format!("{sign}{hours:02}時間{minutes:02}分")
    } else {
        format!("{sign}{hours}時間{minutes:02}分")
    }
}

fn render_calendar_summary(
    writer: &mut dyn SchronuWriter,
    summary: &CalendarSummary,
) -> Result<(), std::io::Error> {
    writer.writeln_newline(&format!(
        "今のタスクが片付く日付: {}日後の{}",
        (summary.first_caught_up_date - summary.last_synced_date).num_days(),
        summary.first_caught_up_date,
    ))?;
    let max_sign = if summary.max_accumulated_free_diff_minutes >= 0 {
        ' '
    } else {
        '-'
    };
    writer.writeln_newline(&format!(
        "最大の累積時間: {}{:02}時間{:02}分 ({}), 最大のrhoの差: {:.2} ({}), 次にタスクを積める日付: {}日後の{} (-{}時間{:02}分)",
        max_sign,
        summary.max_accumulated_free_diff_minutes.abs() / 60,
        summary.max_accumulated_free_diff_minutes.abs() % 60,
        summary.max_accumulated_free_diff_date,
        summary.max_accumulated_rho_diff,
        summary.max_accumulated_rho_diff_date,
        (summary.first_leeway_date - summary.last_synced_date).num_days(),
        summary.first_leeway_date,
        summary.first_leeway_minutes.abs() / 60,
        summary.first_leeway_minutes.abs() % 60,
    ))?;
    writer.writeln_newline("")?;
    Ok(())
}

fn render_calendar_alerts(
    writer: &mut dyn SchronuWriter,
    alerts: CalendarAlerts,
) -> Result<(), std::io::Error> {
    let mut is_all_favorable = true;
    if !alerts.has_today_deadline_leeway {
        writer.writeln_newline("[Crit] 【今日の】〆切に間に合いません。【ただちに】〆切をリスケする調整をしてください。")?;
        is_all_favorable = false;
    }
    if alerts.has_today_freetime_leeway {
        if !alerts.has_today_new_task_leeway {
            writer.writeln_newline("[Warn] 脇道に逸れずに予定の遂行をしてください。見積もりを間違えたり突発タスクが発生したりした場合に終了予定時刻に間に合わなくなる可能性があります。")?;
            is_all_favorable = false;
        }
    } else {
        writer.writeln_newline("[Crit] 【今日の】終了予定時刻に間に合いません。【ただちに】どれかの予定を諦めて明日以降に延期してください。")?;
        is_all_favorable = false;
    }
    if !alerts.has_tomorrow_deadline_leeway {
        writer.writeln_newline("[Warn] 【明日の】〆切に間に合いません。〆切をあさって以降にリスケする調整を【今日中に】してください。")?;
        is_all_favorable = false;
    }
    if !alerts.has_tomorrow_freetime_leeway {
        writer.writeln_newline("[Warn] 【明日の】終了予定時刻に間に合いません。【今日中に】どれかの予定を諦めてあさって以降に延期してください。")?;
        is_all_favorable = false;
    }
    if !alerts.has_weekly_deadline_leeway {
        writer.writeln_newline("[Warn] 【1週間以内の】〆切に間に合いません。【近々】どれかの予定を諦めて来週以降に延期してください。")?;
        is_all_favorable = false;
    }
    if !alerts.has_weekly_freetime_leeway {
        writer.writeln_newline("[Warn] 【1週間以内の】終了予定時刻に間に合いません。【近々】どれかの予定を諦めて来週以降に延期してください。")?;
        is_all_favorable = false;
    }
    if is_all_favorable {
        writer.writeln_newline("[Info] 順調です。突発タスクに対応したり1日の終わり際にタスクを新しく積んだりする余裕があります。ひとまずは脇道に逸れずに予定の遂行をしてください。")?;
    }
    writer.writeln_newline("")?;
    Ok(())
}

pub(super) fn format_task_list_row(row: &TaskListRow) -> String {
    match row {
        TaskListRow::Task(row) => format_task_list_task_row(row),
        TaskListRow::Gap { minutes } => format!(
            "---- ------------------------------------ - ---------- --------------------- - -- -- {minutes}分間の空き時間"
        ),
        TaskListRow::Message { text } => text.clone(),
    }
}

pub(super) fn format_task_list_task_row(row: &TaskListTaskRow) -> String {
    let columns = task_list_columns(row, TaskListIconMode::ApplyGiveUpCandidate);
    format_task_list_columns(&columns)
}

pub(super) fn task_list_columns(
    row: &TaskListTaskRow,
    icon_mode: TaskListIconMode,
) -> TaskListColumns {
    let icon = match icon_mode {
        TaskListIconMode::Original => row.icon.clone(),
        TaskListIconMode::ApplyGiveUpCandidate if row.give_up_candidate => "A".to_string(),
        TaskListIconMode::ApplyGiveUpCandidate => row.icon.clone(),
    };
    TaskListColumns {
        rank: format!("{:04}", row.rank),
        task_id: row.task_id.to_string(),
        icon,
        remaining_time: row.remaining_time.clone(),
        scheduled_time: format!(
            "{}({})-{}~{}",
            row.scheduled_start.format("%m/%d"),
            weekday_jp(row.scheduled_start.weekday()),
            row.scheduled_start.format("%H:%M"),
            row.scheduled_end.format("%H:%M"),
        ),
        priority: row.priority_rank.to_string(),
        estimated_minutes: format!("{:02}", row.estimated_minutes),
        project_number: format!("{:02}", row.project_number_priority),
        category: project_category_symbol(row.project_category).to_string(),
        task_name: row.task_name.clone(),
    }
}

pub(super) fn format_task_list_columns(columns: &TaskListColumns) -> String {
    format_spreadsheet_task_row(&SpreadsheetTaskRow {
        rank: &columns.rank,
        task_id: &columns.task_id,
        icon: &columns.icon,
        remaining_time: &columns.remaining_time,
        scheduled_time: &columns.scheduled_time,
        priority: &columns.priority,
        estimated_minutes: &columns.estimated_minutes,
        project_number: &columns.project_number,
        category: &columns.category,
        task_name: &columns.task_name,
    })
}

pub(super) fn weekday_jp(weekday: Weekday) -> &'static str {
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

pub(super) fn project_category_symbol(project_category: Option<ProjectCategory>) -> &'static str {
    match project_category {
        Some(ProjectCategory::Earning) => "獲",
        Some(ProjectCategory::Sustaining) => "維",
        Some(ProjectCategory::Recovery) => "回",
        Some(ProjectCategory::Investment) => "資",
        Some(ProjectCategory::Consumption) => "消",
        None => "_",
    }
}

pub(super) fn format_task_category_summary(
    category_work_seconds: &[TaskCategoryWorkSeconds],
    denominator_seconds: i64,
) -> String {
    if category_work_seconds
        .iter()
        .map(|entry| entry.seconds)
        .sum::<i64>()
        == 0
    {
        return "予定カテゴリ: 予定なし".to_string();
    }
    let mut cumulative_seconds = 0;
    let parts = category_work_seconds
        .iter()
        .map(|entry| {
            cumulative_seconds += entry.seconds;
            format!(
                "{} {:.1}時間({} | {})",
                project_category_label(entry.project_category),
                entry.seconds as f64 / 3600.0,
                format_category_percentage(entry.seconds, denominator_seconds),
                format_category_percentage(cumulative_seconds, denominator_seconds),
            )
        })
        .collect::<Vec<_>>();
    format!("予定カテゴリ: {}", parts.join(" / "))
}

fn project_category_label(category: Option<ProjectCategory>) -> &'static str {
    match category {
        Some(ProjectCategory::Earning) => "獲得",
        Some(ProjectCategory::Sustaining) => "維持",
        Some(ProjectCategory::Recovery) => "回復",
        Some(ProjectCategory::Investment) => "投資",
        Some(ProjectCategory::Consumption) => "消費",
        None => "未分類",
    }
}

fn format_category_percentage(seconds: i64, denominator_seconds: i64) -> String {
    if denominator_seconds > 0 {
        format!(
            "{:.0}%",
            seconds as f64 / denominator_seconds as f64 * 100.0
        )
    } else if seconds > 0 {
        "inf%".to_string()
    } else {
        "0%".to_string()
    }
}

fn render_tree_display(
    writer: &mut dyn SchronuWriter,
    tree: &TreeDisplay,
) -> Result<(), std::io::Error> {
    match tree {
        TreeDisplay::Debug { rows } => {
            writer.write_all(b"\n")?;
            for row in rows {
                writer.writeln_newline(&row.debug)?;
            }
            writer.write_all(b"\n")?;
        }
        TreeDisplay::Ancestors { rows } => {
            writer.write_all(b"\n")?;
            for row in rows {
                let header = if row.level == 0 {
                    String::new()
                } else {
                    format!("{}`-- ", " ".repeat(4 * (row.level - 1)))
                };
                writer.writeln_newline(&format!(
                    "{header}{} [{}] {}m {}",
                    row.task_id,
                    row.first_available_date.format("%Y/%m/%d"),
                    row.estimated_minutes,
                    row.name,
                ))?;
            }
            writer.writeln_newline("")?;
        }
        TreeDisplay::Leaves { rows } => {
            for row in rows {
                writer.writeln_newline(&format!(
                    "{}\t{}\t{}",
                    row.remaining_count, row.project_name, row.task_debug,
                ))?;
            }
            writer.writeln_newline("")?;
        }
    }
    Ok(())
}

pub(super) fn render_plain_display_model(
    writer: &mut dyn Write,
    model: &DisplayModel,
) -> Result<(), std::io::Error> {
    struct PlainWriter<'a> {
        inner: &'a mut dyn Write,
    }

    impl Write for PlainWriter<'_> {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.inner.write(buffer)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.inner.flush()
        }
    }

    impl SchronuWriter for PlainWriter<'_> {
        fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
            writeln!(self.inner, "{message}")
        }
    }

    render_display_model(&mut PlainWriter { inner: writer }, model)
}

pub(super) struct ErrorCapturingWriter<'a> {
    inner: &'a mut dyn SchronuWriter,
    first_error: Option<std::io::Error>,
}

impl<'a> ErrorCapturingWriter<'a> {
    pub(super) fn new(inner: &'a mut dyn SchronuWriter) -> Self {
        Self {
            inner,
            first_error: None,
        }
    }

    pub(super) fn take_error(&mut self) -> Option<std::io::Error> {
        self.first_error.take()
    }

    fn capture(&mut self, error: std::io::Error) {
        if self.first_error.is_none() {
            self.first_error = Some(error);
        }
    }
}

impl Write for ErrorCapturingWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        match self.inner.write(buffer) {
            Ok(written) => Ok(written),
            Err(error) => {
                self.capture(error);
                Ok(buffer.len())
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Err(error) = self.inner.flush() {
            self.capture(error);
        }
        Ok(())
    }
}

impl SchronuWriter for ErrorCapturingWriter<'_> {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        if let Err(error) = self.inner.writeln_newline(message) {
            self.capture(error);
        }
        Ok(())
    }

    fn supports_ansi_color(&self) -> bool {
        self.inner.supports_ansi_color()
    }
}

impl SchronuWriter for RawTerminal<Stdout> {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        writeln!(self, "{}{}", termion::cursor::Left(MAX_COL), message)
    }

    fn supports_ansi_color(&self) -> bool {
        true
    }
}

impl SchronuWriter for Stdout {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        writeln!(self, "{}", message)
    }

    fn supports_ansi_color(&self) -> bool {
        self.is_terminal()
    }
}

pub(super) fn writeln_newline(
    writer: &mut dyn SchronuWriter,
    message: &str,
) -> Result<(), std::io::Error> {
    writer.writeln_newline(message)
}
