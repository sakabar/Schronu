use chrono::{DateTime, Duration, Local};
use core::cell::BorrowError;
use dendron::{HotNode, InsertAs, Node};
use linked_hash_map::LinkedHashMap;
use std::cmp::{max, min};
use std::fmt;
use uuid::Uuid;
use yaml_rust::Yaml;

#[cfg(test)]
use chrono::TimeZone;

#[cfg(test)]
use dendron::{tree, Tree};

#[cfg(test)]
use yaml_rust::YamlLoader;

#[cfg(test)]
use uuid::uuid;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Status {
    // 初期状態
    Todo,

    // 優先度が低いなどの理由でスコープアウトした状態
    // 相手ボールの場合は相手の返答をウォッチして適宜つつくという作業があるので、Pendingではない
    Pending,

    // 完了
    Done,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Status::Todo => {
                write!(f, "todo")
            }
            Status::Pending => {
                write!(f, "pending")
            }
            Status::Done => {
                write!(f, "done")
            }
        }
    }
}

pub fn read_status(s: &str) -> Option<Status> {
    let lc = s.to_lowercase();

    if lc == "todo" {
        return Some(Status::Todo);
    } else if lc == "pending" {
        return Some(Status::Pending);
    } else if lc == "done" {
        return Some(Status::Done);
    }

    return None;
}

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

#[derive(Clone, Debug, PartialEq)]
pub struct ImmutableTask {
    name: String,
    status: Status,
    pending_until: DateTime<Local>,
    children: Vec<ImmutableTask>,
}

#[test]
#[allow(non_snake_case)]
pub fn test_new_with_current_time_現在時刻がpending_until以前でPending状態であること() {
    let pending_until = DateTime::<Local>::MAX_UTC.into();
    let actual = ImmutableTask::new_with_current_time(
        "タスク".to_string(),
        Status::Pending,
        pending_until,
        vec![],
    );
    let expected = ImmutableTask::new("タスク".to_string(), Status::Pending, pending_until, vec![]);

    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
pub fn test_new_with_current_time_現在時刻がpending_until以降の場合Todo状態となること() {
    let pending_until = DateTime::<Local>::MIN_UTC.into();
    let actual = ImmutableTask::new_with_current_time(
        "タスク".to_string(),
        Status::Pending,
        pending_until,
        vec![],
    );
    let expected = ImmutableTask::new("タスク".to_string(), Status::Todo, pending_until, vec![]);

    assert_eq!(actual, expected);
}

impl ImmutableTask {
    pub fn new(
        name: String,
        status: Status,
        pending_until: DateTime<Local>,
        children: Vec<ImmutableTask>,
    ) -> Self {
        Self {
            name,
            status,
            pending_until,
            children,
        }
    }

    // 現在時刻に依存する関数であることに注意
    pub fn new_with_current_time(
        name: String,
        status: Status,
        pending_until: DateTime<Local>,
        children: Vec<ImmutableTask>,
    ) -> Self {
        let new_status = if status == Status::Pending && Local::now() > pending_until {
            Status::Todo
        } else {
            status
        };

        Self {
            name,
            status: new_status,
            pending_until,
            children,
        }
    }

    pub fn new_with_name(name: String) -> Self {
        Self {
            name,
            status: Status::Todo,
            pending_until: DateTime::<Local>::MIN_UTC.into(),
            children: vec![],
        }
    }

    pub fn new_with_name_status_children(
        name: String,
        status: Status,
        children: Vec<ImmutableTask>,
    ) -> Self {
        // 期限なしPendingはタスクやり忘れの元なので、自動的に1970とする
        // ちょっと迷い中。2037の方がよいのか?
        Self {
            name,
            status,
            pending_until: DateTime::<Local>::MIN_UTC.into(),
            children,
        }
    }

    pub fn new_with_name_children(name: String, children: Vec<ImmutableTask>) -> Self {
        Self {
            name,
            status: Status::Todo,
            pending_until: DateTime::<Local>::MIN_UTC.into(),
            children,
        }
    }

    pub fn get_name(&self) -> &str {
        return &self.name;
    }

    pub fn get_status(&self) -> &Status {
        return &self.status;
    }

    pub fn get_children(&self) -> &Vec<ImmutableTask> {
        return &self.children;
    }
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

pub fn extract_leaf_immutable_tasks_from_project(task: &ImmutableTask) -> Vec<&ImmutableTask> {
    let children_are_all_done = task
        .get_children()
        .iter()
        .all(|task| task.status == Status::Done);

    if task.get_status() == &Status::Todo
        && (task.get_children().is_empty() || children_are_all_done)
    {
        return vec![task];
    }

    let mut ans: Vec<&ImmutableTask> = vec![];

    // 深さ優先
    for child in task.get_children() {
        if child.get_status() != &Status::Done {
            let leaves_with_pending: Vec<&ImmutableTask> =
                extract_leaf_immutable_tasks_from_project(child);
            let mut leaves: Vec<&ImmutableTask> = leaves_with_pending
                .iter()
                .filter(|&leaf| leaf.get_status() != &Status::Pending)
                .map(|&leaf| leaf)
                .collect::<Vec<_>>();
            ans.append(&mut leaves);
        }
    }

    return ans;
}

// Todoの葉タスクを抽出する
pub fn extract_leaf_tasks_from_project(task: &Task) -> Vec<Task> {
    let target_status: Vec<Status> = vec![Status::Todo];
    extract_leaf_tasks_from_project_boyoyo(task, &target_status)
}

// TodoもしくはPendingの葉タスクを抽出する
pub fn extract_leaf_tasks_from_project_with_pending(task: &Task) -> Vec<Task> {
    let target_status: Vec<Status> = vec![Status::Todo, Status::Pending];
    extract_leaf_tasks_from_project_boyoyo(task, &target_status)
}

fn extract_leaf_tasks_from_project_boyoyo(
    task: &Task,
    target_status_arr: &Vec<Status>,
) -> Vec<Task> {
    let children_are_all_done = task
        .node
        .children()
        .all(|child_node| child_node.borrow_data().get_status() == &Status::Done);

    if target_status_arr.contains(&task.get_status())
        && (!task.node.has_children() || children_are_all_done)
    {
        let new_task = Task {
            node: task.node.clone(),
        };
        return vec![new_task];
    }

    let mut ans: Vec<Task> = vec![];

    // 深さ優先
    for child_node in task.node.children() {
        if child_node.borrow_data().get_status() != &Status::Done {
            let child_task = Task { node: child_node };

            let leaves_with_pending: Vec<Task> =
                extract_leaf_tasks_from_project_boyoyo(&child_task, &target_status_arr);

            let mut leaves: Vec<Task> = leaves_with_pending
                .iter()
                .filter(|&leaf| target_status_arr.contains(&leaf.get_status()))
                .map(|leaf| Task {
                    node: leaf.node.clone(),
                })
                .collect::<Vec<_>>();
            ans.append(&mut leaves);
        }
    }

    return ans;
}

pub fn round_up_sec_as_minute(seconds: i64) -> i64 {
    seconds / 60 + if seconds % 60 == 0 { 0 } else { 1 }
}

// pub fn extract_leaf_tasks_from_project_ref(task: &Task) -> Vec<&TaskAttr> {
//     extract_leaf_tasks_from_project_ref_private(&task.node)
// }

// fn extract_leaf_tasks_from_project_ref_private(node: &Node<TaskAttr>) -> Vec<&TaskAttr> {
//     let children_are_all_done = node
//         .children()
//         .all(|child_node| child_node.borrow_data().get_status() == &Status::Done);

//     let task_attr = node.borrow_data();
//     if task_attr.get_status() == &Status::Todo && (!node.has_children() || children_are_all_done) {
//         return vec![&task_attr];
//     }

//     let mut ans: Vec<&TaskAttr> = vec![];

//     // 深さ優先
//     for child_node in node.children() {
//         if child_node.borrow_data().get_status() != &Status::Done {
//             let leaves_with_pending: Vec<&TaskAttr> =
//                 extract_leaf_tasks_from_project_ref_private(&child_node);

//             let mut leaves = leaves_with_pending
//                 .iter()
//                 .filter(|&leaf| leaf.get_status() != &Status::Pending)
//                 .map(|&leaf| leaf)
//                 .collect::<Vec<_>>();
//             ans.append(&mut leaves);
//         }
//     }

//     return ans;
// }

#[test]
fn test_extract_leaf_tasks_from_project_タスクのchildrenが空配列の場合() {
    let task = Task::new("タスク");
    let actual = extract_leaf_tasks_from_project(&task);

    let t = Task::new("タスク");

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
    let mut grand_child_task_1 = Task::new("孫タスク1");
    let ptr_to_grand_child_task_1_node = grand_child_task_1.node.clone();

    let child_task_1 = Task::new("子タスク1");
    grand_child_task_1
        .detach_insert_as_last_child_of(child_task_1)
        .unwrap();

    let mut child_task_1_again = grand_child_task_1.root();

    let mut child_task_2 = Task::new("子タスク2");
    let parent_task_1 = Task::new("親タスク1");

    child_task_1_again
        .detach_insert_as_last_child_of(parent_task_1)
        .unwrap();
    let parent_task_again = child_task_1_again.root();
    child_task_2
        .detach_insert_as_last_child_of(parent_task_again)
        .unwrap();

    let parent_task_again_again = child_task_2.root();

    let actual = extract_leaf_tasks_from_project(&parent_task_again_again);
    let t1 = Task::new("孫タスク1");
    let t2 = Task::new("子タスク2");
    let expected = vec![t1, t2];
    assert_eq!(&actual, &expected);

    // actualの2つのノードに親子関係の情報が残っており、それらの親が同一であること
    assert_eq!(actual.len(), 2);
    let actual1 = actual.first().unwrap();
    let actual2 = actual.last().unwrap();

    assert_ne!(actual1, actual2);
    assert_eq!(actual1.root().node.borrow_data().get_name(), "親タスク1");
    assert_eq!(actual2.root().node.borrow_data().get_name(), "親タスク1");
    assert!(actual1
        .node
        .belongs_to_same_tree(&ptr_to_grand_child_task_1_node));
    assert!(actual2
        .node
        .belongs_to_same_tree(&ptr_to_grand_child_task_1_node));
}

////////////////// ここから要テスト

// #[test]
// fn test_extract_leaf_tasks_from_project_done状態のタスクとその子孫は全て無視されること() {
//     /*
//      parent_task_1
//        - child_task_1 (Done)
//          - grand_child_task (todo, だが親がdoneなので無視される)
//        - child_task_2
//     */
//     let grand_child_task_1 = Task::new_with_name("孫タスク1".to_string());
//     let child_task_1 = Task::new_with_name_status_children(
//         "子タスク1".to_string(),
//         Status::Done,
//         vec![grand_child_task_1],
//     );

//     let child_task_2 = Task::new_with_name("子タスク2".to_string());

//     let parent_task_1 = Task::new_with_name_children(
//         "親タスク1".to_string(),
//         vec![child_task_1, child_task_2],
//     );

//     let actual = extract_leaf_tasks_from_project(&parent_task_1);
//     let expected_child_task_2 = Task::new_with_name("子タスク2".to_string());
//     let expected = vec![&expected_child_task_2];
//     assert_eq!(actual, expected);
// }

// #[test]
// fn test_extract_leaf_tasks_from_project_途中にpending状態のタスクがあった場合は子孫を辿るが_葉がpending状態の場合は結果に入らないこと(
// ) {
//     /*
//      parent_task_1
//        - child_task_1 (Pending)
//          - grand_child_task (todo、親がPendingだがそれは関係なく結果として返る)
//        - child_task_2 (Pendingの葉なので結果に入らない)
//     */
//     let grand_child_task_1 = Task::new_with_name("孫タスク1".to_string());
//     let child_task_1 = Task::new_with_name_status_children(
//         "子タスク1".to_string(),
//         Status::Pending,
//         vec![grand_child_task_1],
//     );

//     let child_task_2 = Task::new_with_name_status_children(
//         "子タスク2".to_string(),
//         Status::Pending,
//         vec![],
//     );

//     let parent_task_1 = Task::new_with_name_children(
//         "親タスク1".to_string(),
//         vec![child_task_1, child_task_2],
//     );

//     let actual = extract_leaf_tasks_from_project(&parent_task_1);
//     let expected_grand_child_task_1 = Task::new_with_name("孫タスク1".to_string());
//     let expected = vec![&expected_grand_child_task_1];
//     assert_eq!(actual, expected);
// }

#[test]
fn test_extract_leaf_tasks_from_project_子が全てdoneのタスクは葉として扱われること() {
    /*
     parent_task_1
       - child_task_1 (子が全てdoneなので葉として返る)
         - grand_child_task_1 (done)
         - grand_child_task_2 (done)
       - child_task_2 (返る)
    */
    let mut grand_child_task_1 = Task::new("孫タスク1");
    grand_child_task_1.set_orig_status(Status::Done);

    let mut grand_child_task_2 = Task::new("孫タスク2");
    grand_child_task_2.set_orig_status(Status::Done);

    let child_task_1 = Task::new("子タスク1");

    grand_child_task_1
        .detach_insert_as_last_child_of(child_task_1)
        .unwrap();
    let child_task_1_again = grand_child_task_1.parent().unwrap();
    grand_child_task_2
        .detach_insert_as_last_child_of(child_task_1_again)
        .unwrap();

    let parent_task = grand_child_task_2.root();

    let expected_child_task_1 = Task {
        node: parent_task.node.first_child().unwrap(),
    };

    let actual = extract_leaf_tasks_from_project(&parent_task);

    assert_eq!(actual.len(), 1);
    assert_task(&actual.first().unwrap(), &expected_child_task_1);
}

#[test]
fn test_extract_leaf_tasks_from_project_子が全てdoneのタスクで親がpendingの時は空配列を返すこと() {
    /*
     parent_task_1 (pending)
       - child_task_1 (done)
    */
    let mut child_task_1 = Task::new("子タスク1");
    child_task_1.set_orig_status(Status::Done);

    let pending_until = Local.with_ymd_and_hms(2037, 12, 31, 0, 0, 0).unwrap();
    let parent_task_1 = Task::new("親タスク1");
    parent_task_1.set_orig_status(Status::Pending);
    parent_task_1.set_pending_until(pending_until);
    child_task_1
        .detach_insert_as_last_child_of(parent_task_1)
        .unwrap();

    let root_task = &child_task_1.root();
    let actual = extract_leaf_tasks_from_project(root_task);
    let expected: Vec<Task> = vec![];
    assert_eq!(actual, expected);
}

#[derive(Clone)]
pub struct TaskAttr {
    id: Uuid,
    name: String,
    orig_status: Status, // 元々のステータス。orig_status=Pendingの時、時刻によらずPendingのまま。
    status: Status, // 評価後のステータス。pendingはpending_untilを加味して評価され、Todo扱いとなる
    is_on_other_side: bool, // 相手ボールか?
    pending_until: DateTime<Local>,
    last_synced_time: DateTime<Local>,

    priority: i64, // 優先度。大きいほど高い

    create_time: DateTime<Local>,               // タスクが生成された日時
    start_time: DateTime<Local>,                // タスクが着手可能になった日時
    end_time_opt: Option<DateTime<Local>>,      // タスクが完了した日時
    deadline_time_opt: Option<DateTime<Local>>, // タスクの〆切

    estimated_work_seconds: i64, // 見積もられた作業時間 (秒)
    actual_work_seconds: i64,    // 実際の作業時間 (秒)

    repetition_interval_days_opt: Option<i64>,
    days_in_advance: i64, // 繰り返しタスクについて、何日前から着手開始可能とするか
}

// 生成するタイミングで結果が変わってしまうid, create_time, start_timeは
// 等価性判定には用いない
impl PartialEq for TaskAttr {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.orig_status == other.orig_status
            && self.status == other.status
            && self.is_on_other_side == other.is_on_other_side
            && self.pending_until == other.pending_until
            && self.last_synced_time == other.last_synced_time
            && self.priority == other.priority
            // && self.create_time == other.create_time
            // && self.start_time == other.start_time
            && self.end_time_opt == other.end_time_opt
            && self.deadline_time_opt == other.deadline_time_opt
            && self.estimated_work_seconds == other.estimated_work_seconds
            && self.actual_work_seconds == other.actual_work_seconds
            && self.repetition_interval_days_opt == other.repetition_interval_days_opt
            && self.days_in_advance == other.days_in_advance
    }
}

// ツリーを出力した際に複数行にまたがると見映えが悪くなるため、情報を落としている
impl fmt::Debug for TaskAttr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status_checkbox: &str = match self.status {
            Status::Todo => "[ ]",
            Status::Pending => "[-]",
            Status::Done => "[+]",
        };

        f.debug_struct("")
            .field(
                "name",
                &format!(
                    "{} {:02}m/{:02}m {}{}",
                    status_checkbox,
                    round_up_sec_as_minute(self.get_actual_work_seconds()),
                    round_up_sec_as_minute(self.get_estimated_work_seconds()),
                    if self.is_on_other_side {
                        "[待ち]"
                    } else {
                        ""
                    },
                    &self.name
                )
                .as_str(),
            )
            .field("id", &self.id)
            // .field("orig_status", &self.orig_status)
            // .field("status", &self.status)
            // .field("pending_until", &self.pending_until)
            // .field("last_synced_time", &self.last_synced_time)
            // .field("priority", &self.priority)
            .finish()
    }
}

impl TaskAttr {
    pub fn new(name: &str) -> Self {
        // 本当はnow()で副作用を持たせたくなかったが、毎回手入力するわけにもいかないので渋々使用
        let now = Local::now();

        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            orig_status: Status::Todo,
            status: Status::Todo,
            is_on_other_side: false,
            pending_until: DateTime::<Local>::MIN_UTC.into(),
            last_synced_time: DateTime::<Local>::MIN_UTC.into(),
            priority: 0,
            create_time: now,
            start_time: now,
            end_time_opt: None,
            deadline_time_opt: None,
            estimated_work_seconds: 900,
            actual_work_seconds: 0,
            repetition_interval_days_opt: None,
            days_in_advance: 0,
        }
    }

    pub fn get_id(&self) -> &Uuid {
        &self.id
    }

    pub fn set_id(&mut self, id: Uuid) {
        self.id = id;
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn set_orig_status(&mut self, orig_status: Status) {
        self.orig_status = orig_status;

        // 〆切の何秒前から強制的にTodo扱いにするか
        let deadline_buffer_seconds_after_start_time = 3600;
        let deadline_buffer_seconds_before_start_time = 300;

        // pending_untilが〆切よりも後ろになってしまっている場合はpending_untilを調整する
        if self.orig_status == Status::Pending && self.deadline_time_opt.is_some() {
            let pending_time_before_deadline = self.deadline_time_opt.unwrap()
                - Duration::seconds(self.estimated_work_seconds)
                - Duration::seconds(deadline_buffer_seconds_before_start_time);

            if pending_time_before_deadline < self.pending_until {
                self.pending_until = pending_time_before_deadline;
            }
        }

        // 変わりうるのは、
        // not Done -> Todo (deadlineが近い)
        // Pending -> Todo (pending_until後 かつ start_time後)
        // Todo -> Pending (start_time起因)
        self.status = if self.orig_status != Status::Done
            && self.last_synced_time > self.start_time
            && self.deadline_time_opt.is_some()
            && self.deadline_time_opt.unwrap()
                - Duration::seconds(max(
                    0,
                    self.estimated_work_seconds - self.actual_work_seconds,
                ))
                - Duration::seconds(deadline_buffer_seconds_after_start_time)
                < self.last_synced_time
        {
            Status::Todo
        } else if self.orig_status != Status::Done
            && self.last_synced_time < self.start_time
            && self.deadline_time_opt.is_some()
            && self.deadline_time_opt.unwrap()
                - Duration::seconds(self.estimated_work_seconds)
                - Duration::seconds(deadline_buffer_seconds_before_start_time)
                < self.last_synced_time
        {
            Status::Todo
        } else if self.orig_status == Status::Pending
            && self.last_synced_time > self.pending_until
            && self.last_synced_time > self.start_time
        {
            Status::Todo
        } else if self.orig_status == Status::Todo && self.start_time > self.last_synced_time {
            Status::Pending
        } else {
            self.orig_status
        };
    }

    pub fn get_status(&self) -> &Status {
        &self.status
    }

    pub fn get_orig_status(&self) -> &Status {
        &self.orig_status
    }

    pub fn get_is_on_other_side(&self) -> &bool {
        &self.is_on_other_side
    }

    pub fn set_is_on_other_side(&mut self, is_on_other_side: bool) {
        self.is_on_other_side = is_on_other_side;
    }

    // 時刻を入力し、その時刻を用いてpending判定を行う。
    pub fn sync_clock(&mut self, now: DateTime<Local>) {
        self.last_synced_time = now;
        self.set_orig_status(*self.get_orig_status());
    }

    pub fn get_last_synced_time(&self) -> &DateTime<Local> {
        &self.last_synced_time
    }

    pub fn set_pending_until(&mut self, pending_until: DateTime<Local>) {
        self.pending_until = pending_until;
        self.set_orig_status(*self.get_orig_status());
    }

    pub fn get_pending_until(&self) -> &DateTime<Local> {
        &self.pending_until
    }

    pub fn set_priority(&mut self, priority: i64) {
        self.priority = priority;
    }

    pub fn get_priority(&self) -> i64 {
        self.priority
    }

    pub fn set_create_time(&mut self, create_time: DateTime<Local>) {
        self.create_time = create_time;
    }

    pub fn get_create_time(&self) -> &DateTime<Local> {
        &self.create_time
    }

    pub fn set_start_time(&mut self, start_time: DateTime<Local>) {
        self.start_time = start_time;
        self.set_orig_status(*self.get_orig_status());
    }

    pub fn get_start_time(&self) -> &DateTime<Local> {
        &self.start_time
    }

    pub fn set_end_time_opt(&mut self, end_time_opt: Option<DateTime<Local>>) {
        self.end_time_opt = end_time_opt;
    }

    pub fn get_end_time_opt(&self) -> &Option<DateTime<Local>> {
        &self.end_time_opt
    }

    pub fn set_deadline_time_opt(&mut self, deadline_time_opt: Option<DateTime<Local>>) {
        self.deadline_time_opt = deadline_time_opt;
    }

    pub fn get_deadline_time_opt(&self) -> &Option<DateTime<Local>> {
        &self.deadline_time_opt
    }

    pub fn set_estimated_work_seconds(&mut self, estimated_work_seconds: i64) {
        self.estimated_work_seconds = estimated_work_seconds;
    }

    pub fn get_estimated_work_seconds(&self) -> i64 {
        self.estimated_work_seconds
    }

    pub fn set_actual_work_seconds(&mut self, actual_work_seconds: i64) {
        self.actual_work_seconds = actual_work_seconds;
    }

    pub fn get_actual_work_seconds(&self) -> i64 {
        self.actual_work_seconds
    }

    pub fn set_repetition_interval_days_opt(&mut self, repetition_interval_days_opt: Option<i64>) {
        self.repetition_interval_days_opt = repetition_interval_days_opt;
    }

    pub fn get_repetition_interval_days_opt(&self) -> Option<i64> {
        self.repetition_interval_days_opt
    }

    pub fn set_days_in_advance(&mut self, days_in_advance: i64) {
        self.days_in_advance = days_in_advance;
    }

    pub fn get_days_in_advance(&self) -> i64 {
        self.days_in_advance
    }
}

#[test]
fn test_task_attr_set_status() {
    let mut attr = TaskAttr::new("タスク");
    attr.set_orig_status(Status::Done);
    let actual = attr.get_status();
    assert_eq!(actual, &Status::Done);
}

#[test]
fn test_task_attr_set_pending_until() {
    let mut attr = TaskAttr::new("タスク");
    let pending_until = Local.with_ymd_and_hms(2000, 1, 1, 0, 0, 0).unwrap();
    attr.set_pending_until(pending_until);
    let actual = attr.get_pending_until();
    assert_eq!(actual, &pending_until);
}

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    node: Node<TaskAttr>,
}

impl Task {
    // dendron::Node::try_detach_insert_subtree()は木そのものを消滅させることができない仕様のようなので、
    // ダミーのルートノードを用意することで、使いたいノードが全て子ノードになるようにする
    pub fn new(name: &str) -> Self {
        let dummy_attr = TaskAttr::new(format!("dummy-for-{}", &name).as_str());
        let dummy_root = Node::new_tree(dummy_attr);

        let grant = dummy_root
            .tree()
            .grant_hierarchy_edit()
            .expect("tree grant");
        let task_attr = TaskAttr::new(name);
        dummy_root.create_as_last_child(&grant, task_attr);

        let node = dummy_root.first_child().expect("has a child");

        Self { node }
    }

    pub fn get_attr(&self) -> TaskAttr {
        // cloneして大丈夫か?
        self.node.borrow_data().clone()
    }

    pub fn get_id(&self) -> Uuid {
        *self.node.borrow_data().get_id()
    }

    pub fn set_id(&mut self, id: Uuid) {
        self.node.borrow_data_mut().set_id(id);
    }

    pub fn get_name(&self) -> String {
        self.node.borrow_data().get_name().to_string()
    }

    pub fn get_status(&self) -> Status {
        *self.node.borrow_data().get_status()
    }

    pub fn get_orig_status(&self) -> Status {
        *self.node.borrow_data().get_orig_status()
    }

    pub fn set_orig_status(&self, orig_status: Status) {
        self.node.borrow_data_mut().set_orig_status(orig_status);
    }

    pub fn get_is_on_other_side(&self) -> bool {
        *self.node.borrow_data().get_is_on_other_side()
    }

    pub fn set_is_on_other_side(&self, is_on_other_side: bool) {
        self.node
            .borrow_data_mut()
            .set_is_on_other_side(is_on_other_side);
    }

    pub fn set_pending_until(&self, pending_until: DateTime<Local>) {
        self.node.borrow_data_mut().set_pending_until(pending_until);
    }

    pub fn get_pending_until(&self) -> DateTime<Local> {
        *self.node.borrow_data().get_pending_until()
    }

    pub fn sync_clock(&self, now: DateTime<Local>) {
        self.node.borrow_data_mut().sync_clock(now);
    }

    pub fn get_last_synced_time(&self) -> DateTime<Local> {
        *self.node.borrow_data().get_last_synced_time()
    }

    pub fn set_priority(&self, priority: i64) {
        self.root().node.borrow_data_mut().set_priority(priority);
    }

    pub fn get_priority(&self) -> i64 {
        self.root().node.borrow_data().get_priority()
    }

    pub fn set_create_time(&self, create_time: DateTime<Local>) {
        self.node.borrow_data_mut().set_create_time(create_time);
    }

    pub fn get_create_time(&self) -> DateTime<Local> {
        *self.node.borrow_data().get_create_time()
    }

    pub fn set_start_time(&self, start_time: DateTime<Local>) {
        self.node.borrow_data_mut().set_start_time(start_time);
    }

    pub fn get_start_time(&self) -> DateTime<Local> {
        *self.node.borrow_data().get_start_time()
    }

    pub fn set_end_time_opt(&self, end_time_opt: Option<DateTime<Local>>) {
        self.node.borrow_data_mut().set_end_time_opt(end_time_opt);
    }

    pub fn get_end_time_opt(&self) -> Option<DateTime<Local>> {
        *self.node.borrow_data().get_end_time_opt()
    }

    // 親タスクと子タスクの〆切のうち、早いほうが子タスクの〆切となる
    // 〆切を設定する時には、子タスクに伝搬させていく
    // Noneの扱いが難しい。Noneを子に伝搬させても子の値に勝てないので、意味ないのでは?
    // 「親が〆切を持っている時は、子も必ず〆切を持っており、それは親より早いか等しい」という制約を維持させたい
    // Todo: 単体テスト
    pub fn set_deadline_time_opt(&self, deadline_time_opt: Option<DateTime<Local>>) {
        // Statusが既にdoneの時は何もしない。再帰もしないので止まる
        if self.get_status() == Status::Done {
            return;
        }

        // 引数で渡された(親タスクから伝搬してきた)値か、元々の値のうち早いほうを採用する
        let original_deadline_time_opt = self.get_deadline_time_opt();

        match deadline_time_opt {
            None => {
                // 何も起こらない。元々Noneなら変化なし、元々がNone以外なら早いほう採用でやはり変化なし。
            }
            Some(deadline_time) => {
                match original_deadline_time_opt {
                    None => {
                        // 引数で渡ってきたほうが勝つ
                        self.node
                            .borrow_data_mut()
                            .set_deadline_time_opt(Some(deadline_time));

                        // 子に伝搬させる
                        for child_node in self.node.children() {
                            let child_task = Self { node: child_node };
                            child_task.set_deadline_time_opt(Some(deadline_time));
                        }
                    }
                    Some(original_deadline_time) => {
                        // 早いほうを採用
                        let earlier_deadline_time = if original_deadline_time < deadline_time {
                            original_deadline_time
                        } else {
                            deadline_time
                        };

                        self.node
                            .borrow_data_mut()
                            .set_deadline_time_opt(Some(earlier_deadline_time));

                        // 子に伝搬させる
                        for child_node in self.node.children() {
                            let child_task = Self { node: child_node };
                            child_task.set_deadline_time_opt(Some(earlier_deadline_time));
                        }
                    }
                }
            }
        }
    }

    pub fn unset_deadline_time_opt(&self) {
        self.node.borrow_data_mut().set_deadline_time_opt(None);
    }

    pub fn get_deadline_time_opt(&self) -> Option<DateTime<Local>> {
        *self.node.borrow_data().get_deadline_time_opt()
    }

    pub fn set_estimated_work_seconds(&self, estimated_work_seconds: i64) {
        self.node
            .borrow_data_mut()
            .set_estimated_work_seconds(estimated_work_seconds);
    }

    pub fn get_estimated_work_seconds(&self) -> i64 {
        self.node.borrow_data().get_estimated_work_seconds()
    }

    pub fn set_actual_work_seconds(&self, actual_work_seconds: i64) {
        self.node
            .borrow_data_mut()
            .set_actual_work_seconds(actual_work_seconds);
    }

    pub fn get_repetition_interval_days_opt(&self) -> Option<i64> {
        self.node.borrow_data().get_repetition_interval_days_opt()
    }

    pub fn set_repetition_interval_days_opt(&self, repetition_interval_days_opt: Option<i64>) {
        self.node
            .borrow_data_mut()
            .set_repetition_interval_days_opt(repetition_interval_days_opt);
    }

    pub fn get_days_in_advance(&self) -> i64 {
        self.node.borrow_data().get_days_in_advance()
    }

    pub fn set_days_in_advance(&self, days_in_advance: i64) {
        self.node
            .borrow_data_mut()
            .set_days_in_advance(days_in_advance);
    }

    pub fn get_actual_work_seconds(&self) -> i64 {
        self.node.borrow_data().get_actual_work_seconds()
    }

    // TODO FIXME テスト
    pub fn get_children(&self) -> Vec<Task> {
        let children = self
            .node
            .children()
            .map(|node| Self { node })
            .collect::<Vec<_>>();

        children
    }

    pub fn make_appointment(&self, appointment_start_time: DateTime<Local>) {
        // マジックナンバーではある
        // 1,2,3,5...のフィボナッチ数列にて、充分大きな値55。アポを最優先として行動しなければならない
        self.set_priority(55);

        // 〆切については、子タスク全体に掛かるようにする
        let deadline_time =
            appointment_start_time + Duration::seconds(self.get_estimated_work_seconds());
        self.unset_deadline_time_opt();
        self.set_deadline_time_opt(Some(deadline_time));

        let mut attr = self.node.borrow_data_mut();
        attr.set_start_time(appointment_start_time);
    }

    pub fn num_children(&self) -> usize {
        self.node.num_children()
    }

    // 外から見て、ダミーノードのことは考慮させないように、ダミーの子の場合はNoneを返す
    pub fn parent(&self) -> Option<Self> {
        if self.node.parent() == Some(self.node.root()) {
            return None;
        }

        match self.node.parent() {
            Some(node) => Some(Task { node }),
            None => None,
        }
    }

    // 外から見て、ダミーノードのことは考慮させないように、ダミーノードの子を返す
    pub fn root(&self) -> Self {
        Task {
            node: self
                .node
                .root()
                .first_child()
                .expect("dummy_root has one child"),
        }
    }

    // 外から見て、ダミーノードのことは考慮させないように、ダミーノードの子で評価
    fn is_root(&self) -> bool {
        let root = self.root();
        self.node.ptr_eq(&root.node)
    }

    // pub fn try_eq_subtree(&self, task: &Task) -> Result<bool, BorrowError> {
    //     self.node.try_eq(&task.node)
    // }

    pub fn tree_debug_pretty_print(&self) -> String {
        format!("{:?}", &self.node.tree().debug_pretty_print())
    }

    pub fn try_eq_tree(&self, task: &Task) -> Result<bool, BorrowError> {
        self.node.tree().try_eq(&task.node.tree())
    }

    // pub fn insert_as_last_child(&self, task: Task) {
    pub fn detach_insert_as_last_child_of(&mut self, parent_task: Task) -> Result<(), String> {
        // taskのsubtreeをコピーしてselfを親から切り離して、parent_taskに結合する
        // という挙動を期待しているが、木を丸ごとくっつけるのはライブラリの仕様(?)により実現できていない。
        // https://gitlab.com/nop_thread/dendron/-/issues/3
        // 仕方がないので、Taskは必ずダミーのrootノードを持つという仕様にして対応している。

        // ダミーのrootノードで行おうとしている場合はエラーとする
        if self.node.is_root() {
            return Err(String::from("cannot use detach_insert for a root node"));
        }

        let self_grant = &self.node.tree().grant_hierarchy_edit().expect("self grant");

        let parent_task_hot: HotNode<TaskAttr> = parent_task
            .node
            .bundle_new_hierarchy_edit_grant()
            .expect("parent hot node");

        self.node
            .try_detach_insert_subtree(&self_grant, InsertAs::LastChildOf(&parent_task_hot))
            .expect("creating valid hierarchy");

        Ok(())
    }

    pub fn create_as_last_child(&self, task_attr: TaskAttr) -> Self {
        let self_grant = &self.node.tree().grant_hierarchy_edit().expect("self grant");

        let child_node = self.node.create_as_last_child(&self_grant, task_attr);
        Self { node: child_node }
    }

    // 親タスクとしてタスクを新規作成する
    #[allow(unused_must_use)]
    pub fn create_as_parent(&mut self, task_attr: TaskAttr) -> Result<(), String> {
        // ダミーのrootノードで行おうとしている場合はエラーとする
        match self.parent() {
            Some(original_parent_task) => {
                let new_task = original_parent_task.create_as_last_child(task_attr);
                self.detach_insert_as_last_child_of(new_task);
                Ok(())
            }
            None => {
                return Err(String::from("cannot use create_as_parent for a root node"));
            }
        }
    }

    // 連番の子タスクを生成する
    pub fn create_sequential_children(
        &self,
        task_name: &str,
        estimated_work_seconds: i64,
        begin_index: u64,
        end_index: u64,
        task_name_suffix: &str,
    ) -> Result<Task, String> {
        let tree_grant = &self.node.tree().grant_hierarchy_edit().expect("tree grant");
        let mut current_node_opt: Option<Node<TaskAttr>> = None;

        for index in (begin_index..=end_index).rev() {
            let task_name = format!("{} {}{}", task_name, index, task_name_suffix);
            let mut task_attr = TaskAttr::new(&task_name);
            task_attr.set_estimated_work_seconds(estimated_work_seconds);

            match current_node_opt {
                Some(current_node) => {
                    current_node_opt =
                        Some(current_node.create_as_last_child(&tree_grant, task_attr));
                }
                None => {
                    current_node_opt = Some(self.node.create_as_last_child(&tree_grant, task_attr));
                }
            }
        }

        match current_node_opt {
            Some(current_node) => Ok(Self { node: current_node }),
            None => Err(String::from("cannot create sequentially")),
        }
    }

    pub fn get_by_id(&self, id: Uuid) -> Option<Task> {
        let node_opt = self.get_by_id_private(&self.node, id);

        match node_opt {
            Some(node) => Some(Self { node }),
            None => None,
        }
    }

    // fn get_by_id_private(&self, id: Uuid) -> Option<&Task>> {
    fn get_by_id_private(&self, node: &Node<TaskAttr>, id: Uuid) -> Option<Node<TaskAttr>> {
        // ベースケース
        if node.borrow_data().get_id() == &id {
            return Some(node.clone());
        }

        // 子あり
        for child_node in node.children() {
            // let child_task = Task { node: child_node };

            // let child_task_found_opt =  child_task.get_by_id(id);
            // if  child_task_found_opt.is_some()   {
            //     return  child_task_found_opt;
            // }

            let child_task_found_opt = self.get_by_id_private(&child_node, id);
            if child_task_found_opt.is_some() {
                return child_task_found_opt;
            }
        }

        None
    }

    pub fn all_sibling_tasks_are_all_done(&self) -> bool {
        let mut ans = true;

        for sibling_node in self.node.siblings() {
            if sibling_node.borrow_data().get_status() != &Status::Done {
                ans = false;
                break;
            }
        }

        ans
    }

    // 親のタスクを考慮せずに、そのタスク単体で見た時に最速で着手できる時刻
    pub fn first_available_time(&self) -> DateTime<Local> {
        let dt_cand = if self.get_orig_status() == Status::Pending {
            vec![self.get_start_time(), self.get_pending_until()]
        } else {
            vec![self.get_start_time()]
        };

        // 1要素以上ありNoneになり得ないのでunwrap()してよい
        *dt_cand.iter().max().unwrap()
    }

    // 親を辿って、Todoのタスクを全て返す
    // タプルの1つ目は最速でTodo化するタイミング
    // ただし、〆切を守れるように、pending_untilよりも〆切を優先する
    pub fn list_all_parent_tasks_with_first_available_time(&self) -> Vec<(DateTime<Local>, Task)> {
        let mut ans: Vec<(DateTime<Local>, Task)> = vec![];
        let mut child_task_first_available_time: DateTime<Local> =
            DateTime::<Local>::MIN_UTC.into();

        // Phase1 子→親に辿って仮のfirst_available_timeを決定する
        //   max(子の最速着手時間 + 子の見積もり時間, 親のstart_time) = 親の最速着手時間

        // Phase2 〆切に対してオーバーしている時間を計算し、逆に親→子の順に時間を修正する

        // ここからPhase1: 子→親に辿って仮のfirst_available_timeを決定する
        let mut task_opt = Some(self.clone());
        loop {
            match task_opt {
                Some(task) => {
                    let first_available_time = task.first_available_time();
                    let dt_cand = vec![child_task_first_available_time, first_available_time];

                    // 2要素なのでNoneになることはない
                    child_task_first_available_time = *dt_cand.iter().max().unwrap();

                    let tpl = (child_task_first_available_time, task.clone());
                    ans.push(tpl);

                    // 再代入
                    task_opt = task.parent();
                }
                None => {
                    break;
                }
            }
        }

        // ここからPhase2: 〆切に対してオーバーしている時間を計算し、逆に親→子の順に時間を修正する
        // ansには子→親の順に結果が格納されているので、これを後ろから辿ればよい
        let mut bring_forward_duration = Duration::seconds(0);

        // 親のfirst_available_timeは〆切を考慮済みなので、それを子でも考慮するために一時保存する
        // 別のメソッドでset_deadline_time()する際に各タスクの見積もりまで考慮して設定するのは、見積もりを変えるたびにdeadline_timeを設定し直さなければいけないため複雑になる
        // そのため、このメソッド内で行う
        let mut parent_required_start_time_for_deadline = DateTime::<Local>::MAX_UTC.into();

        for (rough_first_available_time, task) in ans.iter_mut().rev() {
            // まず、親から引き継いできた早める時間ぶん前に倒す
            *rough_first_available_time = *rough_first_available_time - bring_forward_duration;

            if let Some(deadline_time) = task.get_deadline_time_opt() {
                let lateness_duration = *rough_first_available_time
                    + Duration::seconds(max(
                        0,
                        task.get_estimated_work_seconds() - task.get_actual_work_seconds(),
                    ))
                    - min(deadline_time, parent_required_start_time_for_deadline);

                if lateness_duration > Duration::seconds(0) {
                    *rough_first_available_time = *rough_first_available_time - lateness_duration;
                    bring_forward_duration = bring_forward_duration + lateness_duration;
                }
            }

            parent_required_start_time_for_deadline = *rough_first_available_time;
        }

        ans
    }
}

#[test]
fn test_task_new_タスクを初期化した時に見ているノードはダミーrootノードではないこと() {
    let task = Task::new("親タスク");
    assert_eq!(task.node.borrow_data().get_name(), "親タスク");
    assert!(!task.node.is_root());
}

#[test]
fn test_new_with_node_タスク化したnodeの親子関係が維持されること() {
    let parent_task = Task::new("親タスク");
    let parent_task_node_ptr = parent_task.node.clone();

    let mut child_task = Task::new("子タスク");
    child_task.create_as_last_child(TaskAttr::new("孫タスク"));

    child_task
        .detach_insert_as_last_child_of(parent_task)
        .unwrap();

    let grand_children_task_node = child_task.node.first_child().unwrap();
    let new_grand_children_task = Task {
        node: grand_children_task_node.clone(),
    };
    assert_eq!(
        new_grand_children_task.root().node.borrow_data().get_name(),
        "親タスク"
    );

    assert!(&parent_task_node_ptr.belongs_to_same_tree(&grand_children_task_node));
}

#[test]
fn test_make_appointment_正常系1() {
    let root_task = Task::new("MTGが完了した状態");
    let task = root_task.create_as_last_child(TaskAttr::new("MTG"));

    task.set_estimated_work_seconds(3600);
    let appointment_start_time = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();

    task.make_appointment(appointment_start_time);

    assert_eq!(&root_task.get_priority(), &55);

    assert_eq!(
        &task.get_start_time(),
        &Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap()
    );
    assert_eq!(
        &task.get_deadline_time_opt(),
        &Some(Local.with_ymd_and_hms(2023, 5, 19, 02, 23, 45).unwrap())
    );
}

#[test]
fn test_new_detach_insert_as_last_child_of_正常系1() {
    let parent_task = Task::new("親タスク");
    let mut child_task = Task::new("子タスク");
    let parent_task_ptr = parent_task.node.clone();
    let child_task_ptr = child_task.node.clone();

    child_task
        .detach_insert_as_last_child_of(parent_task)
        .unwrap();
    assert_eq!(*child_task.node.borrow_data(), TaskAttr::new("子タスク"));
    assert_eq!(
        *child_task.root().node.borrow_data(),
        TaskAttr::new("親タスク")
    );

    assert!(child_task.node.belongs_to_same_tree(&parent_task_ptr));
    assert!(child_task.node.belongs_to_same_tree(&child_task_ptr));
}

#[test]
fn test_create_as_last_child_正常系1() {
    let actual_task = Task::new("親タスク");
    actual_task.create_as_last_child(TaskAttr::new("子タスク"));

    let expected_tree = tree! {
    TaskAttr::new("dummy-for-親タスク"), [
        /(TaskAttr::new("親タスク"), [
            TaskAttr::new("子タスク")
        ])
    ]
    };

    assert_task_and_tree(&actual_task, &expected_tree);
}

#[test]
fn test_create_as_parent_正常系1() {
    let actual_task = Task::new("親タスク");
    let mut child_task = actual_task.create_as_last_child(TaskAttr::new("子タスク"));
    child_task.create_as_parent(TaskAttr::new("中タスク")).ok();

    let expected_tree = tree! {
    TaskAttr::new("dummy-for-親タスク"), [
        /(TaskAttr::new("親タスク"), [
            /(TaskAttr::new("中タスク"), [
                TaskAttr::new("子タスク")
            ])
        ])
    ]
    };

    assert_task_and_tree(&actual_task, &expected_tree);
}

#[test]
fn test_create_sequential_children_正常系1() {
    let task = Task::new("親タスク");
    let grand_child_task_result = task.create_sequential_children("鎖タスク", 600, 1, 2, "話");

    let mut child_attr = TaskAttr::new("鎖タスク 2話");
    child_attr.set_estimated_work_seconds(600);

    let mut grand_child_attr = TaskAttr::new("鎖タスク 1話");
    grand_child_attr.set_estimated_work_seconds(600);

    let expected_tree = tree! {
        TaskAttr::new("dummy-for-親タスク"), [
            /(TaskAttr::new("親タスク"), [
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
        _ => {
            assert!(false);
        }
    }
}

#[test]
#[allow(non_snake_case)]
fn test_create_sequential_children_異常系1_begin_indexのほうが大きい場合はエラー() {
    let task = Task::new("親タスク");
    let grand_child_task_result = task.create_sequential_children("鎖タスク", 600, 10, 1, "話");

    match grand_child_task_result {
        Ok(_) => {
            assert!(false);
        }
        _ => {
            // この分岐に入ることを意図している
            assert!(true);
        }
    }
}

#[cfg(test)]
fn get_tree_for_assert_debug(task1: &Task, task2: &Task) -> String {
    format!(
        "actual and expected are not equal:\n\n=== [actual] ===\n{}\n\n=== [expected] ===\n{}\n\n",
        &task1.tree_debug_pretty_print(),
        &task2.tree_debug_pretty_print(),
    )
}

#[cfg(test)]
pub fn assert_task(task1: &Task, task2: &Task) {
    let str_for_debug_string: String = get_tree_for_assert_debug(&task1, &task2);

    assert!(
        &task1.try_eq_tree(&task2).expect("data are not borrowed"),
        "{}",
        str_for_debug_string.as_str()
    );
}

#[cfg(test)]
fn get_task_tree_for_assert_debug(task1: &Task, tree: &Tree<TaskAttr>) -> String {
    format!(
        "actual and expected are not equal:\n\n=== [actual] ===\n{}\n\n=== [expected] ===\n{:?}\n\n",
        &task1.tree_debug_pretty_print(),
        &tree.debug_pretty_print(),
    )
}

#[cfg(test)]
pub fn assert_task_and_tree(task1: &Task, tree: &Tree<TaskAttr>) {
    let str_for_debug_string: String = get_task_tree_for_assert_debug(&task1, &tree);

    assert!(
        &task1
            .node
            .tree()
            .try_eq(&tree)
            .expect("data are not borrowed"),
        "{}",
        str_for_debug_string.as_str()
    );
}

// 詳細な構造を知っていたほうが構築しやすいので、gatewayではなくtaskの中で定義する
pub fn task_to_yaml(task: &Task) -> Yaml {
    let default_attr = TaskAttr::new("デフォルト用");

    let mut task_hash = LinkedHashMap::new();

    task_hash.insert(
        Yaml::String(String::from("name")),
        Yaml::String(String::from(task.get_name())),
    );

    task_hash.insert(
        Yaml::String(String::from("id")),
        Yaml::String(task.get_id().to_string()),
    );

    let orig_status = task.get_orig_status();
    if orig_status != *default_attr.get_orig_status() {
        task_hash.insert(
            Yaml::String(String::from("status")),
            Yaml::String(String::from(orig_status.to_string())),
        );
    }

    let is_on_other_side = task.get_is_on_other_side();
    if is_on_other_side != *default_attr.get_is_on_other_side() {
        task_hash.insert(
            Yaml::String(String::from("is_on_other_side")),
            Yaml::Boolean(is_on_other_side),
        );
    }

    let pending_until = task.get_pending_until();
    if pending_until != *default_attr.get_pending_until() {
        let pending_until_string = pending_until.format("%Y/%m/%d %H:%M:%S").to_string();
        task_hash.insert(
            Yaml::String(String::from("pending_until")),
            Yaml::String(pending_until_string),
        );
    }

    let priority = task.get_priority();
    if task.is_root() && priority != default_attr.get_priority() {
        task_hash.insert(
            Yaml::String(String::from("priority")),
            Yaml::Integer(priority),
        );
    }

    let create_time = task.get_create_time();
    let create_time_string = create_time.format("%Y/%m/%d %H:%M:%S").to_string();
    task_hash.insert(
        Yaml::String(String::from("create_time")),
        Yaml::String(create_time_string),
    );

    let start_time = task.get_start_time();
    let start_time_string = start_time.format("%Y/%m/%d %H:%M:%S").to_string();
    task_hash.insert(
        Yaml::String(String::from("start_time")),
        Yaml::String(start_time_string),
    );

    let end_time_opt = task.get_end_time_opt();
    match end_time_opt {
        Some(end_time) => {
            let end_time_string = end_time.format("%Y/%m/%d %H:%M:%S").to_string();
            task_hash.insert(
                Yaml::String(String::from("end_time")),
                Yaml::String(end_time_string),
            );
        }
        None => {}
    }

    let deadline_time_opt = task.get_deadline_time_opt();
    match deadline_time_opt {
        Some(deadline_time) => {
            let deadline_time_string = deadline_time.format("%Y/%m/%d %H:%M:%S").to_string();
            task_hash.insert(
                Yaml::String(String::from("deadline_time")),
                Yaml::String(deadline_time_string),
            );
        }
        None => {}
    }

    let estimated_work_seconds = task.get_estimated_work_seconds();
    if estimated_work_seconds != default_attr.get_estimated_work_seconds() {
        task_hash.insert(
            Yaml::String(String::from("estimated_work_seconds")),
            Yaml::Integer(estimated_work_seconds),
        );
    }

    let actual_work_seconds = task.get_actual_work_seconds();
    if actual_work_seconds != default_attr.get_actual_work_seconds() {
        task_hash.insert(
            Yaml::String(String::from("actual_work_seconds")),
            Yaml::Integer(actual_work_seconds),
        );
    }

    let repetition_interval_days_opt = task.get_repetition_interval_days_opt();
    match repetition_interval_days_opt {
        Some(repetition_interval_days) => {
            task_hash.insert(
                Yaml::String(String::from("repetition_interval_days")),
                Yaml::Integer(repetition_interval_days),
            );
        }
        None => {}
    }

    let days_in_advance = task.get_days_in_advance();
    if days_in_advance != default_attr.get_days_in_advance() {
        task_hash.insert(
            Yaml::String(String::from("days_in_advance")),
            Yaml::Integer(days_in_advance),
        );
    }

    let mut children = vec![];
    for child_node in task.node.children() {
        let child_task = Task { node: child_node };
        let child_yaml = task_to_yaml(&child_task);
        children.push(child_yaml);
    }

    if !children.is_empty() {
        task_hash.insert(
            Yaml::String(String::from("children")),
            Yaml::Array(children),
        );
    }

    Yaml::Hash(task_hash)
}

#[test]
fn test_task_to_yaml_正常系1_デフォルトの値と同じ場合は出力しない() {
    let mut task = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id);
    let now = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    task.set_create_time(now);
    task.set_start_time(now);
    let actual = task_to_yaml(&task);

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_正常系2_再帰() {
    let now = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    let mut task = Task::new("親タスク1");
    task.set_orig_status(Status::Pending);
    task.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    task.set_create_time(now);
    task.set_start_time(now);
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id);

    let mut task_attr_child_1 = TaskAttr::new("子タスク1");
    task_attr_child_1.set_orig_status(Status::Pending);
    task_attr_child_1.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    task_attr_child_1.set_create_time(now);
    task_attr_child_1.set_start_time(now);
    let id_child_1: Uuid = uuid!("0aaee735-3e22-4216-8b59-d56d5caf29ee");
    task_attr_child_1.set_id(id_child_1);

    let mut task_attr_child_2 = TaskAttr::new("子タスク2");
    task_attr_child_2.set_orig_status(Status::Pending);
    task_attr_child_2.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    task_attr_child_2.set_create_time(now);
    task_attr_child_2.set_start_time(now);
    let id_child_2: Uuid = uuid!("7ffcba2f-80e0-4a44-aee9-d68e0d2d1256");
    task_attr_child_2.set_id(id_child_2);

    task.create_as_last_child(task_attr_child_1);
    task.create_as_last_child(task_attr_child_2);

    let actual = task_to_yaml(&task);

    let s = "
name: '親タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
status: pending
pending_until: '2023/04/01 12:00:00'
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
children:
  - name: '子タスク1'
    id: 0aaee735-3e22-4216-8b59-d56d5caf29ee
    status: pending
    pending_until: '2023/04/01 12:00:00'
    create_time: '2023/05/19 01:23:45'
    start_time: '2023/05/19 01:23:45'
  - name: '子タスク2'
    id: 7ffcba2f-80e0-4a44-aee9-d68e0d2d1256
    status: pending
    pending_until: '2023/04/01 12:00:00'
    create_time: '2023/05/19 01:23:45'
    start_time: '2023/05/19 01:23:45'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_ユニークキー() {
    let mut task = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id);
    let now = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    task.set_create_time(now);
    task.set_start_time(now);
    let actual = task_to_yaml(&task);

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_is_on_other_side() {
    let mut task = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id);
    task.set_is_on_other_side(true);
    let now = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    task.set_create_time(now);
    task.set_start_time(now);
    let actual = task_to_yaml(&task);

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
is_on_other_side: true
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_end_time_opt() {
    let mut task = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id);
    task.set_is_on_other_side(true);
    task.set_create_time(Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap());
    task.set_start_time(Local.with_ymd_and_hms(2023, 5, 19, 02, 34, 56).unwrap());
    task.set_end_time_opt(Some(
        Local.with_ymd_and_hms(2023, 5, 19, 03, 45, 6).unwrap(),
    ));
    let actual = task_to_yaml(&task);

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
is_on_other_side: true
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 02:34:56'
end_time: '2023/05/19 03:45:06'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_deadline_time_opt() {
    let mut task = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id);
    task.set_is_on_other_side(true);
    task.set_create_time(Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap());
    task.set_start_time(Local.with_ymd_and_hms(2023, 5, 19, 02, 34, 56).unwrap());
    task.set_deadline_time_opt(Some(
        Local.with_ymd_and_hms(2023, 5, 19, 03, 45, 6).unwrap(),
    ));
    let actual = task_to_yaml(&task);

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
is_on_other_side: true
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 02:34:56'
deadline_time: '2023/05/19 03:45:06'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_estimated_work_seconds() {
    let mut task = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id);
    task.set_is_on_other_side(true);
    task.set_create_time(Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap());
    task.set_start_time(Local.with_ymd_and_hms(2023, 5, 19, 02, 34, 56).unwrap());
    task.set_estimated_work_seconds(1);
    let actual = task_to_yaml(&task);

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
is_on_other_side: true
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 02:34:56'
estimated_work_seconds: 1
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_actual_work_seconds() {
    let mut task = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id);
    task.set_is_on_other_side(true);
    task.set_create_time(Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap());
    task.set_start_time(Local.with_ymd_and_hms(2023, 5, 19, 02, 34, 56).unwrap());
    task.set_actual_work_seconds(1);
    let actual = task_to_yaml(&task);

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
is_on_other_side: true
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 02:34:56'
actual_work_seconds: 1
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_repetition_interval() {
    let mut task = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id);
    task.set_repetition_interval_days_opt(Some(7));
    let now = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    task.set_create_time(now);
    task.set_start_time(now);
    let actual = task_to_yaml(&task);

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
repetition_interval_days: 7
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_days_in_advance() {
    let mut task = Task::new("タスク1");
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id);
    task.set_days_in_advance(1);
    let now = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    task.set_create_time(now);
    task.set_start_time(now);
    let actual = task_to_yaml(&task);

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
days_in_advance: 1
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_get_by_id_ベースケース() {
    let mut task = Task::new("親タスク1");
    task.set_orig_status(Status::Pending);
    task.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id);

    let task_ptr = &task.node;

    let actual_opt = task.get_by_id(id);
    match actual_opt {
        Some(actual) => {
            assert_eq!(&actual, &task);
            assert!(&actual.node.ptr_eq(&task_ptr));
        }
        None => {
            assert!(false);
        }
    }
}

#[test]
fn test_get_by_id_子なしタスクでヒットしなかった場合() {
    let mut task = Task::new("親タスク1");
    task.set_orig_status(Status::Pending);
    task.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id);

    let actual = task.get_by_id(uuid!("ccdadeab-f60a-4bec-93f8-3d7e003b980f"));

    assert_eq!(actual, None);
}

#[test]
fn test_get_by_id_再帰でヒットする場合() {
    let mut task = Task::new("親タスク1");
    task.set_orig_status(Status::Pending);
    task.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    let id_parent: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id_parent);

    let mut task_attr_child_1 = TaskAttr::new("子タスク1");
    task_attr_child_1.set_orig_status(Status::Pending);
    task_attr_child_1.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    let id_child_1: Uuid = uuid!("0aaee735-3e22-4216-8b59-d56d5caf29ee");
    task_attr_child_1.set_id(id_child_1);

    let mut task_attr_child_2 = TaskAttr::new("子タスク2");
    task_attr_child_2.set_orig_status(Status::Pending);
    task_attr_child_2.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    let id_child_2: Uuid = uuid!("7ffcba2f-80e0-4a44-aee9-d68e0d2d1256");
    task_attr_child_2.set_id(id_child_2);

    let expected_attr = task_attr_child_1.clone();

    task.create_as_last_child(task_attr_child_1);
    task.create_as_last_child(task_attr_child_2);

    let actual_opt = task.get_by_id(id_child_1);
    match actual_opt {
        None => {
            panic!("assert some");
        }
        Some(actual) => {
            assert_eq!(&actual.get_attr(), &expected_attr);

            // 親をたどることができること
            assert_eq!(&actual.root(), &task);
        }
    }
}

#[test]
fn test_get_by_id_再帰でヒットしない場合() {
    let mut task = Task::new("親タスク1");
    task.set_orig_status(Status::Pending);
    task.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    let id_parent: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id_parent);

    let mut task_attr_child_1 = TaskAttr::new("子タスク1");
    task_attr_child_1.set_orig_status(Status::Pending);
    task_attr_child_1.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    let id_child_1: Uuid = uuid!("0aaee735-3e22-4216-8b59-d56d5caf29ee");
    task_attr_child_1.set_id(id_child_1);

    let mut task_attr_child_2 = TaskAttr::new("子タスク2");
    task_attr_child_2.set_orig_status(Status::Pending);
    task_attr_child_2.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    let id_child_2: Uuid = uuid!("7ffcba2f-80e0-4a44-aee9-d68e0d2d1256");
    task_attr_child_2.set_id(id_child_2);

    task.create_as_last_child(task_attr_child_1);
    task.create_as_last_child(task_attr_child_2);

    let actual = task.get_by_id(uuid!("3aa89504-917d-4f20-a1e3-4eb196190c6f"));
    assert_eq!(actual, None);
}

#[test]
fn test_all_sibling_tasks_are_all_done_全ての兄弟タスクが完了していたらtrueとなる() {
    /*
     parent_task_1
       - child_task_1 (完了)
       - child_task_2 (完了)
    */

    let parent_task = Task::new("親タスク");

    let mut task_attr_child_1 = TaskAttr::new("子タスク1");
    task_attr_child_1.set_orig_status(Status::Done);

    let mut task_attr_child_2 = TaskAttr::new("子タスク2");
    task_attr_child_2.set_orig_status(Status::Done);

    let child_task_1 = parent_task.create_as_last_child(task_attr_child_1);
    parent_task.create_as_last_child(task_attr_child_2);

    assert!(child_task_1.all_sibling_tasks_are_all_done());
}

#[test]
fn test_all_sibling_tasks_are_all_done_一部の兄弟タスクが完了でない場合はfalseとなる() {
    /*
     parent_task_1
       - child_task_1 (完了)
       - child_task_2 (Todo)
    */

    let parent_task = Task::new("親タスク");

    let mut task_attr_child_1 = TaskAttr::new("子タスク1");
    task_attr_child_1.set_orig_status(Status::Done);

    let mut task_attr_child_2 = TaskAttr::new("子タスク2");
    task_attr_child_2.set_orig_status(Status::Todo);

    let child_task_1 = parent_task.create_as_last_child(task_attr_child_1);
    parent_task.create_as_last_child(task_attr_child_2);

    assert!(!child_task_1.all_sibling_tasks_are_all_done());
}

#[test]
fn test_parent_ルートタスクの場合() {
    /*
     parent_task_1
    */

    let parent_task = Task::new("親タスク");
    assert_eq!(parent_task.parent(), None);
}

#[test]
fn test_parent_親タスクがある場合() {
    /*
     parent_task_1
       - child_task_1
    */

    let parent_task = Task::new("親タスク");

    let task_attr_child_1 = TaskAttr::new("子タスク1");
    let child_task_1 = parent_task.create_as_last_child(task_attr_child_1);

    match child_task_1.parent() {
        Some(actual_task) => {
            assert_task(&actual_task, &parent_task);
        }
        None => {
            assert!(false);
        }
    }
}

#[test]
fn test_taskをcloneした場合はnodeは同じ木を指すポインタであること() {
    let task_orig = Task::new("タスク");
    let task_cloned = task_orig.clone();

    assert!(&task_orig.node.ptr_eq(&task_cloned.node));
}

#[test]
fn test_first_available_time_pending状態の時はpending_untilとstart_timeの大きい方が採用されること_pending_untilの方が大きい場合(
) {
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    let parent_task = Task::new("親タスク");
    parent_task.set_create_time(dt);
    parent_task.set_start_time(dt);
    parent_task.set_orig_status(Status::Pending);
    parent_task.set_pending_until(dt + Duration::hours(1));
    parent_task.sync_clock(dt);

    let actual = parent_task.first_available_time();
    let expected = dt + Duration::hours(1);

    assert_eq!(actual, expected);
}

#[test]
fn test_list_all_parent_tasks_with_first_available_time_タスク1個でpending状態の時はpending_untilとstart_timeの大きい方が採用されること_pending_untilの方が大きい場合(
) {
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    let parent_task = Task::new("親タスク");
    parent_task.set_create_time(dt);
    parent_task.set_start_time(dt);
    parent_task.set_orig_status(Status::Pending);
    parent_task.set_pending_until(dt + Duration::hours(1));
    parent_task.sync_clock(dt);

    let actual = parent_task.list_all_parent_tasks_with_first_available_time();
    let expected = [(dt + Duration::hours(1), parent_task)];

    assert_eq!(actual, expected);
}

#[test]
fn test_list_all_parent_tasks_with_first_available_time_タスク1個でpending状態の時はpending_untilとstart_timeの大きい方が採用されること_deadline_timeのほうが小さい場合(
) {
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    let parent_task = Task::new("親タスク");
    parent_task.set_create_time(dt);
    parent_task.set_start_time(dt);
    parent_task.set_estimated_work_seconds(3600);
    parent_task.set_orig_status(Status::Pending);
    parent_task.set_pending_until(dt + Duration::hours(1));
    parent_task.set_deadline_time_opt(Some(dt - Duration::hours(1)));
    parent_task.sync_clock(dt);

    let actual = parent_task.list_all_parent_tasks_with_first_available_time();
    let expected = [(
        dt - Duration::hours(1) - Duration::seconds(3600),
        parent_task,
    )];

    assert_eq!(actual, expected);
}

#[test]
fn test_first_available_time_pending状態の時はpending_untilとstart_timeの大きい方が採用されること_start_timeの方が大きい場合(
) {
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    let parent_task = Task::new("親タスク");
    parent_task.set_create_time(dt);
    parent_task.set_start_time(dt + Duration::hours(2));
    parent_task.set_orig_status(Status::Pending);
    parent_task.set_pending_until(dt + Duration::hours(1));
    parent_task.sync_clock(dt);

    let actual = parent_task.first_available_time();
    let expected = dt + Duration::hours(2);

    assert_eq!(actual, expected);
}

#[test]
fn test_list_all_parent_tasks_with_first_available_time_タスク1個でpending状態の時はpending_untilとstart_timeの大きい方が採用されること_start_timeの方が大きい場合(
) {
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    let parent_task = Task::new("親タスク");
    parent_task.set_create_time(dt);
    parent_task.set_start_time(dt + Duration::hours(2));
    parent_task.set_orig_status(Status::Pending);
    parent_task.set_pending_until(dt + Duration::hours(1));
    parent_task.sync_clock(dt);

    let actual = parent_task.list_all_parent_tasks_with_first_available_time();
    let expected = [(dt + Duration::hours(2), parent_task)];

    assert_eq!(actual, expected);
}

#[test]
fn test_list_all_parent_tasks_with_first_available_time_タスク1個でpending状態ではない時はstart_timeが採用されること(
) {
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    let parent_task = Task::new("親タスク");
    parent_task.set_create_time(dt);
    parent_task.set_start_time(dt + Duration::hours(1));
    parent_task.set_orig_status(Status::Todo);
    parent_task.set_pending_until(dt + Duration::hours(2));
    parent_task.sync_clock(dt);

    let actual = parent_task.list_all_parent_tasks_with_first_available_time();
    let expected = [(dt + Duration::hours(1), parent_task)];

    assert_eq!(actual, expected);
}

#[test]
fn test_first_available_time_pending状態ではない時はstart_timeが採用されること() {
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    let parent_task = Task::new("親タスク");
    parent_task.set_create_time(dt);
    parent_task.set_start_time(dt + Duration::hours(1));
    parent_task.set_orig_status(Status::Todo);
    parent_task.set_pending_until(dt + Duration::hours(2));
    parent_task.sync_clock(dt);

    let actual = parent_task.first_available_time();
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
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    let parent_task = Task::new("親タスク");
    parent_task.set_create_time(dt);
    parent_task.set_start_time(dt);

    let mut child_task = Task::new("子タスク");
    child_task.set_create_time(dt);
    child_task.set_start_time(dt);

    let grand_child_task = child_task.create_as_last_child(TaskAttr::new("孫タスク"));
    grand_child_task.set_create_time(dt);
    grand_child_task.set_start_time(dt);

    let expected = vec![
        (dt, grand_child_task.clone()),
        (dt, child_task.clone()),
        (dt, parent_task.clone()),
    ];

    child_task
        .detach_insert_as_last_child_of(parent_task)
        .unwrap();

    let actual = grand_child_task.list_all_parent_tasks_with_first_available_time();

    assert_eq!(actual, expected);
}

#[test]
fn test_list_all_parent_tasks_with_first_available_time_葉に〆切がある場合() {
    /*
     parent_task_1
       - child_task_1
         - grand_child_task (葉)
    */
    let dt = Local.with_ymd_and_hms(2023, 5, 19, 01, 23, 45).unwrap();
    let parent_task = Task::new("親タスク");
    parent_task.set_create_time(dt);
    parent_task.set_start_time(dt);

    let mut child_task = Task::new("子タスク");
    child_task.set_create_time(dt);
    child_task.set_start_time(dt);

    let grand_child_task = child_task.create_as_last_child(TaskAttr::new("孫タスク"));
    grand_child_task.set_create_time(dt);
    grand_child_task.set_start_time(dt);
    grand_child_task.set_estimated_work_seconds(3600);
    grand_child_task.set_deadline_time_opt(Some(dt - Duration::hours(1)));

    let expected = vec![
        (
            dt - Duration::hours(1) - Duration::seconds(3600),
            grand_child_task.clone(),
        ),
        (dt, child_task.clone()),
        (dt, parent_task.clone()),
    ];

    child_task
        .detach_insert_as_last_child_of(parent_task)
        .unwrap();

    let actual = grand_child_task.list_all_parent_tasks_with_first_available_time();

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
    let parent_task = Task::new("親タスク");
    parent_task.set_create_time(dt);
    parent_task.set_start_time(dt);
    parent_task.set_estimated_work_seconds(7200);
    parent_task.set_deadline_time_opt(Some(dt + Duration::hours(24)));

    let mut child_task = Task::new("子タスク");
    child_task.set_create_time(dt);
    child_task.set_start_time(dt);
    child_task.set_estimated_work_seconds(10800);
    child_task.set_deadline_time_opt(Some(dt + Duration::hours(24)));

    let grand_child_task = child_task.create_as_last_child(TaskAttr::new("孫タスク"));
    grand_child_task.set_create_time(dt);
    grand_child_task.set_start_time(dt);
    grand_child_task.set_estimated_work_seconds(3600);
    grand_child_task.set_deadline_time_opt(Some(dt + Duration::hours(24)));
    grand_child_task.set_pending_until(dt + Duration::hours(22));
    grand_child_task.set_orig_status(Status::Pending);

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

    let actual = grand_child_task.list_all_parent_tasks_with_first_available_time();

    assert_eq!(actual, expected);
}
