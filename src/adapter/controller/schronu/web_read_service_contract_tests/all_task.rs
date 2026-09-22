use super::*;
#[test]
fn 全件pageはcliの予定segment順を500行境界でも維持する() {
    let now = Local.with_ymd_and_hms(2026, 9, 5, 8, 0, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
    repository.sync_clock(now).unwrap();
    repository.load().unwrap();
    for index in 1..=501 {
        let name = format!("project {index}");
        let root = TaskHandle::with_identity(&name, Uuid::from_u128(10_000 + index), now).unwrap();
        if index == 1 {
            root.set_orig_status(Status::Pending).unwrap();
        }
        if index == 2 {
            root.create_as_last_child(crate::test_support::new_task_attr_at("child", now));
        }
        if index == 501 {
            root.set_orig_status(Status::Done).unwrap();
        }
        repository.start_new_project(root).unwrap();
    }
    repository.save().unwrap();
    let expected_names = get_schedule(&repository)
        .unwrap()
        .into_iter()
        .map(|segment| segment.task.name)
        .collect::<Vec<_>>();
    assert!(expected_names.len() > 500);
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());
    let mut cursor = None;
    let mut actual_names = Vec::new();
    let mut page_count = 0;
    let mut action_flags = Vec::new();
    loop {
        let page = service.list_all_tasks_page_at(now, cursor).unwrap();
        assert!(page.rows.len() <= 500);
        if page.next_cursor.is_some() {
            assert_eq!(page.rows.len(), 500);
        }
        action_flags.extend(
            page.rows
                .iter()
                .filter(|row| {
                    matches!(
                        row.task.task_name.as_str(),
                        "project 1" | "project 2" | "child"
                    )
                })
                .map(|row| (row.task.task_name.clone(), row.can_start_session)),
        );
        actual_names.extend(page.rows.into_iter().map(|row| row.task.task_name));
        page_count += 1;
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert!(page_count > 1);
    assert_eq!(actual_names, expected_names);
    assert!(!actual_names.iter().any(|name| name == "project 501"));
    assert!(action_flags.contains(&("project 1".to_owned(), false)));
    assert!(action_flags.contains(&("project 2".to_owned(), false)));
    assert!(action_flags.contains(&("child".to_owned(), true)));
    assert_ne!(
        actual_names,
        expected_names.into_iter().rev().collect::<Vec<_>>()
    );

    let first_a = service.list_all_tasks_page_at(now, None).unwrap();
    let first_b = service.list_all_tasks_page_at(now, None).unwrap();
    assert!(service
        .list_all_tasks_page_at(now, Some("invalid cursor".to_owned()))
        .is_err());
    let second_a = service
        .list_all_tasks_page_at(now, first_a.next_cursor)
        .unwrap();
    let second_b = service
        .list_all_tasks_page_at(now, first_b.next_cursor)
        .unwrap();
    assert_eq!(second_a.rows, second_b.rows);
}

#[test]
fn 全件pageは同じtaskの分割segmentを別行にし完了taskを除く() {
    let now = Local.with_ymd_and_hms(2026, 9, 5, 20, 0, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
    repository.sync_clock(now).unwrap();
    repository.load().unwrap();
    let split_id = Uuid::from_u128(30_001);
    let split = TaskHandle::with_identity("split", split_id, now).unwrap();
    split.set_start_time(now + Duration::hours(1)).unwrap();
    split.set_estimated_work_seconds(10 * 3600).unwrap();
    split.set_priority(88).unwrap();
    repository.start_new_project(split).unwrap();
    let interrupt = TaskHandle::with_identity("interrupt", Uuid::from_u128(30_002), now).unwrap();
    interrupt.set_start_time(now + Duration::hours(9)).unwrap();
    interrupt.set_estimated_work_seconds(3600).unwrap();
    interrupt.set_priority(89).unwrap();
    repository.start_new_project(interrupt).unwrap();
    let done = TaskHandle::with_identity("done", Uuid::from_u128(30_003), now).unwrap();
    done.set_orig_status(Status::Done).unwrap();
    repository.start_new_project(done).unwrap();
    repository.save().unwrap();

    let schedule = get_schedule(&repository).unwrap();
    let expected_ids = schedule
        .iter()
        .map(|segment| segment.task.id.hyphenated().to_string())
        .collect::<Vec<_>>();
    let mut service = WebService::new(fixture.storage.clone(), fixture.config());
    let page = service.list_all_tasks_page_at(now, None).unwrap();
    let actual_ids = page
        .rows
        .iter()
        .map(|row| row.task.task_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(actual_ids, expected_ids);
    assert_eq!(
        actual_ids
            .iter()
            .filter(|id| *id == &split_id.to_string())
            .count(),
        2
    );
    assert!(!page.rows.iter().any(|row| row.task.task_name == "done"));
    assert_eq!(
        page.rows
            .iter()
            .map(|row| row.segment_index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(
        page.rows
            .iter()
            .map(|row| row.schedule_date.as_str())
            .collect::<Vec<_>>(),
        vec!["2026-09-05", "2026-09-05", "2026-09-06"]
    );
}

#[test]
#[ignore = "manual large-fixture performance measurement"]
fn 全件pageの大規模fixture取得時間と応答量を計測する() {
    const PROJECTS: u128 = 2_234;
    const CHILDREN_PER_PROJECT: u128 = 11;
    let now = Local.with_ymd_and_hms(2026, 9, 5, 8, 0, 0).unwrap();
    let fixture = WebReadServiceFixture::new();
    let mut repository = TaskRepository::new(fixture.storage.to_str().unwrap());
    repository.sync_clock(now).unwrap();
    repository.load().unwrap();
    for project_index in 0..PROJECTS {
        let root = TaskHandle::with_identity(
            &format!("project {project_index}"),
            Uuid::from_u128(100_000 + project_index),
            now,
        )
        .unwrap();
        for child_index in 0..CHILDREN_PER_PROJECT {
            let index = project_index * CHILDREN_PER_PROJECT + child_index;
            let child = root
                .create_child(TaskAttr::with_identity(
                    &format!("task {index}"),
                    Uuid::from_u128(1_000_000 + index),
                    now,
                ))
                .unwrap();
            child.set_estimated_work_seconds(300).unwrap();
            if index >= 700 {
                child.set_orig_status(Status::Pending).unwrap();
            }
        }
        repository.start_new_project(root).unwrap();
    }
    repository.save().unwrap();
    let expected_segments = get_schedule(&repository).unwrap().len();
    drop(repository);

    let mut service = WebService::new(fixture.storage.clone(), fixture.config());
    let started = Instant::now();
    let mut cursor = None;
    let mut rows = 0;
    let mut pages = 0;
    let mut response_bytes = 0;
    let mut first_page_ms = 0;
    loop {
        let page = service.list_all_tasks_page_at(now, cursor).unwrap();
        if pages == 0 {
            first_page_ms = started.elapsed().as_millis();
        }
        response_bytes += serde_json::to_vec(&page).unwrap().len();
        rows += page.rows.len();
        pages += 1;
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    eprintln!(
        "all-task fixture projects={PROJECTS} tasks={} expected_segments={expected_segments} pages={pages} rows={rows} first_page_ms={first_page_ms} total_ms={} response_bytes={response_bytes}",
        PROJECTS * (CHILDREN_PER_PROJECT + 1),
        started.elapsed().as_millis()
    );
    assert_eq!(rows, expected_segments);
}
