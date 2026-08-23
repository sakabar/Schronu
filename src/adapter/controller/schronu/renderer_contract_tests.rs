use super::renderer::{
    format_spreadsheet_task_row, format_task_list_columns, render_display_model, task_list_columns,
    AncestorTreeRow, BandDayRow, BandDisplay, BandDurations, CalendarAlerts, CalendarDayRow,
    CalendarDisplay, CalendarSummary, DebugTreeRow, DisplayFragment, DisplayModel, DisplayRecorder,
    ErrorCapturingWriter, LeafTreeRow, MessageLevel, PackDisplay, PackRow, SchronuWriter,
    SpreadsheetTaskRow, TaskCategoryWorkSeconds, TaskListDisplay, TaskListIconMode, TaskListRow,
    TaskListTaskRow, TreeDisplay,
};
use chrono::{Local, NaiveDate, TimeZone, Weekday};
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
            "newline:[Crit] critical",
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

#[test]
fn calendar_displayはtyped日別値を逆順と週区切りとsummaryとalertへ描画する() {
    let summary = CalendarSummary {
        last_synced_date: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
        first_caught_up_date: NaiveDate::from_ymd_opt(2026, 8, 25).unwrap(),
        first_leeway_date: NaiveDate::from_ymd_opt(2026, 8, 26).unwrap(),
        first_leeway_minutes: -90,
        max_accumulated_free_diff_minutes: 125,
        max_accumulated_free_diff_date: NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
        max_accumulated_rho_diff: 1.25,
        max_accumulated_rho_diff_date: NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
    };
    let rows = vec![
        CalendarDayRow {
            date: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
            free_time_minutes: 600,
            free_time_diff_minutes: -90,
            adjustable_work_seconds: 0,
            rho_diff: -0.25,
            rho_goal_diff_hours: -1.25,
            accumulated_rho_goal_diff_minutes: -125,
            deadline_diff_seconds: -5400,
            deadline_ratio: -0.15,
            accumulated_free_diff_minutes: -95,
            non_repetitive_free_minutes: 480,
            accumulated_rho_diff: -0.20,
            task_count: 2,
        },
        CalendarDayRow {
            date: NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
            free_time_minutes: 300,
            free_time_diff_minutes: 135,
            adjustable_work_seconds: 3600,
            rho_diff: 0.50,
            rho_goal_diff_hours: 0.75,
            accumulated_rho_goal_diff_minutes: 70,
            deadline_diff_seconds: 7200,
            deadline_ratio: 0.40,
            accumulated_free_diff_minutes: 125,
            non_repetitive_free_minutes: -30,
            accumulated_rho_diff: 1.25,
            task_count: 12,
        },
        CalendarDayRow {
            date: NaiveDate::from_ymd_opt(2026, 8, 25).unwrap(),
            free_time_minutes: 90,
            free_time_diff_minutes: -1,
            adjustable_work_seconds: 900,
            rho_diff: -1.00,
            rho_goal_diff_hours: 3599.0 / 3600.0,
            accumulated_rho_goal_diff_minutes: -1,
            deadline_diff_seconds: 3599,
            deadline_ratio: 0.01,
            accumulated_free_diff_minutes: -1,
            non_repetitive_free_minutes: 0,
            accumulated_rho_diff: 2.50,
            task_count: 99,
        },
    ];
    let display = DisplayModel::Calendar(CalendarDisplay {
        rows: rows.clone(),
        blank_line_weekday: Weekday::Mon,
        summary: summary.clone(),
        alerts: CalendarAlerts {
            has_today_deadline_leeway: false,
            has_today_freetime_leeway: false,
            has_today_new_task_leeway: false,
            has_tomorrow_deadline_leeway: false,
            has_tomorrow_freetime_leeway: false,
            has_weekly_deadline_leeway: false,
            has_weekly_freetime_leeway: false,
        },
    });
    let mut writer = TraceWriter::default();

    render_display_model(&mut writer, &display).unwrap();

    assert_eq!(
        writer.operations,
        [
            "newline:2026-08-25(火)\t 1.5時間\t-0時間01分(17%)\t-1.00\t 0時間60分\t-00時間01分\t 0時間59分\t 0.01\t-00時間01分\t 00時間00分\t 2.50\t99[タスク]",
            "newline:2026-08-24(月)\t 5.0時間\t 2時間15分(20%)\t 0.50\t 0時間45分\t 01時間10分\t 2時間00分\t 0.40\t 02時間05分\t-00時間30分\t 1.25\t12[タスク]",
            "newline:",
            "newline:2026-08-23(日)\t10.0時間\t-1時間30分     \t-0.25\t-1時間15分\t-02時間05分\t-1時間30分\t-0.15\t-01時間35分\t 08時間00分\t-0.20\t02[タスク]",
            "newline:日          \t空          \t空差      \t空差比\t余差    \t余差累    \t〆差      \t〆差比\t空差累    \t単発余暇\t空差累比\tタスク数",
            "newline:",
            "newline:今のタスクが片付く日付: 2日後の2026-08-25",
            "newline:最大の累積時間:  02時間05分 (2026-08-24), 最大のrhoの差: 1.25 (2026-08-24), 次にタスクを積める日付: 3日後の2026-08-26 (-1時間30分)",
            "newline:",
            "newline:[Crit] 【今日の】〆切に間に合いません。【ただちに】〆切をリスケする調整をしてください。",
            "newline:[Crit] 【今日の】終了予定時刻に間に合いません。【ただちに】どれかの予定を諦めて明日以降に延期してください。",
            "newline:[Warn] 【明日の】〆切に間に合いません。〆切をあさって以降にリスケする調整を【今日中に】してください。",
            "newline:[Warn] 【明日の】終了予定時刻に間に合いません。【今日中に】どれかの予定を諦めてあさって以降に延期してください。",
            "newline:[Warn] 【1週間以内の】〆切に間に合いません。【近々】どれかの予定を諦めて来週以降に延期してください。",
            "newline:[Warn] 【1週間以内の】終了予定時刻に間に合いません。【近々】どれかの予定を諦めて来週以降に延期してください。",
            "newline:",
        ]
    );

    let mut tight_writer = TraceWriter::default();
    render_display_model(
        &mut tight_writer,
        &DisplayModel::Calendar(CalendarDisplay {
            rows: rows.clone(),
            blank_line_weekday: Weekday::Mon,
            summary: summary.clone(),
            alerts: CalendarAlerts {
                has_today_deadline_leeway: true,
                has_today_freetime_leeway: true,
                has_today_new_task_leeway: false,
                has_tomorrow_deadline_leeway: true,
                has_tomorrow_freetime_leeway: true,
                has_weekly_deadline_leeway: true,
                has_weekly_freetime_leeway: true,
            },
        }),
    )
    .unwrap();
    assert_eq!(
        &tight_writer.operations[tight_writer.operations.len() - 2..],
        [
            "newline:[Warn] 脇道に逸れずに予定の遂行をしてください。見積もりを間違えたり突発タスクが発生したりした場合に終了予定時刻に間に合わなくなる可能性があります。",
            "newline:",
        ]
    );

    let mut healthy_writer = TraceWriter::default();
    render_display_model(
        &mut healthy_writer,
        &DisplayModel::Calendar(CalendarDisplay {
            rows,
            blank_line_weekday: Weekday::Mon,
            summary,
            alerts: CalendarAlerts {
                has_today_deadline_leeway: true,
                has_today_freetime_leeway: true,
                has_today_new_task_leeway: true,
                has_tomorrow_deadline_leeway: true,
                has_tomorrow_freetime_leeway: true,
                has_weekly_deadline_leeway: true,
                has_weekly_freetime_leeway: true,
            },
        }),
    )
    .unwrap();
    assert_eq!(
        &healthy_writer.operations[healthy_writer.operations.len() - 2..],
        [
            "newline:[Info] 順調です。突発タスクに対応したり1日の終わり際にタスクを新しく積んだりする余裕があります。ひとまずは脇道に逸れずに予定の遂行をしてください。",
            "newline:",
        ]
    );
}

#[test]
fn calendar_displayは日別rowが空でもfooterとsummaryとhealthy_alertを描画する() {
    let display = DisplayModel::Calendar(CalendarDisplay {
        rows: vec![],
        blank_line_weekday: Weekday::Mon,
        summary: CalendarSummary {
            last_synced_date: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
            first_caught_up_date: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
            first_leeway_date: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
            first_leeway_minutes: -30,
            max_accumulated_free_diff_minutes: -65,
            max_accumulated_free_diff_date: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
            max_accumulated_rho_diff: -0.50,
            max_accumulated_rho_diff_date: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
        },
        alerts: CalendarAlerts {
            has_today_deadline_leeway: true,
            has_today_freetime_leeway: true,
            has_today_new_task_leeway: true,
            has_tomorrow_deadline_leeway: true,
            has_tomorrow_freetime_leeway: true,
            has_weekly_deadline_leeway: true,
            has_weekly_freetime_leeway: true,
        },
    });
    let mut writer = TraceWriter::default();

    render_display_model(&mut writer, &display).unwrap();

    assert_eq!(
        writer.operations,
        [
            "newline:日          \t空          \t空差      \t空差比\t余差    \t余差累    \t〆差      \t〆差比\t空差累    \t単発余暇\t空差累比\tタスク数",
            "newline:",
            "newline:今のタスクが片付く日付: 0日後の2026-08-23",
            "newline:最大の累積時間: -01時間05分 (2026-08-23), 最大のrhoの差: -0.50 (2026-08-23), 次にタスクを積める日付: 0日後の2026-08-23 (-0時間30分)",
            "newline:",
            "newline:[Info] 順調です。突発タスクに対応したり1日の終わり際にタスクを新しく積んだりする余裕があります。ひとまずは脇道に逸れずに予定の遂行をしてください。",
            "newline:",
        ]
    );
}

fn band_display_fixture() -> BandDisplay {
    BandDisplay {
        rows: vec![
            BandDayRow {
                date: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
                accumulated_rho_diff_seconds: 62 * 60,
                accumulated_free_diff_seconds: -(3 * 60 + 4) * 60,
                durations: BandDurations {
                    fixed_seconds: 15 * 60,
                    elapsed_seconds: 30 * 60,
                    repetitive_seconds: 45 * 60,
                    non_repetitive_seconds: 60 * 60,
                    rho_leeway_seconds: 75 * 60,
                },
            },
            BandDayRow {
                date: NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
                accumulated_rho_diff_seconds: -(7 * 60 + 8) * 60,
                accumulated_free_diff_seconds: (46 * 60 + 9) * 60,
                durations: BandDurations {
                    fixed_seconds: 450 * 60,
                    elapsed_seconds: 800 * 60,
                    repetitive_seconds: 476 * 60,
                    non_repetitive_seconds: 40 * 60,
                    rho_leeway_seconds: 0,
                },
            },
        ],
        summary: CalendarSummary {
            last_synced_date: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
            first_caught_up_date: NaiveDate::from_ymd_opt(2026, 8, 25).unwrap(),
            first_leeway_date: NaiveDate::from_ymd_opt(2026, 8, 26).unwrap(),
            first_leeway_minutes: -90,
            max_accumulated_free_diff_minutes: 125,
            max_accumulated_free_diff_date: NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
            max_accumulated_rho_diff: 1.25,
            max_accumulated_rho_diff_date: NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
        },
        alerts: CalendarAlerts {
            has_today_deadline_leeway: true,
            has_today_freetime_leeway: true,
            has_today_new_task_leeway: true,
            has_tomorrow_deadline_leeway: true,
            has_tomorrow_freetime_leeway: true,
            has_weekly_deadline_leeway: true,
            has_weekly_freetime_leeway: true,
        },
    }
}

#[test]
fn band_displayは96segmentと超過と逆順と週区切りとsummary_alertを描画する() {
    let display = DisplayModel::Band(band_display_fixture());
    let mut writer = TraceWriter::default();

    render_display_model(&mut writer, &display).unwrap();

    assert_eq!(
        writer.operations,
        [
            "newline:凡例: # 固定  x 経過済み  = 繰返  - 単発  : 余差  . 空き  > 超過  (1文字=15分)",
            "newline:",
            &format!(
                "newline:2026-08-24(月) -07:08 +46:09 [{}{}{}]{}",
                "#".repeat(30),
                "x".repeat(53),
                "=".repeat(13),
                ">".repeat(22),
            ),
            "newline:",
            &format!(
                "newline:2026-08-23(日) +01:02 -03:04 [{}{}{}{}{}{}]",
                "#",
                "x".repeat(2),
                "=".repeat(3),
                "-".repeat(4),
                ":".repeat(5),
                ".".repeat(81),
            ),
            "newline:",
            "newline:今のタスクが片付く日付: 2日後の2026-08-25",
            "newline:最大の累積時間:  02時間05分 (2026-08-24), 最大のrhoの差: 1.25 (2026-08-24), 次にタスクを積める日付: 3日後の2026-08-26 (-1時間30分)",
            "newline:",
            "newline:[Info] 順調です。突発タスクに対応したり1日の終わり際にタスクを新しく積んだりする余裕があります。ひとまずは脇道に逸れずに予定の遂行をしてください。",
            "newline:",
        ]
    );
    let rows = &writer.operations[2..=4];
    assert_eq!(rows[0].matches('[').count(), 1);
    assert_eq!(
        rows[2].split(['[', ']']).nth(1).unwrap().chars().count(),
        96
    );
    assert!(!writer
        .operations
        .iter()
        .any(|operation| operation.contains("\x1b[")));
}

#[test]
fn band_displayはterminalで凡例と帯の7記号を既存ansi色で描画する() {
    let display = DisplayModel::Band(band_display_fixture());
    let mut writer = TraceWriter {
        supports_ansi_color: true,
        ..TraceWriter::default()
    };
    let color = |value: u8, symbols: &str| format!("\x1b[38;5;{value}m{symbols}\x1b[39m");

    render_display_model(&mut writer, &display).unwrap();

    assert_eq!(
        writer.operations[0],
        format!(
            "newline:凡例: {} 固定  {} 経過済み  {} 繰返  {} 単発  {} 余差  {} 空き  {} 超過  (1文字=15分)",
            color(110, "#"),
            color(244, "x"),
            color(33, "="),
            color(208, "-"),
            color(28, ":"),
            color(34, "."),
            color(196, ">"),
        )
    );
    assert_eq!(
        writer.operations[2],
        format!(
            "newline:2026-08-24(月) -07:08 +46:09 [{}{}{}]{}",
            color(110, &"#".repeat(30)),
            color(244, &"x".repeat(53)),
            color(33, &"=".repeat(13)),
            color(196, &">".repeat(22)),
        )
    );
    assert_eq!(
        writer.operations[4],
        format!(
            "newline:2026-08-23(日) +01:02 -03:04 [{}{}{}{}{}{}]",
            color(110, "#"),
            color(244, "xx"),
            color(33, "==="),
            color(208, "----"),
            color(28, ":::::"),
            color(34, &".".repeat(81)),
        )
    );
}

#[test]
fn band_displayは日別rowが空でもlegendとsummaryとhealthy_alertを描画する() {
    let mut band = band_display_fixture();
    band.rows.clear();
    let display = DisplayModel::Band(band);
    let mut writer = TraceWriter::default();

    render_display_model(&mut writer, &display).unwrap();

    assert_eq!(
        writer.operations,
        [
            "newline:凡例: # 固定  x 経過済み  = 繰返  - 単発  : 余差  . 空き  > 超過  (1文字=15分)",
            "newline:",
            "newline:",
            "newline:今のタスクが片付く日付: 2日後の2026-08-25",
            "newline:最大の累積時間:  02時間05分 (2026-08-24), 最大のrhoの差: 1.25 (2026-08-24), 次にタスクを積める日付: 3日後の2026-08-26 (-1時間30分)",
            "newline:",
            "newline:[Info] 順調です。突発タスクに対応したり1日の終わり際にタスクを新しく積んだりする余裕があります。ひとまずは脇道に逸れずに予定の遂行をしてください。",
            "newline:",
        ]
    );
}

#[test]
fn pack_displayはtyped_row順と集計と空結果とskip件数を描画する() {
    let packed = DisplayModel::Pack(PackDisplay {
        rows: vec![
            PackRow {
                source_date: NaiveDate::from_ymd_opt(2026, 8, 25).unwrap(),
                target_date: NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
                work_seconds: 3_661,
                priority: 9,
                task_id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
                name: "最初の前倒し".to_string(),
            },
            PackRow {
                source_date: NaiveDate::from_ymd_opt(2026, 8, 26).unwrap(),
                target_date: NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
                work_seconds: 59,
                priority: -2,
                task_id: Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
                name: "次の前倒し".to_string(),
            },
        ],
        skipped_count: 3,
    });
    let mut packed_writer = TraceWriter::default();

    render_display_model(&mut packed_writer, &packed).unwrap();

    assert_eq!(
        packed_writer.operations,
        [
            "newline:詰\t2026-08-25\t2026-08-23\t01:01\t優先度9\t11111111-1111-1111-1111-111111111111\t最初の前倒し",
            "newline:詰\t2026-08-26\t2026-08-24\t00:00\t優先度-2\t22222222-2222-2222-2222-222222222222\t次の前倒し",
            "newline:詰: 2件 01:02 (スキップ3件)",
        ]
    );

    let mut empty_writer = TraceWriter::default();
    render_display_model(
        &mut empty_writer,
        &DisplayModel::Pack(PackDisplay {
            rows: vec![],
            skipped_count: 0,
        }),
    )
    .unwrap();
    assert_eq!(
        empty_writer.operations,
        ["newline:[Info] 詰められるタスクはありません。"]
    );

    let mut skipped_writer = TraceWriter::default();
    render_display_model(
        &mut skipped_writer,
        &DisplayModel::Pack(PackDisplay {
            rows: vec![],
            skipped_count: 2,
        }),
    )
    .unwrap();
    assert_eq!(
        skipped_writer.operations,
        ["newline:詰: 0件 00:00 (スキップ2件)"]
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
