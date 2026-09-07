use chrono::{Datelike, NaiveDate};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DateInputError {
    InvalidFormat,
    InvalidDate,
    InvalidCurrentLogicalDate,
    DateOverflow,
}

impl fmt::Display for DateInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => formatter.write_str("M/DまたはYYYY/M/D形式で入力してください。"),
            Self::InvalidDate => formatter.write_str("有効な日付を入力してください。"),
            Self::InvalidCurrentLogicalDate | Self::DateOverflow => {
                formatter.write_str("指定した日付を表示できません。")
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedDateInput {
    pub logical_date: String,
    pub display_value: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DateInputState {
    text: String,
    error: Option<DateInputError>,
}

impl DateInputState {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn error(&self) -> Option<DateInputError> {
        self.error
    }

    pub fn edit(&mut self, text: String) {
        self.text = text;
        self.error = None;
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.error = None;
    }

    pub fn submit(&mut self, current_logical_date: &str) -> Option<String> {
        if self.text.trim().is_empty() {
            self.error = None;
            return None;
        }

        match resolve_date_input(&self.text, current_logical_date) {
            Ok(resolved) => {
                self.text = resolved.display_value;
                self.error = None;
                Some(resolved.logical_date)
            }
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }
}

pub fn resolve_date_input(
    input: &str,
    current_logical_date: &str,
) -> Result<ResolvedDateInput, DateInputError> {
    let current = parse_current_logical_date(current_logical_date)?;
    let components = input.trim().split('/').collect::<Vec<_>>();
    let date = match components.as_slice() {
        [month, day] if valid_component(month, 1, 2) && valid_component(day, 1, 2) => {
            let month = parse_component(month)?;
            let day = parse_component(day)?;
            resolve_upcoming_month_day(current, month, day)?
        }
        [year, month, day]
            if valid_component(year, 4, 4)
                && valid_component(month, 1, 2)
                && valid_component(day, 1, 2) =>
        {
            NaiveDate::from_ymd_opt(
                parse_component(year)? as i32,
                parse_component(month)?,
                parse_component(day)?,
            )
            .ok_or(DateInputError::InvalidDate)?
        }
        _ => return Err(DateInputError::InvalidFormat),
    };

    Ok(ResolvedDateInput {
        logical_date: date.format("%Y-%m-%d").to_string(),
        display_value: format!("{}/{}/{}", date.year(), date.month(), date.day()),
    })
}

fn resolve_upcoming_month_day(
    current: NaiveDate,
    month: u32,
    day: u32,
) -> Result<NaiveDate, DateInputError> {
    if NaiveDate::from_ymd_opt(2000, month, day).is_none() {
        return Err(DateInputError::InvalidDate);
    }

    // Gregorian calendarでは有効な同一月日の間隔は最大8年(世紀の閏年境界)となる。
    for year_offset in 0..=8 {
        let year = current
            .year()
            .checked_add(year_offset)
            .filter(|year| *year <= 9999)
            .ok_or(DateInputError::DateOverflow)?;
        if let Some(candidate) = NaiveDate::from_ymd_opt(year, month, day) {
            if candidate >= current {
                return Ok(candidate);
            }
        }
    }

    Err(DateInputError::DateOverflow)
}

fn parse_current_logical_date(input: &str) -> Result<NaiveDate, DateInputError> {
    let date = NaiveDate::parse_from_str(input, "%Y-%m-%d")
        .map_err(|_| DateInputError::InvalidCurrentLogicalDate)?;
    if date.format("%Y-%m-%d").to_string() != input {
        return Err(DateInputError::InvalidCurrentLogicalDate);
    }
    Ok(date)
}

fn valid_component(component: &str, min_len: usize, max_len: usize) -> bool {
    (min_len..=max_len).contains(&component.len())
        && component.bytes().all(|byte| byte.is_ascii_digit())
}

fn parse_component(component: &str) -> Result<u32, DateInputError> {
    component
        .parse::<u32>()
        .map_err(|_| DateInputError::InvalidDate)
}
