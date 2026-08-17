use super::renderer::{format_spreadsheet_task_row, SpreadsheetTaskRow};

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
