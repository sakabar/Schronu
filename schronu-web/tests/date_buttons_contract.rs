use schronu_web::client::date_buttons::{logical_date_buttons, DateButtonsError};

#[test]
fn logical_date_buttons_hold_eight_concrete_dates_and_japanese_labels() {
    let buttons = logical_date_buttons("2026-09-05").unwrap();

    assert_eq!(buttons.len(), 8);
    assert_eq!(buttons[0].logical_date, "2026-09-05");
    assert_eq!(buttons[0].label, "土 今日");
    assert_eq!(buttons[1].logical_date, "2026-09-06");
    assert_eq!(buttons[1].label, "日 明日");
    assert_eq!(
        buttons
            .iter()
            .map(|button| button.label.as_str())
            .collect::<Vec<_>>(),
        ["土 今日", "日 明日", "月", "火", "水", "木", "金", "土"]
    );
    assert_eq!(buttons[7].logical_date, "2026-09-12");
}

#[test]
fn logical_date_buttons_cross_month_and_year_boundaries() {
    let month_end = logical_date_buttons("2026-01-29").unwrap();
    assert_eq!(month_end[2].logical_date, "2026-01-31");
    assert_eq!(month_end[3].logical_date, "2026-02-01");

    let year_end = logical_date_buttons("2026-12-29").unwrap();
    assert_eq!(year_end[2].logical_date, "2026-12-31");
    assert_eq!(year_end[3].logical_date, "2027-01-01");
    assert_eq!(year_end[7].logical_date, "2027-01-05");
}

#[test]
fn logical_date_buttons_reject_invalid_dates() {
    assert_eq!(
        logical_date_buttons("2026-02-29"),
        Err(DateButtonsError::InvalidLogicalDate)
    );
    assert_eq!(
        logical_date_buttons("2026-9-05"),
        Err(DateButtonsError::InvalidLogicalDate)
    );
    assert_eq!(
        logical_date_buttons("not-a-date"),
        Err(DateButtonsError::InvalidLogicalDate)
    );
}

#[test]
fn logical_date_buttons_report_output_date_overflow() {
    assert_eq!(
        logical_date_buttons("9999-12-31"),
        Err(DateButtonsError::DateOverflow)
    );
}
