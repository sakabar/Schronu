use crate::entity::datetime::{LogicalDateTimePolicy, DEFAULT_END_OF_DAY_OFFSET_MINUTES};
use chrono::{DateTime, Local, NaiveDate, TimeZone};
use std::error::Error;
use std::fmt;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscardedSessionSource {
    Web,
    Cli,
}

impl DiscardedSessionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Cli => "cli",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "web" => Some(Self::Web),
            "cli" => Some(Self::Cli),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscardedSessionReason {
    WebDiscardRelease,
    WebDiscardComplete,
    CliUnfocus,
    CliTuckAway,
    CliFocusSwitch,
    CliAutoSwitch,
    CliNormalExit,
}

impl DiscardedSessionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WebDiscardRelease => "web_discard_release",
            Self::WebDiscardComplete => "web_discard_complete",
            Self::CliUnfocus => "cli_unfocus",
            Self::CliTuckAway => "cli_tuck_away",
            Self::CliFocusSwitch => "cli_focus_switch",
            Self::CliAutoSwitch => "cli_auto_switch",
            Self::CliNormalExit => "cli_normal_exit",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "web_discard_release" => Some(Self::WebDiscardRelease),
            "web_discard_complete" => Some(Self::WebDiscardComplete),
            "cli_unfocus" => Some(Self::CliUnfocus),
            "cli_tuck_away" => Some(Self::CliTuckAway),
            "cli_focus_switch" => Some(Self::CliFocusSwitch),
            "cli_auto_switch" => Some(Self::CliAutoSwitch),
            "cli_normal_exit" => Some(Self::CliNormalExit),
            _ => None,
        }
    }

    fn matches(self, source: DiscardedSessionSource) -> bool {
        matches!(
            (source, self),
            (
                DiscardedSessionSource::Web,
                Self::WebDiscardRelease | Self::WebDiscardComplete
            ) | (
                DiscardedSessionSource::Cli,
                Self::CliUnfocus
                    | Self::CliTuckAway
                    | Self::CliFocusSwitch
                    | Self::CliAutoSwitch
                    | Self::CliNormalExit
            )
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscardedSessionEvent {
    event_id: Uuid,
    task_id: Uuid,
    task_name_at_start: String,
    started_at_epoch_ms: i64,
    ended_at_epoch_ms: i64,
    logical_date: NaiveDate,
    source: DiscardedSessionSource,
    reason: DiscardedSessionReason,
}

pub(crate) struct PersistedDiscardedSessionEvent {
    pub(crate) event_id: Uuid,
    pub(crate) task_id: Uuid,
    pub(crate) task_name_at_start: String,
    pub(crate) started_at_epoch_ms: i64,
    pub(crate) ended_at_epoch_ms: i64,
    pub(crate) logical_date: NaiveDate,
    pub(crate) source: DiscardedSessionSource,
    pub(crate) reason: DiscardedSessionReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscardedSessionEventError {
    EndBeforeStart,
    TimestampOutOfRange,
    LogicalDateOutOfRange,
    LogicalDateMismatch,
    ReasonDoesNotMatchSource,
}

impl fmt::Display for DiscardedSessionEventError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EndBeforeStart => "discarded session end precedes start",
            Self::TimestampOutOfRange => "discarded session timestamp is outside the local range",
            Self::LogicalDateOutOfRange => "discarded session logical date is outside the range",
            Self::LogicalDateMismatch => "discarded session logical date does not match its start",
            Self::ReasonDoesNotMatchSource => "discarded session reason does not match its source",
        };
        formatter.write_str(message)
    }
}

impl Error for DiscardedSessionEventError {}

impl DiscardedSessionEvent {
    pub fn new(
        event_id: Uuid,
        task_id: Uuid,
        task_name_at_start: String,
        started_at: DateTime<Local>,
        ended_at: DateTime<Local>,
        source: DiscardedSessionSource,
        reason: DiscardedSessionReason,
    ) -> Result<Option<Self>, DiscardedSessionEventError> {
        Self::from_epoch_milliseconds(DiscardedSessionEventFields {
            event_id,
            task_id,
            task_name_at_start,
            started_at_epoch_ms: started_at.timestamp_millis(),
            ended_at_epoch_ms: ended_at.timestamp_millis(),
            persisted_logical_date: None,
            source,
            reason,
        })
    }

    pub(crate) fn from_persisted(
        persisted: PersistedDiscardedSessionEvent,
    ) -> Result<Option<Self>, DiscardedSessionEventError> {
        let PersistedDiscardedSessionEvent {
            event_id,
            task_id,
            task_name_at_start,
            started_at_epoch_ms,
            ended_at_epoch_ms,
            logical_date,
            source,
            reason,
        } = persisted;
        Self::from_epoch_milliseconds(DiscardedSessionEventFields {
            event_id,
            task_id,
            task_name_at_start,
            started_at_epoch_ms,
            ended_at_epoch_ms,
            persisted_logical_date: Some(logical_date),
            source,
            reason,
        })
    }

    fn from_epoch_milliseconds(
        fields: DiscardedSessionEventFields,
    ) -> Result<Option<Self>, DiscardedSessionEventError> {
        let DiscardedSessionEventFields {
            event_id,
            task_id,
            task_name_at_start,
            started_at_epoch_ms,
            ended_at_epoch_ms,
            persisted_logical_date,
            source,
            reason,
        } = fields;
        let elapsed_milliseconds = ended_at_epoch_ms
            .checked_sub(started_at_epoch_ms)
            .ok_or(DiscardedSessionEventError::EndBeforeStart)?;
        if elapsed_milliseconds < 0 {
            return Err(DiscardedSessionEventError::EndBeforeStart);
        }
        if !reason.matches(source) {
            return Err(DiscardedSessionEventError::ReasonDoesNotMatchSource);
        }
        if elapsed_milliseconds / 1_000 == 0 {
            return Ok(None);
        }
        let started_at = Local
            .timestamp_millis_opt(started_at_epoch_ms)
            .single()
            .ok_or(DiscardedSessionEventError::TimestampOutOfRange)?;
        let logical_date = LogicalDateTimePolicy::new(DEFAULT_END_OF_DAY_OFFSET_MINUTES)
            .logical_date(started_at)
            .ok_or(DiscardedSessionEventError::LogicalDateOutOfRange)?;
        if persisted_logical_date.is_some_and(|persisted| persisted != logical_date) {
            return Err(DiscardedSessionEventError::LogicalDateMismatch);
        }
        Ok(Some(Self {
            event_id,
            task_id,
            task_name_at_start,
            started_at_epoch_ms,
            ended_at_epoch_ms,
            logical_date,
            source,
            reason,
        }))
    }

    pub fn event_id(&self) -> Uuid {
        self.event_id
    }
    pub fn task_id(&self) -> Uuid {
        self.task_id
    }
    pub fn task_name_at_start(&self) -> &str {
        &self.task_name_at_start
    }
    pub fn started_at_epoch_ms(&self) -> i64 {
        self.started_at_epoch_ms
    }
    pub fn ended_at_epoch_ms(&self) -> i64 {
        self.ended_at_epoch_ms
    }
    pub fn logical_date(&self) -> NaiveDate {
        self.logical_date
    }
    pub fn source(&self) -> DiscardedSessionSource {
        self.source
    }
    pub fn reason(&self) -> DiscardedSessionReason {
        self.reason
    }
    pub fn elapsed_seconds(&self) -> i64 {
        (self.ended_at_epoch_ms - self.started_at_epoch_ms) / 1_000
    }
}

struct DiscardedSessionEventFields {
    event_id: Uuid,
    task_id: Uuid,
    task_name_at_start: String,
    started_at_epoch_ms: i64,
    ended_at_epoch_ms: i64,
    persisted_logical_date: Option<NaiveDate>,
    source: DiscardedSessionSource,
    reason: DiscardedSessionReason,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Local, TimeZone};
    use uuid::Uuid;

    fn started_at() -> chrono::DateTime<Local> {
        Local.with_ymd_and_hms(2026, 9, 10, 5, 59, 0).unwrap()
    }

    #[test]
    fn 一秒未満は成功no_opになる() {
        let started_at = started_at();
        let actual = DiscardedSessionEvent::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "開始時の名前".to_string(),
            started_at,
            started_at + Duration::milliseconds(999),
            DiscardedSessionSource::Web,
            DiscardedSessionReason::WebDiscardRelease,
        )
        .unwrap();

        assert_eq!(actual, None);
    }

    #[test]
    fn 経過秒を切り捨て開始時刻の論理日に全量を帰属する() {
        let started_at = started_at();
        let event = DiscardedSessionEvent::new(
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            "開始時の名前".to_string(),
            started_at,
            started_at + Duration::seconds(121) + Duration::milliseconds(999),
            DiscardedSessionSource::Cli,
            DiscardedSessionReason::CliFocusSwitch,
        )
        .unwrap()
        .unwrap();

        assert_eq!(event.elapsed_seconds(), 121);
        assert_eq!(
            event.logical_date(),
            chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap()
        );
        assert_eq!(event.task_name_at_start(), "開始時の名前");
    }

    #[test]
    fn 終了が開始より前なら拒否する() {
        let started_at = started_at();
        let error = DiscardedSessionEvent::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "task".to_string(),
            started_at,
            started_at - Duration::seconds(1),
            DiscardedSessionSource::Cli,
            DiscardedSessionReason::CliNormalExit,
        )
        .unwrap_err();

        assert_eq!(error, DiscardedSessionEventError::EndBeforeStart);
    }

    #[test]
    fn sourceとreasonの不一致を拒否する() {
        let started_at = started_at();
        let error = DiscardedSessionEvent::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "task".to_string(),
            started_at,
            started_at + Duration::seconds(1),
            DiscardedSessionSource::Web,
            DiscardedSessionReason::CliUnfocus,
        )
        .unwrap_err();

        assert_eq!(error, DiscardedSessionEventError::ReasonDoesNotMatchSource);
    }
}
