use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Timelike, Weekday};
use fs2::FileExt;
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};
use regex::Regex;
use schronu::adapter::gateway::free_time_manager::FreeTimeManager;
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::FreeTimeManagerTrait;
use schronu::application::interface::TaskRepositoryTrait;
use schronu::entity::datetime::get_next_morning_datetime;
use schronu::entity::task::{
    extract_leaf_tasks_from_project, extract_leaf_tasks_from_project_with_pending,
    round_up_sec_as_minute, Status, Task, TaskAttr,
};
use std::cmp::max;
use std::cmp::min;
use std::collections::HashMap;
use std::fs::File;
use std::io::Stdout;
use std::io::{stdout, Write};
use std::process;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::raw::RawTerminal;
use termion::style;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;
use url::Url;
use uuid::Uuid;
use webbrowser;

const MAX_COL: u16 = 999;

const IS_LOW_PRIORITY_MODE: bool = false;

// パーセントエンコーディングする対象にスペースを追加する
const MY_ASCII_SET: &AsciiSet = &CONTROLS.add(b' ');

fn writeln_newline(stdout: &mut RawTerminal<Stdout>, message: &str) -> Result<(), std::io::Error> {
    writeln!(stdout, "{}{}", termion::cursor::Left(MAX_COL), message)
}

fn backward_width(line: &str, cursor_x: usize) -> u16 {
    if line.chars().count() == 0 || cursor_x == 0 {
        return 0;
    }

    let ch_opt = line.chars().nth(cursor_x - 1);
    let width = match ch_opt {
        Some(ch) => UnicodeWidthChar::width(ch).unwrap_or(0),
        None => 0,
    } as u16;

    return width;
}

fn get_weekday_jp(date: &NaiveDate) -> &str {
    match date.weekday() {
        Weekday::Mon => "月",
        Weekday::Tue => "火",
        Weekday::Wed => "水",
        Weekday::Thu => "木",
        Weekday::Fri => "金",
        Weekday::Sat => "土",
        Weekday::Sun => "日",
    }
}

#[test]
fn test_backward_width_正常系1() {
    let s = String::from("あ");
    let cursor_x = 1;
    let actual = backward_width(&s, cursor_x);
    let expected = 2;
    assert_eq!(actual, expected);
}

#[test]
fn test_backward_width_異常系1() {
    let s = String::from("");
    let cursor_x = 10;
    let actual = backward_width(&s, cursor_x);
    let expected = 0;
    assert_eq!(actual, expected);
}

#[test]
fn test_backward_width_異常系2() {
    let s = String::from("テスト");
    let cursor_x = 0;
    let actual = backward_width(&s, cursor_x);
    let expected = 0;
    assert_eq!(actual, expected);
}

fn get_byte_offset_for_insert(line: &str, cursor_x: usize) -> usize {
    let char_indices_vec = line.char_indices().collect::<Vec<_>>();
    let byte_offset = if !line.is_empty() && cursor_x <= char_indices_vec.len() - 1 {
        char_indices_vec[cursor_x].0
    } else {
        line.len()
    };

    return byte_offset;
}

#[test]
fn test_get_byte_offset_for_insert_正常系1() {
    // "|"
    let line = String::from("");
    let cursor_x: usize = 0;
    let actual = get_byte_offset_for_insert(&line, cursor_x);
    let expected = 0;
    assert_eq!(actual, expected);
}

#[test]
fn test_get_byte_offset_for_insert_正常系2() {
    // |例1の文字列
    let line = String::from("例1の文字列");
    let cursor_x: usize = 0;
    let actual = get_byte_offset_for_insert(&line, cursor_x);
    let expected = 0;
    assert_eq!(actual, expected);
}

#[test]
fn test_get_byte_offset_for_insert_正常系3() {
    // 例1の|文字列
    let line = String::from("例1の文字列");
    let cursor_x: usize = 3;
    let actual = get_byte_offset_for_insert(&line, cursor_x);
    let expected = String::from("例1の").len(); // 3+1+3=7
    assert_eq!(actual, expected);
}

#[test]
fn test_get_byte_offset_for_insert_正常系4() {
    // あ|
    let line = String::from("あ");
    let cursor_x: usize = 1;
    let actual = get_byte_offset_for_insert(&line, cursor_x);
    let expected = String::from("あ").len(); // 3
    assert_eq!(actual, expected);
}

fn get_width_for_rerender(header: &str, line: &str, cursor_x: usize) -> u16 {
    let mut width = UnicodeWidthStr::width(header);

    for ch in line.chars().take(cursor_x) {
        width += UnicodeWidthChar::width(ch).unwrap_or(0);
    }

    return width as u16;
}

#[test]
fn test_get_width_for_rerender_正常系_アスキー() {
    let header = String::from("schronu>");
    let line = String::from("project new");
    let cursor_x = 3;

    let actual = get_width_for_rerender(&header, &line, cursor_x);
    let expected = 11; // "schronu>pro"
    assert_eq!(actual, expected);
}

#[test]
fn test_get_width_for_rerender_正常系_多バイト1() {
    let header = String::from("schronu>");
    let line = String::from("breakdown タク1"); // 「ス」を入れたい
    let cursor_x = 11;

    let actual = get_width_for_rerender(&header, &line, cursor_x);
    let expected = 20; // "schronu>breakdown タ"
    assert_eq!(actual, expected);
}

#[test]
fn test_get_width_for_rerender_正常系_多バイト2() {
    let header = String::from("schronu>");
    let line = String::from("あい");
    let cursor_x = 2;

    let actual = get_width_for_rerender(&header, &line, cursor_x);
    let expected = 12; // "schronu>あい"
    assert_eq!(actual, expected);
}

fn get_forward_width(line: &str, cursor_x: usize) -> u16 {
    if !line.is_empty() && cursor_x <= line.chars().count() - 1 {
        let ch_opt = line.chars().nth(cursor_x);
        let n = match ch_opt {
            Some(ch) => UnicodeWidthChar::width(ch).unwrap_or(0),
            None => 0,
        } as u16;

        return n;
    }

    return 0;
}

#[test]
fn test_get_forward_width_正常系1() {
    let line = String::from("あ");
    let cursor_x = 0;

    let actual = get_forward_width(&line, cursor_x);
    let expected = 2;
    assert_eq!(actual, expected);
}

fn execute_show_tree(stdout: &mut RawTerminal<Stdout>, focused_task_opt: &Option<Task>) {
    writeln!(stdout, "").unwrap();
    focused_task_opt.as_ref().map(|focused_task| {
        let s: String = focused_task.tree_debug_pretty_print();
        let lines: Vec<_> = s.split('\n').collect();
        for line in lines.iter() {
            // Done([+])のタスクは表示しない
            // 恒久的には、tree_debug_pretty_print()に似た関数を自分で実装してカスタマイズする
            if line.contains("[ ]") || line.contains("[-]") {
                writeln_newline(stdout, line).unwrap()
            }
        }
    });
    writeln!(stdout, "").unwrap();
}

fn execute_start_new_project(
    _stdout: &mut RawTerminal<Stdout>,
    focused_task_id_opt: &mut Option<Uuid>,
    task_repository: &mut dyn TaskRepositoryTrait,
    new_project_name_str: &str,
    defer_days_opt: Option<i64>,
    estimated_work_minutes_opt: Option<i64>,
) {
    let root_task = Task::new(new_project_name_str);

    // 本来的には、TaskAttrのデフォルト値の方を5にすべきかも
    root_task.set_priority(5);

    defer_days_opt.map(|defer_days| {
        // 次回の午前6時
        let pending_until = get_next_morning_datetime(task_repository.get_last_synced_time())
            + Duration::days(defer_days - 1);
        root_task.set_pending_until(pending_until);
        root_task.set_orig_status(Status::Pending);
    });

    match estimated_work_minutes_opt {
        Some(estimated_work_minutes) => {
            root_task.set_estimated_work_seconds(estimated_work_minutes * 60);
        }
        None => {}
    }

    // フォーカスを移す
    *focused_task_id_opt = Some(root_task.get_id());

    task_repository.start_new_project(root_task);
}

fn execute_make_appointment(focused_task_opt: &Option<Task>, start_time: DateTime<Local>) {
    if let Some(task) = focused_task_opt {
        task.make_appointment(start_time);
    }
}

fn execute_show_ancestor(stdout: &mut RawTerminal<Stdout>, focused_task_opt: &Option<Task>) {
    writeln!(stdout, "").unwrap();

    // まずは葉タスクから根に向かいながら後ろに追加していき、
    // 最後に逆順にして表示する
    let mut ancestors: Vec<(DateTime<Local>, Task)> = vec![];

    if let Some(task) = focused_task_opt {
        ancestors = task.list_all_parent_tasks_with_first_available_time();
    }

    ancestors.reverse();

    for (level, (first_available_datetime, task)) in ancestors.iter().enumerate() {
        let header = if level == 0 {
            String::from("")
        } else {
            let indent = ' '.to_string().repeat(4 * (level - 1));
            format!("{}`-- ", &indent)
        };

        let id = task.get_id();
        let name = task.get_name();
        let estimated_work_minutes =
            (task.get_estimated_work_seconds() as f64 / 60.0).ceil() as i64;
        let first_available_date_str = first_available_datetime.format("%Y/%m/%d").to_string();

        let msg = format!(
            "{}{} [{}] {}m {}",
            &header, &id, &first_available_date_str, &estimated_work_minutes, &name
        );
        writeln_newline(stdout, &msg).unwrap();
    }

    writeln_newline(stdout, "").unwrap();
}

fn execute_show_leaf_tasks(
    stdout: &mut RawTerminal<Stdout>,
    task_repository: &mut dyn TaskRepositoryTrait,
    _free_time_manager: &mut dyn FreeTimeManagerTrait,
) {
    let mut ans_tpls = vec![];

    for project_root_task in task_repository.get_all_projects().iter() {
        let project_name = project_root_task.get_name();

        // 優先度が高いタスクほど下に表示されるようにし、フォーカスが当たるタスクは末尾に表示されるようにする。
        let leaf_tasks = extract_leaf_tasks_from_project(&project_root_task);
        for leaf_task in leaf_tasks.iter() {
            let deadline_time_opt = leaf_task.get_deadline_time_opt();
            let neg_priority = -leaf_task.get_priority();
            let id = leaf_task.get_id();
            let message = format!("{}\t{:?}", project_name, leaf_task.get_attr());

            let tpl = (
                deadline_time_opt.is_none(),
                neg_priority,
                deadline_time_opt,
                id,
                message,
            );
            ans_tpls.push(tpl);
        }
    }

    ans_tpls.sort();
    ans_tpls.reverse();

    for (ind, ans_tpl) in ans_tpls.iter().enumerate() {
        let task_cnt = ans_tpls.len() - ind;
        let message = format!("{}\t{}", task_cnt, ans_tpl.4);
        writeln_newline(stdout, &message).unwrap();
    }
    writeln_newline(stdout, "").unwrap();
}

fn execute_show_all_tasks(
    stdout: &mut RawTerminal<Stdout>,
    focused_task_id_opt: &mut Option<Uuid>,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    pattern_opt: &Option<String>,
) {
    // Hash化できる要素しか入れられないので、いったんidだけ入れる
    // pending_until: DateTime<Local>,
    // rank: usize,
    // deadline_time_opt: Option<DateTime<Local>>,
    let mut id_to_dt_map: HashMap<Uuid, (DateTime<Local>, i64, usize, Option<DateTime<Local>>)> =
        HashMap::new();

    // 複数の子タスクがある場合に、親タスクのdtは子の着手可能時期の中で最大の値となるようにする。
    // タプルの第2要素はrankで、葉(0)からの距離の大きい方
    let last_synced_time = task_repository.get_last_synced_time();
    for project_root_task in task_repository.get_all_projects().iter() {
        let leaf_tasks = extract_leaf_tasks_from_project_with_pending(&project_root_task);

        for leaf_task in leaf_tasks.iter() {
            let all_parent_tasks = leaf_task.list_all_parent_tasks_with_first_available_time();
            for (rank, (dt_raw, task)) in all_parent_tasks.iter().enumerate() {
                let id = task.get_id();
                let neg_priority = -task.get_priority();

                // 今日以前に実施可能だったタスクについては、今日のタスクと見なす
                let dt = max(dt_raw, &last_synced_time);

                // 親タスクのdtキーは別の葉ノードがあるかどうかで後で変化しうるので、
                // counterやtotal_estimated_work_seconds_of_the_date_counterの更新は
                // id_to_dt_mapが確定してからにする
                id_to_dt_map
                    .entry(id)
                    .and_modify(|(dt_val, _neg_priority, rank_val, _deadline_time_opt)| {
                        if dt > dt_val {
                            *dt_val = *dt
                        }

                        if rank > *rank_val {
                            *rank_val = rank
                        }
                    })
                    .or_insert((*dt, neg_priority, rank, task.get_deadline_time_opt()));
            }
        }
    }

    let mut dt_id_tpl_arr: Vec<(
        NaiveDate,
        bool,
        DateTime<Local>,
        i64,
        usize,
        Option<DateTime<Local>>,
        Uuid,
    )> = vec![];
    for (id, (dt, neg_priority, rank, deadline_time_opt)) in &id_to_dt_map {
        let naive_date = (get_next_morning_datetime(*dt) - Duration::days(1)).date_naive();
        let tpl = (
            naive_date,
            deadline_time_opt.is_none(),
            *dt,
            *neg_priority,
            *rank,
            *deadline_time_opt,
            *id,
        );
        dt_id_tpl_arr.push(tpl);
    }

    // dt,rank等、タプルの各要素の小さい順にソート。後で逆順に変える
    dt_id_tpl_arr.sort();

    let mut msgs_with_dt: Vec<(DateTime<Local>, usize, Uuid, String)> = vec![];
    let mut available_biggest_msgs_with_dt_opt = None;
    let mut available_biggest_task_estimate_work_seconds = 0;

    // ここからρ計算用
    let last_synced_time = task_repository.get_last_synced_time();

    // FIXME 外部設定ファイルで設定できるようにする
    let eod_duration = Duration::hours(0) + Duration::minutes(30);
    let eod = (get_next_morning_datetime(last_synced_time) + Duration::days(0))
        .with_hour(0)
        .expect("invalid hour")
        .with_minute(0)
        .expect("invalid minute")
        + eod_duration;
    // ここまでρ計算用

    let is_calendar_func = pattern_opt.as_ref().map_or(false, |pattern| {
        pattern == "暦" || pattern == "calendar" || pattern == "cal"
    });

    let is_flatten_func = pattern_opt.as_ref().map_or(false, |pattern| {
        pattern == "平" || pattern == "flatten" || pattern == "flat"
    });

    // 日付ごとのタスク数を集計する
    let mut counter: HashMap<NaiveDate, usize> = HashMap::new();
    let mut total_estimated_work_seconds_of_the_date_counter: HashMap<NaiveDate, i64> =
        HashMap::new();
    let mut total_estimated_work_seconds: i64 = 0;
    let mut deadline_estimated_work_seconds_map: HashMap<NaiveDate, i64> = HashMap::new();

    let mut repetitive_task_estimated_work_seconds_map: HashMap<NaiveDate, i64> = HashMap::new();

    // 日ごとの、前倒し可能なタスクの見積もりの和
    // 前倒し可能という決め方だと、何日まで前倒しできるのか曖昧性が発生する?
    let mut adjustable_estimated_work_seconds_map: HashMap<NaiveDate, i64> = HashMap::new();

    // 「暦」コマンドで、未来のサマリは見ても仕方ないので、直近の28日ぶん(配列の末尾)に絞る
    const SUMMARY_DAYS: usize = 28;

    // タスク一覧で、どのタスクをいつやる見込みかを表示するために、「現在時刻」をズラして見ていく
    let mut current_datetime_cursor = task_repository.get_last_synced_time();

    for (ind, (_naive_date, _has_no_deadline, dt, _neg_priority, rank, deadline_time_opt, id)) in
        dt_id_tpl_arr.iter().enumerate()
    {
        let subjective_naive_date =
            (get_next_morning_datetime(*dt) - Duration::days(1)).date_naive();

        // 「今」「明」コマンドの場合は未来の情報には興味がないので、スキップする
        if let Some(pattern) = pattern_opt {
            if pattern == "今" || pattern == "明" || pattern == "暦" {
                let valid_days = if pattern == "今" {
                    0
                } else if pattern == "明" {
                    1
                } else if pattern == "暦" {
                    SUMMARY_DAYS as i64
                } else {
                    // 事前にif文で囲ってあるので、通常はこのケースに入ることはない
                    9999
                };

                if (get_next_morning_datetime(*dt)
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

        let task_opt = task_repository.get_by_id(*id);
        match task_opt {
            Some(task) => {
                let repetition_prefix_label = if let Some(parent) = task.parent() {
                    let mut ans = "".to_string();

                    if let Some(repetition_interval_days) =
                        parent.get_repetition_interval_days_opt()
                    {
                        ans = format!("{}【繰】({})", ans, repetition_interval_days);
                    }

                    if task.get_is_on_other_side() {
                        ans = format!("{}【待ち】", ans);
                    }

                    ans
                } else {
                    "".to_string()
                };

                // 前倒し可能なタスクの見積もり時間をカウントする
                let mut adjustable_prefix_label = "".to_string();

                if rank == &0
                    && !task.get_is_on_other_side()
                    && task.get_start_time() < get_next_morning_datetime(*dt) - Duration::days(1)
                {
                    adjustable_estimated_work_seconds_map
                        .entry(subjective_naive_date)
                        .and_modify(|estimated_work_seconds_val| {
                            *estimated_work_seconds_val += task.get_estimated_work_seconds()
                        })
                        .or_insert(task.get_estimated_work_seconds());

                    adjustable_prefix_label = "【前】".to_string();
                }

                let name = format!(
                    "{}{}{}",
                    adjustable_prefix_label,
                    repetition_prefix_label,
                    task.get_name()
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

                let shorten_name: String = if let Some(latest_index) = latest_index_opt {
                    format!(
                        "{}...",
                        chars_vec.iter().take(latest_index + 1).collect::<String>()
                    )
                } else {
                    name.to_string()
                };

                // 元々見積もり時間から作業済時間を引いたのが残りの見積もり時間
                // ただし、作業時間が元々の見積もり時間をオーバーしている時には既に想定外の事態になっているため、
                // 残りの見積もりを0とはせず、安全に倒して元々の見積もりの2倍として扱う
                let estimated_work_seconds =
                    if task.get_estimated_work_seconds() >= task.get_actual_work_seconds() {
                        task.get_estimated_work_seconds() - task.get_actual_work_seconds()
                    } else {
                        max(
                            0,
                            task.get_estimated_work_seconds() * 2 - task.get_actual_work_seconds(),
                        )
                    };
                total_estimated_work_seconds += estimated_work_seconds;

                if let Some(deadline_time) = deadline_time_opt {
                    let deadline_naive_date = (get_next_morning_datetime(*deadline_time)
                        - Duration::days(1))
                    .date_naive();

                    deadline_estimated_work_seconds_map
                        .entry(deadline_naive_date)
                        .and_modify(|deadline_estimated_work_seconds| {
                            *deadline_estimated_work_seconds += estimated_work_seconds
                        })
                        .or_insert(estimated_work_seconds);
                }

                if let Some(parent) = task.parent() {
                    if let Some(_repetition_interval_days) =
                        parent.get_repetition_interval_days_opt()
                    {
                        repetitive_task_estimated_work_seconds_map
                            .entry(subjective_naive_date)
                            .and_modify(|repetitive_task_estimated_work_seconds| {
                                *repetitive_task_estimated_work_seconds += estimated_work_seconds
                            })
                            .or_insert(estimated_work_seconds);
                    }
                }

                // 着手時間は、現在時刻か、最速着手可能時間のうち遅い方
                let current_datetime_cursor_clone = &current_datetime_cursor.clone();
                let start_datetime = max(dt, current_datetime_cursor_clone);

                // 「今」か「明」の時のみ、日時カーソルが飛んだ場合には、その間の時間を表示する
                if (*dt - current_datetime_cursor_clone).num_minutes() > 0 {
                    let blank_duration = *dt - current_datetime_cursor_clone;
                    let tmp_id = Uuid::new_v4();

                    let skip_msg = format!(
                        "---- ------------------------------------ - ---------- --------------------- - -- -- {}分間の空き時間",
                        blank_duration.num_minutes()
                    );

                    if let Some(pattern) = pattern_opt {
                        if (pattern == "今"
                            && *dt
                                < get_next_morning_datetime(task_repository.get_last_synced_time()))
                            || (pattern == "明"
                                && *current_datetime_cursor_clone
                                    >= get_next_morning_datetime(
                                        task_repository.get_last_synced_time(),
                                    )
                                && *dt
                                    < get_next_morning_datetime(
                                        task_repository.get_last_synced_time(),
                                    ) + Duration::days(1))
                        {
                            msgs_with_dt.push((
                                *current_datetime_cursor_clone,
                                0,
                                tmp_id,
                                skip_msg,
                            ));
                        }
                    }
                }

                let end_datetime = *start_datetime + Duration::seconds(estimated_work_seconds);
                current_datetime_cursor = end_datetime.clone();

                total_estimated_work_seconds_of_the_date_counter
                    .entry(subjective_naive_date)
                    .and_modify(|estimated_work_seconds_val| {
                        *estimated_work_seconds_val += estimated_work_seconds
                    })
                    .or_insert(estimated_work_seconds);

                let total_estimated_work_hours =
                    (total_estimated_work_seconds as f64 / 3600.0).ceil() as i64;

                // ! : 今日中が締切。締切注意の意
                let deadline_icon: String = "!".to_string();

                // v : もっと着手を手前(下)にせよの意
                let breaking_deadline_icon: String = "v".to_string();

                // / : 今日着手する予定の葉タスク。/という記号自体に強い意味合いはない。
                let today_leaf_icon: String = "/".to_string();

                let icon = if task.get_deadline_time_opt().is_some()
                    && task.get_deadline_time_opt().unwrap()
                        < get_next_morning_datetime(last_synced_time)
                    && task.get_deadline_time_opt().unwrap() < end_datetime
                {
                    &breaking_deadline_icon
                } else if task.get_deadline_time_opt().is_some()
                    && task.get_deadline_time_opt().unwrap()
                        < get_next_morning_datetime(last_synced_time)
                {
                    &deadline_icon
                } else if rank == &0 && dt < &eod {
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
                        } else {
                            if *deadline_time < end_datetime {
                                format!("+{:02}:{:02}____", breaking_hh, breaking_mm)
                            } else {
                                format!("____-{:02}:{:02}", breaking_hh, breaking_mm)
                            }
                        }
                    } else {
                        deadline_time.format("%Y/%m/%d").to_string()
                    }
                } else {
                    "____/__/__".to_string()
                };

                let msg: String = format!(
                    "{:04} {} {} {} {} {} {:02.0} {:02.0} {}",
                    ind,
                    id,
                    icon,
                    deadline_string,
                    format!(
                        "{}({})-{}~{}",
                        start_datetime.format("%m/%d"),
                        get_weekday_jp(&start_datetime.date_naive()),
                        start_datetime.format("%H:%M"),
                        end_datetime.format("%H:%M")
                    ),
                    rank,
                    round_up_sec_as_minute(estimated_work_seconds),
                    total_estimated_work_hours,
                    shorten_name
                );

                let yyyymmdd_reg = Regex::new(r"^(\d{4})/(\d{2})/(\d{2})$").unwrap();
                let days_of_week = vec!["月", "火", "水", "木", "金", "土", "日"];

                // 指定した時間(分)以下で達成完了な最大のタスクを抽出する
                let integer_reg = Regex::new(r"^\d+$").unwrap();

                match pattern_opt {
                    Some(pattern) => {
                        // Todo: 文字列マッチの絞り込み機能とその他の属性による絞り込みを機能を分ける
                        if pattern == "葉" {
                            if rank == &0
                                || task.get_deadline_time_opt().is_some()
                                    && task.get_deadline_time_opt().unwrap()
                                        < get_next_morning_datetime(last_synced_time)
                            {
                                msgs_with_dt.push((*dt, *rank, *id, msg));
                            }
                        } else if pattern == "枝" {
                            if rank > &0 {
                                msgs_with_dt.push((*dt, *rank, *id, msg));
                            }
                        } else if pattern == "印" {
                            if msg.contains(&format!(" {} ", &deadline_icon))
                                || msg.contains(&format!(" {} ", &breaking_deadline_icon))
                                || msg.contains(&format!(" {} ", &today_leaf_icon))
                            {
                                msgs_with_dt.push((*dt, *rank, *id, msg));
                            }
                        } else if pattern == "〆" {
                            if msg.contains(&format!(" {} ", &deadline_icon))
                                || msg.contains(&format!(" {} ", &breaking_deadline_icon))
                            {
                                msgs_with_dt.push((*dt, *rank, *id, msg));
                            }
                        } else if is_calendar_func || is_flatten_func {
                            // カレンダー表示機能を使う時には、タスク一覧は表示しない。
                        } else if pattern == "今" {
                            if get_next_morning_datetime(*dt)
                                == get_next_morning_datetime(last_synced_time)
                            {
                                msgs_with_dt.push((*dt, *rank, *id, msg));
                            }
                        } else if pattern == "明" {
                            if get_next_morning_datetime(*dt)
                                == get_next_morning_datetime(last_synced_time) + Duration::days(1)
                            {
                                msgs_with_dt.push((*dt, *rank, *id, msg));
                            }
                        } else if days_of_week.contains(&pattern.as_str()) {
                            // 月 火 水 木 金 土 日 が指定された時は、明日以降で、直近のその曜日のタスクを表示する
                            let todays_morning_datetime =
                                get_next_morning_datetime(last_synced_time) - Duration::days(1);
                            let dn = todays_morning_datetime.date_naive();
                            let now_weekday_jp = get_weekday_jp(&dn);

                            let now_days_of_week_ind = days_of_week
                                .iter()
                                .position(|&x| &x == &now_weekday_jp)
                                .unwrap();
                            let target_days_of_week_ind =
                                days_of_week.iter().position(|&x| x == pattern).unwrap();

                            let ind_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                            // 今日のデータについては「全 今」で表示できるので、その代わりに、1週間後の同じ曜日の情報を表示するようにする
                            let days: i64 = if ind_diff == 0 { 7 } else { ind_diff as i64 };

                            if get_next_morning_datetime(last_synced_time) + Duration::days(days)
                                == get_next_morning_datetime(*dt)
                            {
                                msgs_with_dt.push((*dt, *rank, *id, msg));
                            }
                        } else if pattern == "週" {
                            // 今日を含む直近1週間のタスクを表示する
                            if get_next_morning_datetime(*dt)
                                - get_next_morning_datetime(last_synced_time)
                                < Duration::days(7)
                            {
                                msgs_with_dt.push((*dt, *rank, *id, msg));
                            }
                        } else if pattern == "末" {
                            // 週末までのタスクを表示する
                            let todays_morning_datetime =
                                get_next_morning_datetime(last_synced_time) - Duration::days(1);
                            let dn = todays_morning_datetime.date_naive();
                            let now_weekday_jp = get_weekday_jp(&dn);

                            let now_days_of_week_ind = days_of_week
                                .iter()
                                .position(|&x| &x == &now_weekday_jp)
                                .unwrap();
                            let target_days_of_week_ind =
                                days_of_week.iter().position(|&x| x == "日").unwrap();

                            let days_diff =
                                (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                            if get_next_morning_datetime(*dt)
                                - get_next_morning_datetime(last_synced_time)
                                <= Duration::days(days_diff as i64)
                            {
                                msgs_with_dt.push((*dt, *rank, *id, msg));
                            }
                        } else if pattern == "翌" {
                            // 翌週末までのタスクを表示する
                            let todays_morning_datetime =
                                get_next_morning_datetime(last_synced_time) - Duration::days(1);
                            let dn = todays_morning_datetime.date_naive();
                            let now_weekday_jp = get_weekday_jp(&dn);

                            let now_days_of_week_ind = days_of_week
                                .iter()
                                .position(|&x| &x == &now_weekday_jp)
                                .unwrap();
                            let target_days_of_week_ind =
                                days_of_week.iter().position(|&x| x == "日").unwrap();

                            let days_diff =
                                ((7 + target_days_of_week_ind - now_days_of_week_ind) % 7) as i64;

                            let diff = get_next_morning_datetime(*dt)
                                - get_next_morning_datetime(last_synced_time);
                            if Duration::days(days_diff) < diff
                                && diff <= Duration::days(days_diff + 7)
                            {
                                msgs_with_dt.push((*dt, *rank, *id, msg));
                            }
                        } else if yyyymmdd_reg.is_match(pattern) {
                            let caps = yyyymmdd_reg.captures(pattern).unwrap();
                            let yyyy: i32 = caps[1].parse().unwrap();
                            let mm: u32 = caps[2].parse().unwrap();
                            let dd: u32 = caps[3].parse().unwrap();

                            let yyyymmdd = Local.with_ymd_and_hms(yyyy, mm, dd, 0, 0, 0).unwrap();

                            if get_next_morning_datetime(*dt) - Duration::days(1)
                                == get_next_morning_datetime(yyyymmdd)
                            {
                                msgs_with_dt.push((*dt, *rank, *id, msg));
                            }
                        } else if integer_reg.is_match(pattern) {
                            let caps = integer_reg.captures(pattern).unwrap();
                            let input_minute: i64 = caps[0].parse().unwrap();
                            let target_free_time_seconds = input_minute * 60;

                            if *start_datetime > get_next_morning_datetime(last_synced_time)
                                || last_synced_time < task.get_start_time()
                            {
                                continue;
                            }

                            // 【待ち】がマジックナンバーなのがちょっとよくない
                            if *rank == 0
                                && !msg.contains("【待ち】")
                                && estimated_work_seconds < target_free_time_seconds
                                && estimated_work_seconds
                                    > available_biggest_task_estimate_work_seconds
                            {
                                available_biggest_task_estimate_work_seconds =
                                    estimated_work_seconds;

                                available_biggest_msgs_with_dt_opt = Some((*dt, *rank, *id, msg));
                            }
                        } else if name.to_lowercase().contains(&pattern.to_lowercase())
                            || msg.contains(pattern)
                        {
                            msgs_with_dt.push((*dt, *rank, *id, msg));
                        }
                    }
                    None => {
                        msgs_with_dt.push((*dt, *rank, *id, msg));
                    }
                }
            }
            None => {}
        }
    }

    // 着手可能な最大のタスクを実施するモード
    if let Some((dt, rank, id, msg)) = available_biggest_msgs_with_dt_opt {
        msgs_with_dt.push((dt, rank, id, msg));
    }

    // 逆順にする: dtの大きい順となる
    msgs_with_dt.reverse();

    if !is_calendar_func && !is_flatten_func {
        for (_, _, id, msg) in msgs_with_dt.iter() {
            *focused_task_id_opt = Some(*id);
            writeln_newline(stdout, &msg).unwrap();
        }

        writeln_newline(stdout, "").unwrap();
    }

    // 日付の小さい順にソートする
    let mut counter_arr: Vec<(&NaiveDate, &usize)> = counter.iter().collect();
    counter_arr.sort_by(|a, b| a.0.cmp(&b.0));

    let mut daily_stat_msgs: Vec<String> = vec![];

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

    // 平坦化可能ポイント
    let mut flattenable_date_opt: Option<NaiveDate> = None;
    let mut overload_day_is_found = false;
    let mut flattenable_duration = Duration::seconds(0);

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

        let weekday_jp = get_weekday_jp(&date);

        let local_datetime_base = get_next_morning_datetime(
            Local::now()
                .timezone()
                .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
                .unwrap(),
        );

        let free_time_minutes = if local_datetime_base < last_synced_time
            && last_synced_time < get_next_morning_datetime(local_datetime_base)
        {
            if last_synced_time.hour() < get_next_morning_datetime(last_synced_time).hour() {
                if last_synced_time < eod {
                    (eod - last_synced_time).num_minutes()
                } else {
                    0
                }
            } else {
                free_time_manager.get_free_minutes(&last_synced_time, &eod)
            }
        } else {
            // 明日以降
            let local_tz = Local::now().timezone();

            let start = get_next_morning_datetime(
                local_tz
                    .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
                    .unwrap(),
            );

            let end = local_tz
                .from_local_datetime(&date.and_hms_opt(23, 59, 59).unwrap())
                .unwrap()
                + eod_duration;
            free_time_manager.get_free_minutes(&start, &end)
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

        const RHO_GOAL: f64 = 0.7;

        let diff_to_goal = if free_time_hours - total_repetitive_task_work_hours_of_the_date > 0.0 {
            (total_estimated_work_hours_of_the_date - total_repetitive_task_work_hours_of_the_date)
                - (free_time_hours - total_repetitive_task_work_hours_of_the_date) * RHO_GOAL
        } else {
            0.0
        };
        let diff_to_goal_sign: char = if diff_to_goal > 0.0 { ' ' } else { '-' };
        let diff_to_goal_hour = diff_to_goal.abs().floor();
        let diff_to_goal_minute = (diff_to_goal.abs() - diff_to_goal_hour) * 60.0;

        let over_time_hours_f = total_estimated_work_hours_of_the_date - free_time_hours;
        let over_time_hours = over_time_hours_f.abs().floor() as i64;
        let over_time_minutes = (over_time_hours_f.abs() * 60.0) as i64 % 60;

        let adjustable_estimated_work_seconds: i64 = *adjustable_estimated_work_seconds_map
            .get(&date)
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
        accumulate_duration_diff_to_limit = accumulate_duration_diff_to_limit + over_time_duration;

        if accumulate_duration_diff_to_limit > max_accumulate_duration_diff_to_limit {
            max_accumulate_duration_diff_to_limit = accumulate_duration_diff_to_limit;
            max_accumulate_duration_diff_to_limit_date = **date;
        }

        if daily_stat_msgs.len() > 0
            && accumulate_duration_diff_to_limit < Duration::seconds(0)
            && **date < first_caught_up_date
        {
            first_caught_up_date = **date;
        }

        if !overload_day_is_found && accumulate_duration_diff_to_limit > Duration::seconds(0) {
            overload_day_is_found = true;
        } else if accumulate_duration_diff_to_limit <= Duration::seconds(300) {
            let flattenable_duration_cand = Duration::seconds(
                free_time_minutes * 60 - total_estimated_work_seconds_of_the_date,
            );
            if flattenable_date_opt == None
                && overload_day_is_found
                && flattenable_duration_cand >= Duration::seconds(900)
            {
                flattenable_date_opt = Some(**date);
                flattenable_duration = flattenable_duration_cand;
            }
        }

        let diff_to_limit_sign: char = if accumulate_duration_diff_to_limit > Duration::minutes(0) {
            ' '
        } else {
            '-'
        };

        let repetitive_task_estimated_work_seconds = *repetitive_task_estimated_work_seconds_map
            .get(&date)
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
        } else {
            if accumulated_rho_diff < 0.0 {
                // なんとかその日のうちに捌けている状態。積む余裕は無い
                Duration::minutes(0)
            } else {
                accumulate_duration_diff_to_goal_rho
            }
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

        if daily_stat_msgs.len() > 0
            && accumulated_rho_diff.is_finite()
            && accumulated_rho_diff > max_accumulated_rho_diff
        {
            max_accumulated_rho_diff = accumulated_rho_diff;
            max_accumulated_rho_diff_date = **date;
        }

        let deadline_rest_duration_seconds: i64 =
            deadline_estimated_work_seconds_map.get(&date).unwrap_or(&0)
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
        if daily_stat_msgs.len() == 0 {
            has_today_deadline_leeway = deadline_rest_sign == '-';
            has_today_freetime_leeway = diff_to_limit_in_day_sign == '-';
            has_today_new_task_leeway = diff_to_goal_sign == '-';
        }

        if daily_stat_msgs.len() == 1 {
            has_tomorrow_deadline_leeway = deadline_rest_sign == '-';
            has_tomorrow_freetime_leeway = diff_to_limit_in_day_sign == '-';
        }

        // 一度フラグが折れていたら復活させない
        // 今日と明日については個別にアラートを出すので、判定はそれ以降について行う。
        if 2 <= daily_stat_msgs.len() && daily_stat_msgs.len() < 7 && has_weekly_deadline_leeway {
            has_weekly_deadline_leeway = deadline_rest_sign == '-';
        }

        if 2 <= daily_stat_msgs.len() && daily_stat_msgs.len() < 7 && has_weekly_freetime_leeway {
            has_weekly_freetime_leeway = diff_to_limit_sign == '-';
        }

        // 今日より前には前倒せないため
        let adjustable_estimated_work_hours = if daily_stat_msgs.len() == 0 {
            0.0
        } else {
            *adjustable_estimated_work_seconds_map
                .get(&date)
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

        daily_stat_msgs.push(s);
    }

    // 逆順にして、下側に直近の日付があるようにする
    daily_stat_msgs.reverse();

    if is_calendar_func && !is_flatten_func {
        for (cal_ind, s) in daily_stat_msgs.iter().enumerate() {
            writeln_newline(stdout, &s).unwrap();

            if s.contains("(月)") && cal_ind != daily_stat_msgs.len() - 1 {
                writeln_newline(stdout, "").unwrap();
            }
        }
        // フッター
        let footer: String = vec![
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

        // 順調フラグが折れている時にアラート表示
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

    let today_total_deadline_estimated_work_hours =
        today_total_deadline_estimated_work_minutes as f64 / 60.0;
    let mu_minutes = max(0, (eod - last_synced_time).num_minutes());
    let mu_hours = mu_minutes as f64 / 60.0;

    let rho1 =
        today_total_deadline_estimated_work_minutes as f64 / (mu_minutes - busy_minutes) as f64;
    let lq1_opt = if rho1 < 1.0 {
        Some(rho1 / (1.0 - rho1))
    } else {
        None
    };

    let today_total_repetitive_estimated_work_seconds = *repetitive_task_estimated_work_seconds_map
        .get(&naive_dt_today)
        .unwrap_or(&0);
    let today_total_repetitive_estimated_work_hours =
        today_total_repetitive_estimated_work_seconds as f64 / 3600.0;

    let non_repetitive_rho =
        if (mu_minutes - busy_minutes - today_total_repetitive_estimated_work_seconds / 60) > 0 {
            (today_total_deadline_estimated_work_minutes as f64
                - today_total_repetitive_estimated_work_seconds as f64 / 60.0)
                / (mu_minutes - busy_minutes - today_total_repetitive_estimated_work_seconds / 60)
                    as f64
        } else {
            f64::INFINITY
        };
    let non_repetitive_lq_opt = if non_repetitive_rho < 1.0 {
        Some(non_repetitive_rho / (1.0 - non_repetitive_rho))
    } else {
        None
    };

    let non_repetitive_rho_msg = format!(
        "one ρ = ({:.2} + 0.00) / ({:.2} + 0.00) = {:4.2}",
        today_total_deadline_estimated_work_hours - today_total_repetitive_estimated_work_hours,
        mu_hours - busy_hours - today_total_repetitive_estimated_work_hours,
        non_repetitive_rho,
    );
    let non_repetitive_lq_msg = match non_repetitive_lq_opt {
        Some(non_repetitive_lq) => format!("Lq = {:.1}", non_repetitive_lq),
        None => "Lq = inf".to_string(),
    };

    let s_for_non_repetitive_rho = format!("{}, {}", non_repetitive_rho_msg, non_repetitive_lq_msg);

    let rho1_msg = format!(
        "rep ρ = ({:.2} + {:.2}) / ({:.2} + {:.2}) = {:4.2}",
        today_total_deadline_estimated_work_hours - today_total_repetitive_estimated_work_hours,
        today_total_repetitive_estimated_work_hours,
        mu_hours - busy_hours - today_total_repetitive_estimated_work_hours,
        today_total_repetitive_estimated_work_hours,
        rho1,
    );

    let lq_msg = match lq1_opt {
        Some(lq1) => format!("Lq = {:.1}", lq1),
        None => "Lq = inf".to_string(),
    };

    let s_for_rho1 = format!("{}, {}", rho1_msg, lq_msg);

    if !is_flatten_func {
        writeln_newline(stdout, &busy_s).unwrap();
        writeln_newline(stdout, &s).unwrap();
        writeln_newline(stdout, &s_for_rho1).unwrap();
        writeln_newline(stdout, &s_for_non_repetitive_rho).unwrap();
    }

    writeln_newline(stdout, "").unwrap();

    // flatten
    if pattern_opt == &Some("平".to_string()) {
        writeln_newline(
            stdout,
            &format!(
                "flatten dst date : {:?} for {:?}",
                flattenable_date_opt, flattenable_duration
            ),
        )
        .unwrap();

        if let Some(flattenable_date) = flattenable_date_opt {
            let mut any_was_flattened = false;
            let mut src_date = flattenable_date - Duration::days(1);

            while !any_was_flattened && src_date >= naive_dt_today {
                writeln_newline(stdout, &format!("src_date: {:?}", src_date)).unwrap();

                // dt_dictを未来から見ていき、〆切に違反しない範囲で、翌日に飛ばしていく
                for (
                    _ind,
                    (_naive_date, _has_no_deadline, dt, _neg_priority, rank, deadline_time_opt, id),
                ) in dt_id_tpl_arr.iter().enumerate().rev()
                {
                    let days_until_deadline = match deadline_time_opt {
                        Some(deadline_time) => (*deadline_time - *dt).num_days(),
                        None => 100,
                    };

                    if dt.date_naive() == src_date && days_until_deadline > 0 {
                        if let Some(task) = task_repository.get_by_id(*id) {
                            if !task.get_is_on_other_side()
                                && task.get_estimated_work_seconds() > 0
                                && flattenable_duration.num_seconds()
                                    > task.get_estimated_work_seconds()
                            // && rank != &0
                            {
                                flattenable_duration = flattenable_duration
                                    - Duration::seconds(task.get_estimated_work_seconds());
                                let dst_dt = get_next_morning_datetime(*dt);
                                task.set_pending_until(dst_dt);
                                task.set_orig_status(Status::Pending);

                                writeln_newline(
                                    stdout,
                                    &format!(
                                        "{}\t{}\t{}\t{}",
                                        // dt,
                                        // dst_dt,
                                        rank,
                                        task.get_id(),
                                        task.get_estimated_work_seconds(),
                                        task.get_name(),
                                    ),
                                )
                                .unwrap();

                                any_was_flattened = true;
                            }
                        }
                    }
                }

                src_date = src_date - Duration::days(1);
            }
        }
    }
}

fn execute_focus(focused_task_id_opt: &mut Option<Uuid>, new_task_id_str: &str) {
    match Uuid::parse_str(new_task_id_str) {
        Ok(id) => *focused_task_id_opt = Some(id),
        Err(_) => {}
    }
}

fn execute_pick(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    new_task_id_str: &str,
) {
    match Uuid::parse_str(new_task_id_str) {
        Ok(id) => {
            *focused_task_id_opt = Some(id);

            // Statusをtodoに戻す
            task_repository.get_by_id(id).map(|task| {
                task.set_orig_status(Status::Todo);
            });
        }
        Err(_) => {
            // 今フォーカスが当たっているタスクをtodoに戻す
            match focused_task_id_opt {
                Some(focused_task_id) => {
                    task_repository.get_by_id(*focused_task_id).map(|task| {
                        task.set_orig_status(Status::Todo);
                    });
                }
                None => {}
            }
        }
    }
}

fn execute_unfocus(focused_task_id_opt: &mut Option<Uuid>) {
    *focused_task_id_opt = None;
}

// 文字列の中からhttpから始まる部分文字列でURLとして解釈できる一番長い文字列を抽出する
fn extract_url(s: &str) -> Option<String> {
    // "http"が始まるインデックスを探す
    if let Some(start) = s.find("http") {
        // "http"から始まる部分文字列を取得する
        let (_, http_str) = s.split_at(start);

        // 末尾の文字を必ずNGにするために、番兵として日本語の文字を置く
        let chars: Vec<char> = (http_str.to_owned() + "あ").chars().collect();

        // その中で二分探索する
        let mut ok: usize = 0;
        let mut ng: usize = chars.len();

        let mut mid = (ok + ng) / 2;

        while ng - ok > 1 {
            let cand_str: String = chars[0..mid].iter().collect();
            let encoded_cand_str: String =
                percent_encode(cand_str.as_bytes(), MY_ASCII_SET).to_string();

            // Url::parse()は未パーセントエンコーディングの文字列(日本語)も受け付けてしまう。
            // もし cand_str == encoded_cand_str なら、日本語が混ざっていないということ
            if Url::parse(&cand_str).is_ok() && cand_str == encoded_cand_str {
                ok = mid;
            } else {
                ng = mid;
            }

            mid = (ok + ng) / 2;
        }

        let ans: String = chars[0..ok].iter().collect();
        return Some(ans);
    } else {
        return None;
    }
}

#[test]
fn test_extract_url_正常系() {
    let input = "これはhttps://example.com?param1=hoge&param2=barというURLです。";
    let actual = extract_url(input);
    let expected = Some(String::from("https://example.com?param1=hoge&param2=bar"));

    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_extract_url_正常系_URLが2つ() {
    let input = "これはhttps://example.com?param1=hoge&param2=barとhttps://example.com";
    let actual = extract_url(input);
    let expected = Some(String::from("https://example.com?param1=hoge&param2=bar"));

    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_extract_url_正常系_2つのURLがスペース区切り() {
    let input = "これはhttps://example.com?param1=hoge&param2=bar https://example.com";
    let actual = extract_url(input);
    let expected = Some(String::from("https://example.com?param1=hoge&param2=bar"));

    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_extract_url_正常系_正しいURLのまま文字列が終わるケース() {
    let input = "正しいURLのまま文字列が終わるケースhttps://example.com/hoge";
    let actual = extract_url(input);
    let expected = Some(String::from("https://example.com/hoge"));

    assert_eq!(actual, expected);
}

//親に辿っていって見つかった最初のリンクを開く
fn execute_open_link(focused_task_opt: &Option<Task>) {
    let mut t_opt: Option<Task> = focused_task_opt.clone();

    // Todo: while-letとかで書ける?
    loop {
        match &t_opt {
            Some(t) => {
                match extract_url(&t.get_name()) {
                    Some(url) => {
                        match webbrowser::open(&url) {
                            // エラーは無視する
                            _ => {}
                        }
                        return;
                    }
                    None => {}
                }

                t_opt = t.parent();
            }
            None => {
                break;
            }
        }
    }
}

#[allow(unused_must_use)]
fn execute_next_up(
    _stdout: &mut RawTerminal<Stdout>,
    focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<Task>,
    new_task_name_str: &str,
    estimated_work_minutes_opt: &Option<i64>,
) {
    focused_task_opt.clone().and_then(|mut focused_task| {
        let mut new_task_attr = TaskAttr::new(new_task_name_str);

        // 親タスクの〆切を引き継ぐ
        if let Some(parent_task) = focused_task.parent() {
            new_task_attr.set_deadline_time_opt(parent_task.get_deadline_time_opt());
        }

        if let Some(estimated_work_minutes) = estimated_work_minutes_opt {
            let new_task_estimated_work_seconds = estimated_work_minutes * 60;
            new_task_attr.set_estimated_work_seconds(new_task_estimated_work_seconds);

            // 親タスクの見積もりをそのぶん減らす
            if let Some(parent_task) = focused_task.parent() {
                let parent_task_estimated_work_seconds = parent_task.get_estimated_work_seconds();
                parent_task.set_estimated_work_seconds(max(
                    0,
                    parent_task_estimated_work_seconds - new_task_estimated_work_seconds,
                ));
            }
        }

        let new_task_id = new_task_attr.get_id().clone();

        focused_task.create_as_parent(new_task_attr);
        *focused_task_id_opt = Some(new_task_id);

        // dummy
        None::<i32>
    });
}

fn execute_breakdown(
    stdout: &mut RawTerminal<Stdout>,
    focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<Task>,
    new_task_names: &[&str],
    pending_until_opt: &Option<DateTime<Local>>,
) {
    // 複数の子タスクを作成した場合は、作成した最初の子タスクにフォーカスを当てる
    let mut focus_is_moved = false;

    focused_task_opt.as_ref().and_then(|focused_task| {
        for new_task_name in new_task_names {
            let mut new_task_attr = TaskAttr::new(new_task_name);

            match pending_until_opt {
                Some(pending_until) => {
                    new_task_attr.set_orig_status(Status::Pending);
                    new_task_attr.set_pending_until(*pending_until);
                }
                None => {}
            }

            let new_task = focused_task.create_as_last_child(new_task_attr);

            // 親タスクに〆切がある場合には、それを引き継ぐ
            match focused_task.get_deadline_time_opt() {
                Some(deadline_time) => new_task.set_deadline_time_opt(Some(deadline_time)),
                None => {
                    // pass
                }
            }

            let msg: String = format!("{} {}", new_task.get_id(), &new_task_name);
            writeln_newline(stdout, msg.as_str()).unwrap();
            if !focus_is_moved {
                // 新しい子タスクにフォーカス(id)を移す
                *focused_task_id_opt = Some(new_task.get_id());
                focus_is_moved = true;
            }
        }

        // dummy
        None::<i32>
    });
}

fn execute_breakdown_sequentially(
    _stdout: &mut RawTerminal<Stdout>,
    focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<Task>,
    new_task_name_str: &str,
    estimated_work_minutes: i64,
    begin_index: u64,
    end_index: u64,
    new_task_name_suffix_str: &str,
) {
    if let Some(focused_task) = focused_task_opt {
        let grand_child_task_result = focused_task.create_sequential_children(
            new_task_name_str,
            estimated_work_minutes * 60,
            begin_index,
            end_index,
            new_task_name_suffix_str,
        );

        if let Ok(grand_child_task) = grand_child_task_result {
            // フォーカスを移す
            *focused_task_id_opt = Some(grand_child_task.get_id());
        }
    }
}

fn execute_create_repetition_task(
    _stdout: &mut RawTerminal<Stdout>,
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<Task>,
    new_task_name_str: &str,
    exec_day_str: &str,
    estimated_work_minutes: i64,
    start_time_str: &str,
    deadline_time_str: &str,
) {
    // まず繰り返しタスクの親タスクを作る。
    execute_breakdown(
        _stdout,
        focused_task_id_opt,
        focused_task_opt,
        &[new_task_name_str],
        &None,
    );
    let repetition_parent_task_opt =
        focused_task_id_opt.and_then(|id| task_repository.get_by_id(id));
    execute_set_estimated_work_minutes(
        &repetition_parent_task_opt,
        &format!("{}", estimated_work_minutes),
    );

    let task_num = if exec_day_str == "毎" { 7 } else { 4 };

    if let Some(focused_task_id) = focused_task_id_opt {
        let repetition_parent_task_id = focused_task_id.clone();
        let focused_task_opt = focused_task_id_opt.and_then(|id| task_repository.get_by_id(id));

        // ループを回して子タスクを作る
        for _ in 0..task_num {
            execute_breakdown(
                _stdout,
                focused_task_id_opt,
                &focused_task_opt,
                &[new_task_name_str],
                &None,
            );
            let child_task_opt = focused_task_id_opt.and_then(|id| task_repository.get_by_id(id));
            execute_set_estimated_work_minutes(
                &child_task_opt,
                &format!("{}", estimated_work_minutes),
            );

            // 次ここから作業再開する。start_timeを作るために、「毎」か「月~日」でそれぞれ日付をループさせたい
            focused_task.set_start_time(start_dst_time);

            execute_focus(
                focused_task_id_opt,
                &repetition_parent_task_id.hyphenated().to_string(),
            );
        }
    }
}

fn execute_split(
    stdout: &mut RawTerminal<Stdout>,
    focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<Task>,
    new_task_name: &str,
    splitted_work_minutes_str: &str,
) {
    match focused_task_opt {
        None => {}
        Some(focused_task) => {
            // 今のタスクの予時間をn減らす
            // 下 <new_task_name>
            // 予 n

            let focused_estimated_work_seconds = focused_task.get_estimated_work_seconds();

            // もしsplitted_work_minutes_strがマイナスの場合は、親タスクにその値だけ残すようにする
            // 割 -30 <新タスク> なら、(親タスク-30)を見積もりとして<新タスク>を作るよ、という意味合い
            let splitted_work_minutes: i64 = splitted_work_minutes_str.parse::<i64>().unwrap();

            let splitted_work_seconds: i64 = if splitted_work_minutes > 0 {
                min(splitted_work_minutes * 60, focused_estimated_work_seconds)
            } else {
                // このif分岐では負の場合splitted_work_minutesは負だが、
                // 分かりやすいようにabs()して引き算している
                max(
                    0,
                    focused_estimated_work_seconds - splitted_work_minutes.abs() * 60,
                )
            };

            focused_task
                .set_estimated_work_seconds(focused_estimated_work_seconds - splitted_work_seconds);

            let mut new_task_attr = TaskAttr::new(new_task_name);
            new_task_attr.set_estimated_work_seconds(splitted_work_seconds);

            // 親タスクに〆切がある場合には、それを引き継ぐ
            match focused_task.get_deadline_time_opt() {
                Some(deadline_time) => new_task_attr.set_deadline_time_opt(Some(deadline_time)),
                None => {
                    // pass
                }
            }

            let new_task = focused_task.create_as_last_child(new_task_attr);

            let msg: String = format!("{} {}", new_task.get_id(), &new_task_name);
            writeln_newline(stdout, msg.as_str()).unwrap();

            // 新しい子タスクにフォーカス(id)を移す
            *focused_task_id_opt = Some(new_task.get_id());
        }
    }
}

fn split_amount_and_unit(input: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut buffer = String::new();

    for c in input.chars() {
        if c.is_numeric() {
            buffer.push(c);
        } else {
            break;
        }
    }

    result.push(buffer);
    result.push(input[result[0].len()..].to_string());

    result
}

#[test]
fn test_split_amount_and_unit() {
    let input = "暦";
    let actual = split_amount_and_unit(input);

    assert_eq!(actual, vec!["".to_string(), "暦".to_string()]);
}

#[test]
fn test_split_amount_and_unit_err() {
    let input = "6543abc123def456gh789";
    let actual = split_amount_and_unit(input);

    assert_eq!(
        actual,
        vec!["6543".to_string(), "abc123def456gh789".to_string()]
    );
}

fn execute_wait_for_others(focused_task_opt: &Option<Task>) {
    focused_task_opt
        .as_ref()
        .map(|focused_task| focused_task.set_is_on_other_side(true));
}

fn execute_defer(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<Task>,
    amount_str: &str,
    unit_str: &str,
) {
    let amount: i64 = amount_str.parse().unwrap();
    let duration = match unit_str.chars().nth(0) {
        // 24時間単位ではなく、next_monring単位とする
        Some('日') | Some('d') => {
            let mut dt = task_repository.get_last_synced_time();

            for _ in 0..amount {
                dt = get_next_morning_datetime(dt);
            }

            dt - task_repository.get_last_synced_time()
        }
        Some('時') | Some('h') => Duration::hours(amount),
        Some('分') | Some('m') => Duration::minutes(amount),
        // 誤入力した時に傷が浅いように、デフォルトは秒としておく
        Some('秒') | Some('s') | _ => Duration::seconds(amount),
    };

    focused_task_opt.as_ref().and_then(|focused_task| {
        focused_task.set_pending_until(task_repository.get_last_synced_time() + duration);
        focused_task.set_orig_status(Status::Pending);

        // dummy
        None::<i32>
    });

    *focused_task_id_opt = None;
}

// 指定の日付から、step_days間隔でdeferしていく
fn execute_extrude(
    _focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<Task>,
    first_datetime: &DateTime<Local>,
    step_days: u16,
) {
    if let Some(focused_task) = focused_task_opt {
        let mut pending_until_datetime = *first_datetime;

        for (_, task) in focused_task
            .list_all_parent_tasks_with_first_available_time()
            .iter()
        {
            if focused_task.get_status() != Status::Done {
                task.set_orig_status(Status::Pending);
                task.set_pending_until(pending_until_datetime);

                pending_until_datetime = pending_until_datetime + Duration::days(step_days as i64);

                // 平日の仕事用: 土日にはextrudeせずにスキップする
                // match pending_until_datetime.weekday() {
                //     Weekday::Sat => {
                //         pending_until_datetime = pending_until_datetime + Duration::days(2);
                //     }
                //     Weekday::Sun => {
                //         pending_until_datetime = pending_until_datetime + Duration::days(1);
                //     }
                //     _ => {}
                // }
            }
        }
    }
}

fn execute_finish(focused_task_id_opt: &mut Option<Uuid>, focused_task_opt: &Option<Task>) {
    focused_task_opt.as_ref().and_then(|focused_task| {
        focused_task.set_orig_status(Status::Done);
        focused_task.set_end_time_opt(Some(Local::now()));

        // 親タスクがrepetition_interval_daysを持っているなら、
        // その値に従って兄弟ノードを生成する
        // start_timeは日付は(repetition_interval_days-1)日後で、時刻は親タスクのstart_timeを引き継ぐ
        // タスク名は「親タスク名(日付)」
        // deadline_timeの時刻は親タスクのdeadline_timeを引き継ぐ (無ければ23:59)
        // estimated_work_secondsは親タスクを引き継ぐ
        match focused_task.parent() {
            Some(parent_task) => match parent_task.get_repetition_interval_days_opt() {
                Some(repetition_interval_days) => {
                    // まず、親タスクの見積もり時間を実作業時間に応じて調整する
                    // 子タスクの実作業時間が 0(不明) の時は調整しない
                    if focused_task.get_actual_work_seconds() > 0 {
                        let orig_estimated_sec = parent_task.get_estimated_work_seconds();

                        let diff = focused_task.get_actual_work_seconds() - orig_estimated_sec;

                        if diff > 0 {
                            // ブレがあることを踏まえて、その値そのものにはしないようにする。
                            // 2分探索の気分で、2で割るのを基本としたかったが、人は見積もりを過小評価しがちなので、大きくする方向については75%採用する
                            let new_estimated_work_seconds = orig_estimated_sec + diff * 3 / 4;
                            parent_task.set_estimated_work_seconds(new_estimated_work_seconds);
                        } else if diff < 0 {
                            // 見積もりは最短でも1分になるようにする
                            // 人は見積もりを過小評価しがちなので、見積もりをさらに小さくする方向については慎重に。25%採用する
                            let new_estimated_work_seconds = max(60, orig_estimated_sec + diff / 4);
                            parent_task.set_estimated_work_seconds(new_estimated_work_seconds);
                        }
                    }

                    let parent_task_name = parent_task.get_name();
                    let parent_task_start_time = parent_task.get_start_time();
                    let days_in_advance = parent_task.get_days_in_advance();
                    let new_start_time = get_next_morning_datetime(
                        Local::now() + Duration::days(repetition_interval_days - 1),
                    )
                    .with_hour(parent_task_start_time.hour())
                    .unwrap()
                    .with_minute(parent_task_start_time.minute())
                    .unwrap()
                    .with_second(parent_task_start_time.second())
                    .unwrap()
                    .with_nanosecond(0)
                    .unwrap();
                    let new_task_month = new_start_time.month();
                    let new_task_day = new_start_time.day();
                    let new_task_name =
                        format!("{}({}/{})", parent_task_name, new_task_month, new_task_day);

                    let new_deadline_time = match parent_task.get_deadline_time_opt() {
                        Some(parent_task_deadline_time) => new_start_time
                            .with_hour(parent_task_deadline_time.hour())
                            .unwrap()
                            .with_minute(parent_task_deadline_time.minute())
                            .unwrap()
                            .with_nanosecond(0)
                            .unwrap(),
                        None => new_start_time
                            .with_hour(23)
                            .unwrap()
                            .with_minute(59)
                            .unwrap()
                            .with_second(59)
                            .unwrap(),
                    };

                    let estimated_work_seconds = parent_task.get_estimated_work_seconds();

                    let mut new_task_attr = TaskAttr::new(&new_task_name);
                    // deadlineの決定などに影響を与えたくないので、最後にdays_in_advanceを引く
                    new_task_attr.set_start_time(new_start_time - Duration::days(days_in_advance));
                    new_task_attr.set_deadline_time_opt(Some(new_deadline_time));
                    new_task_attr.set_estimated_work_seconds(estimated_work_seconds);
                    parent_task.create_as_last_child(new_task_attr);
                }
                None => {}
            },
            None => {}
        }

        // 兄弟タスクが全て完了している場合は、フォーカスを親タスクに移す。
        // そうでなければ、フォースを外す
        *focused_task_id_opt = if focused_task.all_sibling_tasks_are_all_done() {
            focused_task.parent().map(|t| t.get_id())
        } else {
            None
        };

        // dummy
        None::<i32>
    });
}

fn execute_set_deadline(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_opt: &Option<Task>,
    deadline_date_str: &str,
) {
    if deadline_date_str == "消" {
        focused_task_opt
            .as_ref()
            .map(|focused_task| focused_task.unset_deadline_time_opt());
        return;
    }

    let mut deadline_time_str = format!("{} 23:59:59", deadline_date_str);
    let hhmm_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})$").unwrap();

    // 時刻のみを指定した場合は、日付は今日にする
    if hhmm_reg.is_match(deadline_date_str) {
        let caps = hhmm_reg.captures(deadline_date_str).unwrap();
        let hh: u32 = caps[1].parse().unwrap();
        let mm: u32 = caps[2].parse().unwrap();

        let now = task_repository.get_last_synced_time();
        deadline_time_str = format!(
            "{} {:02}:{:02}:00",
            now.format("%Y/%m/%d").to_string(),
            hh,
            mm
        );
    }

    let deadline_time_opt_result = Local.datetime_from_str(&deadline_time_str, "%Y/%m/%d %H:%M:%S");

    if let Ok(deadline_time) = deadline_time_opt_result {
        focused_task_opt
            .as_ref()
            .map(|focused_task| focused_task.set_deadline_time_opt(Some(deadline_time)));
    }
}

#[allow(unused_must_use)]
fn execute_set_estimated_work_minutes(
    focused_task_opt: &Option<Task>,
    estimated_work_minutes_str: &str,
) {
    let estimated_minutes_result = estimated_work_minutes_str.parse::<i64>();

    estimated_minutes_result.map(|estimated_work_minutes| {
        let estimated_work_seconds = estimated_work_minutes * 60;
        focused_task_opt
            .as_ref()
            .map(|focused_task| focused_task.set_estimated_work_seconds(estimated_work_seconds));
    });
}

fn execute_set_arrange_children_work_minutes(
    focused_task_opt: &Option<Task>,
    estimated_work_minutes_str: &str,
) {
    let estimated_minutes_result = estimated_work_minutes_str.parse::<i64>();

    // 繰り返しタスクについて、その子タスクでDoneでないものの時間を一律変更する。
    if let Ok(estimated_minutes) = estimated_minutes_result {
        if let Some(focused_task) = focused_task_opt {
            if focused_task.get_repetition_interval_days_opt().is_some() {
                print!("{:?}", focused_task.get_repetition_interval_days_opt());
                let children = focused_task.get_children();
                for child_task in children.iter() {
                    if child_task.get_status() != Status::Done {
                        child_task.set_estimated_work_seconds(estimated_minutes * 60);
                    }
                }
            }
        }
    }
}

#[allow(unused_must_use)]
fn execute_set_actual_work_minutes(focused_task_opt: &Option<Task>, actual_work_minutes_str: &str) {
    let actual_minutes_result = actual_work_minutes_str.parse::<i64>();

    actual_minutes_result.map(|actual_work_minutes| {
        let actual_work_seconds = actual_work_minutes * 60;
        focused_task_opt
            .as_ref()
            .map(|focused_task| focused_task.set_actual_work_seconds(actual_work_seconds));
    });
}

fn decide_time(tokens: &Vec<&str>, now: &DateTime<Local>) -> Option<DateTime<Local>> {
    let mut start_time = None;

    if tokens.len() >= 2 {
        let start_hhmm_str = &tokens[1];

        // 日付はオプショナル引数。入力されなかった場合は今日の日付とする。
        let start_date_str = if tokens.len() >= 3 {
            &tokens[2]
        } else {
            "dummy"
        };

        let hhmm_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})$").unwrap();
        let (hh, mm) = if hhmm_reg.is_match(start_hhmm_str) {
            let caps = hhmm_reg.captures(start_hhmm_str).unwrap();
            let hh: u32 = caps[1].parse().unwrap();
            let mm: u32 = caps[2].parse().unwrap();

            (hh, mm)
        } else {
            (12, 00)
        };

        let yyyymmdd_reg = Regex::new(r"^(\d{2,4})/(\d{1,2})/(\d{1,2})$").unwrap();
        let mmdd_reg = Regex::new(r"^(\d{1,2})/(\d{1,2})$").unwrap();

        let start_time_tmp = if yyyymmdd_reg.is_match(start_date_str) {
            let caps = yyyymmdd_reg.captures(start_date_str).unwrap();
            let tmp_yyyy: i32 = caps[1].parse().unwrap();
            let yyyy = if tmp_yyyy < 100 {
                tmp_yyyy + 2000
            } else {
                tmp_yyyy
            };
            let mm_month: u32 = caps[2].parse().unwrap();
            let dd: u32 = caps[3].parse().unwrap();

            Local
                .with_ymd_and_hms(yyyy, mm_month, dd, hh, mm, 0)
                .unwrap()
        } else if mmdd_reg.is_match(start_date_str) {
            // 年なしの日付が指定された場合は未来方向でその日付に合致する日付に送る
            let caps = mmdd_reg.captures(start_date_str).unwrap();
            let mm_month: u32 = caps[1].parse().unwrap();
            let dd: u32 = caps[2].parse().unwrap();

            let mut ans_datetime = Local
                .with_ymd_and_hms(now.year(), mm_month, dd, hh, mm, 0)
                .unwrap();

            if ans_datetime < *now {
                ans_datetime = Local
                    .with_ymd_and_hms(now.year() + 1, mm_month, dd, hh, mm, 0)
                    .unwrap()
            }

            ans_datetime
        } else if tokens.len() >= 3
            && vec!["月", "火", "水", "木", "金", "土", "日"].contains(&tokens[2])
        {
            // 月 火 水 木 金 土 日 が指定された時は、明日以降で、直近のその曜日とする。
            // (show_all_tasksとロジック重複...)
            let days_of_week = vec!["月", "火", "水", "木", "金", "土", "日"];

            let todays_morning_datetime = get_next_morning_datetime(*now) - Duration::days(1);

            let dn = todays_morning_datetime.date_naive();
            let now_weekday_jp = get_weekday_jp(&dn);

            let now_days_of_week_ind = days_of_week
                .iter()
                .position(|&x| &x == &now_weekday_jp)
                .unwrap();
            let target_days_of_week_ind =
                days_of_week.iter().position(|&x| x == tokens[2]).unwrap();

            let ind_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

            // 今日の6:00にdeferする味意はないので、その代わりに、1週間後の同じ曜日にdeferできるようにする
            let days: i64 = if ind_diff == 0 { 7 } else { ind_diff as i64 };
            let n_days_after_datetime = get_next_morning_datetime(*now) + Duration::days(days - 1);
            let ans_datetime = Local
                .with_ymd_and_hms(
                    n_days_after_datetime.year(),
                    n_days_after_datetime.month(),
                    n_days_after_datetime.day(),
                    hh,
                    mm,
                    0,
                )
                .unwrap();

            ans_datetime
        } else {
            Local
                .with_ymd_and_hms(now.year(), now.month(), now.day(), hh, mm, 0)
                .unwrap()
        };

        start_time = Some(start_time_tmp);
    }

    start_time
}

fn execute(
    stdout: &mut RawTerminal<Stdout>,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &DateTime<Local>,
    untrimmed_line: &str,
) {
    // 整形
    let re = Regex::new(r"\s+").unwrap();
    let line: String = re
        .replace_all(untrimmed_line, " ")
        .to_string()
        .trim()
        .to_string();

    let focused_task_opt: Option<Task> =
        focused_task_id_opt.and_then(|id| task_repository.get_by_id(id));

    let tokens: Vec<&str> = line.split(' ').collect();

    if tokens.is_empty() {
        return;
    }

    match tokens[0] {
        "新" | "遊" | "new" | "hobby" => {
            if tokens.len() >= 2 {
                let new_project_name_str = &tokens[1];

                let estimated_work_minutes_opt: Option<i64> = if tokens.len() >= 3 {
                    match tokens[2].parse() {
                        Ok(m) => Some(m),
                        Err(_) => None,
                    }
                } else {
                    None
                };

                let defer_days_opt = if tokens[0] == "新" || tokens[0] == "new" {
                    Some(1)
                } else {
                    Some(1400)
                };
                execute_start_new_project(
                    stdout,
                    focused_task_id_opt,
                    task_repository,
                    new_project_name_str,
                    defer_days_opt,
                    estimated_work_minutes_opt,
                );
            }
        }
        "突" | "unplanned" => {
            if tokens.len() >= 2 {
                let new_project_name_str = &tokens[1];

                let estimated_work_minutes_opt: Option<i64> = if tokens.len() >= 3 {
                    match tokens[2].parse() {
                        Ok(m) => Some(m),
                        Err(_) => None,
                    }
                } else {
                    None
                };

                let defer_days_opt = None;
                execute_start_new_project(
                    stdout,
                    focused_task_id_opt,
                    task_repository,
                    new_project_name_str,
                    defer_days_opt,
                    estimated_work_minutes_opt,
                );
            }
        }
        "連" | "sequential" | "seq" => {
            if tokens.len() >= 5 {
                let new_task_name_str = &tokens[1];
                let estimated_work_minutes_result = &tokens[2].parse();
                let begin_index_result = &tokens[3].parse();
                let end_index_result = &tokens[4].parse();
                let new_task_name_suffix_str = if tokens.len() >= 6 { &tokens[5] } else { "" };

                if let Ok(estimated_work_minutes) = estimated_work_minutes_result {
                    if let Ok(begin_index) = begin_index_result {
                        if let Ok(end_index) = end_index_result {
                            if begin_index <= end_index {
                                execute_breakdown_sequentially(
                                    stdout,
                                    focused_task_id_opt,
                                    &focused_task_opt,
                                    new_task_name_str,
                                    *estimated_work_minutes,
                                    *begin_index,
                                    *end_index,
                                    new_task_name_suffix_str,
                                );
                            }
                        }
                    }
                }
            }
        }
        "繰" | "repeat" => {
            if tokens.len() == 6 {
                let new_task_name_str = &tokens[1];
                let estimated_work_minutes_result = &tokens[2].parse();
                let day = &tokens[3];
                let start_time_str = &tokens[4];
                let deadline_time_str = &tokens[5];

                if let Ok(estimated_work_minutes) = estimated_work_minutes_result {
                    execute_create_repetition_task(
                        stdout,
                        task_repository,
                        focused_task_id_opt,
                        &focused_task_opt,
                        new_task_name_str,
                        day,
                        *estimated_work_minutes,
                        start_time_str,
                        deadline_time_str,
                    )
                }
            }
        }
        "約" | "appointment" => {
            let now = task_repository.get_last_synced_time();
            let start_time_opt = decide_time(&tokens, &now);

            if let Some(start_time) = start_time_opt {
                execute_make_appointment(&focused_task_opt, start_time);
            }
        }
        "始" | "start" => {
            let now: DateTime<Local> = task_repository.get_last_synced_time();
            let start_dst_time_opt = decide_time(&tokens, &now);

            if let Some(start_dst_time) = start_dst_time_opt {
                if let Some(focused_task) =
                    focused_task_id_opt.and_then(|id| task_repository.get_by_id(id))
                {
                    focused_task.set_start_time(start_dst_time);
                }
            }
        }
        // 最初は「木」コマンドだったが、曜日だけを指定して直近のその曜日について「全」コマンドを動かすコマンドとコンフリクトしてしまったためリネームした。
        "樹" | "tree" => {
            execute_show_tree(stdout, &focused_task_opt);
        }
        "条" | "祖" | "ancestor" | "anc" => {
            execute_show_ancestor(stdout, &focused_task_opt);
        }
        "根" | "root" => match focused_task_opt {
            Some(focused_task) => {
                let root_task = focused_task.root();
                let root_task_id = root_task.get_id();
                execute_focus(focused_task_id_opt, &root_task_id.hyphenated().to_string());
            }
            None => {}
        },
        "葉" | "leaves" | "leaf" | "lf" => {
            execute_show_leaf_tasks(stdout, task_repository, free_time_manager);
        }
        "全" | "all" => {
            let pattern_opt = if tokens.len() >= 2 {
                Some(tokens[1].to_string())
            } else {
                None
            };

            execute_show_all_tasks(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
            );
        }
        "今" | "today" => {
            let pattern_opt = Some("今".to_string());
            execute_show_all_tasks(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
            );
        }
        "暦" | "cal" => {
            let pattern_opt = Some("暦".to_string());
            execute_show_all_tasks(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
            );
        }
        "見" | "focus" | "fc" => {
            if tokens.len() >= 2 {
                let new_task_id_str = &tokens[1];
                execute_focus(focused_task_id_opt, new_task_id_str);
            }
        }
        "選" | "pick" => {
            let new_task_id_str = if tokens.len() >= 2 { &tokens[1] } else { "" };
            execute_pick(task_repository, focused_task_id_opt, new_task_id_str);
        }
        "開" | "open" | "op" => {
            execute_open_link(&focused_task_opt);
        }
        "外" | "unfocus" | "ufc" => {
            execute_unfocus(focused_task_id_opt);
        }
        "親" | "parent" => match focused_task_opt {
            Some(focused_task) => match focused_task.parent() {
                Some(parent_task) => {
                    let parent_task_id = parent_task.get_id();
                    execute_focus(
                        focused_task_id_opt,
                        &parent_task_id.hyphenated().to_string(),
                    );
                }
                None => {}
            },
            None => {}
        },
        "子" | "children" | "ch" => {
            // 今見ているノードの子タスクが1つだけの時、その子に移動する
            // 2つ以上ある時には、「木」コマンドを実行してツリーの様子を表示する

            if let Some(ref focused_task) = focused_task_opt {
                let tmp_children = focused_task.get_children();
                let children: Vec<_> = tmp_children
                    .iter()
                    .filter(|child| child.get_status() != Status::Done)
                    .collect();

                match children.len() {
                    0 => {
                        // Do nothing
                    }
                    1 => {
                        *focused_task_id_opt = Some(children[0].get_id());
                    }
                    _ => {
                        execute_show_tree(stdout, &focused_task_opt);
                    }
                }
            }
        }
        "深" | "deep" | "deepest" => {
            // 今見ているノードの子タスクが1つだけである限り、その子に移動して同じことを繰り返す
            // 2つ以上ある時には、「木」コマンドを実行してツリーの様子を表示する

            if let Some(ref focused_task) = focused_task_opt {
                let mut tmp_focused_task_opt: Option<Task> = Some(focused_task.clone());

                loop {
                    if let Some(ref tmp_focused_task) = tmp_focused_task_opt {
                        let tmp_children = tmp_focused_task.get_children();
                        let children: Vec<_> = tmp_children
                            .iter()
                            .filter(|child| child.get_status() != Status::Done)
                            .collect();

                        if children.len() != 1 {
                            break;
                        }

                        tmp_focused_task_opt = Some(children[0].clone());
                    } else {
                        break;
                    }
                }

                if let Some(ref tmp_focused_task) = tmp_focused_task_opt {
                    *focused_task_id_opt = Some(tmp_focused_task.get_id());

                    if tmp_focused_task.get_children().len() > 1 {
                        execute_show_tree(stdout, &tmp_focused_task_opt);
                    }
                }
            }
        }
        "上" | "nextup" | "nu" => {
            if tokens.len() >= 2 {
                let new_task_name_str = &tokens[1];

                let estimated_work_minutes_opt: Option<i64> = if tokens.len() >= 3 {
                    match tokens[2].parse() {
                        Ok(m) => Some(m),
                        Err(_) => None,
                    }
                } else {
                    None
                };

                execute_next_up(
                    stdout,
                    focused_task_id_opt,
                    &focused_task_opt,
                    new_task_name_str,
                    &estimated_work_minutes_opt,
                );
            }
        }
        "下" | "breakdown" | "bd" => {
            if tokens.len() >= 2 {
                let new_task_names = &tokens[1..];

                // 「割」コマンドと間違えて数値を引数に取った場合は何もしない
                if !tokens.iter().any(|token| token.parse::<i64>().is_ok()) {
                    execute_breakdown(
                        stdout,
                        focused_task_id_opt,
                        &focused_task_opt,
                        new_task_names,
                        &None,
                    );
                }
            }
        }
        "割" | "split" | "sp" => {
            if tokens.len() == 3 {
                let splitted_work_minutes_str = &tokens[1];
                let new_task_name = &tokens[2];

                execute_split(
                    stdout,
                    focused_task_id_opt,
                    &focused_task_opt,
                    new_task_name,
                    splitted_work_minutes_str,
                );
            }
        }
        // "詳" | "description" | "desc" => {}
        "待" | "wait" => {
            // フラグを立てるだけか、deferコマンドを自動実行するかは迷う。
            execute_wait_for_others(&focused_task_opt);
        }
        "〆" | "締" | "deadline" => {
            if tokens.len() >= 2 {
                // "2023/05/23"とか。簡単のため、時刻は指定不要とし、自動的に23:59を〆切と設定する
                // 5/23のようにhh/mmで指定した場合は、年の情報を補完してその日の23:59を〆切と設定する
                // 月~日と指定した場合、明日以降で直近のその曜日の23:59を〆切と設定する

                let deadline_date_str = &tokens[1];

                let now: DateTime<Local> = task_repository.get_last_synced_time();

                let mmdd_reg = Regex::new(r"^(\d{1,2})/(\d{1,2})$").unwrap();

                if tokens[1].starts_with('今') {
                    let s = (get_next_morning_datetime(now) - Duration::days(1))
                        .format("%Y/%m/%d")
                        .to_string();
                    execute_set_deadline(task_repository, &focused_task_opt, &s);
                } else if tokens[1].starts_with('明') {
                    let s = get_next_morning_datetime(now)
                        .format("%Y/%m/%d")
                        .to_string();
                    execute_set_deadline(task_repository, &focused_task_opt, &s);
                } else if vec!["月", "火", "水", "木", "金", "土", "日"].contains(&tokens[1])
                {
                    // 月 火 水 木 金 土 日 が指定された時は、明日以降で、直近のその曜日の23:59を〆切とする
                    // (show_all_tasksとロジック重複...)

                    let days_of_week = vec!["月", "火", "水", "木", "金", "土", "日"];

                    let todays_morning_datetime =
                        get_next_morning_datetime(now) - Duration::days(1);

                    let dn = todays_morning_datetime.date_naive();
                    let now_weekday_jp = get_weekday_jp(&dn);

                    let now_days_of_week_ind = days_of_week
                        .iter()
                        .position(|&x| &x == &now_weekday_jp)
                        .unwrap();
                    let target_days_of_week_ind =
                        days_of_week.iter().position(|&x| x == tokens[1]).unwrap();

                    let ind_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                    // 今日の〆切については「〆 今」で設定できるので、その代わりに、1週間後の同じ曜日の情報を設定するようにする
                    let days: i64 = if ind_diff == 0 { 7 } else { ind_diff as i64 };

                    let s = (get_next_morning_datetime(now) + Duration::days(days - 1))
                        .format("%Y/%m/%d")
                        .to_string();

                    execute_set_deadline(task_repository, &focused_task_opt, &s);
                } else if mmdd_reg.is_match(&tokens[1]) {
                    // FIXME 「後」コマンドとロジック重複

                    let caps = mmdd_reg.captures(tokens[1]).unwrap();
                    let mm: u32 = caps[1].parse().unwrap();
                    let dd: u32 = caps[2].parse().unwrap();

                    // この時点では12:00にしているが、後で時刻を無視するので問題ない
                    let mut deadline_dst_time = Local
                        .with_ymd_and_hms(now.year(), mm, dd, 12, 0, 0)
                        .unwrap();

                    if deadline_dst_time < now {
                        deadline_dst_time = get_next_morning_datetime(
                            Local
                                .with_ymd_and_hms(now.year() + 1, mm, dd, 12, 0, 0)
                                .unwrap(),
                        ) - Duration::days(1);
                    }

                    let s = deadline_dst_time.format("%Y/%m/%d").to_string();

                    execute_set_deadline(task_repository, &focused_task_opt, &s);
                } else {
                    execute_set_deadline(task_repository, &focused_task_opt, deadline_date_str);
                }
            }
        }
        "予" | "estimate" | "es" => {
            if tokens.len() >= 2 {
                let estimated_work_minutes_str = &tokens[1];
                execute_set_estimated_work_minutes(&focused_task_opt, estimated_work_minutes_str);
            }
        }
        "揃" | "arrange" | "arr" => {
            if tokens.len() >= 2 {
                let estimated_work_minutes_str = &tokens[1];
                execute_set_arrange_children_work_minutes(
                    &focused_task_opt,
                    estimated_work_minutes_str,
                );
            }
        }
        "実" | "actual" | "ac" => {
            if tokens.len() >= 2 {
                let actual_work_minutes_str = &tokens[1];
                execute_set_actual_work_minutes(&focused_task_opt, actual_work_minutes_str);
            }
        }
        "働" | "work" | "wk" => {
            let additional_actual_work_minutes: i64 = if tokens.len() >= 2 {
                tokens[1].parse().unwrap()
            } else {
                (Local::now() - *focus_started_datetime).num_minutes() + 1
            };

            if let Some(ref focused_task) = focused_task_opt {
                let original_actual_work_minutes = focused_task.get_actual_work_seconds() / 60;
                let actual_work_minutes_str = format!(
                    "{}",
                    original_actual_work_minutes + additional_actual_work_minutes
                );
                execute_set_actual_work_minutes(&focused_task_opt, &actual_work_minutes_str);
                *focused_task_id_opt = None;
            }
        }
        "後" | "defer" => {
            if tokens.len() >= 3 {
                let amount_str = &tokens[1];
                let unit_str = &tokens[2].to_lowercase();

                execute_defer(
                    task_repository,
                    focused_task_id_opt,
                    &focused_task_opt,
                    amount_str,
                    unit_str,
                );
            } else if tokens.len() == 2 {
                let yyyymmdd_reg = Regex::new(r"^\d{4}/\d{2}/\d{2}$").unwrap();
                let mmdd_reg = Regex::new(r"^(\d{1,2})/(\d{1,2})$").unwrap();
                let hhmm_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})$").unwrap();

                if yyyymmdd_reg.is_match(tokens[1]) {
                    let defer_dst_str = format!("{} 12:00:00", tokens[1]);
                    let defer_dst_date_result =
                        Local.datetime_from_str(&defer_dst_str, "%Y/%m/%d %H:%M:%S");

                    match defer_dst_date_result {
                        Ok(defer_dst_date) => {
                            let defer_dst_time =
                                get_next_morning_datetime(defer_dst_date) - Duration::days(1);

                            let now: DateTime<Local> = task_repository.get_last_synced_time();
                            let seconds = (defer_dst_time - now).num_seconds() + 1;

                            execute_defer(
                                task_repository,
                                focused_task_id_opt,
                                &focused_task_opt,
                                &format!("{}", seconds),
                                "秒",
                            );
                        }
                        Err(_) => {
                            // pass
                        }
                    }
                } else if mmdd_reg.is_match(tokens[1]) {
                    // 年なしの日付が指定された場合は未来方向でその日付に合致する日付に送る
                    let now: DateTime<Local> = task_repository.get_last_synced_time();

                    let caps = mmdd_reg.captures(tokens[1]).unwrap();
                    let mm: u32 = caps[1].parse().unwrap();
                    let dd: u32 = caps[2].parse().unwrap();

                    let defer_dst_date = Local
                        .with_ymd_and_hms(now.year(), mm, dd, 12, 0, 0)
                        .unwrap();

                    let mut defer_dst_time =
                        get_next_morning_datetime(defer_dst_date) - Duration::days(1);

                    if defer_dst_time < now {
                        defer_dst_time = get_next_morning_datetime(
                            Local
                                .with_ymd_and_hms(now.year() + 1, mm, dd, 12, 0, 0)
                                .unwrap(),
                        ) - Duration::days(1);
                    }

                    let seconds = (defer_dst_time - now).num_seconds() + 1;

                    if seconds > 0 {
                        execute_defer(
                            task_repository,
                            focused_task_id_opt,
                            &focused_task_opt,
                            &format!("{}", seconds),
                            "秒",
                        );
                    }
                } else if hhmm_reg.is_match(tokens[1]) {
                    // 時刻が指定された時は今日のその時刻まで送る。25:00のような指定も可能
                    let now: DateTime<Local> = task_repository.get_last_synced_time();

                    let caps = hhmm_reg.captures(tokens[1]).unwrap();
                    let hh_i64: i64 = caps[1].parse().unwrap();
                    let mm: u32 = caps[2].parse().unwrap();

                    let hh = (hh_i64 % 24) as u32;

                    let defer_dst_time = now
                        .with_hour(hh % 24)
                        .expect("invalid hour")
                        .with_minute(mm)
                        .expect("invalid minute")
                        + Duration::days(hh_i64 / 24);

                    let seconds = (defer_dst_time - now).num_seconds() + 1;

                    if seconds > 0 {
                        execute_defer(
                            task_repository,
                            focused_task_id_opt,
                            &focused_task_opt,
                            &format!("{}", seconds),
                            "秒",
                        );
                    }
                } else if vec!["月", "火", "水", "木", "金", "土", "日"].contains(&tokens[1])
                {
                    // 月 火 水 木 金 土 日 が指定された時は、明日以降で、直近のその曜日の06:00にpendingする
                    // (show_all_tasksとロジック重複...)

                    let now: DateTime<Local> = task_repository.get_last_synced_time();
                    let days_of_week = vec!["月", "火", "水", "木", "金", "土", "日"];

                    let todays_morning_datetime =
                        get_next_morning_datetime(now) - Duration::days(1);

                    let dn = todays_morning_datetime.date_naive();
                    let now_weekday_jp = get_weekday_jp(&dn);

                    let now_days_of_week_ind = days_of_week
                        .iter()
                        .position(|&x| &x == &now_weekday_jp)
                        .unwrap();
                    let target_days_of_week_ind =
                        days_of_week.iter().position(|&x| x == tokens[1]).unwrap();

                    let ind_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                    // 今日の6:00にdeferする味意はないので、その代わりに、1週間後の同じ曜日にdeferできるようにする
                    let days: i64 = if ind_diff == 0 { 7 } else { ind_diff as i64 };

                    let seconds = (get_next_morning_datetime(now) + Duration::days(days - 1) - now)
                        .num_seconds()
                        + 1;

                    if seconds > 0 {
                        execute_defer(
                            task_repository,
                            focused_task_id_opt,
                            &focused_task_opt,
                            &format!("{}", seconds),
                            "秒",
                        );
                    }
                } else {
                    // "defer 5days" のように引数が1つしか与えられなかった場合は、数字部分とそれ以降に分割する
                    let splitted = split_amount_and_unit(tokens[1]);
                    if splitted.len() == 2 && splitted[0] != "" {
                        let amount_str = &splitted[0];
                        let unit_str = &splitted[1].to_lowercase();

                        execute_defer(
                            task_repository,
                            focused_task_id_opt,
                            &focused_task_opt,
                            amount_str,
                            unit_str,
                        );
                    }
                }
            }
        }
        "逃" | "escape" | "esc" => {
            // 先延ばしにしてしまう時。要求している見積もりが小さすぎる可能性があるので、2倍にする
            if let Some(focused_task) = focused_task_opt {
                let estimated_work_seconds = focused_task.get_estimated_work_seconds();
                focused_task.set_estimated_work_seconds(estimated_work_seconds * 2);

                // 引数が与えられた時はそのままdeferする
                if tokens.len() >= 2 {
                    let s = format!("後 {}", tokens[1..].join(" "));

                    execute(
                        stdout,
                        task_repository,
                        free_time_manager,
                        focused_task_id_opt,
                        &focus_started_datetime,
                        &s,
                    );
                }
            }
        }
        "平" | "flatten" | "flat" => {
            for _ in 0..7 {
                let pattern_opt = Some("平".to_string());
                execute_show_all_tasks(
                    stdout,
                    focused_task_id_opt,
                    task_repository,
                    free_time_manager,
                    &pattern_opt,
                );
            }
        }
        "押" | "extrude" => {
            if tokens.len() >= 2 {
                if let Some(ref focused_task) = focused_task_opt {
                    let first_datetime =
                        focused_task.list_all_parent_tasks_with_first_available_time()[0].0;
                    let step_days: u16 = tokens[1].parse().unwrap_or(1);

                    execute_extrude(
                        focused_task_id_opt,
                        &focused_task_opt,
                        &first_datetime,
                        step_days,
                    );
                }
            }
        }
        "空" | "clear" | "集" | "gather" => {
            // 空 13:00
            // 今着手可能なタスクについてactiveなものを、指定したタイミングまでpendingする

            // 集 13:00
            // 指定したタイミングまでに着手する予定のタスクを全てTodoに直す
            let hhmm_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})$").unwrap();
            if tokens.len() >= 2 && hhmm_reg.is_match(tokens[1]) {
                let cmd_str = tokens[0];
                let hhmm_str = tokens[1];

                let now: DateTime<Local> = task_repository.get_last_synced_time();

                let caps = hhmm_reg.captures(hhmm_str).unwrap();
                let hh_orig: u32 = caps[1].parse().unwrap();
                let hh = hh_orig % 24;
                let mm: u32 = caps[2].parse().unwrap();
                let days: i64 = hh_orig as i64 / 24;

                for project_root_task in task_repository.get_all_projects().iter() {
                    let leaf_tasks =
                        extract_leaf_tasks_from_project_with_pending(&project_root_task);
                    let todays_start = get_next_morning_datetime(now) - Duration::days(1);
                    let defer_to_datetime = Local
                        .with_ymd_and_hms(
                            todays_start.year(),
                            todays_start.month(),
                            todays_start.day(),
                            hh,
                            mm,
                            0,
                        )
                        .unwrap()
                        + Duration::days(days);
                    for leaf_task in leaf_tasks.iter() {
                        match cmd_str {
                            "空" | "clear" => {
                                if leaf_task.get_start_time() < defer_to_datetime
                                    && (leaf_task.get_orig_status() == Status::Todo
                                        || (leaf_task.get_orig_status() == Status::Pending
                                            && leaf_task.get_pending_until() < defer_to_datetime))
                                {
                                    leaf_task.set_orig_status(Status::Pending);
                                    leaf_task.set_pending_until(defer_to_datetime);
                                }
                            }
                            "集" | "gather" => {
                                if leaf_task.get_status() == Status::Pending
                                    && leaf_task.get_start_time() < defer_to_datetime
                                    && leaf_task.get_pending_until() < defer_to_datetime
                                {
                                    leaf_task.set_orig_status(Status::Todo);
                                }
                            }
                            _ => {
                                // Skip
                            }
                        }
                    }
                }
            }
        }
        "終" | "finish" | "fin" => {
            // 現在のフォーカス時間を実作業時間に追加する
            // 基本的にはそれを自動で行うが、もし引数を追加した時には発動させないようにする
            if tokens.len() == 1 {
                if let Some(ref focused_task) = focused_task_opt {
                    let past_actual_work_seconds = focused_task.get_actual_work_seconds();

                    let now_focus_duration_seconds = (task_repository.get_last_synced_time()
                        - *focus_started_datetime)
                        .num_seconds();
                    focused_task.set_actual_work_seconds(
                        past_actual_work_seconds
                            + if now_focus_duration_seconds >= 60 {
                                now_focus_duration_seconds
                            } else {
                                0
                            },
                    );
                }
            }

            // 完了操作
            execute_finish(focused_task_id_opt, &focused_task_opt);
        }
        "" => {}
        &_ => {
            // 何も該当するコマンドが無い場合には「全」コマンドとして実行する
            // ただし、最初が数字の0から始まる場合は無視する
            // show_all_commandの結果をコピーしたものを誤って貼り付けた場合に迅速に停止させるため。
            // 精緻に書こうと思えば条件を変えられる。

            if let Some(first_char) = untrimmed_line.chars().next() {
                if first_char != '0' {
                    let cmd_of_show_all = String::from("全 ") + untrimmed_line;

                    execute(
                        stdout,
                        task_repository,
                        free_time_manager,
                        focused_task_id_opt,
                        focus_started_datetime,
                        &cmd_of_show_all,
                    );
                }
            }
        }
    }

    stdout.flush().unwrap();
}

// 削除できない時はNoneを返す。例えば、文字列が空の時
fn get_byte_offset_for_deletion(line: &str, cursor_x: usize) -> Option<usize> {
    let byte_offset_opt = if line.is_empty() || cursor_x == 0 {
        None
    } else {
        let char_indices_vec = line.char_indices().collect::<Vec<_>>();

        Some(char_indices_vec[cursor_x - 1].0)
    };

    return byte_offset_opt;
}

#[test]
fn get_byte_offset_for_deletion_noneを返す場合() {
    let line = "あ";
    let cursor_x = 0;
    let actual = get_byte_offset_for_deletion(&line, cursor_x);
    let expected = None;
    assert_eq!(actual, expected);
}

#[test]
fn get_byte_offset_for_deletion_正常系() {
    let line = "あ";
    let cursor_x = 1;
    let actual = get_byte_offset_for_deletion(&line, cursor_x);
    let expected = Some(0);
    assert_eq!(actual, expected);
}

fn main() {
    let mut task_repository = TaskRepository::new("../Schronu-private/tasks/");
    let mut free_time_manager = FreeTimeManager::new();

    // 複数プロセスで同時に実行すると片方の操作がもう片方の操作により上書かれてしまうので、
    // ロックファイルを置いて制御する
    let lock_path: &str = &format!("{}/.lock", task_repository.get_project_storage_dir_name());

    // ロックファイルを開く。なければ作成する。
    let file = File::create(lock_path).expect("Unable to create lock file");

    // 排他ロックを試みる。
    match file.try_lock_exclusive() {
        Ok(_) => {
            // ロック取得成功。アプリケーションのメインロジックを実行。

            // controllerで実体を見るのを避けるために、1つ関数を切る
            application(&mut task_repository, &mut free_time_manager);

            // 終了時にロックは自動的に解放される。
        }
        Err(_) => {
            // ロック取得失敗。すでに別のインスタンスが実行中。
            eprintln!("[Error] Another instance of the application is already running.");
        }
    }
}

fn make_message_about_focus(
    focused_task: &Task,
    focus_started_datetime: &DateTime<Local>,
    now: &DateTime<Local>,
) -> String {
    let estimated_finish_datetime = *focus_started_datetime
        + Duration::seconds(
            focused_task.get_estimated_work_seconds() - focused_task.get_actual_work_seconds(),
        );

    let left_duration = estimated_finish_datetime - *now;
    let for_duration = *now - *focus_started_datetime;

    let msg = format!(
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
        for_duration.num_minutes() + 1,
    );

    msg
}

fn application(
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) {
    // 時計を合わせる
    let now = Local::now();
    task_repository.sync_clock(now);

    // let next_morning = get_next_morning_datetime(now)
    //     .with_hour(6)
    //     .expect("invalid hour")
    //     .with_minute(0)
    //     .expect("invalid minute");
    // task_repository.sync_clock(next_morning);

    task_repository.load();

    free_time_manager
        .load_busy_time_slots_from_file("../Schronu-private/busy_time_slots.yaml", &now);

    // RawModeを有効にする
    let mut stdout = stdout().into_raw_mode().unwrap();

    write!(stdout, "{}", termion::clear::All).unwrap();
    write!(stdout, "{}", termion::cursor::BlinkingBar).unwrap();
    stdout.flush().unwrap();

    // 起動直後はrhoの値を見たいので葉は出力しない
    // execute_show_leaf_tasks(&mut stdout, task_repository, free_time_manager);

    // 優先度の最も高いPJを一つ選ぶ
    // 一番下のタスクにフォーカスが自動的に当たる

    let mut focused_task_id_opt: Option<Uuid> = if IS_LOW_PRIORITY_MODE {
        task_repository.get_lowest_priority_leaf_task_id()
    } else {
        task_repository.get_highest_priority_leaf_task_id()
    };

    let mut last_focused_task_id_opt: Option<Uuid> = None;
    let mut focus_started_datetime: DateTime<Local> = now;

    ///////////////////////

    // 最初に、今後の忙しさ具合を表示する
    execute_show_all_tasks(
        &mut stdout,
        &mut focused_task_id_opt,
        task_repository,
        free_time_manager,
        &Some("暦".to_string()),
    );

    ///////////////////////

    // この処理、よく使いそう
    match focused_task_id_opt {
        Some(focused_task_id) => {
            let focused_task_opt = task_repository.get_by_id(focused_task_id);

            // 以前とフォーカスしているタスクが変わった場合には、タスクの実作業時間の記録をリセットする
            if focused_task_id_opt != last_focused_task_id_opt {
                focus_started_datetime = Local::now();
                last_focused_task_id_opt = focused_task_id_opt;
            }

            execute_show_ancestor(&mut stdout, &focused_task_opt);

            match focused_task_opt {
                Some(focused_task) => {
                    println!("{}focused task is:", termion::cursor::Left(MAX_COL));
                    println!(
                        "{}{:?}",
                        termion::cursor::Left(MAX_COL),
                        focused_task.get_attr()
                    );
                    stdout.flush().unwrap();

                    let msg = make_message_about_focus(
                        &focused_task,
                        &focus_started_datetime,
                        &Local::now(),
                    );
                    writeln_newline(&mut stdout, &msg).unwrap();
                }
                None => {}
            }
        }
        None => {}
    }

    ///////////////////////

    let header: &str = "schronu> ";
    let mut line = String::from("");

    // 画面に表示されている「文字」単位でのカーソル。
    let mut cursor_x: usize = 0;

    write!(stdout, "{}{}", termion::cursor::Left(MAX_COL), header).unwrap();
    stdout.flush().unwrap();

    // キー入力を受け付ける
    for c in std::io::stdin().keys() {
        match c.unwrap() {
            Key::Ctrl('d') => {
                if line.is_empty() {
                    // 最後に、今後の忙しさ具合を表示する
                    let now = Local::now();
                    task_repository.sync_clock(now);

                    execute_show_all_tasks(
                        &mut stdout,
                        &mut focused_task_id_opt,
                        task_repository,
                        free_time_manager,
                        &Some("暦".to_string()),
                    );

                    break;
                }
            }
            Key::Ctrl('c') => {
                // 保存などせずに強制終了する
                process::exit(1);
            }
            // Key::Up => write!(stdout, "{}", termion::cursor::Up(1)).unwrap(),
            // Key::Down => write!(stdout, "{}", termion::cursor::Down(1)).unwrap(),
            Key::Left | Key::Ctrl('b') => {
                let width = backward_width(&line, cursor_x);

                if width > 0 {
                    cursor_x -= 1;
                    write!(stdout, "{}", termion::cursor::Left(width)).unwrap();
                    stdout.flush().unwrap();
                }
            }
            Key::Right | Key::Ctrl('f') => {
                let width = get_forward_width(&line, cursor_x);

                if width > 0 {
                    cursor_x += 1;
                    write!(stdout, "{}", termion::cursor::Right(width)).unwrap();
                    stdout.flush().unwrap();
                }
            }
            Key::Ctrl('a') => {
                cursor_x = 0;

                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::clear::CurrentLine
                )
                .unwrap();

                let width = get_width_for_rerender(&header, &line, cursor_x);
                write!(stdout, "{}{}", header, line).unwrap();
                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::cursor::Right(width)
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            Key::Ctrl('e') => {
                loop {
                    let width = get_forward_width(&line, cursor_x);

                    if width == 0 {
                        break;
                    }
                    cursor_x += 1;
                    write!(stdout, "{}", termion::cursor::Right(width)).unwrap();
                }
                stdout.flush().unwrap();
            }
            Key::Ctrl('u') => {
                cursor_x = 0;
                line.clear();

                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::clear::CurrentLine,
                )
                .unwrap();

                let width = get_width_for_rerender(&header, &line, cursor_x);
                write!(stdout, "{}{}", header, line).unwrap();
                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::cursor::Right(width)
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            Key::Ctrl('k') => {
                // カーソルの位置を変えずに後ろをカットする
                line = line.chars().take(cursor_x).collect();

                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::clear::CurrentLine,
                )
                .unwrap();

                let width = get_width_for_rerender(&header, &line, cursor_x);
                write!(stdout, "{}{}", header, line).unwrap();
                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::cursor::Right(width)
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            Key::Backspace | Key::Ctrl('h') => {
                let byte_offset_opt = get_byte_offset_for_deletion(&line, cursor_x);
                match byte_offset_opt {
                    Some(byte_offset) => {
                        line.remove(byte_offset);
                        cursor_x -= 1;
                    }
                    None => {}
                }

                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::clear::CurrentLine,
                )
                .unwrap();

                let width = get_width_for_rerender(&header, &line, cursor_x);
                write!(stdout, "{}{}", header, line).unwrap();
                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::cursor::Right(width)
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            Key::Char('\n') | Key::Ctrl('m') => {
                // 時計を合わせる
                task_repository.sync_clock(Local::now());

                line = line.trim().to_string();

                writeln_newline(&mut stdout, "").unwrap();

                println!(
                    "{}{}> {}{}",
                    style::Bold,
                    &Local::now().format("%Y/%m/%d %H:%M:%S.%f").to_string(),
                    line,
                    style::Reset
                );
                writeln_newline(&mut stdout, "").unwrap();
                stdout.flush().unwrap();

                if line == "t" {
                    // do it "t"oday
                    let s = "後 1秒".to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s,
                    );
                } else if line == "h" {
                    // skip an "h"our
                    let s = "後 1時間".to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s,
                    );
                } else if line == "d" {
                    // skip "d"aily
                    let now: DateTime<Local> = task_repository.get_last_synced_time();
                    let next_morning = get_next_morning_datetime(now);
                    let sec = (next_morning - now).num_seconds() + 1;
                    let s = format!("後 {}秒", sec).to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s,
                    );
                } else if line == "D" {
                    // skip "D"aily (24h)
                    let sec = 24 * 60 * 60;
                    let s = format!("後 {}秒", sec).to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s,
                    );
                } else if line == "w" {
                    // skip "w"eekly
                    let now: DateTime<Local> = task_repository.get_last_synced_time();
                    let next_morning = get_next_morning_datetime(now);
                    let sec = (next_morning - now).num_seconds() + 86400 * 6 + 1;

                    let s = format!("後 {}秒", sec).to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s,
                    );
                } else if line == "W" {
                    // 〆切をrepetition_interval_daysのぶん伸ばし、pendingにする
                    // start_timeも伸ばすが、時刻は元のstart_timeを維持する
                    if let Some(focused_task_id) = focused_task_id_opt {
                        if let Some(ref focused_task) = task_repository.get_by_id(focused_task_id) {
                            if let Some(orig_deadline_time) = focused_task.get_deadline_time_opt() {
                                if let Some(parent_task) = focused_task.parent() {
                                    if let Some(repetition_interval_days) =
                                        parent_task.get_repetition_interval_days_opt()
                                    {
                                        let new_deadline_time = if let Some(parent_deadline_time) =
                                            parent_task.get_deadline_time_opt()
                                        {
                                            (get_next_morning_datetime(orig_deadline_time)
                                                + Duration::days(repetition_interval_days - 1))
                                            .with_hour(parent_deadline_time.hour())
                                            .expect("invalid hour")
                                            .with_minute(parent_deadline_time.minute())
                                            .expect("invalid minute")
                                            .with_second(parent_deadline_time.second())
                                            .expect("invalid second")
                                        } else {
                                            orig_deadline_time
                                                + Duration::days(repetition_interval_days)
                                        };

                                        focused_task.unset_deadline_time_opt();
                                        focused_task.set_deadline_time_opt(Some(new_deadline_time));

                                        focused_task.set_orig_status(Status::Todo);

                                        // 〆切の日に合わせる
                                        let new_start_time = focused_task.get_start_time()
                                            + Duration::days(
                                                (new_deadline_time - orig_deadline_time).num_days(),
                                            );

                                        focused_task.set_start_time(new_start_time);

                                        focused_task_id_opt = None;
                                    }
                                }
                            }
                        }
                    }
                } else if line == "y" {
                    // skip "y"early
                    let now: DateTime<Local> = task_repository.get_last_synced_time();
                    let next_morning = get_next_morning_datetime(now);
                    let sec = (next_morning - now).num_seconds() + 86400 * (7 * 52 * 5 - 1) + 1;

                    let s = format!("後 {}秒", sec).to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s,
                    );
                } else {
                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &line,
                    );
                }

                // 時計を合わせる
                task_repository.sync_clock(Local::now());

                //////////////////////////////

                // もしfocused_task_id_optがNoneの時は最も優先度が高いタスクの選出をやり直す

                if focused_task_id_opt.is_none() {
                    focused_task_id_opt = if IS_LOW_PRIORITY_MODE {
                        task_repository.get_lowest_priority_leaf_task_id()
                    } else {
                        task_repository.get_highest_priority_leaf_task_id()
                    };
                    last_focused_task_id_opt = None;
                }

                //////////////////////////////

                // スクロールするのが面倒なので、新や突のように付加情報を表示するコマンドの直後は葉を表示しない
                // Todo: "new" や  "unplanned" の場合にも対応する
                let fst_char_opt = line.chars().nth(0);
                if fst_char_opt != Some('新')
                    && fst_char_opt != Some('突')
                    && fst_char_opt != Some('全')
                    && fst_char_opt != Some('今')
                    && fst_char_opt != Some('明')
                    && fst_char_opt != Some('週')
                    && fst_char_opt != Some('末')
                    && fst_char_opt != Some('翌')
                    && fst_char_opt != Some('暦')
                    && fst_char_opt != Some('平')
                    && fst_char_opt != Some('葉')
                    && fst_char_opt != Some('樹')
                {
                    execute_show_leaf_tasks(&mut stdout, task_repository, free_time_manager);
                }

                match focused_task_id_opt {
                    Some(focused_task_id) => {
                        let focused_task_opt = task_repository.get_by_id(focused_task_id);

                        // 以前とフォーカスしているタスクが変わった場合には、タスクの実作業時間の記録をリセットする
                        if focused_task_id_opt != last_focused_task_id_opt {
                            focus_started_datetime = Local::now();
                            last_focused_task_id_opt = focused_task_id_opt;
                        }

                        execute_show_ancestor(&mut stdout, &focused_task_opt);

                        // フォーカスしているタスクを表示
                        match focused_task_opt {
                            Some(focused_task) => {
                                println!("{}focused task is:", termion::cursor::Left(MAX_COL));
                                println!(
                                    "{}{:?}",
                                    termion::cursor::Left(MAX_COL),
                                    focused_task.get_attr()
                                );
                                stdout.flush().unwrap();
                                let msg = make_message_about_focus(
                                    &focused_task,
                                    &focus_started_datetime,
                                    &Local::now(),
                                );
                                writeln_newline(&mut stdout, &msg).unwrap();
                            }
                            None => {}
                        }
                    }
                    None => {}
                }

                //////////////////////////////

                // 初期化
                cursor_x = 0;
                line.clear();

                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::clear::CurrentLine,
                )
                .unwrap();

                let width = get_width_for_rerender(&header, &line, cursor_x);
                write!(stdout, "{}{}", header, line).unwrap();
                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::cursor::Right(width)
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            Key::Char(c) => {
                // 多バイト文字の挿入位置を知る
                let byte_offset = get_byte_offset_for_insert(&line, cursor_x);
                line.insert(byte_offset, c);

                cursor_x += 1;
                write!(stdout, "{}", c).unwrap();
                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::clear::CurrentLine
                )
                .unwrap();

                let width = get_width_for_rerender(&header, &line, cursor_x);
                write!(stdout, "{}{}", header, line).unwrap();
                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::cursor::Right(width)
                )
                .unwrap();
                stdout.flush().unwrap();
            }
            _key => {
                // write!(stdout, "{:?}", x).unwrap();
                // stdout.flush().unwrap();

                // キー入力をリアルタイムで反映させる
                // write!(stdout, "{}", termion::clear::CurrentLine).unwrap();
                // write!(stdout, "{}", termion::cursor::Left(999)).unwrap();
                // stdout.flush().unwrap();
                // write!(stdout, "{}", line).unwrap();
                // stdout.flush().unwrap();
            }
        }
    }

    write!(stdout, "{}", termion::clear::CurrentLine).unwrap();
    println!("{}{}{}", style::Bold, line, style::Reset);

    // 保存して終わり
    task_repository.save();

    // SteadyBlockに戻す
    // Todo: 本当は、元々の状態を保存しておいてそれに戻したい。
    writeln!(stdout, "{}", termion::cursor::SteadyBlock).unwrap();
}
