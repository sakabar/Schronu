use super::renderer::{
    format_spreadsheet_task_row, render_display_model, DisplayFragment, DisplayModel,
    DisplayRecorder, ErrorCapturingWriter, MessageLevel, SchronuWriter, SpreadsheetTaskRow,
};
use std::io::Write;

#[test]
fn spreadsheet_task_rowはaからjの10列を既存cli形式で出力する() {
    let row = SpreadsheetTaskRow {
        rank: "0001",
        task_id: "11111111-1111-1111-1111-111111111111",
        icon: "!",
        remaining_time: "____-01:20",
        scheduled_time: "06/21(土)-18:40~19:20",
        priority: "0",
        estimated_minutes: "40",
        project_number: "01",
        category: "維",
        task_name: "夕食 の 準備",
    };

    let formatted = format_spreadsheet_task_row(&row);

    assert_eq!(
        formatted,
        "0001 11111111-1111-1111-1111-111111111111 ! ____-01:20 06/21(土)-18:40~19:20 0 40 01 維 夕食 の 準備"
    );
    let mut columns = formatted.split_whitespace();
    assert_eq!(columns.nth(8), Some("維"), "I列はcategory");
    assert_eq!(
        formatted.splitn(10, char::is_whitespace).nth(9),
        Some("夕食 の 準備"),
        "J列はtask_name"
    );
}

#[derive(Default)]
struct TraceWriter {
    operations: Vec<String>,
    flush_count: usize,
    supports_ansi_color: bool,
}

impl Write for TraceWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.operations
            .push(format!("raw:{}", String::from_utf8_lossy(buffer)));
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flush_count += 1;
        Ok(())
    }
}

impl SchronuWriter for TraceWriter {
    fn writeln_newline(&mut self, message: &str) -> std::io::Result<()> {
        self.operations.push(format!("newline:{message}"));
        Ok(())
    }

    fn supports_ansi_color(&self) -> bool {
        self.supports_ansi_color
    }
}

#[test]
fn display_modelはrawとwriter固有newlineとansiとflushの順序を保持する() {
    let mut recorder = DisplayRecorder::with_ansi_color(false);
    assert!(!recorder.supports_ansi_color());
    recorder.write_all(b"\x1b[31mraw").unwrap();
    recorder.writeln_newline("line").unwrap();
    recorder.write_all(b"tail").unwrap();
    recorder.flush().unwrap();

    assert_eq!(
        recorder.model().fragments(),
        &[
            DisplayFragment::Raw(b"\x1b[31mraw".to_vec()),
            DisplayFragment::Newline("line".to_string()),
            DisplayFragment::Raw(b"tail".to_vec()),
            DisplayFragment::Flush,
        ]
    );

    let mut writer = TraceWriter::default();
    render_display_model(&mut writer, recorder.model()).unwrap();
    assert_eq!(
        writer.operations,
        ["raw:\x1b[31mraw", "newline:line", "raw:tail"]
    );
    assert_eq!(writer.flush_count, 1);
}

#[test]
fn semantic_message_sequenceはlevelとwriter固有newlineの順序を保持する() {
    let display = DisplayModel::Sequence(vec![
        DisplayModel::Message {
            level: MessageLevel::Plain,
            text: "plain".to_string(),
        },
        DisplayModel::Message {
            level: MessageLevel::Info,
            text: "information".to_string(),
        },
        DisplayModel::Message {
            level: MessageLevel::Warn,
            text: "warning".to_string(),
        },
        DisplayModel::Message {
            level: MessageLevel::Critical,
            text: "critical".to_string(),
        },
        DisplayModel::Message {
            level: MessageLevel::Error,
            text: "failure".to_string(),
        },
    ]);
    let mut writer = TraceWriter::default();

    render_display_model(&mut writer, &display).unwrap();

    assert_eq!(
        writer.operations,
        [
            "newline:plain",
            "newline:[Info] information",
            "newline:[Warn] warning",
            "newline:[Critical] critical",
            "newline:[Error] failure",
        ]
    );
}

struct AlwaysFailWriter;

impl Write for AlwaysFailWriter {
    fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "first write failure",
        ))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::other("later flush failure"))
    }
}

impl SchronuWriter for AlwaysFailWriter {
    fn writeln_newline(&mut self, _message: &str) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "later newline failure",
        ))
    }
}

#[test]
fn error_capturing_writerは最初のio_errorを保持して後続fragmentを処理する() {
    let mut inner = AlwaysFailWriter;
    let mut writer = ErrorCapturingWriter::new(&mut inner);
    writer.write_all(b"raw").unwrap();
    writer.writeln_newline("line").unwrap();
    writer.flush().unwrap();

    let error = writer.take_error().unwrap();
    assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    assert!(writer.take_error().is_none());
}

#[test]
fn command_errorはdisplay_modelを経由してrendererへ渡される() {
    let error = std::io::Error::other("command failed");
    let display = DisplayModel::newline(format!("[Error] {error}"));
    let mut writer = TraceWriter::default();

    render_display_model(&mut writer, &display).unwrap();

    assert_eq!(writer.operations, ["newline:[Error] command failed"]);

    let mut broken_writer = AlwaysFailWriter;
    let error = render_display_model(&mut broken_writer, &display).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert_eq!(error.to_string(), "later newline failure");
}
