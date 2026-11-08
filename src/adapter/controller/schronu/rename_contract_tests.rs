use super::*;

#[test]
fn renameはfocus対象だけを変更して保存し同名更新を追跡しない() {
    for rename_root in [false, true] {
        let storage = TestStorageDir::new();
        std::fs::create_dir_all(&storage.path).unwrap();
        let now = test_task_time();
        let root = new_test_task_handle("project root").unwrap();
        let child = root.create_as_last_child(new_test_task_attr("child task"));
        let sibling = root.create_as_last_child(new_test_task_attr("sibling task"));
        let root_id = root.get_id().unwrap();
        let child_id = child.get_id().unwrap();
        let sibling_id = sibling.get_id().unwrap();
        let target_id = if rename_root { root_id } else { child_id };
        let new_name = "  更新後の  名前 '引用' \"二重\" C:\\path  ";
        let old_name = if rename_root {
            "project root"
        } else {
            "child task"
        };
        let mut repository = TaskRepository::new(storage.path.to_str().unwrap());
        repository.sync_clock(now).unwrap();
        repository.load().unwrap();
        repository.start_new_project(root.clone()).unwrap();
        repository.sync_clock(now).unwrap();
        let expected_tree = complete_task_tree_snapshot(&root)
            .into_iter()
            .map(|row| {
                row.replace(
                    &format!("name={old_name:?}|"),
                    &format!("name={new_name:?}|"),
                )
            })
            .collect::<Vec<_>>();
        repository.save().unwrap();
        let revision = repository
            .get_by_id(target_id)
            .unwrap()
            .unwrap()
            .get_persistent_mutation_revision()
            .unwrap();
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut stdout = TestWriter::new();
        let mut focus = Some(target_id);
        let mut last_focus = focus;
        let started_at = now - Duration::minutes(5);
        let mut focus_started = started_at;
        let mut selection = FocusSelectionMode::highest_priority();
        selection.set_explicit(true);
        let line = r#"名 "  更新後の  名前 '引用' \"二重\" C:\\path  ""#;
        for expected_revision in [revision + 1, revision + 1] {
            let outcome = handle_interactive_submit_at(
                &mut stdout,
                &mut repository,
                &mut free_time_manager,
                InteractiveRepositoryState {
                    focused_task_id_opt: &mut focus,
                    last_focused_task_id_opt: &mut last_focus,
                    focus_started_datetime: &mut focus_started,
                    focus_selection_mode: &mut selection,
                    timer_sessions: None,
                },
                line,
                now,
            );
            assert!(matches!(
                outcome,
                InteractiveRepositoryEventOutcome::CommandExecuted(..)
            ));
            assert_eq!(focus, Some(target_id));
            assert_eq!(last_focus, focus);
            assert_eq!(focus_started, started_at);
            assert!(selection.is_explicit());
            assert_eq!(
                repository
                    .get_by_id(target_id)
                    .unwrap()
                    .unwrap()
                    .get_name()
                    .unwrap(),
                new_name
            );
            assert_eq!(
                repository
                    .get_by_id(target_id)
                    .unwrap()
                    .unwrap()
                    .get_persistent_mutation_revision()
                    .unwrap(),
                expected_revision
            );
        }
        let mut reloaded = TaskRepository::new(storage.path.to_str().unwrap());
        reloaded.sync_clock(now).unwrap();
        reloaded.load().unwrap();
        let root = reloaded.get_by_id(root_id).unwrap().unwrap();
        assert_eq!(complete_task_tree_snapshot(&root), expected_tree);
        let child = reloaded.get_by_id(child_id).unwrap().unwrap();
        assert_eq!(
            root.get_name().unwrap(),
            if rename_root {
                new_name
            } else {
                "project root"
            }
        );
        assert_eq!(
            child.get_name().unwrap(),
            if rename_root { "child task" } else { new_name }
        );
        assert_eq!(child.parent().unwrap().unwrap().get_id().unwrap(), root_id);
        assert_eq!(root.get_children().unwrap().len(), 2);
        assert_eq!(
            reloaded
                .get_by_id(sibling_id)
                .unwrap()
                .unwrap()
                .get_name()
                .unwrap(),
            "sibling task"
        );
    }
}

#[test]
fn renameは不正名を拒否してfocusなしでは変更しない() {
    let now = test_task_time();
    let task = new_test_task_handle("unchanged task").unwrap();
    let id = task.get_id().unwrap();
    let mut repository = TestTaskRepository::new(task.clone(), now);
    let before = complete_task_tree_snapshot(&task);
    let revision = task.get_persistent_mutation_revision().unwrap();
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut stdout = TestWriter::new();
    for line in [
        "名",
        "名 名前 余分",
        "名 \"\"",
        "名 '  '",
        "名 123",
        "名 +123",
        "名 -123",
        "名 'tab\tname'",
        "名 'ESC\u{1b}name'",
    ] {
        let mut focus = Some(id);
        assert!(
            parse_interactive_command(line)
                .map_err(map_command_parse_error)
                .and_then(|command| execute_parsed(
                    &mut stdout,
                    &mut repository,
                    &mut free_time_manager,
                    &mut focus,
                    &now,
                    &command,
                    OutcomeApplicationMode::Flushed,
                ))
                .is_err(),
            "{line:?}"
        );
        assert_eq!(focus, Some(id));
        assert_eq!(complete_task_tree_snapshot(&task), before);
        assert_eq!(task.get_persistent_mutation_revision().unwrap(), revision);
    }
    let mut focus = None;
    execute(
        &mut stdout,
        &mut repository,
        &mut free_time_manager,
        &mut focus,
        &now,
        "名 new-name",
    )
    .unwrap();
    assert_eq!(focus, None);
    assert_eq!(complete_task_tree_snapshot(&task), before);
    assert_eq!(task.get_persistent_mutation_revision().unwrap(), revision);
}
