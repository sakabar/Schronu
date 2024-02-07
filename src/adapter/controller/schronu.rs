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
    extract_leaf_tasks_from_project, extract_leaf_tasks_from_project_with_pending, Status, Task,
    TaskAttr,
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
    let mut task_cnt = 1;
    let mut _total_estimated_work_seconds = 0;
    for project_root_task in task_repository.get_all_projects().iter() {
        let project_name = project_root_task.get_name();

        // 優先度が高いタスクほど下に表示されるようにし、フォーカスが当たるタスクは末尾に表示されるようにする。
        let mut leaf_tasks = extract_leaf_tasks_from_project(&project_root_task);
        leaf_tasks.reverse();

        for leaf_task in leaf_tasks.iter() {
            let message = format!("{}\t{}\t{:?}", task_cnt, project_name, leaf_task.get_attr());
            writeln_newline(stdout, &message).unwrap();
            task_cnt += 1;

            let estimated_work_seconds = leaf_task.get_estimated_work_seconds();
            _total_estimated_work_seconds += estimated_work_seconds;
        }
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

    let mut dt_id_tpl_arr: Vec<(DateTime<Local>, i64, usize, Option<DateTime<Local>>, Uuid)> =
        vec![];
    for (id, (dt, neg_priority, rank, deadline_time_opt)) in &id_to_dt_map {
        let tpl = (*dt, *neg_priority, *rank, *deadline_time_opt, *id);
        dt_id_tpl_arr.push(tpl);
    }

    // dt,rank等、タプルの各要素の小さい順にソート。後で逆順に変える
    dt_id_tpl_arr.sort();

    let mut msgs_with_dt: Vec<(DateTime<Local>, usize, Uuid, String)> = vec![];

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

    // タスク一覧で、どのタスクをいつやる見込みかを表示するために、「現在時刻」をズラして見ていく
    let mut current_datetime_cursor = task_repository.get_last_synced_time();

    for (ind, (dt, _neg_priority, rank, deadline_time_opt, id)) in dt_id_tpl_arr.iter().enumerate()
    {
        let subjective_naive_date =
            (get_next_morning_datetime(*dt) - Duration::days(1)).date_naive();
        counter
            .entry(subjective_naive_date)
            .and_modify(|cnt| *cnt += 1)
            .or_insert(1);

        let task_opt = task_repository.get_by_id(*id);
        match task_opt {
            Some(task) => {
                let name = task.get_name();
                let chars_vec: Vec<char> = name.chars().collect();
                let max_len: usize = 13;
                let shorten_name: String = if chars_vec.len() >= max_len {
                    format!("{}...", chars_vec.iter().take(max_len).collect::<String>())
                } else {
                    name.to_string()
                };

                // 元々見積もり時間から作業済時間を引いたのが残りの見積もり時間
                // ただし、作業時間が元々の見積もり時間をオーバーしている時には既に想定外の事態になっているため、
                // 残りの見積もりを0とはせず、安全に倒して元々の見積もりをそのまま使用する
                let estimated_work_seconds =
                    if task.get_estimated_work_seconds() >= task.get_actual_work_seconds() {
                        task.get_estimated_work_seconds() - task.get_actual_work_seconds()
                    } else {
                        task.get_estimated_work_seconds()
                    };
                total_estimated_work_seconds += estimated_work_seconds;

                if let Some(deadline_time) = deadline_time_opt {
                    if subjective_naive_date
                        == (get_next_morning_datetime(*deadline_time) - Duration::days(1))
                            .date_naive()
                    {
                        deadline_estimated_work_seconds_map
                            .entry(subjective_naive_date)
                            .and_modify(|deadline_estimated_work_seconds| {
                                *deadline_estimated_work_seconds += estimated_work_seconds
                            })
                            .or_insert(estimated_work_seconds);
                    }
                }

                // 着手時間は、現在時刻か、最速着手可能時間のうち遅い方
                let current_datetime_cursor_clone = &current_datetime_cursor.clone();
                let start_datetime = max(dt, current_datetime_cursor_clone);
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

                let deadline_string = match deadline_time_opt {
                    Some(d) => d.format("%Y/%m/%d").to_string(),
                    None => "____/__/__".to_string(),
                };

                let deadline_icon: String = "!".to_string();
                let today_leaf_icon: String = "/".to_string();
                // Todo: この判定が分散しているので、後で関数化したほうがよいかも
                let icon = if task.get_deadline_time_opt().is_some()
                    && task.get_deadline_time_opt().unwrap()
                        < get_next_morning_datetime(last_synced_time)
                {
                    &deadline_icon
                } else if rank == &0 && dt < &eod {
                    &today_leaf_icon
                } else {
                    "-"
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
                    estimated_work_seconds as f64 / 60.0,
                    total_estimated_work_hours,
                    shorten_name
                );

                let yyyymmdd_reg = Regex::new(r"^(\d{4})/(\d{2})/(\d{2})$").unwrap();
                let days_of_week = vec!["月", "火", "水", "木", "金", "土", "日"];

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
                                || msg.contains(&format!(" {} ", &today_leaf_icon))
                            {
                                msgs_with_dt.push((*dt, *rank, *id, msg));
                            }
                        } else if pattern == "〆" {
                            if msg.contains(&format!(" {} ", &deadline_icon)) {
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

    // 未来のサマリは見ても仕方ないので、直近の8日ぶん(配列の末尾)に絞る
    const SUMMARY_DAYS: usize = 8;

    let mut daily_stat_msgs: Vec<String> = vec![];

    // 順調フラグ
    let mut has_today_deadline_leeway = true;
    let mut has_today_freetime_leeway = true;
    let mut has_today_new_task_leeway = true;
    let mut has_tomorrow_deadline_leeway = true;
    let mut has_tomorrow_freetime_leeway = true;
    let mut has_weekly_freetime_leeway = true;

    // 「それぞれの日の rho (0.7) との差」の累積和。
    // どれくらい突発を吸収できるかの指標となる。
    // 元々は単に0.7との差で計算していたが、それだと0.7<rho<1.0でその日のタスクがなんとかなっているのに
    // 0.7との差の累積和が肥大化して使いものにならなかったため、以下の定義で計算するようにした。
    // ただし、特定の日にタスクを寄せて無理矢理rho<0.7の日を作るほうが良く見えてしまうので注意が必要。
    // rho < 0.7 : 累積和はそのぶん減る
    // 0.7<= rho <=1.0 : ノーカウント。その日のうちに吸収できる
    // 1.0 < rho : 累積和はそのぶん増える
    let mut accumurate_duration_diff_to_goal_rho = Duration::minutes(0);

    // 「それぞれの日の自由時間との差」の累積和
    let mut accumurate_duration_diff_to_limit = Duration::minutes(0);

    // 平坦化可能ポイント
    let mut flattenable_date_opt: Option<NaiveDate> = None;
    let mut overload_day_is_found = false;
    let mut flattenable_duration = Duration::seconds(0);

    for (date, _cnt) in &counter_arr[0..SUMMARY_DAYS] {
        let total_estimated_work_seconds_of_the_date: i64 =
            *total_estimated_work_seconds_of_the_date_counter
                .get(date)
                .unwrap_or(&0);
        let total_estimated_work_hours_of_the_date =
            total_estimated_work_seconds_of_the_date as f64 / 3600.0;

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

        const RHO_GOAL: f64 = 0.7;

        let diff_to_goal = total_estimated_work_hours_of_the_date - free_time_hours * RHO_GOAL;
        let diff_to_goal_sign: char = if diff_to_goal > 0.0 { ' ' } else { '-' };
        let diff_to_goal_hour = diff_to_goal.abs().floor();
        let diff_to_goal_minute = (diff_to_goal.abs() - diff_to_goal_hour) * 60.0;

        let over_time_hours_f = total_estimated_work_hours_of_the_date - free_time_hours;
        let over_time_hours = over_time_hours_f.abs().floor() as i64;
        let over_time_minutes = (over_time_hours_f.abs() * 60.0) as i64 % 60;

        accumurate_duration_diff_to_limit = if over_time_hours_f > 0.0 {
            accumurate_duration_diff_to_limit
                + Duration::hours(over_time_hours)
                + Duration::minutes(over_time_minutes)
        } else {
            accumurate_duration_diff_to_limit
                - Duration::hours(over_time_hours)
                - Duration::minutes(over_time_minutes)
        };

        if !overload_day_is_found && accumurate_duration_diff_to_limit > Duration::seconds(0) {
            overload_day_is_found = true;
        } else if accumurate_duration_diff_to_limit <= Duration::seconds(300) {
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

        let diff_to_limit_sign: char = if accumurate_duration_diff_to_limit > Duration::minutes(0) {
            ' '
        } else {
            '-'
        };

        accumurate_duration_diff_to_goal_rho = if diff_to_goal >= 0.0 && rho_in_date >= 1.0 {
            accumurate_duration_diff_to_goal_rho
                + Duration::hours(over_time_hours)
                + Duration::minutes(over_time_minutes)
        } else if rho_in_date < RHO_GOAL {
            accumurate_duration_diff_to_goal_rho
                - Duration::hours(diff_to_goal_hour as i64)
                - Duration::minutes(diff_to_goal_minute as i64)
        } else {
            accumurate_duration_diff_to_goal_rho
        };

        let acc_diff_to_goal_sign: char =
            if accumurate_duration_diff_to_goal_rho > Duration::minutes(0) {
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

        let accumulated_rho_diff =
            accumurate_duration_diff_to_limit.num_minutes() as f64 / 60.0 / free_time_hours;

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

        let indicator_about_diff_to_limit = format!(
            "{}{:02}時間{:02}分\t{:5.2}",
            diff_to_limit_sign,
            accumurate_duration_diff_to_limit.num_hours().abs(),
            accumurate_duration_diff_to_limit.num_minutes().abs() % 60,
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
        if daily_stat_msgs.len() < 7 && has_weekly_freetime_leeway {
            has_weekly_freetime_leeway = diff_to_limit_sign == '-';
        }

        let s = format!(
            "{}({})\t{:4.1}時間\t{}{:.0}時間{:02.0}分\t{:5.2}\t{}{:.0}時間{:02.0}分\t{}{:02}時間{:02}分\t{}\t{}\t{:02}[タスク]",
            date,
            weekday_jp,

            free_time_hours,

            diff_to_limit_in_day_sign,
            diff_to_limit_hours_in_day,
            diff_to_limit_minutes_in_day,

            rho_in_date - 1.0,

            diff_to_goal_sign,
            diff_to_goal_hour,
            diff_to_goal_minute,

            acc_diff_to_goal_sign,
            accumurate_duration_diff_to_goal_rho.num_hours().abs(),
            accumurate_duration_diff_to_goal_rho.num_minutes().abs() % 60,

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
            "空差累 ",
            "空差累比",
            "タスク数",
        ]
        .join("\t");
        writeln_newline(stdout, &footer).unwrap();
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
    let s_for_rho1 = match lq1_opt {
        Some(lq1) => {
            format!(
                "ρ_1 = ({:.1} + 0.0) / ({:.1} + 0.0) = {:.2}, Lq = {:.1}",
                today_total_deadline_estimated_work_hours,
                mu_hours - busy_hours,
                rho1,
                lq1
            )
        }
        None => {
            format!(
                "ρ_1 = ({:.1} + 0.0) / ({:.1} + 0.0) = {:.2}, Lq = inf",
                today_total_deadline_estimated_work_hours,
                mu_hours - busy_hours,
                rho1
            )
        }
    };

    if !is_flatten_func {
        writeln_newline(stdout, &busy_s).unwrap();
        writeln_newline(stdout, &s).unwrap();
        writeln_newline(stdout, &s_for_rho1).unwrap();
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

            while !any_was_flattened && src_date > naive_dt_today {
                writeln_newline(stdout, &format!("src_date: {:?}", src_date)).unwrap();

                // dt_dictを未来から見ていき、〆切に違反しない範囲で、翌日に飛ばしていく
                for (_ind, (dt, _neg_priority, rank, deadline_time_opt, id)) in
                    dt_id_tpl_arr.iter().enumerate().rev()
                {
                    let days_until_deadline = match deadline_time_opt {
                        Some(deadline_time) => (*deadline_time - *dt).num_days(),
                        None => 100,
                    };

                    if dt.date_naive() == src_date && days_until_deadline > 0 {
                        if let Some(task) = task_repository.get_by_id(*id) {
                            if !task.get_is_on_other_side()
                                && rank != &0
                                && task.get_estimated_work_seconds() > 0
                                && flattenable_duration.num_seconds()
                                    > task.get_estimated_work_seconds()
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
        // deadline_timeはその日付の23:59とする
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
                        } else if diff <= -120 {
                            // 予定より2分以内早いというズレは誤差の範囲とする
                            // 見積もりは最短でも1分になるようにする
                            // 人は見積もりを過小評価しがちなので、見積もりをさらに小さくする方向については慎重に。25%採用する
                            let new_estimated_work_seconds = max(60, orig_estimated_sec + diff / 4);
                            parent_task.set_estimated_work_seconds(new_estimated_work_seconds);
                        }
                    }

                    let parent_task_name = parent_task.get_name();
                    let parent_task_start_time = parent_task.get_start_time();
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
                    let new_deadline_time = new_start_time
                        .with_hour(23)
                        .unwrap()
                        .with_minute(59)
                        .unwrap()
                        .with_second(59)
                        .unwrap();
                    let estimated_work_seconds = parent_task.get_estimated_work_seconds();

                    let mut new_task_attr = TaskAttr::new(&new_task_name);
                    new_task_attr.set_start_time(new_start_time);
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

fn execute_set_deadline(focused_task_opt: &Option<Task>, deadline_date_str: &str) {
    if deadline_date_str == "消" {
        focused_task_opt
            .as_ref()
            .map(|focused_task| focused_task.unset_deadline_time_opt());
        return;
    }

    let deadline_time_str = format!("{} 23:59:59", deadline_date_str);
    let deadline_time_opt = Local
        .datetime_from_str(&deadline_time_str, "%Y/%m/%d %H:%M:%S")
        .ok();

    if deadline_time_opt.is_some() {
        focused_task_opt
            .as_ref()
            .map(|focused_task| focused_task.set_deadline_time_opt(deadline_time_opt));
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
        "約" | "appointment" => {
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

                let start_time = if yyyymmdd_reg.is_match(start_date_str) {
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
                    let now: DateTime<Local> = task_repository.get_last_synced_time();

                    let caps = mmdd_reg.captures(start_date_str).unwrap();
                    let mm_month: u32 = caps[1].parse().unwrap();
                    let dd: u32 = caps[2].parse().unwrap();

                    let mut ans_datetime = Local
                        .with_ymd_and_hms(now.year(), mm_month, dd, hh, mm, 0)
                        .unwrap();

                    if ans_datetime < now {
                        ans_datetime = Local
                            .with_ymd_and_hms(now.year() + 1, mm_month, dd, hh, mm, 0)
                            .unwrap()
                    }

                    ans_datetime
                } else {
                    let now = task_repository.get_last_synced_time();
                    Local
                        .with_ymd_and_hms(now.year(), now.month(), now.day(), hh, mm, 0)
                        .unwrap()
                };

                execute_make_appointment(&focused_task_opt, start_time);
            }
        }
        "木" | "tree" => {
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
        "子" | "children" | "ch" => {}
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
                let deadline_date_str = &tokens[1];

                let now: DateTime<Local> = task_repository.get_last_synced_time();
                if tokens[1].starts_with('今') {
                    let s = (get_next_morning_datetime(now) - Duration::days(1))
                        .format("%Y/%m/%d")
                        .to_string();
                    execute_set_deadline(&focused_task_opt, &s);
                } else if tokens[1].starts_with('明') {
                    let s = get_next_morning_datetime(now)
                        .format("%Y/%m/%d")
                        .to_string();
                    execute_set_deadline(&focused_task_opt, &s);
                } else {
                    execute_set_deadline(&focused_task_opt, deadline_date_str);
                }
            }
        }
        "予" | "estimate" | "es" => {
            if tokens.len() >= 2 {
                let estimated_work_minutes_str = &tokens[1];
                execute_set_estimated_work_minutes(&focused_task_opt, estimated_work_minutes_str);
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
                let hh: u32 = caps[1].parse().unwrap();
                let mm: u32 = caps[2].parse().unwrap();

                for project_root_task in task_repository.get_all_projects().iter() {
                    let leaf_tasks =
                        extract_leaf_tasks_from_project_with_pending(&project_root_task);
                    let defer_to_datetime = Local
                        .with_ymd_and_hms(now.year(), now.month(), now.day(), hh, mm, 0)
                        .unwrap();
                    for leaf_task in leaf_tasks.iter() {
                        match cmd_str {
                            "空" | "clear" => {
                                if leaf_task.get_status() == Status::Todo
                                    || (leaf_task.get_status() == Status::Pending
                                        && leaf_task.get_pending_until() < defer_to_datetime)
                                {
                                    leaf_task.set_orig_status(Status::Pending);
                                    leaf_task.set_pending_until(defer_to_datetime);
                                }
                            }
                            "集" | "gather" => {
                                if leaf_task.get_status() == Status::Pending
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
        &_ => {}
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
    let mut focused_task_id_opt: Option<Uuid> = task_repository.get_highest_priority_leaf_task_id();

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
                    let msg = format!(
                        "for {} minutes (since {} until {})",
                        (Local::now() - focus_started_datetime).num_minutes() + 1,
                        focus_started_datetime.format("%H:%M:%S"),
                        (focus_started_datetime
                            + Duration::seconds(max(
                                0,
                                focused_task.get_estimated_work_seconds()
                                    - focused_task.get_actual_work_seconds()
                            )))
                        .format("%H:%M:%S")
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
                    &Local::now().format("%Y/%m/%d %H:%M:%S").to_string(),
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
                    // defer deadline and skip "w"eekly
                    let s1 = "〆 消".to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s1,
                    );

                    let now: DateTime<Local> = task_repository.get_last_synced_time();
                    let next_morning = get_next_morning_datetime(now);
                    let sec = (next_morning - now).num_seconds() + 86400 * 6 + 1;

                    let new_deadline_yyyymmdd = (now + Duration::seconds(sec)).format("%Y/%m/%d");
                    let s3 = format!("〆 {}", new_deadline_yyyymmdd).to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s3,
                    );

                    let s2 = format!("後 {}秒", sec).to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &focus_started_datetime,
                        &s2,
                    );
                } else if line == "y" {
                    // skip "y"early
                    let now: DateTime<Local> = task_repository.get_last_synced_time();
                    let next_morning = get_next_morning_datetime(now);
                    let sec = (next_morning - now).num_seconds() + 86400 * 365 * 5 + 1;

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
                    focused_task_id_opt = task_repository.get_highest_priority_leaf_task_id();
                    last_focused_task_id_opt = None;
                }

                //////////////////////////////

                // スクロールするのが面倒なので、新や突のように付加情報を表示するコマンドの直後は葉を表示しない
                // Todo: "new" や  "unplanned" の場合にも対応する
                let fst_char_opt = line.chars().nth(0);
                if fst_char_opt != Some('新')
                    && fst_char_opt != Some('突')
                    && fst_char_opt != Some('全')
                    && fst_char_opt != Some('暦')
                    && fst_char_opt != Some('平')
                    && fst_char_opt != Some('葉')
                    && fst_char_opt != Some('木')
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
                                let msg = format!(
                                    "for {} minutes (since {} until {})",
                                    (Local::now() - focus_started_datetime).num_minutes() + 1,
                                    focus_started_datetime.format("%H:%M:%S"),
                                    (focus_started_datetime
                                        + Duration::seconds(max(
                                            0,
                                            focused_task.get_estimated_work_seconds()
                                                - focused_task.get_actual_work_seconds()
                                        )))
                                    .format("%H:%M:%S")
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
