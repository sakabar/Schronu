#[test]
fn runtime_driverは表示と更新command後に葉を一度だけ描画しfocusとflushを保持する() {
    for (command, expected_seconds) in [("葉", 600), ("予 20", 1200)] {
        let storage_dir = TestStorageDir::new();
        std::fs::create_dir_all(&storage_dir.path).unwrap();
        let now = Local::now();
        let task = TaskHandle::with_identity("redraw対象", next_test_task_id(), now).unwrap();
        task.set_estimated_work_seconds(600).unwrap();
        let task_id = task.get_id().unwrap();
        let mut repository = TestTaskRepository::new(task, now)
            .with_storage_directory(&storage_dir.path);
        let mut free_time_manager = TestFreeTimeManager::default();
        let mut focused = Some(task_id);
        let mut last_focused = Some(task_id);
        let mut focus_started = now;
        let mut selection = FocusSelectionMode::highest_priority();
        selection.set_explicit(true);
        let output = Rc::new(RefCell::new(Vec::new()));
        let flush_count = Rc::new(Cell::new(0));
        let drop_count = Rc::new(Cell::new(0));
        let mut terminal = SharedSignaledFailureTerminalFactory {
            fail_output: Rc::new(Cell::new(false)),
            output: Rc::clone(&output),
            drop_count: Rc::clone(&drop_count),
            flush_count: Rc::clone(&flush_count),
            error_kind: std::io::ErrorKind::BrokenPipe,
            fail_after_output_marker: None,
        };
        let mut input = ScriptedInteractiveInput::command(command);
        input.inputs.push_back(interactive::ReceivedInput::Key(termion::event::Key::Ctrl('c')));

        let result = run_interactive_runtime_for_test(
            now, &mut terminal, &mut input, &mut repository, &mut free_time_manager,
            &mut focused, &mut last_focused, &mut focus_started, &mut selection,
        );

        assert!(matches!(classify_interactive_run_result(result), Err(RunError::Interrupted)));
        let output = String::from_utf8(output.borrow().clone()).unwrap();
        assert_eq!(output.matches("1\tredraw対象\t").count(), 1, "{command}: {output}");
        assert_eq!(output.matches(&format_focused_task_header(None, 0)).count(), 2, "{command}: {output}");
        assert!(output.rfind(&format_focused_task_header(None, 0)).unwrap()
            > output.find("1\tredraw対象\t").unwrap());
        assert_eq!(focused, Some(task_id));
        assert_eq!(last_focused, Some(task_id));
        assert_eq!(focus_started, now);
        assert!(selection.is_explicit());
        assert_eq!(repository.task.get_estimated_work_seconds().unwrap(), expected_seconds);
        // Startup, initial focus/prompt, each character prompt, command echo/outcome, focus,
        // submitted prompt and terminal restoration each flush once.
        assert_eq!(flush_count.get(), command.chars().count() + 8, "{command}");
        assert_eq!(drop_count.get(), 1);
    }
}
