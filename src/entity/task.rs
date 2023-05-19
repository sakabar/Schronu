use chrono::{DateTime, Local};
use core::cell::BorrowError;
use dendron::{HotNode, InsertAs, Node};
use linked_hash_map::LinkedHashMap;
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

pub fn extract_leaf_tasks_from_project(task: &Task) -> Vec<Task> {
    let children_are_all_done = task
        .node
        .children()
        .all(|child_node| child_node.borrow_data().get_status() == &Status::Done);

    if task.get_status() == Status::Todo && (!task.node.has_children() || children_are_all_done) {
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

            let leaves_with_pending: Vec<Task> = extract_leaf_tasks_from_project(&child_task);

            let mut leaves: Vec<Task> = leaves_with_pending
                .iter()
                .filter(|&leaf| leaf.get_status() != Status::Pending)
                .map(|leaf| Task {
                    node: leaf.node.clone(),
                })
                .collect::<Vec<_>>();
            ans.append(&mut leaves);
        }
    }

    return ans;
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

    create_time: DateTime<Local>,   // タスクが生成された日時
    start_time: DateTime<Local>,    // タスクが着手可能になった日時
    end_time: DateTime<Local>,      // タスクが完了した日時
    deadline_time: DateTime<Local>, // タスクの〆切

    estimated_work_seconds : i64 , // 見積もられた作業時間 (秒)
    actual_work_seconds : i64 , // 実際の作業時間 (秒)
}

// idはあくまで検索用に使い、等価性判定には用いない
impl PartialEq for TaskAttr {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.orig_status == other.orig_status
            && self.status == other.status
            && self.is_on_other_side == other.is_on_other_side
            && self.pending_until == other.pending_until
            && self.last_synced_time == other.last_synced_time
            && self.priority == other.priority
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
                &format!("{} {}", status_checkbox, &self.name).as_str(),
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
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            orig_status: Status::Todo,
            status: Status::Todo,
            is_on_other_side: false,
            pending_until: DateTime::<Local>::MIN_UTC.into(),
            last_synced_time: DateTime::<Local>::MIN_UTC.into(),
            priority: 0,
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

        self.status =
            if self.orig_status == Status::Pending && self.last_synced_time > self.pending_until {
                Status::Todo
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
        self.node.borrow_data_mut().set_priority(priority);
    }

    pub fn get_priority(&self) -> i64 {
        self.node.borrow_data().get_priority()
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
    if priority != default_attr.get_priority() {
        task_hash.insert(
            Yaml::String(String::from("priority")),
            Yaml::Integer(priority),
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
    let actual = task_to_yaml(&task);

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
";

    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_正常系2_再帰() {
    let mut task = Task::new("親タスク1");
    task.set_orig_status(Status::Pending);
    task.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id);

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

    let actual = task_to_yaml(&task);

    let s = "
name: '親タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
status: pending
pending_until: '2023/04/01 12:00:00'
children:
  - name: '子タスク1'
    id: 0aaee735-3e22-4216-8b59-d56d5caf29ee
    status: pending
    pending_until: '2023/04/01 12:00:00'
  - name: '子タスク2'
    id: 7ffcba2f-80e0-4a44-aee9-d68e0d2d1256
    status: pending
    pending_until: '2023/04/01 12:00:00'
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
    let actual = task_to_yaml(&task);

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
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
    let actual = task_to_yaml(&task);

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
is_on_other_side: true
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
