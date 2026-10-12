use super::*;
use crate::application::schedule_use_case::get_schedule;

#[test]
fn numeric_all_command_selects_longest_strictly_shorter_task_in_schedule_order() {
    let now = Local.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap();
    let root = new_test_task_handle("SELECTION_ROOT").unwrap();
    root.set_estimated_work_seconds(0).unwrap();
    for (name, minutes) in [("SHORT", 10), ("TIE_A", 20), ("TIE_B", 20), ("EXACT", 30)] {
        let mut attr = new_test_task_attr(name);
        attr.set_start_time(now);
        attr.set_estimated_work_seconds(minutes * 60);
        root.create_as_last_child(attr).sync_clock(now).unwrap();
    }
    let repository = TestTaskRepository::new(root.clone(), now);
    let schedule = get_schedule(&repository).unwrap();
    let expected = schedule
        .iter()
        .find(|row| row.scheduled_work_seconds == 20 * 60)
        .unwrap();
    let result = execute_command_for_test(root.clone(), now, None, "全 30");
    assert_eq!(result.focused_task_id_opt, expected.actual_task_id());
    assert!(result.output.contains(&expected.task.name));
    for name in ["SHORT", "TIE_A", "TIE_B", "EXACT"] {
        if name != expected.task.name {
            assert!(!result.output.contains(name), "{}", result.output);
        }
    }
    let shorter = schedule
        .iter()
        .find(|row| row.task.name == "SHORT")
        .unwrap();
    let result = execute_command_for_test(root, now, None, "全 20");
    assert_eq!(result.focused_task_id_opt, shorter.actual_task_id());
}
