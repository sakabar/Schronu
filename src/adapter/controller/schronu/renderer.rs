use super::*;

pub(super) fn render_focus_selection_mode(
    stdout: &mut dyn SchronuWriter,
    label: &str,
) -> Result<(), CommandError> {
    writeln_newline(stdout, &format!("フォーカス選択モード: {label}")).map_err(CommandError::Output)
}

pub(super) trait SchronuWriter: Write {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error>;

    fn supports_ansi_color(&self) -> bool {
        true
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct DisplayModel {
    fragments: Vec<DisplayFragment>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DisplayFragment {
    Bytes(Vec<u8>),
    Line(String),
}

pub(super) struct DisplayRecorder {
    model: DisplayModel,
    supports_ansi_color: bool,
}

impl DisplayRecorder {
    pub(super) fn new(supports_ansi_color: bool) -> Self {
        Self {
            model: DisplayModel::default(),
            supports_ansi_color,
        }
    }

    pub(super) fn finish(self) -> DisplayModel {
        self.model
    }
}

impl Write for DisplayRecorder {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.model
            .fragments
            .push(DisplayFragment::Bytes(buffer.to_vec()));
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl SchronuWriter for DisplayRecorder {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        self.model
            .fragments
            .push(DisplayFragment::Line(message.to_string()));
        Ok(())
    }

    fn supports_ansi_color(&self) -> bool {
        self.supports_ansi_color
    }
}

pub(super) fn render_display_model(
    stdout: &mut dyn SchronuWriter,
    model: &DisplayModel,
) -> Result<(), std::io::Error> {
    for fragment in &model.fragments {
        match fragment {
            DisplayFragment::Bytes(bytes) => stdout.write_all(bytes)?,
            DisplayFragment::Line(message) => stdout.writeln_newline(message)?,
        }
    }
    stdout.flush()
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

pub(super) fn writeln_newline(
    stdout: &mut dyn SchronuWriter,
    message: &str,
) -> Result<(), std::io::Error> {
    stdout.writeln_newline(message)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SpreadsheetTaskRow {
    pub(super) index: usize,
    pub(super) task_id: Uuid,
    pub(super) icon: String,
    pub(super) deadline: String,
    pub(super) date: String,
    pub(super) time_range: String,
    pub(super) rank: i64,
    pub(super) estimated_minutes: i64,
    pub(super) category: String,
    pub(super) task_name: String,
}

pub(super) fn format_spreadsheet_task_row(row: &SpreadsheetTaskRow) -> String {
    [
        format!("{:04}", row.index),
        row.task_id.to_string(),
        row.icon.clone(),
        row.deadline.clone(),
        row.date.clone(),
        row.time_range.clone(),
        row.rank.to_string(),
        row.estimated_minutes.to_string(),
        row.category.clone(),
        row.task_name.clone(),
    ]
    .join("\t")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CliSpreadsheetTaskRow {
    pub(super) index: usize,
    pub(super) task_id: Uuid,
    pub(super) icon: String,
    pub(super) remaining_time: String,
    pub(super) scheduled_time: String,
    pub(super) priority: usize,
    pub(super) estimated_minutes: i64,
    pub(super) project_number: i64,
    pub(super) category: String,
    pub(super) task_name: String,
}

pub(super) fn format_cli_spreadsheet_task_row(row: &CliSpreadsheetTaskRow) -> String {
    let tab_separated = format_spreadsheet_task_row(&SpreadsheetTaskRow {
        index: row.index,
        task_id: row.task_id,
        icon: row.icon.clone(),
        deadline: row.remaining_time.clone(),
        date: row.scheduled_time.clone(),
        time_range: row.priority.to_string(),
        rank: row.estimated_minutes,
        estimated_minutes: row.project_number,
        category: row.category.clone(),
        task_name: row.task_name.clone(),
    });
    let mut columns = tab_separated
        .split('\t')
        .map(str::to_string)
        .collect::<Vec<_>>();
    columns[6] = format!("{:02}", row.estimated_minutes);
    columns[7] = format!("{:02}", row.project_number);
    columns.join(" ")
}

pub(super) fn format_task_list_row(
    message_prefix: &str,
    task_name: &str,
    give_up_candidate: bool,
) -> String {
    let message_prefix = if give_up_candidate {
        // A means Abandon candidate.
        replace_task_list_icon(message_prefix, "A")
    } else {
        message_prefix.to_string()
    };
    format!("{message_prefix}{task_name}")
}

pub(super) fn replace_task_list_icon(message_prefix: &str, icon: &str) -> String {
    let mut parts = message_prefix.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 8 {
        return message_prefix.to_string();
    }

    parts[2] = icon;
    format!("{} ", parts.join(" "))
}

#[allow(clippy::type_complexity)]
pub(super) fn execute_show_all_tasks_with_config(
    stdout: &mut dyn SchronuWriter,
    focused_task_id_opt: &mut Option<Uuid>,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    pattern_opt: &Option<String>,
    display_order: TaskListDisplayOrder,
    config: &SchronuConfig,
) -> Result<(), ApplicationError> {
    let supports_ansi_color = stdout.supports_ansi_color();
    let scheduled_tasks = get_schedule(task_repository)?;
    let mut task_list_display_rows: Vec<TaskListDisplayRow> = vec![];
    let mut available_biggest_row_opt: Option<TaskListDisplayRow> = None;
    let mut available_biggest_task_estimate_work_seconds = 0;

    // ここからρ計算用
    let last_synced_time = task_repository.get_last_synced_time();

    let eod = (get_next_morning_datetime(last_synced_time) + Duration::days(0))
        .with_hour(0)
        .expect("invalid hour")
        .with_minute(0)
        .expect("invalid minute")
        + Duration::minutes(config.end_of_day_offset_minutes);
    // ここまでρ計算用

    let is_calendar_func = pattern_opt
        .as_ref()
        .is_some_and(|pattern| pattern == "暦" || pattern == "calendar" || pattern == "cal");

    let is_band_func = pattern_opt
        .as_ref()
        .is_some_and(|pattern| pattern == "帯" || pattern == "band");

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
    let yyyymmdd_reg = Regex::new(r"^(\d{4})/(\d{2})/(\d{2})$").unwrap();
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
        let subjective_naive_date =
            (get_next_morning_datetime(*scheduled_start) - Duration::days(1)).date_naive();

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

                if (get_next_morning_datetime(*scheduled_start)
                    - get_next_morning_datetime(task_repository.get_last_synced_time()))
                    > Duration::days(valid_days)
                {
                    break;
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
                let deadline_naive_date =
                    (get_next_morning_datetime(*deadline_time) - Duration::days(1)).date_naive();

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

                let skip_msg = format!(
                    "---- ------------------------------------ - ---------- --------------------- - -- -- {}分間の空き時間",
                    blank_duration.num_minutes()
                );

                if let Some(pattern) = pattern_opt {
                    if (pattern == "今"
                        && *scheduled_start
                            < get_next_morning_datetime(task_repository.get_last_synced_time()))
                        || (pattern == "明"
                            && *current_datetime_cursor_clone
                                >= get_next_morning_datetime(
                                    task_repository.get_last_synced_time(),
                                )
                            && *scheduled_start
                                < get_next_morning_datetime(task_repository.get_last_synced_time())
                                    + Duration::days(1))
                        || (pattern == "近"
                            && *scheduled_start
                                < get_next_morning_datetime(task_repository.get_last_synced_time())
                                    + Duration::days(1))
                    {
                        task_list_display_rows.push(TaskListDisplayRow::new_message(
                            *current_datetime_cursor_clone,
                            0,
                            tmp_id,
                            0,
                            skip_msg,
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
                && task_deadline_time_opt.unwrap() < get_next_morning_datetime(last_synced_time)
                && task_deadline_time_opt.unwrap() < end_datetime
            {
                &breaking_deadline_icon
            } else if task_deadline_time_opt.is_some()
                && task_deadline_time_opt.unwrap() < get_next_morning_datetime(last_synced_time)
            {
                &deadline_icon
            } else if rank == &0 && scheduled_start < &eod {
                &today_leaf_icon
            } else {
                // - : 特に無しだが、空白にすると列数が乱れるので目立たない記号を入れる
                "-"
            };

            let deadline_string = if let Some(deadline_time) = deadline_time_opt {
                if *deadline_time < get_next_morning_datetime(last_synced_time) {
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

            let message_prefix = format_cli_spreadsheet_task_row(&CliSpreadsheetTaskRow {
                index: ind,
                task_id: *id,
                icon: icon.to_string(),
                remaining_time: deadline_string.clone(),
                scheduled_time: format!(
                    "{}({})-{}~{}",
                    start_datetime.format("%m/%d"),
                    get_weekday_jp(&start_datetime.date_naive()),
                    start_datetime.format("%H:%M"),
                    end_datetime.format("%H:%M")
                ),
                priority: *rank,
                estimated_minutes: round_up_sec_as_minute(estimated_work_seconds),
                project_number: task_priority,
                category: project_category_symbol(task_project_category_opt).to_string(),
                task_name: String::new(),
            });
            let msg = format!("{}{}", message_prefix, shorten_name);
            let task_list_display_row = TaskListDisplayRow::new_task(
                *scheduled_start,
                subjective_naive_date,
                *rank,
                *id,
                task_priority,
                estimated_work_seconds,
                task_project_category_opt,
                message_prefix,
                shorten_name,
            );

            match pattern_opt {
                Some(pattern) => {
                    // FIXME 文字列マッチの絞り込み機能とその他の属性による絞り込みを機能を分ける
                    if pattern == "葉" {
                        if rank == &0
                            || task_deadline_time_opt.is_some()
                                && task_deadline_time_opt.unwrap()
                                    < get_next_morning_datetime(last_synced_time)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "枝" {
                        if rank > &0 {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "印" {
                        if msg.contains(&format!(" {} ", deadline_icon))
                            || msg.contains(&format!(" {} ", breaking_deadline_icon))
                            || msg.contains(&format!(" {} ", today_leaf_icon))
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "〆" {
                        if msg.contains(&format!(" {} ", deadline_icon))
                            || msg.contains(&format!(" {} ", breaking_deadline_icon))
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if is_daily_summary_func {
                        // カレンダー表示機能を使う時には、タスク一覧は表示しない。
                    } else if pattern == "今" {
                        if get_next_morning_datetime(*scheduled_start)
                            == get_next_morning_datetime(last_synced_time)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "明" {
                        if get_next_morning_datetime(*scheduled_start)
                            == get_next_morning_datetime(last_synced_time) + Duration::days(1)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "近" {
                        if get_next_morning_datetime(*scheduled_start)
                            == get_next_morning_datetime(last_synced_time)
                            || get_next_morning_datetime(*scheduled_start)
                                == get_next_morning_datetime(last_synced_time) + Duration::days(1)
                        {
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
                        let todays_morning_datetime =
                            get_next_morning_datetime(last_synced_time) - Duration::days(1);
                        let dn = todays_morning_datetime.date_naive();
                        let now_weekday_jp = get_weekday_jp(&dn);

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

                        if get_next_morning_datetime(last_synced_time) + Duration::days(days)
                            == get_next_morning_datetime(*scheduled_start)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "週" {
                        // 今日を含む直近1週間のタスクを表示する
                        if get_next_morning_datetime(*scheduled_start)
                            - get_next_morning_datetime(last_synced_time)
                            < Duration::days(7)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "末" {
                        // 週末までのタスクを表示する
                        let todays_morning_datetime =
                            get_next_morning_datetime(last_synced_time) - Duration::days(1);
                        let dn = todays_morning_datetime.date_naive();
                        let now_weekday_jp = get_weekday_jp(&dn);

                        let now_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == now_weekday_jp)
                            .unwrap();
                        let target_days_of_week_ind =
                            days_of_week.iter().position(|&x| x == "日").unwrap();

                        let days_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                        if get_next_morning_datetime(*scheduled_start)
                            - get_next_morning_datetime(last_synced_time)
                            <= Duration::days(days_diff as i64)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if pattern == "翌" {
                        // 翌週末までのタスクを表示する
                        let todays_morning_datetime =
                            get_next_morning_datetime(last_synced_time) - Duration::days(1);
                        let dn = todays_morning_datetime.date_naive();
                        let now_weekday_jp = get_weekday_jp(&dn);

                        let now_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == now_weekday_jp)
                            .unwrap();
                        let target_days_of_week_ind =
                            days_of_week.iter().position(|&x| x == "日").unwrap();

                        let days_diff =
                            ((7 + target_days_of_week_ind - now_days_of_week_ind) % 7) as i64;

                        let diff = get_next_morning_datetime(*scheduled_start)
                            - get_next_morning_datetime(last_synced_time);
                        if Duration::days(days_diff) < diff && diff <= Duration::days(days_diff + 7)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if yyyymmdd_reg.is_match(pattern) {
                        let caps = yyyymmdd_reg.captures(pattern).unwrap();
                        let yyyy: i32 = caps[1].parse().unwrap();
                        let mm: u32 = caps[2].parse().unwrap();
                        let dd: u32 = caps[3].parse().unwrap();

                        let yyyymmdd = Local.with_ymd_and_hms(yyyy, mm, dd, 0, 0, 0).unwrap();

                        if get_next_morning_datetime(*scheduled_start) - Duration::days(1)
                            == get_next_morning_datetime(yyyymmdd)
                        {
                            task_list_display_rows.push(task_list_display_row.clone());
                        }
                    } else if integer_reg.is_match(pattern) {
                        let caps = integer_reg.captures(pattern).unwrap();
                        let input_minute: i64 = caps[0].parse().unwrap();
                        let target_free_time_seconds = input_minute * 60;

                        if *scheduled_start > get_next_morning_datetime(last_synced_time)
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

    let naive_dt_today =
        (get_next_morning_datetime(last_synced_time) - Duration::days(1)).date_naive();
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
            );
        let full_day_free_time_minutes_opt = if is_band_func {
            Some(
                calculate_full_day_free_time_minutes_for_subjective_date_with_end_of_day_offset_minutes(
                    date,
                    free_time_manager,
                    config.end_of_day_offset_minutes,
                ),
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

    if !is_daily_summary_func {
        for row in task_list_display_rows.iter() {
            *focused_task_id_opt = Some(row.id);
            writeln_newline(stdout, &row.render_message()).unwrap();
        }

        writeln_newline(stdout, "").unwrap();
        let project_category_summary =
            summarize_scheduled_work_seconds_by_project_category(&task_list_display_rows);
        let project_category_denominator_seconds = calculate_project_category_denominator_seconds(
            &task_list_display_rows,
            last_synced_time,
            free_time_manager,
            config.end_of_day_offset_minutes,
        );
        writeln_newline(
            stdout,
            &format_scheduled_work_seconds_by_project_category(
                &project_category_summary,
                project_category_denominator_seconds,
            ),
        )
        .unwrap();
        writeln_newline(stdout, "").unwrap();
    }

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

    if is_calendar_func || is_band_func {
        writeln_newline(stdout, &busy_s).unwrap();
        writeln_newline(stdout, &s).unwrap();
        writeln_newline(stdout, &s_for_rho1).unwrap();
        writeln_newline(stdout, &s_for_non_repetitive_rho).unwrap();
    }

    writeln_newline(stdout, "").unwrap();
    Ok(())
}
