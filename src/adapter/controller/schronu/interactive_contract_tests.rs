use super::command::{parse_interactive_command, representative_valid_commands, CommandKind};
use super::interactive::should_suppress_leaf_tasks_after_command;

#[test]
fn interactive再描画判断はtyped_command_kindだけで決まる() {
    for kind in [
        CommandKind::NewProject,
        CommandKind::UnplannedProject,
        CommandKind::Tree,
        CommandKind::Leaves,
        CommandKind::ShowAll,
        CommandKind::Tail,
        CommandKind::Today,
        CommandKind::Calendar,
        CommandKind::Band,
        CommandKind::DeferRoutines,
        CommandKind::Flatten,
        CommandKind::Pack,
    ] {
        assert!(
            should_suppress_leaf_tasks_after_command(kind),
            "{kind:?} must not append the leaf tree after its own display"
        );
    }

    for kind in [
        CommandKind::Estimate,
        CommandKind::Focus,
        CommandKind::Defer,
        CommandKind::Finish,
    ] {
        assert!(
            !should_suppress_leaf_tasks_after_command(kind),
            "{kind:?} must refresh the leaf tree after mutation"
        );
    }
}

#[test]
fn interactive_aliasは同じtyped_kindと再描画方針になる() {
    for (aliases, expected_kind) in [
        (["新 project 15", "new project 15"], CommandKind::NewProject),
        (["全", "all"], CommandKind::ShowAll),
    ] {
        let kinds = aliases.map(|command| {
            parse_interactive_command(command)
                .expect("alias fixture must parse")
                .kind()
        });

        assert_eq!(kinds, [expected_kind, expected_kind]);
        assert!(kinds
            .into_iter()
            .all(should_suppress_leaf_tasks_after_command));
    }
}

#[test]
fn interactive再描画分類は全command_kindを網羅する() {
    let all_command_kinds = representative_valid_commands().into_iter().fold(
        Vec::<CommandKind>::new(),
        |mut kinds, command| {
            let kind = command.kind();
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
            kinds
        },
    );
    assert_eq!(
        all_command_kinds.len(),
        55,
        "shared representative command fixture must cover every CommandKind"
    );
    for (index, kind) in all_command_kinds.iter().enumerate() {
        assert!(
            !all_command_kinds[..index].contains(kind),
            "classification fixture must contain {kind:?} only once"
        );
    }

    for kind in all_command_kinds {
        let expected = match kind {
            CommandKind::NewProject
            | CommandKind::UnplannedProject
            | CommandKind::Tree
            | CommandKind::Leaves
            | CommandKind::ShowAll
            | CommandKind::Tail
            | CommandKind::Today
            | CommandKind::Calendar
            | CommandKind::Band
            | CommandKind::DeferRoutines
            | CommandKind::Flatten
            | CommandKind::Pack => true,
            CommandKind::Noop
            | CommandKind::HobbyProject
            | CommandKind::Sequential
            | CommandKind::Repeat
            | CommandKind::Appointment
            | CommandKind::Start
            | CommandKind::Ancestor
            | CommandKind::Root
            | CommandKind::NonRepetitive
            | CommandKind::Focus
            | CommandKind::Pick
            | CommandKind::Open
            | CommandKind::Timer
            | CommandKind::Obsidian
            | CommandKind::Unfocus
            | CommandKind::Parent
            | CommandKind::Children
            | CommandKind::Deepest
            | CommandKind::NextUp
            | CommandKind::Breakdown
            | CommandKind::Split
            | CommandKind::Wait
            | CommandKind::Deadline
            | CommandKind::Estimate
            | CommandKind::Arrange
            | CommandKind::TuckAway
            | CommandKind::Actual
            | CommandKind::Priority
            | CommandKind::Category
            | CommandKind::Work
            | CommandKind::Defer
            | CommandKind::Escape
            | CommandKind::Extrude
            | CommandKind::Clear
            | CommandKind::Gather
            | CommandKind::Finish
            | CommandKind::FocusHighest
            | CommandKind::FocusLowest
            | CommandKind::Backup
            | CommandKind::BackupVerify
            | CommandKind::Restore
            | CommandKind::RestoreCurrent
            | CommandKind::Verify => false,
        };

        assert_eq!(
            should_suppress_leaf_tasks_after_command(kind),
            expected,
            "classification changed for {kind:?}"
        );
    }
}
