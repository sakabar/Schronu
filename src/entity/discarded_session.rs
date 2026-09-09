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
