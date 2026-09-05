use super::command::CommandKind;
use super::renderer::{writeln_newline, SchronuWriter, MAX_COL};
use chrono::{DateTime, Local};
use std::fmt::Display;
use std::io::stdout;
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

#[derive(Debug)]
pub(super) enum InteractiveIoError {
    RawMode(std::io::Error),
    Output(std::io::Error),
}

impl std::fmt::Display for InteractiveIoError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RawMode(error) => write!(formatter, "failed to initialize raw mode: {error}"),
            Self::Output(error) => write!(formatter, "interactive output failed: {error}"),
        }
    }
}

impl std::error::Error for InteractiveIoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RawMode(error) | Self::Output(error) => Some(error),
        }
    }
}

#[derive(Debug)]
pub(super) enum DriverRunError<E> {
    Io(InteractiveIoError),
    Handler(E),
}

impl<E: Display> std::fmt::Display for DriverRunError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Handler(error) => error.fmt(formatter),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for DriverRunError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Handler(error) => Some(error),
        }
    }
}

fn expect_continue<R, E>(outcome: DriverOutcome<R, E>, event: &str) -> Result<(), E> {
    match outcome {
        DriverOutcome::Continue => Ok(()),
        DriverOutcome::Fatal(error) => Err(error),
        _ => unreachable!("{event} event returned an invalid outcome"),
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

pub(super) enum ReceivedInput {
    Key(Key),
    Refresh,
    ReadError(std::io::Error),
    Disconnected,
}

pub(super) trait InputSource {
    fn receive(&mut self, wait_duration: Duration) -> ReceivedInput;
}

struct ChannelInput<'a> {
    receiver: &'a Receiver<std::io::Result<Key>>,
}

pub(super) trait TerminalFactory {
    fn open_terminal(&mut self) -> std::io::Result<Box<dyn SchronuWriter>>;
}

struct SystemTerminalFactory;

impl TerminalFactory for SystemTerminalFactory {
    fn open_terminal(&mut self) -> std::io::Result<Box<dyn SchronuWriter>> {
        let terminal: termion::raw::RawTerminal<std::io::Stdout> = stdout().into_raw_mode()?;
        Ok(Box::new(terminal))
    }
}

struct TerminalGuard {
    terminal: Box<dyn SchronuWriter>,
}

impl TerminalGuard {
    fn open(factory: &mut dyn TerminalFactory) -> Result<Self, InteractiveIoError> {
        factory
            .open_terminal()
            .map(|terminal| Self { terminal })
            .map_err(InteractiveIoError::RawMode)
    }

    fn writer(&mut self) -> &mut dyn SchronuWriter {
        self.terminal.as_mut()
    }
}

impl InputSource for ChannelInput<'_> {
    fn receive(&mut self, wait_duration: Duration) -> ReceivedInput {
        receive_input(self.receiver, wait_duration)
    }
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
) -> Result<(), std::io::Error> {
    write!(
        stdout,
        "{}{}{}{}",
        termion::cursor::Left(MAX_COL),
        termion::clear::CurrentLine,
        header,
        line
    )?;
    let width = get_width_for_rerender(header, line, cursor_x);
    write!(
        stdout,
        "{}{}",
        termion::cursor::Left(MAX_COL),
        termion::cursor::Right(width)
    )?;
    stdout.flush()
}

fn clear_screen(stdout: &mut dyn SchronuWriter) -> Result<(), std::io::Error> {
    write!(
        stdout,
        "{}{}",
        termion::clear::All,
        termion::cursor::Goto(1, 1)
    )
}

pub(super) fn run<R, E>(
    initial_now: DateTime<Local>,
    handle_event: impl FnMut(&mut dyn SchronuWriter, DriverEvent<'_>) -> DriverOutcome<R, E>,
) -> Result<(), DriverRunError<E>>
where
    R: Display,
    E: Display,
{
    let (key_sender, key_receiver) = mpsc::channel();
    thread::spawn(move || {
        for key_result in std::io::stdin().keys() {
            if key_sender.send(key_result).is_err() {
                break;
            }
        }
    });

    let mut input = ChannelInput {
        receiver: &key_receiver,
    };
    run_with_terminal_factory(
        initial_now,
        &mut SystemTerminalFactory,
        &mut input,
        handle_event,
    )
}

pub(super) fn run_with_terminal_factory<R, E>(
    initial_now: DateTime<Local>,
    terminal_factory: &mut dyn TerminalFactory,
    input: &mut dyn InputSource,
    handle_event: impl FnMut(&mut dyn SchronuWriter, DriverEvent<'_>) -> DriverOutcome<R, E>,
) -> Result<(), DriverRunError<E>>
where
    R: Display,
    E: Display,
{
    let mut terminal = TerminalGuard::open(terminal_factory).map_err(DriverRunError::Io)?;
    run_driver(initial_now, terminal.writer(), input, handle_event)
}

fn run_driver<R, E>(
    initial_now: DateTime<Local>,
    stdout: &mut dyn SchronuWriter,
    input: &mut dyn InputSource,
    mut handle_event: impl FnMut(&mut dyn SchronuWriter, DriverEvent<'_>) -> DriverOutcome<R, E>,
) -> Result<(), DriverRunError<E>>
where
    R: Display,
    E: Display,
{
    write!(stdout, "{}", termion::cursor::BlinkingBar)
        .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
    stdout
        .flush()
        .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;

    let header = "schronu> ";
    let mut line = String::new();
    let mut cursor_x = 0;
    clear_screen(stdout).map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
    expect_continue(
        handle_event(stdout, DriverEvent::RenderScreen { now: initial_now }),
        "render screen",
    )
    .map_err(DriverRunError::Handler)?;
    render_prompt(stdout, header, &line, cursor_x)
        .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;

    let mut next_refresh_at = idle_refresh_deadline(Instant::now());
    let mut loop_error_opt = None;
    'input: loop {
        let wait_duration = idle_wait_duration(next_refresh_at, Instant::now());
        let key = match input.receive(wait_duration) {
            ReceivedInput::Key(key) => {
                next_refresh_at = idle_refresh_deadline(Instant::now());
                key
            }
            ReceivedInput::ReadError(error) => {
                loop_error_opt = Some(expect_fatal(
                    handle_event(stdout, DriverEvent::InputRead(error)),
                    "input read",
                ));
                break;
            }
            ReceivedInput::Refresh => {
                match handle_event(stdout, DriverEvent::Refresh) {
                    DriverOutcome::Continue => {}
                    DriverOutcome::Retry(error) => {
                        writeln_newline(stdout, &format!("[Error] {error}")).map_err(|error| {
                            DriverRunError::Io(InteractiveIoError::Output(error))
                        })?;
                        render_prompt(stdout, header, &line, cursor_x).map_err(|error| {
                            DriverRunError::Io(InteractiveIoError::Output(error))
                        })?;
                        next_refresh_at = idle_refresh_deadline(Instant::now());
                        continue;
                    }
                    DriverOutcome::Fatal(error) => {
                        loop_error_opt = Some(error);
                        break;
                    }
                    _ => unreachable!("refresh event returned an invalid outcome"),
                }
                clear_screen(stdout)
                    .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
                expect_continue(
                    handle_event(stdout, DriverEvent::RenderScreen { now: Local::now() }),
                    "render screen",
                )
                .map_err(DriverRunError::Handler)?;
                render_prompt(stdout, header, &line, cursor_x)
                    .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
                next_refresh_at = idle_refresh_deadline(Instant::now());
                continue;
            }
            ReceivedInput::Disconnected => {
                loop_error_opt = Some(expect_fatal(
                    handle_event(stdout, DriverEvent::InputDisconnected),
                    "input disconnected",
                ));
                break;
            }
        };

        match control_key(&key, line.is_empty()) {
            Some(ControlKey::Exit) => match handle_event(stdout, DriverEvent::Exit) {
                DriverOutcome::Exit => break,
                DriverOutcome::Retry(error) => {
                    writeln_newline(stdout, &format!("[Error] {error}"))
                        .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
                    render_prompt(stdout, header, &line, cursor_x)
                        .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
                }
                DriverOutcome::Fatal(error) => {
                    loop_error_opt = Some(error);
                    break;
                }
                DriverOutcome::Continue => {
                    render_prompt(stdout, header, &line, cursor_x)
                        .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
                }
                _ => unreachable!("exit event returned an invalid outcome"),
            },
            Some(ControlKey::Interrupted) => {
                loop_error_opt = Some(expect_fatal(
                    handle_event(stdout, DriverEvent::Interrupted),
                    "interrupted",
                ));
                break;
            }
            Some(ControlKey::Submit) => {
                match handle_event(stdout, DriverEvent::Submit { line: &line }) {
                    DriverOutcome::Submitted => {}
                    DriverOutcome::Fatal(error) => {
                        loop_error_opt = Some(error);
                        break 'input;
                    }
                    DriverOutcome::Retry(error) => {
                        writeln_newline(stdout, &format!("[Error] {error}")).map_err(|error| {
                            DriverRunError::Io(InteractiveIoError::Output(error))
                        })?;
                        render_prompt(stdout, header, &line, cursor_x).map_err(|error| {
                            DriverRunError::Io(InteractiveIoError::Output(error))
                        })?;
                        continue;
                    }
                    _ => unreachable!("submit event returned an invalid outcome"),
                }
                reset_submitted_line(&mut line, &mut cursor_x);
                render_prompt(stdout, header, &line, cursor_x)
                    .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
            }
            None => match key {
                Key::Left | Key::Ctrl('b') => {
                    let width = backward_width(&line, cursor_x);
                    if width > 0 {
                        cursor_x -= 1;
                        write!(stdout, "{}", termion::cursor::Left(width)).map_err(|error| {
                            DriverRunError::Io(InteractiveIoError::Output(error))
                        })?;
                        stdout.flush().map_err(|error| {
                            DriverRunError::Io(InteractiveIoError::Output(error))
                        })?;
                    }
                }
                Key::Right | Key::Ctrl('f') => {
                    let width = get_forward_width(&line, cursor_x);
                    if width > 0 {
                        cursor_x += 1;
                        write!(stdout, "{}", termion::cursor::Right(width)).map_err(|error| {
                            DriverRunError::Io(InteractiveIoError::Output(error))
                        })?;
                        stdout.flush().map_err(|error| {
                            DriverRunError::Io(InteractiveIoError::Output(error))
                        })?;
                    }
                }
                Key::Ctrl('a') => {
                    cursor_x = 0;
                    render_prompt(stdout, header, &line, cursor_x)
                        .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
                }
                Key::Ctrl('e') => {
                    while get_forward_width(&line, cursor_x) > 0 {
                        let width = get_forward_width(&line, cursor_x);
                        cursor_x += 1;
                        write!(stdout, "{}", termion::cursor::Right(width)).map_err(|error| {
                            DriverRunError::Io(InteractiveIoError::Output(error))
                        })?;
                    }
                    stdout
                        .flush()
                        .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
                }
                Key::Ctrl('u') => {
                    cursor_x = 0;
                    line.clear();
                    render_prompt(stdout, header, &line, cursor_x)
                        .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
                }
                Key::Ctrl('k') => {
                    line = line.chars().take(cursor_x).collect();
                    render_prompt(stdout, header, &line, cursor_x)
                        .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
                }
                Key::Backspace | Key::Ctrl('h') => {
                    if let Some(byte_offset) = get_byte_offset_for_deletion(&line, cursor_x) {
                        line.remove(byte_offset);
                        cursor_x -= 1;
                    }
                    render_prompt(stdout, header, &line, cursor_x)
                        .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
                }
                Key::Char(character) => {
                    let byte_offset = get_byte_offset_for_insert(&line, cursor_x);
                    line.insert(byte_offset, character);
                    cursor_x += 1;
                    write!(stdout, "{character}")
                        .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
                    render_prompt(stdout, header, &line, cursor_x)
                        .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error)))?;
                }
                _ => {}
            },
        }
    }

    let exit_render_result = (|| {
        write!(stdout, "{}", termion::clear::CurrentLine)?;
        writeln!(stdout, "{}{}{}", style::Bold, line, style::Reset)?;
        writeln!(stdout, "{}", termion::cursor::SteadyBlock)?;
        stdout.flush()
    })();
    match loop_error_opt {
        Some(error) => Err(DriverRunError::Handler(error)),
        None => exit_render_result
            .map_err(|error| DriverRunError::Io(InteractiveIoError::Output(error))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::io::Write;
    use std::rc::Rc;

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

    struct ScriptedInput(VecDeque<ReceivedInput>);

    impl InputSource for ScriptedInput {
        fn receive(&mut self, _wait_duration: Duration) -> ReceivedInput {
            self.0.pop_front().expect("scripted input must not end")
        }
    }

    impl ScriptedInput {
        fn new(inputs: impl IntoIterator<Item = ReceivedInput>) -> Self {
            Self(inputs.into_iter().collect())
        }
    }

    enum FailurePoint {
        WriteContaining { needle: String, occurrence: usize },
        LineContaining(String),
        Flush(usize),
    }

    struct FailureWriter {
        failure: FailurePoint,
        error_kind: std::io::ErrorKind,
        matching_writes: usize,
        flushes: usize,
        output: Vec<u8>,
    }

    impl FailureWriter {
        fn write_containing(needle: impl Into<String>, occurrence: usize) -> Self {
            Self {
                failure: FailurePoint::WriteContaining {
                    needle: needle.into(),
                    occurrence,
                },
                error_kind: std::io::ErrorKind::PermissionDenied,
                matching_writes: 0,
                flushes: 0,
                output: Vec::new(),
            }
        }

        fn line_containing(needle: impl Into<String>) -> Self {
            Self {
                failure: FailurePoint::LineContaining(needle.into()),
                error_kind: std::io::ErrorKind::PermissionDenied,
                matching_writes: 0,
                flushes: 0,
                output: Vec::new(),
            }
        }

        fn flush(number: usize) -> Self {
            Self {
                failure: FailurePoint::Flush(number),
                error_kind: std::io::ErrorKind::PermissionDenied,
                matching_writes: 0,
                flushes: 0,
                output: Vec::new(),
            }
        }

        fn permission_denied() -> std::io::Error {
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected output failure",
            )
        }

        fn with_error_kind(mut self, error_kind: std::io::ErrorKind) -> Self {
            self.error_kind = error_kind;
            self
        }

        fn injected_error(&self) -> std::io::Error {
            std::io::Error::new(self.error_kind, "injected output failure")
        }
    }

    impl Write for FailureWriter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            if let FailurePoint::WriteContaining { needle, occurrence } = &self.failure {
                self.output.extend_from_slice(buffer);
                let occurrences = String::from_utf8_lossy(&self.output)
                    .matches(needle)
                    .count();
                if occurrences >= *occurrence && self.matching_writes < *occurrence {
                    return Err(self.injected_error());
                }
                self.matching_writes = occurrences;
            }
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes += 1;
            if matches!(self.failure, FailurePoint::Flush(number) if self.flushes == number) {
                return Err(self.injected_error());
            }
            Ok(())
        }
    }

    impl SchronuWriter for FailureWriter {
        fn writeln_newline(&mut self, message: &str) -> std::io::Result<()> {
            if matches!(&self.failure, FailurePoint::LineContaining(needle) if message.contains(needle))
            {
                return Err(self.injected_error());
            }
            Ok(())
        }
    }

    fn assert_output_failure<E: std::fmt::Debug>(result: Result<(), DriverRunError<E>>) {
        match result {
            Err(DriverRunError::Io(InteractiveIoError::Output(error))) => {
                assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
                assert!(std::error::Error::source(&InteractiveIoError::Output(error)).is_some());
            }
            other => panic!("expected an output failure, got {other:?}"),
        }
    }

    fn run_script(
        writer: &mut dyn SchronuWriter,
        inputs: impl IntoIterator<Item = ReceivedInput>,
        mut outcome_for: impl FnMut(&DriverEvent<'_>) -> DriverOutcome<&'static str, &'static str>,
    ) -> Result<(), DriverRunError<&'static str>> {
        let mut input = ScriptedInput::new(inputs);
        run_driver(Local::now(), writer, &mut input, |_, event| {
            outcome_for(&event)
        })
    }

    #[test]
    fn prompt_output_failure_is_returned() {
        let mut writer =
            FailureWriter::write_containing(termion::clear::CurrentLine.to_string(), 1);
        let result =
            run_script(
                &mut writer,
                [ReceivedInput::Key(Key::Ctrl('d'))],
                |event| match event {
                    DriverEvent::RenderScreen { .. } => DriverOutcome::Continue,
                    DriverEvent::Exit => DriverOutcome::Exit,
                    _ => unreachable!(),
                },
            );
        assert_output_failure(result);
    }

    #[test]
    fn refresh_output_failure_is_returned() {
        let mut writer = FailureWriter::write_containing(termion::clear::All.to_string(), 2);
        let result = run_script(
            &mut writer,
            [ReceivedInput::Refresh, ReceivedInput::Key(Key::Ctrl('d'))],
            |event| match event {
                DriverEvent::RenderScreen { .. } | DriverEvent::Refresh => DriverOutcome::Continue,
                DriverEvent::Exit => DriverOutcome::Exit,
                _ => unreachable!(),
            },
        );
        assert_output_failure(result);
    }

    #[test]
    fn cursor_output_failure_is_returned() {
        let mut writer = FailureWriter::write_containing(termion::cursor::Left(1).to_string(), 1);
        let result = run_script(
            &mut writer,
            [
                ReceivedInput::Key(Key::Char('a')),
                ReceivedInput::Key(Key::Left),
                ReceivedInput::Key(Key::Ctrl('d')),
            ],
            |event| match event {
                DriverEvent::RenderScreen { .. } => DriverOutcome::Continue,
                DriverEvent::Exit => DriverOutcome::Exit,
                _ => unreachable!(),
            },
        );
        assert_output_failure(result);
    }

    #[test]
    fn retry_error_output_failure_is_returned() {
        let mut writer = FailureWriter::line_containing("[Error] retry");
        let result = run_script(
            &mut writer,
            [ReceivedInput::Refresh, ReceivedInput::Key(Key::Ctrl('d'))],
            |event| match event {
                DriverEvent::RenderScreen { .. } => DriverOutcome::Continue,
                DriverEvent::Refresh => DriverOutcome::Retry("retry"),
                DriverEvent::Exit => DriverOutcome::Exit,
                _ => unreachable!(),
            },
        );
        assert_output_failure(result);
    }

    #[test]
    fn submitted_prompt_output_failure_is_returned() {
        let mut writer = FailureWriter::write_containing("schronu> ", 2);
        let result = run_script(
            &mut writer,
            [
                ReceivedInput::Key(Key::Char('a')),
                ReceivedInput::Key(Key::Char('\n')),
                ReceivedInput::Key(Key::Ctrl('d')),
            ],
            |event| match event {
                DriverEvent::RenderScreen { .. } => DriverOutcome::Continue,
                DriverEvent::Submit { .. } => DriverOutcome::Submitted,
                DriverEvent::Exit => DriverOutcome::Exit,
                _ => unreachable!(),
            },
        );
        assert_output_failure(result);
    }

    #[test]
    fn exit_output_failure_is_returned() {
        let mut writer =
            FailureWriter::write_containing(termion::clear::CurrentLine.to_string(), 2);
        let result =
            run_script(
                &mut writer,
                [ReceivedInput::Key(Key::Ctrl('d'))],
                |event| match event {
                    DriverEvent::RenderScreen { .. } => DriverOutcome::Continue,
                    DriverEvent::Exit => DriverOutcome::Exit,
                    _ => unreachable!(),
                },
            );
        assert_output_failure(result);
    }

    #[test]
    fn flush_failure_is_returned() {
        let mut writer = FailureWriter::flush(1);
        let result =
            run_script(
                &mut writer,
                [ReceivedInput::Key(Key::Ctrl('d'))],
                |event| match event {
                    DriverEvent::RenderScreen { .. } => DriverOutcome::Continue,
                    DriverEvent::Exit => DriverOutcome::Exit,
                    _ => unreachable!(),
                },
            );
        assert_output_failure(result);
    }

    #[test]
    fn render_handler_failure_is_returned_without_panic() {
        let mut writer = TestWriter::default();
        let result = run_script(&mut writer, [], |event| match event {
            DriverEvent::RenderScreen { .. } => DriverOutcome::Fatal("render failed"),
            _ => unreachable!(),
        });

        assert!(matches!(
            result,
            Err(DriverRunError::Handler("render failed"))
        ));
    }

    #[test]
    fn handler_failure_is_not_suppressed_by_broken_pipe_during_exit_render() {
        let mut writer =
            FailureWriter::write_containing(termion::clear::CurrentLine.to_string(), 2)
                .with_error_kind(std::io::ErrorKind::BrokenPipe);
        let result =
            run_script(
                &mut writer,
                [ReceivedInput::Key(Key::Ctrl('c'))],
                |event| match event {
                    DriverEvent::RenderScreen { .. } => DriverOutcome::Continue,
                    DriverEvent::Interrupted => DriverOutcome::Fatal("handler failed"),
                    _ => unreachable!(),
                },
            );

        assert!(matches!(
            result,
            Err(DriverRunError::Handler("handler failed"))
        ));
    }

    struct FakeTerminalFactory {
        terminal: Option<std::io::Result<Box<dyn SchronuWriter>>>,
    }

    impl TerminalFactory for FakeTerminalFactory {
        fn open_terminal(&mut self) -> std::io::Result<Box<dyn SchronuWriter>> {
            self.terminal
                .take()
                .expect("fake terminal factory must be called once")
        }
    }

    struct DropTrackingWriter {
        drops: Rc<Cell<usize>>,
        fail_output: bool,
    }

    impl Drop for DropTrackingWriter {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }

    impl Write for DropTrackingWriter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            if self.fail_output {
                Err(FailureWriter::permission_denied())
            } else {
                Ok(buffer.len())
            }
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl SchronuWriter for DropTrackingWriter {
        fn writeln_newline(&mut self, _message: &str) -> std::io::Result<()> {
            if self.fail_output {
                Err(FailureWriter::permission_denied())
            } else {
                Ok(())
            }
        }
    }

    fn tracking_factory(drops: &Rc<Cell<usize>>, fail_output: bool) -> FakeTerminalFactory {
        FakeTerminalFactory {
            terminal: Some(Ok(Box::new(DropTrackingWriter {
                drops: Rc::clone(drops),
                fail_output,
            }))),
        }
    }

    #[test]
    fn raw_mode_initialization_failure_preserves_source() {
        let mut factory = FakeTerminalFactory {
            terminal: Some(Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "raw mode unavailable",
            ))),
        };
        let mut input = ScriptedInput::new([]);
        let result = run_with_terminal_factory(Local::now(), &mut factory, &mut input, |_, _| {
            DriverOutcome::<&str, &str>::Continue
        });

        match result {
            Err(DriverRunError::Io(InteractiveIoError::RawMode(error))) => {
                assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
                assert!(std::error::Error::source(&InteractiveIoError::RawMode(error)).is_some());
            }
            other => panic!("expected raw mode initialization failure, got {other:?}"),
        }
    }

    #[test]
    fn terminal_guard_restores_on_normal_exit() {
        let drops = Rc::new(Cell::new(0));
        let mut factory = tracking_factory(&drops, false);
        let mut input = ScriptedInput::new([ReceivedInput::Key(Key::Ctrl('d'))]);

        let result = run_with_terminal_factory(
            Local::now(),
            &mut factory,
            &mut input,
            |_, event| match event {
                DriverEvent::RenderScreen { .. } => DriverOutcome::<&str, &str>::Continue,
                DriverEvent::Exit => DriverOutcome::Exit,
                _ => unreachable!(),
            },
        );

        assert!(result.is_ok());
        assert_eq!(drops.get(), 1);
    }

    #[test]
    fn terminal_guard_restores_on_handler_failure() {
        let drops = Rc::new(Cell::new(0));
        let mut factory = tracking_factory(&drops, false);
        let mut input = ScriptedInput::new([ReceivedInput::Key(Key::Ctrl('c'))]);

        let result = run_with_terminal_factory(
            Local::now(),
            &mut factory,
            &mut input,
            |_, event| match event {
                DriverEvent::RenderScreen { .. } => DriverOutcome::<&str, &str>::Continue,
                DriverEvent::Interrupted => DriverOutcome::Fatal("handler failed"),
                _ => unreachable!(),
            },
        );

        assert!(matches!(
            result,
            Err(DriverRunError::Handler("handler failed"))
        ));
        assert_eq!(drops.get(), 1);
    }

    #[test]
    fn terminal_guard_restores_on_output_failure() {
        let drops = Rc::new(Cell::new(0));
        let mut factory = tracking_factory(&drops, true);
        let mut input = ScriptedInput::new([]);

        let result = run_with_terminal_factory(Local::now(), &mut factory, &mut input, |_, _| {
            DriverOutcome::<&str, &str>::Continue
        });

        assert_output_failure(result);
        assert_eq!(drops.get(), 1);
    }

    #[test]
    fn render_prompt_restores_cursor_after_multibyte_input() {
        let mut stdout = TestWriter::default();
        render_prompt(&mut stdout, "schronu> ", "あいう", 1).unwrap();
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
    fn submit_retryは入力と送信時のカーソル位置を保持する() {
        let mut writer = TestWriter(Vec::new());
        let mut submitted_lines = Vec::new();
        let result = run_script(
            &mut writer,
            [
                ReceivedInput::Key(Key::Char('見')),
                ReceivedInput::Key(Key::Char('積')),
                ReceivedInput::Key(Key::Left),
                ReceivedInput::Key(Key::Char('\n')),
                ReceivedInput::Key(Key::Backspace),
                ReceivedInput::Key(Key::Char('\n')),
                ReceivedInput::Key(Key::Ctrl('d')),
            ],
            |event| match event {
                DriverEvent::RenderScreen { .. } => DriverOutcome::Continue,
                DriverEvent::Submit { line } => {
                    submitted_lines.push(line.to_string());
                    if submitted_lines.len() == 1 {
                        DriverOutcome::Retry("retry")
                    } else {
                        DriverOutcome::Submitted
                    }
                }
                DriverEvent::Exit => DriverOutcome::Exit,
                _ => unreachable!(),
            },
        );

        assert!(result.is_ok());
        assert_eq!(submitted_lines, ["見積", "積"]);
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
        expect_continue::<(), ()>(DriverOutcome::Continue, "render screen").unwrap();
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
