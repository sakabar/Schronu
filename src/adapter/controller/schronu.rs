use chrono::Local;
use regex::Regex;
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::interface::TaskRepositoryTrait;
use schronu::entity::task::{extract_leaf_tasks_from_project, Task, TaskAttr};
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

    let focused_task_opt = match focused_task_id_opt {
        None => None,
        Some(id) => task_repository.get_by_id(*id),
    };

    println!("{:?}", focused_task_opt);
    stdout.flush().unwrap();

    let tokens: Vec<&str> = line.split(' ').collect();
    match tokens[0] {
        "新" | "new" => {}
        "木" | "tree" => {}
        "根" | "root" => {}
        "葉" | "leaves" | "leaf" | "lf" => {}
        "見" | "focus" | "fc" => {}
        "外" | "unfocus" | "ufc" => {}
        "親" | "parent" => {}
        "子" | "children" | "ch" => {}
        "上" | "nextup" | "nu" => {}
        "下" | "breakdown" | "bd" => {
            match focused_task_opt {
                Some(focused_task) => {
                    if tokens.len() >= 2 {
                        let new_task_name = &tokens[1];
                        let new_task_attr = TaskAttr::new(new_task_name);

                        // 新しい子タスクにフォーカス(id)を移す
                        *focused_task_id_opt =
                            Some(focused_task.create_as_last_child(new_task_attr).get_id());
                    }
                }
                None => {}
            }
        }
        // "詳" | "description" | "desc" => {}
        "後" | "defer" => {}
        "終" | "finish" | "fin" => {}
        &_ => {}
    }
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
    task_repository.sync_clock(Local::now());
    task_repository.load();

    // RawModeを有効にする
    let mut stdout = stdout().into_raw_mode().unwrap();

    write!(stdout, "{}", termion::clear::All).unwrap();
    write!(stdout, "{}", termion::cursor::BlinkingBar).unwrap();
    stdout.flush().unwrap();

    ///////////////////////

    // 優先度の最も高いPJを一つ選ぶ
    // 一番下のタスクにフォーカスが自動的に当たる
    let highest_pj_opt = task_repository.get_highest_priority_project();
    let mut focused_task_id_opt: Option<Uuid> = match highest_pj_opt {
        Some(highest_pj) => {
            let leaf_tasks: Vec<Task> = extract_leaf_tasks_from_project(highest_pj);
            let last_opt = leaf_tasks.last();
            let id = last_opt.and_then(|task| Some(task.get_id()));

            id
        }
        None => None,
    };

    // この処理、よく使いそう
    match focused_task_id_opt {
        Some(focused_task_id) => {
            let focused_task_opt = task_repository.get_by_id(focused_task_id);

            match focused_task_opt {
                Some(focused_task) => {
                    println!("{:?}", focused_task);
                    stdout.flush().unwrap();
                }
                None => {}
            }
        }
        None => {}
    }

    ///////////////////////

    let header: &str = "schronu>";
    let mut line = String::from("");

    // 画面に表示されている「文字」単位でのカーソル。
    let mut cursor_x: usize = 0;

    // write!(stdout, "{}", termion::cursor::Left(999)).unwrap();
    write!(stdout, "{}", header).unwrap();
    stdout.flush().unwrap();

    // キー入力を受け付ける
    for c in std::io::stdin().keys() {
        match c.unwrap() {
            Key::Char('q') | Key::Ctrl('d') => {
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
                stdout.flush().unwrap();

                execute(
                    &mut stdout,
                    task_repository,
                    &mut focused_task_id_opt,
                    &line,
                );

                match focused_task_id_opt {
                    Some(focused_task_id) => {
                        let focused_task_opt = task_repository.get_by_id(focused_task_id);

                        match focused_task_opt {
                            Some(focused_task) => {
                                println!("{:?}", focused_task);
                                stdout.flush().unwrap();
                            }
                            None => {}
                        }
                    }
                    None => {}
                }

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
