use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Timelike};
use regex::Regex;
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::TaskRepositoryTrait;
use schronu::entity::task::{extract_leaf_tasks_from_project, Status, Task, TaskAttr};
use std::io::Stdout;
use std::io::{stdout, Write};
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::raw::RawTerminal;
use termion::style;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;
use uuid::Uuid;

const MAX_COL: u16 = 999;

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
    // as_ref()の必要性が分かっていないので後で調べる
    // これが無いと:
    // cannot move out of `*focused_task_opt` which is behind a shared reference
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

fn execute_show_leaf_tasks(
    stdout: &mut RawTerminal<Stdout>,
    task_repository: &mut dyn TaskRepositoryTrait,
) {
    let mut task_cnt = 1;
    for project_root_task in task_repository.get_all_projects().iter() {
        let project_name = project_root_task.get_name();

        for leaf_task in extract_leaf_tasks_from_project(&project_root_task).iter() {
            let message = format!("{}\t{}\t{:?}", task_cnt, project_name, leaf_task.get_attr());
            writeln_newline(stdout, &message).unwrap();
            task_cnt += 1;
        }
    }
    writeln_newline(stdout, "").unwrap();
}

fn execute_focus(focused_task_id_opt: &mut Option<Uuid>, new_task_id_str: &str) {
    match Uuid::parse_str(new_task_id_str) {
        Ok(id) => *focused_task_id_opt = Some(id),
        Err(_) => {}
    }
}

fn execute_unfocus(focused_task_id_opt: &mut Option<Uuid>) {
    *focused_task_id_opt = None;
}

fn get_next_morning_datetime(now: DateTime<Local>) -> DateTime<Local> {
    if now.hour() >= 6 {
        // 翌日の午前6時
        let dt = now + Duration::days(1);
        let datetime_str = format!("{}/{}/{} 06:00", dt.year(), dt.month(), dt.day());
        Local
            .datetime_from_str(&datetime_str, "%Y/%m/%d %H:%M")
            .unwrap()
    } else {
        // 今日の午前6時
        let datetime_str = format!("{}/{}/{} 06:00", now.year(), now.month(), now.day());
        Local
            .datetime_from_str(&datetime_str, "%Y/%m/%d %H:%M")
            .unwrap()
    }
}

#[test]
fn test_get_next_morning_datetime_6時以降の場合() {
    let dt = Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap();
    let actual = get_next_morning_datetime(dt);

    assert_eq!(actual, Local.with_ymd_and_hms(2023, 4, 2, 6, 0, 0).unwrap());
}

#[test]
fn test_get_next_morning_datetime_6時以前の場合() {
    let dt = Local.with_ymd_and_hms(2023, 4, 1, 1, 0, 0).unwrap();
    let actual = get_next_morning_datetime(dt);

    assert_eq!(actual, Local.with_ymd_and_hms(2023, 4, 1, 6, 0, 0).unwrap());
}

fn execute_impluse(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    new_task_names: &[&str],
) {
    // 今フォーカスしているIDを退避する
    let stashed_focused_task_id_opt = focused_task_id_opt.clone();

    // TODO: ここ、コンフィグで雑務idを読み書きする
    let impulse_task_id_string = String::from("6d19cdb2-1dbb-41bd-899f-551a83bf4800");
    execute_focus(focused_task_id_opt, &impulse_task_id_string);
    let focused_task_opt = focused_task_id_opt.and_then(|id| task_repository.get_by_id(id));

    // 次回の午前6時
    let now: DateTime<Local> = Local::now();
    let pending_until = get_next_morning_datetime(now);

    execute_breakdown(
        focused_task_id_opt,
        &focused_task_opt,
        new_task_names,
        &Some(pending_until),
    );

    // フォーカスを元のタスクに戻す
    *focused_task_id_opt = stashed_focused_task_id_opt;
}

fn execute_breakdown(
    focused_task_id_opt: &mut Option<Uuid>,
    focused_task_opt: &Option<Task>,
    new_task_names: &[&str],
    pending_until_opt: &Option<DateTime<Local>>,
) {
    // as_ref()の必要性が分かっていないので後で調べる
    // これが無いと:
    // cannot move out of `*focused_task_opt` which is behind a shared reference
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

            // 新しい子タスクにフォーカス(id)を移す
            *focused_task_id_opt = Some(new_task.get_id());
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

        // 兄弟ノードが無い場合は、フォーカスを親タスクに移す。
        // そうでない場合は、フォーカスを外す。
        // 兄弟ノードがある場合に、本来はそちらの葉のdone状況を確認してからOKであれば親タスクにフォーカスを移すべきだが、難しいので今は対象としない
        *focused_task_id_opt = focused_task.parent().and_then(|parent| {
            if parent.num_children() == 1 {
                Some(parent.get_id())
            } else {
                None
            }
        });

        // dummy
        None::<i32>
    });
}

fn execute(
    stdout: &mut RawTerminal<Stdout>,
    task_repository: &mut dyn TaskRepositoryTrait,
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
        "新" | "new" => {}
        "木" | "tree" => {
            execute_show_tree(stdout, &focused_task_opt);
        }
        "根" | "root" => {}
        "葉" | "leaves" | "leaf" | "lf" => {
            execute_show_leaf_tasks(stdout, task_repository);
        }
        "見" | "focus" | "fc" => {
            if tokens.len() >= 2 {
                let new_task_id_str = &tokens[1];
                execute_focus(focused_task_id_opt, new_task_id_str);
            }
        }
        "外" | "unfocus" | "ufc" => {
            execute_unfocus(focused_task_id_opt);
        }
        "親" | "parent" => {}
        "子" | "children" | "ch" => {}
        "上" | "nextup" | "nu" => {}
        "下" | "breakdown" | "bd" => {
            if tokens.len() >= 2 {
                let new_task_names = &tokens[1..];
                execute_breakdown(
                    focused_task_id_opt,
                    &focused_task_opt,
                    new_task_names,
                    &None,
                );
            }
        }
        // "詳" | "description" | "desc" => {}
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
        "衝" | "impulse" | "imp" => {
            if tokens.len() >= 2 {
                let new_task_names = &tokens[1..];

                execute_impluse(task_repository, focused_task_id_opt, new_task_names);
            }
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
    let mut task_repository = TaskRepository::new("../Schronu-alpha/tasks/");

    // controllerで実体を見るのを避けるために、1つ関数を切る
    application(&mut task_repository);
}

fn application(task_repository: &mut dyn TaskRepositoryTrait) {
    // 初期化
    let now = Local::now();
    task_repository.sync_clock(now);

    // let next_morning = get_next_morning_datetime(now);
    // task_repository.sync_clock(next_morning + Duration::hours(1));
    task_repository.load();

    // RawModeを有効にする
    let mut stdout = stdout().into_raw_mode().unwrap();

    write!(stdout, "{}", termion::clear::All).unwrap();
    write!(stdout, "{}", termion::cursor::BlinkingBar).unwrap();
    stdout.flush().unwrap();

    ///////////////////////

    // 優先度の最も高いPJを一つ選ぶ
    // 一番下のタスクにフォーカスが自動的に当たる
    let mut focused_task_id_opt: Option<Uuid> = task_repository.get_highest_priority_leaf_task_id();

    // この処理、よく使いそう
    match focused_task_id_opt {
        Some(focused_task_id) => {
            let focused_task_opt = task_repository.get_by_id(focused_task_id);

            match focused_task_opt {
                Some(focused_task) => {
                    println!("{}focused task is:", termion::cursor::Left(MAX_COL));
                    println!("{}{:?}", termion::cursor::Left(MAX_COL), focused_task);
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
            // バグあり。Ctrl-Eがうまく動かない
            Key::Ctrl('e') => {
                let s: String = String::from(format!("{}{}", header, line));
                let width = UnicodeWidthStr::width(s.as_str()) as u16;
                write!(stdout, "{}", termion::cursor::Left(MAX_COL)).unwrap();

                write!(stdout, "{}", termion::cursor::Right(width)).unwrap();
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
                // todo!("未実装です");
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

                    execute(&mut stdout, task_repository, &mut focused_task_id_opt, &s);
                } else if line == "d" {
                    // skip "d"aily
                    let now: DateTime<Local> = Local::now();
                    let next_morning = get_next_morning_datetime(now);
                    let sec = (next_morning - now).num_seconds();
                    let s = format!("後 {}秒", sec).to_string();

                    execute(&mut stdout, task_repository, &mut focused_task_id_opt, &s);
                } else if line == "w" {
                    // skip "w"eekly
                    let now: DateTime<Local> = Local::now();
                    let next_morning = get_next_morning_datetime(now);
                    let sec = (next_morning - now).num_seconds() + 86400 * 4;

                    let s = format!("後 {}秒", sec).to_string();

                    execute(&mut stdout, task_repository, &mut focused_task_id_opt, &s);
                } else {
                    execute(
                        &mut stdout,
                        task_repository,
                        &mut focused_task_id_opt,
                        &line,
                    );
                }

                //////////////////////////////

                // もしfocused_task_id_optがNoneの時は最も優先度が高いタスクの選出をやり直す

                if focused_task_id_opt.is_none() {
                    focused_task_id_opt = task_repository.get_highest_priority_leaf_task_id();
                }

                //////////////////////////////

                match focused_task_id_opt {
                    Some(focused_task_id) => {
                        let focused_task_opt = task_repository.get_by_id(focused_task_id);

                        match focused_task_opt {
                            Some(focused_task) => {
                                println!("{}focused task is:", termion::cursor::Left(MAX_COL));
                                println!("{}{:?}", termion::cursor::Left(MAX_COL), focused_task);
                                println!("{}", termion::cursor::Left(MAX_COL));
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

    // BlinkingBlockに戻す
    writeln!(stdout, "{}", termion::cursor::BlinkingBlock).unwrap();
}
