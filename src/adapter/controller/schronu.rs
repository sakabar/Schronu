use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Timelike, Weekday};
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
use std::collections::HashMap;
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

// 単位時間は「1日」
const LAMBDA: f64 = 4.0;
const MU: f64 = 4.0;
const RHO: f64 = if LAMBDA <= MU { LAMBDA / MU } else { 1.0 };

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
    is_deferred: bool,
    estimated_work_minutes_opt: Option<i64>,
) {
    let root_task = Task::new(new_project_name_str);

    // 本来的には、TaskAttrのデフォルト値の方を5にすべきかも
    root_task.set_priority(5);

    if is_deferred {
        // 次回の午前6時
        let pending_until = get_next_morning_datetime(task_repository.get_last_synced_time());
        root_task.set_pending_until(pending_until);
        root_task.set_orig_status(Status::Pending);
    }

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

fn execute_show_ancestor(stdout: &mut RawTerminal<Stdout>, focused_task_opt: &Option<Task>) {
    writeln!(stdout, "").unwrap();

    let mut t_opt: Option<Task> = focused_task_opt.clone();

    // まずは葉タスクから根に向かいながら後ろに追加していき、
    // 最後に逆順にして表示する
    let mut ancestors: Vec<Task> = vec![];

    loop {
        match &t_opt {
            Some(t) => {
                ancestors.push(t.clone());
                t_opt = t.parent();
            }
            None => {
                break;
            }
        }
    }

    ancestors.reverse();

    for (level, task) in ancestors.iter().enumerate() {
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
        let msg = format!("{}{}\t{}m {}", &header, &id, &estimated_work_minutes, &name);
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

    // rhoや見込み完了時間を計算する処理はshow_all_tasks()に移したのでコメントアウト
    // let last_synced_time = task_repository.get_last_synced_time();

    // // タスクができない時間を決め打ちで登録する
    // let busy_time_slots = [((0, 0), (21, 0))];

    // for ((start_hour, start_minute), (end_hour, end_minute)) in busy_time_slots.iter() {
    //     free_time_manager.register_busy_time_slot(
    //         &last_synced_time
    //             .with_hour(*start_hour)
    //             .expect("invalid hour")
    //             .with_minute(*start_minute)
    //             .expect("invalid minute"),
    //         &last_synced_time
    //             .with_hour(*end_hour)
    //             .expect("invalid hour")
    //             .with_minute(*end_minute)
    //             .expect("invalid minute"),
    //     );
    // }

    // let eod = last_synced_time
    //     .with_hour(23)
    //     .expect("invalid hour")
    //     .with_minute(59)
    //     .expect("invalid minute");
    // let busy_minutes = free_time_manager.get_busy_minutes(&last_synced_time, &eod);

    // // コストを正確に算出できるようになるまでのつなぎとして、概算を表示する
    // // task_cntは「次に表示されるタスク番号」なので、マイナス1する
    // let minutes = (total_estimated_work_seconds as f64 / 60.0 / RHO).ceil() as i64 + busy_minutes;

    // let dt = last_synced_time + Duration::minutes(minutes);

    // let busy_hours = (busy_minutes as f64 / 60.0).ceil() as i64;
    // let busy_s = format!("残り拘束時間は{}時間です", busy_hours);
    // writeln_newline(stdout, &busy_s).unwrap();

    // let hours = (minutes as f64 / 60.0).ceil() as i64;
    // let s = format!("完了見込み日時は{}時間後の{}です", hours, dt);
    // writeln_newline(stdout, &s).unwrap();

    // let lq = LAMBDA / (MU - LAMBDA);
    // let s2 = format!("rho = {}, Lq = {:.1}", RHO, lq);
    // writeln_newline(stdout, &s2).unwrap();
    // writeln_newline(stdout, "").unwrap();
}

fn execute_show_all_tasks(
    stdout: &mut RawTerminal<Stdout>,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    pattern_opt: &Option<String>,
) {
    // Hash化できる要素しか入れられないので、いったんidだけ入れる
    // pending_until: DateTime<Local>,
    // rank: usize,
    // deadline_time_opt: Option<DateTime<Local>>,
    let mut id_to_dt_map: HashMap<Uuid, (DateTime<Local>, usize, Option<DateTime<Local>>)> =
        HashMap::new();

    let mut leaf_counter: HashMap<NaiveDate, usize> = HashMap::new();
    let mut total_leaf_estimated_work_minutes_of_the_date_counter: HashMap<NaiveDate, i64> =
        HashMap::new();

    // 複数の子タスクがある場合に、親タスクのdtは子の着手可能時期の中で最大の値となるようにする。
    // タプルの第2要素はrankで、葉(0)からの距離の大きい方
    for project_root_task in task_repository.get_all_projects().iter() {
        let leaf_tasks = extract_leaf_tasks_from_project_with_pending(&project_root_task);

        for leaf_task in leaf_tasks.iter() {
            let all_parent_tasks = leaf_task.list_all_parent_tasks_with_first_available_time();
            for (rank, (dt, task)) in all_parent_tasks.iter().enumerate() {
                let id = task.get_id();
                let deadline_time_opt = task.get_deadline_time_opt();

                if rank == 0 {
                    leaf_counter
                        .entry(dt.date_naive())
                        .and_modify(|cnt| *cnt += 1)
                        .or_insert(1);

                    let estimated_work_minutes =
                        (task.get_estimated_work_seconds() as f64 / 60.0 / RHO).ceil() as i64;

                    total_leaf_estimated_work_minutes_of_the_date_counter
                        .entry(dt.date_naive())
                        .and_modify(|estimated_work_minutes_val| {
                            *estimated_work_minutes_val += estimated_work_minutes
                        })
                        .or_insert(estimated_work_minutes);
                }

                id_to_dt_map
                    .entry(id)
                    .and_modify(|(dt_val, rank_val, _deadline_time_opt)| {
                        if dt > dt_val {
                            *dt_val = *dt
                        }

                        if rank > *rank_val {
                            *rank_val = rank
                        }
                    })
                    .or_insert((*dt, rank, deadline_time_opt));
            }
        }
    }

    let mut dt_id_tpl_arr: Vec<(DateTime<Local>, usize, Option<DateTime<Local>>, Uuid)> = vec![];
    for (id, (dt, rank, deadline_time_opt)) in &id_to_dt_map {
        let tpl = (*dt, *rank, *deadline_time_opt, *id);
        dt_id_tpl_arr.push(tpl);
    }

    // dt,rank等、タプルの各要素の小さい順にソート。後で逆順に変える
    dt_id_tpl_arr.sort();

    // 日付ごとのタスク数を集計する
    let mut counter: HashMap<NaiveDate, usize> = HashMap::new();
    let mut total_estimated_work_minutes_of_the_date_counter: HashMap<NaiveDate, i64> =
        HashMap::new();

    let mut msgs_with_dt: Vec<(DateTime<Local>, usize, String)> = vec![];

    // ここからρ計算用
    let last_synced_time = task_repository.get_last_synced_time();
    // let eod = last_synced_time
    //     .with_hour(23)
    //     .expect("invalid hour")
    //     .with_minute(59)
    //     .expect("invalid minute");
    let eod = (get_next_morning_datetime(last_synced_time) + Duration::days(1))
        .with_hour(1)
        .expect("invalid hour")
        .with_minute(30)
        .expect("invalid minute");

    // 今日着手可能な葉タスクまたは今日までが〆切のタスクの合計
    let mut total_deadline_estimated_work_seconds = 0;
    // ここまでρ計算用

    let mut total_estimated_work_minutes: i64 = 0;
    for (ind, (dt, rank, deadline_time_opt, id)) in dt_id_tpl_arr.iter().enumerate() {
        let task_opt = task_repository.get_by_id(*id);
        match task_opt {
            Some(task) => {
                counter
                    .entry(dt.date_naive())
                    .and_modify(|cnt| *cnt += 1)
                    .or_insert(1);

                let name = task.get_name();
                let chars_vec: Vec<char> = name.chars().collect();
                let max_len: usize = 19;
                let shorten_name: String = if chars_vec.len() >= max_len {
                    format!("{}...", chars_vec.iter().take(max_len).collect::<String>())
                } else {
                    name.to_string()
                };
                let estimated_work_minutes =
                    (task.get_estimated_work_seconds() as f64 / 60.0 / RHO).ceil() as i64;

                // ここからρ計算用
                if (rank == &0 && dt < &eod)
                    || (task.get_deadline_time_opt().is_some()
                        && task.get_deadline_time_opt().unwrap()
                            < last_synced_time + Duration::days(1))
                {
                    total_deadline_estimated_work_seconds += task.get_estimated_work_seconds();
                }
                // ここまでρ計算用

                total_estimated_work_minutes_of_the_date_counter
                    .entry(dt.date_naive())
                    .and_modify(|estimated_work_minutes_val| {
                        *estimated_work_minutes_val += estimated_work_minutes
                    })
                    .or_insert(estimated_work_minutes);

                total_estimated_work_minutes += estimated_work_minutes;
                let total_estimated_work_hours =
                    (total_estimated_work_minutes as f64 / 60.0).ceil() as i64;

                let deadline_string = match deadline_time_opt {
                    Some(d) => d.format("%Y/%m/%d").to_string(),
                    None => "____/__/__".to_string(),
                };
                let msg: String = format!(
                    "{:04} {} {} {} {} e{:02} t{:02} {}",
                    ind,
                    dt.format("%m/%d-%H:%M"),
                    rank,
                    deadline_string,
                    id,
                    estimated_work_minutes,
                    total_estimated_work_hours,
                    shorten_name
                );

                match pattern_opt {
                    Some(pattern) => {
                        // Todo: 文字列マッチの絞り込み機能とその他の属性による絞り込みを機能を分ける
                        if pattern == "葉" {
                            if rank == &0 {
                                msgs_with_dt.push((*dt, *rank, msg));
                            }
                        } else if name.to_lowercase().contains(&pattern.to_lowercase()) {
                            msgs_with_dt.push((*dt, *rank, msg));
                        }
                    }
                    None => {
                        msgs_with_dt.push((*dt, *rank, msg));
                    }
                }
            }
            None => {}
        }
    }

    // 逆順にする: dtの大きい順となる
    msgs_with_dt.reverse();

    // 日付の大きい順にソートする
    let mut counter_arr: Vec<(&NaiveDate, &usize)> = counter.iter().collect();
    counter_arr.sort_by(|a, b| b.0.cmp(&a.0));

    for (_, _, msg) in msgs_with_dt.iter() {
        writeln_newline(stdout, &msg).unwrap();
    }

    writeln_newline(stdout, "").unwrap();

    // 未来のサマリは見ても仕方ないので、直近の8日ぶん(配列の末尾)に絞る
    let start_ind = if counter_arr.len() >= 8 {
        counter_arr.len() - 8
    } else {
        0
    };

    for (date, cnt) in &counter_arr[start_ind..] {
        let total_estimated_work_minutes_of_the_date: i64 =
            *total_estimated_work_minutes_of_the_date_counter
                .get(date)
                .unwrap_or(&0);
        let total_estimated_work_hours_of_the_date =
            (total_estimated_work_minutes_of_the_date as f64 / 60.0).ceil() as i64;

        let total_leaf_estimated_work_minutes_of_the_date: i64 =
            *total_leaf_estimated_work_minutes_of_the_date_counter
                .get(date)
                .unwrap_or(&0);
        let total_leaf_estimated_work_hours_of_the_date =
            (total_leaf_estimated_work_minutes_of_the_date as f64 / 60.0).ceil() as i64;

        let leaf_cnt_of_the_date = *leaf_counter.get(date).unwrap_or(&0);

        let weekday_jp = match date.weekday() {
            Weekday::Mon => "月",
            Weekday::Tue => "火",
            Weekday::Wed => "水",
            Weekday::Thu => "木",
            Weekday::Fri => "金",
            Weekday::Sat => "土",
            Weekday::Sun => "日",
        };

        let s = format!(
            "{}({})\t{:02}/{:02}[時間]\t{:02}/{:02}[タスク]\t{:02}/{:02}[分/タスク]",
            date,
            weekday_jp,
            total_leaf_estimated_work_hours_of_the_date,
            total_estimated_work_hours_of_the_date,
            leaf_cnt_of_the_date,
            cnt,
            if leaf_cnt_of_the_date > 0 {
                (total_leaf_estimated_work_minutes_of_the_date as f64 / leaf_cnt_of_the_date as f64)
                    .ceil() as i64
            } else {
                0
            },
            if **cnt > 0 {
                (total_estimated_work_minutes_of_the_date as f64 / **cnt as f64).ceil() as i64
            } else {
                0
            },
        );
        writeln_newline(stdout, &s).unwrap();
    }
    writeln_newline(stdout, "").unwrap();

    // タスクができない時間を決め打ちで登録する
    let busy_time_slots = [];

    for ((start_hour, start_minute), (end_hour, end_minute)) in busy_time_slots.iter() {
        free_time_manager.register_busy_time_slot(
            &last_synced_time
                .with_hour(*start_hour)
                .expect("invalid hour")
                .with_minute(*start_minute)
                .expect("invalid minute"),
            &last_synced_time
                .with_hour(*end_hour)
                .expect("invalid hour")
                .with_minute(*end_minute)
                .expect("invalid minute"),
        );
    }

    // 1日の残りの時間から稼働率ρを計算する
    let busy_seconds = free_time_manager.get_busy_minutes(&last_synced_time, &eod) * 60;
    let lambda_seconds = total_deadline_estimated_work_seconds + busy_seconds;

    let estimated_finish_dt = last_synced_time + Duration::seconds(lambda_seconds);

    let busy_hours = (busy_seconds as f64 / 60.0 / 60.0).ceil() as i64;
    let busy_s = format!("残り拘束時間は{}時間です", busy_hours);
    writeln_newline(stdout, &busy_s).unwrap();

    let hours = (lambda_seconds as f64 / 60.0 / 60.0).ceil() as i64;
    let s = format!(
        "完了見込み日時は{}時間後の{}です",
        hours, estimated_finish_dt
    );
    writeln_newline(stdout, &s).unwrap();

    let mu_seconds = (eod - last_synced_time).num_seconds();
    let lambda_hours = lambda_seconds as f64 / 3600.0;
    let mu_hours = mu_seconds as f64 / 3600.0;
    let rho = lambda_seconds as f64 / mu_seconds as f64;
    let lq_opt = if rho < 1.0 {
        Some(rho / (1.0 - rho))
    } else {
        None
    };
    let s2 = match lq_opt {
        Some(lq) => {
            format!(
                "rho = {:.1} / {:.1} = {:.2}, Lq = {:.1}",
                lambda_hours, mu_hours, rho, lq
            )
        }
        None => {
            format!(
                "rho = {:.1} / {:.1} = {:.2}, Lq = inf",
                lambda_hours, mu_hours, rho
            )
        }
    };
    writeln_newline(stdout, &s2).unwrap();
    writeln_newline(stdout, "").unwrap();
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
    focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<Task>,
    amount_str: &str,
    unit_str: &str,
) {
    let amount: i64 = amount_str.parse().unwrap();
    let duration = match unit_str.chars().nth(0) {
        Some('日') | Some('d') => Duration::days(amount),
        Some('時') | Some('h') => Duration::hours(amount),
        Some('分') | Some('m') => Duration::minutes(amount),
        // 誤入力した時に傷が浅いように、デフォルトは秒としておく
        Some('秒') | Some('s') | _ => Duration::seconds(amount),
    };

    focused_task_opt.as_ref().and_then(|focused_task| {
        focused_task.set_pending_until(Local::now() + duration);
        focused_task.set_orig_status(Status::Pending);

        // dummy
        None::<i32>
    });

    *focused_task_id_opt = None;
}

fn execute_finish(focused_task_id_opt: &mut Option<Uuid>, focused_task_opt: &Option<Task>) {
    focused_task_opt.as_ref().and_then(|focused_task| {
        focused_task.set_orig_status(Status::Done);
        focused_task.set_end_time_opt(Some(Local::now()));

        // 親タスクがrepetition_interval_daysを持っているなら、
        // その値に従って兄弟ノードを生成する
        // start_timeは日付は(repetition_interval_days-1)日後で、時刻は親タスクのstart_timeを引き継ぐ
        // タスク名は「親タスク名(日付)」
        // estimated_work_secondsは親タスクを引き継ぐ
        match focused_task.parent() {
            Some(parent_task) => match parent_task.get_repetition_interval_days_opt() {
                Some(repetition_interval_days) => {
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

                    let estimated_work_seconds = parent_task.get_estimated_work_seconds();

                    let mut new_task_attr = TaskAttr::new(&new_task_name);
                    new_task_attr.set_start_time(new_start_time);
                    new_task_attr.set_estimated_work_seconds(estimated_work_seconds);
                    parent_task.create_as_last_child(new_task_attr);
                }
                None => {}
            },
            None => {}
        }

        // もし親タスクがTodoでないならば、フォーカスを外す
        match focused_task.parent().map(|t| t.get_status()) {
            Some(Status::Todo) => {
                // do nothing
            }
            _ => {
                *focused_task_id_opt = None;

                // dummy
                return None::<i32>;
            }
        }

        // 親タスクがTodoの時
        // 兄弟タスクが無い場合は、フォーカスを親タスクに移す。
        // そうでない場合は、フォーカスを外す。
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
    let deadline_time_str = format!("{} 23:59:59", deadline_date_str);
    let deadline_time = Local
        .datetime_from_str(&deadline_time_str, "%Y/%m/%d %H:%M:%S")
        .unwrap();

    focused_task_opt
        .as_ref()
        .map(|focused_task| focused_task.set_deadline_time_opt(Some(deadline_time)));
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

fn execute(
    stdout: &mut RawTerminal<Stdout>,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
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
        "新" | "new" => {
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

                let is_deferred = true;
                execute_start_new_project(
                    stdout,
                    focused_task_id_opt,
                    task_repository,
                    new_project_name_str,
                    is_deferred,
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

                let is_deferred = false;
                execute_start_new_project(
                    stdout,
                    focused_task_id_opt,
                    task_repository,
                    new_project_name_str,
                    is_deferred,
                    estimated_work_minutes_opt,
                );
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

            execute_show_all_tasks(stdout, task_repository, free_time_manager, &pattern_opt);
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
        "上" | "nextup" | "nu" => {}
        "下" | "breakdown" | "bd" => {
            if tokens.len() >= 2 {
                let new_task_names = &tokens[1..];
                execute_breakdown(
                    stdout,
                    focused_task_id_opt,
                    &focused_task_opt,
                    new_task_names,
                    &None,
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

                execute_set_deadline(&focused_task_opt, deadline_date_str);
            }
        }
        "予" | "estimate" | "es" => {
            if tokens.len() >= 2 {
                let estimated_work_minutes_str = &tokens[1];
                execute_set_estimated_work_minutes(&focused_task_opt, estimated_work_minutes_str);
            }
        }
        // "実" | "actual" | "ac" => {
        //     if tokens.len() >= 2 {
        //         let actual_work_minutes_str = &tokens[1];
        //         execute_set_actual_work_minutes(&focused_task_opt, actual_work_minutes_str);
        //     }
        // }
        "後" | "defer" => {
            if tokens.len() >= 3 {
                let amount_str = &tokens[1];
                let unit_str = &tokens[2].to_lowercase();

                execute_defer(focused_task_id_opt, &focused_task_opt, amount_str, unit_str);
            } else if tokens.len() == 2 {
                // "defer 5days" のように引数が1つしか与えられなかった場合は、数字部分とそれ以降に分割する
                let splitted = split_amount_and_unit(tokens[1]);
                if splitted.len() == 2 {
                    let amount_str = &splitted[0];
                    let unit_str = &splitted[1].to_lowercase();

                    execute_defer(focused_task_id_opt, &focused_task_opt, amount_str, unit_str);
                }
            }
        }
        "終" | "finish" | "fin" => {
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

    // controllerで実体を見るのを避けるために、1つ関数を切る
    application(&mut task_repository, &mut free_time_manager);
}

fn application(
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) {
    // 時計を合わせる
    task_repository.sync_clock(Local::now());

    // let next_morning = get_next_morning_datetime(now);
    // task_repository.sync_clock(next_morning + Duration::hours(1));
    task_repository.load();

    // RawModeを有効にする
    let mut stdout = stdout().into_raw_mode().unwrap();

    write!(stdout, "{}", termion::clear::All).unwrap();
    write!(stdout, "{}", termion::cursor::BlinkingBar).unwrap();
    stdout.flush().unwrap();

    ///////////////////////

    execute_show_all_tasks(&mut stdout, task_repository, free_time_manager, &None);

    ///////////////////////

    execute_show_leaf_tasks(&mut stdout, task_repository, free_time_manager);

    // 優先度の最も高いPJを一つ選ぶ
    // 一番下のタスクにフォーカスが自動的に当たる
    let mut focused_task_id_opt: Option<Uuid> = task_repository.get_highest_priority_leaf_task_id();

    // この処理、よく使いそう
    match focused_task_id_opt {
        Some(focused_task_id) => {
            let focused_task_opt = task_repository.get_by_id(focused_task_id);

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
                writeln!(stdout, "").unwrap();
                write!(stdout, "{}", termion::cursor::Left(MAX_COL),).unwrap();

                println!("{}{}{}", style::Bold, line, style::Reset);
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
                        &s,
                    );
                } else if line == "d" {
                    // skip "d"aily
                    let now: DateTime<Local> = Local::now();
                    let next_morning = get_next_morning_datetime(now);
                    let sec = (next_morning - now).num_seconds();
                    let s = format!("後 {}秒", sec).to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &s,
                    );
                } else if line == "w" {
                    // skip "w"eekly
                    let now: DateTime<Local> = Local::now();
                    let next_morning = get_next_morning_datetime(now);
                    let sec = (next_morning - now).num_seconds() + 86400 * 6;

                    let s = format!("後 {}秒", sec).to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &s,
                    );
                } else if line == "y" {
                    // skip "y"early
                    let now: DateTime<Local> = Local::now();
                    let next_morning = get_next_morning_datetime(now);
                    let sec = (next_morning - now).num_seconds() + 86400 * 365 * 5;

                    let s = format!("後 {}秒", sec).to_string();

                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &s,
                    );
                } else {
                    execute(
                        &mut stdout,
                        task_repository,
                        free_time_manager,
                        &mut focused_task_id_opt,
                        &line,
                    );
                }

                // 時計を合わせる
                task_repository.sync_clock(Local::now());

                //////////////////////////////

                // もしfocused_task_id_optがNoneの時は最も優先度が高いタスクの選出をやり直す

                if focused_task_id_opt.is_none() {
                    focused_task_id_opt = task_repository.get_highest_priority_leaf_task_id();
                }

                //////////////////////////////

                // スクロールするのが面倒なので、新や突のように付加情報を表示するコマンドの直後は葉を表示しない
                // Todo: "new" や  "unplanned" の場合にも対応する
                let fst_char_opt = line.chars().nth(0);
                if fst_char_opt != Some('新')
                    && fst_char_opt != Some('突')
                    && fst_char_opt != Some('全')
                    && fst_char_opt != Some('葉')
                    && fst_char_opt != Some('木')
                {
                    execute_show_leaf_tasks(&mut stdout, task_repository, free_time_manager);
                }

                match focused_task_id_opt {
                    Some(focused_task_id) => {
                        let focused_task_opt = task_repository.get_by_id(focused_task_id);

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
