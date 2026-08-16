use super::*;
use std::io::{IsTerminal, Stdout};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration as StdDuration, Instant};
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};
use termion::style;

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

const IDLE_REFRESH_INTERVAL: StdDuration = StdDuration::from_secs(60);

pub(super) fn idle_refresh_deadline(now: Instant) -> Instant {
    now + IDLE_REFRESH_INTERVAL
}

pub(super) fn idle_wait_duration(deadline: Instant, now: Instant) -> StdDuration {
    deadline.saturating_duration_since(now)
}

pub(super) fn application(
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
) -> Result<(), RunError> {
    let now = Local::now();

    // let next_morning = get_next_morning_datetime(now)
    //     .with_hour(6)
    //     .expect("invalid hour")
    //     .with_minute(0)
    //     .expect("invalid minute");
    // task_repository.sync_clock(next_morning);

    drop(reload_repository_for_cli(task_repository, now)?);

    load_busy_time_slots_for_interactive_application(
        free_time_manager,
        active_config()
            .busy_time_slots_yaml_path
            .to_str()
            .expect("config path was validated"),
    )?;

    // RawModeを有効にする
    let mut stdout = stdout().into_raw_mode().unwrap();

    write!(stdout, "{}", termion::cursor::BlinkingBar).unwrap();
    stdout.flush().unwrap();

    // 起動直後はrhoの値を見たいので葉は出力しない
    // execute_show_leaf_tasks(&mut stdout, task_repository, free_time_manager);

    // 優先度の最も高いPJを一つ選ぶ
    // 一番下のタスクにフォーカスが自動的に当たる

    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;
    let mut focused_task_id_opt: Option<Uuid> =
        select_focus_task_id(task_repository, focus_selection_mode)
            .map_err(CommandError::from)
            .map_err(RunError::from)?;

    let mut last_focused_task_id_opt: Option<Uuid> = None;
    let mut focus_started_datetime: DateTime<Local> = now;

    let header: &str = "schronu> ";
    let mut line = String::from("");

    // 画面に表示されている「文字」単位でのカーソル。
    let mut cursor_x: usize = 0;

    render_interactive_screen(
        &mut stdout,
        task_repository,
        free_time_manager,
        FocusRenderState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
        },
        PromptRenderState {
            header,
            line: &line,
            cursor_x,
        },
        now,
    );

    let (key_sender, key_receiver) = mpsc::channel();
    thread::spawn(move || {
        for key_result in std::io::stdin().keys() {
            if key_sender.send(key_result).is_err() {
                break;
            }
        }
    });

    let mut next_refresh_at = idle_refresh_deadline(Instant::now());
    let mut loop_error_opt = None;

    // キー入力を受け付け、無操作が60秒続いたら画面を再描画する
    loop {
        let wait_duration = idle_wait_duration(next_refresh_at, Instant::now());
        let key = match key_receiver.recv_timeout(wait_duration) {
            Ok(Ok(key)) => {
                next_refresh_at = idle_refresh_deadline(Instant::now());
                key
            }
            Ok(Err(input_error)) => {
                let outcome = handle_interactive_repository_event(
                    &mut stdout,
                    task_repository,
                    free_time_manager,
                    InteractiveRepositoryState {
                        focused_task_id_opt: &mut focused_task_id_opt,
                        last_focused_task_id_opt: &mut last_focused_task_id_opt,
                        focus_started_datetime: &mut focus_started_datetime,
                        focus_selection_mode: &mut focus_selection_mode,
                    },
                    InteractiveRepositoryEvent::InputRead(input_error),
                );
                if let InteractiveRepositoryEventOutcome::Fatal(error) = outcome {
                    loop_error_opt = Some(error);
                }
                break;
            }
            Err(RecvTimeoutError::Timeout) => {
                let outcome = handle_interactive_repository_event(
                    &mut stdout,
                    task_repository,
                    free_time_manager,
                    InteractiveRepositoryState {
                        focused_task_id_opt: &mut focused_task_id_opt,
                        last_focused_task_id_opt: &mut last_focused_task_id_opt,
                        focus_started_datetime: &mut focus_started_datetime,
                        focus_selection_mode: &mut focus_selection_mode,
                    },
                    InteractiveRepositoryEvent::Refresh,
                );
                match outcome {
                    InteractiveRepositoryEventOutcome::Continue => {}
                    InteractiveRepositoryEventOutcome::Retry(error) => {
                        writeln_newline(&mut stdout, &format!("[Error] {error}")).unwrap();
                        render_prompt(&mut stdout, header, &line, cursor_x);
                        next_refresh_at = idle_refresh_deadline(Instant::now());
                        continue;
                    }
                    InteractiveRepositoryEventOutcome::Fatal(error) => {
                        loop_error_opt = Some(error);
                        break;
                    }
                    _ => unreachable!("refresh event returned an invalid outcome"),
                }
                render_interactive_screen(
                    &mut stdout,
                    task_repository,
                    free_time_manager,
                    FocusRenderState {
                        focused_task_id_opt: &mut focused_task_id_opt,
                        last_focused_task_id_opt: &mut last_focused_task_id_opt,
                        focus_started_datetime: &mut focus_started_datetime,
                    },
                    PromptRenderState {
                        header,
                        line: &line,
                        cursor_x,
                    },
                    Local::now(),
                );
                next_refresh_at = idle_refresh_deadline(Instant::now());
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                let outcome = handle_interactive_repository_event(
                    &mut stdout,
                    task_repository,
                    free_time_manager,
                    InteractiveRepositoryState {
                        focused_task_id_opt: &mut focused_task_id_opt,
                        last_focused_task_id_opt: &mut last_focused_task_id_opt,
                        focus_started_datetime: &mut focus_started_datetime,
                        focus_selection_mode: &mut focus_selection_mode,
                    },
                    InteractiveRepositoryEvent::InputDisconnected,
                );
                if let InteractiveRepositoryEventOutcome::Fatal(error) = outcome {
                    loop_error_opt = Some(error);
                }
                break;
            }
        };

        match key {
            Key::Ctrl('d') => {
                let outcome = handle_interactive_repository_event(
                    &mut stdout,
                    task_repository,
                    free_time_manager,
                    InteractiveRepositoryState {
                        focused_task_id_opt: &mut focused_task_id_opt,
                        last_focused_task_id_opt: &mut last_focused_task_id_opt,
                        focus_started_datetime: &mut focus_started_datetime,
                        focus_selection_mode: &mut focus_selection_mode,
                    },
                    InteractiveRepositoryEvent::Exit {
                        header,
                        line: &line,
                        cursor_x,
                    },
                );
                match outcome {
                    InteractiveRepositoryEventOutcome::Exit => break,
                    InteractiveRepositoryEventOutcome::Retry(error) => {
                        writeln_newline(&mut stdout, &format!("[Error] {error}")).unwrap();
                        render_prompt(&mut stdout, header, &line, cursor_x);
                    }
                    InteractiveRepositoryEventOutcome::Fatal(error) => {
                        loop_error_opt = Some(error);
                        break;
                    }
                    InteractiveRepositoryEventOutcome::Continue => {}
                    _ => unreachable!("exit event returned an invalid outcome"),
                }
            }
            Key::Ctrl('c') => {
                // 未送信の入力を破棄し、terminalを後始末してから異常終了する
                let outcome = handle_interactive_repository_event(
                    &mut stdout,
                    task_repository,
                    free_time_manager,
                    InteractiveRepositoryState {
                        focused_task_id_opt: &mut focused_task_id_opt,
                        last_focused_task_id_opt: &mut last_focused_task_id_opt,
                        focus_started_datetime: &mut focus_started_datetime,
                        focus_selection_mode: &mut focus_selection_mode,
                    },
                    InteractiveRepositoryEvent::Interrupted,
                );
                if let InteractiveRepositoryEventOutcome::Fatal(error) = outcome {
                    loop_error_opt = Some(error);
                }
                break;
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

                let width = get_width_for_rerender(header, &line, cursor_x);
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

                let width = get_width_for_rerender(header, &line, cursor_x);
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

                let width = get_width_for_rerender(header, &line, cursor_x);
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
                if let Some(byte_offset) = byte_offset_opt {
                    line.remove(byte_offset);
                    cursor_x -= 1;
                }

                write!(
                    stdout,
                    "{}{}",
                    termion::cursor::Left(MAX_COL),
                    termion::clear::CurrentLine,
                )
                .unwrap();

                let width = get_width_for_rerender(header, &line, cursor_x);
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
                let outcome = handle_interactive_repository_event(
                    &mut stdout,
                    task_repository,
                    free_time_manager,
                    InteractiveRepositoryState {
                        focused_task_id_opt: &mut focused_task_id_opt,
                        last_focused_task_id_opt: &mut last_focused_task_id_opt,
                        focus_started_datetime: &mut focus_started_datetime,
                        focus_selection_mode: &mut focus_selection_mode,
                    },
                    InteractiveRepositoryEvent::Submit { line: &line },
                );
                let command = match outcome {
                    InteractiveRepositoryEventOutcome::CommandExecuted(command) => command,
                    InteractiveRepositoryEventOutcome::Fatal(error) => {
                        loop_error_opt = Some(error);
                        break;
                    }
                    InteractiveRepositoryEventOutcome::Retry(error) => {
                        writeln_newline(&mut stdout, &format!("[Error] {error}")).unwrap();
                        render_prompt(&mut stdout, header, &line, cursor_x);
                        continue;
                    }
                    _ => unreachable!("submit event returned an invalid outcome"),
                };

                // スクロールするのが面倒なので、新や突のように付加情報を表示するコマンドの直後は葉を表示しない
                // Todo: "new" や  "unplanned" の場合にも対応する
                if !should_suppress_leaf_tasks_after_command(&command) {
                    let result =
                        execute_show_leaf_tasks(&mut stdout, task_repository, free_time_manager);
                    report_application_result(&mut stdout, result);
                }

                render_focused_task(
                    &mut stdout,
                    task_repository,
                    focused_task_id_opt,
                    &mut last_focused_task_id_opt,
                    &mut focus_started_datetime,
                    Local::now(),
                );

                //////////////////////////////

                // 初期化
                cursor_x = 0;
                line.clear();
                render_prompt(&mut stdout, header, &line, cursor_x);
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

                let width = get_width_for_rerender(header, &line, cursor_x);
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

    // SteadyBlockに戻す
    // Todo: 本当は、元々の状態を保存しておいてそれに戻したい。
    writeln!(stdout, "{}", termion::cursor::SteadyBlock).unwrap();
    match loop_error_opt {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
