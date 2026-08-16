use super::command::{parse_command, Command, CommandKind, ParseMode};
use super::renderer::{format_spreadsheet_task_row, SpreadsheetTaskRow};
use uuid::Uuid;

#[test]
fn 全aliasは同じtyped_commandへparseされる() {
    let aliases = [
        (&["新", "new"][..], CommandKind::NewProject),
        (&["遊", "hobby"][..], CommandKind::HobbyProject),
        (&["突", "unplanned"][..], CommandKind::UnplannedProject),
        (&["連", "sequential", "seq"][..], CommandKind::Sequential),
        (&["繰", "repeat"][..], CommandKind::Repeat),
        (&["約", "appointment"][..], CommandKind::Appointment),
        (&["始", "start"][..], CommandKind::Start),
        (&["樹", "tree"][..], CommandKind::Tree),
        (&["条", "祖", "ancestor", "anc"][..], CommandKind::Ancestor),
        (&["根", "root"][..], CommandKind::Root),
        (&["葉", "leaves", "leaf", "lf"][..], CommandKind::Leaves),
        (&["全", "all"][..], CommandKind::ShowAll),
        (&["尾"][..], CommandKind::Tail),
        (&["今", "today"][..], CommandKind::Today),
        (&["単", "non_repetitive"][..], CommandKind::NonRepetitive),
        (&["暦", "cal"][..], CommandKind::Calendar),
        (&["帯", "band"][..], CommandKind::Band),
        (&["見", "focus", "fc"][..], CommandKind::Focus),
        (&["選", "pick"][..], CommandKind::Pick),
        (&["開", "open", "op"][..], CommandKind::Open),
        (&["黒", "obs"][..], CommandKind::Obsidian),
        (&["外", "unfocus", "ufc"][..], CommandKind::Unfocus),
        (&["親", "parent"][..], CommandKind::Parent),
        (&["子", "children", "ch"][..], CommandKind::Children),
        (&["深", "deep", "deepest"][..], CommandKind::Deepest),
        (&["上", "nextup", "nu"][..], CommandKind::NextUp),
        (&["下", "breakdown", "bd"][..], CommandKind::Breakdown),
        (&["割", "split", "sp"][..], CommandKind::Split),
        (&["待", "wait"][..], CommandKind::Wait),
        (&["〆", "締", "deadline"][..], CommandKind::Deadline),
        (&["予", "estimate", "es"][..], CommandKind::Estimate),
        (&["揃", "arrange", "arr"][..], CommandKind::Arrange),
        (&["実", "actual", "ac"][..], CommandKind::Actual),
        (&["重", "priority", "pr"][..], CommandKind::Priority),
        (&["類", "category", "cat"][..], CommandKind::Category),
        (&["働", "work", "wk"][..], CommandKind::Work),
        (&["後", "defer"][..], CommandKind::Defer),
        (
            &["清", "defer_all_frequent_routines"][..],
            CommandKind::DeferRoutines,
        ),
        (&["逃", "escape", "esc"][..], CommandKind::Escape),
        (&["平", "flatten", "flat"][..], CommandKind::Flatten),
        (&["詰", "pack"][..], CommandKind::Pack),
        (&["押", "extrude"][..], CommandKind::Extrude),
        (&["空", "clear"][..], CommandKind::Clear),
        (&["集", "gather"][..], CommandKind::Gather),
        (&["終", "finish", "fin"][..], CommandKind::Finish),
    ];

    for (names, expected) in aliases {
        for name in names {
            let input = command_with_minimum_valid_arguments(name);
            let actual = parse_command(&input, ParseMode::NonInteractive).unwrap();
            assert_eq!(actual.kind(), expected, "alias: {name}");
        }
    }
}

#[test]
fn parserは代表的な引数をtyped_valueへ変換する() {
    assert_eq!(
        parse_command("予 45", ParseMode::NonInteractive).unwrap(),
        Command::Estimate { minutes: 45 }
    );
    let task_id = Uuid::new_v4();
    assert_eq!(
        parse_command(&format!("見 {task_id}"), ParseMode::NonInteractive).unwrap(),
        Command::Focus { task_id }
    );
    assert_eq!(
        parse_command("揃 15 全", ParseMode::NonInteractive).unwrap(),
        Command::Arrange {
            minutes: 15,
            includes_zero_estimate: true,
        }
    );
}

#[test]
fn parserは空入力_comment_検索fallback_interactive_shortcutを区別する() {
    assert_eq!(
        parse_command("   ", ParseMode::NonInteractive).unwrap(),
        Command::Noop
    );
    assert_eq!(
        parse_command("# memo", ParseMode::NonInteractive).unwrap(),
        Command::Noop
    );
    assert_eq!(
        parse_command("検索語", ParseMode::NonInteractive).unwrap(),
        Command::ShowAll {
            pattern: Some("検索語".to_string()),
        }
    );
    assert_eq!(
        parse_command("t", ParseMode::Interactive).unwrap(),
        Command::Defer {
            amount: 1,
            unit: "秒".to_string(),
        }
    );
}

#[test]
fn parserの不正引数はfield付きerrorになる() {
    let error = parse_command("予 x", ParseMode::NonInteractive).unwrap_err();
    assert_eq!(error.command(), "予");
    assert_eq!(error.field(), "estimated_work_minutes");
}

#[test]
fn spreadsheet_formatterはaからj列を固定する() {
    let row = SpreadsheetTaskRow {
        index: 7,
        task_id: Uuid::parse_str("00000000-0000-0000-0000-000000000007").unwrap(),
        icon: "/".to_string(),
        deadline: "____-00:30".to_string(),
        date: "08/16(日)".to_string(),
        time_range: "10:00-10:15".to_string(),
        rank: 0,
        estimated_minutes: 15,
        category: "資".to_string(),
        task_name: "契約テスト".to_string(),
    };

    assert_eq!(
        format_spreadsheet_task_row(&row),
        "0007\t00000000-0000-0000-0000-000000000007\t/\t____-00:30\t08/16(日)\t10:00-10:15\t0\t15\t資\t契約テスト"
    );
}

fn command_with_minimum_valid_arguments(command: &str) -> String {
    let arguments = match command {
        "新" | "new" | "遊" | "hobby" | "突" | "unplanned" => " project 15",
        "連" | "sequential" | "seq" => " task 15 1 2",
        "繰" | "repeat" => " task 15 月 09:00 10:00",
        "約" | "appointment" | "始" | "start" => " 今",
        "見" | "focus" | "fc" | "選" | "pick" => {
            " 00000000-0000-0000-0000-000000000001"
        }
        "上" | "nextup" | "nu" | "下" | "breakdown" | "bd" => " task 15",
        "割" | "split" | "sp" => " 15 child",
        "〆" | "締" | "deadline" => " 今",
        "予" | "estimate" | "es" | "揃" | "arrange" | "arr" | "実" | "actual"
        | "ac" | "重" | "priority" | "pr" | "働" | "work" | "wk" | "押"
        | "extrude" => " 15",
        "類" | "category" | "cat" => " 資",
        "後" | "defer" | "逃" | "escape" | "esc" => " 1 秒",
        "空" | "clear" | "集" | "gather" => " 明",
        "終" | "finish" | "fin" => " 今",
        _ => "",
    };
    format!("{command}{arguments}")
}
