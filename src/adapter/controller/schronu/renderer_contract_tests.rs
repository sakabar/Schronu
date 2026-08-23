use super::renderer::{
    format_spreadsheet_task_row, format_task_list_columns, render_display_model, task_list_columns,
    AncestorTreeRow, DebugTreeRow, DisplayFragment, DisplayModel, DisplayRecorder,
    ErrorCapturingWriter, LeafTreeRow, MessageLevel, SchronuWriter, SpreadsheetTaskRow,
    TaskCategoryWorkSeconds, TaskListDisplay, TaskListIconMode, TaskListRow, TaskListTaskRow,
    TreeDisplay,
};
use chrono::{Local, NaiveDate, TimeZone};
use schronu::entity::task::ProjectCategory;
use std::io::Write;
use uuid::Uuid;

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

#[test]
fn tree_displayはtyped_rowから既存のraw空行とwriter固有newlineを生成する() {
    let root_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let child_id = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
    let display = DisplayModel::Sequence(vec![
        DisplayModel::Tree(TreeDisplay::Debug {
            rows: vec![
                DebugTreeRow {
                    debug: "[ ] debug root".to_string(),
                },
                DebugTreeRow {
                    debug: "    [-] debug child".to_string(),
                },
            ],
        }),
        DisplayModel::Tree(TreeDisplay::Ancestors {
            rows: vec![
                AncestorTreeRow {
                    level: 0,
                    task_id: root_id,
                    first_available_date: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
                    estimated_minutes: 30,
                    name: "root".to_string(),
                },
                AncestorTreeRow {
                    level: 2,
                    task_id: child_id,
                    first_available_date: NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
                    estimated_minutes: 15,
                    name: "child".to_string(),
                },
            ],
        }),
        DisplayModel::Tree(TreeDisplay::Leaves {
            rows: vec![
                LeafTreeRow {
                    remaining_count: 2,
                    project_name: "project A".to_string(),
                    task_debug: "TaskAttr { name: \"first\" }".to_string(),
                },
                LeafTreeRow {
                    remaining_count: 1,
                    project_name: "project B".to_string(),
                    task_debug: "TaskAttr { name: \"second\" }".to_string(),
                },
            ],
        }),
    ]);
    let mut writer = TraceWriter::default();

    render_display_model(&mut writer, &display).unwrap();

    assert_eq!(
        writer.operations,
        [
            "raw:\n",
            "newline:[ ] debug root",
            "newline:    [-] debug child",
            "raw:\n",
            "raw:\n",
            "newline:11111111-1111-1111-1111-111111111111 [2026/08/23] 30m root",
            "newline:    `-- 22222222-2222-2222-2222-222222222222 [2026/08/24] 15m child",
            "newline:",
            "newline:2\tproject A\tTaskAttr { name: \"first\" }",
            "newline:1\tproject B\tTaskAttr { name: \"second\" }",
            "newline:",
        ]
    );
}

#[test]
fn task_list_displayはtyped_rowからa_j列とカテゴリ集計を既存順序で生成する() {
    let first_task_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let second_task_id = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
    let display = DisplayModel::TaskList(TaskListDisplay {
        rows: vec![
            TaskListRow::Task(TaskListTaskRow {
                rank: 1,
                task_id: first_task_id,
                icon: "!".to_string(),
                remaining_time: "____-01:20".to_string(),
                scheduled_start: Local.with_ymd_and_hms(2026, 8, 23, 9, 0, 0).unwrap(),
                scheduled_end: Local.with_ymd_and_hms(2026, 8, 23, 9, 40, 0).unwrap(),
                priority_rank: 0,
                estimated_minutes: 40,
                project_number_priority: 1,
                project_category: Some(ProjectCategory::Sustaining),
                task_name: "夕食 の 準備".to_string(),
                give_up_candidate: true,
            }),
            TaskListRow::Gap { minutes: 15 },
            TaskListRow::Message {
                text: "予定外の案内".to_string(),
            },
            TaskListRow::Task(TaskListTaskRow {
                rank: 2,
                task_id: second_task_id,
                icon: "/".to_string(),
                remaining_time: "____/__/__".to_string(),
                scheduled_start: Local.with_ymd_and_hms(2026, 8, 23, 10, 0, 0).unwrap(),
                scheduled_end: Local.with_ymd_and_hms(2026, 8, 23, 10, 5, 0).unwrap(),
                priority_rank: 3,
                estimated_minutes: 5,
                project_number_priority: 8,
                project_category: None,
                task_name: "短い task".to_string(),
                give_up_candidate: false,
            }),
        ],
        category_work_seconds: vec![
            TaskCategoryWorkSeconds {
                project_category: Some(ProjectCategory::Earning),
                seconds: 3600,
            },
            TaskCategoryWorkSeconds {
                project_category: Some(ProjectCategory::Sustaining),
                seconds: 0,
            },
            TaskCategoryWorkSeconds {
                project_category: Some(ProjectCategory::Recovery),
                seconds: 0,
            },
            TaskCategoryWorkSeconds {
                project_category: Some(ProjectCategory::Investment),
                seconds: 1800,
            },
            TaskCategoryWorkSeconds {
                project_category: Some(ProjectCategory::Consumption),
                seconds: 0,
            },
            TaskCategoryWorkSeconds {
                project_category: None,
                seconds: 1800,
            },
        ],
        category_denominator_seconds: 7200,
    });
    let mut writer = TraceWriter::default();

    render_display_model(&mut writer, &display).unwrap();

    assert_eq!(
        writer.operations,
        [
            "newline:0001 11111111-1111-1111-1111-111111111111 A ____-01:20 08/23(日)-09:00~09:40 0 40 01 維 夕食 の 準備",
            "newline:---- ------------------------------------ - ---------- --------------------- - -- -- 15分間の空き時間",
            "newline:予定外の案内",
            "newline:0002 22222222-2222-2222-2222-222222222222 / ____/__/__ 08/23(日)-10:00~10:05 3 05 08 _ 短い task",
            "newline:",
            "newline:予定カテゴリ: 獲得 1.0時間(50% | 50%) / 維持 0.0時間(0% | 50%) / 回復 0.0時間(0% | 50%) / 投資 0.5時間(25% | 75%) / 消費 0.0時間(0% | 75%) / 未分類 0.5時間(25% | 100%)",
            "newline:",
        ]
    );

    let first_task_line = writer.operations[0]
        .strip_prefix("newline:")
        .expect("task row must use writer-specific newline");
    let columns = first_task_line
        .splitn(10, char::is_whitespace)
        .collect::<Vec<_>>();
    assert_eq!(columns.len(), 10, "Spreadsheet連携はA-Jの10列");
    assert_eq!(columns[8], "維", "I列はcategory");
    assert_eq!(columns[9], "夕食 の 準備", "J列はtask_name");
}

#[test]
fn task_list_icon_modeは同じgive_up候補の検索iconと表示iconを区別する() {
    let row = TaskListTaskRow {
        rank: 1,
        task_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
        icon: "!".to_string(),
        remaining_time: "____-01:20".to_string(),
        scheduled_start: Local.with_ymd_and_hms(2026, 8, 23, 9, 0, 0).unwrap(),
        scheduled_end: Local.with_ymd_and_hms(2026, 8, 23, 9, 40, 0).unwrap(),
        priority_rank: 0,
        estimated_minutes: 40,
        project_number_priority: 1,
        project_category: Some(ProjectCategory::Sustaining),
        task_name: "give-up候補".to_string(),
        give_up_candidate: true,
    };

    let search_text =
        format_task_list_columns(&task_list_columns(&row, TaskListIconMode::Original));
    let display_text = format_task_list_columns(&task_list_columns(
        &row,
        TaskListIconMode::ApplyGiveUpCandidate,
    ));

    assert!(search_text.contains(" ! ____-01:20"), "{search_text}");
    assert!(display_text.contains(" A ____-01:20"), "{display_text}");
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
