fn storage_data_files(path: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut files = std::fs::read_dir(path)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_file()))
        .filter(|entry| !matches!(entry.file_name().to_str(), Some(".revision" | ".lock")))
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

#[test]
fn high_and_low_selection_modes_record_cli_auto_switch() {
    for command in ["高", "低 3"] {
        let storage_dir = TestStorageDir::new();
        std::fs::create_dir_all(&storage_dir.path).unwrap();
        let started_at = Local.with_ymd_and_hms(2026, 9, 10, 9, 0, 0).unwrap();
        let now = started_at + Duration::seconds(30);
        let root = new_test_task_handle("root").unwrap();
        let high = root.create_as_last_child(new_test_task_attr("high"));
        let low = root.create_as_last_child(new_test_task_attr("low"));
        let high_id = high.get_id().unwrap();
        let low_id = low.get_id().unwrap();
        let mut repository = TestTaskRepository::new(root, started_at)
            .with_storage_directory(&storage_dir.path);
        repository.highest_priority_leaf_task_id_opt = Some(high_id);
        repository.defer_candidate_leaf_task_id_opt = Some(low_id);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut stdout = TestWriter::new();
        let mut focus = Some(if command == "高" { low_id } else { high_id });
        let mut last_focus = focus;
        let old_focus = focus.unwrap();
        let mut focus_started = started_at;
        let mut mode = if command == "高" {
            FocusSelectionMode::lowest_priority(3)
        } else {
            FocusSelectionMode::highest_priority()
        };

        let outcome = handle_interactive_submit_at(
            &mut stdout,
            &mut repository,
            &mut free_time_manager,
            InteractiveRepositoryState {
                focused_task_id_opt: &mut focus,
                last_focused_task_id_opt: &mut last_focus,
                focus_started_datetime: &mut focus_started,
                focus_selection_mode: &mut mode,
            },
            command,
            now,
        );

        assert!(matches!(outcome, InteractiveRepositoryEventOutcome::CommandExecuted(..)));
        assert_ne!(focus, Some(old_focus), "{command}");
        assert_eq!(repository.discarded_sessions.len(), 1, "{command}");
        assert_eq!(
            repository.discarded_sessions[0].reason(),
            DiscardedSessionReason::CliAutoSwitch,
            "{command}"
        );
    }
}

#[test]
fn excluded_focus_transitions_do_not_create_discard_events() {
    enum Scenario {
        SameTask,
        NoPreviousFocus,
        UnderOneSecond,
        FailedCommand,
        Finish,
    }

    for scenario in [
        Scenario::SameTask,
        Scenario::NoPreviousFocus,
        Scenario::UnderOneSecond,
        Scenario::FailedCommand,
        Scenario::Finish,
    ] {
        let storage_dir = TestStorageDir::new();
        std::fs::create_dir_all(&storage_dir.path).unwrap();
        let started_at = Local.with_ymd_and_hms(2026, 9, 10, 9, 0, 0).unwrap();
        let task = new_test_task_handle("task").unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(task, started_at)
            .with_storage_directory(&storage_dir.path);
        let mut focus = Some(task_id);
        let (command, now) = match scenario {
            Scenario::SameTask => (format!("見 {task_id}"), started_at + Duration::seconds(5)),
            Scenario::NoPreviousFocus => {
                focus = None;
                (format!("見 {task_id}"), started_at + Duration::seconds(5))
            }
            Scenario::UnderOneSecond => {
                repository.highest_priority_leaf_task_id_opt = None;
                ("外".to_string(), started_at + Duration::milliseconds(999))
            }
            Scenario::FailedCommand => (
                "見 invalid-uuid".to_string(),
                started_at + Duration::seconds(5),
            ),
            Scenario::Finish => {
                repository.highest_priority_leaf_task_id_opt = None;
                ("終".to_string(), started_at + Duration::seconds(5))
            }
        };
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut stdout = TestWriter::new();
        let mut last_focus = focus;
        let mut focus_started = started_at;
        let mut mode = FocusSelectionMode::highest_priority();

        let _outcome = handle_interactive_submit_at(
            &mut stdout,
            &mut repository,
            &mut free_time_manager,
            InteractiveRepositoryState {
                focused_task_id_opt: &mut focus,
                last_focused_task_id_opt: &mut last_focus,
                focus_started_datetime: &mut focus_started,
                focus_selection_mode: &mut mode,
            },
            &command,
            now,
        );

        assert!(repository.discarded_sessions.is_empty(), "{command}");
    }
}

#[test]
fn abnormal_interactive_endings_do_not_create_discard_events() {
    for index in 0..3 {
        let storage_dir = TestStorageDir::new();
        std::fs::create_dir_all(&storage_dir.path).unwrap();
        let started_at = Local::now() - Duration::seconds(5);
        let task = new_test_task_handle("task").unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(task, started_at)
            .with_storage_directory(&storage_dir.path);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut stdout = TestWriter::new();
        let mut focus = Some(task_id);
        let mut last_focus = focus;
        let mut focus_started = started_at;
        let mut mode = FocusSelectionMode::highest_priority();
        let event = match index {
            0 => InteractiveRepositoryEvent::Interrupted,
            1 => InteractiveRepositoryEvent::InputDisconnected,
            _ => InteractiveRepositoryEvent::InputRead(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "test read failure",
            )),
        };

        let outcome = handle_interactive_repository_event(
            &mut stdout,
            &mut repository,
            &mut free_time_manager,
            InteractiveRepositoryState {
                focused_task_id_opt: &mut focus,
                last_focused_task_id_opt: &mut last_focus,
                focus_started_datetime: &mut focus_started,
                focus_selection_mode: &mut mode,
            },
            event,
        );

        assert!(matches!(outcome, InteractiveRepositoryEventOutcome::Fatal(_)));
        assert!(repository.discarded_sessions.is_empty());
    }
}

#[test]
fn refresh_focus_change_cancels_pending_submit_and_next_command_uses_new_payload() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let started_at = Local::now() - Duration::seconds(5);
    let first_attempt_at = started_at + Duration::seconds(2);
    let root = new_test_task_handle("root").unwrap();
    let old = root.create_as_last_child(new_test_task_attr("old focus"));
    let next = root.create_as_last_child(new_test_task_attr("next focus"));
    let old_id = old.get_id().unwrap();
    let next_id = next.get_id().unwrap();
    let mut repository = TestTaskRepository::new(root, started_at)
        .with_storage_directory(&storage_dir.path);
    repository.highest_priority_leaf_task_id_opt = None;
    repository.save_failures_remaining.set(1);
    repository.save_failure_is_retryable = true;
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focus = Some(old_id);
    let mut last_focus = focus;
    let mut focus_started = started_at;
    let mut mode = FocusSelectionMode::highest_priority();

    let first = handle_interactive_submit_at(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focus,
            last_focused_task_id_opt: &mut last_focus,
            focus_started_datetime: &mut focus_started,
            focus_selection_mode: &mut mode,
        },
        "外",
        first_attempt_at,
    );
    assert!(matches!(first, InteractiveRepositoryEventOutcome::Retry(_)));
    let old_pending = mode.pending_submit().unwrap().clone();

    old.set_orig_status(Status::Done).unwrap();
    repository.highest_priority_leaf_task_id_opt = Some(next_id);
    let refresh = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focus,
            last_focused_task_id_opt: &mut last_focus,
            focus_started_datetime: &mut focus_started,
            focus_selection_mode: &mut mode,
        },
        InteractiveRepositoryEvent::Refresh,
    );
    assert!(matches!(refresh, InteractiveRepositoryEventOutcome::Continue));
    assert_eq!(focus, Some(next_id));
    assert!(mode.pending_submit().is_none());

    repository.highest_priority_leaf_task_id_opt = None;
    let next_attempt_at = Local::now() + Duration::seconds(2);
    let second = handle_interactive_submit_at(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focus,
            last_focused_task_id_opt: &mut last_focus,
            focus_started_datetime: &mut focus_started,
            focus_selection_mode: &mut mode,
        },
        "外",
        next_attempt_at,
    );
    assert!(matches!(second, InteractiveRepositoryEventOutcome::CommandExecuted(..)));
    let event = repository.discarded_sessions.last().unwrap();
    assert_ne!(event.event_id(), old_pending.command_event_id);
    assert_ne!(event.ended_at_epoch_ms(), old_pending.operation_now.timestamp_millis());
    assert_eq!(event.task_id(), next_id);
    assert_eq!(event.task_name_at_start(), "next focus");
    assert_eq!(event.reason(), DiscardedSessionReason::CliUnfocus);
}

#[test]
fn refresh_focus_change_cancels_pending_exit_and_next_exit_uses_new_payload() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let started_at = Local::now() - Duration::seconds(5);
    let root = new_test_task_handle("root").unwrap();
    let old = root.create_as_last_child(new_test_task_attr("old focus"));
    let next = root.create_as_last_child(new_test_task_attr("next focus"));
    let old_id = old.get_id().unwrap();
    let next_id = next.get_id().unwrap();
    let mut repository = TestTaskRepository::new(root, started_at)
        .with_storage_directory(&storage_dir.path);
    repository.highest_priority_leaf_task_id_opt = Some(old_id);
    repository.save_failures_remaining.set(1);
    repository.save_failure_is_retryable = true;
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focus = Some(old_id);
    let mut last_focus = focus;
    let mut focus_started = started_at;
    let mut mode = FocusSelectionMode::highest_priority();

    let first = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focus,
            last_focused_task_id_opt: &mut last_focus,
            focus_started_datetime: &mut focus_started,
            focus_selection_mode: &mut mode,
        },
        InteractiveRepositoryEvent::Exit,
    );
    assert!(matches!(first, InteractiveRepositoryEventOutcome::Retry(_)));
    let old_pending = mode.pending_exit().unwrap().clone();

    old.set_orig_status(Status::Done).unwrap();
    repository.highest_priority_leaf_task_id_opt = Some(next_id);
    let refresh = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focus,
            last_focused_task_id_opt: &mut last_focus,
            focus_started_datetime: &mut focus_started,
            focus_selection_mode: &mut mode,
        },
        InteractiveRepositoryEvent::Refresh,
    );
    assert!(matches!(refresh, InteractiveRepositoryEventOutcome::Continue));
    assert_eq!(focus, Some(next_id));
    assert!(mode.pending_exit().is_none());

    focus_started = Local::now() - Duration::seconds(2);
    let second = handle_interactive_repository_event(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focus,
            last_focused_task_id_opt: &mut last_focus,
            focus_started_datetime: &mut focus_started,
            focus_selection_mode: &mut mode,
        },
        InteractiveRepositoryEvent::Exit,
    );
    assert!(matches!(second, InteractiveRepositoryEventOutcome::Exit));
    let event = repository.discarded_sessions.last().unwrap();
    assert_ne!(event.event_id(), old_pending.exit_event_id);
    assert_ne!(event.ended_at_epoch_ms(), old_pending.ended_at.timestamp_millis());
    assert_eq!(event.task_id(), next_id);
    assert_eq!(event.task_name_at_start(), "next focus");
    assert_eq!(event.reason(), DiscardedSessionReason::CliNormalExit);
}

#[test]
fn interactive_backup_retry_does_not_create_destination_before_journal_commit() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();
    let started_at = now - Duration::minutes(10);
    let root = new_test_task_handle("root").unwrap();
    let old = root.create_as_last_child(new_test_task_attr("old focus"));
    let next = root.create_as_last_child(new_test_task_attr("next focus"));
    let old_id = old.get_id().unwrap();
    let next_id = next.get_id().unwrap();
    let _revision_observer = seed_clean_task_revision_observer(&storage_dir.path, &root, now);
    old.set_orig_status(Status::Done).unwrap();
    let snapshot = storage_dir.path.parent().unwrap().join(format!(
        "schronu-backup-retry-{}",
        Uuid::new_v4().hyphenated()
    ));
    let mut repository = TestTaskRepository::new(root, started_at)
        .with_storage_directory(&storage_dir.path);
    repository.highest_priority_leaf_task_id_opt = Some(next_id);
    repository.save_failures_remaining.set(1);
    repository.save_failure_is_retryable = true;
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focus = Some(old_id);
    let mut last_focus = focus;
    let mut focus_started = started_at;
    let mut mode = FocusSelectionMode::highest_priority();
    let command = format!("backup {}", snapshot.display());

    let first = handle_interactive_submit_at(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focus,
            last_focused_task_id_opt: &mut last_focus,
            focus_started_datetime: &mut focus_started,
            focus_selection_mode: &mut mode,
        },
        &command,
        now,
    );
    assert!(matches!(first, InteractiveRepositoryEventOutcome::Retry(_)));
    assert!(!snapshot.exists());
    assert!(!String::from_utf8_lossy(&stdout.buffer).contains("backup: OK"));
    assert_eq!(focus, Some(old_id));
    assert_eq!(focus_started, started_at);
    assert!(repository.discarded_sessions.is_empty());

    let second = handle_interactive_submit_at(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focus,
            last_focused_task_id_opt: &mut last_focus,
            focus_started_datetime: &mut focus_started,
            focus_selection_mode: &mut mode,
        },
        &command,
        now,
    );
    assert!(matches!(
        second,
        InteractiveRepositoryEventOutcome::CommandExecuted(CommandKind::Backup, _)
    ));
    assert!(snapshot.is_dir());
    assert!(String::from_utf8_lossy(&stdout.buffer).contains("backup: OK"));
    assert_eq!(focus, Some(next_id));
    assert_eq!(repository.discarded_sessions.len(), 1);
    std::fs::remove_dir_all(snapshot).unwrap();
}

#[test]
fn interactive_restore_current_retry_cleans_failed_pre_backup_and_preserves_state() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();
    let started_at = now - Duration::minutes(10);
    let root = new_test_task_handle("root").unwrap();
    let old = root.create_as_last_child(new_test_task_attr("old focus"));
    let next = root.create_as_last_child(new_test_task_attr("next focus"));
    let old_id = old.get_id().unwrap();
    let next_id = next.get_id().unwrap();
    let _revision_observer = seed_clean_task_revision_observer(&storage_dir.path, &root, now);
    let snapshot = storage_dir.path.parent().unwrap().join(format!(
        "schronu-restore-source-{}",
        Uuid::new_v4().hyphenated()
    ));
    crate::adapter::gateway::storage_snapshot::create_snapshot(&storage_dir.path, &snapshot)
        .unwrap();
    let task_files_before = storage_data_files(&storage_dir.path);
    let pre_backup = storage_dir.path.parent().unwrap().join(format!(
        "schronu-restore-pre-backup-{}",
        Uuid::new_v4().hyphenated()
    ));
    old.set_orig_status(Status::Done).unwrap();
    let mut repository = TestTaskRepository::new(root, started_at)
        .with_storage_directory(&storage_dir.path);
    repository.highest_priority_leaf_task_id_opt = Some(next_id);
    repository.save_failures_remaining.set(1);
    repository.save_failure_is_retryable = true;
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focus = Some(old_id);
    let mut last_focus = focus;
    let mut focus_started = started_at;
    let mut mode = FocusSelectionMode::highest_priority();
    let command = format!(
        "restore current {} {} REPLACE_CURRENT_STORAGE",
        snapshot.display(),
        pre_backup.display()
    );

    let first = handle_interactive_submit_at(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focus,
            last_focused_task_id_opt: &mut last_focus,
            focus_started_datetime: &mut focus_started,
            focus_selection_mode: &mut mode,
        },
        &command,
        now,
    );
    assert!(matches!(first, InteractiveRepositoryEventOutcome::Retry(_)));
    assert!(!pre_backup.exists());
    assert!(!String::from_utf8_lossy(&stdout.buffer).contains("restore current: OK"));
    assert_eq!(focus, Some(old_id));
    assert_eq!(focus_started, started_at);
    assert!(repository.discarded_sessions.is_empty());
    assert_eq!(storage_data_files(&storage_dir.path), task_files_before);
    assert!(storage_dir.path.join(".revision").is_file());
    assert_eq!(old.get_orig_status().unwrap(), Status::Done);

    let second = handle_interactive_submit_at(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focus,
            last_focused_task_id_opt: &mut last_focus,
            focus_started_datetime: &mut focus_started,
            focus_selection_mode: &mut mode,
        },
        &command,
        now,
    );
    assert!(matches!(
        second,
        InteractiveRepositoryEventOutcome::CommandExecuted(CommandKind::RestoreCurrent, _)
    ));
    assert!(pre_backup.is_dir());
    assert!(String::from_utf8_lossy(&stdout.buffer).contains("restore current: OK"));
    assert_eq!(focus, Some(next_id));
    assert_eq!(repository.discarded_sessions.len(), 1);

    std::fs::remove_dir_all(snapshot).unwrap();
    std::fs::remove_dir_all(pre_backup).unwrap();
}

#[test]
fn restore_current_compensation_reload_failure_is_fatal_after_storage_restore() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();
    let started_at = now - Duration::minutes(10);
    let root = new_test_task_handle("root").unwrap();
    let old = root.create_as_last_child(new_test_task_attr("old focus"));
    let next = root.create_as_last_child(new_test_task_attr("next focus"));
    let old_id = old.get_id().unwrap();
    let next_id = next.get_id().unwrap();
    let _revision_observer = seed_clean_task_revision_observer(&storage_dir.path, &root, now);
    let snapshot = storage_dir.path.parent().unwrap().join(format!(
        "schronu-restore-source-{}",
        Uuid::new_v4().hyphenated()
    ));
    crate::adapter::gateway::storage_snapshot::create_snapshot(&storage_dir.path, &snapshot)
        .unwrap();
    let task_files_before = storage_data_files(&storage_dir.path);
    let pre_backup = storage_dir.path.parent().unwrap().join(format!(
        "schronu-restore-pre-backup-{}",
        Uuid::new_v4().hyphenated()
    ));
    old.set_orig_status(Status::Done).unwrap();
    let mut repository = TestTaskRepository::new(root, started_at)
        .with_storage_directory(&storage_dir.path);
    repository.highest_priority_leaf_task_id_opt = Some(next_id);
    repository.save_failures_remaining.set(1);
    repository.save_failure_is_retryable = true;
    repository.load_failure_on_attempt_opt = Some(3);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focus = Some(old_id);
    let mut last_focus = focus;
    let mut focus_started = started_at;
    let mut mode = FocusSelectionMode::highest_priority();
    let command = format!(
        "restore current {} {} REPLACE_CURRENT_STORAGE",
        snapshot.display(),
        pre_backup.display()
    );

    let outcome = handle_interactive_submit_at(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focus,
            last_focused_task_id_opt: &mut last_focus,
            focus_started_datetime: &mut focus_started,
            focus_selection_mode: &mut mode,
        },
        &command,
        now,
    );

    let error = match outcome {
        InteractiveRepositoryEventOutcome::Fatal(error) => error.to_string(),
        _ => panic!("compensation reload failure must be fatal"),
    };
    assert!(error.contains("repository reload"));
    assert!(error.contains(storage_dir.path.to_str().unwrap()));
    assert!(!String::from_utf8_lossy(&stdout.buffer).contains("restore current: OK"));
    assert_eq!(repository.load_attempt_count.get(), 3);
    assert_eq!(focus, Some(old_id));
    assert_eq!(focus_started, started_at);
    assert!(repository.discarded_sessions.is_empty());
    assert_eq!(storage_data_files(&storage_dir.path), task_files_before);

    std::fs::remove_dir_all(snapshot).unwrap();
    std::fs::remove_dir_all(pre_backup).unwrap();
}

#[cfg(unix)]
#[test]
fn restore_current_compensation_cleanup_failure_is_fatal_after_storage_restore() {
    use std::os::unix::fs::PermissionsExt;

    let storage_dir = TestStorageDir::new();
    let maintenance_root = storage_dir.path.join("maintenance-root");
    let storage = maintenance_root.join("storage");
    std::fs::create_dir_all(&storage).unwrap();
    let now = Local.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();
    let started_at = now - Duration::minutes(10);
    let root = new_test_task_handle("root").unwrap();
    let old = root.create_as_last_child(new_test_task_attr("old focus"));
    let next = root.create_as_last_child(new_test_task_attr("next focus"));
    let old_id = old.get_id().unwrap();
    let next_id = next.get_id().unwrap();
    let _revision_observer = seed_clean_task_revision_observer(&storage, &root, now);
    let snapshot = maintenance_root.join("source");
    crate::adapter::gateway::storage_snapshot::create_snapshot(&storage, &snapshot).unwrap();
    let task_files_before = storage_data_files(&storage);
    let pre_backup = maintenance_root.join("pre-backup");
    old.set_orig_status(Status::Done).unwrap();
    let mut repository =
        TestTaskRepository::new(root, started_at).with_storage_directory(&storage);
    repository.highest_priority_leaf_task_id_opt = Some(next_id);
    repository.save_failures_remaining.set(1);
    repository.save_failure_is_retryable = true;
    let cleanup_root = maintenance_root.clone();
    repository.load_hook_on_attempt_opt = Some((
        3,
        Rc::new(move || {
            std::fs::set_permissions(&cleanup_root, std::fs::Permissions::from_mode(0o555))
                .unwrap();
        }),
    ));
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    let mut focus = Some(old_id);
    let mut last_focus = focus;
    let mut focus_started = started_at;
    let mut mode = FocusSelectionMode::highest_priority();
    let command = format!(
        "restore current {} {} REPLACE_CURRENT_STORAGE",
        snapshot.display(),
        pre_backup.display()
    );

    let outcome = handle_interactive_submit_at(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        InteractiveRepositoryState {
            focused_task_id_opt: &mut focus,
            last_focused_task_id_opt: &mut last_focus,
            focus_started_datetime: &mut focus_started,
            focus_selection_mode: &mut mode,
        },
        &command,
        now,
    );
    std::fs::set_permissions(&maintenance_root, std::fs::Permissions::from_mode(0o755)).unwrap();

    let error = match outcome {
        InteractiveRepositoryEventOutcome::Fatal(error) => error.to_string(),
        _ => panic!("compensation cleanup failure must be fatal"),
    };
    assert!(error.contains("cleanup"));
    assert!(error.contains("storage restored: true"));
    assert!(!String::from_utf8_lossy(&stdout.buffer).contains("restore current: OK"));
    assert_eq!(repository.load_attempt_count.get(), 3);
    assert_eq!(focus, Some(old_id));
    assert_eq!(focus_started, started_at);
    assert!(repository.discarded_sessions.is_empty());
    assert_eq!(storage_data_files(&storage), task_files_before);
}
