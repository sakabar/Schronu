#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::discarded_session::{
        DiscardedSessionEvent, DiscardedSessionReason, DiscardedSessionSource,
    };
    use chrono::{Duration, Local, TimeZone};
    use uuid::Uuid;

    fn event(id: u128, task_id: u128, name: &str, hour: u32, seconds: i64) -> DiscardedSessionEvent {
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
