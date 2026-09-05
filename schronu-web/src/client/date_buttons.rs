use chrono::{DateTime, Datelike, Days, Local, LocalResult, NaiveDate, TimeZone, Weekday};

const BUTTON_COUNT: u64 = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalDateButton {
    pub logical_date: String,
    pub label: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DateButtonsError {
    InvalidLogicalDate,
    DateOverflow,
}

pub fn logical_date_buttons(
    logical_date: &str,
) -> Result<Vec<LogicalDateButton>, DateButtonsError> {
    let start = parse_logical_date(logical_date)?;

    (0..BUTTON_COUNT)
        .map(|index| {
            let date = start
                .checked_add_days(Days::new(index))
                .filter(|date| (0..=9_999).contains(&date.year()))
                .ok_or(DateButtonsError::DateOverflow)?;
            let suffix = match index {
                0 => " 今日",
                1 => " 明日",
                _ => "",
            };
            Ok(LogicalDateButton {
                logical_date: date.format("%Y-%m-%d").to_string(),
                label: format!("{}{suffix}", japanese_weekday(date.weekday())),
            })
        })
        .collect()
}

pub(crate) fn logical_date_start(
    logical_date: &str,
) -> Result<LocalResult<DateTime<Local>>, DateButtonsError> {
    let local_start = parse_logical_date(logical_date)?
        .and_hms_opt(6, 0, 0)
        .ok_or(DateButtonsError::DateOverflow)?;
    Ok(Local.from_local_datetime(&local_start))
}

fn parse_logical_date(input: &str) -> Result<NaiveDate, DateButtonsError> {
    if input.len() != 10
        || !input.as_bytes()[..4].iter().all(u8::is_ascii_digit)
        || input.as_bytes()[4] != b'-'
        || !input.as_bytes()[5..7].iter().all(u8::is_ascii_digit)
        || input.as_bytes()[7] != b'-'
        || !input.as_bytes()[8..].iter().all(u8::is_ascii_digit)
    {
        return Err(DateButtonsError::InvalidLogicalDate);
    }

    NaiveDate::parse_from_str(input, "%Y-%m-%d").map_err(|_| DateButtonsError::InvalidLogicalDate)
}

fn japanese_weekday(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "月",
        Weekday::Tue => "火",
        Weekday::Wed => "水",
        Weekday::Thu => "木",
        Weekday::Fri => "金",
        Weekday::Sat => "土",
        Weekday::Sun => "日",
    }
}
