use super::*;

fn parse_focus_selection_mode_command(line: &str) -> Option<FocusSelectionMode> {
    parse_command(line, ParseMode::Interactive)
        .ok()
        .and_then(|command| focus_selection_mode_from_command(&command))
}

#[test]
fn test_get_adjustable_prefix_label_前倒し可能日数を表示する() {
    let task = new_test_task_handle("タスク").unwrap();
    task.set_start_time(Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap());
    let dt = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

    let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time).unwrap();

    assert_eq!(actual, "【前3】");
}

#[test]
fn test_get_adjustable_prefix_label_今日より前には戻さない() {
    let task = new_test_task_handle("タスク").unwrap();
    task.set_start_time(Local.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap());
    let dt = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

    let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time).unwrap();

    assert_eq!(actual, "【前3】");
}

#[test]
fn test_get_adjustable_prefix_label_同日着手可能なら表示しない() {
    let task = new_test_task_handle("タスク").unwrap();
    task.set_start_time(Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap());
    let dt = Local.with_ymd_and_hms(2026, 5, 10, 18, 0, 0).unwrap();
    let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

    let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time).unwrap();

    assert_eq!(actual, "");
}

#[test]
fn test_get_adjustable_prefix_label_今日と予定日が同じなら過去の着手可能日は表示しない() {
    let task = new_test_task_handle("タスク").unwrap();
    task.set_start_time(Local.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap());
    let dt = Local.with_ymd_and_hms(2026, 5, 7, 18, 0, 0).unwrap();
    let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

    let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time).unwrap();

    assert_eq!(actual, "");
}

#[test]
fn test_get_adjustable_prefix_label_相手待ちは表示しない() {
    let task = new_test_task_handle("タスク").unwrap();
    task.set_start_time(Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap());
    task.set_is_on_other_side(true);
    let dt = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

    let actual = get_adjustable_prefix_label(&task, dt, 0, last_synced_time).unwrap();

    assert_eq!(actual, "");
}

#[test]
fn test_get_adjustable_prefix_label_葉以外は表示しない() {
    let task = new_test_task_handle("タスク").unwrap();
    task.set_start_time(Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap());
    let dt = Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
    let last_synced_time = Local.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();

    let actual = get_adjustable_prefix_label(&task, dt, 1, last_synced_time).unwrap();

    assert_eq!(actual, "");
}

#[test]
fn test_parse_clear_or_gather_defer_to_datetime_空の分指定は現在時刻からの分として解釈する() {
    let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

    let actual = parse_clear_or_gather_defer_to_datetime("空", "120", now);

    assert_eq!(actual, Ok(Some(now + Duration::minutes(120))));
}

#[test]
fn test_parse_clear_or_gather_defer_to_datetime_hhmm指定は従来通り当日の時刻として解釈する() {
    let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

    let actual = parse_clear_or_gather_defer_to_datetime("空", "10:00", now);

    assert_eq!(
        actual,
        Ok(Some(Local.with_ymd_and_hms(2026, 5, 7, 10, 0, 0).unwrap()))
    );
}

#[test]
fn test_parse_clear_or_gather_defer_to_datetime_集の分指定は現在時刻からの分として解釈する() {
    let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

    let actual = parse_clear_or_gather_defer_to_datetime("集", "120", now);

    assert_eq!(actual, Ok(Some(now + Duration::minutes(120))));
}

#[test]
fn test_parse_clear_or_gather_defer_to_datetime_不正なcalendar時刻を拒否する() {
    let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

    assert_eq!(
        parse_clear_or_gather_defer_to_datetime("空", "13:99", now),
        Ok(None)
    );
}

#[test]
fn test_parse_clear_or_gather_defer_to_datetime_i64範囲外のminutesを拒否する() {
    let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

    assert_eq!(
        parse_clear_or_gather_defer_to_datetime("空", "9223372036854775808", now),
        Ok(None)
    );
}

#[test]
fn test_parse_clear_or_gather_defer_to_datetime_minutesの日時範囲外を情報付きerrorにする() {
    let now = Local.with_ymd_and_hms(2026, 5, 7, 12, 34, 56).unwrap();

    assert_eq!(
        parse_clear_or_gather_defer_to_datetime("空", "9223372036854775807", now),
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "clear_or_gather_minutes",
            datetime: now,
        })
    );
}

#[test]
fn test_parse_dated_clear_or_gather_time_range_深夜と24時以降を指定業務日へ対応付ける() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let start = Local.with_ymd_and_hms(2026, 8, 15, 6, 0, 0).unwrap();

    assert_eq!(
        parse_dated_clear_or_gather_time_range("03:00", "8/15", now),
        Ok(Some((
            start,
            Local.with_ymd_and_hms(2026, 8, 16, 3, 0, 0).unwrap()
        )))
    );
    assert_eq!(
        parse_dated_clear_or_gather_time_range("24:30", "8/15", now),
        Ok(Some((
            start,
            Local.with_ymd_and_hms(2026, 8, 16, 0, 30, 0).unwrap()
        )))
    );
}

#[test]
fn test_resolve_dated_clear_or_gather_end_naive_最終壁時計日付を変換前に確定する() {
    let day_start = NaiveDate::from_ymd_opt(2026, 3, 28)
        .unwrap()
        .and_hms_opt(6, 0, 0)
        .unwrap();

    assert_eq!(
        resolve_dated_clear_or_gather_end_naive(day_start, 24, 30),
        NaiveDate::from_ymd_opt(2026, 3, 29)
            .unwrap()
            .and_hms_opt(0, 30, 0)
    );
    assert_eq!(
        resolve_dated_clear_or_gather_end_naive(day_start, 3, 0),
        NaiveDate::from_ymd_opt(2026, 3, 29)
            .unwrap()
            .and_hms_opt(3, 0, 0)
    );
}

#[test]
fn test_parse_dated_clear_or_gather_time_range_不正値と空区間を拒否する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    for time in ["120", "06:00", "10:60", "invalid", "9223372036854775807:00"] {
        assert_eq!(
            parse_dated_clear_or_gather_time_range(time, "8/15", now),
            Ok(None)
        );
    }
    assert_eq!(
        parse_dated_clear_or_gather_time_range("13:00", "13/40", now),
        Ok(None)
    );
}

#[test]
fn test_parse_focus_selection_mode_command_low() {
    assert_eq!(
        parse_focus_selection_mode_command("低"),
        Some(FocusSelectionMode::LowestPriority {
            recent_days: DEFAULT_LOWEST_PRIORITY_RECENT_DAYS
        })
    );
    assert_eq!(
        parse_focus_selection_mode_command("low"),
        Some(FocusSelectionMode::LowestPriority {
            recent_days: DEFAULT_LOWEST_PRIORITY_RECENT_DAYS
        })
    );
}

#[test]
fn test_parse_focus_selection_mode_command_low_with_recent_days() {
    assert_eq!(
        parse_focus_selection_mode_command("低 0"),
        Some(FocusSelectionMode::LowestPriority { recent_days: 0 })
    );
    assert_eq!(
        parse_focus_selection_mode_command("low 0"),
        Some(FocusSelectionMode::LowestPriority { recent_days: 0 })
    );
    assert_eq!(
        parse_focus_selection_mode_command("lo 3"),
        Some(FocusSelectionMode::LowestPriority { recent_days: 3 })
    );
    assert_eq!(
        parse_focus_selection_mode_command("lowest 12"),
        Some(FocusSelectionMode::LowestPriority { recent_days: 12 })
    );
}

#[test]
fn test_parse_focus_selection_mode_command_high() {
    assert_eq!(
        parse_focus_selection_mode_command("高"),
        Some(FocusSelectionMode::HighestPriority)
    );
    assert_eq!(
        parse_focus_selection_mode_command("high"),
        Some(FocusSelectionMode::HighestPriority)
    );
}

#[test]
fn test_parse_focus_selection_mode_command_trims_spaces() {
    assert_eq!(
        parse_focus_selection_mode_command("  low  "),
        Some(FocusSelectionMode::LowestPriority {
            recent_days: DEFAULT_LOWEST_PRIORITY_RECENT_DAYS
        })
    );
    assert_eq!(
        parse_focus_selection_mode_command("  高  "),
        Some(FocusSelectionMode::HighestPriority)
    );
}

#[test]
fn test_parse_focus_selection_mode_command_unknown() {
    assert_eq!(parse_focus_selection_mode_command("後 7日"), None);
    assert_eq!(parse_focus_selection_mode_command("低 abc"), None);
    assert_eq!(parse_focus_selection_mode_command("低 -1"), None);
    assert_eq!(parse_focus_selection_mode_command("低 1 2"), None);
}

#[test]
fn test_execute_set_priority_優先度を変更する() {
    let task = new_test_task_handle("タスク").unwrap();
    let focused_task_opt = Some(task.clone());

    execute_set_priority(&focused_task_opt, "8");

    assert_eq!(task.get_priority().unwrap(), 8);
}

#[test]
fn test_execute_set_priority_不正値なら変更しない() {
    let task = new_test_task_handle("タスク").unwrap();
    task.set_priority(5);
    let focused_task_opt = Some(task.clone());

    execute_set_priority(&focused_task_opt, "invalid");

    assert_eq!(task.get_priority().unwrap(), 5);
}

#[test]
fn test_execute_set_priority_フォーカスなしなら何もしない() {
    let focused_task_opt = None;

    execute_set_priority(&focused_task_opt, "8");
}

#[test]
fn test_advance_display_datetime_cursor_過去の終了時刻では巻き戻さない() {
    let current_datetime_cursor = Local.with_ymd_and_hms(2026, 5, 10, 14, 54, 0).unwrap();
    let end_datetime = Local.with_ymd_and_hms(2026, 5, 10, 14, 2, 0).unwrap();

    let actual = advance_display_datetime_cursor(current_datetime_cursor, end_datetime);

    assert_eq!(actual, current_datetime_cursor);
}

#[test]
fn test_advance_display_datetime_cursor_未来の終了時刻には進める() {
    let current_datetime_cursor = Local.with_ymd_and_hms(2026, 5, 10, 14, 2, 0).unwrap();
    let end_datetime = Local.with_ymd_and_hms(2026, 5, 10, 14, 54, 0).unwrap();

    let actual = advance_display_datetime_cursor(current_datetime_cursor, end_datetime);

    assert_eq!(actual, end_datetime);
}

#[test]
fn test_sort_task_list_display_rows_通常表示は予定時刻の逆順にする() {
    let early_id = Uuid::new_v4();
    let late_id = Uuid::new_v4();
    let target_date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
    let mut rows = vec![
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
            target_date,
            0,
            early_id,
            10,
            60,
            None,
            "".to_string(),
            "early".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
            target_date,
            0,
            late_id,
            1,
            60,
            None,
            "".to_string(),
            "late".to_string(),
        ),
    ];

    sort_task_list_display_rows(&mut rows, TaskListDisplayOrder::ScheduledStartDesc);

    assert_eq!(
        rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        vec![late_id, early_id]
    );
}

#[test]
fn test_sort_task_list_display_rows_尾表示は低優先度を下側にする() {
    let high_priority_id = Uuid::new_v4();
    let low_priority_id = Uuid::new_v4();
    let target_date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
    let mut rows = vec![
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
            target_date,
            0,
            high_priority_id,
            10,
            60,
            None,
            "".to_string(),
            "high".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
            target_date,
            0,
            low_priority_id,
            1,
            60,
            None,
            "".to_string(),
            "low".to_string(),
        ),
    ];

    sort_task_list_display_rows(&mut rows, TaskListDisplayOrder::LowPriorityTail);

    assert_eq!(
        rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        vec![high_priority_id, low_priority_id]
    );
}

#[test]
fn test_sort_task_list_display_rows_尾表示で同じ優先度なら予定時刻が遅いものを下側にする() {
    let early_id = Uuid::new_v4();
    let late_id = Uuid::new_v4();
    let target_date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
    let mut rows = vec![
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
            target_date,
            0,
            early_id,
            1,
            60,
            None,
            "".to_string(),
            "early".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
            target_date,
            0,
            late_id,
            1,
            60,
            None,
            "".to_string(),
            "late".to_string(),
        ),
    ];

    sort_task_list_display_rows(&mut rows, TaskListDisplayOrder::LowPriorityTail);

    assert_eq!(
        rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        vec![early_id, late_id]
    );
}

#[test]
fn test_mark_give_up_candidate_rows_低優先度側から不足時間を満たすまで印を付ける() {
    let target_date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
    let high_id = Uuid::new_v4();
    let nineteen_min_id = Uuid::new_v4();
    let twenty_min_id = Uuid::new_v4();
    let fifteen_min_id = Uuid::new_v4();
    let six_min_id = Uuid::new_v4();
    let thirteen_min_id = Uuid::new_v4();
    let eighteen_min_id = Uuid::new_v4();
    let mut rows = vec![
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 21, 0, 0).unwrap(),
            target_date,
            0,
            high_id,
            89,
            120 * 60,
            None,
            "prefix ".to_string(),
            "high".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 23, 11, 0).unwrap(),
            target_date,
            0,
            nineteen_min_id,
            5,
            19 * 60,
            None,
            "0001 00000000-0000-0000-0000-000000000000 / ____/__/__ 05/10(日)-23:11~23:30 0 19 05 "
                .to_string(),
            "<19/60>レビュー".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 22, 36, 0).unwrap(),
            target_date,
            1,
            twenty_min_id,
            5,
            20 * 60,
            None,
            "prefix ".to_string(),
            "回収する".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 22, 21, 0).unwrap(),
            target_date,
            0,
            fifteen_min_id,
            5,
            15 * 60,
            None,
            "prefix ".to_string(),
            "心当たりがある店に電話して確認".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 22, 16, 0).unwrap(),
            target_date,
            0,
            six_min_id,
            5,
            6 * 60,
            None,
            "prefix ".to_string(),
            "日から土までの実績を確認する".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 22, 3, 0).unwrap(),
            target_date,
            0,
            thirteen_min_id,
            5,
            13 * 60,
            None,
            "prefix ".to_string(),
            "<13/30>一次レビュー".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 21, 42, 0).unwrap(),
            target_date,
            0,
            eighteen_min_id,
            5,
            18 * 60,
            None,
            "prefix ".to_string(),
            "<18/30>一次レビュー".to_string(),
        ),
    ];

    mark_give_up_candidate_rows(&mut rows, 83 * 60, target_date);

    let give_up_ids = rows
        .iter()
        .filter_map(|row| {
            if row.give_up_candidate {
                Some(row.id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    assert_eq!(
        give_up_ids,
        vec![
            nineteen_min_id,
            twenty_min_id,
            fifteen_min_id,
            six_min_id,
            thirteen_min_id,
            eighteen_min_id
        ]
    );
    let rendered = rows
        .iter()
        .find(|row| row.id == nineteen_min_id)
        .unwrap()
        .render_message();
    assert!(rendered.contains(" A "));
    assert!(rendered.ends_with("<19/60>レビュー"));
    assert!(
        !rows
            .iter()
            .find(|row| row.id == high_id)
            .unwrap()
            .give_up_candidate
    );
}

#[test]
fn test_mark_give_up_candidate_rows_空き時間行と別日は候補にしない() {
    let target_date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
    let other_date = NaiveDate::from_ymd_opt(2026, 5, 11).unwrap();
    let target_id = Uuid::new_v4();
    let other_date_id = Uuid::new_v4();
    let blank_id = Uuid::new_v4();
    let mut rows = vec![
        TaskListDisplayRow::new_message(
            Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
            0,
            blank_id,
            0,
            "空き時間".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 11, 12, 0, 0).unwrap(),
            other_date,
            0,
            other_date_id,
            1,
            60 * 60,
            None,
            "".to_string(),
            "tomorrow".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 11, 0, 0).unwrap(),
            target_date,
            0,
            target_id,
            10,
            30 * 60,
            None,
            "".to_string(),
            "today".to_string(),
        ),
    ];

    mark_give_up_candidate_rows(&mut rows, 10 * 60, target_date);

    assert!(
        !rows
            .iter()
            .find(|row| row.id == blank_id)
            .unwrap()
            .give_up_candidate
    );
    assert!(
        !rows
            .iter()
            .find(|row| row.id == other_date_id)
            .unwrap()
            .give_up_candidate
    );
    assert!(
        rows.iter()
            .find(|row| row.id == target_id)
            .unwrap()
            .give_up_candidate
    );
}

#[test]
fn test_mark_give_up_candidate_rows_不足なしなら印を付けない() {
    let target_date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
    let id = Uuid::new_v4();
    let mut rows = vec![TaskListDisplayRow::new_task(
        Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
        target_date,
        0,
        id,
        1,
        60 * 60,
        None,
        "".to_string(),
        "task".to_string(),
    )];

    mark_give_up_candidate_rows(&mut rows, 0, target_date);

    assert!(!rows[0].give_up_candidate);
}

#[test]
fn test_mark_give_up_candidate_rows_by_date_未来日にも空差累に応じて印を付ける() {
    let today = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
    let tomorrow = NaiveDate::from_ymd_opt(2026, 5, 11).unwrap();
    let today_id = Uuid::new_v4();
    let tomorrow_high_id = Uuid::new_v4();
    let tomorrow_low_late_id = Uuid::new_v4();
    let tomorrow_low_early_id = Uuid::new_v4();
    let mut rows = vec![
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
            today,
            0,
            today_id,
            1,
            60 * 60,
            None,
            "prefix ".to_string(),
            "today".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 11, 10, 0, 0).unwrap(),
            tomorrow,
            0,
            tomorrow_high_id,
            10,
            60 * 60,
            None,
            "prefix ".to_string(),
            "tomorrow high".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 11, 18, 0, 0).unwrap(),
            tomorrow,
            0,
            tomorrow_low_late_id,
            1,
            45 * 60,
            None,
            "prefix ".to_string(),
            "tomorrow low late".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 11, 13, 0, 0).unwrap(),
            tomorrow,
            0,
            tomorrow_low_early_id,
            1,
            30 * 60,
            None,
            "prefix ".to_string(),
            "tomorrow low early".to_string(),
        ),
    ];
    let mut shortage_duration_by_date = HashMap::new();
    shortage_duration_by_date.insert(today, Duration::seconds(0));
    shortage_duration_by_date.insert(tomorrow, Duration::minutes(50));

    mark_give_up_candidate_rows_by_date(&mut rows, &shortage_duration_by_date);

    let give_up_ids = rows
        .iter()
        .filter_map(|row| {
            if row.give_up_candidate {
                Some(row.id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    assert_eq!(
        give_up_ids,
        vec![tomorrow_low_late_id, tomorrow_low_early_id]
    );
    assert!(
        !rows
            .iter()
            .find(|row| row.id == today_id)
            .unwrap()
            .give_up_candidate
    );
    assert!(
        !rows
            .iter()
            .find(|row| row.id == tomorrow_high_id)
            .unwrap()
            .give_up_candidate
    );
}

#[test]
fn test_replace_task_list_icon_アイコン列だけを置き換える() {
    let message_prefix =
        "0028 task-id / ____/__/__ 06/28(日)-23:11~23:30 0 19 05 資 夕食  の 準備".to_string();

    let actual = replace_task_list_icon(&message_prefix, "A");

    assert_eq!(
        actual,
        "0028 task-id A ____/__/__ 06/28(日)-23:11~23:30 0 19 05 資 夕食  の 準備"
    );
}

#[test]
fn test_project_category_symbol_カテゴリ表示記号を返す() {
    assert_eq!(
        project_category_symbol(Some(ProjectCategory::Earning)),
        "獲"
    );
    assert_eq!(
        project_category_symbol(Some(ProjectCategory::Sustaining)),
        "維"
    );
    assert_eq!(
        project_category_symbol(Some(ProjectCategory::Recovery)),
        "回"
    );
    assert_eq!(
        project_category_symbol(Some(ProjectCategory::Investment)),
        "資"
    );
    assert_eq!(
        project_category_symbol(Some(ProjectCategory::Consumption)),
        "消"
    );
    assert_eq!(project_category_symbol(None), "_");
}

#[test]
fn test_format_focused_task_header_project_categoryを表示する() {
    assert_eq!(
        format_focused_task_header(Some(ProjectCategory::Investment)),
        "focused task is: project_category=資"
    );
    assert_eq!(
        format_focused_task_header(None),
        "focused task is: project_category=_"
    );
}

#[test]
fn test_summarize_scheduled_work_seconds_by_project_category_実タスクだけをカテゴリ別に集計する() {
    let target_date = NaiveDate::from_ymd_opt(2026, 5, 10).unwrap();
    let rows = vec![
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap(),
            target_date,
            0,
            Uuid::new_v4(),
            1,
            60 * 60,
            Some(ProjectCategory::Earning),
            "".to_string(),
            "earning".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 13, 0, 0).unwrap(),
            target_date,
            0,
            Uuid::new_v4(),
            1,
            30 * 60,
            Some(ProjectCategory::Investment),
            "".to_string(),
            "investment".to_string(),
        ),
        TaskListDisplayRow::new_task(
            Local.with_ymd_and_hms(2026, 5, 10, 14, 0, 0).unwrap(),
            target_date,
            0,
            Uuid::new_v4(),
            1,
            30 * 60,
            None,
            "".to_string(),
            "uncategorized".to_string(),
        ),
        TaskListDisplayRow::new_message(
            Local.with_ymd_and_hms(2026, 5, 10, 15, 0, 0).unwrap(),
            0,
            Uuid::new_v4(),
            1,
            "message".to_string(),
        ),
    ];

    let summary = summarize_scheduled_work_seconds_by_project_category(&rows);

    assert_eq!(summary[0], 60 * 60);
    assert_eq!(summary[3], 30 * 60);
    assert_eq!(summary[5], 30 * 60);
}

#[test]
fn test_format_scheduled_work_seconds_by_project_category_比率を表示する() {
    let summary = [60 * 60, 0, 0, 30 * 60, 0, 30 * 60];

    let actual = format_scheduled_work_seconds_by_project_category(&summary, 2 * 60 * 60);

    assert_eq!(
        actual,
        "予定カテゴリ: 獲得 1.0時間(50% | 50%) / 維持 0.0時間(0% | 50%) / 回復 0.0時間(0% | 50%) / 投資 0.5時間(25% | 75%) / 消費 0.0時間(0% | 75%) / 未分類 0.5時間(25% | 100%)"
    );
}

#[test]
fn test_format_scheduled_work_seconds_by_project_category_空き時間超過を表示する() {
    let summary = [60 * 60, 0, 0, 30 * 60, 0, 30 * 60];

    let actual = format_scheduled_work_seconds_by_project_category(&summary, 60 * 60);

    assert_eq!(
        actual,
        "予定カテゴリ: 獲得 1.0時間(100% | 100%) / 維持 0.0時間(0% | 100%) / 回復 0.0時間(0% | 100%) / 投資 0.5時間(50% | 150%) / 消費 0.0時間(0% | 150%) / 未分類 0.5時間(50% | 200%)"
    );
}

#[test]
fn test_format_scheduled_work_seconds_by_project_category_空き時間なし() {
    let summary = [60 * 60, 0, 0, 0, 0, 0];

    let actual = format_scheduled_work_seconds_by_project_category(&summary, 0);

    assert_eq!(
        actual,
        "予定カテゴリ: 獲得 1.0時間(inf% | inf%) / 維持 0.0時間(0% | inf%) / 回復 0.0時間(0% | inf%) / 投資 0.0時間(0% | inf%) / 消費 0.0時間(0% | inf%) / 未分類 0.0時間(0% | inf%)"
    );
}

#[test]
fn test_format_scheduled_work_seconds_by_project_category_予定なし() {
    let summary = [0; PROJECT_CATEGORY_SUMMARY_LEN];

    let actual = format_scheduled_work_seconds_by_project_category(&summary, 0);

    assert_eq!(actual, "予定カテゴリ: 予定なし");
}
