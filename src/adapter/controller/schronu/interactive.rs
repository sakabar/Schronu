use super::command::CommandKind;
use super::renderer::{writeln_newline, SchronuWriter, MAX_COL};
use chrono::{DateTime, Local};
use std::fmt::Display;
use std::io::{stdout, Write};
use std::sync::mpsc::Receiver;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::style;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const IDLE_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

pub(super) enum DriverEvent<'a> {
    RenderScreen { now: DateTime<Local> },
    Refresh,
    Submit { line: &'a str },
    Exit,
    Interrupted,
    InputDisconnected,
    InputRead(std::io::Error),
}

pub(super) enum DriverOutcome<R, E> {
    Continue,
    Submitted,
    Retry(R),
    Exit,
    Fatal(E),
}

fn expect_continue<R, E>(outcome: DriverOutcome<R, E>, event: &str) {
    if !matches!(outcome, DriverOutcome::Continue) {
        unreachable!("{event} event returned an invalid outcome");
    }
}

fn expect_fatal<R, E>(outcome: DriverOutcome<R, E>, event: &str) -> E {
    match outcome {
        DriverOutcome::Fatal(error) => error,
        _ => unreachable!("{event} event returned an invalid outcome"),
    }
}

#[derive(Debug, Eq, PartialEq)]
enum ControlKey {
    Exit,
    Interrupted,
    Submit,
}

fn control_key(key: &Key, line_is_empty: bool) -> Option<ControlKey> {
    match key {
        Key::Ctrl('d') if line_is_empty => Some(ControlKey::Exit),
        Key::Ctrl('c') => Some(ControlKey::Interrupted),
        Key::Char('\n') | Key::Ctrl('m') => Some(ControlKey::Submit),
        _ => None,
    }
}

enum ReceivedInput {
    Key(Key),
    Refresh,
    ReadError(std::io::Error),
    Disconnected,
}

fn receive_input(
    receiver: &Receiver<std::io::Result<Key>>,
    wait_duration: Duration,
) -> ReceivedInput {
    match receiver.recv_timeout(wait_duration) {
        Ok(Ok(key)) => ReceivedInput::Key(key),
        Ok(Err(error)) => ReceivedInput::ReadError(error),
        Err(RecvTimeoutError::Timeout) => ReceivedInput::Refresh,
        Err(RecvTimeoutError::Disconnected) => ReceivedInput::Disconnected,
    }
}

fn reset_submitted_line(line: &mut String, cursor_x: &mut usize) {
    line.clear();
    *cursor_x = 0;
}

pub(super) fn backward_width(line: &str, cursor_x: usize) -> u16 {
    if line.chars().count() == 0 || cursor_x == 0 {
        return 0;
    }
    line.chars()
        .nth(cursor_x - 1)
        .and_then(UnicodeWidthChar::width)
        .unwrap_or(0) as u16
}

pub(super) fn get_byte_offset_for_insert(line: &str, cursor_x: usize) -> usize {
    let char_indices = line.char_indices().collect::<Vec<_>>();
    if !line.is_empty() && cursor_x < char_indices.len() {
        char_indices[cursor_x].0
    } else {
        line.len()
    }
}

pub(super) fn get_byte_offset_for_deletion(line: &str, cursor_x: usize) -> Option<usize> {
    if line.is_empty() || cursor_x == 0 {
        None
    } else {
        Some(line.char_indices().collect::<Vec<_>>()[cursor_x - 1].0)
    }
}

pub(super) fn get_width_for_rerender(header: &str, line: &str, cursor_x: usize) -> u16 {
    let width = UnicodeWidthStr::width(header)
        + line
            .chars()
            .take(cursor_x)
            .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
            .sum::<usize>();
    width as u16
}

pub(super) fn get_forward_width(line: &str, cursor_x: usize) -> u16 {
    if !line.is_empty() && cursor_x < line.chars().count() {
        line.chars()
            .nth(cursor_x)
            .and_then(UnicodeWidthChar::width)
            .unwrap_or(0) as u16
    } else {
        0
    }
}

pub(super) fn idle_refresh_deadline(now: Instant) -> Instant {
    now + IDLE_REFRESH_INTERVAL
}

pub(super) fn idle_wait_duration(deadline: Instant, now: Instant) -> Duration {
    deadline.saturating_duration_since(now)
}

pub(super) fn should_suppress_leaf_tasks_after_command(kind: CommandKind) -> bool {
    matches!(
        kind,
        CommandKind::NewProject
            | CommandKind::UnplannedProject
            | CommandKind::Tree
            | CommandKind::Leaves
            | CommandKind::ShowAll
            | CommandKind::Tail
            | CommandKind::Today
            | CommandKind::Calendar
            | CommandKind::Band
            | CommandKind::DeferRoutines
            | CommandKind::Flatten
            | CommandKind::Pack
    )
}

pub(super) fn render_prompt(
    stdout: &mut dyn SchronuWriter,
    header: &str,
    line: &str,
    cursor_x: usize,
) {
    write!(
        stdout,
        "{}{}{}{}",
        termion::cursor::Left(MAX_COL),
        termion::clear::CurrentLine,
        header,
        line
    )
    .unwrap();
    let width = get_width_for_rerender(header, line, cursor_x);
    write!(
        stdout,
        "{}{}",
        termion::cursor::Left(MAX_COL),
        termion::cursor::Right(width)
    )
    .unwrap();
    stdout.flush().unwrap();
}

fn clear_screen(stdout: &mut dyn SchronuWriter) {
    write!(
        stdout,
        "{}{}",
        termion::clear::All,
        termion::cursor::Goto(1, 1)
    )
    .unwrap();
}

pub(super) fn run<R, E>(
    initial_now: DateTime<Local>,
    mut handle_event: impl FnMut(&mut dyn SchronuWriter, DriverEvent<'_>) -> DriverOutcome<R, E>,
) -> Result<(), E>
where
    R: Display,
    E: Display,
{
    let mut stdout: termion::raw::RawTerminal<std::io::Stdout> = stdout().into_raw_mode().unwrap();
    write!(stdout, "{}", termion::cursor::BlinkingBar).unwrap();
    stdout.flush().unwrap();

    let header = "schronu> ";
    let mut line = String::new();
    let mut cursor_x = 0;
    clear_screen(&mut stdout);
    expect_continue(
        handle_event(&mut stdout, DriverEvent::RenderScreen { now: initial_now }),
        "render screen",
    );
    render_prompt(&mut stdout, header, &line, cursor_x);

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
    'input: loop {
        let wait_duration = idle_wait_duration(next_refresh_at, Instant::now());
        let key = match receive_input(&key_receiver, wait_duration) {
            ReceivedInput::Key(key) => {
                next_refresh_at = idle_refresh_deadline(Instant::now());
                key
            }
            ReceivedInput::ReadError(error) => {
                loop_error_opt = Some(expect_fatal(
                    handle_event(&mut stdout, DriverEvent::InputRead(error)),
                    "input read",
                ));
                break;
            }
            ReceivedInput::Refresh => {
                match handle_event(&mut stdout, DriverEvent::Refresh) {
                    DriverOutcome::Continue => {}
                    DriverOutcome::Retry(error) => {
                        writeln_newline(&mut stdout, &format!("[Error] {error}")).unwrap();
                        render_prompt(&mut stdout, header, &line, cursor_x);
                        next_refresh_at = idle_refresh_deadline(Instant::now());
                        continue;
                    }
                    DriverOutcome::Fatal(error) => {
                        loop_error_opt = Some(error);
                        break;
                    }
                    _ => unreachable!("refresh event returned an invalid outcome"),
                }
                clear_screen(&mut stdout);
                expect_continue(
                    handle_event(&mut stdout, DriverEvent::RenderScreen { now: Local::now() }),
                    "render screen",
                );
                render_prompt(&mut stdout, header, &line, cursor_x);
                next_refresh_at = idle_refresh_deadline(Instant::now());
                continue;
            }
            ReceivedInput::Disconnected => {
                loop_error_opt = Some(expect_fatal(
                    handle_event(&mut stdout, DriverEvent::InputDisconnected),
                    "input disconnected",
                ));
                break;
            }
        };

        match control_key(&key, line.is_empty()) {
            Some(ControlKey::Exit) => match handle_event(&mut stdout, DriverEvent::Exit) {
                DriverOutcome::Exit => break,
                DriverOutcome::Retry(error) => {
                    writeln_newline(&mut stdout, &format!("[Error] {error}")).unwrap();
                    render_prompt(&mut stdout, header, &line, cursor_x);
                }
                DriverOutcome::Fatal(error) => {
                    loop_error_opt = Some(error);
                    break;
                }
                DriverOutcome::Continue => {
                    render_prompt(&mut stdout, header, &line, cursor_x);
                }
                _ => unreachable!("exit event returned an invalid outcome"),
            },
            Some(ControlKey::Interrupted) => {
                loop_error_opt = Some(expect_fatal(
                    handle_event(&mut stdout, DriverEvent::Interrupted),
                    "interrupted",
                ));
                break;
            }
            Some(ControlKey::Submit) => {
                match handle_event(&mut stdout, DriverEvent::Submit { line: &line }) {
                    DriverOutcome::Submitted => {}
                    DriverOutcome::Fatal(error) => {
                        loop_error_opt = Some(error);
                        break 'input;
                    }
                    DriverOutcome::Retry(error) => {
                        writeln_newline(&mut stdout, &format!("[Error] {error}")).unwrap();
                        render_prompt(&mut stdout, header, &line, cursor_x);
                        continue;
                    }
                    _ => unreachable!("submit event returned an invalid outcome"),
                }
                reset_submitted_line(&mut line, &mut cursor_x);
                render_prompt(&mut stdout, header, &line, cursor_x);
            }
            None => match key {
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
                    render_prompt(&mut stdout, header, &line, cursor_x);
                }
                Key::Ctrl('e') => {
                    while get_forward_width(&line, cursor_x) > 0 {
                        let width = get_forward_width(&line, cursor_x);
                        cursor_x += 1;
                        write!(stdout, "{}", termion::cursor::Right(width)).unwrap();
                    }
                    stdout.flush().unwrap();
                }
                Key::Ctrl('u') => {
                    cursor_x = 0;
                    line.clear();
                    render_prompt(&mut stdout, header, &line, cursor_x);
                }
                Key::Ctrl('k') => {
                    line = line.chars().take(cursor_x).collect();
                    render_prompt(&mut stdout, header, &line, cursor_x);
                }
                Key::Backspace | Key::Ctrl('h') => {
                    if let Some(byte_offset) = get_byte_offset_for_deletion(&line, cursor_x) {
                        line.remove(byte_offset);
                        cursor_x -= 1;
                    }
                    render_prompt(&mut stdout, header, &line, cursor_x);
                }
                Key::Char(character) => {
                    let byte_offset = get_byte_offset_for_insert(&line, cursor_x);
                    line.insert(byte_offset, character);
                    cursor_x += 1;
                    write!(stdout, "{character}").unwrap();
                    render_prompt(&mut stdout, header, &line, cursor_x);
                }
                _ => {}
            },
        }
    }

    write!(stdout, "{}", termion::clear::CurrentLine).unwrap();
    println!("{}{}{}", style::Bold, line, style::Reset);
    writeln!(stdout, "{}", termion::cursor::SteadyBlock).unwrap();
    match loop_error_opt {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestWriter(Vec<u8>);

    impl Write for TestWriter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl SchronuWriter for TestWriter {
        fn writeln_newline(&mut self, message: &str) -> std::io::Result<()> {
            writeln!(self, "{message}")
        }
    }

    #[test]
    fn render_prompt_restores_cursor_after_multibyte_input() {
        let mut stdout = TestWriter::default();
        render_prompt(&mut stdout, "schronu> ", "あいう", 1);
        let actual = String::from_utf8(stdout.0).unwrap();
        let expected = format!(
            "{}{}schronu> あいう{}{}",
            termion::cursor::Left(MAX_COL),
            termion::clear::CurrentLine,
            termion::cursor::Left(MAX_COL),
            termion::cursor::Right(11),
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn control_keys_map_to_driver_events() {
        assert_eq!(
            control_key(&Key::Ctrl('c'), true),
            Some(ControlKey::Interrupted)
        );
        assert_eq!(control_key(&Key::Ctrl('d'), true), Some(ControlKey::Exit));
        assert_eq!(control_key(&Key::Ctrl('d'), false), None);
        assert_eq!(
            control_key(&Key::Char('\n'), true),
            Some(ControlKey::Submit)
        );
        assert_eq!(control_key(&Key::Ctrl('m'), true), Some(ControlKey::Submit));
    }

    #[test]
    fn receiver_maps_key_read_error_disconnect_and_refresh() {
        let (sender, receiver) = mpsc::channel();
        sender.send(Ok(Key::Ctrl('c'))).unwrap();
        assert!(matches!(
            receive_input(&receiver, Duration::ZERO),
            ReceivedInput::Key(Key::Ctrl('c'))
        ));

        sender
            .send(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "read failure",
            )))
            .unwrap();
        assert!(matches!(
            receive_input(&receiver, Duration::ZERO),
            ReceivedInput::ReadError(error) if error.kind() == std::io::ErrorKind::BrokenPipe
        ));
        assert!(matches!(
            receive_input(&receiver, Duration::ZERO),
            ReceivedInput::Refresh
        ));
        drop(sender);
        assert!(matches!(
            receive_input(&receiver, Duration::ZERO),
            ReceivedInput::Disconnected
        ));
    }

    #[test]
    fn successful_submit_resets_line_and_cursor() {
        let mut line = String::from("見");
        let mut cursor_x = 1;
        reset_submitted_line(&mut line, &mut cursor_x);
        assert!(line.is_empty());
        assert_eq!(cursor_x, 0);
    }

    #[test]
    fn event_specific_outcomes_preserve_fatal_error() {
        expect_continue::<(), ()>(DriverOutcome::Continue, "render screen");
        assert_eq!(
            expect_fatal::<(), _>(DriverOutcome::Fatal("fatal"), "input read"),
            "fatal"
        );
    }

    #[test]
    #[should_panic(expected = "input read event returned an invalid outcome")]
    fn event_specific_outcomes_reject_invalid_variant() {
        expect_fatal::<(), ()>(DriverOutcome::Continue, "input read");
    }
}
