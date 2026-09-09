use super::renderer::{DisplayModel, MessageLevel};
use crate::application::discarded_session_journal::{
    DiscardedSessionConflictError, DiscardedSessionDaySummary, DiscardedSessionSummaryError,
};
use crate::application::interface::{DiscardedSessionJournalTrait, TaskRepositoryTrait};
use crate::application::task_use_case::ApplicationError;
use crate::entity::discarded_session::{
    DiscardedSessionEvent, DiscardedSessionEventError, DiscardedSessionReason,
    DiscardedSessionSource,
};
use chrono::{DateTime, Local, NaiveDate, TimeZone};
use std::error::Error;
use std::fmt;
use uuid::Uuid;

pub(super) trait CliRepositoryTrait:
    TaskRepositoryTrait + DiscardedSessionJournalTrait
{
}

impl<T> CliRepositoryTrait for T where T: TaskRepositoryTrait + DiscardedSessionJournalTrait + ?Sized
{}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FocusSnapshot {
    task_id: Uuid,
    task_name: String,
}

pub(super) struct FocusTransition<'a> {
    pub(super) snapshot: Option<&'a FocusSnapshot>,
    pub(super) old_focus_id: Option<Uuid>,
    pub(super) new_focus_id: Option<Uuid>,
    pub(super) started_at: DateTime<Local>,
    pub(super) ended_at: DateTime<Local>,
    pub(super) event_id: Uuid,
    pub(super) reason: DiscardedSessionReason,
}

#[derive(Debug)]
pub(super) enum CliDiscardedSessionError {
    Application(ApplicationError),
    Event(DiscardedSessionEventError),
    Conflict(DiscardedSessionConflictError),
    Summary(DiscardedSessionSummaryError),
    TimestampOutOfRange,
}

impl fmt::Display for CliDiscardedSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Application(error) => {
                write!(formatter, "破棄sessionのtask取得に失敗しました: {error}")
            }
            Self::Event(error) => write!(formatter, "破棄sessionの生成に失敗しました: {error}"),
            Self::Conflict(error) => write!(formatter, "破棄sessionの保存に失敗しました: {error}"),
            Self::Summary(error) => write!(formatter, "破棄sessionの集計に失敗しました: {error}"),
            Self::TimestampOutOfRange => formatter.write_str("破棄sessionのlocal時刻が範囲外です"),
        }
    }
}

impl Error for CliDiscardedSessionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Application(error) => Some(error),
            Self::Event(error) => Some(error),
            Self::Conflict(error) => Some(error),
            Self::Summary(error) => Some(error),
            Self::TimestampOutOfRange => None,
        }
    }
}

pub(super) fn focus_snapshot(
    repository: &dyn TaskRepositoryTrait,
    focused_task_id: Option<Uuid>,
) -> Result<Option<FocusSnapshot>, CliDiscardedSessionError> {
    let Some(task_id) = focused_task_id else {
        return Ok(None);
    };
    let task = repository
        .get_by_id(task_id)
        .map_err(ApplicationError::TaskTree)
        .map_err(CliDiscardedSessionError::Application)?;
    task.map(|task| {
        task.get_name()
            .map(|task_name| FocusSnapshot { task_id, task_name })
            .map_err(ApplicationError::TaskTree)
            .map_err(CliDiscardedSessionError::Application)
    })
    .transpose()
}

pub(super) fn append_focus_transition(
    repository: &mut dyn CliRepositoryTrait,
    transition: FocusTransition<'_>,
) -> Result<bool, CliDiscardedSessionError> {
    let FocusTransition {
        snapshot,
        old_focus_id,
        new_focus_id,
        started_at,
        ended_at,
        event_id,
        reason,
    } = transition;
    if old_focus_id == new_focus_id {
        return Ok(false);
    }
    let Some(snapshot) = snapshot.filter(|snapshot| Some(snapshot.task_id) == old_focus_id) else {
        return Ok(false);
    };
    let event = DiscardedSessionEvent::new(
        event_id,
        snapshot.task_id,
        snapshot.task_name.clone(),
        started_at,
        ended_at,
        DiscardedSessionSource::Cli,
        reason,
    )
    .map_err(CliDiscardedSessionError::Event)?;
    let Some(event) = event else {
        return Ok(false);
    };
    repository
        .append_discarded_session(event)
        .map_err(CliDiscardedSessionError::Conflict)?;
    Ok(true)
}

pub(super) fn discarded_sessions_display(
    repository: &dyn DiscardedSessionJournalTrait,
    logical_date: NaiveDate,
) -> Result<DisplayModel, CliDiscardedSessionError> {
    let summary = repository
        .discarded_sessions_on(logical_date)
        .map_err(CliDiscardedSessionError::Summary)?;
    Ok(DisplayModel::Message {
        level: MessageLevel::Plain,
        text: format_summary(&summary)?,
    })
}

fn format_summary(
    summary: &DiscardedSessionDaySummary,
) -> Result<String, CliDiscardedSessionError> {
    let mut lines = vec![
        format!("破棄時間: {}", summary.logical_date().format("%Y/%m/%d")),
        format!("合計: {}", format_duration(summary.total_seconds())),
    ];
    if summary.events().is_empty() {
        lines.push("記録はありません".to_string());
        return Ok(lines.join("\n"));
    }
    lines.push("task別:".to_string());
    for total in summary.task_totals() {
        lines.push(format!(
            "  {}  {}",
            format_duration(total.total_seconds()),
            total.task_name_at_latest_start()
        ));
    }
    lines.push("内訳:".to_string());
    for event in summary.events() {
        let started_at = Local
            .timestamp_millis_opt(event.started_at_epoch_ms())
            .single()
            .ok_or(CliDiscardedSessionError::TimestampOutOfRange)?;
        let ended_at = Local
            .timestamp_millis_opt(event.ended_at_epoch_ms())
            .single()
            .ok_or(CliDiscardedSessionError::TimestampOutOfRange)?;
        lines.push(format!(
            "  {}-{}  {}  {}  {}",
            started_at.format("%H:%M:%S"),
            ended_at.format("%H:%M:%S"),
            format_duration(event.elapsed_seconds()),
            event.task_name_at_start(),
            reason_label(event.reason()),
        ));
    }
    Ok(lines.join("\n"))
}

fn format_duration(seconds: i64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

fn reason_label(reason: DiscardedSessionReason) -> &'static str {
    match reason {
        DiscardedSessionReason::WebDiscardRelease => "Webで破棄して解除",
        DiscardedSessionReason::WebDiscardComplete => "Webで破棄して完了",
        DiscardedSessionReason::CliUnfocus => "フォーカス解除",
        DiscardedSessionReason::CliTuckAway => "伏せる",
        DiscardedSessionReason::CliFocusSwitch => "フォーカス切替",
        DiscardedSessionReason::CliAutoSwitch => "自動切替",
        DiscardedSessionReason::CliNormalExit => "正常終了",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::discarded_session_journal::{
        summarize_discarded_sessions, AppendDiscardedSessionOutcome,
    };
    use chrono::{Duration, TimeZone};

    struct Journal(Vec<DiscardedSessionEvent>);

    impl DiscardedSessionJournalTrait for Journal {
        fn append_discarded_session(
            &mut self,
            event: DiscardedSessionEvent,
        ) -> Result<AppendDiscardedSessionOutcome, DiscardedSessionConflictError> {
            self.0.push(event);
            Ok(AppendDiscardedSessionOutcome::Appended)
        }

        fn discarded_sessions_on(
            &self,
            logical_date: NaiveDate,
        ) -> Result<DiscardedSessionDaySummary, DiscardedSessionSummaryError> {
            summarize_discarded_sessions(&self.0, logical_date)
        }
    }

    #[test]
    fn summary_display_contains_daily_task_and_chronological_event_details() {
        let started_at = Local.with_ymd_and_hms(2026, 9, 10, 9, 2, 3).unwrap();
        let event = DiscardedSessionEvent::new(
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            "開始時task".to_string(),
            started_at,
            started_at + Duration::seconds(61),
            DiscardedSessionSource::Cli,
            DiscardedSessionReason::CliTuckAway,
        )
        .unwrap()
        .unwrap();
        let display = discarded_sessions_display(
            &Journal(vec![event]),
            NaiveDate::from_ymd_opt(2026, 9, 10).unwrap(),
        )
        .unwrap();

        let DisplayModel::Message { text, .. } = display else {
            panic!("discarded summary must be a message model");
        };
        assert!(text.contains("破棄時間: 2026/09/10"));
        assert!(text.contains("合計: 00:01:01"));
        assert!(text.contains("09:02:03-09:03:04  00:01:01  開始時task  伏せる"));
    }

    #[test]
    fn empty_summary_is_explicit() {
        let display = discarded_sessions_display(
            &Journal(Vec::new()),
            NaiveDate::from_ymd_opt(2026, 9, 10).unwrap(),
        )
        .unwrap();
        let DisplayModel::Message { text, .. } = display else {
            panic!("discarded summary must be a message model");
        };
        assert!(text.contains("合計: 00:00:00"));
        assert!(text.contains("記録はありません"));
    }

    #[test]
    fn summary_display_orders_events_and_labels_every_reason_in_japanese() {
        let base = Local.with_ymd_and_hms(2026, 9, 10, 9, 0, 0).unwrap();
        let reasons = [
            (
                DiscardedSessionSource::Web,
                DiscardedSessionReason::WebDiscardRelease,
                "Webで破棄して解除",
            ),
            (
                DiscardedSessionSource::Web,
                DiscardedSessionReason::WebDiscardComplete,
                "Webで破棄して完了",
            ),
            (
                DiscardedSessionSource::Cli,
                DiscardedSessionReason::CliUnfocus,
                "フォーカス解除",
            ),
            (
                DiscardedSessionSource::Cli,
                DiscardedSessionReason::CliTuckAway,
                "伏せる",
            ),
            (
                DiscardedSessionSource::Cli,
                DiscardedSessionReason::CliFocusSwitch,
                "フォーカス切替",
            ),
            (
                DiscardedSessionSource::Cli,
                DiscardedSessionReason::CliAutoSwitch,
                "自動切替",
            ),
            (
                DiscardedSessionSource::Cli,
                DiscardedSessionReason::CliNormalExit,
                "正常終了",
            ),
        ];
        let mut events = reasons
            .iter()
            .enumerate()
            .map(|(index, (source, reason, _))| {
                let started_at = base + Duration::minutes(index as i64);
                DiscardedSessionEvent::new(
                    Uuid::from_u128(index as u128 + 1),
                    Uuid::from_u128(index as u128 % 2 + 10),
                    format!("task{}", index % 2),
                    started_at,
                    started_at + Duration::seconds(2),
                    *source,
                    *reason,
                )
                .unwrap()
                .unwrap()
            })
            .collect::<Vec<_>>();
        events.reverse();

        let display = discarded_sessions_display(
            &Journal(events),
            NaiveDate::from_ymd_opt(2026, 9, 10).unwrap(),
        )
        .unwrap();
        let DisplayModel::Message { text, .. } = display else {
            panic!("discarded summary must be a message model");
        };

        assert!(text.contains("合計: 00:00:14"));
        assert!(text.contains("  00:00:08  task0"));
        assert!(text.contains("  00:00:06  task1"));
        let mut previous_position = 0;
        for (_, _, label) in reasons {
            let position = text.find(label).unwrap();
            assert!(position >= previous_position, "{label}");
            previous_position = position;
        }
    }
}
