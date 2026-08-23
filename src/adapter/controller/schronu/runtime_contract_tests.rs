#[test]
fn test_resolve_upcoming_mmdd_未来の日付は現在年を使う() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let target_date = Local.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap();
    let expected = try_next_business_day_start(target_date).unwrap() - Duration::days(1);

    assert_eq!(resolve_upcoming_mmdd("9/26", now), Ok(Some(expected)));
}

#[test]
fn test_resolve_upcoming_mmdd_過去の日付は翌年を使う() {
    let now = Local.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();
    let target_date = Local.with_ymd_and_hms(2027, 9, 26, 12, 0, 0).unwrap();
    let expected = try_next_business_day_start(target_date).unwrap() - Duration::days(1);

    assert_eq!(resolve_upcoming_mmdd("09/26", now), Ok(Some(expected)));
}

#[test]
fn test_resolve_upcoming_mmdd_当日の境界時刻は現在年を使う() {
    let target_date = Local.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap();
    let now = try_next_business_day_start(target_date).unwrap() - Duration::days(1);

    assert_eq!(resolve_upcoming_mmdd("9/26", now), Ok(Some(now)));
}

#[test]
fn test_resolve_upcoming_clear_or_gather_day_明は次の業務日を返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    assert_eq!(
        resolve_upcoming_clear_or_gather_day("明", now),
        Ok(Some(Local.with_ymd_and_hms(2026, 8, 15, 6, 0, 0).unwrap()))
    );
}

#[test]
fn test_resolve_upcoming_clear_or_gather_day_曜日は明日以降で最も近い日を返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    for (weekday, day) in [
        ("月", 17),
        ("火", 18),
        ("水", 19),
        ("木", 20),
        ("金", 21),
        ("土", 15),
        ("日", 16),
    ] {
        assert_eq!(
            resolve_upcoming_clear_or_gather_day(weekday, now),
            Ok(Some(Local.with_ymd_and_hms(2026, 8, day, 6, 0, 0).unwrap()))
        );
    }
}

#[test]
fn test_resolve_upcoming_clear_or_gather_day_午前6時前の明と不正値を扱う() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 2, 0, 0).unwrap();

    assert_eq!(
        resolve_upcoming_clear_or_gather_day("明", now),
        Ok(Some(Local.with_ymd_and_hms(2026, 8, 14, 6, 0, 0).unwrap()))
    );
    assert_eq!(resolve_upcoming_clear_or_gather_day("翌", now), Ok(None));
}

#[test]
fn test_resolve_upcoming_clear_or_gather_day_業務日計算不能を情報付きerrorにする() {
    let now = maximum_local_datetime();

    assert_eq!(
        resolve_upcoming_clear_or_gather_day("明", now),
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "next_business_day_start",
            datetime: now,
        })
    );
}

#[test]
fn test_resolve_upcoming_clear_or_gather_day_曜日範囲外は曜日計算errorにする() {
    let now = maximum_local_datetime();

    assert_eq!(
        resolve_upcoming_clear_or_gather_day("月", now),
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "weekday_date",
            datetime: now,
        })
    );
}

#[test]
fn test_resolve_upcoming_clear_or_gather_day_mmddの翌年計算不能を情報付きerrorにする() {
    let now = maximum_local_datetime();

    assert_eq!(
        resolve_upcoming_clear_or_gather_day("12/31", now),
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "upcoming_calendar_date",
            datetime: now,
        })
    );
}

#[test]
fn test_resolve_show_all_pattern_年なし日付を完全日付へ変換する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    assert_eq!(
        resolve_show_all_pattern("9/26", now),
        Ok("2026/09/26".to_string())
    );
}

#[test]
fn test_resolve_show_all_pattern_過ぎた日付は翌年へ変換する() {
    let now = Local.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();

    assert_eq!(
        resolve_show_all_pattern("9/26", now),
        Ok("2027/09/26".to_string())
    );
}

#[test]
fn test_resolve_show_all_pattern_完全日付と検索語は変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    assert_eq!(
        resolve_show_all_pattern("2026/09/26", now),
        Ok("2026/09/26".to_string())
    );
    assert_eq!(
        resolve_show_all_pattern("タスク", now),
        Ok("タスク".to_string())
    );
}

#[test]
fn test_show_task_list_mmddの日時errorを伝搬して表示と状態を変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("show all日時範囲外対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut display = TestWriter::new();
    let mut next_id = || Uuid::nil();
    let mut task_factory = TaskFactory::new(now, &mut next_id);
    let mut context = RuntimeTaskTreeCommandContext {
        task_repository: &mut task_repository,
        free_time_manager: &mut free_time_manager,
        focused_task_id_opt: &mut focused_task_id_opt,
        task_factory: &mut task_factory,
        config: active_config(),
        supports_ansi_color: false,
    };

    let actual = context.show_task_list(
        &mut display,
        Some("12/31"),
        TaskListOrder::ScheduledStartDesc,
        true,
    );

    assert!(matches!(
        actual,
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "upcoming_calendar_date",
            datetime,
        }) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(display.into_string().is_empty());
}

#[test]
fn test_show_task_listの不正な完全日付をerrorにして表示と状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("show all不正日付対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut display = TestWriter::new();
    let mut next_id = || Uuid::nil();
    let mut task_factory = TaskFactory::new(now, &mut next_id);
    let mut context = RuntimeTaskTreeCommandContext {
        task_repository: &mut task_repository,
        free_time_manager: &mut free_time_manager,
        focused_task_id_opt: &mut focused_task_id_opt,
        task_factory: &mut task_factory,
        config: active_config(),
        supports_ansi_color: false,
    };

    let actual = context.show_task_list(
        &mut display,
        Some("2026/02/30"),
        TaskListOrder::ScheduledStartDesc,
        true,
    );

    assert_eq!(
        actual,
        Err(ApplicationError::InvalidInput {
            field: "pattern",
            reason: "invalid calendar date",
        })
    );
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(display.into_string().is_empty());
}

#[test]
fn test_backward_width_正常系1() {
    let s = String::from("あ");
    let cursor_x = 1;
    let actual = backward_width(&s, cursor_x);
    let expected = 2;
    assert_eq!(actual, expected);
}

#[test]
fn test_backward_width_異常系1() {
    let s = String::from("");
    let cursor_x = 10;
    let actual = backward_width(&s, cursor_x);
    let expected = 0;
    assert_eq!(actual, expected);
}

#[test]
fn test_backward_width_異常系2() {
    let s = String::from("テスト");
    let cursor_x = 0;
    let actual = backward_width(&s, cursor_x);
    let expected = 0;
    assert_eq!(actual, expected);
}

#[test]
fn test_calculate_rho_metrics_単発作業量に端数が漏れないこと() {
    let actual = calculate_rho_metrics(61, 61, 120);

    assert_eq!(actual.non_repetitive_work_hours, 0.0);
    assert_eq!(actual.non_repetitive_rho, 0.0);
}

#[test]
fn test_calculate_rho_metrics_混在ケースでも整合すること() {
    let actual = calculate_rho_metrics(5400, 1800, 120);

    assert!((actual._total_work_hours - 1.5).abs() < 1e-9);
    assert!((actual.repetitive_work_hours - 0.5).abs() < 1e-9);
    assert!((actual.non_repetitive_work_hours - 1.0).abs() < 1e-9);
    assert!((actual._available_hours - 2.0).abs() < 1e-9);
    assert!((actual.free_hours - 0.5).abs() < 1e-9);
    assert!((actual.rho - 0.75).abs() < 1e-9);
    assert!((actual.non_repetitive_rho - (1.0 / 1.5)).abs() < 1e-9);
}

#[test]
fn test_calculate_lq_opt_負荷率が1以上ならinf扱いになること() {
    assert_eq!(calculate_lq_opt(1.0), None);
    assert_eq!(calculate_lq_opt(f64::INFINITY), None);
}

#[test]
fn test_get_byte_offset_for_insert_正常系1() {
    // "|"
    let line = String::from("");
    let cursor_x: usize = 0;
    let actual = get_byte_offset_for_insert(&line, cursor_x);
    let expected = 0;
    assert_eq!(actual, expected);
}

#[test]
fn test_get_byte_offset_for_insert_正常系2() {
    // |例1の文字列
    let line = String::from("例1の文字列");
    let cursor_x: usize = 0;
    let actual = get_byte_offset_for_insert(&line, cursor_x);
    let expected = 0;
    assert_eq!(actual, expected);
}

#[test]
fn test_get_byte_offset_for_insert_正常系3() {
    // 例1の|文字列
    let line = String::from("例1の文字列");
    let cursor_x: usize = 3;
    let actual = get_byte_offset_for_insert(&line, cursor_x);
    let expected = String::from("例1の").len(); // 3+1+3=7
    assert_eq!(actual, expected);
}

#[test]
fn test_get_byte_offset_for_insert_正常系4() {
    // あ|
    let line = String::from("あ");
    let cursor_x: usize = 1;
    let actual = get_byte_offset_for_insert(&line, cursor_x);
    let expected = String::from("あ").len(); // 3
    assert_eq!(actual, expected);
}

#[test]
fn test_get_width_for_rerender_正常系_アスキー() {
    let header = String::from("schronu>");
    let line = String::from("project new");
    let cursor_x = 3;

    let actual = get_width_for_rerender(&header, &line, cursor_x);
    let expected = 11; // "schronu>pro"
    assert_eq!(actual, expected);
}

#[test]
fn test_get_width_for_rerender_正常系_多バイト1() {
    let header = String::from("schronu>");
    let line = String::from("breakdown タク1"); // 「ス」を入れたい
    let cursor_x = 11;

    let actual = get_width_for_rerender(&header, &line, cursor_x);
    let expected = 20; // "schronu>breakdown タ"
    assert_eq!(actual, expected);
}

#[test]
fn test_get_width_for_rerender_正常系_多バイト2() {
    let header = String::from("schronu>");
    let line = String::from("あい");
    let cursor_x = 2;

    let actual = get_width_for_rerender(&header, &line, cursor_x);
    let expected = 12; // "schronu>あい"
    assert_eq!(actual, expected);
}

#[test]
fn test_get_forward_width_正常系1() {
    let line = String::from("あ");
    let cursor_x = 0;

    let actual = get_forward_width(&line, cursor_x);
    let expected = 2;
    assert_eq!(actual, expected);
}

#[test]
fn test_extract_url_正常系() {
    let input = "これはhttps://example.com?param1=hoge&param2=barというURLです。";
    let actual = extract_url(input);
    let expected = Some(String::from("https://example.com?param1=hoge&param2=bar"));

    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_extract_url_正常系_URLが2つ() {
    let input = "これはhttps://example.com?param1=hoge&param2=barとhttps://example.com";
    let actual = extract_url(input);
    let expected = Some(String::from("https://example.com?param1=hoge&param2=bar"));

    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_extract_url_正常系_2つのURLがスペース区切り() {
    let input = "これはhttps://example.com?param1=hoge&param2=bar https://example.com";
    let actual = extract_url(input);
    let expected = Some(String::from("https://example.com?param1=hoge&param2=bar"));

    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
fn test_extract_url_正常系_正しいURLのまま文字列が終わるケース() {
    let input = "正しいURLのまま文字列が終わるケースhttps://example.com/hoge";
    let actual = extract_url(input);
    let expected = Some(String::from("https://example.com/hoge"));

    assert_eq!(actual, expected);
}

#[test]
fn test_make_obsidian_search_url_task_idをqueryにする() {
    let query = "11111111-1111-1111-1111-111111111111";
    let actual = make_obsidian_search_url(query);
    let expected =
        "obsidian://search?vault=Obsidian-Work&query=11111111-1111-1111-1111-111111111111";

    assert_eq!(actual, expected);
}

#[test]
fn test_make_obsidian_search_url_vault名をpercent_encodeする() {
    let actual = make_obsidian_search_url_with_vault("task id", "Work & Personal");

    assert_eq!(
        actual,
        "obsidian://search?vault=Work%20%26%20Personal&query=task%20id"
    );
}

#[test]
fn test_make_obsidian_root_task_search_url_子タスクからrootのtask_idをqueryにする() {
    let mut root_task = new_test_task_handle("root").unwrap();
    let root_task_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    root_task.set_id(root_task_id);
    let child_task = root_task.create_as_last_child(new_test_task_attr("child"));

    let actual = make_obsidian_root_task_search_url(&child_task);
    let expected =
        "obsidian://search?vault=Obsidian-Work&query=11111111-1111-1111-1111-111111111111";

    assert_eq!(actual, expected);
}

#[test]
fn test_split_amount_and_unit() {
    let input = "暦";
    let actual = split_amount_and_unit(input);

    assert_eq!(actual, vec!["".to_string(), "暦".to_string()]);
}

#[test]
fn test_split_amount_and_unit_err() {
    let input = "6543abc123def456gh789";
    let actual = split_amount_and_unit(input);

    assert_eq!(
        actual,
        vec!["6543".to_string(), "abc123def456gh789".to_string()]
    );
}

#[test]
fn test_decide_time_明_6時以降は次のschronu日付にする() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 15, 0).unwrap();
    let tokens = vec!["始", "7:00", "明"];

    let actual = decide_time(&tokens, &now);
    let expected = Some(Local.with_ymd_and_hms(2026, 5, 18, 7, 0, 0).unwrap());

    assert_eq!(actual, expected);
}

#[test]
fn test_decide_time_明_24時過ぎは直近6時を使う() {
    let now = Local.with_ymd_and_hms(2026, 5, 18, 0, 15, 0).unwrap();
    let tokens = vec!["始", "7:00", "明"];

    let actual = decide_time(&tokens, &now);
    let expected = Some(Local.with_ymd_and_hms(2026, 5, 18, 7, 0, 0).unwrap());

    assert_eq!(actual, expected);
}

#[test]
fn test_decide_finish_time_今は現在時刻を返す() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 15, 0).unwrap();
    let tokens = vec!["終", "今"];

    let actual = decide_finish_time(&tokens, &now);

    assert_eq!(actual, Some(now));
}

#[test]
fn test_decide_finish_time_時刻指定はdecide_timeと同じ形式で解釈する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 15, 0).unwrap();
    let tokens = vec!["終", "7:00", "明"];

    let actual = decide_finish_time(&tokens, &now);
    let expected = Some(Local.with_ymd_and_hms(2026, 5, 18, 7, 0, 0).unwrap());

    assert_eq!(actual, expected);
}

#[test]
fn test_decide_finish_time_秒つき時刻を解釈する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 15, 0).unwrap();
    let tokens = vec!["終", "9:23:45", "2026/7/4"];

    let actual = decide_finish_time(&tokens, &now);
    let expected = Some(Local.with_ymd_and_hms(2026, 7, 4, 9, 23, 45).unwrap());

    assert_eq!(actual, expected);
}

#[test]
fn test_decide_finish_time_不正な時刻は完了時刻にしない() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 15, 0).unwrap();
    let tokens = vec!["終", "xxx"];

    let actual = decide_finish_time(&tokens, &now);

    assert_eq!(actual, None);
}

#[test]
fn test_decide_finish_time_不正な秒は完了時刻にしない() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 15, 0).unwrap();
    let tokens = vec!["終", "9:23:60", "2026/7/4"];

    let actual = decide_finish_time(&tokens, &now);

    assert_eq!(actual, None);
}

#[test]
fn test_decide_finish_time_不正な日付は完了時刻にしない() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 15, 0).unwrap();
    let tokens = vec!["終", "14:30", "xxx"];

    let actual = decide_finish_time(&tokens, &now);

    assert_eq!(actual, None);
}

#[test]
fn test_report_command_resultはtask_tree_errorを既存の操作エラー形式で表示する() {
    let mut stdout = TestWriter::new();

    report_command_result(
        &mut stdout,
        Err(CommandError::Application(ApplicationError::TaskTree(
            TaskTreeError::Borrow,
        ))),
    );

    assert_eq!(
        stdout.into_string(),
        "[Error] 操作エラー: task tree operation failed: cannot borrow task tree data\n"
    );
}

#[test]
fn test_error_display_modelはcommand_errorをwriter固有newlineで表示する() {
    let error = CommandError::Application(ApplicationError::TaskTree(TaskTreeError::Borrow));
    let display = error_display_model(&error);
    let mut stdout = TestWriter::new_with_newline_prefix("<reset>");

    render_display_model(&mut stdout, &display).unwrap();

    assert_eq!(
        stdout.into_string(),
        "<reset>[Error] 操作エラー: task tree operation failed: cannot borrow task tree data\n"
    );

    let mut failing_stdout = FailingNewlineWriter::fail_once();
    let output_error = render_display_model(&mut failing_stdout, &display).unwrap_err();
    assert_eq!(output_error.kind(), std::io::ErrorKind::Other);
    assert_eq!(output_error.to_string(), "newline write failure");
}

#[test]
fn test_execute_空_日付指定は指定日の予定開始時刻でtodoをpendingにする() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let schronu_day_start = Local.with_ymd_and_hms(2026, 8, 15, 6, 0, 0).unwrap();
    let task = new_test_task_handle("日付指定の空対象").unwrap();
    task.set_start_time(schronu_day_start + Duration::hours(4));
    task.set_estimated_work_seconds(30 * 60);
    let task_id = task.get_id().unwrap();
    let original_start_time = task.get_start_time().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "空 13:00 8/15");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        schronu_day_start + Duration::hours(7)
    );
    assert_eq!(result.task.get_start_time().unwrap(), original_start_time);
}

#[test]
fn test_execute_空_明指定は次の業務日の予定をpendingにする() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let schronu_day_start = Local.with_ymd_and_hms(2026, 8, 15, 6, 0, 0).unwrap();
    let task = new_test_task_handle("明指定の空対象").unwrap();
    task.set_start_time(schronu_day_start + Duration::hours(4));
    task.set_estimated_work_seconds(30 * 60);
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "空 13:00 明");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        schronu_day_start + Duration::hours(7)
    );
}

#[test]
fn test_execute_空_日付selectorの業務日計算不能を情報付きerrorにして変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時範囲外の空対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    let actual = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "空 13:00 明",
    );

    assert!(matches!(
        actual,
        Err(CommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "next_business_day_start",
                datetime,
            }
        )) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(stdout.into_string().is_empty());
}

#[test]
fn test_execute_空_mmddの翌年計算不能を情報付きerrorにして変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時範囲外のMMDD空対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    let actual = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "空 13:00 12/31",
    );

    assert!(matches!(
        actual,
        Err(CommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "upcoming_calendar_date",
                datetime,
            }
        )) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(stdout.into_string().is_empty());
}

#[test]
fn test_execute_集_日付指定はpendingを業務日開始へ集める() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let schronu_day_start = Local.with_ymd_and_hms(2026, 8, 15, 6, 0, 0).unwrap();
    let task = new_test_task_handle("日付指定の集対象").unwrap();
    task.set_start_time(schronu_day_start + Duration::hours(4));
    task.set_estimated_work_seconds(30 * 60);
    task.set_orig_status(Status::Pending);
    task.set_pending_until(schronu_day_start + Duration::hours(6));
    let task_id = task.get_id().unwrap();
    let original_start_time = task.get_start_time().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "集 13:00 8/15");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(result.task.get_pending_until().unwrap(), schronu_day_start);
    assert_eq!(result.task.get_start_time().unwrap(), original_start_time);
}

#[test]
fn test_execute_集_曜日指定は次に来る曜日の業務日開始へ集める() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let schronu_day_start = Local.with_ymd_and_hms(2026, 8, 17, 6, 0, 0).unwrap();
    let task = new_test_task_handle("曜日指定の集対象").unwrap();
    task.set_start_time(schronu_day_start + Duration::hours(4));
    task.set_estimated_work_seconds(30 * 60);
    task.set_orig_status(Status::Pending);
    task.set_pending_until(schronu_day_start + Duration::hours(6));
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "集 24:00 月");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(result.task.get_pending_until().unwrap(), schronu_day_start);
}

#[test]
fn test_execute_集_曜日selector範囲外を情報付きerrorにして変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時範囲外の曜日集約対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    let actual = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "集 13:00 月",
    );

    assert!(matches!(
        actual,
        Err(CommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "weekday_date",
                datetime,
            }
        )) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(stdout.into_string().is_empty());
}

#[test]
fn test_execute_空_日付指定はpending_untilの半開区間だけを変更する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let schronu_day_start = Local.with_ymd_and_hms(2026, 8, 15, 6, 0, 0).unwrap();
    let task = new_test_task_handle("日付指定のpending対象").unwrap();
    task.set_start_time(schronu_day_start + Duration::hours(4));
    task.set_estimated_work_seconds(30 * 60);
    task.set_orig_status(Status::Pending);
    task.set_pending_until(schronu_day_start + Duration::hours(5));
    let task_id = task.get_id().unwrap();
    let original_start_time = task.get_start_time().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "clear 13:00 8/15");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        schronu_day_start + Duration::hours(7)
    );
    assert_eq!(result.task.get_start_time().unwrap(), original_start_time);
}

#[test]
fn test_execute_空_日付指定は予定候補外のpendingを変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let schronu_day_start = Local.with_ymd_and_hms(2026, 8, 15, 6, 0, 0).unwrap();
    let task = new_test_task_handle("予定候補外のpending").unwrap();
    task.set_start_time(schronu_day_start + Duration::days(1));
    task.set_estimated_work_seconds(30 * 60);
    task.set_orig_status(Status::Pending);
    let original_pending_until = schronu_day_start + Duration::hours(5);
    task.set_pending_until(original_pending_until);
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "空 13:00 8/15");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        original_pending_until
    );
}

#[test]
fn test_execute_日付指定の不正入力は状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let task = new_test_task_handle("不正入力対象").unwrap();
    task.set_start_time(now);
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "空 06:00 8/15");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Todo);
    assert_eq!(result.task.get_start_time().unwrap(), now);
}

#[test]
fn test_execute_始と約の不正時刻はtask日時を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let original_start = Local.with_ymd_and_hms(2026, 8, 15, 8, 0, 0).unwrap();
    let original_deadline = Local.with_ymd_and_hms(2026, 8, 16, 18, 0, 0).unwrap();

    for command in ["始 invalid", "約 invalid"] {
        let task = new_test_task_handle("不正時刻対象").unwrap();
        task.set_start_time(original_start);
        task.set_deadline_time_opt(Some(original_deadline));
        let task_id = task.get_id().unwrap();

        let result = execute_command_for_test(task, now, Some(task_id), command);

        assert_eq!(result.task.get_start_time().unwrap(), original_start);
        assert_eq!(
            result.task.get_deadline_time_opt().unwrap(),
            Some(original_deadline)
        );
        assert_eq!(result.focused_task_id_opt, Some(task_id));
    }
}

#[test]
fn test_execute_空_2引数は従来通り現在時刻基準で処理する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let task = new_test_task_handle("従来の空対象").unwrap();
    task.set_start_time(now);
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "空 120");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        now + Duration::minutes(120)
    );
}

#[test]
fn test_execute_空と集_2引数の不正なcalendar時刻は変更せず拒否する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    for command in ["空 13:99", "集 13:99"] {
        let task = new_test_task_handle("不正calendar時刻対象").unwrap();
        let task_id = task.get_id().unwrap();
        let original_snapshot = task.snapshot().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut focused_task_id_opt = Some(task_id);
        let mut stdout = TestWriter::new();

        let actual = execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            command,
        );

        assert!(actual.is_ok());
        assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
        assert_eq!(focused_task_id_opt, Some(task_id));
        assert!(stdout.into_string().is_empty());
    }
}

#[test]
fn test_execute_空と集_i64範囲外のminutesは変更せず拒否する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    for command in ["空 9223372036854775808", "集 9223372036854775808"] {
        let task = new_test_task_handle("i64範囲外minutes対象").unwrap();
        let task_id = task.get_id().unwrap();
        let original_snapshot = task.snapshot().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut focused_task_id_opt = Some(task_id);
        let mut stdout = TestWriter::new();

        let actual = execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            command,
        );

        assert!(actual.is_ok());
        assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
        assert_eq!(focused_task_id_opt, Some(task_id));
        assert!(stdout.into_string().is_empty());
    }
}

#[test]
fn test_execute_空と集_minutesの日時範囲外を情報付きerrorにして変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    for command in ["空 9223372036854775807", "集 9223372036854775807"] {
        let task = new_test_task_handle("minutes日時範囲外対象").unwrap();
        let task_id = task.get_id().unwrap();
        let original_snapshot = task.snapshot().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut focused_task_id_opt = Some(task_id);
        let mut stdout = TestWriter::new();

        let actual = execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            command,
        );

        assert!(matches!(
            actual,
            Err(CommandError::Application(
                ApplicationError::SubjectiveDateOutOfRange {
                    operation: "clear_or_gather_minutes",
                    datetime,
                }
            )) if datetime == now
        ));
        assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
        assert_eq!(focused_task_id_opt, Some(task_id));
        assert!(stdout.into_string().is_empty());
    }
}

#[test]
fn test_execute_集_2引数は従来通りtodoへ戻す() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let task = new_test_task_handle("従来の集対象").unwrap();
    task.set_start_time(now);
    task.set_orig_status(Status::Pending);
    task.set_pending_until(now + Duration::minutes(60));
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "集 120");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Todo);
}

#[test]
fn test_execute_pack_前倒し内容と集計を表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("前倒し対象").unwrap();
    task.sync_clock(now);
    task.set_start_time(now);
    task.set_estimated_work_seconds(30 * 60);
    task.set_priority(9);
    task.set_pending_until(now + Duration::days(10));
    task.set_orig_status(Status::Pending);
    let task_id = task.get_id().unwrap();
    let repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager::with_free_minutes(120);
    let mut stdout = TestWriter::new();

    execute_pack(&mut stdout, &repository, &mut free_time_manager);

    let output = stdout.into_string();
    assert!(output.contains(&format!(
        "詰\t2026-08-21\t2026-08-11\t00:30\t優先度9\t{}\t前倒し対象",
        task_id
    )));
    assert!(output.contains("詰: 1件 00:30 (スキップ0件)"));
}

#[test]
fn test_execute_pack_候補なしを表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("対象外").unwrap();
    task.sync_clock(now);
    let repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager::with_free_minutes(120);
    let mut stdout = TestWriter::new();

    execute_pack(&mut stdout, &repository, &mut free_time_manager);

    assert_eq!(
        stdout.into_string(),
        "[Info] 詰められるタスクはありません。\n"
    );
}

#[test]
fn test_execute_pack_収まらない候補はスキップ件数だけを表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("大きい").unwrap();
    task.sync_clock(now);
    task.set_start_time(now);
    task.set_estimated_work_seconds(60 * 60);
    task.set_priority(9);
    task.set_pending_until(now + Duration::days(10));
    task.set_orig_status(Status::Pending);
    let repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager::with_free_minutes(60);
    let mut stdout = TestWriter::new();

    execute_pack(&mut stdout, &repository, &mut free_time_manager);

    let output = stdout.into_string();
    assert!(!output.contains("[Skip]"));
    assert!(!output.contains("大きい"));
    assert!(output.contains("詰: 0件 00:00 (スキップ1件)"));
}

#[test]
fn test_execute_詰とpackの両aliasで製品command経路を実行する() {
    for command in ["詰", "pack"] {
        let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
        let task = new_test_task_handle("対象").unwrap();
        task.sync_clock(now);
        task.set_start_time(now);
        task.set_estimated_work_seconds(30 * 60);
        task.set_pending_until(now + Duration::days(10));
        task.set_orig_status(Status::Pending);
        let mut repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager::with_free_minutes(120);
        let mut stdout = TestWriter::new();
        let mut focused_task_id_opt = None;

        execute(
            &mut stdout,
            &mut repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            command,
        );

        assert!(stdout.into_string().contains("詰: 1件 00:30 (スキップ0件)"));
        assert!(repository.task.get_pending_until().unwrap() < now + Duration::days(10));
    }
}

#[test]
fn test_execute_表示コマンドはwriter固有の改行処理を保持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in ["今", "暦", "帯"] {
        let task = new_test_task_handle("改行処理確認用タスク").unwrap();
        task.set_estimated_work_seconds(60 * 60);
        task.set_start_time(now);
        task.set_pending_until(now);
        task.set_orig_status(Status::Pending);
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager::with_free_minutes(10 * 60);
        let mut focused_task_id_opt = None;
        let mut stdout = TestWriter::new_with_newline_prefix("<reset>");

        execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            command,
        )
        .unwrap();

        let output = stdout.into_string();
        assert!(output.contains("<reset>"), "{command}: {output}");
        assert!(
            output
                .lines()
                .filter(|line| !line.is_empty())
                .all(|line| line.starts_with("<reset>")),
            "{command}: {output}"
        );
    }
}

#[test]
fn test_execute_改行出力の失敗を捕捉して後続出力を継続する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("出力失敗確認用タスク").unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = None;
    let mut stdout = FailingNewlineWriter::fail_once();

    let actual = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "今",
    );

    assert!(matches!(
        actual,
        Err(CommandError::Output(error)) if error.kind() == std::io::ErrorKind::Other
    ));
    assert!(stdout.newline_call_count > 1);
    let output = String::from_utf8(stdout.buffer).unwrap();
    assert!(output.contains("<reset>"), "{output}");
}

#[test]
fn task_tree表示commandは製品経路でtyped_fieldと表示modelを反映する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let root = new_test_task_handle("ROOT").unwrap();
    root.set_estimated_work_seconds(0);
    let mut matched_attr = new_test_task_attr("BOUNDARY_MATCH");
    matched_attr.set_estimated_work_seconds(15 * 60);
    matched_attr.set_start_time(now);
    let matched = root.create_as_last_child(matched_attr);
    matched.sync_clock(now);
    let mut other_attr = new_test_task_attr("BOUNDARY_OTHER");
    other_attr.set_estimated_work_seconds(15 * 60);
    other_attr.set_start_time(now);
    let other = root.create_as_last_child(other_attr);
    other.sync_clock(now);
    let root_id = root.get_id().unwrap();
    let matched_id = matched.get_id().unwrap();
    let other_id = other.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(root, now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(root_id);

    let mut show_all_output = TestWriter::new();
    execute(
        &mut show_all_output,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "全 BOUNDARY_MATCH",
    )
    .unwrap();
    let show_all_output = show_all_output.into_string();
    assert!(show_all_output.contains("BOUNDARY_MATCH"));
    assert!(!show_all_output.contains("BOUNDARY_OTHER"));

    focused_task_id_opt = Some(root_id);
    let mut tree_output = TestWriter::new();
    execute(
        &mut tree_output,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "樹",
    )
    .unwrap();
    let tree_output = tree_output.into_string();
    assert!(tree_output.contains("BOUNDARY_MATCH"), "{tree_output}");
    assert!(tree_output.contains("BOUNDARY_OTHER"), "{tree_output}");

    let mut list_output = TestWriter::new();
    execute(
        &mut list_output,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "今",
    )
    .unwrap();
    assert!(list_output.into_string().contains("BOUNDARY_MATCH"));

    let mut focus_output = TestWriter::new();
    execute(
        &mut focus_output,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        &format!("見 {matched_id}"),
    )
    .unwrap();
    assert_eq!(focused_task_id_opt, Some(matched_id));

    other.set_orig_status(Status::Pending).unwrap();
    let mut pick_output = TestWriter::new();
    execute(
        &mut pick_output,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        &format!("選 {other_id}"),
    )
    .unwrap();
    assert_eq!(focused_task_id_opt, Some(other_id));
    assert_eq!(other.get_orig_status().unwrap(), Status::Todo);
}

#[test]
fn calendarとbandは製品経路で代表出力とansi_capabilityを維持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let make_task = || {
        let task = new_test_task_handle("BOUNDARY_DAILY").unwrap();
        task.set_estimated_work_seconds(60 * 60);
        task.set_start_time(now);
        task.set_pending_until(now);
        task.set_orig_status(Status::Pending);
        task
    };

    let calendar = execute_calendar_command_for_test("暦", now, make_task(), 10 * 60);
    assert!(calendar.contains("2026-08-11(火)"));
    assert!(calendar.contains("日          \t空"));

    let band =
        execute_calendar_command_with_ansi_color_for_test("帯", now, make_task(), 10 * 60, true);
    assert!(band.contains("凡例:"));
    assert!(band.contains("\x1b[38;5;"));

    let pipe_band =
        execute_calendar_command_with_ansi_color_for_test("帯", now, make_task(), 10 * 60, false);
    assert!(pipe_band.contains("凡例:"));
    assert!(!pipe_band.contains("\x1b["));
}

#[test]
fn task_tree表示commandは製品経路で必ず1回flushする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("flush対象").unwrap();
    task.set_estimated_work_seconds(15 * 60);
    task.set_start_time(now);
    let task_id = task.get_id().unwrap();
    let commands = [
        "樹".to_string(),
        "条".to_string(),
        "根".to_string(),
        "葉".to_string(),
        "全".to_string(),
        "尾".to_string(),
        "今".to_string(),
        "単".to_string(),
        "暦".to_string(),
        "帯".to_string(),
        format!("見 {task_id}"),
        format!("選 {task_id}"),
        "親".to_string(),
        "子".to_string(),
        "深".to_string(),
        "上 next 15".to_string(),
    ];

    for command in commands {
        let mut task_repository = TestTaskRepository::new(task.clone(), now);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut focused_task_id_opt = Some(task_id);
        let mut stdout = FlushTrackingWriter::successful(true);

        execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            &command,
        )
        .unwrap();

        assert_eq!(stdout.flush_count, 1, "{command}");
    }
}

#[test]
fn task_tree表示commandはflush_errorとbroken_pipeを製品経路で分類する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("flush error対象").unwrap();
    let task_id = task.get_id().unwrap();

    let execute_with_error = |kind| {
        let mut task_repository = TestTaskRepository::new(task.clone(), now);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut focused_task_id_opt = Some(task_id);
        let mut stdout = FlushTrackingWriter::failing(kind);
        let result = execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            "樹",
        );
        (result, stdout.flush_count)
    };

    let (output_error, output_flush_count) = execute_with_error(std::io::ErrorKind::Other);
    assert!(matches!(
        output_error,
        Err(CommandError::Output(error)) if error.kind() == std::io::ErrorKind::Other
    ));
    assert_eq!(output_flush_count, 1);

    let (broken_pipe, broken_pipe_flush_count) = execute_with_error(std::io::ErrorKind::BrokenPipe);
    assert!(broken_pipe.is_ok());
    assert_eq!(broken_pipe_flush_count, 1);
}

#[test]
fn breakdownとsplitは製品経路で必ず1回flushする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in ["下 child", "割 15 child", "待"] {
        let task = new_test_task_handle("flush対象").unwrap();
        task.set_estimated_work_seconds(30 * 60);
        let task_id = task.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut focused_task_id_opt = Some(task_id);
        let mut stdout = FlushTrackingWriter::successful(true);

        execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            command,
        )
        .unwrap();

        assert_eq!(stdout.flush_count, 1, "{command}");
    }
}

#[test]
fn breakdownとsplitはflush_errorとbroken_pipeを製品経路で分類する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in ["下 child", "割 15 child", "待"] {
        let execute_with_error = |kind| {
            let task = new_test_task_handle("flush error対象").unwrap();
            task.set_estimated_work_seconds(30 * 60);
            let task_id = task.get_id().unwrap();
            let mut task_repository = TestTaskRepository::new(task, now);
            let mut free_time_manager = TestFreeTimeManager::default();
            let mut focused_task_id_opt = Some(task_id);
            let mut stdout = FlushTrackingWriter::failing(kind);
            let result = execute(
                &mut stdout,
                &mut task_repository,
                &mut free_time_manager,
                &mut focused_task_id_opt,
                &now,
                command,
            );
            (result, stdout.flush_count)
        };

        let (output_error, output_flush_count) = execute_with_error(std::io::ErrorKind::Other);
        assert!(matches!(
            output_error,
            Err(CommandError::Output(error)) if error.kind() == std::io::ErrorKind::Other
        ));
        assert_eq!(output_flush_count, 1, "{command}");

        let (broken_pipe, broken_pipe_flush_count) =
            execute_with_error(std::io::ErrorKind::BrokenPipe);
        assert!(broken_pipe.is_ok(), "{command}");
        assert_eq!(broken_pipe_flush_count, 1, "{command}");
    }
}

#[test]
fn task属性更新commandは製品経路で必ず1回flushする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    for command in [
        "〆 2026/08/20",
        "予 15",
        "揃 15",
        "実 20",
        "重 3",
        "類 資",
        "働 5",
    ] {
        let task = new_test_task_handle("flush対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut focused_task_id_opt = Some(task_id);
        let parsed = parse_command(command, ParseMode::NonInteractive).unwrap();
        let mut stdout = FlushTrackingWriter::successful(true);

        execute_parsed(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            &parsed,
        )
        .unwrap();

        assert_eq!(stdout.flush_count, 1, "{command}");
    }
}

#[test]
fn task属性更新commandはflush_errorとbroken_pipeを製品経路で分類する() {
    let execute_with_error = |error_kind| {
        let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
        let task = new_test_task_handle("flush error対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut focused_task_id_opt = Some(task_id);
        let parsed = parse_command("予 15", ParseMode::NonInteractive).unwrap();
        let mut stdout = FlushTrackingWriter::failing(error_kind);

        let result = execute_parsed(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            &parsed,
        );
        (result, stdout.flush_count)
    };

    let (output_error, output_flush_count) = execute_with_error(std::io::ErrorKind::Other);
    assert!(matches!(
        output_error,
        Err(CommandError::Output(error)) if error.kind() == std::io::ErrorKind::Other
    ));
    assert_eq!(output_flush_count, 1);

    let (broken_pipe, broken_pipe_flush_count) = execute_with_error(std::io::ErrorKind::BrokenPipe);
    assert!(broken_pipe.is_ok());
    assert_eq!(broken_pipe_flush_count, 1);
}

#[test]
fn defer系の通常interactive_commandはflushしshortcutはflushしない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in [
        "後 09:30",
        "後 abc 日 extra",
        "清",
        "逃",
        "押",
        "空 10:00",
        "集 10:00",
    ] {
        let task = new_test_task_handle("通常commandのflush対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut focused_task_id_opt = Some(task_id);
        let mut focus_selection_mode = FocusSelectionMode::Explicit;
        let mut stdout = FlushTrackingWriter::successful(true);

        execute_interactive_command(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            &mut focus_selection_mode,
            now,
            command,
        )
        .unwrap();

        assert_eq!(stdout.flush_count, 1, "{command}");
    }

    for command in ["t", "h", "D", "d", "w", "W", "y"] {
        let task = new_test_task_handle("shortcutのflush対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut focused_task_id_opt = Some(task_id);
        let mut focus_selection_mode = FocusSelectionMode::Explicit;
        let mut stdout = FlushTrackingWriter::successful(true);

        execute_interactive_command(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            &mut focus_selection_mode,
            now,
            command,
        )
        .unwrap();

        assert_eq!(stdout.flush_count, 0, "{command}");
    }
}

#[test]
fn interactive低優先度modeは共通outcome経路でfocusと表示を更新する() {
    let now = Local.with_ymd_and_hms(2026, 8, 18, 12, 0, 0).unwrap();
    let root = new_test_task_handle("root").unwrap();
    let high_priority_task = root.create_as_last_child(new_test_task_attr("高優先度候補"));
    let low_priority_task = root.create_as_last_child(new_test_task_attr("低優先度候補"));
    let high_priority_task_id = high_priority_task.get_id().unwrap();
    let low_priority_task_id = low_priority_task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(root, now);
    task_repository.highest_priority_leaf_task_id_opt = Some(high_priority_task_id);
    task_repository.defer_candidate_leaf_task_id_opt = Some(low_priority_task_id);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(high_priority_task_id);
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;
    let mut stdout = FlushTrackingWriter::successful(true);

    execute_interactive_command(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        &mut focus_selection_mode,
        now,
        "低 3",
    )
    .unwrap();

    assert_eq!(
        focus_selection_mode,
        FocusSelectionMode::LowestPriority { recent_days: 3 }
    );
    assert_eq!(focused_task_id_opt, Some(low_priority_task_id));
    assert_eq!(
        task_repository.last_defer_candidate_recent_threshold_opt,
        Some(Local.with_ymd_and_hms(2026, 8, 22, 6, 0, 0).unwrap())
    );
    assert_eq!(stdout.flush_count, 0);
    assert!(String::from_utf8(stdout.buffer)
        .unwrap()
        .contains("フォーカス選択モード: 低 3"));
}

#[test]
fn test_select_focus_task_id_高優先度modeでは最優先leafを返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    let expected_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    task_repository.highest_priority_leaf_task_id_opt = Some(expected_id);

    let actual = select_focus_task_id(&mut task_repository, FocusSelectionMode::HighestPriority);

    assert_eq!(actual, Ok(Some(expected_id)));
}

#[test]
fn test_select_focus_task_id_低優先度modeでは0日と10日の完成閾値をrepositoryへ渡す() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    for (recent_days, expected_threshold) in [
        (0, Local.with_ymd_and_hms(2026, 8, 12, 6, 0, 0).unwrap()),
        (10, Local.with_ymd_and_hms(2026, 8, 22, 6, 0, 0).unwrap()),
    ] {
        let task = new_test_task_handle("タスク").unwrap();
        let expected_id = Uuid::new_v4();
        let mut task_repository = TestTaskRepository::new(task, now);
        task_repository.defer_candidate_leaf_task_id_opt = Some(expected_id);

        let actual = select_focus_task_id(
            &mut task_repository,
            FocusSelectionMode::LowestPriority { recent_days },
        );

        assert_eq!(actual, Ok(Some(expected_id)));
        assert_eq!(
            task_repository.last_defer_candidate_recent_threshold_opt,
            Some(expected_threshold)
        );
    }
}

#[test]
fn test_select_focus_task_id_延期候補の日数範囲外を閾値errorにする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("延期候補の日数範囲外").unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);

    let actual = select_focus_task_id(
        &mut task_repository,
        FocusSelectionMode::LowestPriority {
            recent_days: i64::MAX,
        },
    );

    assert!(matches!(
        actual,
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "defer_candidate_threshold",
            datetime,
        }) if datetime == now
    ));
    assert_eq!(
        task_repository.last_defer_candidate_recent_threshold_opt,
        None
    );
}

#[test]
fn test_select_focus_task_id_延期候補の日時閾値errorを伝搬して状態を変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時閾値範囲外の延期候補").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let focused_task_id_opt = Some(task_id);
    let mut task_repository = TestTaskRepository::new(task, now);

    let actual = select_focus_task_id(
        &mut task_repository,
        FocusSelectionMode::LowestPriority { recent_days: 3 },
    );

    assert!(matches!(
        actual,
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "next_business_day_start",
            datetime,
        }) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert_eq!(
        task_repository.last_defer_candidate_recent_threshold_opt,
        None
    );
}

#[test]
fn test_execute_all_pendingタスクを予定時刻に含め_doneタスクを除外する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    let pending_task = new_test_task_handle("延期中タスク").unwrap();
    pending_task.set_start_time(now);
    pending_task.sync_clock(now);
    pending_task.set_pending_until(now + Duration::hours(2));
    pending_task.set_orig_status(Status::Pending);
    let pending_result = execute_command_for_test(
        pending_task.clone(),
        now,
        Some(pending_task.get_id().unwrap()),
        "全",
    );

    let done_task = new_test_task_handle("完了済みタスク").unwrap();
    done_task.set_start_time(now);
    done_task.sync_clock(now);
    done_task.set_orig_status(Status::Done);
    let done_result = execute_command_for_test(
        done_task.clone(),
        now,
        Some(done_task.get_id().unwrap()),
        "全",
    );

    assert!(pending_result.output.contains("延期中タスク"));
    assert!(pending_result.output.contains("14:00~14:15"));
    assert!(!done_result.output.contains("完了済みタスク"));
}

#[test]
fn test_execute_all_project_categoryで絞り込む() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("カテゴリ対象タスク").unwrap();
    task.sync_clock(now);
    task.set_project_category_opt(Some(ProjectCategory::Investment));

    let matched =
        execute_command_for_test(task.clone(), now, Some(task.get_id().unwrap()), "全 資");
    let unmatched =
        execute_command_for_test(task.clone(), now, Some(task.get_id().unwrap()), "全 獲");

    assert!(matched.output.contains("カテゴリ対象タスク"));
    assert!(!unmatched.output.contains("カテゴリ対象タスク"));
}

#[test]
fn test_execute_allはspreadsheet_a_j列を製品formatterで出力する() {
    assert_show_all_spreadsheet_formatter_contract();
}

#[test]
fn show_allの製品経路はspreadsheet_formatterを使う() {
    assert_show_all_spreadsheet_formatter_contract();
}

#[test]
fn test_execute_all_締切順の予定時刻を表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let root_task = new_test_task_handle("親タスク").unwrap();
    root_task.sync_clock(now);
    root_task.set_estimated_work_seconds(0);

    let mut late_deadline_attr = new_test_task_attr("締切が遅いタスク");
    late_deadline_attr.set_estimated_work_seconds(30 * 60);
    late_deadline_attr.set_start_time(now);
    late_deadline_attr.set_deadline_time_opt(Some(now + Duration::hours(3)));
    let late_deadline_task = root_task.create_as_last_child(late_deadline_attr);
    late_deadline_task.sync_clock(now);

    let mut early_deadline_attr = new_test_task_attr("締切が早いタスク");
    early_deadline_attr.set_estimated_work_seconds(15 * 60);
    early_deadline_attr.set_start_time(now);
    early_deadline_attr.set_deadline_time_opt(Some(now + Duration::hours(2)));
    let early_deadline_task = root_task.create_as_last_child(early_deadline_attr);
    early_deadline_task.sync_clock(now);

    let result = execute_command_for_test(
        root_task.clone(),
        now,
        Some(root_task.get_id().unwrap()),
        "全",
    );
    let early_deadline_line = result
        .output
        .lines()
        .find(|line| line.contains("締切が早いタスク"))
        .expect("early-deadline task line");
    let late_deadline_line = result
        .output
        .lines()
        .find(|line| line.contains("締切が遅いタスク"))
        .expect("late-deadline task line");

    assert!(
        early_deadline_line.contains("12:00~12:15"),
        "unexpected schedule output: {}",
        result.output
    );
    assert!(
        late_deadline_line.contains("12:15~12:45"),
        "unexpected schedule output: {}",
        result.output
    );
}

#[test]
fn test_execute_new_新規projectを翌朝までpendingで作成する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let original_task = new_test_task_handle("既存タスク").unwrap();
    let result = execute_command_for_test(
        original_task.clone(),
        now,
        Some(original_task.get_id().unwrap()),
        "新 新規project 30",
    );

    assert_eq!(result.task.get_name().unwrap(), "新規project");
    assert_eq!(result.task.get_priority().unwrap(), 5);
    assert_eq!(result.task.get_estimated_work_seconds().unwrap(), 30 * 60);
    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        try_next_business_day_start(now).unwrap()
    );
    assert_eq!(
        result.focused_task_id_opt,
        Some(result.task.get_id().unwrap())
    );
}

#[test]
fn test_execute_unplanned_延期と見積もりを省略して即時着手可能で作成する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let original_task = new_test_task_handle("既存タスク").unwrap();
    let result = execute_command_for_test(
        original_task.clone(),
        now,
        Some(original_task.get_id().unwrap()),
        "突 割り込みproject",
    );

    assert_eq!(result.task.get_name().unwrap(), "割り込みproject");
    assert_eq!(result.task.get_orig_status().unwrap(), Status::Todo);
    assert_eq!(result.task.get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(
        result.focused_task_id_opt,
        Some(result.task.get_id().unwrap())
    );
}

#[test]
fn test_project作成commandの製品handler経路がtyped_fieldと表示とfocusを反映する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    let new_root = new_test_task_handle("既存").unwrap();
    let new_result = execute_command_for_test(
        new_root.clone(),
        now,
        Some(new_root.get_id().unwrap()),
        "新 新規 25",
    );
    assert_eq!(new_result.task.get_name().unwrap(), "新規");
    assert_eq!(
        new_result.task.get_estimated_work_seconds().unwrap(),
        25 * 60
    );
    assert_eq!(
        new_result.focused_task_id_opt,
        Some(new_result.task.get_id().unwrap())
    );

    let hobby_root = new_test_task_handle("既存").unwrap();
    let hobby_result = execute_command_for_test(
        hobby_root.clone(),
        now,
        Some(hobby_root.get_id().unwrap()),
        "遊 趣味 20",
    );
    assert_eq!(hobby_result.task.get_name().unwrap(), "趣味");
    assert_eq!(
        hobby_result.task.get_pending_until().unwrap(),
        try_next_business_day_start(now).unwrap() + Duration::days(1399)
    );

    let unplanned_root = new_test_task_handle("既存").unwrap();
    let unplanned_result = execute_command_for_test(
        unplanned_root.clone(),
        now,
        Some(unplanned_root.get_id().unwrap()),
        "突 割り込み 10",
    );
    assert_eq!(unplanned_result.task.get_name().unwrap(), "割り込み");
    assert_eq!(
        unplanned_result.task.get_orig_status().unwrap(),
        Status::Todo
    );

    let sequential_root = new_test_task_handle("親").unwrap();
    let sequential_result = execute_command_for_test(
        sequential_root.clone(),
        now,
        Some(sequential_root.get_id().unwrap()),
        "連 手順 15 2 3 章",
    );
    let sequential_children = sequential_result.task.get_children().unwrap();
    assert_eq!(sequential_children[0].get_name().unwrap(), "手順 3-章");
    assert_eq!(
        sequential_result.focused_task_id_opt,
        Some(
            sequential_children[0].get_children().unwrap()[0]
                .get_id()
                .unwrap()
        )
    );

    let repeat_root = new_test_task_handle("親").unwrap();
    let repeat_result = execute_command_for_test(
        repeat_root.clone(),
        now,
        Some(repeat_root.get_id().unwrap()),
        "繰 習慣 10 毎 09:00 10:00",
    );
    assert_eq!(repeat_result.task.get_children().unwrap().len(), 1);
    assert!(repeat_result.output.contains("習慣"));

    let appointment_task = new_test_task_handle("予定").unwrap();
    let appointment_id = appointment_task.get_id().unwrap();
    let appointment_result =
        execute_command_for_test(appointment_task, now, Some(appointment_id), "約 14:30 8/12");
    assert_eq!(
        appointment_result.task.get_start_time().unwrap(),
        Local.with_ymd_and_hms(2026, 8, 12, 14, 30, 0).unwrap()
    );
    assert_eq!(appointment_result.focused_task_id_opt, Some(appointment_id));

    let start_task = new_test_task_handle("開始").unwrap();
    let start_id = start_task.get_id().unwrap();
    let start_result = execute_command_for_test(start_task, now, Some(start_id), "始 16:45 8/13");
    assert_eq!(
        start_result.task.get_start_time().unwrap(),
        Local.with_ymd_and_hms(2026, 8, 13, 16, 45, 0).unwrap()
    );

    for invalid_command in ["新 123 10", "連 手順 -1 1 2", "繰 習慣 -1 毎 09:00 10:00"] {
        let task = new_test_task_handle("変更なし").unwrap();
        let result = execute_command_for_test(
            task.clone(),
            now,
            Some(task.get_id().unwrap()),
            invalid_command,
        );
        assert_eq!(result.task.get_name().unwrap(), "変更なし");
        assert!(result.task.get_children().unwrap().is_empty());
    }
}

#[test]
fn test_execute_breakdown_子を順に作り締切を継承して最初の子へfocusする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let deadline = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.sync_clock(now);
    parent_task.set_deadline_time_opt(Some(deadline));

    let result = execute_command_for_test(
        parent_task.clone(),
        now,
        Some(parent_task.get_id().unwrap()),
        "下 子A 子B",
    );
    let children = result.task.get_children().unwrap();

    assert_eq!(
        children
            .iter()
            .map(|task| task.get_name().unwrap())
            .collect::<Vec<_>>(),
        vec!["子A", "子B"]
    );
    assert!(children
        .iter()
        .all(|task| task.get_deadline_time_opt().unwrap() == Some(deadline)));
    assert_eq!(
        result.focused_task_id_opt,
        Some(children[0].get_id().unwrap())
    );
    assert!(result.output.contains("子A"));
    assert!(result.output.contains("子B"));
}

#[test]
fn test_execute_breakdown_数値を含む引数では子を作らない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    let parent_task_id = parent_task.get_id().unwrap();

    let result = execute_command_for_test(parent_task, now, Some(parent_task_id), "下 子タスク 15");

    assert!(result.task.get_children().unwrap().is_empty());
    assert_eq!(result.focused_task_id_opt, Some(parent_task_id));
}

#[test]
fn test_execute_breakdown_親に締切がなければ子も締切なしで作る() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();

    let result = execute_command_for_test(
        parent_task.clone(),
        now,
        Some(parent_task.get_id().unwrap()),
        "下 子タスク",
    );
    let children = result.task.get_children().unwrap();

    assert_eq!(children.len(), 1);
    assert_eq!(children[0].get_deadline_time_opt().unwrap(), None);
}

#[test]
fn test_execute_wait_相手待ちにしてfocusを維持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("待機対象").unwrap();
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "待");

    assert!(result.task.get_is_on_other_side().unwrap());
    assert_eq!(result.focused_task_id_opt, Some(task_id));
}

#[test]
fn test_execute_next_up_数値名と負の見積もりでは変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in [
        "上 123 10",
        "上 新しい親 -1",
        "上 新しい親 abc",
        "上 新しい親 9223372036854775808",
    ] {
        let root = new_test_task_handle("root").unwrap();
        let focused = root.create_as_last_child(new_test_task_attr("focus"));
        let result = execute_command_for_test(root, now, Some(focused.get_id().unwrap()), command);

        assert_eq!(result.task.get_children().unwrap().len(), 1);
        assert_eq!(
            result.task.get_children().unwrap()[0].get_name().unwrap(),
            "focus"
        );
        assert_eq!(result.focused_task_id_opt, Some(focused.get_id().unwrap()));
    }
}

#[test]
fn test_execute_next_up_入力不正とfocusなしではidentityを消費しない() {
    let assert_identity_not_consumed =
        |focused_task_opt: Option<TaskHandle>,
         name: &str,
         estimated_minutes: Option<i64>,
         expected: Result<Option<Uuid>, ApplicationError>| {
            let operation_now = Local.with_ymd_and_hms(2026, 8, 19, 14, 30, 0).unwrap();
            let id_generator_call_count = Cell::new(0);
            let mut next_id = || {
                id_generator_call_count.set(id_generator_call_count.get() + 1);
                Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap()
            };
            let mut factory = TaskFactory::new(operation_now, &mut next_id);
            let mut focused_task_id_opt =
                focused_task_opt.as_ref().map(|task| task.get_id().unwrap());
            let mut stdout = TestWriter::new();

            let actual = execute_next_up(
                &mut stdout,
                &mut focused_task_id_opt,
                &focused_task_opt,
                name,
                &estimated_minutes,
                &mut factory,
            );

            assert_eq!(actual, expected);
            assert_eq!(id_generator_call_count.get(), 0);
        };

    let root = new_test_task_handle("root").unwrap();
    let focused = root.create_as_last_child(new_test_task_attr("focused"));
    assert_identity_not_consumed(
        Some(focused.clone()),
        "123",
        Some(10),
        Err(ApplicationError::InvalidInput {
            field: "name",
            reason: "must not be an integer-only name",
        }),
    );
    assert_identity_not_consumed(
        Some(focused),
        "new parent",
        Some(-1),
        Err(ApplicationError::InvalidInput {
            field: "estimated_work_minutes",
            reason: "must not be negative",
        }),
    );
    assert_identity_not_consumed(None, "new parent", Some(10), Ok(None));
}

#[test]
fn test_execute_next_up_rootへの親追加失敗を構造化errorで返す() {
    let operation_now = Local.with_ymd_and_hms(2026, 8, 19, 14, 30, 0).unwrap();
    let id_generator_call_count = Cell::new(0);
    let mut next_id = || {
        id_generator_call_count.set(id_generator_call_count.get() + 1);
        Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap()
    };
    let mut factory = TaskFactory::new(operation_now, &mut next_id);
    let root = new_test_task_handle("root").unwrap();
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(root.get_id().unwrap());
    let before_estimated_work_seconds = root.get_estimated_work_seconds().unwrap();

    let actual = execute_next_up(
        &mut stdout,
        &mut focused_task_id_opt,
        &Some(root.clone()),
        "new parent",
        &Some(10),
        &mut factory,
    );

    assert_eq!(
        actual,
        Err(ApplicationError::TaskTree(TaskTreeError::RootOperation))
    );
    assert_eq!(
        root.get_estimated_work_seconds().unwrap(),
        before_estimated_work_seconds
    );
    assert_eq!(focused_task_id_opt, Some(root.get_id().unwrap()));
    assert_eq!(id_generator_call_count.get(), 0);
}

#[test]
fn test_execute_next_up_task生成contextと既存の親挿入契約を固定する() {
    let operation_now = Local.with_ymd_and_hms(2026, 8, 19, 14, 30, 0).unwrap();
    let deadline = operation_now + Duration::days(2);
    let expected_new_parent_id = Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap();
    let mut next_id = || expected_new_parent_id;
    let mut factory = TaskFactory::new(operation_now, &mut next_id);

    let root = new_test_task_handle("root").unwrap();
    root.set_deadline_time_opt(Some(deadline)).unwrap();
    root.set_estimated_work_seconds(120 * 60).unwrap();
    let focused = root.create_as_last_child(new_test_task_attr("focused"));
    let focused_id = focused.get_id().unwrap();
    let mut focused_task_id_opt = Some(focused_id);
    let mut stdout = TestWriter::new();

    let actual = execute_next_up(
        &mut stdout,
        &mut focused_task_id_opt,
        &Some(focused),
        "new parent",
        &Some(15),
        &mut factory,
    );

    assert_eq!(actual, Ok(Some(expected_new_parent_id)));
    assert_eq!(focused_task_id_opt, Some(expected_new_parent_id));
    assert_eq!(root.get_estimated_work_seconds().unwrap(), 105 * 60);

    let root_children = root.get_children().unwrap();
    assert_eq!(root_children.len(), 1);
    let new_parent = &root_children[0];
    assert_eq!(new_parent.get_id().unwrap(), expected_new_parent_id);
    assert_eq!(new_parent.get_name().unwrap(), "new parent");
    assert_eq!(new_parent.get_start_time().unwrap(), operation_now);
    assert_eq!(new_parent.get_deadline_time_opt().unwrap(), Some(deadline));
    assert_eq!(new_parent.get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(
        new_parent.get_children().unwrap()[0].get_id().unwrap(),
        focused_id
    );
}

#[test]
fn test_execute_sequential_数値名と負の見積もりでは変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in ["連 123 10 1 2", "連 子 -1 1 2"] {
        let root = new_test_task_handle("root").unwrap();
        let result =
            execute_command_for_test(root.clone(), now, Some(root.get_id().unwrap()), command);

        assert!(result
            .task
            .get_children()
            .expect("command result tree must be readable")
            .is_empty());
        assert_eq!(result.focused_task_id_opt, Some(root.get_id().unwrap()));
    }
}

#[test]
fn test_execute_split_負数は親に残す時間として扱う() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let root = new_test_task_handle("root").unwrap();
    root.set_estimated_work_seconds(100 * 60);

    let result =
        execute_command_for_test(root.clone(), now, Some(root.get_id().unwrap()), "割 -15 子");
    let children = result
        .task
        .get_children()
        .expect("split result tree must be readable");
    let child = &children[0];

    assert_eq!(result.task.get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(child.get_name().unwrap(), "子");
    assert_eq!(child.get_estimated_work_seconds().unwrap(), 85 * 60);
    assert_eq!(result.focused_task_id_opt, Some(child.get_id().unwrap()));
}

#[test]
fn test_execute_split_数値名とoverflowでは変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for command in [
        "割 -15 123",
        "割 -9223372036854775808 子",
        "割 9223372036854775807 子",
    ] {
        let root = new_test_task_handle("root").unwrap();
        root.set_estimated_work_seconds(100 * 60);
        let result =
            execute_command_for_test(root.clone(), now, Some(root.get_id().unwrap()), command);

        assert_eq!(result.task.get_estimated_work_seconds().unwrap(), 100 * 60);
        assert!(result
            .task
            .get_children()
            .expect("command result tree must be readable")
            .is_empty());
        assert_eq!(result.focused_task_id_opt, Some(root.get_id().unwrap()));
    }
}

#[test]
fn test_execute_defer_指定時間までpendingにしてfocusを外す() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("延期対象").unwrap();

    let result =
        execute_command_for_test(task.clone(), now, Some(task.get_id().unwrap()), "後 5 分");

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        now + Duration::minutes(5)
    );
    assert_eq!(result.focused_task_id_opt, None);
}

#[test]
fn test_execute_defer_日付指定はその日の朝までpendingにする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("延期対象").unwrap();

    let result = execute_command_for_test(
        task.clone(),
        now,
        Some(task.get_id().unwrap()),
        "後 2026/08/13",
    );

    assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        result.task.get_pending_until().unwrap(),
        Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 1).unwrap()
    );
    assert_eq!(result.focused_task_id_opt, None);
}

#[test]
fn test_execute_defer_expression_曜日指定は次の該当曜日までpendingにする() {
    let now = Local.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();

    for (weekday, expected) in [
        ("月", Local.with_ymd_and_hms(2026, 8, 24, 6, 0, 1).unwrap()),
        ("火", Local.with_ymd_and_hms(2026, 8, 18, 6, 0, 1).unwrap()),
    ] {
        let task = new_test_task_handle("曜日延期対象").unwrap();
        let task_id = task.get_id().unwrap();

        let result = execute_command_for_test(task, now, Some(task_id), &format!("後 {weekday}"));

        assert_eq!(result.task.get_orig_status().unwrap(), Status::Pending);
        assert_eq!(result.task.get_pending_until().unwrap(), expected);
        assert_eq!(result.focused_task_id_opt, None);
    }
}

#[test]
fn test_execute_defer_余剰引数でも単位正規化と入力error表示を維持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("余剰引数の延期対象").unwrap();
    let task_id = task.get_id().unwrap();

    let valid = execute_command_for_test(task.clone(), now, Some(task_id), "後 2 DAYS extra");
    assert_eq!(valid.task.get_orig_status().unwrap(), Status::Pending);
    assert_eq!(
        valid.task.get_pending_until().unwrap(),
        Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap()
    );
    assert_eq!(valid.focused_task_id_opt, None);

    task.set_orig_status(Status::Todo).unwrap();
    let invalid = execute_command_for_test(task.clone(), now, Some(task_id), "後 abc 日 extra");
    assert_eq!(invalid.task.get_orig_status().unwrap(), Status::Todo);
    assert_eq!(invalid.focused_task_id_opt, Some(task_id));
    assert!(invalid.output.contains(
        "[Error] 入力エラー: amount: 整数で指定してください (コマンド: 後, 使い方: 後 <数値> <単位>)"
    ));
}

#[test]
fn test_execute_defer_翌朝計算不能を情報付きerrorにして状態を変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時範囲外の延期対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut focused_task_id_opt = Some(task_id);

    let actual = execute_defer(&mut task_repository, &mut focused_task_id_opt, 1, "日");

    assert_eq!(
        actual,
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "next_business_day_start",
            datetime: now,
        })
    );
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
}

#[test]
fn test_execute_defer_巨大な日数を即座に情報付きerrorにして状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 21, 12, 0, 0).unwrap();
    assert_eq!(defer_business_day_target(now, 0), Ok(now));
    assert_eq!(defer_business_day_target(now, -1), Ok(now));
    assert_eq!(
        defer_business_day_target(now, i64::MAX),
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "defer_business_days",
            datetime: now,
        })
    );
    let task = new_test_task_handle("巨大日数の延期対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut focused_task_id_opt = Some(task_id);

    let actual = execute_defer(
        &mut task_repository,
        &mut focused_task_id_opt,
        i64::MAX,
        "日",
    );

    assert_eq!(
        actual,
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "defer_business_days",
            datetime: now,
        })
    );
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
}

#[test]
fn test_runtime_defer_shortcut_翌朝計算不能を情報付きerrorにして状態を変更しない() {
    for shortcut in ["next_morning", "next_week", "five_years"] {
        let now = maximum_local_datetime();
        let task = new_test_task_handle("日時範囲外の延期shortcut対象").unwrap();
        let task_id = task.get_id().unwrap();
        let original_snapshot = task.snapshot().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut focused_task_id_opt = Some(task_id);
        let mut context = RuntimeDeferCommandContext {
            task_repository: &mut task_repository,
            focused_task_id_opt: &mut focused_task_id_opt,
            config: active_config(),
        };

        let actual = match shortcut {
            "next_morning" => context.defer_next_morning(),
            "next_week" => context.defer_next_week(),
            "five_years" => context.defer_five_years(),
            _ => unreachable!("test shortcut table must contain supported values"),
        };

        assert!(matches!(
            actual,
            Err(DeferCommandError::Application(
                ApplicationError::SubjectiveDateOutOfRange {
                    operation: "next_business_day_start",
                    datetime,
                }
            )) if datetime == now
        ));
        assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
        assert_eq!(focused_task_id_opt, Some(task_id));
    }
}

#[test]
fn test_execute_defer_expression_曜日の業務日計算不能を情報付きerrorにして状態を変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時範囲外の曜日延期対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut focused_task_id_opt = Some(task_id);
    let mut context = RuntimeDeferCommandContext {
        task_repository: &mut task_repository,
        focused_task_id_opt: &mut focused_task_id_opt,
        config: active_config(),
    };

    let actual = context.defer_expression(&["月".to_string()]);

    assert!(matches!(
        actual,
        Err(DeferCommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "next_business_day_start",
                datetime,
            }
        )) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
}

#[test]
fn test_execute_defer_expression_mmddの日時errorを伝搬して状態を変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時範囲外のMMDD延期対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut focused_task_id_opt = Some(task_id);
    let stdout = TestWriter::new();
    let mut context = RuntimeDeferCommandContext {
        task_repository: &mut task_repository,
        focused_task_id_opt: &mut focused_task_id_opt,
        config: active_config(),
    };

    let actual = context.defer_expression(&["12/31".to_string()]);

    assert!(matches!(
        actual,
        Err(DeferCommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "upcoming_calendar_date",
                datetime,
            }
        )) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(stdout.into_string().is_empty());
}

#[test]
fn test_execute_defer_expression_不正なcalendar時刻を変更せず拒否する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let task = new_test_task_handle("不正calendar時刻の延期対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut focused_task_id_opt = Some(task_id);
    let stdout = TestWriter::new();
    let mut context = RuntimeDeferCommandContext {
        task_repository: &mut task_repository,
        focused_task_id_opt: &mut focused_task_id_opt,
        config: active_config(),
    };

    let actual = context.defer_expression(&["13:99".to_string()]);

    assert!(actual.is_ok());
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(stdout.into_string().is_empty());
}

#[test]
fn test_execute_defer_expression_同日と24時超過を現在calendar日基準で解釈する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    for (value, expected) in [
        (
            "13:30",
            Local.with_ymd_and_hms(2026, 8, 14, 13, 30, 1).unwrap(),
        ),
        (
            "25:30",
            Local.with_ymd_and_hms(2026, 8, 15, 1, 30, 1).unwrap(),
        ),
    ] {
        let task = new_test_task_handle("時刻指定の延期対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut focused_task_id_opt = Some(task_id);
        let mut context = RuntimeDeferCommandContext {
            task_repository: &mut task_repository,
            focused_task_id_opt: &mut focused_task_id_opt,
            config: active_config(),
        };

        let actual = context.defer_expression(&[value.to_string()]);

        assert!(actual.is_ok());
        assert_eq!(task_repository.task.get_pending_until().unwrap(), expected);
        assert_eq!(focused_task_id_opt, None);
    }
}

#[test]
fn test_execute_defer_routine_翌朝計算不能を情報付きerrorにして親子とfocusを変更しない() {
    let orig_deadline = maximum_local_datetime();
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let parent = new_test_task_handle("反復routine親").unwrap();
    parent.set_repetition_interval_days_opt(Some(7)).unwrap();
    parent
        .set_deadline_time_opt(Some(Local.with_ymd_and_hms(2026, 8, 20, 18, 0, 0).unwrap()))
        .unwrap();
    let mut child_attr = new_test_task_attr("延期対象routine子");
    child_attr.set_deadline_time_opt(Some(orig_deadline));
    let child = parent.create_as_last_child(child_attr);
    let child_id = child.get_id().unwrap();
    let parent_snapshot = parent.snapshot().unwrap();
    let child_snapshot = child.snapshot().unwrap();
    let child_ids = parent
        .get_children()
        .unwrap()
        .into_iter()
        .map(|task| task.get_id().unwrap())
        .collect::<Vec<_>>();
    let mut task_repository = TestTaskRepository::new(parent.clone(), now);
    let mut focused_task_id_opt = Some(child_id);
    let mut context = RuntimeDeferCommandContext {
        task_repository: &mut task_repository,
        focused_task_id_opt: &mut focused_task_id_opt,
        config: active_config(),
    };

    let actual = context.defer_routine();

    assert_eq!(
        actual,
        Err(ApplicationError::SubjectiveDateOutOfRange {
            operation: "next_business_day_start",
            datetime: orig_deadline,
        })
    );
    assert_eq!(parent.snapshot().unwrap(), parent_snapshot);
    assert_eq!(child.snapshot().unwrap(), child_snapshot);
    assert_eq!(
        parent
            .get_children()
            .unwrap()
            .into_iter()
            .map(|task| task.get_id().unwrap())
            .collect::<Vec<_>>(),
        child_ids
    );
    assert_eq!(focused_task_id_opt, Some(child_id));
}

#[test]
fn test_execute_defer_routine_親の反復間隔と任意deadline時刻で延期する() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();
    let orig_deadline = Local.with_ymd_and_hms(2026, 8, 13, 10, 0, 0).unwrap();
    let orig_start = Local.with_ymd_and_hms(2026, 8, 10, 9, 0, 0).unwrap();
    let expected_start = Local.with_ymd_and_hms(2026, 8, 17, 9, 0, 0).unwrap();

    for (parent_deadline, expected_deadline) in [
        (
            Some(Local.with_ymd_and_hms(2026, 8, 20, 18, 0, 0).unwrap()),
            Local.with_ymd_and_hms(2026, 8, 20, 18, 0, 0).unwrap(),
        ),
        (None, Local.with_ymd_and_hms(2026, 8, 20, 10, 0, 0).unwrap()),
    ] {
        let parent = new_test_task_handle("正常反復routine親").unwrap();
        parent.set_repetition_interval_days_opt(Some(7)).unwrap();
        parent.set_deadline_time_opt(parent_deadline).unwrap();
        let mut child_attr = new_test_task_attr("正常延期routine子");
        child_attr.set_deadline_time_opt(Some(orig_deadline));
        child_attr.set_start_time(orig_start);
        child_attr.set_orig_status(Status::Pending);
        let child = parent.create_as_last_child(child_attr);
        let child_id = child.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(parent, now);
        let mut focused_task_id_opt = Some(child_id);
        let mut context = RuntimeDeferCommandContext {
            task_repository: &mut task_repository,
            focused_task_id_opt: &mut focused_task_id_opt,
            config: active_config(),
        };

        let actual = context.defer_routine();

        assert_eq!(actual, Ok(()));
        assert_eq!(
            child.get_deadline_time_opt().unwrap(),
            Some(expected_deadline)
        );
        assert_eq!(child.get_start_time().unwrap(), expected_start);
        assert_eq!(child.get_orig_status().unwrap(), Status::Todo);
        assert_eq!(focused_task_id_opt, None);
    }
}

#[test]
fn test_execute_deadline_翌朝計算不能を情報付きerrorにして状態を変更しない() {
    let now = maximum_local_datetime();
    let task = new_test_task_handle("日時範囲外のdeadline対象").unwrap();
    let task_id = task.get_id().unwrap();
    let original_snapshot = task.snapshot().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    let actual = execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        "〆 明",
    );

    assert!(matches!(
        actual,
        Err(CommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "next_business_day_start",
                datetime,
            }
        )) if datetime == now
    ));
    assert_eq!(task_repository.task.snapshot().unwrap(), original_snapshot);
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert!(stdout.into_string().is_empty());
}

#[test]
fn test_resolve_deadline_date_今は最大日時でも同じcalendar日を返す() {
    let now = maximum_local_datetime();
    let expected = now.format("%Y/%m/%d").to_string();

    assert!(matches!(
        resolve_deadline_date("今", now),
        Ok(actual) if actual == expected
    ));
}

#[test]
fn test_resolve_deadline_date_曜日の範囲外を曜日計算errorにする() {
    let now = maximum_local_datetime();

    assert!(matches!(
        resolve_deadline_date("月", now),
        Err(CommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "deadline_weekday_date",
                datetime,
            }
        )) if datetime == now
    ));
}

#[test]
fn test_resolve_deadline_date_曜日は同じ曜日を7日後にする() {
    let now = Local.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();

    assert!(matches!(
        resolve_deadline_date("月", now),
        Ok(actual) if actual == "2026/08/24"
    ));
    assert!(matches!(
        resolve_deadline_date("火", now),
        Ok(actual) if actual == "2026/08/18"
    ));
}

#[test]
fn test_resolve_deadline_date_mmddは同日正午を過ぎると翌年を選ぶ() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 13, 0, 0).unwrap();

    assert!(matches!(
        resolve_deadline_date("8/14", now),
        Ok(actual) if actual == "2027/08/14"
    ));
}

#[test]
fn test_resolve_deadline_date_mmddは同日正午直前なら現在年を選ぶ() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 11, 59, 59).unwrap();

    assert!(matches!(
        resolve_deadline_date("8/14", now),
        Ok(actual) if actual == "2026/08/14"
    ));
}

#[test]
fn test_resolve_deadline_date_mmddは同日正午ちょうどなら現在年を選ぶ() {
    let now = Local.with_ymd_and_hms(2026, 8, 14, 12, 0, 0).unwrap();

    assert!(matches!(
        resolve_deadline_date("8/14", now),
        Ok(actual) if actual == "2026/08/14"
    ));
}

#[test]
fn test_resolve_deadline_date_mmddの翌年範囲外を情報付きerrorにする() {
    let now = maximum_local_datetime()
        .checked_add_signed(Duration::hours(1))
        .unwrap();

    assert!(matches!(
        resolve_deadline_date("12/31", now),
        Err(CommandError::Application(
            ApplicationError::SubjectiveDateOutOfRange {
                operation: "deadline_calendar_date",
                datetime,
            }
        )) if datetime == now
    ));
}

#[test]
fn test_execute_finish_未完了の子があれば完了しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.create_as_last_child(new_test_task_attr("未完了の子"));

    let result = execute_command_for_test(
        parent_task.clone(),
        now,
        Some(parent_task.get_id().unwrap()),
        "終 今",
    );

    assert_ne!(result.task.get_status().unwrap(), Status::Done);
    assert_eq!(result.task.get_end_time_opt().unwrap(), None);
    assert_eq!(
        result.focused_task_id_opt,
        Some(parent_task.get_id().unwrap())
    );
    assert!(result.output.contains("未完了の子"));
}

#[test]
fn test_execute_finish_未完了の子があれば不正引数でもtreeを表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.create_as_last_child(new_test_task_attr("未完了の子"));

    let result = execute_command_for_test(
        parent_task.clone(),
        now,
        Some(parent_task.get_id().unwrap()),
        "終 invalid",
    );

    assert_ne!(result.task.get_status().unwrap(), Status::Done);
    assert_eq!(result.task.get_end_time_opt().unwrap(), None);
    assert_eq!(
        result.focused_task_id_opt,
        Some(parent_task.get_id().unwrap())
    );
    assert!(result.output.contains("未完了の子"));
}

#[test]
fn test_execute_finish_唯一の子を完了すると親へfocusする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    let child_task = parent_task.create_as_last_child(new_test_task_attr("子タスク"));

    let result = execute_command_for_test(
        parent_task.clone(),
        now,
        Some(child_task.get_id().unwrap()),
        "終 今",
    );
    let finished_child = result
        .task
        .get_by_id(child_task.get_id().unwrap())
        .unwrap()
        .expect("finished child must remain in the fixture tree");

    assert_eq!(finished_child.get_status().unwrap(), Status::Done);
    assert_eq!(finished_child.get_end_time_opt().unwrap(), Some(now));
    assert_eq!(
        result.focused_task_id_opt,
        Some(parent_task.get_id().unwrap())
    );
}

#[test]
fn test_execute_finish_唯一の反復子から次回taskを生成したらfocusを解除する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let parent_task = new_test_task_handle("繰り返しtask").unwrap();
    parent_task
        .set_repetition_interval_days_opt(Some(7))
        .unwrap();
    let child_task = parent_task.create_as_last_child(new_test_task_attr("今回分"));
    let child_id = child_task.get_id().unwrap();

    let result = execute_command_for_test(parent_task, now, Some(child_id), "終 今");

    assert_eq!(result.focused_task_id_opt, None);
    assert_eq!(child_task.get_status().unwrap(), Status::Done);
    let children = result.task.get_children().unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(
        children
            .into_iter()
            .filter(|task| task.get_status().unwrap() != Status::Done)
            .count(),
        1
    );
}

#[test]
fn test_execute_finish_繰り返しtaskの見積もりを実績との差に応じて補正する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let cases = [(1_000, 900), (200, 500), (600, 600)];

    for (actual_work_seconds, expected_estimated_work_seconds) in cases {
        let parent_task = new_test_task_handle("繰り返しtask").unwrap();
        parent_task.set_repetition_interval_days_opt(Some(7));
        parent_task.set_estimated_work_seconds(600);
        let mut child_attr = new_test_task_attr("今回分");
        child_attr.set_actual_work_seconds(actual_work_seconds);
        let child_task = parent_task.create_as_last_child(child_attr);

        let result = execute_command_for_test(
            parent_task,
            now,
            Some(child_task.get_id().unwrap()),
            "終 今",
        );

        assert_eq!(
            result.task.get_estimated_work_seconds().unwrap(),
            expected_estimated_work_seconds
        );
    }
}

#[test]
fn test_execute_repetition_数値だけの名前は拒否して元taskを変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("既存タスク").unwrap();
    task.set_estimated_work_seconds(45 * 60);

    let result = execute_command_for_test(
        task.clone(),
        now,
        Some(task.get_id().unwrap()),
        "繰 123 10 毎 09:00 10:00",
    );

    assert_eq!(result.task.get_estimated_work_seconds().unwrap(), 45 * 60);
    assert!(result
        .task
        .get_children()
        .expect("command result tree must be readable")
        .is_empty());
    assert_eq!(result.focused_task_id_opt, Some(task.get_id().unwrap()));
}

#[test]
fn test_execute_new_数値だけの名前は拒否して元taskを変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("既存タスク").unwrap();

    let result =
        execute_command_for_test(task.clone(), now, Some(task.get_id().unwrap()), "新 123 10");

    assert_eq!(result.task.get_id().unwrap(), task.get_id().unwrap());
    assert_eq!(result.task.get_name().unwrap(), "既存タスク");
    assert_eq!(result.focused_task_id_opt, Some(task.get_id().unwrap()));
}

#[test]
fn test_execute_repetition_不正な見積もりでは元taskを変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for estimated_work_minutes in ["-1", "9223372036854775807"] {
        let task = new_test_task_handle("既存タスク").unwrap();
        task.set_estimated_work_seconds(45 * 60);
        let command = format!("繰 反復 {estimated_work_minutes} 毎 09:00 10:00");

        let result =
            execute_command_for_test(task.clone(), now, Some(task.get_id().unwrap()), &command);

        assert_eq!(result.task.get_estimated_work_seconds().unwrap(), 45 * 60);
        assert!(result
            .task
            .get_children()
            .expect("command result tree must be readable")
            .is_empty());
        assert_eq!(result.focused_task_id_opt, Some(task.get_id().unwrap()));
    }
}

#[test]
fn test_execute_estimate_見積もりを更新し不正値では維持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();

    let updated = execute_command_for_test(task, now, Some(task_id), "予 45");
    assert_eq!(updated.task.get_estimated_work_seconds().unwrap(), 45 * 60);

    let unchanged = execute_command_for_test(updated.task, now, Some(task_id), "予 invalid");
    assert_eq!(
        unchanged.task.get_estimated_work_seconds().unwrap(),
        45 * 60
    );
}

#[test]
fn test_execute_estimate_不正値はfield付き入力エラーを表示して状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    task.set_estimated_work_seconds(45 * 60);
    let task_id = task.get_id().unwrap();

    let result = execute_command_for_test(task, now, Some(task_id), "予 invalid");

    assert_eq!(result.task.get_estimated_work_seconds().unwrap(), 45 * 60);
    assert_eq!(result.focused_task_id_opt, Some(task_id));
    assert!(result
        .output
        .contains("[Error] 入力エラー: estimated_work_minutes:"));
}

#[test]
fn test_execute_actual_priority_work_typed値でtaskを更新する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();

    let actual = execute_command_for_test(task, now, Some(task_id), "実 20");
    assert_eq!(actual.task.get_actual_work_seconds().unwrap(), 20 * 60);

    let prioritized = execute_command_for_test(actual.task, now, Some(task_id), "重 7");
    assert_eq!(prioritized.task.get_priority().unwrap(), 7);

    let worked = execute_command_for_test(prioritized.task, now, Some(task_id), "働 5");
    assert_eq!(worked.task.get_actual_work_seconds().unwrap(), 25 * 60);
    assert_eq!(worked.focused_task_id_opt, None);
}

#[test]
fn test_execute_actual_priority_work_不正値はfield付き入力エラーで状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    for command in ["実 invalid", "重 invalid", "働 invalid"] {
        let task = new_test_task_handle("更新対象").unwrap();
        task.set_actual_work_seconds(20 * 60);
        task.set_priority(7);
        let task_id = task.get_id().unwrap();

        let result = execute_command_for_test(task, now, Some(task_id), command);

        assert_eq!(result.task.get_actual_work_seconds().unwrap(), 20 * 60);
        assert_eq!(result.task.get_priority().unwrap(), 7);
        assert_eq!(result.focused_task_id_opt, Some(task_id));
        assert!(result.output.contains("[Error] 入力エラー:"));
    }
}

#[test]
fn test_execute_actual_work_overflowはfield付きerrorで状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    for command in [
        format!("実 {}", i64::MAX),
        format!("実 {}", i64::MIN),
        format!("働 {}", i64::MAX),
        format!("働 {}", i64::MIN),
    ] {
        let task = new_test_task_handle("更新対象").unwrap();
        task.set_actual_work_seconds(20 * 60);
        let task_id = task.get_id().unwrap();

        let result = execute_command_for_test(task, now, Some(task_id), &command);

        assert_eq!(result.task.get_actual_work_seconds().unwrap(), 20 * 60);
        assert_eq!(result.focused_task_id_opt, Some(task_id));
        assert!(result.output.contains("actual_work_minutes"));
    }
}

#[test]
fn test_execute_deadline_締切を設定して解除する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();

    let updated = execute_command_for_test(task, now, Some(task_id), "〆 2026/08/20");
    assert_eq!(
        updated.task.get_deadline_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap())
    );

    let cleared = execute_command_for_test(updated.task, now, Some(task_id), "〆 消");
    assert_eq!(cleared.task.get_deadline_time_opt().unwrap(), None);

    let time_updated = execute_command_for_test(cleared.task, now, Some(task_id), "〆 14:30");
    assert_eq!(
        time_updated.task.get_deadline_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 8, 11, 14, 30, 0).unwrap())
    );

    let invalid = execute_command_for_test(time_updated.task, now, Some(task_id), "〆 invalid");
    assert_eq!(
        invalid.task.get_deadline_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 8, 11, 14, 30, 0).unwrap())
    );

    let today_task = new_test_task_handle("今日締切").unwrap();
    let today_task_id = today_task.get_id().unwrap();
    let today = execute_command_for_test(today_task, now, Some(today_task_id), "〆 今日");
    assert_eq!(
        today.task.get_deadline_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 8, 11, 23, 59, 59).unwrap())
    );

    let tomorrow_task = new_test_task_handle("明日締切").unwrap();
    let tomorrow_task_id = tomorrow_task.get_id().unwrap();
    let tomorrow = execute_command_for_test(tomorrow_task, now, Some(tomorrow_task_id), "〆 明日");
    assert_eq!(
        tomorrow.task.get_deadline_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 8, 12, 23, 59, 59).unwrap())
    );
}

#[test]
fn test_execute_deadline_不正日時はfield付き入力エラーを表示して状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();
    let previous_deadline = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
    task.set_deadline_time_opt(Some(previous_deadline));

    let result = execute_command_for_test(task, now, Some(task_id), "〆 invalid");

    assert_eq!(
        result.task.get_deadline_time_opt().unwrap(),
        Some(previous_deadline)
    );
    assert!(result.output.contains("[Error] 入力エラー: deadline:"));

    for command in ["〆 13/40", "〆 25:99"] {
        let result = execute_command_for_test(result.task.clone(), now, Some(task_id), command);
        assert_eq!(
            result.task.get_deadline_time_opt().unwrap(),
            Some(previous_deadline)
        );
        assert!(result.output.contains("コマンド: 〆"));
        assert!(result.output.contains("使い方: 〆 <日付または時刻>"));
    }
}

#[test]
fn test_execute_arrange_デフォルトで見積もり0と完了済みを維持する() {
    let task = execute_arrange_command("揃 15");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 0);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_arrange_全指定で見積もり0も変更し完了済みは維持する() {
    let task = execute_arrange_command("揃 15 全");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_arrange_all指定は全指定と同じ挙動になる() {
    let task = execute_arrange_command("arr 15 all");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_arrange_未知の第3引数で見積もり0を維持する() {
    let task = execute_arrange_command("揃 15 unknown");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 15 * 60);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 0);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_arrange_見積もり0分を受理する() {
    let task = execute_arrange_command("揃 0");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 0);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 0);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_arrange_見積もり1439分を受理する() {
    let task = execute_arrange_command("揃 1439");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 1439 * 60);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 0);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_arrange_見積もり1440分では変更しない() {
    let task = execute_arrange_command("揃 1440");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 5 * 60);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 0);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_arrange_負の見積もりでは変更しない() {
    let task = execute_arrange_command("揃 -1");
    let children = task
        .get_children()
        .expect("arrange result tree must be readable");

    assert_eq!(children[0].get_estimated_work_seconds().unwrap(), 5 * 60);
    assert_eq!(children[1].get_estimated_work_seconds().unwrap(), 0);
    assert_eq!(children[2].get_estimated_work_seconds().unwrap(), 10 * 60);
}

#[test]
fn test_execute_sequential_接尾辞の前にハイフンを付ける() {
    let (task, focused_task_id_opt) = execute_sequential_command("連 鎖タスク 10 1 2 話");

    let children = task
        .get_children()
        .expect("sequential result tree must be readable");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].get_name().unwrap(), "鎖タスク 2-話");

    let grand_children = children[0]
        .get_children()
        .expect("sequential result subtree must be readable");
    assert_eq!(grand_children.len(), 1);
    assert_eq!(grand_children[0].get_name().unwrap(), "鎖タスク 1-話");
    assert_eq!(
        focused_task_id_opt,
        Some(grand_children[0].get_id().unwrap())
    );
}

#[test]
fn test_execute_sequential_接尾辞なしではハイフンを付けない() {
    let (task, focused_task_id_opt) = execute_sequential_command("連 鎖タスク 10 1 2");

    let children = task
        .get_children()
        .expect("sequential result tree must be readable");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].get_name().unwrap(), "鎖タスク 2");

    let grand_children = children[0]
        .get_children()
        .expect("sequential result subtree must be readable");
    assert_eq!(grand_children.len(), 1);
    assert_eq!(grand_children[0].get_name().unwrap(), "鎖タスク 1");
    assert_eq!(
        focused_task_id_opt,
        Some(grand_children[0].get_id().unwrap())
    );
}

#[test]
fn test_execute_finish_引数なしは実作業時間を自動加算して現在時刻で完了する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    task.set_actual_work_seconds(60);
    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "終",
    );

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(actual.get_status().unwrap(), Status::Done);
    assert_eq!(actual.get_actual_work_seconds().unwrap(), 360);
    assert_eq!(actual.get_end_time_opt().unwrap(), Some(now));
}

#[test]
fn test_execute_finish_今は実作業時間を自動加算せず現在時刻で完了する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    task.set_actual_work_seconds(60);
    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "終 今",
    );

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(actual.get_status().unwrap(), Status::Done);
    assert_eq!(actual.get_actual_work_seconds().unwrap(), 60);
    assert_eq!(actual.get_end_time_opt().unwrap(), Some(now));
}

#[test]
fn test_execute_finish_時刻指定は実作業時間を自動加算せず指定時刻で完了する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    task.set_actual_work_seconds(60);
    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "終 14:30",
    );

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(actual.get_status().unwrap(), Status::Done);
    assert_eq!(actual.get_actual_work_seconds().unwrap(), 60);
    assert_eq!(
        actual.get_end_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 5, 17, 14, 30, 0).unwrap())
    );
}

#[test]
fn test_execute_finish_秒つき時刻指定は指定秒で完了する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    task.set_actual_work_seconds(60);
    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "終 9:23:45 2026/7/4",
    );

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(actual.get_status().unwrap(), Status::Done);
    assert_eq!(actual.get_actual_work_seconds().unwrap(), 60);
    assert_eq!(
        actual.get_end_time_opt().unwrap(),
        Some(Local.with_ymd_and_hms(2026, 7, 4, 9, 23, 45).unwrap())
    );
}

#[test]
fn test_execute_finish_不正な引数では完了しない() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 5, 0).unwrap();
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    task.set_actual_work_seconds(60);
    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "終 xxx",
    );

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(actual.get_status().unwrap(), Status::Todo);
    assert_eq!(actual.get_actual_work_seconds().unwrap(), 60);
    assert_eq!(actual.get_end_time_opt().unwrap(), None);
}

#[test]
fn test_execute_today_カテゴリ別の予定時間集計を表示する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let focus_started_datetime = now;
    let task = new_test_task_handle("投資タスク").unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Investment));
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager::with_free_minutes(30);
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "今",
    );

    let actual = String::from_utf8(stdout.buffer).unwrap();
    assert!(actual.contains(" 00 資 投資タスク"));
    assert!(actual.contains(
        "予定カテゴリ: 獲得 0.0時間(0% | 0%) / 維持 0.0時間(0% | 0%) / 回復 0.0時間(0% | 0%) / 投資 1.0時間(200% | 200%) / 消費 0.0時間(0% | 200%) / 未分類 0.0時間(0% | 200%)"
    ));
}

#[test]
fn test_execute_today_今を絞る全経路で負荷指標を表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("今の負荷指標用タスク").unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    task.set_pending_until(now);
    task.set_orig_status(Status::Pending);
    let expected_footer = concat!(
        "残り拘束時間は0.0時間です\n",
        "完了見込み日時は1.0時間後の2026/08/11 13:00:00です\n",
        "rep ρ = (1.00 + 0.00) / (1.00 + 0.00 + 11 + 30/60) = 0.08, Lq = 0.1\n",
        "one ρ = (1.00 + 0.00) / (1.00 + 0.00 + 11 + 30/60) = 0.08, Lq = 0.1\n",
        "\n",
    );

    for command in ["今", "today", "全 今", "尾", "尾 今"] {
        let actual = execute_calendar_command_for_test(command, now, task.clone(), 10 * 60);
        assert!(
            actual.ends_with(expected_footer),
            "{command} must end with today's load metrics: {actual}"
        );
    }

    let weekly = execute_calendar_command_for_test("全 週", now, task, 10 * 60);
    assert!(!weekly.contains("残り拘束時間は"), "{weekly}");
    assert!(!weekly.contains("rep ρ ="), "{weekly}");
}

#[test]
fn test_execute_set_project_category_表示記号でカテゴリを設定する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let focus_started_datetime = now;
    let task = new_test_task_handle("タスク").unwrap();
    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "類 資",
    );

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(
        actual.get_project_category_opt().unwrap(),
        Some(ProjectCategory::Investment)
    );
}

#[test]
fn test_execute_set_project_category_英語aliasでカテゴリを設定する() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let focus_started_datetime = now;
    let task = new_test_task_handle("タスク").unwrap();
    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "category earning",
    );

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(
        actual.get_project_category_opt().unwrap(),
        Some(ProjectCategory::Earning)
    );

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "cat 消",
    );

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(
        actual.get_project_category_opt().unwrap(),
        Some(ProjectCategory::Consumption)
    );
}

#[test]
fn test_execute_set_project_category_未分類に戻す() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let focus_started_datetime = now;
    let task = new_test_task_handle("タスク").unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Investment));
    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    for cmd in ["類 _", "類 none", "類 clear"] {
        task.set_project_category_opt(Some(ProjectCategory::Investment));

        execute(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &focus_started_datetime,
            cmd,
        );

        let actual = task_repository
            .get_by_id(task_id)
            .expect("fixture repository lookup must succeed")
            .expect("fixture task must exist");
        assert_eq!(actual.get_project_category_opt().unwrap(), None);
    }
}

#[test]
fn test_execute_set_project_category_不正カテゴリでは変更しない() {
    let now = Local.with_ymd_and_hms(2026, 5, 17, 12, 0, 0).unwrap();
    let focus_started_datetime = now;
    let task = new_test_task_handle("タスク").unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Investment));
    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task.clone(), now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    execute(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        "類 invalid",
    );

    let actual = task_repository
        .get_by_id(task_id)
        .expect("fixture repository lookup must succeed")
        .expect("fixture task must exist");
    assert_eq!(
        actual.get_project_category_opt().unwrap(),
        Some(ProjectCategory::Investment)
    );
}

#[test]
fn test_execute_category_不正値はfield付き入力エラーを表示して状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("カテゴリ対象").unwrap();
    let task_id = task.get_id().unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Investment));

    let result = execute_command_for_test(task, now, Some(task_id), "類 invalid");

    assert_eq!(
        result.task.get_project_category_opt().unwrap(),
        Some(ProjectCategory::Investment)
    );
    assert!(result.output.contains("[Error] 入力エラー: category:"));
}

#[test]
fn runtime外部ioとoutcome調停は共通境界に集約する() {
    let now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();
    let task = new_test_task_handle("外部要求の参照対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut focused_task_id_opt = Some(task_id);

    let open_command = parse_command("開", ParseMode::NonInteractive).unwrap();
    let mut open_outcome = handle(&open_command).expect("open must be handler-owned");
    open_outcome.display = DisplayModel::newline("外部要求の前に表示");
    let mut flushed_output = FlushTrackingWriter::successful(false);
    apply_command_outcome(
        &mut flushed_output,
        &mut task_repository,
        &mut focused_task_id_opt,
        OutcomeApplicationMode::Flushed,
        open_outcome,
        active_config(),
    )
    .unwrap();

    assert_eq!(
        String::from_utf8(flushed_output.buffer).unwrap(),
        "外部要求の前に表示\n"
    );
    assert_eq!(task_repository.get_by_id_attempt_count.get(), 1);
    assert_eq!(flushed_output.flush_count, 1);

    let noop_command = parse_command("", ParseMode::NonInteractive).unwrap();
    let noop_outcome = handle(&noop_command).expect("noop must be handler-owned");
    let mut noop_output = FlushTrackingWriter::successful(false);
    apply_command_outcome(
        &mut noop_output,
        &mut task_repository,
        &mut focused_task_id_opt,
        OutcomeApplicationMode::Flushed,
        noop_outcome,
        active_config(),
    )
    .unwrap();
    assert_eq!(noop_output.flush_count, 0);

    let focus_command = parse_command("高", ParseMode::Interactive).unwrap();
    let focus_outcome = handle(&focus_command).expect("focus mode must be handler-owned");
    let mut focus_output = FlushTrackingWriter::successful(false);
    let mut focus_selection_mode = FocusSelectionMode::Explicit;
    apply_command_outcome(
        &mut focus_output,
        &mut task_repository,
        &mut focused_task_id_opt,
        OutcomeApplicationMode::InteractiveUnflushed(&mut focus_selection_mode),
        focus_outcome,
        active_config(),
    )
    .unwrap();

    assert_eq!(
        String::from_utf8(focus_output.buffer).unwrap(),
        "フォーカス選択モード: 高\n"
    );
    assert_eq!(focus_selection_mode, FocusSelectionMode::HighestPriority);
    assert_eq!(focused_task_id_opt, None);
    assert_eq!(focus_output.flush_count, 0);

    focused_task_id_opt = Some(task_id);
    let clear_command = parse_command("外", ParseMode::Interactive).unwrap();
    let clear_outcome = handle(&clear_command).expect("unfocus must be handler-owned");
    let mut clear_output = FlushTrackingWriter::successful(false);
    let mut clear_selection_mode = FocusSelectionMode::LowestPriority { recent_days: 3 };
    apply_command_outcome(
        &mut clear_output,
        &mut task_repository,
        &mut focused_task_id_opt,
        OutcomeApplicationMode::InteractiveUnflushed(&mut clear_selection_mode),
        clear_outcome,
        active_config(),
    )
    .unwrap();

    assert_eq!(
        clear_selection_mode,
        FocusSelectionMode::LowestPriority { recent_days: 3 }
    );
    assert_eq!(focused_task_id_opt, None);
    assert_eq!(clear_output.flush_count, 0);

    focused_task_id_opt = Some(task_id);
    let low_command = parse_command("低 3", ParseMode::Interactive).unwrap();
    let low_outcome = handle(&low_command).expect("low focus mode must be handler-owned");
    let mut low_output = FlushTrackingWriter::successful(false);
    let mut low_selection_mode = FocusSelectionMode::Explicit;
    apply_command_outcome(
        &mut low_output,
        &mut task_repository,
        &mut focused_task_id_opt,
        OutcomeApplicationMode::InteractiveUnflushed(&mut low_selection_mode),
        low_outcome,
        active_config(),
    )
    .unwrap();

    assert_eq!(
        String::from_utf8(low_output.buffer).unwrap(),
        "フォーカス選択モード: 低 3\n"
    );
    assert_eq!(
        low_selection_mode,
        FocusSelectionMode::LowestPriority { recent_days: 3 }
    );
    assert_eq!(focused_task_id_opt, None);
    assert_eq!(low_output.flush_count, 0);

    for (error_kind, expects_success) in [
        (std::io::ErrorKind::BrokenPipe, true),
        (std::io::ErrorKind::Other, false),
    ] {
        let task = new_test_task_handle("出力errorの対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut task_repository = TestTaskRepository::new(task, now);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut focused_task_id_opt = Some(task_id);
        let parsed = parse_command("予 15", ParseMode::NonInteractive).unwrap();
        let mut output = FlushTrackingWriter::failing(error_kind);

        let result = execute_parsed(
            &mut output,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            &now,
            &parsed,
        );

        if expects_success {
            assert!(result.is_ok());
        } else {
            assert!(matches!(
                result,
                Err(CommandError::Output(error)) if error.kind() == std::io::ErrorKind::Other
            ));
        }
        assert_eq!(output.flush_count, 1);
    }

    let runtime_source = include_str!("runtime.rs");
    assert!(
        !runtime_source.contains("\nfn execute_handler_outcome("),
        "the superseded outcome coordinator must be removed"
    );

    for isolated_source in [
        include_str!("handler.rs"),
        include_str!("interactive.rs"),
        include_str!("renderer.rs"),
    ] {
        for forbidden in [
            "run_repository_transaction",
            "webbrowser::open",
            "process::Command",
        ] {
            assert!(
                !isolated_source.contains(forbidden),
                "external I/O and repository transactions must remain in runtime: {forbidden}"
            );
        }
    }
}

#[test]
fn external_requestは副作用なしでtyped_targetへ解決する() {
    let root = new_test_task_handle("root https://example.com/tasks/42").unwrap();
    let root_id = root.get_id().unwrap();
    let focused_task = root.create_as_last_child(new_test_task_attr("focused task"));
    let focused_task_opt = Some(focused_task);
    let config = SchronuConfig {
        obsidian_vault_name: "Work & Notes".to_string(),
        ..SchronuConfig::default()
    };

    assert_eq!(
        resolve_external_request(
            ExternalRequest::OpenFocusedLink,
            &focused_task_opt,
            &config,
        )
        .unwrap(),
        Some(ResolvedExternalRequest::BrowserUrl(
            "https://example.com/tasks/42".to_string()
        ))
    );
    assert_eq!(
        resolve_external_request(
            ExternalRequest::OpenObsidianRootSearch,
            &focused_task_opt,
            &config,
        )
        .unwrap(),
        Some(ResolvedExternalRequest::ObsidianUrl(format!(
            "obsidian://search?vault=Work%20%26%20Notes&query={root_id}"
        )))
    );
}

#[test]
fn external_open_errorはtargetとsource_reason_chainを保持する() {
    let error = external_open_error("test-target", std::io::Error::other("test-reason"));

    assert_eq!(
        error.to_string(),
        "外部起動エラー (test-target): test-reason"
    );
    assert_eq!(
        std::error::Error::source(&error).map(ToString::to_string),
        Some("test-reason".to_string())
    );
}

#[test]
fn test_execute_flatten_過負荷日では葉より親を先に翌日へ延期する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(30 * 60);
    root.set_start_time(now);
    root.set_pending_until(now);
    root.set_orig_status(Status::Pending);
    let child = add_scheduled_child_for_test(&root, "着手可能な葉", now, 60);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root.clone(),
        HashMap::from([(today, 60), (today + Duration::days(1), 60)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(root.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        try_subjective_date_start(today + Duration::days(1)).unwrap()
    );
    assert_eq!(
        result
            .task
            .get_by_id(child.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        now
    );
    assert!(result
        .output
        .contains(&format!("\t{}\t平テスト", root.get_id().unwrap())));
    assert!(result.output.contains("平: 1件 00:30"));
}

#[test]
fn test_execute_flatten_多階層ではrankが大きい親から延期する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(30 * 60);
    root.set_start_time(now);
    root.set_pending_until(now);
    root.set_orig_status(Status::Pending);
    let middle = add_scheduled_child_for_test(&root, "中間親", now, 30);
    add_scheduled_child_for_test(&middle, "葉", now, 60);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root.clone(),
        HashMap::from([(today, 60), (today + Duration::days(1), 120)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(root.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        try_subjective_date_start(today + Duration::days(1)).unwrap()
    );
    assert_eq!(
        result
            .task
            .get_by_id(middle.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        try_subjective_date_start(today + Duration::days(1)).unwrap()
    );
    let root_position = result.output.find("\t平テスト\n").unwrap();
    let middle_position = result.output.find("\t中間親\n").unwrap();
    assert!(root_position < middle_position);
}

#[test]
fn test_execute_flatten_親だけで解消できなければ低優先度の葉も連鎖延期する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(30 * 60);
    root.set_start_time(now);
    root.set_pending_until(now);
    root.set_orig_status(Status::Pending);
    let high = add_scheduled_child_for_test(&root, "先に予定された葉", now, 45);
    let low =
        add_scheduled_child_for_test(&root, "後に予定された葉", now + Duration::minutes(45), 45);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root.clone(),
        HashMap::from([
            (today, 60),
            (today + Duration::days(1), 30),
            (today + Duration::days(2), 90),
        ]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(root.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        try_subjective_date_start(today + Duration::days(2)).unwrap()
    );
    assert_eq!(
        result
            .task
            .get_by_id(low.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        try_subjective_date_start(today + Duration::days(2)).unwrap()
    );
    assert_eq!(
        result
            .task
            .get_by_id(high.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        now
    );
    assert_eq!(
        result
            .output
            .matches(&low.get_id().unwrap().to_string())
            .count(),
        1
    );
}

#[test]
fn test_execute_flatten_余裕日と100percentちょうどの日は変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();

    for command in ["平", "flatten", "flat"] {
        let root = new_test_task_handle("平テスト").unwrap();
        root.set_estimated_work_seconds(0);
        let target = add_scheduled_child_for_test(&root, "変更しない", now, 60);

        let result = execute_flatten_command_for_test(
            command,
            now,
            root,
            HashMap::from([(today, 60), (today + Duration::days(1), 60)]),
        );

        assert_eq!(
            result
                .task
                .get_by_id(target.get_id().unwrap())
                .unwrap()
                .get_pending_until()
                .unwrap(),
            now
        );
        assert_eq!(result.output, "[Info] 100%を超過している日はありません。\n");
    }
}

#[test]
fn test_execute_flatten_28日境界の超過を29日から34日を飛ばして35日後へ退避する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let boundary_date = today + Duration::days(28);
    let overflow_date = today + Duration::days(35);
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let boundary_start = try_subjective_date_start(boundary_date).unwrap();
    let keeper = add_scheduled_child_for_test(&root, "境界に残す", boundary_start, 30);
    let first = add_scheduled_child_for_test(
        &root,
        "退避対象1",
        boundary_start + Duration::minutes(30),
        30,
    );
    let second = add_scheduled_child_for_test(
        &root,
        "退避対象2",
        boundary_start + Duration::minutes(60),
        30,
    );

    let result =
        execute_flatten_command_for_test("平", now, root, HashMap::from([(boundary_date, 30)]));

    assert_eq!(
        result
            .task
            .get_by_id(keeper.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        try_subjective_date_start(boundary_date).unwrap()
    );
    assert_eq!(
        result
            .task
            .get_by_id(first.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        try_subjective_date_start(overflow_date).unwrap()
    );
    assert_eq!(
        result
            .task
            .get_by_id(second.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        try_subjective_date_start(overflow_date).unwrap()
    );
    assert_eq!(
        result
            .output
            .matches(&format!("平\t{}\t{}\t00:30", boundary_date, overflow_date))
            .count(),
        2
    );
    assert!(result
        .output
        .contains("[Warn] 35日後の退避先は日次容量の上限を適用していません: 2件 01:00"));
}

#[test]
fn test_execute_flatten_日容量を超えるtaskだけでは解消不能として状態を変更しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let target = add_scheduled_child_for_test(&root, "大きすぎる", now, 90);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 60), (today + Duration::days(1), 60)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(target.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        now
    );
    assert!(result.output.starts_with("平: 0件 00:00 (未解消1日)\n"));
    assert!(result
        .output
        .contains("[Warn] 平\t2026-08-13\t未解消 00:30"));
    assert!(result.output.contains("1日の最大容量を超える: 1件"));
    assert!(result
        .output
        .contains(&format!("{}\t大きすぎる", target.get_id().unwrap())));
    assert!(!result.output.contains("[Stop]"));
}

#[test]
fn test_execute_flatten_未解消の超過が1分未満でも切り上げて表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let target = add_scheduled_child_for_test(&root, "1秒超過", now, 60);
    target.set_estimated_work_seconds(60 * 60 + 1);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 60), (today + Duration::days(1), 60)]),
    );

    assert!(result
        .output
        .contains("[Warn] 平\t2026-08-13\t未解消 00:01"));
}

#[test]
fn test_execute_flatten_業務日境界をまたぐtaskは延期しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let target = add_scheduled_child_for_test(&root, "境界をまたぐ", now, 25 * 60);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 60), (today + Duration::days(1), 26 * 60)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(target.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        now
    );
    assert!(result.output.contains("業務日境界をまたぐ: 1件"));
    assert!(result
        .output
        .contains(&format!("{}\t境界をまたぐ", target.get_id().unwrap())));
}

#[test]
fn test_execute_flatten_業務日境界をまたぐtaskの全作業時間を開始日の業務日に計上する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    add_scheduled_child_for_test(&root, "日境界をまたぐ", now, 25 * 60);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 24 * 60), (today + Duration::days(1), 26 * 60)]),
    );

    assert!(result.output.starts_with("平: 0件 00:00 (未解消1日)\n"));
    assert!(result
        .output
        .contains(&format!("[Warn] 平\t{}\t未解消 01:00", today)));
    assert!(result.output.contains("業務日境界をまたぐ: 1件"));
}

#[test]
fn test_execute_flatten_終了時刻が期限と等しいtaskは延期できる() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    root.set_deadline_time_opt(Some(
        try_subjective_date_start(today + Duration::days(1)).unwrap() + Duration::minutes(30),
    ));
    let target = add_scheduled_child_for_test(&root, "期限ちょうど", now, 30);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 15), (today + Duration::days(1), 30)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(target.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        try_subjective_date_start(today + Duration::days(1)).unwrap()
    );
}

#[test]
fn test_execute_flatten_延期対象自身の期限補正で翌日06時を維持できなければ延期しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let target = add_scheduled_child_for_test(&root, "平日を表すダミータスク(8/21)", now, 30);
    target.set_deadline_time_opt(Some(
        try_subjective_date_start(today + Duration::days(1)).unwrap() + Duration::minutes(30),
    ));

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 15), (today + Duration::days(1), 30)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(target.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        now
    );
    assert!(result
        .output
        .contains("自身の期限により翌日06:00を維持できない: 1件"));
    assert!(result.output.contains(&format!(
        "{}\t平日を表すダミータスク(8/21)",
        target.get_id().unwrap()
    )));
}

#[test]
fn test_execute_flatten_待機taskと残作業0を延期候補から除外する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let movable = add_scheduled_child_for_test(&root, "移動対象", now, 30);
    let waiting = add_scheduled_child_for_test(&root, "待機", now, 30);
    waiting.set_is_on_other_side(true);
    let zero = add_scheduled_child_for_test(&root, "残作業0", now, 0);

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 30), (today + Duration::days(1), 30)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(movable.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        try_subjective_date_start(today + Duration::days(1)).unwrap()
    );
    for unchanged in [waiting.get_id().unwrap(), zero.get_id().unwrap()] {
        assert_eq!(
            result
                .task
                .get_by_id(unchanged)
                .unwrap()
                .get_pending_until()
                .unwrap(),
            now
        );
    }
}

#[test]
fn test_execute_flatten_35日後への退避で親の期限を超えるなら未解消として残す() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let boundary_date = today + Duration::days(28);
    let root = new_test_task_handle("期限のある親").unwrap();
    root.set_estimated_work_seconds(30 * 60);
    root.set_start_time(try_subjective_date_start(boundary_date).unwrap());
    root.set_pending_until(try_subjective_date_start(boundary_date).unwrap());
    root.set_orig_status(Status::Pending);
    root.set_deadline_time_opt(Some(
        try_subjective_date_start(today + Duration::days(35)).unwrap(),
    ));
    let child = add_scheduled_child_for_test(
        &root,
        "境界の葉",
        try_subjective_date_start(boundary_date).unwrap(),
        60,
    );

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root.clone(),
        HashMap::from([(boundary_date, 60)]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(root.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        try_subjective_date_start(boundary_date).unwrap()
    );
    assert_eq!(
        result
            .task
            .get_by_id(child.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        try_subjective_date_start(boundary_date).unwrap()
    );
    assert!(result.output.contains("平: 0件 00:00 (未解消1日)"));
    assert!(result
        .output
        .contains("仮延期によって関連taskの期限を超える: 1件"));
    assert!(!result.output.contains("[Stop]"));
}

#[test]
fn test_execute_flatten_延期不能日を飛ばして翌日以降の平坦化を保存する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let blocked = add_scheduled_child_for_test(&root, "今日の固定負荷", now, 90);
    let tomorrow_start = try_subjective_date_start(today + Duration::days(1)).unwrap();
    let tomorrow_first = add_scheduled_child_for_test(&root, "翌日の先行", tomorrow_start, 30);
    let tomorrow_late = add_scheduled_child_for_test(
        &root,
        "翌日の延期対象",
        tomorrow_start + Duration::minutes(30),
        30,
    );

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([
            (today, 60),
            (today + Duration::days(1), 30),
            (today + Duration::days(2), 30),
        ]),
    );

    assert_eq!(
        result
            .task
            .get_by_id(blocked.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        now
    );
    assert_eq!(
        result
            .task
            .get_by_id(tomorrow_first.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        tomorrow_start
    );
    assert_eq!(
        result
            .task
            .get_by_id(tomorrow_late.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        try_subjective_date_start(today + Duration::days(2)).unwrap()
    );
    assert!(result.output.contains("平: 1件 00:30 (未解消1日)"));
    assert_eq!(result.output.matches("[Warn] 平\t2026-08-13").count(), 1);
}

#[test]
fn test_execute_flatten_未解消理由を固定順で表示して同じtaskを重複計上しない() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let waiting = add_scheduled_child_for_test(&root, "待機かつ大きすぎる", now, 90);
    waiting.set_is_on_other_side(true);
    let own_deadline = add_scheduled_child_for_test(&root, "自身に期限", now, 30);
    own_deadline.set_deadline_time_opt(Some(
        try_subjective_date_start(today + Duration::days(1)).unwrap() + Duration::minutes(30),
    ));

    let result = execute_flatten_command_for_test(
        "平",
        now,
        root,
        HashMap::from([(today, 60), (today + Duration::days(1), 60)]),
    );

    let waiting_reason = result.output.find("相手待ち: 1件").unwrap();
    let deadline_reason = result
        .output
        .find("自身の期限により翌日06:00を維持できない: 1件")
        .unwrap();
    assert!(waiting_reason < deadline_reason);
    assert_eq!(
        result
            .output
            .matches(&waiting.get_id().unwrap().to_string())
            .count(),
        1
    );
    assert!(!result.output.contains("1日の最大容量を超える:"));
}

#[test]
fn test_execute_flatten_28日目は延期可能分を35日目へ退避して固定負荷を未解消にする() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();
    let boundary_date = today + Duration::days(28);
    let overflow_date = today + Duration::days(35);
    let boundary_start = try_subjective_date_start(boundary_date).unwrap();
    let root = new_test_task_handle("平テスト").unwrap();
    root.set_estimated_work_seconds(0);
    let waiting = add_scheduled_child_for_test(&root, "境界の待機", boundary_start, 30);
    waiting.set_is_on_other_side(true);
    let movable = add_scheduled_child_for_test(
        &root,
        "35日目へ退避",
        boundary_start + Duration::minutes(30),
        30,
    );
    let deadline = add_scheduled_child_for_test(
        &root,
        "境界期限",
        boundary_start + Duration::minutes(60),
        30,
    );
    deadline.set_deadline_time_opt(Some(boundary_start + Duration::hours(18)));

    let result =
        execute_flatten_command_for_test("平", now, root, HashMap::from([(boundary_date, 30)]));

    assert_eq!(
        result
            .task
            .get_by_id(movable.get_id().unwrap())
            .unwrap()
            .get_pending_until()
            .unwrap(),
        try_subjective_date_start(overflow_date).unwrap()
    );
    assert!(result.output.contains("平: 1件 00:30 (未解消1日)"));
    assert!(result
        .output
        .contains("[Warn] 35日後の退避先は日次容量の上限を適用していません: 1件 00:30"));
    assert!(result
        .output
        .contains(&format!("[Warn] 平\t{}\t未解消 00:30", boundary_date)));
}

#[test]
fn test_execute_flatten_各aliasで未解消日を飛ばして後続を延期する() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 6, 0, 0).unwrap();
    let today = now.date_naive();

    for command in ["平", "flatten", "flat"] {
        let root = new_test_task_handle("平テスト").unwrap();
        root.set_estimated_work_seconds(0);
        add_scheduled_child_for_test(&root, "固定負荷", now, 90);
        let tomorrow = try_subjective_date_start(today + Duration::days(1)).unwrap();
        add_scheduled_child_for_test(&root, "翌日の先行", tomorrow, 30);
        let movable = add_scheduled_child_for_test(
            &root,
            "翌日の延期対象",
            tomorrow + Duration::minutes(30),
            30,
        );

        let result = execute_flatten_command_for_test(
            command,
            now,
            root,
            HashMap::from([
                (today, 60),
                (today + Duration::days(1), 30),
                (today + Duration::days(2), 30),
            ]),
        );

        assert_eq!(
            result
                .task
                .get_by_id(movable.get_id().unwrap())
                .unwrap()
                .get_pending_until()
                .unwrap(),
            try_subjective_date_start(today + Duration::days(2)).unwrap()
        );
        assert!(result.output.contains("平: 1件 00:30 (未解消1日)"));
    }
}

#[test]
fn test_execute_calendar_現行出力を固定する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("暦出力固定用タスク").unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    task.set_pending_until(now);
    task.set_orig_status(Status::Pending);

    let actual = execute_calendar_command_for_test("暦", now, task.clone(), 10 * 60);
    let expected = concat!(
        "2026-08-11(火)\t10.0時間\t-9時間00分     \t-0.90\t-6時間00分\t-06時間00分\t-10時間00分\t-1.00\t-09時間00分\t 10時間00分\t-0.90\t01[タスク]\n",
        "日          \t空          \t空差      \t空差比\t余差    \t余差累    \t〆差      \t〆差比\t空差累    \t単発余暇\t空差累比\tタスク数\n",
        "\n",
        "今のタスクが片付く日付: 4160日後の2037-12-31\n",
        "最大の累積時間: -09時間00分 (2026-08-11), 最大のrhoの差: -1.00 (1900-01-01), 次にタスクを積める日付: 0日後の2026-08-11 (-6時間00分)\n",
        "\n",
        "[Info] 順調です。突発タスクに対応したり1日の終わり際にタスクを新しく積んだりする余裕があります。ひとまずは脇道に逸れずに予定の遂行をしてください。\n",
        "\n",
        "残り拘束時間は0.0時間です\n",
        "完了見込み日時は1.0時間後の2026/08/11 13:00:00です\n",
        "rep ρ = (1.00 + 0.00) / (1.00 + 0.00 + 11 + 30/60) = 0.08, Lq = 0.1\n",
        "one ρ = (1.00 + 0.00) / (1.00 + 0.00 + 11 + 30/60) = 0.08, Lq = 0.1\n",
        "\n",
    );

    assert_eq!(actual, expected);

    let english_alias = execute_calendar_command_for_test("cal", now, task, 10 * 60);
    assert_eq!(english_alias, expected);
}

#[test]
fn test_execute_calendar_日付逆順と週区切りと28日境界を固定する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let root = new_test_task_handle("暦複数日fixture").unwrap();
    root.set_estimated_work_seconds(0);
    add_scheduled_child_for_test(&root, "当日", now, 15);
    add_scheduled_child_for_test(
        &root,
        "月曜日",
        Local.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap(),
        15,
    );
    add_scheduled_child_for_test(
        &root,
        "28日境界",
        Local.with_ymd_and_hms(2026, 9, 8, 12, 0, 0).unwrap(),
        15,
    );
    add_scheduled_child_for_test(
        &root,
        "29日目",
        Local.with_ymd_and_hms(2026, 9, 9, 12, 0, 0).unwrap(),
        15,
    );

    let actual = execute_calendar_command_for_test("暦", now, root, 10 * 60);
    let lines = actual.lines().collect::<Vec<_>>();
    let boundary_index = lines
        .iter()
        .position(|line| line.starts_with("2026-09-08(火)"))
        .unwrap();
    let monday_index = lines
        .iter()
        .position(|line| line.starts_with("2026-08-17(月)"))
        .unwrap();
    let today_index = lines
        .iter()
        .position(|line| line.starts_with("2026-08-11(火)"))
        .unwrap();

    assert!(boundary_index < monday_index);
    assert!(monday_index < today_index);
    assert_eq!(lines[monday_index + 1], "");
    assert!(!actual.contains("2026-09-09(水)"));
}

#[test]
fn test_format_daily_band_累積境界で端数を丸めて96文字にする() {
    let date = NaiveDate::from_ymd_opt(2026, 8, 15).unwrap();
    let actual = format_daily_band(
        date,
        "土",
        Duration::hours(46) + Duration::minutes(9),
        -Duration::hours(7) - Duration::minutes(8),
        &DailyBandDurations {
            fixed_seconds: 450 * 60,
            elapsed_seconds: 0,
            repetitive_seconds: 855 * 60,
            non_repetitive_seconds: 71 * 60,
            rho_leeway_seconds: 24 * 60,
        },
        true,
    );
    let expected = format!(
        "2026-08-15(土) -07:08 +46:09 [{}{}{}{}{}]",
        "#".repeat(30),
        "=".repeat(57),
        "-".repeat(5),
        ":",
        ".".repeat(3),
    );

    assert_eq!(strip_ansi_escape_sequences(&actual), expected);
}

#[test]
fn test_calculate_daily_band_durations_経過した空き時間を当日だけ計上する() {
    let today = calculate_daily_band_durations(true, 990, 190, 60 * 60, 40 * 60, -1.0);
    let future = calculate_daily_band_durations(false, 990, 990, 60 * 60, 40 * 60, -1.0);

    assert_eq!(today.fixed_seconds, 450 * 60);
    assert_eq!(today.elapsed_seconds, 800 * 60);
    assert_eq!(today.repetitive_seconds, 40 * 60);
    assert_eq!(today.non_repetitive_seconds, 20 * 60);
    assert_eq!(today.rho_leeway_seconds, 60 * 60);
    assert_eq!(future.elapsed_seconds, 0);
}

#[test]
fn test_format_signed_hours_minutes_符号付きで時分を2桁ゼロ埋めする() {
    assert_eq!(format_signed_hours_minutes(Duration::zero()), "+00:00");
    assert_eq!(
        format_signed_hours_minutes(Duration::hours(6) + Duration::minutes(5)),
        "+06:05"
    );
    assert_eq!(
        format_signed_hours_minutes(-Duration::hours(6) - Duration::minutes(5)),
        "-06:05"
    );
}

#[test]
fn test_format_daily_band_当日経過と24時間超過を表示する() {
    let date = NaiveDate::from_ymd_opt(2026, 8, 11).unwrap();
    let actual = format_daily_band(
        date,
        "火",
        -Duration::hours(3) - Duration::minutes(4),
        Duration::hours(5) + Duration::minutes(6),
        &DailyBandDurations {
            fixed_seconds: 450 * 60,
            elapsed_seconds: 800 * 60,
            repetitive_seconds: 476 * 60,
            non_repetitive_seconds: 40 * 60,
            rho_leeway_seconds: 0,
        },
        true,
    );
    let expected = format!(
        "2026-08-11(火) +05:06 -03:04 [{}{}{}]{}",
        "#".repeat(30),
        "x".repeat(53),
        "=".repeat(13),
        ">".repeat(22),
    );

    assert_eq!(strip_ansi_escape_sequences(&actual), expected);
    assert!(actual.ends_with(&format!("\x1b[38;5;196m{}\x1b[39m", ">".repeat(22))));
}

#[test]
fn test_execute_band_日本語と英語で凡例と棒とサマリーを表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("帯出力固定用タスク").unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    task.set_pending_until(now);
    task.set_orig_status(Status::Pending);

    let japanese = execute_calendar_command_for_test("帯", now, task.clone(), 10 * 60);
    let english = execute_calendar_command_for_test("band", now, task, 10 * 60);
    let expected = format!(
        concat!(
            "凡例: # 固定  x 経過済み  = 繰返  - 単発  : 余差  . 空き  > 超過  (1文字=15分)\n",
            "\n",
            "2026-08-11(火) -06:00 -09:00 [{}{}{}{}]\n",
            "\n",
            "今のタスクが片付く日付: 4160日後の2037-12-31\n",
            "最大の累積時間: -09時間00分 (2026-08-11), 最大のrhoの差: -1.00 (1900-01-01), 次にタスクを積める日付: 0日後の2026-08-11 (-6時間00分)\n",
            "\n",
            "[Info] 順調です。突発タスクに対応したり1日の終わり際にタスクを新しく積んだりする余裕があります。ひとまずは脇道に逸れずに予定の遂行をしてください。\n",
            "\n",
            "残り拘束時間は0.0時間です\n",
            "完了見込み日時は1.0時間後の2026/08/11 13:00:00です\n",
            "rep ρ = (1.00 + 0.00) / (1.00 + 0.00 + 11 + 30/60) = 0.08, Lq = 0.1\n",
            "one ρ = (1.00 + 0.00) / (1.00 + 0.00 + 11 + 30/60) = 0.08, Lq = 0.1\n",
            "\n",
        ),
        "#".repeat(56),
        "-".repeat(4),
        ":".repeat(24),
        ".".repeat(12),
    );

    assert_eq!(strip_ansi_escape_sequences(&japanese), expected);
    assert_eq!(strip_ansi_escape_sequences(&english), expected);
    assert!(!japanese.contains("日          "));
    assert!(!japanese.contains("帯出力固定用タスク"));
}

#[test]
fn test_execute_band_当日終了時刻と翌日締切のアラートを表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let tomorrow = now + Duration::days(1);
    let root = new_test_task_handle("帯アラートfixture").unwrap();
    root.set_estimated_work_seconds(0);
    add_scheduled_child_for_test(&root, "今日の超過", now, 11 * 60);
    add_scheduled_child_for_test(&root, "明日の予定", tomorrow, 1);
    let tomorrow_task = add_scheduled_child_for_test(&root, "明日締切", now, 11 * 60);
    tomorrow_task.set_deadline_time_opt(Some(tomorrow));

    let actual = execute_calendar_command_for_test("帯", now, root, 10 * 60);

    assert!(actual.contains(
        "[Crit] 【今日の】終了予定時刻に間に合いません。【ただちに】どれかの予定を諦めて明日以降に延期してください。"
    ), "{actual}");
    assert!(actual.contains(
        "[Warn] 【明日の】〆切に間に合いません。〆切をあさって以降にリスケする調整を【今日中に】してください。"
    ), "{actual}");
}

#[test]
fn test_execute_band_凡例と帯を7色の_ansi前景色で表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("帯色出力固定用タスク").unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    task.set_pending_until(now);
    task.set_orig_status(Status::Pending);

    let actual = execute_calendar_command_for_test("帯", now, task, 10 * 60);
    let color = |value: u8, symbol: &str| format!("\x1b[38;5;{value}m{symbol}\x1b[39m");
    let expected = format!(
        concat!(
            "凡例: {} 固定  {} 経過済み  {} 繰返  {} 単発  {} 余差  {} 空き  {} 超過  (1文字=15分)\n",
            "\n",
            "2026-08-11(火) -06:00 -09:00 [{}{}{}{}]\n",
            "\n",
            "今のタスクが片付く日付: 4160日後の2037-12-31\n",
            "最大の累積時間: -09時間00分 (2026-08-11), 最大のrhoの差: -1.00 (1900-01-01), 次にタスクを積める日付: 0日後の2026-08-11 (-6時間00分)\n",
            "\n",
            "[Info] 順調です。突発タスクに対応したり1日の終わり際にタスクを新しく積んだりする余裕があります。ひとまずは脇道に逸れずに予定の遂行をしてください。\n",
            "\n",
            "残り拘束時間は0.0時間です\n",
            "完了見込み日時は1.0時間後の2026/08/11 13:00:00です\n",
            "rep ρ = (1.00 + 0.00) / (1.00 + 0.00 + 11 + 30/60) = 0.08, Lq = 0.1\n",
            "one ρ = (1.00 + 0.00) / (1.00 + 0.00 + 11 + 30/60) = 0.08, Lq = 0.1\n",
            "\n",
        ),
        color(110, "#"),
        color(244, "x"),
        color(33, "="),
        color(208, "-"),
        color(28, ":"),
        color(34, "."),
        color(196, ">"),
        color(110, &"#".repeat(56)),
        color(208, &"-".repeat(4)),
        color(28, &":".repeat(24)),
        color(34, &".".repeat(12)),
    );

    assert_eq!(actual, expected);
}

#[test]
fn test_execute_band_パイプ出力では_ansi前景色を含めない() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("帯パイプ出力固定用タスク").unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    task.set_pending_until(now);
    task.set_orig_status(Status::Pending);

    let actual = execute_calendar_command_with_ansi_color_for_test("帯", now, task, 10 * 60, false);

    assert!(!actual.contains("\x1b["));
    assert!(actual.contains("凡例: # 固定  x 経過済み"));
}

#[test]
fn test_execute_band_全日空き差分と繰り返し判定を帯へ反映する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let root = new_test_task_handle("帯データフローfixture").unwrap();
    root.set_estimated_work_seconds(0);
    let repetitive_group = root.create_as_last_child(new_test_task_attr("繰り返しグループ"));
    repetitive_group.set_estimated_work_seconds(0);
    repetitive_group.set_repetition_interval_days_opt(Some(7));
    add_scheduled_child_for_test(&repetitive_group, "繰り返しタスク", now, 40);

    let actual = execute_band_command_with_elapsed_for_test("帯", now, root);
    let expected_row = format!(
        "2026-08-11(火) -01:45 -02:30 [{}{}{}{}{}]",
        "#".repeat(30),
        "x".repeat(53),
        "=".repeat(3),
        ":".repeat(7),
        ".".repeat(3),
    );

    assert!(
        strip_ansi_escape_sequences(&actual).contains(&expected_row),
        "{actual}"
    );
}

#[test]
fn test_should_suppress_leaf_tasks_after_command_帯とbandでは葉を追加表示しない() {
    assert!(should_suppress_leaf_tasks_after_command("帯"));
    assert!(should_suppress_leaf_tasks_after_command("band"));
    assert!(!should_suppress_leaf_tasks_after_command("見"));
}

#[test]
fn test_execute_show_all_年なし日付は完全日付と同じ予定を表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let scheduled_start = Local.with_ymd_and_hms(2026, 9, 26, 6, 0, 0).unwrap();
    let task = new_test_task_handle("TARGET_DATE_TASK").unwrap();
    task.set_start_time(scheduled_start);
    task.set_pending_until(scheduled_start);
    task.set_orig_status(Status::Pending);

    let abbreviated = execute_show_all_command_for_test("全 9/26", now, task.clone());
    let full = execute_show_all_command_for_test("全 2026/09/26", now, task.clone());
    let other_date = execute_show_all_command_for_test("全 9/27", now, task);

    assert_eq!(abbreviated, full);
    assert!(abbreviated.contains("TARGET_DATE_TASK"));
    assert!(!other_date.contains("TARGET_DATE_TASK"));
}

#[test]
fn test_execute_show_all_過ぎた年なし日付は翌年の予定を表示する() {
    let now = Local.with_ymd_and_hms(2026, 10, 1, 12, 0, 0).unwrap();
    let scheduled_start = Local.with_ymd_and_hms(2027, 9, 26, 6, 0, 0).unwrap();
    let task = new_test_task_handle("TARGET_DATE_TASK").unwrap();
    task.set_start_time(scheduled_start);
    task.set_pending_until(scheduled_start);
    task.set_orig_status(Status::Pending);

    let abbreviated = execute_show_all_command_for_test("all 9/26", now, task.clone());
    let full = execute_show_all_command_for_test("all 2027/09/26", now, task);

    assert_eq!(abbreviated, full);
    assert!(abbreviated.contains("TARGET_DATE_TASK"));
}

#[test]
fn get_byte_offset_for_deletion_noneを返す場合() {
    let line = "あ";
    let cursor_x = 0;
    let actual = get_byte_offset_for_deletion(line, cursor_x);
    let expected = None;
    assert_eq!(actual, expected);
}

#[test]
fn get_byte_offset_for_deletion_正常系() {
    let line = "あ";
    let cursor_x = 1;
    let actual = get_byte_offset_for_deletion(line, cursor_x);
    let expected = Some(0);
    assert_eq!(actual, expected);
}

#[test]
fn test_report_run_result_load_errorを表示して失敗を返す() {
    let mut stderr = Vec::new();
    let error = TaskRepositoryError::new(
        TaskRepositoryOperation::Load,
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "ParseProject failed for /test/project.yaml: broken YAML",
        ),
    );

    let actual = report_run_result(&mut stderr, Err(RunError::Repository(error)));

    assert!(!actual);
    let output = String::from_utf8(stderr).unwrap();
    assert!(output.contains("[Error]"));
    assert!(output.contains("Load"));
    assert!(output.contains("/test/project.yaml"));
    assert!(output.contains("broken YAML"));
}

#[test]
fn test_report_run_result_input切断を表示して失敗を返す() {
    let mut stderr = Vec::new();

    let actual = report_run_result(
        &mut stderr,
        Err(RunError::InputDisconnected {
            save_error_opt: None,
        }),
    );

    assert!(!actual);
    let output = String::from_utf8(stderr).unwrap();
    assert!(output.contains("[Error]"));
    assert!(output.contains("interactive input channel disconnected"));
}

#[test]
fn test_report_run_result_ctrl_cを表示して失敗を返す() {
    let mut stderr = Vec::new();

    let actual = report_run_result(&mut stderr, Err(RunError::Interrupted));

    assert!(!actual);
    let output = String::from_utf8(stderr).unwrap();
    assert!(output.contains("[Error]"));
    assert!(output.contains("interactive input interrupted"));
}

#[test]
fn test_parse_non_interactive_command_引数なしは_none() {
    let actual = parse_non_interactive_command(vec![]);
    let expected = None;

    assert_eq!(actual, expected);
}

#[test]
fn test_parse_non_interactive_command_単一引数をコマンドにする() {
    let actual = parse_non_interactive_command(vec!["今".to_string()]);
    let expected = Some("今".to_string());

    assert_eq!(actual, expected);
}

#[test]
fn test_parse_non_interactive_command_複数引数を1コマンドにする() {
    let actual = parse_non_interactive_command(vec!["尾".to_string(), "週".to_string()]);
    let expected = Some("尾 週".to_string());

    assert_eq!(actual, expected);
}

#[test]
fn test_execute_non_interactive_command_project作成はoperation時刻を共有する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let previous_synced_time = Local.with_ymd_and_hms(2026, 8, 19, 9, 0, 0).unwrap();
    let operation_now = Local.with_ymd_and_hms(2026, 8, 20, 14, 30, 45).unwrap();
    let mut task_repository = TestTaskRepository::new(
        new_test_task_handle("既存project").unwrap(),
        previous_synced_time,
    )
    .with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager::default();

    execute_non_interactive_command_at(
        &mut task_repository,
        &mut free_time_manager,
        "新 snapshot_project 30",
        operation_now,
    )
    .unwrap();

    assert_eq!(task_repository.get_last_synced_time(), operation_now);
    assert_eq!(task_repository.reload_if_changed_attempt_count.get(), 1);
    assert_eq!(task_repository.task.get_name().unwrap(), "snapshot_project");
    assert_eq!(
        task_repository.task.get_create_time().unwrap(),
        operation_now
    );
    assert_eq!(
        task_repository.task.get_start_time().unwrap(),
        operation_now
    );
}

#[test]
fn test_execute_non_interactive_command_finishはoperation時刻を共有する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let previous_synced_time = Local.with_ymd_and_hms(2026, 8, 19, 9, 0, 0).unwrap();
    let operation_now = Local.with_ymd_and_hms(2026, 8, 20, 14, 30, 45).unwrap();
    let repetitive_parent = new_test_task_handle("反復project").unwrap();
    repetitive_parent
        .set_repetition_interval_days_opt(Some(7))
        .unwrap();
    let focused = repetitive_parent.create_as_last_child(new_test_task_attr("今回の反復task"));
    let focused_id = focused.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(repetitive_parent, previous_synced_time)
        .with_storage_directory(&storage_dir.path);
    task_repository.highest_priority_leaf_task_id_opt = Some(focused_id);
    let mut free_time_manager = TestFreeTimeManager::default();

    execute_non_interactive_command_at(
        &mut task_repository,
        &mut free_time_manager,
        "終",
        operation_now,
    )
    .unwrap();

    assert_eq!(task_repository.get_last_synced_time(), operation_now);
    assert_eq!(task_repository.reload_if_changed_attempt_count.get(), 1);
    let finished = task_repository
        .get_by_id(focused_id)
        .unwrap()
        .expect("完了対象のtaskはtreeに残るべきです");
    assert_eq!(finished.get_end_time_opt().unwrap(), Some(operation_now));
    let next_repetition = task_repository
        .task
        .get_children()
        .unwrap()
        .into_iter()
        .find(|task| task.get_id().unwrap() != focused_id)
        .expect("反復taskの完了時は次回taskを生成すべきです");
    assert_eq!(next_repetition.get_create_time().unwrap(), operation_now);
}

#[test]
fn test_execute_non_interactive_command_省略作業時間はoperation時刻を使う() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let previous_synced_time = Local.with_ymd_and_hms(2026, 8, 19, 9, 0, 0).unwrap();
    let operation_now = Local.with_ymd_and_hms(2026, 8, 20, 14, 30, 45).unwrap();
    let focused = new_test_task_handle("作業対象").unwrap();
    focused.set_actual_work_seconds(2 * 60).unwrap();
    let focused_id = focused.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(focused, previous_synced_time)
        .with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager::default();

    execute_non_interactive_command_at(
        &mut task_repository,
        &mut free_time_manager,
        "働",
        operation_now,
    )
    .unwrap();

    assert_eq!(task_repository.get_last_synced_time(), operation_now);
    assert_eq!(task_repository.reload_if_changed_attempt_count.get(), 1);
    assert_eq!(task_repository.save_attempt_count.get(), 1);
    assert_eq!(
        task_repository
            .get_by_id(focused_id)
            .unwrap()
            .expect("作業対象のtaskはtreeに残るべきです")
            .get_actual_work_seconds()
            .unwrap(),
        3 * 60
    );
}

#[test]
fn test_execute_non_interactive_command_load失敗時はcommandを実行しない() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("変更しないtask").unwrap();
    let task_id = task.get_id().unwrap();
    let original_estimated_work_seconds = task.get_estimated_work_seconds().unwrap();
    let mut task_repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    task_repository.load_should_fail = true;
    let mut free_time_manager = TestFreeTimeManager::default();

    let actual =
        execute_non_interactive_command(&mut task_repository, &mut free_time_manager, "予 45");

    assert!(matches!(
        actual,
        Err(RunError::Repository(ref error))
            if error.operation() == TaskRepositoryOperation::Load
    ));
    assert_eq!(
        task_repository
            .get_by_id(task_id)
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        original_estimated_work_seconds
    );
}

#[test]
fn test_execute_non_interactive_command_検証はsaveとfree_time読込を行わない() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("検証対象").unwrap();
    let mut task_repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager::default();

    execute_non_interactive_command(&mut task_repository, &mut free_time_manager, "検証").unwrap();

    assert_eq!(task_repository.save_attempt_count.get(), 0);
}

#[test]
fn test_execute_non_interactive_command_gatewayの変換errorをstderrへ表示する() {
    let storage_dir = TestStorageDir::new();
    let project_dir = storage_dir.path.join("broken-project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let project_yaml_path = project_dir.join("project.yaml");
    std::fs::write(
        &project_yaml_path,
        "project:\n  name: broken\n  children: not-an-array\n",
    )
    .unwrap();
    let mut task_repository = TaskRepository::new(storage_dir.path.to_str().unwrap());
    let mut free_time_manager = TestFreeTimeManager::default();

    let result =
        execute_non_interactive_command(&mut task_repository, &mut free_time_manager, "予 45");
    let mut stderr = Vec::new();
    let succeeded = report_run_result(&mut stderr, result);

    assert!(!succeeded);
    let output = String::from_utf8(stderr).unwrap();
    assert!(output.contains("repository Load failed"));
    assert!(output.contains(project_yaml_path.to_str().unwrap()));
    assert!(output.contains("project.children: must be an array or null"));
}

#[test]
fn test_execute_non_interactive_command_busy_time_slots読込失敗はstderrへ表示しrepository_transactionとcommandを実行しない(
) {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("変更しないtask").unwrap();
    let task_id = task.get_id().unwrap();
    let original_estimated_work_seconds = task.get_estimated_work_seconds().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerWithLoadError::default();
    let busy_time_slots_yaml_path = active_config().busy_time_slots_yaml_path.clone();

    let error =
        execute_non_interactive_command(&mut task_repository, &mut free_time_manager, "予 45")
            .expect_err("busy time slotsの読込失敗はRunErrorとして返るべきです");

    assert!(matches!(
        error,
        RunError::BusyTimeSlots(ref error)
            if error.to_string().contains(busy_time_slots_yaml_path.to_str().unwrap())
                && error.to_string().contains("$")
    ));
    assert_eq!(task_repository.load_attempt_count.get(), 0);
    assert_eq!(task_repository.reload_if_changed_attempt_count.get(), 0);
    assert_eq!(task_repository.save_attempt_count.get(), 0);
    assert_eq!(
        free_time_manager.loaded_path(),
        Some(busy_time_slots_yaml_path.clone())
    );
    assert_eq!(
        task_repository
            .get_by_id(task_id)
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        original_estimated_work_seconds
    );

    let mut stderr = Vec::new();
    let succeeded = report_run_result(&mut stderr, Err(error));

    assert!(!succeeded);
    let output = String::from_utf8(stderr).unwrap();
    assert!(output.contains("[Error]"));
    assert!(output.contains(busy_time_slots_yaml_path.to_str().unwrap()));
    assert!(output.contains("$"));
}

#[test]
fn test_interactive起動前のbusy_time_slots読込失敗はraw_modeなしでrun_errorとして返す() {
    let mut free_time_manager = TestFreeTimeManagerWithLoadError::default();
    let busy_time_slots_yaml_path = active_config().busy_time_slots_yaml_path.clone();

    let error = load_busy_time_slots_for_interactive_application(
        &mut free_time_manager,
        busy_time_slots_yaml_path.to_str().unwrap(),
    )
    .expect_err("対話起動前の設定読込失敗はRawModeを有効化せずRunErrorとして返すべきです");

    assert!(matches!(
        error,
        RunError::BusyTimeSlots(ref error)
            if error.path() == busy_time_slots_yaml_path.as_path()
                && error.field_path() == "$"
    ));
    assert_eq!(
        free_time_manager.loaded_path(),
        Some(busy_time_slots_yaml_path)
    );
}

#[test]
fn test_cli_repository初期load後はmcpがlockを取得できる() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let mut repository = TaskRepository::new(storage_dir.path.to_str().unwrap());

    let storage_lock = reload_repository_for_cli(&mut repository, now).unwrap();
    drop(storage_lock);

    let mcp_lock = StorageLock::acquire(&storage_dir.path, LockMode::Mcp);
    assert!(mcp_lock.is_ok());
}

#[test]
fn test_cli_repository_transactionは外部更新を再読込してcommandを即時保存する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let mut cli_repository = TaskRepository::new(storage_dir.path.to_str().unwrap());
    drop(reload_repository_for_cli(&mut cli_repository, now).unwrap());

    {
        let _mcp_lock = StorageLock::acquire(&storage_dir.path, LockMode::Mcp).unwrap();
        let mut mcp_repository = TaskRepository::new(storage_dir.path.to_str().unwrap());
        mcp_repository.sync_clock(now);
        mcp_repository.load().unwrap();
        mcp_repository
            .start_new_project(new_test_task_handle("MCP更新").unwrap())
            .unwrap();
        mcp_repository.save().unwrap();
    }

    run_cli_repository_transaction(&mut cli_repository, now, |repository| {
        repository
            .start_new_project(new_test_task_handle("CLI更新").unwrap())
            .unwrap();
        Ok(())
    })
    .unwrap();

    let _mcp_lock = StorageLock::acquire(&storage_dir.path, LockMode::Mcp).unwrap();
    let mut reloaded = TaskRepository::new(storage_dir.path.to_str().unwrap());
    reloaded.sync_clock(now);
    reloaded.load().unwrap();
    let names = reloaded
        .get_all_projects()
        .iter()
        .map(|task| task.get_name().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"MCP更新".to_string()));
    assert!(names.contains(&"CLI更新".to_string()));
}

#[test]
fn test_cli_repository_transactionはread_only_operationでsaveしない() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let mut repository = TestTaskRepository::new(new_test_task_handle("cache経路").unwrap(), now)
        .with_storage_directory(&storage_dir.path);
    repository.has_pending_changes.set(false);

    run_cli_repository_transaction(&mut repository, now, |_| Ok(())).unwrap();

    assert_eq!(repository.reload_if_changed_attempt_count.get(), 1);
    assert_eq!(repository.load_attempt_count.get(), 1);
    assert_eq!(repository.save_attempt_count.get(), 0);
}

#[test]
fn test_cli_repository_transactionはload失敗時にcommandもsaveも実行しない() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let mut repository = TestTaskRepository::new(new_test_task_handle("変更前").unwrap(), now)
        .with_storage_directory(&storage_dir.path);
    repository.load_should_fail = true;
    let command_executed = Cell::new(false);

    let result = run_cli_repository_transaction(&mut repository, now, |_| {
        command_executed.set(true);
        Ok(())
    });

    assert!(matches!(result, Err(RunError::Repository(_))));
    assert!(!command_executed.get());
    assert_eq!(repository.save_attempt_count.get(), 0);
    assert!(StorageLock::acquire(&storage_dir.path, LockMode::Mcp).is_ok());
}

#[test]
fn test_cli_repository_transactionはsave失敗をfatalなphase付きerrorにする() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("変更前").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    repository.save_failures_remaining.set(1);

    let result = run_cli_repository_transaction(&mut repository, now, |repository| {
        repository
            .get_by_id(task_id)
            .unwrap()
            .set_estimated_work_seconds(45 * 60);
        Ok(())
    });

    assert!(matches!(
        result,
        Err(RunError::CliRepositoryTransaction(
            CliRepositoryTransactionError::Save(_)
        ))
    ));
    assert_eq!(repository.save_attempt_count.get(), 1);
    assert_eq!(
        repository
            .get_by_id(task_id)
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        45 * 60
    );
    assert!(StorageLock::acquire(&storage_dir.path, LockMode::Mcp).is_ok());
}

#[test]
fn test_reload後にfocus中taskがdoneなら次候補を選び直す() {
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let root = new_test_task_handle("root").unwrap();
    let done = root.create_as_last_child(new_test_task_attr("完了済みfocus"));
    done.set_orig_status(Status::Done);
    let next = root.create_as_last_child(new_test_task_attr("次候補"));
    let mut repository = TestTaskRepository::new(root, now);
    repository.highest_priority_leaf_task_id_opt = Some(next.get_id().unwrap());
    let mut focused_task_id_opt = Some(done.get_id().unwrap());
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let changed = reconcile_focus_after_reload(
        &mut repository,
        &mut focused_task_id_opt,
        &mut focus_selection_mode,
    );

    assert!(changed.unwrap());
    assert_eq!(focused_task_id_opt, Some(next.get_id().unwrap()));
}

#[test]
fn test_低優先度modeで外したfocusは低優先度候補を再選択する() {
    let now = Local.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap();
    let root = new_test_task_handle("root").unwrap();
    let high_priority_task = root.create_as_last_child(new_test_task_attr("高優先度候補"));
    let low_priority_task = root.create_as_last_child(new_test_task_attr("低優先度候補"));
    let high_priority_task_id = high_priority_task.get_id().unwrap();
    let low_priority_task_id = low_priority_task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(root, now);
    repository.highest_priority_leaf_task_id_opt = Some(high_priority_task_id);
    repository.defer_candidate_leaf_task_id_opt = Some(low_priority_task_id);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(high_priority_task_id);
    let focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::LowestPriority { recent_days: 3 };

    execute_interactive_command(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &focus_started_datetime,
        &mut focus_selection_mode,
        now,
        "外",
    )
    .unwrap();

    assert_eq!(focused_task_id_opt, Some(low_priority_task_id));
    assert_eq!(
        focus_selection_mode,
        FocusSelectionMode::LowestPriority { recent_days: 3 }
    );
    assert_eq!(
        repository.last_defer_candidate_recent_threshold_opt,
        Some(Local.with_ymd_and_hms(2026, 8, 20, 6, 0, 0).unwrap())
    );
}

#[test]
fn test_interactive_task属性更新_不正deadlineはfield付きerrorを表示して状態を維持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 16, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();
    let previous_deadline = Local.with_ymd_and_hms(2026, 8, 20, 23, 59, 59).unwrap();
    task.set_deadline_time_opt(Some(previous_deadline));
    let mut repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    execute_interactive_command(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        &now,
        &mut focus_selection_mode,
        now,
        "〆 invalid",
    )
    .unwrap();

    let actual = repository.get_by_id(task_id).unwrap().unwrap();
    assert_eq!(
        actual.get_deadline_time_opt().unwrap(),
        Some(previous_deadline)
    );
    assert!(stdout
        .into_string()
        .contains("[Error] 入力エラー: deadline:"));
}

#[test]
fn test_interactive_submitは製品event経路でload実行保存する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Submit { line: " 予 45 " },
    );

    assert!(matches!(
        outcome,
        InteractiveRepositoryEventOutcome::CommandExecuted(ref command, _) if command == "予 45"
    ));
    assert_eq!(repository.load_attempt_count.get(), 1);
    assert_eq!(repository.save_attempt_count.get(), 1);
    assert_eq!(
        repository.operation_trace(),
        ["reload_if_changed", "load", "has_pending_changes", "save"]
    );
    assert_eq!(
        repository
            .get_by_id(task_id)
            .unwrap()
            .get_estimated_work_seconds()
            .unwrap(),
        45 * 60
    );
    assert!(StorageLock::acquire(&storage_dir.path, LockMode::Mcp).is_ok());
}

#[test]
fn test_interactive_verifyは出力errorを分類してtransactionを継続する() {
    let operation_now = Local.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap();

    for error_kind in [
        std::io::ErrorKind::BrokenPipe,
        std::io::ErrorKind::Other,
    ] {
        let storage_dir = TestStorageDir::new();
        std::fs::create_dir_all(&storage_dir.path).unwrap();
        let task = new_test_task_handle("検証対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(
            task,
            operation_now - chrono::Duration::hours(1),
        )
        .with_storage_directory(&storage_dir.path);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut stdout = FlushTrackingWriter::failing_on_nth_flush(2, error_kind);
        let mut focused_task_id_opt = Some(task_id);
        let mut last_focused_task_id_opt = Some(task_id);
        let mut focus_started_datetime = operation_now;
        let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

        let outcome = handle_interactive_submit_at(
            &mut stdout,
            &mut repository,
            &mut free_time_manager,
            InteractiveRepositoryState {
                focused_task_id_opt: &mut focused_task_id_opt,
                last_focused_task_id_opt: &mut last_focused_task_id_opt,
                focus_started_datetime: &mut focus_started_datetime,
                focus_selection_mode: &mut focus_selection_mode,
            },
            "検証",
            operation_now,
        );

        assert!(matches!(
            outcome,
            InteractiveRepositoryEventOutcome::CommandExecuted(ref command, now)
                if command == "検証" && now == operation_now
        ));
        assert_eq!(stdout.flush_count, 2);
        let output = String::from_utf8(stdout.buffer).unwrap();
        assert_eq!(
            output.contains("[Error] 出力エラー: flush failure"),
            error_kind == std::io::ErrorKind::Other
        );
        assert_eq!(repository.get_last_synced_time(), operation_now);
        assert_eq!(repository.save_attempt_count.get(), 1);
        assert_eq!(
            repository.operation_trace(),
            ["reload_if_changed", "load", "has_pending_changes", "save"]
        );
    }
}

#[test]
fn test_interactive_submitとnoninteractive実行は共通command_transaction経路を通る() {
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let mut traces = Vec::new();

    for is_interactive in [false, true] {
        let storage_dir = TestStorageDir::new();
        std::fs::create_dir_all(&storage_dir.path).unwrap();
        let task = new_test_task_handle("更新対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository =
            TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
        let mut free_time_manager = TestFreeTimeManager::default();

        if is_interactive {
            let mut stdout = TestWriter::new();
            let mut focused_task_id_opt = Some(task_id);
            let mut last_focused_task_id_opt = Some(task_id);
            let mut focus_started_datetime = now;
            let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

            let outcome = handle_interactive_submit_at(
                &mut stdout,
                &mut repository,
                &mut free_time_manager,
                InteractiveRepositoryState {
                    focused_task_id_opt: &mut focused_task_id_opt,
                    last_focused_task_id_opt: &mut last_focused_task_id_opt,
                    focus_started_datetime: &mut focus_started_datetime,
                    focus_selection_mode: &mut focus_selection_mode,
                },
                " estimate 45 ",
                now,
            );
            assert!(matches!(
                outcome,
                InteractiveRepositoryEventOutcome::CommandExecuted(ref command, operation_now)
                    if command == "estimate 45" && operation_now == now
            ));
        } else {
            execute_non_interactive_command_at(
                &mut repository,
                &mut free_time_manager,
                "estimate 45",
                now,
            )
            .unwrap();
        }

        assert_eq!(
            repository
                .get_by_id(task_id)
                .unwrap()
                .unwrap()
                .get_estimated_work_seconds()
                .unwrap(),
            45 * 60
        );
        traces.push(repository.operation_trace());
    }

    assert_eq!(traces[0], traces[1]);
    assert_eq!(
        traces[0],
        ["reload_if_changed", "load", "has_pending_changes", "save"]
    );
}

#[test]
fn test_interactive_submitはoperation時刻をcommandと直後renderへ共有する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let previous_synced_time = Local.with_ymd_and_hms(2026, 8, 19, 9, 0, 0).unwrap();
    let operation_now = Local.with_ymd_and_hms(2026, 8, 20, 14, 30, 45).unwrap();
    let existing = new_test_task_handle("既存project").unwrap();
    let existing_id = existing.get_id().unwrap();
    let mut repository = TestTaskRepository::new(existing, previous_synced_time)
        .with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(existing_id);
    let mut last_focused_task_id_opt = Some(existing_id);
    let mut focus_started_datetime = previous_synced_time;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_submit_at(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        " 新 interactive_snapshot 30 ",
        operation_now,
    );
    let render_now = match outcome {
        InteractiveRepositoryEventOutcome::CommandExecuted(command, now) => {
            assert_eq!(command, "新 interactive_snapshot 30");
            now
        }
        _ => panic!("固定時刻のSubmitはcommand実行に成功すべきです"),
    };

    assert_eq!(render_now, operation_now);
    assert_eq!(repository.get_last_synced_time(), operation_now);
    assert_eq!(repository.reload_if_changed_attempt_count.get(), 1);
    assert_eq!(repository.save_attempt_count.get(), 1);
    assert_eq!(repository.task.get_name().unwrap(), "interactive_snapshot");
    assert_eq!(repository.task.get_create_time().unwrap(), operation_now);
    assert_eq!(repository.task.get_start_time().unwrap(), operation_now);
    let output = strip_ansi_escape_sequences(&stdout.into_string());
    assert!(
        output.contains("2026/08/20 14:30:45.000000000> 新 interactive_snapshot 30"),
        "{output}"
    );

    let mut render_stdout = TestWriter::new();
    render_focused_task(
        &mut render_stdout,
        &repository,
        focused_task_id_opt,
        &mut last_focused_task_id_opt,
        &mut focus_started_datetime,
        render_now,
    );

    assert_eq!(last_focused_task_id_opt, focused_task_id_opt);
    assert_eq!(focus_started_datetime, operation_now);
}

#[test]
fn test_interactive_submitの見は完了済みtaskへの明示focusを更新後も保持する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let root = new_test_task_handle("root").unwrap();
    let done = root.create_as_last_child(new_test_task_attr("完了済みtask"));
    done.set_orig_status(Status::Done);
    let next = root.create_as_last_child(new_test_task_attr("次候補"));
    let done_id = done.get_id().unwrap();
    let next_id = next.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(root, now).with_storage_directory(&storage_dir.path);
    repository.highest_priority_leaf_task_id_opt = Some(next_id);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(next_id);
    let mut last_focused_task_id_opt = Some(next_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;
    let command = format!("見 {done_id}");

    let submit_outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Submit { line: &command },
    );

    assert!(matches!(
        submit_outcome,
        InteractiveRepositoryEventOutcome::CommandExecuted(..)
    ));
    assert_eq!(focused_task_id_opt, Some(done_id));

    let refresh_outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Refresh,
    );

    assert!(matches!(
        refresh_outcome,
        InteractiveRepositoryEventOutcome::Continue
    ));
    assert_eq!(focused_task_id_opt, Some(done_id));
}

#[test]
fn test_interactive_submitは外部完了によるfocus切替時に開始時刻を更新する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let old_focus_started_datetime = Local.with_ymd_and_hms(2020, 8, 12, 12, 0, 0).unwrap();
    let root = new_test_task_handle("root").unwrap();
    let done = root.create_as_last_child(new_test_task_attr("外部で完了したfocus"));
    done.set_orig_status(Status::Done);
    let next = root.create_as_last_child(new_test_task_attr("次候補"));
    let done_id = done.get_id().unwrap();
    let next_id = next.get_id().unwrap();
    let mut repository = TestTaskRepository::new(root, old_focus_started_datetime)
        .with_storage_directory(&storage_dir.path);
    repository.highest_priority_leaf_task_id_opt = Some(next_id);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(done_id);
    let mut last_focused_task_id_opt = Some(done_id);
    let mut focus_started_datetime = old_focus_started_datetime;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Submit { line: "" },
    );

    assert!(matches!(
        outcome,
        InteractiveRepositoryEventOutcome::CommandExecuted(..)
    ));
    assert_eq!(focused_task_id_opt, Some(next_id));
    assert!(focus_started_datetime > old_focus_started_datetime);
}

#[test]
fn test_interactive_refreshとctrl_dは外部完了によるfocus切替時に開始時刻を更新する() {
    for event in [
        InteractiveRepositoryEvent::Refresh,
        InteractiveRepositoryEvent::Exit,
    ] {
        let storage_dir = TestStorageDir::new();
        std::fs::create_dir_all(&storage_dir.path).unwrap();
        let old_focus_started_datetime = Local.with_ymd_and_hms(2020, 8, 12, 12, 0, 0).unwrap();
        let root = new_test_task_handle("root").unwrap();
        let done = root.create_as_last_child(new_test_task_attr("外部で完了したfocus"));
        done.set_orig_status(Status::Done);
        let next = root.create_as_last_child(new_test_task_attr("次候補"));
        let done_id = done.get_id().unwrap();
        let next_id = next.get_id().unwrap();
        let mut repository = TestTaskRepository::new(root, old_focus_started_datetime)
            .with_storage_directory(&storage_dir.path);
        repository.highest_priority_leaf_task_id_opt = Some(next_id);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut stdout = TestWriter::new();
        let mut focused_task_id_opt = Some(done_id);
        let mut last_focused_task_id_opt = Some(done_id);
        let mut focus_started_datetime = old_focus_started_datetime;
        let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

        let outcome = handle_interactive_repository_event(
            &mut stdout,
            &mut repository,
            &mut free_time_manager,
            InteractiveRepositoryState {
                focused_task_id_opt: &mut focused_task_id_opt,
                last_focused_task_id_opt: &mut last_focused_task_id_opt,
                focus_started_datetime: &mut focus_started_datetime,
                focus_selection_mode: &mut focus_selection_mode,
            },
            event,
        );

        assert!(matches!(
            outcome,
            InteractiveRepositoryEventOutcome::Continue | InteractiveRepositoryEventOutcome::Exit
        ));
        assert_eq!(focused_task_id_opt, Some(next_id));
        assert_eq!(last_focused_task_id_opt, None);
        assert!(focus_started_datetime > old_focus_started_datetime);
    }
}

#[test]
fn test_interactive_commandによるfocus切替は次のrender時刻を開始時刻にする() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let old_focus_started_datetime = Local.with_ymd_and_hms(2020, 8, 12, 12, 0, 0).unwrap();
    let first_render_datetime = Local.with_ymd_and_hms(2026, 8, 12, 13, 0, 0).unwrap();
    let second_render_datetime = Local.with_ymd_and_hms(2026, 8, 12, 14, 0, 0).unwrap();
    let task = new_test_task_handle("focus対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(task, old_focus_started_datetime)
        .with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = old_focus_started_datetime;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Submit { line: "高" },
    );
    render_focused_task(
        &mut stdout,
        &repository,
        focused_task_id_opt,
        &mut last_focused_task_id_opt,
        &mut focus_started_datetime,
        first_render_datetime,
    );
    render_focused_task(
        &mut stdout,
        &repository,
        focused_task_id_opt,
        &mut last_focused_task_id_opt,
        &mut focus_started_datetime,
        second_render_datetime,
    );

    assert!(matches!(
        outcome,
        InteractiveRepositoryEventOutcome::CommandExecuted(..)
    ));
    assert_eq!(focused_task_id_opt, Some(task_id));
    assert_eq!(last_focused_task_id_opt, Some(task_id));
    assert_eq!(focus_started_datetime, first_render_datetime);
}

#[test]
fn test_interactive_submitはload失敗ならretryしsave失敗ならfatalにする() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();

    for (load_should_fail, save_failures, expected_fatal) in [(true, 0, false), (false, 1, true)] {
        let task = new_test_task_handle("更新対象").unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository =
            TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
        repository.load_should_fail = load_should_fail;
        repository.save_failures_remaining.set(save_failures);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut stdout = TestWriter::new();
        let mut focused_task_id_opt = Some(task_id);
        let mut last_focused_task_id_opt = Some(task_id);
        let mut focus_started_datetime = now;
        let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

        let outcome = handle_interactive_repository_event(
            &mut stdout,
            &mut repository,
            &mut free_time_manager,
            InteractiveRepositoryState {
                focused_task_id_opt: &mut focused_task_id_opt,
                last_focused_task_id_opt: &mut last_focused_task_id_opt,
                focus_started_datetime: &mut focus_started_datetime,
                focus_selection_mode: &mut focus_selection_mode,
            },
            InteractiveRepositoryEvent::Submit { line: "予 45" },
        );

        assert_eq!(
            matches!(outcome, InteractiveRepositoryEventOutcome::Fatal(_)),
            expected_fatal
        );
        assert_eq!(
            matches!(outcome, InteractiveRepositoryEventOutcome::Retry(_)),
            !expected_fatal
        );
        assert_eq!(
            repository.save_attempt_count.get(),
            usize::from(!load_should_fail)
        );
        assert_eq!(
            repository.operation_trace(),
            if load_should_fail {
                vec!["reload_if_changed", "load"]
            } else {
                vec!["reload_if_changed", "load", "has_pending_changes", "save"]
            }
        );
    }
}

#[test]
fn test_interactive_refreshは再読込後にlockを解放する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("表示対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Refresh,
    );

    assert!(matches!(
        outcome,
        InteractiveRepositoryEventOutcome::Continue
    ));
    assert_eq!(repository.load_attempt_count.get(), 1);
    assert_eq!(repository.reload_if_changed_attempt_count.get(), 1);
    assert_eq!(repository.save_attempt_count.get(), 0);
    assert_eq!(
        repository.operation_trace(),
        ["reload_if_changed", "load"]
    );
    assert!(StorageLock::acquire(&storage_dir.path, LockMode::Mcp).is_ok());
}

#[test]
fn test_interactive_ctrl_cは成功済みcommandを再保存せずfatal終了する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let submitted = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Submit { line: "予 45" },
    );
    let interrupted = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Interrupted,
    );

    assert!(matches!(
        submitted,
        InteractiveRepositoryEventOutcome::CommandExecuted(..)
    ));
    assert!(matches!(
        interrupted,
        InteractiveRepositoryEventOutcome::Fatal(RunError::Interrupted)
    ));
    assert_eq!(repository.save_attempt_count.get(), 1);
}

#[test]
fn test_interactive_input切断はreload後に保存してfatal終了する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("保存対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::InputDisconnected,
    );

    assert!(matches!(
        outcome,
        InteractiveRepositoryEventOutcome::Fatal(_)
    ));
    assert_eq!(repository.load_attempt_count.get(), 1);
    assert_eq!(repository.save_attempt_count.get(), 1);
    assert!(StorageLock::acquire(&storage_dir.path, LockMode::Mcp).is_ok());
}

#[test]
fn test_interactive_ctrl_dは製品event経路でreload後に保存して終了する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("保存対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::Exit,
    );

    assert!(matches!(outcome, InteractiveRepositoryEventOutcome::Exit));
    assert_eq!(repository.load_attempt_count.get(), 1);
    assert_eq!(repository.save_attempt_count.get(), 1);
    assert!(StorageLock::acquire(&storage_dir.path, LockMode::Mcp).is_ok());
}

#[test]
fn test_interactive_input読込errorは製品event経路でreload後に保存してfatal終了する() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("保存対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::HighestPriority;

    let outcome = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
            focus_selection_mode: &mut focus_selection_mode,
        },
        InteractiveRepositoryEvent::InputRead(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "stdin read failure",
        )),
    );

    assert!(matches!(
        outcome,
        InteractiveRepositoryEventOutcome::Fatal(RunError::InputRead {
            input_error,
            save_error_opt: None,
        }) if input_error.kind() == std::io::ErrorKind::BrokenPipe
    ));
    assert_eq!(repository.load_attempt_count.get(), 1);
    assert_eq!(repository.save_attempt_count.get(), 1);
    assert!(StorageLock::acquire(&storage_dir.path, LockMode::Mcp).is_ok());
}

#[test]
fn test_format_focus_progress_100パーセントで全区画を塗る() {
    let actual = format_focus_progress(60 * 60, 59 * 60, 60);

    assert_eq!(actual, format!("[{}] 100%", "█".repeat(100)));
}

#[test]
fn test_format_focus_progress_101パーセントで超過記号を表示する() {
    let actual = format_focus_progress(100, 101, 0);

    assert_eq!(actual, format!("[{}]> 101%", "█".repeat(100)));
}

#[test]
fn test_format_focus_progress_114パーセントで超過分の記号を表示する() {
    let actual = format_focus_progress(100, 114, 0);

    assert_eq!(
        actual,
        format!("[{}]{} 114%", "█".repeat(100), ">".repeat(14))
    );
}

#[test]
fn test_format_focus_progress_開始直後は実経過0秒として扱う() {
    let actual = format_focus_progress(100 * 60, 0, 0);

    assert_eq!(actual, format!("[{}] 0%", "░".repeat(100)));
}

#[test]
fn test_format_focus_progress_見積と作業時間を秒数基準で計算する() {
    let actual = format_focus_progress(4 * 60 + 33, 0, 2 * 60);

    assert_eq!(
        actual,
        format!("[{}{}] 43%", "█".repeat(43), "░".repeat(57))
    );
}

#[test]
fn test_format_focus_progress_表示が2分でも実経過秒数を使う() {
    let actual = format_focus_progress(4 * 60 + 33, 0, 60);

    assert_eq!(
        actual,
        format!("[{}{}] 21%", "█".repeat(21), "░".repeat(79))
    );
}

#[test]
fn test_format_focus_progress_99パーセントでは1区画を未達として残す() {
    let actual = format_focus_progress(100, 99, 0);

    assert_eq!(actual, format!("[{}░] 99%", "█".repeat(99)));
}

#[test]
fn test_make_messages_about_focus_既存実績と表示中の作業時間から進捗を表示する() {
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 7, 25, 12, 0, 0).unwrap();
    let now = Local.with_ymd_and_hms(2026, 7, 25, 12, 19, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_actual_work_seconds(10 * 60);

    let actual = make_messages_about_focus(&task, &focus_started_datetime, &now).unwrap();

    assert!(actual[0].ends_with("focusing for 20 minutes"));
    assert_eq!(
        actual[1],
        format!("[{}{}] 48%", "█".repeat(48), "░".repeat(52))
    );
}

#[test]
fn test_make_messages_about_focus_バーを1パーセント単位で表示する() {
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 7, 25, 12, 0, 0).unwrap();
    let now = Local.with_ymd_and_hms(2026, 7, 25, 12, 19, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    task.set_estimated_work_seconds(100 * 60);
    task.set_actual_work_seconds(39 * 60);

    let actual = make_messages_about_focus(&task, &focus_started_datetime, &now).unwrap();

    assert!(actual[0].ends_with("focusing for 20 minutes"));
    assert_eq!(
        actual[1],
        format!("[{}{}] 58%", "█".repeat(58), "░".repeat(42))
    );
}

#[test]
fn test_make_messages_about_focus_見積時間超過時はバーだけ100パーセントを上限にする() {
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 7, 25, 12, 0, 0).unwrap();
    let now = Local.with_ymd_and_hms(2026, 7, 25, 12, 59, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    task.set_estimated_work_seconds(100 * 60);
    task.set_actual_work_seconds(57 * 60);

    let actual = make_messages_about_focus(&task, &focus_started_datetime, &now).unwrap();

    assert!(actual[0].ends_with("focusing for 60 minutes"));
    assert_eq!(
        actual[1],
        format!("[{}]{} 116%", "█".repeat(100), ">".repeat(16))
    );
}

#[test]
fn test_make_messages_about_focus_見積時間が0なら進捗を未算定として表示する() {
    let focus_started_datetime = Local.with_ymd_and_hms(2026, 7, 25, 12, 0, 0).unwrap();
    let now = Local.with_ymd_and_hms(2026, 7, 25, 12, 19, 0).unwrap();
    let task = new_test_task_handle("タスク").unwrap();
    task.set_estimated_work_seconds(0);
    task.set_actual_work_seconds(10 * 60);

    let actual = make_messages_about_focus(&task, &focus_started_datetime, &now).unwrap();

    assert!(actual[0].ends_with("focusing for 20 minutes"));
    assert_eq!(actual[1], format!("[{}] --%", "-".repeat(100)));
}

#[test]
fn test_render_interactive_screen_起動時と自動更新時の既定表示は帯() {
    let now = Local.with_ymd_and_hms(2026, 8, 12, 12, 0, 0).unwrap();
    let task = new_test_task_handle("対話画面表示対象").unwrap();
    let task_id = task.get_id().unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    task.set_pending_until(now);
    task.set_orig_status(Status::Pending);
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerForBand;
    let mut stdout = TestWriter::new();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = None;
    let mut focus_started_datetime = now;

    render_interactive_screen(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        FocusRenderState {
            focused_task_id_opt: &mut focused_task_id_opt,
            last_focused_task_id_opt: &mut last_focused_task_id_opt,
            focus_started_datetime: &mut focus_started_datetime,
        },
        now,
    );

    let output = stdout.into_string();
    let band_line = output
        .lines()
        .find(|line| line.starts_with("2026-08-12(水) "))
        .expect("起動時と自動更新時には日次帯を表示する");
    let band = band_line
        .split_once('[')
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(band, _)| band)
        .expect("日次帯は角括弧内に表示する");
    assert_eq!(
        strip_ansi_escape_sequences(band).chars().count(),
        DAILY_BAND_SEGMENTS
    );
    assert!(!output.contains("日          \t空          \t空差"));
}

#[test]
fn test_idle_refresh_deadline_現在時刻の60秒後を返す() {
    let now = Instant::now();

    assert_eq!(
        idle_refresh_deadline(now).duration_since(now),
        StdDuration::from_secs(60)
    );
}

#[test]
fn test_idle_wait_duration_期限までの残り時間を返す() {
    let now = Instant::now();
    let deadline = now + StdDuration::from_secs(15);

    assert_eq!(
        idle_wait_duration(deadline, now + StdDuration::from_secs(5)),
        StdDuration::from_secs(10)
    );
}

#[test]
fn test_idle_wait_duration_期限を過ぎていれば0秒を返す() {
    let now = Instant::now();
    let deadline = now + StdDuration::from_secs(15);

    assert_eq!(
        idle_wait_duration(deadline, now + StdDuration::from_secs(16)),
        StdDuration::ZERO
    );
}

#[test]
fn test_try_save_before_exit_保存成功なら終了可能にする() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task_repository = TestTaskRepository::new(new_test_task_handle("保存対象").unwrap(), now);
    let mut stdout = TestWriter::new();

    let actual = try_save_before_exit(&mut stdout, &task_repository);

    assert!(actual);
    assert_eq!(stdout.into_string(), "");
}

#[test]
fn test_try_save_before_exit_保存失敗ならerrorを表示して終了を止める() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("memoryに残すtask").unwrap();
    let task_id = task.get_id().unwrap();
    let task_repository = TestTaskRepository::new(task, now);
    task_repository.save_failures_remaining.set(1);
    let mut stdout = TestWriter::new();

    let actual = try_save_before_exit(&mut stdout, &task_repository);

    assert!(!actual);
    assert_eq!(
        task_repository
            .get_by_id(task_id)
            .unwrap()
            .get_name()
            .unwrap(),
        "memoryに残すtask"
    );
    let output = stdout.into_string();
    assert!(output.contains("[Error]"));
    assert!(output.contains("WriteFile"));
    assert!(output.contains("/test/project.yaml"));
    assert!(output.contains("test save failure"));
}

#[test]
fn test_handle_input_disconnected_保存を1回試して入力異常を返す() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for save_failures in [0, 1] {
        let task_repository =
            TestTaskRepository::new(new_test_task_handle("保存対象").unwrap(), now);
        task_repository.save_failures_remaining.set(save_failures);

        let actual = handle_input_disconnected(&task_repository);

        assert!(matches!(
            &actual,
            RunError::InputDisconnected {
                save_error_opt
            } if save_error_opt.is_some() == (save_failures == 1)
        ));
        assert_eq!(task_repository.save_attempt_count.get(), 1);
        let message = actual.to_string();
        assert!(message.contains("interactive input channel disconnected"));
        if save_failures == 1 {
            assert!(message.contains("repository Save failed"));
            assert!(message.contains("test save failure"));
        }
    }
}

#[test]
fn test_handle_input_read_error_保存を1回試して両方のerrorを保持する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

    for save_failures in [0, 1] {
        let task_repository =
            TestTaskRepository::new(new_test_task_handle("保存対象").unwrap(), now);
        task_repository.save_failures_remaining.set(save_failures);

        let actual = handle_input_read_error(
            &task_repository,
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdin read failure"),
        );

        assert!(matches!(
            &actual,
            RunError::InputRead {
                input_error,
                save_error_opt,
            } if input_error.kind() == std::io::ErrorKind::BrokenPipe
                && save_error_opt.is_some() == (save_failures == 1)
        ));
        assert_eq!(task_repository.save_attempt_count.get(), 1);
        let message = actual.to_string();
        assert!(message.contains("stdin read failure"));
        if save_failures == 1 {
            assert!(message.contains("repository Save failed"));
            assert!(message.contains("test save failure"));
        }
    }
}

#[test]
fn test_try_exit_interactive_保存失敗後の再試行で成功する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("再試行中もmemoryに残すtask").unwrap();
    let task_id = task.get_id().unwrap();
    let mut task_repository = TestTaskRepository::new(task, now);
    task_repository.save_failures_remaining.set(1);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();
    let mut exited = false;

    for _attempt in 0..2 {
        if try_exit_interactive(
            &mut stdout,
            &mut task_repository,
            &mut free_time_manager,
            &mut focused_task_id_opt,
            now,
        ) {
            exited = true;
            break;
        }
    }

    assert!(exited);
    assert_eq!(task_repository.save_attempt_count.get(), 2);
    assert_eq!(
        task_repository
            .get_by_id(task_id)
            .unwrap()
            .get_name()
            .unwrap(),
        "再試行中もmemoryに残すtask"
    );
    let output = stdout.into_string();
    assert_eq!(output.matches("[Error]").count(), 1);
}

#[test]
fn test_try_exit_interactive_ctrl_d終了時は帯を表示する() {
    let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();
    let task = new_test_task_handle("Ctrl-D終了表示対象").unwrap();
    let task_id = task.get_id().unwrap();
    task.set_estimated_work_seconds(60 * 60);
    task.set_start_time(now);
    task.set_pending_until(now);
    task.set_orig_status(Status::Pending);
    let mut task_repository = TestTaskRepository::new(task, now);
    let mut free_time_manager = TestFreeTimeManagerForBand;
    let mut focused_task_id_opt = Some(task_id);
    let mut stdout = TestWriter::new();

    let exited = try_exit_interactive(
        &mut stdout,
        &mut task_repository,
        &mut free_time_manager,
        &mut focused_task_id_opt,
        now,
    );

    assert!(exited);
    let output = stdout.into_string();
    assert!(strip_ansi_escape_sequences(&output).contains(
        "凡例: # 固定  x 経過済み  = 繰返  - 単発  : 余差  . 空き  > 超過  (1文字=15分)"
    ));
    assert!(!output.contains("日          \t空          \t空差"));
}
