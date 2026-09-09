use crate::entity::discarded_session::DiscardedSessionEvent;
use chrono::NaiveDate;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppendDiscardedSessionOutcome {
    Appended,
    AlreadyPresent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiscardedSessionConflictError {
    event_id: Uuid,
}

impl DiscardedSessionConflictError {
    pub fn new(event_id: Uuid) -> Self {
        Self { event_id }
    }
    pub fn event_id(&self) -> Uuid {
        self.event_id
    }
}

impl fmt::Display for DiscardedSessionConflictError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "discarded session event ID conflicts: {}",
            self.event_id
        )
    }
}

impl Error for DiscardedSessionConflictError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscardedSessionTaskTotal {
    task_id: Uuid,
    task_name_at_latest_start: String,
    total_seconds: i64,
}

impl DiscardedSessionTaskTotal {
    pub fn task_id(&self) -> Uuid {
        self.task_id
    }
    pub fn task_name_at_latest_start(&self) -> &str {
        &self.task_name_at_latest_start
    }
    pub fn total_seconds(&self) -> i64 {
        self.total_seconds
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscardedSessionDaySummary {
    logical_date: NaiveDate,
    total_seconds: i64,
    task_totals: Vec<DiscardedSessionTaskTotal>,
    events: Vec<DiscardedSessionEvent>,
}

impl DiscardedSessionDaySummary {
    pub fn logical_date(&self) -> NaiveDate {
        self.logical_date
    }
    pub fn total_seconds(&self) -> i64 {
        self.total_seconds
    }
    pub fn task_totals(&self) -> &[DiscardedSessionTaskTotal] {
        &self.task_totals
    }
    pub fn events(&self) -> &[DiscardedSessionEvent] {
        &self.events
    }
}

pub fn summarize_discarded_sessions(
    events: &[DiscardedSessionEvent],
    logical_date: NaiveDate,
) -> DiscardedSessionDaySummary {
    let mut events = events
        .iter()
        .filter(|event| event.logical_date() == logical_date)
        .cloned()
        .collect::<Vec<_>>();
    events.sort_by_key(|event| (event.started_at_epoch_ms(), event.event_id()));
    let mut totals = BTreeMap::<Uuid, DiscardedSessionTaskTotal>::new();
    for event in &events {
        let total = totals
            .entry(event.task_id())
            .or_insert_with(|| DiscardedSessionTaskTotal {
                task_id: event.task_id(),
                task_name_at_latest_start: event.task_name_at_start().to_string(),
                total_seconds: 0,
            });
        total.task_name_at_latest_start = event.task_name_at_start().to_string();
        total.total_seconds += event.elapsed_seconds();
    }
    DiscardedSessionDaySummary {
        logical_date,
        total_seconds: events
            .iter()
            .map(DiscardedSessionEvent::elapsed_seconds)
            .sum(),
        task_totals: totals.into_values().collect(),
        events,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::discarded_session::{
        DiscardedSessionEvent, DiscardedSessionReason, DiscardedSessionSource,
    };
    use chrono::{Duration, Local, TimeZone};
    use uuid::Uuid;

    fn event(
        id: u128,
        task_id: u128,
        name: &str,
        hour: u32,
        seconds: i64,
    ) -> DiscardedSessionEvent {
        let started_at = Local.with_ymd_and_hms(2026, 9, 10, hour, 0, 0).unwrap();
        DiscardedSessionEvent::new(
            Uuid::from_u128(id),
            Uuid::from_u128(task_id),
            name.to_string(),
            started_at,
            started_at + Duration::seconds(seconds),
            DiscardedSessionSource::Cli,
            DiscardedSessionReason::CliUnfocus,
        )
        .unwrap()
        .unwrap()
    }

    #[test]
    fn 日次集計はtotalとtask別totalと開始時刻順eventを返す() {
        let logical_date = chrono::NaiveDate::from_ymd_opt(2026, 9, 10).unwrap();
        let events = vec![
            event(2, 10, "旧名", 9, 90),
            event(1, 10, "新名", 7, 30),
            event(3, 20, "別task", 8, 60),
        ];

        let summary = summarize_discarded_sessions(&events, logical_date);

        assert_eq!(summary.total_seconds(), 180);
        assert_eq!(summary.task_totals().len(), 2);
        assert_eq!(summary.task_totals()[0].task_id(), Uuid::from_u128(10));
        assert_eq!(summary.task_totals()[0].total_seconds(), 120);
        assert_eq!(summary.task_totals()[1].total_seconds(), 60);
        assert_eq!(
            summary
                .events()
                .iter()
                .map(DiscardedSessionEvent::event_id)
                .collect::<Vec<_>>(),
            vec![Uuid::from_u128(1), Uuid::from_u128(3), Uuid::from_u128(2)]
        );
        assert_eq!(summary.events()[0].task_name_at_start(), "新名");
        assert_eq!(summary.events()[2].task_name_at_start(), "旧名");
    }
}
