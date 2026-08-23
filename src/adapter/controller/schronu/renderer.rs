use chrono::{DateTime, Datelike, Local, NaiveDate, Weekday};
use schronu::entity::task::ProjectCategory;
use std::io::{IsTerminal, Stdout, Write};
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

#[derive(Clone, Debug, Eq, PartialEq)]
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
            Self::Tree(_) | Self::TaskList(_) => false,
            Self::Sequence(models) => models.iter().all(Self::is_empty),
        }
    }

    #[allow(dead_code)] // DisplayRecorder compatibility is retained during incremental migration.
    pub(super) fn fragments(&self) -> &[DisplayFragment] {
        match self {
            Self::Legacy { fragments } => fragments,
            Self::Message { .. } | Self::Tree(_) | Self::TaskList(_) | Self::Sequence(_) => {
                unreachable!("semantic display models do not expose legacy fragments")
            }
        }
    }

    fn legacy_fragments_mut(&mut self) -> &mut Vec<DisplayFragment> {
        match self {
            Self::Legacy { fragments } => fragments,
            Self::Message { .. } | Self::Tree(_) | Self::TaskList(_) | Self::Sequence(_) => {
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
                MessageLevel::Critical => "[Critical] ",
                MessageLevel::Error => "[Error] ",
            };
            writer.writeln_newline(&format!("{prefix}{text}"))?;
        }
        DisplayModel::Tree(tree) => render_tree_display(writer, tree)?,
        DisplayModel::TaskList(task_list) => render_task_list_display(writer, task_list)?,
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
    let rank = format!("{:04}", row.rank);
    let task_id = row.task_id.to_string();
    let icon = if row.give_up_candidate {
        "A"
    } else {
        &row.icon
    };
    let scheduled_time = format!(
        "{}({})-{}~{}",
        row.scheduled_start.format("%m/%d"),
        weekday_jp(row.scheduled_start.weekday()),
        row.scheduled_start.format("%H:%M"),
        row.scheduled_end.format("%H:%M"),
    );
    let priority = row.priority_rank.to_string();
    let estimated_minutes = format!("{:02}", row.estimated_minutes);
    let project_number = format!("{:02}", row.project_number_priority);
    format_spreadsheet_task_row(&SpreadsheetTaskRow {
        rank: &rank,
        task_id: &task_id,
        icon,
        remaining_time: &row.remaining_time,
        scheduled_time: &scheduled_time,
        priority: &priority,
        estimated_minutes: &estimated_minutes,
        project_number: &project_number,
        category: project_category_symbol(row.project_category),
        task_name: &row.task_name,
    })
}

fn weekday_jp(weekday: Weekday) -> &'static str {
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
