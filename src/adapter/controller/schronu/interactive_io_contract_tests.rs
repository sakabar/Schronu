#[test]
fn test_interactive出力failureは成功済みcommandだけを保存済みにする() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
    let task = new_test_task_handle("更新対象").unwrap();
    let task_id = task.get_id().unwrap();
    let mut repository =
        TestTaskRepository::new(task, now).with_storage_directory(&storage_dir.path);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::highest_priority();
    let fail_output = Rc::new(Cell::new(false));
    let mut terminal_factory = SignaledFailureTerminalFactory {
        fail_output: Rc::clone(&fail_output),
        error_kind: std::io::ErrorKind::PermissionDenied,
    };
    let mut input = ScriptedInteractiveInput::command("予 45");

    let result = interactive::run_with_terminal_factory(
        now,
        &mut terminal_factory,
        &mut input,
        |stdout, event| {
            if matches!(event, interactive::DriverEvent::RenderScreen { .. }) {
                return interactive::DriverOutcome::Continue;
            }
            let repository_event = match event {
                interactive::DriverEvent::Submit { line } => {
                    InteractiveRepositoryEvent::Submit { line }
                }
                interactive::DriverEvent::Refresh => InteractiveRepositoryEvent::Refresh,
                interactive::DriverEvent::Exit => InteractiveRepositoryEvent::Exit,
                interactive::DriverEvent::Interrupted => InteractiveRepositoryEvent::Interrupted,
                interactive::DriverEvent::InputDisconnected => {
                    InteractiveRepositoryEvent::InputDisconnected
                }
                interactive::DriverEvent::InputRead(error) => {
                    InteractiveRepositoryEvent::InputRead(error)
                }
                interactive::DriverEvent::RenderScreen { .. } => unreachable!(),
            };
            match handle_interactive_repository_event(
                stdout,
                &mut repository,
                &mut free_time_manager,
                InteractiveRepositoryState {
                    focused_task_id_opt: &mut focused_task_id_opt,
                    last_focused_task_id_opt: &mut last_focused_task_id_opt,
                    focus_started_datetime: &mut focus_started_datetime,
                    focus_selection_mode: &mut focus_selection_mode,
                },
                repository_event,
            ) {
                InteractiveRepositoryEventOutcome::Continue => {
                    interactive::DriverOutcome::Continue
                }
                InteractiveRepositoryEventOutcome::CommandExecuted(..) => {
                    fail_output.set(true);
                    interactive::DriverOutcome::Submitted
                }
                InteractiveRepositoryEventOutcome::Retry(error) => {
                    interactive::DriverOutcome::Retry(error)
                }
                InteractiveRepositoryEventOutcome::Exit => interactive::DriverOutcome::Exit,
                InteractiveRepositoryEventOutcome::Fatal(error) => {
                    interactive::DriverOutcome::Fatal(error)
                }
            }
        },
    );

    assert!(matches!(
        result,
        Err(interactive::DriverRunError::Io(
            interactive::InteractiveIoError::Output(ref error)
        )) if error.kind() == std::io::ErrorKind::PermissionDenied
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
    assert_eq!(
        repository
            .operation_trace()
            .iter()
            .filter(|operation| **operation == "save")
            .count(),
        1
    );
}

#[test]
fn test_interactive_raw_mode初期化failureはcommandを実行も保存もしない() {
    let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
    let task = new_test_task_handle("未更新").unwrap();
    let repository = TestTaskRepository::new(task, now);
    let mut terminal_factory = RawModeFailureTerminalFactory;
    let mut input = ScriptedInteractiveInput::empty();
    let callback_call_count = Cell::new(0);

    let result = interactive::run_with_terminal_factory(
        now,
        &mut terminal_factory,
        &mut input,
        |_, _| -> interactive::DriverOutcome<CliRepositoryTransactionError, RunError> {
            callback_call_count.set(callback_call_count.get() + 1);
            interactive::DriverOutcome::Continue
        },
    );

    assert!(matches!(
        result,
        Err(interactive::DriverRunError::Io(
            interactive::InteractiveIoError::RawMode(ref error)
        )) if error.kind() == std::io::ErrorKind::PermissionDenied
    ));
    assert_eq!(callback_call_count.get(), 0);
    assert_eq!(repository.save_attempt_count.get(), 0);
    assert!(repository.operation_trace().is_empty());
}

#[test]
fn test_interactive_ctrl_dのsaveと診断出力が共に失敗してもsave_errorを失わない() {
    let storage_dir = TestStorageDir::new();
    std::fs::create_dir_all(&storage_dir.path).unwrap();
    let now = Local.with_ymd_and_hms(2026, 9, 4, 12, 0, 0).unwrap();
    let task = new_test_task_handle("保存失敗対象").unwrap();
    let task_id = task.get_id().unwrap();
    let fail_output = Rc::new(Cell::new(false));
    let mut repository = TestTaskRepository::new(task, now)
        .with_storage_directory(&storage_dir.path)
        .with_save_attempt_signal(Rc::clone(&fail_output));
    repository.save_failures_remaining.set(1);
    let mut free_time_manager = TestFreeTimeManager::default();
    let mut focused_task_id_opt = Some(task_id);
    let mut last_focused_task_id_opt = Some(task_id);
    let mut focus_started_datetime = now;
    let mut focus_selection_mode = FocusSelectionMode::highest_priority();
    let mut terminal_factory = SignaledFailureTerminalFactory {
        fail_output: Rc::clone(&fail_output),
        error_kind: std::io::ErrorKind::BrokenPipe,
    };
    let mut input = ScriptedInteractiveInput {
        inputs: VecDeque::from([interactive::ReceivedInput::Key(termion::event::Key::Ctrl(
            'd',
        ))]),
    };

    let driver_result = interactive::run_with_terminal_factory(
        now,
        &mut terminal_factory,
        &mut input,
        |stdout, event| {
            if matches!(event, interactive::DriverEvent::RenderScreen { .. }) {
                return interactive::DriverOutcome::Continue;
            }
            let repository_event = match event {
                interactive::DriverEvent::Submit { line } => {
                    InteractiveRepositoryEvent::Submit { line }
                }
                interactive::DriverEvent::Refresh => InteractiveRepositoryEvent::Refresh,
                interactive::DriverEvent::Exit => InteractiveRepositoryEvent::Exit,
                interactive::DriverEvent::Interrupted => InteractiveRepositoryEvent::Interrupted,
                interactive::DriverEvent::InputDisconnected => {
                    InteractiveRepositoryEvent::InputDisconnected
                }
                interactive::DriverEvent::InputRead(error) => {
                    InteractiveRepositoryEvent::InputRead(error)
                }
                interactive::DriverEvent::RenderScreen { .. } => unreachable!(),
            };
            match handle_interactive_repository_event(
                stdout,
                &mut repository,
                &mut free_time_manager,
                InteractiveRepositoryState {
                    focused_task_id_opt: &mut focused_task_id_opt,
                    last_focused_task_id_opt: &mut last_focused_task_id_opt,
                    focus_started_datetime: &mut focus_started_datetime,
                    focus_selection_mode: &mut focus_selection_mode,
                },
                repository_event,
            ) {
                InteractiveRepositoryEventOutcome::Continue => {
                    interactive::DriverOutcome::Continue
                }
                InteractiveRepositoryEventOutcome::CommandExecuted(..) => {
                    interactive::DriverOutcome::Submitted
                }
                InteractiveRepositoryEventOutcome::Retry(error) => {
                    interactive::DriverOutcome::Retry(error)
                }
                InteractiveRepositoryEventOutcome::Exit => interactive::DriverOutcome::Exit,
                InteractiveRepositoryEventOutcome::Fatal(error) => {
                    interactive::DriverOutcome::Fatal(error)
                }
            }
        },
    );
    let error = classify_interactive_run_result(driver_result).unwrap_err();

    assert!(matches!(
        &error,
        RunError::Command(CommandError::ExitSaveDiagnostic {
            save_error,
            output_error,
        }) if save_error.to_string().contains("test save failure")
            && output_error.kind() == std::io::ErrorKind::BrokenPipe
    ));
    let command_error = std::error::Error::source(&error).unwrap();
    let save_error = command_error.source().unwrap();
    assert!(save_error.to_string().contains("test save failure"));
    assert_eq!(repository.save_attempt_count.get(), 1);
}
