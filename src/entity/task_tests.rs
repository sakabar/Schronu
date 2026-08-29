#[cfg(test)]
use chrono::TimeZone;

#[cfg(test)]
use dendron::{tree, Tree};

#[cfg(test)]
use uuid::uuid;

#[test]
fn test_read_status_doneの文字列を変換する() {
    let s = "done";
    let actual = read_status(s);
    assert_eq!(actual, Some(Status::Done));
}

#[test]
#[allow(non_snake_case)]
fn test_read_status_大文字のDoneの文字列を変換する() {
    let s = "done";
    let actual = read_status(s);
    assert_eq!(actual, Some(Status::Done));
}

#[test]
fn test_read_status_todoの文字列を変換する() {
    let s = "todo";
    let actual = read_status(s);
    assert_eq!(actual, Some(Status::Todo));
}

#[test]
fn test_read_status_pendingの文字列を変換する() {
    let s = "pending";
    let actual = read_status(s);
    assert_eq!(actual, Some(Status::Pending));
}

#[test]
#[allow(non_snake_case)]
fn test_read_status_パーズできなかったときはNoneを返す() {
    let s = "invalid_status";
    let actual = read_status(s);
    assert_eq!(actual, None);
}

#[test]
fn test_read_repetition_anchor_deadlineの文字列を変換する() {
    let actual = read_repetition_anchor("deadline");
    assert_eq!(actual, RepetitionAnchor::Deadline);
}

#[test]
fn test_read_repetition_anchor_completionの文字列を変換する() {
    let actual = read_repetition_anchor("completion");
    assert_eq!(actual, RepetitionAnchor::Completion);
}

#[test]
fn test_read_repetition_anchor_不正値ならdeadlineを返す() {
    let actual = read_repetition_anchor("invalid");
    assert_eq!(actual, RepetitionAnchor::Deadline);
}

#[test]
fn test_read_project_category_文字列を変換する() {
    assert_eq!(
        read_project_category("earning"),
        Some(ProjectCategory::Earning)
    );
    assert_eq!(
        read_project_category("sustaining"),
        Some(ProjectCategory::Sustaining)
    );
    assert_eq!(
        read_project_category("recovery"),
        Some(ProjectCategory::Recovery)
    );
    assert_eq!(
        read_project_category("investment"),
        Some(ProjectCategory::Investment)
    );
    assert_eq!(
        read_project_category("consumption"),
        Some(ProjectCategory::Consumption)
    );
}

#[test]
fn test_read_project_category_表示記号を変換する() {
    assert_eq!(read_project_category("獲"), Some(ProjectCategory::Earning));
    assert_eq!(
        read_project_category("維"),
        Some(ProjectCategory::Sustaining)
    );
    assert_eq!(read_project_category("回"), Some(ProjectCategory::Recovery));
    assert_eq!(
        read_project_category("資"),
        Some(ProjectCategory::Investment)
    );
    assert_eq!(
        read_project_category("消"),
        Some(ProjectCategory::Consumption)
    );
}

#[test]
fn test_read_project_category_空文字と不正値はnoneを返す() {
    assert_eq!(read_project_category(""), None);
    assert_eq!(read_project_category("invalid"), None);
}

#[test]
#[allow(non_snake_case)]
pub fn test_new_with_current_time_現在時刻がpending_until以前でPending状態であること() {
    let now = Local.with_ymd_and_hms(2026, 8, 19, 12, 0, 0).unwrap();
    let pending_until = DateTime::<Local>::MAX_UTC.into();
    let actual = ImmutableTask::new_with_current_time(
        "タスク".to_string(),
        Status::Pending,
        pending_until,
        vec![],
        now,
    );
    let expected = ImmutableTask::new("タスク".to_string(), Status::Pending, pending_until, vec![]);

    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
pub fn test_new_with_current_time_現在時刻がpending_until以降の場合Todo状態となること() {
    let now = Local.with_ymd_and_hms(2026, 8, 19, 12, 0, 0).unwrap();
    let pending_until = DateTime::<Local>::MIN_UTC.into();
    let actual = ImmutableTask::new_with_current_time(
        "タスク".to_string(),
        Status::Pending,
        pending_until,
        vec![],
        now,
    );
    let expected = ImmutableTask::new("タスク".to_string(), Status::Todo, pending_until, vec![]);

    assert_eq!(actual, expected);
}

#[test]
fn test_new_with_current_time_caller指定時刻でpending状態を評価する() {
    let now = Local.with_ymd_and_hms(2026, 8, 19, 12, 0, 0).unwrap();
    let pending_until = now + Duration::minutes(1);

    let pending = ImmutableTask::new_with_current_time(
        "タスク".to_string(),
        Status::Pending,
        pending_until,
        vec![],
        now,
    );
    let todo = ImmutableTask::new_with_current_time(
        "タスク".to_string(),
        Status::Pending,
        pending_until,
        vec![],
        pending_until + Duration::seconds(1),
    );

    assert_eq!(pending.get_status(), &Status::Pending);
    assert_eq!(todo.get_status(), &Status::Todo);
}

#[test]
fn test_extract_leaf_immutable_tasks_from_project_タスクのchildrenが空配列の場合() {
    let task = ImmutableTask::new_with_name("タスク".to_string());
    let actual = extract_leaf_immutable_tasks_from_project(&task);

    let t = ImmutableTask::new_with_name("タスク".to_string());

    let expected = vec![&t];
    assert_eq!(actual, expected);
}

#[test]
fn test_extract_leaf_immutable_tasks_from_project_タスクのchildrenが空配列ではない場合は再帰して結果を返す(
) {
    /*
     parent_task_1
       - child_task_1
         - grand_child_task (葉)
       - child_task_2 (葉)
    */

    let grand_child_task_1 = ImmutableTask::new_with_name("孫タスク1".to_string());
    let child_task_1 =
        ImmutableTask::new_with_name_children("子タスク1".to_string(), vec![grand_child_task_1]);
    let child_task_2 = ImmutableTask::new_with_name("子タスク2".to_string());
    let parent_task_1 = ImmutableTask::new_with_name_children(
        "親タスク1".to_string(),
        vec![child_task_1, child_task_2],
    );

    let actual = extract_leaf_immutable_tasks_from_project(&parent_task_1);
    let t1 = ImmutableTask::new_with_name("孫タスク1".to_string());
    let t2 = ImmutableTask::new_with_name("子タスク2".to_string());
    let expected = vec![&t1, &t2];
    assert_eq!(actual, expected);
}

#[test]
fn test_extract_leaf_immutable_tasks_from_project_done状態のタスクとその子孫は全て無視されること() {
    /*
     parent_task_1
       - child_task_1 (Done)
         - grand_child_task (todo, だが親がdoneなので無視される)
       - child_task_2
    */

    let grand_child_task_1 = ImmutableTask::new_with_name("孫タスク1".to_string());
    let child_task_1 = ImmutableTask::new_with_name_status_children(
        "子タスク1".to_string(),
        Status::Done,
        vec![grand_child_task_1],
    );

    let child_task_2 = ImmutableTask::new_with_name("子タスク2".to_string());

    let parent_task_1 = ImmutableTask::new_with_name_children(
        "親タスク1".to_string(),
        vec![child_task_1, child_task_2],
    );

    let actual = extract_leaf_immutable_tasks_from_project(&parent_task_1);
    let expected_child_task_2 = ImmutableTask::new_with_name("子タスク2".to_string());
    let expected = vec![&expected_child_task_2];
    assert_eq!(actual, expected);
}

#[test]
fn test_extract_leaf_immutable_tasks_from_project_途中にpending状態のタスクがあった場合は子孫を辿るが_葉がpending状態の場合は結果に入らないこと(
) {
    /*
     parent_task_1
       - child_task_1 (Pending)
         - grand_child_task (todo、親がPendingだがそれは関係なく結果として返る)
       - child_task_2 (Pendingの葉なので結果に入らない)
    */

    let grand_child_task_1 = ImmutableTask::new_with_name("孫タスク1".to_string());
    let child_task_1 = ImmutableTask::new_with_name_status_children(
        "子タスク1".to_string(),
        Status::Pending,
        vec![grand_child_task_1],
    );

    let child_task_2 = ImmutableTask::new_with_name_status_children(
        "子タスク2".to_string(),
        Status::Pending,
        vec![],
    );

    let parent_task_1 = ImmutableTask::new_with_name_children(
        "親タスク1".to_string(),
        vec![child_task_1, child_task_2],
    );

    let actual = extract_leaf_immutable_tasks_from_project(&parent_task_1);
    let expected_grand_child_task_1 = ImmutableTask::new_with_name("孫タスク1".to_string());
    let expected = vec![&expected_grand_child_task_1];
    assert_eq!(actual, expected);
}

#[test]
fn test_extract_leaf_immutable_tasks_from_project_子が全てdoneのタスクは葉として扱われること() {
    /*
     parent_task_1
       - child_task_1 (子が全てdoneなので葉として返る)
         - grand_child_task_1 (done)
         - grand_child_task_2 (done)
       - child_task_2 (返る)
    */

    let grand_child_task_1 =
        ImmutableTask::new_with_name_status_children("孫タスク1".to_string(), Status::Done, vec![]);
    let grand_child_task_2 =
        ImmutableTask::new_with_name_status_children("孫タスク2".to_string(), Status::Done, vec![]);

    let child_task_1 = ImmutableTask::new_with_name_status_children(
        "子タスク1".to_string(),
        Status::Todo,
        vec![grand_child_task_1, grand_child_task_2],
    );

    let expected_child_task_1 = child_task_1.clone();

    let child_task_2 = ImmutableTask::new_with_name("子タスク2".to_string());

    let parent_task_1 = ImmutableTask::new_with_name_children(
        "親タスク1".to_string(),
        vec![child_task_1, child_task_2],
    );

    let actual = extract_leaf_immutable_tasks_from_project(&parent_task_1);
    let expected_child_task_2 = ImmutableTask::new_with_name("子タスク2".to_string());
    let expected = vec![&expected_child_task_1, &expected_child_task_2];
    assert_eq!(actual, expected);
}

#[test]
fn test_extract_leaf_immutable_tasks_from_project_子が全てdoneのタスクで親がpendingの時は空配列を返すこと(
) {
    /*
     parent_task_1 (pending)
       - child_task_1 (done)
    */

    let child_task_1 =
        ImmutableTask::new_with_name_status_children("子タスク1".to_string(), Status::Done, vec![]);

    let pending_until = Local.with_ymd_and_hms(2037, 12, 31, 0, 0, 0).unwrap();
    let parent_task_1 = ImmutableTask::new(
        "親タスク1".to_string(),
        Status::Pending,
        pending_until,
        vec![child_task_1],
    );

    let actual = extract_leaf_immutable_tasks_from_project(&parent_task_1);
    let expected: Vec<&ImmutableTask> = vec![];
    assert_eq!(actual, expected);
}

#[test]
fn test_extract_leaf_tasks_from_project_タスクのchildrenが空配列の場合() {
    let task = new_test_task_handle("タスク").unwrap();
    let actual = extract_leaf_tasks_from_project(&task).unwrap();

    let t = new_test_task_handle("タスク").unwrap();

    let expected = vec![t];
    assert_eq!(actual, expected);
}

#[test]
fn test_extract_leaf_tasks_from_project_タスクのchildrenが空配列ではない場合は再帰して結果を返す() {
    /*
     parent_task_1
       - child_task_1
         - grand_child_task (葉)
       - child_task_2 (葉)
    */
    let mut grand_child_task_1 = new_test_task_handle("孫タスク1").unwrap();
    let ptr_to_grand_child_task_1_node = grand_child_task_1.node.clone();

    let child_task_1 = new_test_task_handle("子タスク1").unwrap();
    grand_child_task_1
        .detach_insert_as_last_child_of(child_task_1)
        .unwrap();

    let mut child_task_1_again = grand_child_task_1.root().unwrap();

    let mut child_task_2 = new_test_task_handle("子タスク2").unwrap();
    let parent_task_1 = new_test_task_handle("親タスク1").unwrap();

    child_task_1_again
        .detach_insert_as_last_child_of(parent_task_1)
        .unwrap();
    let parent_task_again = child_task_1_again.root().unwrap();
    child_task_2
        .detach_insert_as_last_child_of(parent_task_again)
        .unwrap();

    let parent_task_again_again = child_task_2.root().unwrap();

    let actual = extract_leaf_tasks_from_project(&parent_task_again_again).unwrap();
    let t1 = new_test_task_handle("孫タスク1").unwrap();
    let t2 = new_test_task_handle("子タスク2").unwrap();
    let expected = vec![t1, t2];
    assert_eq!(&actual, &expected);

    // actualの2つのノードに親子関係の情報が残っており、それらの親が同一であること
    assert_eq!(actual.len(), 2);
    let actual1 = actual.first().unwrap();
    let actual2 = actual.last().unwrap();

    assert_ne!(actual1, actual2);
    assert_eq!(
        actual1.root().unwrap().node.borrow_data().get_name(),
        "親タスク1"
    );
    assert_eq!(
        actual2.root().unwrap().node.borrow_data().get_name(),
        "親タスク1"
    );
    assert!(actual1
        .node
        .belongs_to_same_tree(&ptr_to_grand_child_task_1_node));
    assert!(actual2
        .node
        .belongs_to_same_tree(&ptr_to_grand_child_task_1_node));
}

#[test]
fn test_extract_leaf_tasks_from_project_子が全てdoneのタスクは葉として扱われること() {
    /*
     parent_task_1
       - child_task_1 (子が全てdoneなので葉として返る)
         - grand_child_task_1 (done)
         - grand_child_task_2 (done)
       - child_task_2 (返る)
    */
    let mut grand_child_task_1 = new_test_task_handle("孫タスク1").unwrap();
    grand_child_task_1.set_orig_status(Status::Done).unwrap();

    let mut grand_child_task_2 = new_test_task_handle("孫タスク2").unwrap();
    grand_child_task_2.set_orig_status(Status::Done).unwrap();

    let child_task_1 = new_test_task_handle("子タスク1").unwrap();

    grand_child_task_1
        .detach_insert_as_last_child_of(child_task_1)
        .unwrap();
    let child_task_1_again = grand_child_task_1.parent().unwrap().unwrap();
    grand_child_task_2
        .detach_insert_as_last_child_of(child_task_1_again)
        .unwrap();

    let parent_task = grand_child_task_2.root().unwrap();

    let expected_child_task_1 = TaskHandle {
        node: parent_task.node.first_child().unwrap(),
    };

    let actual = extract_leaf_tasks_from_project(&parent_task).unwrap();

    assert_eq!(actual.len(), 1);
    assert_task(actual.first().unwrap(), &expected_child_task_1);
}

#[test]
fn test_extract_leaf_tasks_from_project_子が全てdoneのタスクで親がpendingの時は空配列を返すこと() {
    /*
     parent_task_1 (pending)
       - child_task_1 (done)
    */
    let mut child_task_1 = new_test_task_handle("子タスク1").unwrap();
    child_task_1.set_orig_status(Status::Done).unwrap();

    let pending_until = Local.with_ymd_and_hms(2037, 12, 31, 0, 0, 0).unwrap();
    let parent_task_1 = new_test_task_handle("親タスク1").unwrap();
    parent_task_1.set_orig_status(Status::Pending).unwrap();
    parent_task_1.set_pending_until(pending_until).unwrap();
    child_task_1
        .detach_insert_as_last_child_of(parent_task_1)
        .unwrap();

    let root_task = child_task_1.root().unwrap();
    let actual = extract_leaf_tasks_from_project(&root_task).unwrap();
    let expected: Vec<TaskHandle> = vec![];
    assert_eq!(actual, expected);
}

#[test]
fn test_task_attr_with_identity_caller指定のidと時刻を保持する() {
    let id = uuid!("018d578c-3f3b-7bd6-9384-9b4b00d69c21");
    let now = Local.with_ymd_and_hms(2026, 8, 19, 12, 34, 56).unwrap();

    let attr = TaskAttr::with_identity("タスク", id, now);

    assert_eq!(attr.get_id(), &id);
    assert_eq!(attr.get_create_time(), &now);
    assert_eq!(attr.get_start_time(), &now);
}

#[test]
fn test_task_attr_set_status() {
    let mut attr = new_test_task_attr("タスク");
    attr.set_orig_status(Status::Done);
    let actual = attr.get_status();
    assert_eq!(actual, &Status::Done);
}

#[test]
fn test_task_attr_new_atomicはfalse() {
    let attr = new_test_task_attr("タスク");
    assert!(!attr.get_atomic());
}

#[test]
fn test_task_attr_set_pending_until() {
    let mut attr = new_test_task_attr("タスク");
    let pending_until = Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
    attr.set_pending_until(pending_until);
    let actual = attr.get_pending_until();
    assert_eq!(actual, &pending_until);
}

#[cfg(test)]
mod deadline_buffer_contract_tests {
    use super::*;

    fn local_datetime(hour: u32, minute: u32, second: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 8, 20, hour, minute, second)
            .unwrap()
    }

    fn task_at(
        now: DateTime<Local>,
        start_time: DateTime<Local>,
        deadline: DateTime<Local>,
        estimated_work_seconds: i64,
        actual_work_seconds: i64,
        pending_until: DateTime<Local>,
        orig_status: Status,
    ) -> TaskAttr {
        let mut task = TaskAttr::with_identity("task", Uuid::nil(), local_datetime(0, 0, 0));
        task.set_start_time(start_time);
        task.set_deadline_time_opt(Some(deadline));
        task.set_estimated_work_seconds(estimated_work_seconds);
        task.set_actual_work_seconds(actual_work_seconds);
        task.set_pending_until(pending_until);
        task.sync_clock(now);
        task.set_orig_status(orig_status);
        task
    }

    #[test]
    fn start前はdeadlineから見積と5分を引いた境界を超えるとtodoになる() {
        let deadline = local_datetime(12, 0, 0);
        let cutoff = local_datetime(11, 25, 0);
        let start_time = local_datetime(11, 50, 0);

        for (now, expected) in [
            (cutoff - Duration::seconds(1), Status::Pending),
            (cutoff, Status::Pending),
            (cutoff + Duration::seconds(1), Status::Todo),
        ] {
            let task = task_at(
                now,
                start_time,
                deadline,
                30 * 60,
                0,
                deadline,
                Status::Pending,
            );

            assert_eq!(task.get_status(), &expected, "now={now}");
        }
    }

    #[test]
    fn start後はdeadlineから残作業と60分を引いた境界を超えるとtodoになる() {
        let deadline = local_datetime(12, 0, 0);
        let cutoff = local_datetime(10, 30, 0);
        let start_time = local_datetime(10, 0, 0);

        for (now, expected) in [
            (cutoff - Duration::seconds(1), Status::Pending),
            (cutoff, Status::Pending),
            (cutoff + Duration::seconds(1), Status::Todo),
        ] {
            let task = task_at(
                now,
                start_time,
                deadline,
                30 * 60,
                0,
                deadline,
                Status::Pending,
            );

            assert_eq!(task.get_status(), &expected, "now={now}");
        }
    }

    #[test]
    fn start後の残作業が0でもdeadlineの60分前境界を使う() {
        let deadline = local_datetime(12, 0, 0);
        let cutoff = local_datetime(11, 0, 0);
        let start_time = local_datetime(10, 0, 0);

        for (now, expected) in [
            (cutoff - Duration::seconds(1), Status::Pending),
            (cutoff, Status::Pending),
            (cutoff + Duration::seconds(1), Status::Todo),
        ] {
            let task = task_at(
                now,
                start_time,
                deadline,
                30 * 60,
                30 * 60,
                deadline,
                Status::Pending,
            );

            assert_eq!(task.get_status(), &expected, "now={now}");
        }
    }

    #[test]
    fn pending_untilはdeadlineから見積と5分を引いた時刻より後だけ補正する() {
        let deadline = local_datetime(12, 0, 0);
        let cutoff = local_datetime(11, 25, 0);

        for (pending_until, expected) in [
            (cutoff - Duration::seconds(1), cutoff - Duration::seconds(1)),
            (cutoff, cutoff),
            (cutoff + Duration::seconds(1), cutoff),
        ] {
            let task = task_at(
                local_datetime(9, 0, 0),
                local_datetime(10, 0, 0),
                deadline,
                30 * 60,
                0,
                pending_until,
                Status::Pending,
            );

            assert_eq!(task.get_pending_until(), &expected);
        }
    }

    #[test]
    fn doneはdeadlineによるstatusとpending_untilの補正対象外である() {
        let deadline = local_datetime(12, 0, 0);
        let pending_until = deadline + Duration::hours(1);
        let task = task_at(
            local_datetime(11, 59, 0),
            local_datetime(10, 0, 0),
            deadline,
            30 * 60,
            0,
            pending_until,
            Status::Done,
        );

        assert_eq!(task.get_status(), &Status::Done);
        assert_eq!(task.get_pending_until(), &pending_until);
    }
}

#[cfg(test)]
fn test_task_time() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 8, 19, 0, 0, 0).unwrap()
}

#[cfg(test)]
fn new_test_task_attr(name: &str) -> TaskAttr {
    crate::test_support::new_task_attr_at(name, test_task_time())
}

#[cfg(test)]
fn new_test_task_handle(name: &str) -> Result<TaskHandle, TaskTreeError> {
    crate::test_support::new_task_handle_at(name, test_task_time())
}

impl TaskHandle {
    pub(crate) fn with_exclusive_data_borrow_for_test<T>(&self, action: impl FnOnce() -> T) -> T {
        let _exclusive_borrow = self.node.borrow_data_mut();
        action()
    }

    pub(crate) fn with_shared_data_borrow_for_test<T>(&self, action: impl FnOnce() -> T) -> T {
        let _shared_borrow = self.node.borrow_data();
        action()
    }

    pub(crate) fn eq_tree(&self, task: &TaskHandle) -> Result<bool, TaskTreeError> {
        self.node
            .tree()
            .try_eq(&task.node.tree())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub(crate) fn detach_insert_as_last_child_of(
        &mut self,
        parent_task: TaskHandle,
    ) -> Result<(), String> {
        self.reparent_to(&parent_task)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn create_as_last_child(&self, task_attr: TaskAttr) -> Self {
        self.create_child(task_attr)
            .expect("test hierarchy child creation must succeed")
    }

    pub(crate) fn create_as_parent(&mut self, task_attr: TaskAttr) -> Result<(), String> {
        self.create_parent(task_attr)
            .map_err(|error| error.to_string())
    }
}

#[test]
fn test_persistent_mutation_revisionはrootとchildの永続化変更で進む() {
    let root = new_test_task_handle("root").unwrap();
    let child = root.create_as_last_child(new_test_task_attr("child"));
    let initial_revision = root.get_persistent_mutation_revision().unwrap();

    child.set_estimated_work_seconds(30 * 60).unwrap();

    assert!(root.get_persistent_mutation_revision().unwrap() > initial_revision);
    assert_eq!(
        child.get_persistent_mutation_revision().unwrap(),
        root.get_persistent_mutation_revision().unwrap()
    );
}

#[test]
fn test_persistent_mutation_revisionは同じ値の設定では進まない() {
    let task = new_test_task_handle("task").unwrap();
    let initial_revision = task.get_persistent_mutation_revision().unwrap();

    task.set_estimated_work_seconds(task.get_estimated_work_seconds().unwrap())
        .unwrap();
    task.set_priority(task.get_priority().unwrap()).unwrap();

    assert_eq!(
        task.get_persistent_mutation_revision().unwrap(),
        initial_revision
    );
}

#[test]
fn test_deadline伝搬はrootのrevisionを一度だけ進める() {
    let root = new_test_task_handle("root").unwrap();
    let child = root.create_as_last_child(new_test_task_attr("child"));
    child.create_as_last_child(new_test_task_attr("grand_child"));
    let before_revision = root.get_persistent_mutation_revision().unwrap();
    let deadline = Local.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap();

    root.set_deadline_time_opt(Some(deadline)).unwrap();

    assert_eq!(root.get_deadline_time_opt().unwrap(), Some(deadline));
    assert_eq!(child.get_deadline_time_opt().unwrap(), Some(deadline));
    assert_eq!(
        root.get_persistent_mutation_revision().unwrap(),
        before_revision.wrapping_add(1)
    );
}

#[test]
fn test_persistent_mutation_revisionはtree構造変更で進む() {
    let root = new_test_task_handle("root").unwrap();
    let initial_revision = root.get_persistent_mutation_revision().unwrap();
    let child = root.create_as_last_child(new_test_task_attr("child"));
    let after_child_revision = root.get_persistent_mutation_revision().unwrap();

    root.create_sequential_children("step", 60, 1, 2, "", new_test_task_attr)
        .unwrap();
    let after_sequential_revision = root.get_persistent_mutation_revision().unwrap();
    let mut child = child;
    child
        .create_as_parent(new_test_task_attr("parent"))
        .unwrap();

    assert!(after_child_revision > initial_revision);
    assert!(after_sequential_revision > after_child_revision);
    assert!(root.get_persistent_mutation_revision().unwrap() > after_sequential_revision);
}

#[test]
fn test_persistent_mutation_revisionはclockの永続化変更だけで進む() {
    let now = Local.with_ymd_and_hms(2026, 8, 13, 12, 0, 0).unwrap();
    let unchanged = new_test_task_handle("unchanged").unwrap();
    let unchanged_revision = unchanged.get_persistent_mutation_revision().unwrap();

    unchanged.sync_clock(now).unwrap();

    assert_eq!(
        unchanged.get_persistent_mutation_revision().unwrap(),
        unchanged_revision
    );

    let adjusted = new_test_task_handle("adjusted").unwrap();
    adjusted.set_orig_status(Status::Pending).unwrap();
    adjusted
        .set_pending_until(now + Duration::days(10))
        .unwrap();
    adjusted
        .set_deadline_time_opt(Some(now + Duration::hours(2)))
        .unwrap();
    let before_sync_revision = adjusted.get_persistent_mutation_revision().unwrap();
    let before_sync_pending_until = adjusted.get_pending_until().unwrap();

    adjusted.sync_clock(now).unwrap();

    assert!(adjusted.get_pending_until().unwrap() < before_sync_pending_until);
    assert!(adjusted.get_persistent_mutation_revision().unwrap() > before_sync_revision);
}

#[test]
fn test_task_handle_with_identity_caller指定のidと時刻を保持しdummy_rootだけnil_idにする() {
    let id = uuid!("018d578c-3f3b-7bd6-9384-9b4b00d69c22");
    let now = Local.with_ymd_and_hms(2026, 8, 19, 12, 34, 56).unwrap();

    let task = TaskHandle::with_identity("タスク", id, now).unwrap();

    assert_eq!(task.get_id().unwrap(), id);
    assert_eq!(task.get_create_time().unwrap(), now);
    assert_eq!(task.get_start_time().unwrap(), now);

    let dummy_root = task.node.root();
    let dummy_attr = dummy_root.borrow_data();
    assert_eq!(dummy_attr.get_id(), &Uuid::nil());
    assert_eq!(dummy_attr.get_create_time(), &now);
    assert_eq!(dummy_attr.get_start_time(), &now);
}

#[test]
fn test_task_new_タスクを初期化した時に見ているノードはダミーrootノードではないこと() {
    let task = new_test_task_handle("親タスク").unwrap();
    assert_eq!(task.node.borrow_data().get_name(), "親タスク");
    assert!(!task.node.is_root());
}

#[test]
fn test_new_with_node_タスク化したnodeの親子関係が維持されること() {
    let parent_task = new_test_task_handle("親タスク").unwrap();
    let parent_task_node_ptr = parent_task.node.clone();

    let mut child_task = new_test_task_handle("子タスク").unwrap();
    child_task.create_as_last_child(new_test_task_attr("孫タスク"));

    child_task
        .detach_insert_as_last_child_of(parent_task)
        .unwrap();

    let grand_children_task_node = child_task.node.first_child().unwrap();
    let new_grand_children_task = TaskHandle {
        node: grand_children_task_node.clone(),
    };
    assert_eq!(
        new_grand_children_task
            .root()
            .unwrap()
            .node
            .borrow_data()
            .get_name(),
        "親タスク"
    );

    assert!(&parent_task_node_ptr.belongs_to_same_tree(&grand_children_task_node));
}

#[test]
fn test_make_appointment_正常系1() {
    let root_task = new_test_task_handle("MTGが完了した状態").unwrap();
    let task = root_task.create_as_last_child(new_test_task_attr("MTG"));

    task.set_estimated_work_seconds(3600).unwrap();
    let appointment_start_time = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let before_revision = root_task.get_persistent_mutation_revision().unwrap();

    task.make_appointment(appointment_start_time).unwrap();

    assert_eq!(
        &task.get_start_time().unwrap(),
        &Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap()
    );
    assert_eq!(
        &task.get_deadline_time_opt().unwrap(),
        &Some(Local.with_ymd_and_hms(2023, 5, 19, 2, 23, 45).unwrap())
    );
    assert!(task.get_fixed_start().unwrap());
    assert_eq!(
        root_task.get_persistent_mutation_revision().unwrap(),
        before_revision + 1
    );
}

#[test]
fn test_make_appointmentは完了済みtaskから子孫へdeadlineを伝搬しない() {
    let root = new_test_task_handle("完了済み予定").unwrap();
    let child = root.create_child(new_test_task_attr("未完了の子")).unwrap();
    root.set_orig_status(Status::Done).unwrap();
    root.set_estimated_work_seconds(60 * 60).unwrap();
    let original_child_deadline = Local.with_ymd_and_hms(2026, 8, 22, 18, 0, 0).unwrap();
    child
        .set_deadline_time_opt(Some(original_child_deadline))
        .unwrap();
    let appointment_start = Local.with_ymd_and_hms(2026, 8, 21, 9, 0, 0).unwrap();

    root.make_appointment(appointment_start).unwrap();

    assert_eq!(
        child.get_deadline_time_opt().unwrap(),
        Some(original_child_deadline)
    );
    assert!(root.get_fixed_start().unwrap());
}

#[test]
fn test_set_flexible_start_timeは開始時刻とfixed_startを1回の更新で変更する() {
    let task = new_test_task_handle("通常task").unwrap();
    task.set_fixed_start(true).unwrap();
    let start_time = Local.with_ymd_and_hms(2026, 8, 21, 9, 0, 0).unwrap();
    let before_revision = task.get_persistent_mutation_revision().unwrap();

    task.set_flexible_start_time(start_time).unwrap();

    assert_eq!(task.get_start_time().unwrap(), start_time);
    assert!(!task.get_fixed_start().unwrap());
    assert_eq!(
        task.get_persistent_mutation_revision().unwrap(),
        before_revision + 1
    );
}

#[test]
fn test_set_flexible_start_timeは借用競合時に部分更新とrevision更新をしない() {
    let task = new_test_task_handle("通常task").unwrap();
    task.set_fixed_start(true).unwrap();
    let before_snapshot = task.snapshot().unwrap();
    let before_revision = task.get_persistent_mutation_revision().unwrap();
    let new_start_time = Local.with_ymd_and_hms(2026, 8, 21, 9, 0, 0).unwrap();

    let actual = task.with_shared_data_borrow_for_test(|| {
        task.set_flexible_start_time(new_start_time)
    });

    assert_eq!(actual, Err(TaskTreeError::Borrow));
    assert_eq!(task.snapshot().unwrap(), before_snapshot);
    assert_eq!(
        task.get_persistent_mutation_revision().unwrap(),
        before_revision
    );
}

#[test]
fn test_new_detach_insert_as_last_child_of_正常系1() {
    let parent_task = new_test_task_handle("親タスク").unwrap();
    let mut child_task = new_test_task_handle("子タスク").unwrap();
    let parent_task_ptr = parent_task.node.clone();
    let child_task_ptr = child_task.node.clone();

    child_task
        .detach_insert_as_last_child_of(parent_task)
        .unwrap();
    assert_eq!(
        *child_task.node.borrow_data(),
        new_test_task_attr("子タスク")
    );
    assert_eq!(
        *child_task.root().unwrap().node.borrow_data(),
        new_test_task_attr("親タスク")
    );

    assert!(child_task.node.belongs_to_same_tree(&parent_task_ptr));
    assert!(child_task.node.belongs_to_same_tree(&child_task_ptr));
}

#[test]
fn test_create_as_last_child_正常系1() {
    let actual_task = new_test_task_handle("親タスク").unwrap();
    actual_task.create_as_last_child(new_test_task_attr("子タスク"));

    let expected_tree = tree! {
    new_test_task_attr("dummy-for-親タスク"), [
        /(new_test_task_attr("親タスク"), [
            new_test_task_attr("子タスク")
        ])
    ]
    };

    assert_task_and_tree(&actual_task, &expected_tree);
}

#[test]
fn test_create_as_parent_正常系1() {
    let actual_task = new_test_task_handle("親タスク").unwrap();
    let mut child_task = actual_task.create_as_last_child(new_test_task_attr("子タスク"));
    child_task
        .create_as_parent(new_test_task_attr("中タスク"))
        .ok();

    let expected_tree = tree! {
    new_test_task_attr("dummy-for-親タスク"), [
        /(new_test_task_attr("親タスク"), [
            /(new_test_task_attr("中タスク"), [
                new_test_task_attr("子タスク")
            ])
        ])
    ]
    };

    assert_task_and_tree(&actual_task, &expected_tree);
}

#[test]
fn test_try_create_parentは子を親の直下に残さず挿入する() {
    let root = new_test_task_handle("root").unwrap();
    let mut child = root.create_child(new_test_task_attr("child")).unwrap();

    child.create_parent(new_test_task_attr("parent")).unwrap();

    let parent = child.parent().unwrap().unwrap();
    assert_eq!(parent.get_name().unwrap(), "parent");
    assert_eq!(
        parent.parent().unwrap().unwrap().get_name().unwrap(),
        "root"
    );
}

#[test]
fn test_try_create_parentはhierarchy_grant取得失敗時にtreeとrevisionを変更しない() {
    let root = new_test_task_handle("root").unwrap();
    let mut child = root.create_child(new_test_task_attr("child")).unwrap();
    let before_snapshot = root.snapshot();
    let before_revision = root.get_persistent_mutation_revision();
    let hierarchy_edit_prohibition = root
        .node
        .tree()
        .prohibit_hierarchy_edit()
        .expect("test hierarchy edit prohibition");

    let actual = child.create_parent(new_test_task_attr("parent"));

    assert_eq!(actual, Err(TaskTreeError::HierarchyGrant));
    assert_eq!(root.snapshot(), before_snapshot);
    assert_eq!(root.get_persistent_mutation_revision(), before_revision);
    drop(hierarchy_edit_prohibition);
}

#[test]
fn test_try_create_sequential_childrenは不正な範囲でtreeとrevisionを変更しない() {
    let root = new_test_task_handle("root").unwrap();
    let before_snapshot = root.snapshot();
    let before_revision = root.get_persistent_mutation_revision();

    let actual = root.create_sequential_children("step", 60, 2, 1, "", new_test_task_attr);

    assert_eq!(actual, Err(TaskTreeError::InvalidSequence));
    assert_eq!(root.snapshot(), before_snapshot);
    assert_eq!(root.get_persistent_mutation_revision(), before_revision);
}

#[test]
fn test_get_inherited_repetition_interval_days_opt_直接の親の値を返す() {
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task
        .set_repetition_interval_days_opt(Some(7))
        .unwrap();
    let child_task = parent_task.create_as_last_child(new_test_task_attr("子タスク"));

    assert_eq!(
        child_task
            .get_inherited_repetition_interval_days_opt()
            .unwrap(),
        Some(7)
    );
}

#[test]
fn test_get_inherited_repetition_interval_days_opt_祖父の値を返す() {
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task
        .set_repetition_interval_days_opt(Some(7))
        .unwrap();
    let child_task = parent_task.create_as_last_child(new_test_task_attr("子タスク"));
    let grand_child_task = child_task.create_as_last_child(new_test_task_attr("孫タスク"));

    assert_eq!(
        grand_child_task
            .get_inherited_repetition_interval_days_opt()
            .unwrap(),
        Some(7)
    );
}

#[test]
fn test_get_inherited_repetition_interval_days_opt_祖先に値がなければ_noneを返す() {
    let parent_task = new_test_task_handle("親タスク").unwrap();
    let child_task = parent_task.create_as_last_child(new_test_task_attr("子タスク"));
    let grand_child_task = child_task.create_as_last_child(new_test_task_attr("孫タスク"));

    assert_eq!(
        grand_child_task
            .get_inherited_repetition_interval_days_opt()
            .unwrap(),
        None
    );
}

#[test]
fn test_get_inherited_repetition_interval_days_opt_自分自身の値は見ない() {
    let parent_task = new_test_task_handle("親タスク").unwrap();
    let child_task = parent_task.create_as_last_child(new_test_task_attr("子タスク"));
    child_task
        .set_repetition_interval_days_opt(Some(7))
        .unwrap();

    assert_eq!(
        child_task
            .get_inherited_repetition_interval_days_opt()
            .unwrap(),
        None
    );
}

#[test]
fn test_get_inherited_repetition_interval_days_opt_最も近い祖先の値を返す() {
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task
        .set_repetition_interval_days_opt(Some(30))
        .unwrap();
    let child_task = parent_task.create_as_last_child(new_test_task_attr("子タスク"));
    child_task
        .set_repetition_interval_days_opt(Some(7))
        .unwrap();
    let grand_child_task = child_task.create_as_last_child(new_test_task_attr("孫タスク"));

    assert_eq!(
        grand_child_task
            .get_inherited_repetition_interval_days_opt()
            .unwrap(),
        Some(7)
    );
}

#[test]
fn test_create_sequential_children_正常系1() {
    let task = new_test_task_handle("親タスク").unwrap();
    let grand_child_task_result =
        task.create_sequential_children("鎖タスク", 600, 1, 2, "話", new_test_task_attr);

    let mut child_attr = new_test_task_attr("鎖タスク 2話");
    child_attr.set_estimated_work_seconds(600);

    let mut grand_child_attr = new_test_task_attr("鎖タスク 1話");
    grand_child_attr.set_estimated_work_seconds(600);

    let expected_tree = tree! {
        new_test_task_attr("dummy-for-親タスク"), [
            /(new_test_task_attr("親タスク"), [
                /(child_attr, [
                    /(grand_child_attr, [])
                ])
            ])
        ]
    };

    match grand_child_task_result {
        Ok(grand_child_task) => {
            assert_task_and_tree(&grand_child_task, &expected_tree);
        }
        _ => panic!("create_sequential_children must succeed"),
    }
}

#[test]
#[allow(non_snake_case)]
fn test_create_sequential_children_異常系1_begin_indexのほうが大きい場合はエラー() {
    let task = new_test_task_handle("親タスク").unwrap();
    let grand_child_task_result =
        task.create_sequential_children("鎖タスク", 600, 10, 1, "話", new_test_task_attr);

    assert!(grand_child_task_result.is_err());
}

#[cfg(test)]
fn get_tree_for_assert_debug(task1: &TaskHandle, task2: &TaskHandle) -> String {
    format!(
        "actual and expected are not equal:\n\n=== [actual] ===\n{}\n\n=== [expected] ===\n{}\n\n",
        task1
            .tree_debug_pretty_print()
            .expect("data are not borrowed"),
        task2
            .tree_debug_pretty_print()
            .expect("data are not borrowed"),
    )
}

#[cfg(test)]
pub fn assert_task(task1: &TaskHandle, task2: &TaskHandle) {
    let str_for_debug_string: String = get_tree_for_assert_debug(task1, task2);

    assert!(
        &task1.eq_tree(task2).expect("data are not borrowed"),
        "{}",
        str_for_debug_string.as_str()
    );
}

#[cfg(test)]
fn get_task_tree_for_assert_debug(task1: &TaskHandle, tree: &Tree<TaskAttr>) -> String {
    format!(
        "actual and expected are not equal:\n\n=== [actual] ===\n{}\n\n=== [expected] ===\n{:?}\n\n",
        task1.tree_debug_pretty_print().expect("data are not borrowed"),
        tree.debug_pretty_print(),
    )
}

#[cfg(test)]
pub fn assert_task_and_tree(task1: &TaskHandle, tree: &Tree<TaskAttr>) {
    let str_for_debug_string: String = get_task_tree_for_assert_debug(task1, tree);

    assert!(
        &task1
            .node
            .tree()
            .try_eq(tree)
            .expect("data are not borrowed"),
        "{}",
        str_for_debug_string.as_str()
    );
}


#[test]
fn test_get_by_id_ベースケース() {
    let mut task = new_test_task_handle("親タスク1").unwrap();
    task.set_orig_status(Status::Pending).unwrap();
    task.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap())
        .unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();

    let task_ptr = &task.node;

    let actual_opt = task.get_by_id(id).unwrap();
    match actual_opt {
        Some(actual) => {
            assert_eq!(&actual, &task);
            assert!(&actual.node.ptr_eq(task_ptr));
        }
        None => panic!("task ID must be found"),
    }
}

#[test]
fn test_get_by_id_子なしタスクでヒットしなかった場合() {
    let mut task = new_test_task_handle("親タスク1").unwrap();
    task.set_orig_status(Status::Pending).unwrap();
    task.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap())
        .unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();

    let actual = task
        .get_by_id(uuid!("ccdadeab-f60a-4bec-93f8-3d7e003b980f"))
        .unwrap();

    assert_eq!(actual, None);
}

#[test]
fn test_get_by_id_再帰でヒットする場合() {
    let mut task = new_test_task_handle("親タスク1").unwrap();
    task.set_orig_status(Status::Pending).unwrap();
    task.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap())
        .unwrap();
    let id_parent: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id_parent).unwrap();

    let mut task_attr_child_1 = new_test_task_attr("子タスク1");
    task_attr_child_1.set_orig_status(Status::Pending);
    task_attr_child_1.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    let id_child_1: Uuid = uuid!("0aaee735-3e22-4216-8b59-d56d5caf29ee");
    task_attr_child_1.set_id(id_child_1);

    let mut task_attr_child_2 = new_test_task_attr("子タスク2");
    task_attr_child_2.set_orig_status(Status::Pending);
    task_attr_child_2.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    let id_child_2: Uuid = uuid!("7ffcba2f-80e0-4a44-aee9-d68e0d2d1256");
    task_attr_child_2.set_id(id_child_2);

    let expected_attr = task_attr_child_1.clone();

    task.create_as_last_child(task_attr_child_1);
    task.create_as_last_child(task_attr_child_2);

    let actual_opt = task.get_by_id(id_child_1).unwrap();
    match actual_opt {
        None => {
            panic!("assert some");
        }
        Some(actual) => {
            assert_eq!(&actual.get_attr().unwrap(), &expected_attr);

            // 親をたどることができること
            assert_eq!(&actual.root().unwrap(), &task);
        }
    }
}

#[test]
fn test_get_by_id_再帰でヒットしない場合() {
    let mut task = new_test_task_handle("親タスク1").unwrap();
    task.set_orig_status(Status::Pending).unwrap();
    task.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap())
        .unwrap();
    let id_parent: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id_parent).unwrap();

    let mut task_attr_child_1 = new_test_task_attr("子タスク1");
    task_attr_child_1.set_orig_status(Status::Pending);
    task_attr_child_1.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    let id_child_1: Uuid = uuid!("0aaee735-3e22-4216-8b59-d56d5caf29ee");
    task_attr_child_1.set_id(id_child_1);

    let mut task_attr_child_2 = new_test_task_attr("子タスク2");
    task_attr_child_2.set_orig_status(Status::Pending);
    task_attr_child_2.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    let id_child_2: Uuid = uuid!("7ffcba2f-80e0-4a44-aee9-d68e0d2d1256");
    task_attr_child_2.set_id(id_child_2);

    task.create_as_last_child(task_attr_child_1);
    task.create_as_last_child(task_attr_child_2);

    let actual = task
        .get_by_id(uuid!("3aa89504-917d-4f20-a1e3-4eb196190c6f"))
        .unwrap();
    assert_eq!(actual, None);
}

#[test]
fn test_all_sibling_tasks_are_all_done_全ての兄弟タスクが完了していたらtrueとなる() {
    /*
     parent_task_1
       - child_task_1 (完了)
       - child_task_2 (完了)
    */

    let parent_task = new_test_task_handle("親タスク").unwrap();

    let mut task_attr_child_1 = new_test_task_attr("子タスク1");
    task_attr_child_1.set_orig_status(Status::Done);

    let mut task_attr_child_2 = new_test_task_attr("子タスク2");
    task_attr_child_2.set_orig_status(Status::Done);

    let child_task_1 = parent_task.create_as_last_child(task_attr_child_1);
    parent_task.create_as_last_child(task_attr_child_2);

    assert!(child_task_1.all_sibling_tasks_are_all_done().unwrap());
}

#[test]
fn test_all_sibling_tasks_are_all_done_一部の兄弟タスクが完了でない場合はfalseとなる() {
    /*
     parent_task_1
       - child_task_1 (完了)
       - child_task_2 (Todo)
    */

    let parent_task = new_test_task_handle("親タスク").unwrap();

    let mut task_attr_child_1 = new_test_task_attr("子タスク1");
    task_attr_child_1.set_orig_status(Status::Done);

    let mut task_attr_child_2 = new_test_task_attr("子タスク2");
    task_attr_child_2.set_orig_status(Status::Todo);

    let child_task_1 = parent_task.create_as_last_child(task_attr_child_1);
    parent_task.create_as_last_child(task_attr_child_2);

    assert!(!child_task_1.all_sibling_tasks_are_all_done().unwrap());
}

#[test]
fn test_has_undone_children_子が存在しない場合はfalseとなる() {
    let task = new_test_task_handle("親タスク").unwrap();

    assert!(!task.has_undone_children().unwrap());
}

#[test]
fn test_has_undone_children_全ての子が完了済みの場合はfalseとなる() {
    let parent_task = new_test_task_handle("親タスク").unwrap();

    let mut task_attr_child_1 = new_test_task_attr("子タスク1");
    task_attr_child_1.set_orig_status(Status::Done);

    let mut task_attr_child_2 = new_test_task_attr("子タスク2");
    task_attr_child_2.set_orig_status(Status::Done);

    parent_task.create_as_last_child(task_attr_child_1);
    parent_task.create_as_last_child(task_attr_child_2);

    assert!(!parent_task.has_undone_children().unwrap());
}

#[test]
fn test_has_undone_children_未完了の子がある場合はtrueとなる() {
    let parent_task = new_test_task_handle("親タスク").unwrap();

    let mut task_attr_child_1 = new_test_task_attr("子タスク1");
    task_attr_child_1.set_orig_status(Status::Done);

    let mut task_attr_child_2 = new_test_task_attr("子タスク2");
    task_attr_child_2.set_orig_status(Status::Pending);

    parent_task.create_as_last_child(task_attr_child_1);
    parent_task.create_as_last_child(task_attr_child_2);

    assert!(parent_task.has_undone_children().unwrap());
}

#[test]
fn test_parent_ルートタスクの場合() {
    /*
     parent_task_1
    */

    let parent_task = new_test_task_handle("親タスク").unwrap();
    assert_eq!(parent_task.parent().unwrap(), None);
}

#[test]
fn test_parent_親タスクがある場合() {
    /*
     parent_task_1
       - child_task_1
    */

    let parent_task = new_test_task_handle("親タスク").unwrap();

    let task_attr_child_1 = new_test_task_attr("子タスク1");
    let child_task_1 = parent_task.create_as_last_child(task_attr_child_1);

    match child_task_1.parent().unwrap() {
        Some(actual_task) => {
            assert_task(&actual_task, &parent_task);
        }
        None => panic!("child task must have its parent"),
    }
}

#[test]
fn test_taskをcloneした場合はnodeは同じ木を指すポインタであること() {
    let task_orig = new_test_task_handle("タスク").unwrap();
    let task_cloned = task_orig.clone();

    assert!(&task_orig.node.ptr_eq(&task_cloned.node));
}

#[test]
fn test_task_handle_snapshotは独立した読み取り値を返す() {
    let root = new_test_task_handle("親").unwrap();
    let child = root.create_child(new_test_task_attr("子")).unwrap();
    child.set_estimated_work_seconds(60).unwrap();

    let snapshot = root.snapshot().unwrap();
    child.set_estimated_work_seconds(120).unwrap();

    assert_eq!(snapshot.name(), "親");
    assert_eq!(snapshot.children()[0].estimated_work_seconds(), 60);
    assert_eq!(
        snapshot.children()[0].attr().get_id(),
        child.get_attr().unwrap().get_id()
    );
    assert_eq!(
        snapshot.children()[0].attr().get_pending_until(),
        child.get_attr().unwrap().get_pending_until()
    );
    assert_eq!(
        root.get_children().unwrap()[0]
            .get_estimated_work_seconds()
            .unwrap(),
        120
    );
}

#[test]
fn test_try_snapshotは借用競合をerrorとして返す() {
    let task = new_test_task_handle("タスク").unwrap();
    let _exclusive_borrow = task.node.borrow_data_mut();

    assert_eq!(task.snapshot(), Err(TaskTreeError::Borrow));
}

#[test]
fn test_task_handleの公開read_apiは借用競合をerrorとして返す() {
    let task = new_test_task_handle("タスク").unwrap();
    let _exclusive_borrow = task.node.borrow_data_mut();

    assert_eq!(task.get_name(), Err(TaskTreeError::Borrow));
}

#[test]
fn test_task_handleのcore_read_apiは借用競合をerrorとして返す() {
    let task = new_test_task_handle("タスク").unwrap();
    let _exclusive_borrow = task.node.borrow_data_mut();

    assert_eq!(task.get_attr(), Err(TaskTreeError::Borrow));
    assert_eq!(task.get_id(), Err(TaskTreeError::Borrow));
    assert_eq!(task.get_status(), Err(TaskTreeError::Borrow));
    assert_eq!(task.get_children(), Err(TaskTreeError::Borrow));
    assert_eq!(task.num_children(), Err(TaskTreeError::Borrow));
    assert_eq!(task.snapshot(), Err(TaskTreeError::Borrow));
}

#[test]
fn test_rootはdummy_rootの子が欠落した場合にinvariant_errorを返す() {
    let task = new_test_task_handle("タスク").unwrap();
    let dummy_root = task.node.root();
    let grant = dummy_root.tree().grant_hierarchy_edit().unwrap();
    task.node.detach_subtree(&grant);

    assert_eq!(task.root(), Err(TaskTreeError::MissingDummyRootChild));
}

#[test]
fn test_rootはdummy_rootに複数の子がある場合にinvariant_errorを返す() {
    let first_task = new_test_task_handle("第一プロジェクト").unwrap();
    first_task.set_priority(10).unwrap();
    first_task
        .set_project_category_opt(Some(ProjectCategory::Earning))
        .unwrap();
    let first_revision = first_task.get_persistent_mutation_revision().unwrap();
    let dummy_root = first_task.node.root();
    let grant = dummy_root.tree().grant_hierarchy_edit().unwrap();
    let second_task = TaskHandle {
        node: dummy_root.create_as_last_child(&grant, new_test_task_attr("第二プロジェクト")),
    };
    let second_revision = second_task.node.borrow_data().persistent_mutation_revision;

    assert_eq!(
        second_task.root(),
        Err(TaskTreeError::MissingDummyRootChild)
    );
    assert_eq!(
        second_task.get_priority(),
        Err(TaskTreeError::MissingDummyRootChild)
    );
    assert_eq!(
        second_task.get_project_category_opt(),
        Err(TaskTreeError::MissingDummyRootChild)
    );
    assert_eq!(
        second_task.set_priority(20),
        Err(TaskTreeError::MissingDummyRootChild)
    );
    assert_eq!(
        second_task.set_project_category_opt(Some(ProjectCategory::Recovery)),
        Err(TaskTreeError::MissingDummyRootChild)
    );
    assert_eq!(first_task.node.borrow_data().get_priority(), 10);
    assert_eq!(
        first_task.node.borrow_data().get_project_category_opt(),
        Some(ProjectCategory::Earning)
    );
    assert_eq!(
        first_task.node.borrow_data().persistent_mutation_revision,
        first_revision
    );
    assert_eq!(
        second_task.node.borrow_data().persistent_mutation_revision,
        second_revision
    );
}

#[test]
fn test_rootはhandleがdummy_root自身を指す場合にinvariant_errorを返す() {
    let task = new_test_task_handle("タスク").unwrap();
    let invalid_handle = TaskHandle {
        node: task.node.root(),
    };

    assert_eq!(
        invalid_handle.root(),
        Err(TaskTreeError::MissingDummyRootChild)
    );
    assert_eq!(
        invalid_handle.get_priority(),
        Err(TaskTreeError::MissingDummyRootChild)
    );
}

#[test]
fn test_task_viewは借用競合をtask_tree_errorとして返す() {
    let task = new_test_task_handle("タスク").unwrap();
    let _exclusive_borrow = task.node.borrow_data_mut();

    assert_eq!(
        crate::application::task_use_case::TaskView::try_from(&task),
        Err(TaskTreeError::Borrow)
    );
}

#[test]
fn test_reparent_toは循環をerrorにして木とrevisionを変更しない() {
    let root = new_test_task_handle("親").unwrap();
    let mut child = root.create_child(new_test_task_attr("子")).unwrap();
    let grandchild = child.create_child(new_test_task_attr("孫")).unwrap();
    let before_root_snapshot = root.snapshot();
    let before_root_revision = root.get_persistent_mutation_revision();

    let actual = child.reparent_to(&grandchild);

    assert_eq!(actual, Err(TaskTreeError::Cycle));
    assert_eq!(root.snapshot(), before_root_snapshot);
    assert_eq!(
        root.get_persistent_mutation_revision(),
        before_root_revision
    );
    assert_eq!(
        child.parent().unwrap().unwrap().get_id().unwrap(),
        root.get_id().unwrap()
    );
}

#[test]
fn test_deadline伝搬は子の借用競合時に部分更新とrevision更新をしない() {
    let root = new_test_task_handle("親").unwrap();
    let child = root.create_child(new_test_task_attr("子")).unwrap();
    let deadline = Local.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap();
    let before_revision = root.get_persistent_mutation_revision().unwrap();
    let exclusive_borrow = child.node.borrow_data_mut();

    assert_eq!(
        root.set_deadline_time_opt(Some(deadline)),
        Err(TaskTreeError::Borrow)
    );
    drop(exclusive_borrow);

    assert_eq!(root.get_deadline_time_opt().unwrap(), None);
    assert_eq!(child.get_deadline_time_opt().unwrap(), None);
    assert_eq!(
        root.get_persistent_mutation_revision().unwrap(),
        before_revision
    );
}

#[test]
fn test_deadline伝搬は子の共有借用競合時に部分更新とrevision更新をしない() {
    let root = new_test_task_handle("親").unwrap();
    let child = root.create_child(new_test_task_attr("子")).unwrap();
    let deadline = Local.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap();
    let before_snapshot = root.snapshot().unwrap();
    let before_revision = root.get_persistent_mutation_revision().unwrap();

    let actual =
        child.with_shared_data_borrow_for_test(|| root.set_deadline_time_opt(Some(deadline)));

    assert_eq!(actual, Err(TaskTreeError::Borrow));
    assert_eq!(root.snapshot().unwrap(), before_snapshot);
    assert_eq!(
        root.get_persistent_mutation_revision().unwrap(),
        before_revision
    );
}

#[test]
fn test_make_appointmentは子の共有借用競合時に部分更新とrevision更新をしない() {
    let root = new_test_task_handle("親").unwrap();
    let child = root.create_child(new_test_task_attr("子")).unwrap();
    root.set_estimated_work_seconds(30 * 60).unwrap();
    root.set_deadline_time_opt(Some(Local.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap()))
        .unwrap();
    let appointment_start_time = Local.with_ymd_and_hms(2026, 8, 14, 9, 0, 0).unwrap();
    let before_snapshot = root.snapshot().unwrap();
    let before_revision = root.get_persistent_mutation_revision().unwrap();

    let actual =
        child.with_shared_data_borrow_for_test(|| root.make_appointment(appointment_start_time));

    assert_eq!(actual, Err(TaskTreeError::Borrow));
    assert_eq!(root.snapshot().unwrap(), before_snapshot);
    assert_eq!(
        root.get_persistent_mutation_revision().unwrap(),
        before_revision
    );
}

#[test]
fn test_deadline伝搬はrootのshared_borrow競合時に全属性とtreeとrevisionを変更しない() {
    let root = new_test_task_handle("親").unwrap();
    let child = root.create_child(new_test_task_attr("子")).unwrap();
    let deadline = Local.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap();
    let before_snapshot = root.snapshot().unwrap();
    let before_revision = root.get_persistent_mutation_revision().unwrap();

    root.with_shared_data_borrow_for_test(|| {
        assert_eq!(
            root.set_deadline_time_opt(Some(deadline)),
            Err(TaskTreeError::Borrow)
        );
    });

    assert_eq!(root.snapshot().unwrap(), before_snapshot);
    assert_eq!(
        root.get_start_time().unwrap(),
        before_snapshot.attr.get_start_time().to_owned()
    );
    assert_eq!(
        child.get_start_time().unwrap(),
        before_snapshot.children[0].attr.get_start_time().to_owned()
    );
    assert_eq!(
        root.get_persistent_mutation_revision().unwrap(),
        before_revision
    );
}

#[test]
fn test_deadline伝搬は子のshared_borrow競合時に全属性とtreeとrevisionを変更しない() {
    let root = new_test_task_handle("親").unwrap();
    let child = root.create_child(new_test_task_attr("子")).unwrap();
    let deadline = Local.with_ymd_and_hms(2026, 8, 15, 12, 0, 0).unwrap();
    let before_snapshot = root.snapshot().unwrap();
    let before_revision = root.get_persistent_mutation_revision().unwrap();

    child.with_shared_data_borrow_for_test(|| {
        assert_eq!(
            root.set_deadline_time_opt(Some(deadline)),
            Err(TaskTreeError::Borrow)
        );
    });

    assert_eq!(root.snapshot().unwrap(), before_snapshot);
    assert_eq!(
        root.get_start_time().unwrap(),
        before_snapshot.attr.get_start_time().to_owned()
    );
    assert_eq!(
        child.get_start_time().unwrap(),
        before_snapshot.children[0].attr.get_start_time().to_owned()
    );
    assert_eq!(
        root.get_persistent_mutation_revision().unwrap(),
        before_revision
    );
}

#[test]
fn test_make_appointmentはrootのshared_borrow競合時に全属性とtreeとrevisionを変更しない() {
    let root = new_test_task_handle("親").unwrap();
    let child = root.create_child(new_test_task_attr("子")).unwrap();
    let appointment_start_time = Local.with_ymd_and_hms(2026, 8, 15, 9, 0, 0).unwrap();
    let before_snapshot = root.snapshot().unwrap();
    let before_revision = root.get_persistent_mutation_revision().unwrap();

    root.with_shared_data_borrow_for_test(|| {
        assert_eq!(
            root.make_appointment(appointment_start_time),
            Err(TaskTreeError::Borrow)
        );
    });

    assert_eq!(root.snapshot().unwrap(), before_snapshot);
    assert_eq!(
        root.get_start_time().unwrap(),
        before_snapshot.attr.get_start_time().to_owned()
    );
    assert_eq!(
        child.get_start_time().unwrap(),
        before_snapshot.children[0].attr.get_start_time().to_owned()
    );
    assert_eq!(
        root.get_persistent_mutation_revision().unwrap(),
        before_revision
    );
}

#[test]
fn test_make_appointmentは子のshared_borrow競合時に全属性とtreeとrevisionを変更しない() {
    let root = new_test_task_handle("親").unwrap();
    let child = root.create_child(new_test_task_attr("子")).unwrap();
    let appointment_start_time = Local.with_ymd_and_hms(2026, 8, 15, 9, 0, 0).unwrap();
    let before_snapshot = root.snapshot().unwrap();
    let before_revision = root.get_persistent_mutation_revision().unwrap();

    child.with_shared_data_borrow_for_test(|| {
        assert_eq!(
            root.make_appointment(appointment_start_time),
            Err(TaskTreeError::Borrow)
        );
    });

    assert_eq!(root.snapshot().unwrap(), before_snapshot);
    assert_eq!(
        root.get_start_time().unwrap(),
        before_snapshot.attr.get_start_time().to_owned()
    );
    assert_eq!(
        child.get_start_time().unwrap(),
        before_snapshot.children[0].attr.get_start_time().to_owned()
    );
    assert_eq!(
        root.get_persistent_mutation_revision().unwrap(),
        before_revision
    );
}

#[test]
fn test_tree追加はrootの借用競合時にtreeとrevisionを変更しない() {
    let root = new_test_task_handle("親").unwrap();
    let before_revision = root.get_persistent_mutation_revision().unwrap();
    let exclusive_borrow = root.node.borrow_data_mut();

    assert_eq!(
        root.create_child(new_test_task_attr("子")),
        Err(TaskTreeError::Borrow)
    );
    drop(exclusive_borrow);

    assert_eq!(root.num_children().unwrap(), 0);
    assert_eq!(
        root.get_persistent_mutation_revision().unwrap(),
        before_revision
    );
}

#[test]
fn test_updateはrootのshared_borrow競合時に属性とrevisionを変更しない() {
    let root = new_test_task_handle("親").unwrap();
    let child = root.create_child(new_test_task_attr("子")).unwrap();
    let before_snapshot = root.snapshot().unwrap();
    let before_revision = root.get_persistent_mutation_revision().unwrap();

    root.with_shared_data_borrow_for_test(|| {
        assert_eq!(child.set_atomic(true), Err(TaskTreeError::Borrow));
    });

    assert_eq!(root.snapshot().unwrap(), before_snapshot);
    assert_eq!(
        root.get_persistent_mutation_revision().unwrap(),
        before_revision
    );
}

#[test]
fn test_create_childはrootのshared_borrow競合時にtreeとrevisionを変更しない() {
    let root = new_test_task_handle("親").unwrap();
    let before_snapshot = root.snapshot().unwrap();
    let before_revision = root.get_persistent_mutation_revision().unwrap();

    root.with_shared_data_borrow_for_test(|| {
        assert_eq!(
            root.create_child(new_test_task_attr("子")),
            Err(TaskTreeError::Borrow)
        );
    });

    assert_eq!(root.snapshot().unwrap(), before_snapshot);
    assert_eq!(
        root.get_persistent_mutation_revision().unwrap(),
        before_revision
    );
}

#[test]
fn test_create_parentはrootのshared_borrow競合時にtreeとrevisionを変更しない() {
    let root = new_test_task_handle("親").unwrap();
    let mut child = root.create_child(new_test_task_attr("子")).unwrap();
    let before_snapshot = root.snapshot().unwrap();
    let before_revision = root.get_persistent_mutation_revision().unwrap();

    root.with_shared_data_borrow_for_test(|| {
        assert_eq!(
            child.create_parent(new_test_task_attr("新しい親")),
            Err(TaskTreeError::Borrow)
        );
    });

    assert_eq!(root.snapshot().unwrap(), before_snapshot);
    assert_eq!(
        root.get_persistent_mutation_revision().unwrap(),
        before_revision
    );
}

#[test]
fn test_create_sequential_childrenはrootのshared_borrow競合時にtreeとrevisionを変更しない() {
    let root = new_test_task_handle("親").unwrap();
    let before_snapshot = root.snapshot().unwrap();
    let before_revision = root.get_persistent_mutation_revision().unwrap();

    root.with_shared_data_borrow_for_test(|| {
        assert_eq!(
            root.create_sequential_children("子", 60, 1, 2, "", new_test_task_attr,),
            Err(TaskTreeError::Borrow)
        );
    });

    assert_eq!(root.snapshot().unwrap(), before_snapshot);
    assert_eq!(
        root.get_persistent_mutation_revision().unwrap(),
        before_revision
    );
}

#[test]
fn test_reparent_toはsource_rootのshared_borrow競合時にtreeとrevisionを変更しない() {
    let source_root = new_test_task_handle("移動元").unwrap();
    let mut child = source_root.create_child(new_test_task_attr("子")).unwrap();
    let destination_root = new_test_task_handle("移動先").unwrap();
    let before_source_snapshot = source_root.snapshot().unwrap();
    let before_destination_snapshot = destination_root.snapshot().unwrap();
    let before_source_revision = source_root.get_persistent_mutation_revision().unwrap();
    let before_destination_revision = destination_root.get_persistent_mutation_revision().unwrap();

    source_root.with_shared_data_borrow_for_test(|| {
        assert_eq!(
            child.reparent_to(&destination_root),
            Err(TaskTreeError::Borrow)
        );
    });

    assert_eq!(source_root.snapshot().unwrap(), before_source_snapshot);
    assert_eq!(
        destination_root.snapshot().unwrap(),
        before_destination_snapshot
    );
    assert_eq!(
        source_root.get_persistent_mutation_revision().unwrap(),
        before_source_revision
    );
    assert_eq!(
        destination_root.get_persistent_mutation_revision().unwrap(),
        before_destination_revision
    );
}

#[test]
fn test_first_available_time_pending状態の時はpending_untilとstart_timeの大きい方が採用されること_pending_untilの方が大きい場合(
) {
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.set_create_time(dt).unwrap();
    parent_task.set_start_time(dt).unwrap();
    parent_task.set_orig_status(Status::Pending).unwrap();
    parent_task
        .set_pending_until(dt + Duration::hours(1))
        .unwrap();
    parent_task.sync_clock(dt).unwrap();

    let actual = parent_task.first_available_time().unwrap();
    let expected = dt + Duration::hours(1);

    assert_eq!(actual, expected);
}

#[test]
fn test_list_all_parent_tasks_with_first_available_time_タスク1個でpending状態の時はpending_untilとstart_timeの大きい方が採用されること_pending_untilの方が大きい場合(
) {
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.set_create_time(dt).unwrap();
    parent_task.set_start_time(dt).unwrap();
    parent_task.set_orig_status(Status::Pending).unwrap();
    parent_task
        .set_pending_until(dt + Duration::hours(1))
        .unwrap();
    parent_task.sync_clock(dt).unwrap();

    let actual = parent_task
        .list_all_parent_tasks_with_first_available_time()
        .unwrap();
    let expected = [(dt + Duration::hours(1), parent_task)];

    assert_eq!(actual, expected);
}

#[test]
fn test_list_all_parent_tasks_with_first_available_time_タスク1個でpending状態の時はpending_untilとstart_timeの大きい方が採用されること_deadline_timeのほうが小さい場合(
) {
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.set_create_time(dt).unwrap();
    parent_task.set_start_time(dt).unwrap();
    parent_task.set_estimated_work_seconds(3600).unwrap();
    parent_task.set_orig_status(Status::Pending).unwrap();
    parent_task
        .set_pending_until(dt + Duration::hours(1))
        .unwrap();
    parent_task
        .set_deadline_time_opt(Some(dt - Duration::hours(1)))
        .unwrap();
    parent_task.sync_clock(dt).unwrap();

    let actual = parent_task
        .list_all_parent_tasks_with_first_available_time()
        .unwrap();
    let expected = [(
        dt - Duration::hours(1) - Duration::seconds(3600),
        parent_task,
    )];

    assert_eq!(actual, expected);
}

#[test]
fn test_first_available_time_pending状態の時はpending_untilとstart_timeの大きい方が採用されること_start_timeの方が大きい場合(
) {
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.set_create_time(dt).unwrap();
    parent_task.set_start_time(dt + Duration::hours(2)).unwrap();
    parent_task.set_orig_status(Status::Pending).unwrap();
    parent_task
        .set_pending_until(dt + Duration::hours(1))
        .unwrap();
    parent_task.sync_clock(dt).unwrap();

    let actual = parent_task.first_available_time().unwrap();
    let expected = dt + Duration::hours(2);

    assert_eq!(actual, expected);
}

#[test]
fn test_list_all_parent_tasks_with_first_available_time_タスク1個でpending状態の時はpending_untilとstart_timeの大きい方が採用されること_start_timeの方が大きい場合(
) {
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.set_create_time(dt).unwrap();
    parent_task.set_start_time(dt + Duration::hours(2)).unwrap();
    parent_task.set_orig_status(Status::Pending).unwrap();
    parent_task
        .set_pending_until(dt + Duration::hours(1))
        .unwrap();
    parent_task.sync_clock(dt).unwrap();

    let actual = parent_task
        .list_all_parent_tasks_with_first_available_time()
        .unwrap();
    let expected = [(dt + Duration::hours(2), parent_task)];

    assert_eq!(actual, expected);
}

#[test]
fn test_list_all_parent_tasks_with_first_available_time_タスク1個でpending状態ではない時はstart_timeが採用されること(
) {
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.set_create_time(dt).unwrap();
    parent_task.set_start_time(dt + Duration::hours(1)).unwrap();
    parent_task.set_orig_status(Status::Todo).unwrap();
    parent_task
        .set_pending_until(dt + Duration::hours(2))
        .unwrap();
    parent_task.sync_clock(dt).unwrap();

    let actual = parent_task
        .list_all_parent_tasks_with_first_available_time()
        .unwrap();
    let expected = [(dt + Duration::hours(1), parent_task)];

    assert_eq!(actual, expected);
}

#[test]
fn test_first_available_time_pending状態ではない時はstart_timeが採用されること() {
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.set_create_time(dt).unwrap();
    parent_task.set_start_time(dt + Duration::hours(1)).unwrap();
    parent_task.set_orig_status(Status::Todo).unwrap();
    parent_task
        .set_pending_until(dt + Duration::hours(2))
        .unwrap();
    parent_task.sync_clock(dt).unwrap();

    let actual = parent_task.first_available_time().unwrap();
    let expected = dt + Duration::hours(1);

    assert_eq!(actual, expected);
}

#[test]
fn test_list_all_parent_tasks_with_first_available_time_正常系() {
    /*
     parent_task_1
       - child_task_1
         - grand_child_task (葉)
    */
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.set_create_time(dt).unwrap();
    parent_task.set_start_time(dt).unwrap();

    let mut child_task = new_test_task_handle("子タスク").unwrap();
    child_task.set_create_time(dt).unwrap();
    child_task.set_start_time(dt).unwrap();

    let grand_child_task = child_task.create_as_last_child(new_test_task_attr("孫タスク"));
    grand_child_task.set_create_time(dt).unwrap();
    grand_child_task.set_start_time(dt).unwrap();

    let expected = vec![
        (dt, grand_child_task.clone()),
        (dt + Duration::minutes(15), child_task.clone()),
        (dt + Duration::minutes(30), parent_task.clone()),
    ];

    child_task
        .detach_insert_as_last_child_of(parent_task)
        .unwrap();

    let actual = grand_child_task
        .list_all_parent_tasks_with_first_available_time()
        .unwrap();

    assert_eq!(actual, expected);
}

#[test]
fn test_list_all_parent_tasks_with_first_available_time_親は子の残作業後に着手可能になる() {
    /*
     parent_task_1 (見積もり0m)
       - child_task_1 (見積もり15m)
         - grand_child_task (葉) (見積もり1m)
    */
    let dt = Local.with_ymd_and_hms(2026, 5, 10, 14, 5, 0).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.set_create_time(dt).unwrap();
    parent_task.set_start_time(dt).unwrap();
    parent_task.set_estimated_work_seconds(0).unwrap();

    let mut child_task = new_test_task_handle("子タスク").unwrap();
    child_task.set_create_time(dt).unwrap();
    child_task.set_start_time(dt).unwrap();
    child_task.set_estimated_work_seconds(15 * 60).unwrap();

    let grand_child_task = child_task.create_as_last_child(new_test_task_attr("孫タスク"));
    grand_child_task.set_create_time(dt).unwrap();
    grand_child_task.set_start_time(dt).unwrap();
    grand_child_task.set_estimated_work_seconds(60).unwrap();

    let expected = vec![
        (dt, grand_child_task.clone()),
        (dt + Duration::minutes(1), child_task.clone()),
        (dt + Duration::minutes(16), parent_task.clone()),
    ];

    child_task
        .detach_insert_as_last_child_of(parent_task)
        .unwrap();

    let actual = grand_child_task
        .list_all_parent_tasks_with_first_available_time()
        .unwrap();

    assert_eq!(actual, expected);
}

#[test]
fn test_list_all_parent_tasks_with_first_available_time_葉に〆切がある場合() {
    /*
     parent_task_1
       - child_task_1
         - grand_child_task (葉)
    */
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.set_create_time(dt).unwrap();
    parent_task.set_start_time(dt).unwrap();

    let mut child_task = new_test_task_handle("子タスク").unwrap();
    child_task.set_create_time(dt).unwrap();
    child_task.set_start_time(dt).unwrap();

    let grand_child_task = child_task.create_as_last_child(new_test_task_attr("孫タスク"));
    grand_child_task.set_create_time(dt).unwrap();
    grand_child_task.set_start_time(dt).unwrap();
    grand_child_task.set_estimated_work_seconds(3600).unwrap();
    grand_child_task
        .set_deadline_time_opt(Some(dt - Duration::hours(1)))
        .unwrap();

    let expected = vec![
        (
            dt - Duration::hours(1) - Duration::seconds(3600),
            grand_child_task.clone(),
        ),
        (dt, child_task.clone()),
        (dt + Duration::minutes(15), parent_task.clone()),
    ];

    child_task
        .detach_insert_as_last_child_of(parent_task)
        .unwrap();

    let actual = grand_child_task
        .list_all_parent_tasks_with_first_available_time()
        .unwrap();

    assert_eq!(actual, expected);
}

#[test]
fn test_list_all_parent_tasks_with_first_available_time_単に計算すると〆切をオーバーする場合は〆切優先とする(
) {
    /*
     parent_task_1 (見積もり1h)
       - child_task_1 (見積もり3h)
         - grand_child_task (葉) (見積もり1h)
    */
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 0, 0, 0).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.set_create_time(dt).unwrap();
    parent_task.set_start_time(dt).unwrap();
    parent_task.set_estimated_work_seconds(3600 * 2).unwrap();
    parent_task
        .set_deadline_time_opt(Some(dt + Duration::hours(24)))
        .unwrap();

    let mut child_task = new_test_task_handle("子タスク").unwrap();
    child_task.set_create_time(dt).unwrap();
    child_task.set_start_time(dt).unwrap();
    child_task.set_estimated_work_seconds(3600 * 3).unwrap();
    child_task
        .set_deadline_time_opt(Some(dt + Duration::hours(24)))
        .unwrap();

    let grand_child_task = child_task.create_as_last_child(new_test_task_attr("孫タスク"));
    grand_child_task.set_create_time(dt).unwrap();
    grand_child_task.set_start_time(dt).unwrap();
    grand_child_task.set_estimated_work_seconds(3600).unwrap();
    grand_child_task
        .set_pending_until(dt + Duration::hours(22))
        .unwrap();
    grand_child_task.set_orig_status(Status::Pending).unwrap();
    // 先に締切を設定してしまうと、pending_untilを設定する時にその〆切からバッファを考慮して前倒ししてしまう
    // バッファの量はset_pending_until()の中で設定されており、このテストでは考慮したくない
    // よって、締切は最後に設定する
    grand_child_task
        .set_deadline_time_opt(Some(dt + Duration::hours(24)))
        .unwrap();

    let expected = vec![
        (
            // grand_child_task自体のpending_untilは22時、見積もりは1hだが、
            // 親タスクの〆切を逆算すると19時に作業開始する必要がある
            dt + Duration::hours(24) - Duration::hours(2 + 3 + 1),
            grand_child_task.clone(),
        ),
        (
            dt + Duration::hours(24) - Duration::hours(2 + 3),
            child_task.clone(),
        ),
        (
            dt + Duration::hours(24) - Duration::hours(2),
            parent_task.clone(),
        ),
    ];

    child_task
        .detach_insert_as_last_child_of(parent_task)
        .unwrap();

    let actual = grand_child_task
        .list_all_parent_tasks_with_first_available_time()
        .unwrap();

    assert_eq!(actual, expected);
}

#[test]
fn test_list_all_parent_tasks_with_first_available_time_繰り返しタスクの例() {
    /*
     parent_task_1 (見積もり0h)
       - child_task_1 (repetition_interval_days=28で〆切は2037年, 見積もり8h)
         - grand_child_task (葉) (見積もり8h)
    */
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 0, 0, 0).unwrap();
    let parent_task = new_test_task_handle("親タスク").unwrap();
    parent_task.set_create_time(dt).unwrap();
    parent_task.set_start_time(dt).unwrap();
    parent_task.set_estimated_work_seconds(0).unwrap();

    let mut child_task = new_test_task_handle("子タスク").unwrap();
    child_task.set_create_time(dt).unwrap();
    child_task.set_start_time(dt).unwrap();
    child_task.set_estimated_work_seconds(3600 * 8).unwrap();
    child_task
        .set_pending_until(Local.with_ymd_and_hms(2038, 1, 1, 0, 0, 0).unwrap())
        .unwrap();
    child_task.set_orig_status(Status::Pending).unwrap();
    // 先に締切を設定してしまうと、pending_untilを設定する時にその〆切からバッファを考慮して前倒ししてしまう
    // バッファの量はset_pending_until()の中で設定されており、このテストでは考慮したくない
    // よって、締切は最後に設定する
    child_task
        .set_deadline_time_opt(Some(
            Local.with_ymd_and_hms(2037, 12, 31, 20, 0, 0).unwrap(),
        ))
        .unwrap();

    let grand_child_task = child_task.create_as_last_child(new_test_task_attr("孫タスク"));
    grand_child_task.set_create_time(dt).unwrap();
    grand_child_task.set_start_time(dt).unwrap();
    grand_child_task
        .set_estimated_work_seconds(3600 * 8)
        .unwrap();
    grand_child_task
        .set_pending_until(dt + Duration::hours(22))
        .unwrap();
    grand_child_task.set_orig_status(Status::Pending).unwrap();
    // 上に同じく、締切は最後に設定する
    grand_child_task
        .set_deadline_time_opt(Some(dt + Duration::hours(20)))
        .unwrap();

    let expected = vec![
        (
            // grand_child_task自体のpending_untilは22時、見積もりは8hだが、
            // 〆切(20時)を逆算すると12時に作業開始する必要がある
            dt + Duration::hours(12),
            grand_child_task.clone(),
        ),
        (
            // (繰り返しタスクだが)論理的にはgrand_child_taskが終わった時間から着手できる
            // dt + Duration::hours(12 + 8),
            Local.with_ymd_and_hms(2037, 12, 31, 12, 0, 0).unwrap(),
            child_task.clone(),
        ),
        (
            // (繰り返しタスクだが)論理的にはgrand_child_taskが終わった時間から着手できる
            // dt + Duration::hours(12 + 8),
            Local.with_ymd_and_hms(2037, 12, 31, 20, 0, 0).unwrap(),
            parent_task.clone(),
        ),
    ];

    child_task
        .detach_insert_as_last_child_of(parent_task)
        .unwrap();

    let actual = grand_child_task
        .list_all_parent_tasks_with_first_available_time()
        .unwrap();

    assert_eq!(actual, expected);
}
