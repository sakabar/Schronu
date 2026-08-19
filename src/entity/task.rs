use chrono::{DateTime, Duration, Local};
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

    None
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum RepetitionAnchor {
    Deadline,
    Completion,
}

impl fmt::Display for RepetitionAnchor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RepetitionAnchor::Deadline => write!(f, "deadline"),
            RepetitionAnchor::Completion => write!(f, "completion"),
        }
    }
}

pub fn read_repetition_anchor(s: &str) -> RepetitionAnchor {
    match s.to_lowercase().as_str() {
        "completion" => RepetitionAnchor::Completion,
        _ => RepetitionAnchor::Deadline,
    }
}

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub enum ProjectCategory {
    Earning,
    Sustaining,
    Recovery,
    Investment,
    Consumption,
}

impl fmt::Display for ProjectCategory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProjectCategory::Earning => write!(f, "earning"),
            ProjectCategory::Sustaining => write!(f, "sustaining"),
            ProjectCategory::Recovery => write!(f, "recovery"),
            ProjectCategory::Investment => write!(f, "investment"),
            ProjectCategory::Consumption => write!(f, "consumption"),
        }
    }
}

pub fn read_project_category(s: &str) -> Option<ProjectCategory> {
    match s.to_lowercase().as_str() {
        "earning" | "獲" => Some(ProjectCategory::Earning),
        "sustaining" | "維" => Some(ProjectCategory::Sustaining),
        "recovery" | "回" => Some(ProjectCategory::Recovery),
        "investment" | "資" => Some(ProjectCategory::Investment),
        "consumption" | "消" => Some(ProjectCategory::Consumption),
        _ => None,
    }
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

    pub fn new_with_current_time(
        name: String,
        status: Status,
        pending_until: DateTime<Local>,
        children: Vec<ImmutableTask>,
        now: DateTime<Local>,
    ) -> Self {
        let new_status = if status == Status::Pending && now > pending_until {
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
        &self.name
    }

    pub fn get_status(&self) -> &Status {
        &self.status
    }

    pub fn get_children(&self) -> &Vec<ImmutableTask> {
        &self.children
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
                .copied()
                .collect::<Vec<_>>();
            ans.append(&mut leaves);
        }
    }

    ans
}

// Todoの葉タスクを抽出する
pub fn extract_leaf_tasks_from_project(
    task: &TaskHandle,
) -> Result<Vec<TaskHandle>, TaskTreeError> {
    let target_status: Vec<Status> = vec![Status::Todo];
    extract_leaf_tasks_from_project_rec(task, &target_status)
}

// TodoもしくはPendingの葉タスクを抽出する
pub fn extract_leaf_tasks_from_project_with_pending(
    task: &TaskHandle,
) -> Result<Vec<TaskHandle>, TaskTreeError> {
    let target_status: Vec<Status> = vec![Status::Todo, Status::Pending];
    extract_leaf_tasks_from_project_rec(task, &target_status)
}

fn extract_leaf_tasks_from_project_rec(
    task: &TaskHandle,
    target_status_arr: &Vec<Status>,
) -> Result<Vec<TaskHandle>, TaskTreeError> {
    let mut children_are_all_done = true;
    for child_node in task.node.children() {
        if child_node
            .try_borrow_data()
            .map_err(|_| TaskTreeError::Borrow)?
            .get_status()
            != &Status::Done
        {
            children_are_all_done = false;
            break;
        }
    }

    if target_status_arr.contains(&task.get_status()?)
        && (!task.node.has_children() || children_are_all_done)
    {
        let new_task = TaskHandle {
            node: task.node.clone(),
        };
        return Ok(vec![new_task]);
    }

    let mut ans: Vec<TaskHandle> = vec![];

    // 深さ優先
    for child_node in task.node.children() {
        if child_node
            .try_borrow_data()
            .map_err(|_| TaskTreeError::Borrow)?
            .get_status()
            != &Status::Done
        {
            let child_task = TaskHandle { node: child_node };

            let leaves_with_pending: Vec<TaskHandle> =
                extract_leaf_tasks_from_project_rec(&child_task, target_status_arr)?;

            let mut leaves = Vec::new();
            for leaf in leaves_with_pending {
                if target_status_arr.contains(&leaf.get_status()?) {
                    leaves.push(TaskHandle {
                        node: leaf.node.clone(),
                    });
                }
            }
            ans.append(&mut leaves);
        }
    }

    Ok(ans)
}

pub fn round_up_sec_as_minute(seconds: i64) -> i64 {
    seconds / 60 + if seconds % 60 == 0 { 0 } else { 1 }
}

// pub fn extract_leaf_tasks_from_project_ref(task: &TaskHandle) -> Vec<&TaskAttr> {
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

////////////////// ここから要テスト

// #[test]
// fn test_extract_leaf_tasks_from_project_done状態のタスクとその子孫は全て無視されること() {
//     /*
//      parent_task_1
//        - child_task_1 (Done)
//          - grand_child_task (todo, だが親がdoneなので無視される)
//        - child_task_2
//     */
//     let grand_child_task_1 = TaskHandle::new_with_name("孫タスク1".to_string());
//     let child_task_1 = TaskHandle::new_with_name_status_children(
//         "子タスク1".to_string(),
//         Status::Done,
//         vec![grand_child_task_1],
//     );

//     let child_task_2 = TaskHandle::new_with_name("子タスク2".to_string());

//     let parent_task_1 = TaskHandle::new_with_name_children(
//         "親タスク1".to_string(),
//         vec![child_task_1, child_task_2],
//     );

//     let actual = extract_leaf_tasks_from_project(&parent_task_1);
//     let expected_child_task_2 = TaskHandle::new_with_name("子タスク2".to_string());
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
//     let grand_child_task_1 = TaskHandle::new_with_name("孫タスク1".to_string());
//     let child_task_1 = TaskHandle::new_with_name_status_children(
//         "子タスク1".to_string(),
//         Status::Pending,
//         vec![grand_child_task_1],
//     );

//     let child_task_2 = TaskHandle::new_with_name_status_children(
//         "子タスク2".to_string(),
//         Status::Pending,
//         vec![],
//     );

//     let parent_task_1 = TaskHandle::new_with_name_children(
//         "親タスク1".to_string(),
//         vec![child_task_1, child_task_2],
//     );

//     let actual = extract_leaf_tasks_from_project(&parent_task_1);
//     let expected_grand_child_task_1 = TaskHandle::new_with_name("孫タスク1".to_string());
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

#[derive(Clone)]
pub struct TaskAttr {
    id: Uuid,
    name: String,
    orig_status: Status, // 元々のステータス。orig_status=Pendingの時、時刻によらずPendingのまま。
    status: Status, // 評価後のステータス。pendingはpending_untilを加味して評価され、Todo扱いとなる
    is_on_other_side: bool, // 相手ボールか?
    atomic: bool,   // 分割できないタスクか?
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
    repetition_anchor: RepetitionAnchor,
    days_in_advance: i64, // 繰り返しタスクについて、何日前から着手開始可能とするか
    project_category_opt: Option<ProjectCategory>,
    persistent_mutation_revision: u64,
}

// 生成するタイミングで結果が変わってしまうid, create_time, start_timeは
// 等価性判定には用いない
impl PartialEq for TaskAttr {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.orig_status == other.orig_status
            && self.status == other.status
            && self.is_on_other_side == other.is_on_other_side
            && self.atomic == other.atomic
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
            && self.repetition_anchor == other.repetition_anchor
            && self.days_in_advance == other.days_in_advance
            && self.project_category_opt == other.project_category_opt
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
                    self.name
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

        Self::with_identity(name, Uuid::new_v4(), now)
    }

    pub fn with_identity(name: &str, id: Uuid, now: DateTime<Local>) -> Self {
        Self {
            id,
            name: name.to_string(),
            orig_status: Status::Todo,
            status: Status::Todo,
            is_on_other_side: false,
            atomic: false,
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
            repetition_anchor: RepetitionAnchor::Deadline,
            days_in_advance: 0,
            project_category_opt: None,
            persistent_mutation_revision: 0,
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
        if let Some(deadline_time) = self
            .deadline_time_opt
            .filter(|_| self.orig_status == Status::Pending)
        {
            let pending_time_before_deadline = deadline_time
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
        let should_be_todo = (self.orig_status != Status::Done
            && self.last_synced_time > self.start_time
            && self.deadline_time_opt.is_some()
            && self.deadline_time_opt.unwrap()
                - Duration::seconds(max(
                    0,
                    self.estimated_work_seconds - self.actual_work_seconds,
                ))
                - Duration::seconds(deadline_buffer_seconds_after_start_time)
                < self.last_synced_time)
            || (self.orig_status != Status::Done
                && self.last_synced_time < self.start_time
                && self.deadline_time_opt.is_some()
                && self.deadline_time_opt.unwrap()
                    - Duration::seconds(self.estimated_work_seconds)
                    - Duration::seconds(deadline_buffer_seconds_before_start_time)
                    < self.last_synced_time)
            || (self.orig_status == Status::Pending
                && self.last_synced_time > self.pending_until
                && self.last_synced_time > self.start_time);

        self.status = if should_be_todo {
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

    pub fn get_atomic(&self) -> bool {
        self.atomic
    }

    pub fn set_atomic(&mut self, atomic: bool) {
        self.atomic = atomic;
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

    pub fn set_repetition_anchor(&mut self, repetition_anchor: RepetitionAnchor) {
        self.repetition_anchor = repetition_anchor;
    }

    pub fn get_repetition_anchor(&self) -> RepetitionAnchor {
        self.repetition_anchor
    }

    pub fn set_days_in_advance(&mut self, days_in_advance: i64) {
        self.days_in_advance = days_in_advance;
    }

    pub fn get_days_in_advance(&self) -> i64 {
        self.days_in_advance
    }

    pub fn set_project_category_opt(&mut self, project_category_opt: Option<ProjectCategory>) {
        self.project_category_opt = project_category_opt;
    }

    pub fn get_project_category_opt(&self) -> Option<ProjectCategory> {
        self.project_category_opt
    }
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

#[derive(Clone, Debug, PartialEq)]
pub struct TaskHandle {
    node: Node<TaskAttr>,
}

/// An owned, recursively independent view of a task subtree.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskSnapshot {
    attr: TaskAttr,
    children: Vec<Self>,
}

impl TaskSnapshot {
    pub fn attr(&self) -> &TaskAttr {
        &self.attr
    }

    pub fn name(&self) -> &str {
        self.attr.get_name()
    }

    pub fn estimated_work_seconds(&self) -> i64 {
        self.attr.get_estimated_work_seconds()
    }

    pub fn children(&self) -> &[Self] {
        &self.children
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskTreeError {
    RootOperation,
    Cycle,
    InvalidSequence,
    Borrow,
    HierarchyGrant,
    Insert,
    /// The hidden dummy root no longer has exactly one task child, or this handle is outside it.
    MissingDummyRootChild,
}

impl fmt::Display for TaskTreeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self {
            Self::RootOperation => "cannot modify the project root hierarchy",
            Self::Cycle => "cannot insert a task into its own descendant",
            Self::InvalidSequence => "sequential child range must not be empty",
            Self::Borrow => "cannot borrow task tree data",
            Self::HierarchyGrant => "cannot acquire hierarchy edit grant",
            Self::Insert => "cannot insert task subtree",
            Self::MissingDummyRootChild => {
                "task tree dummy root must have exactly one task child containing this handle"
            }
        };
        formatter.write_str(reason)
    }
}

impl std::error::Error for TaskTreeError {}

#[cfg(test)]
fn next_test_task_id() -> Uuid {
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    Uuid::from_u128(u128::from(SEQUENCE.fetch_add(1, Ordering::Relaxed)))
}

#[cfg(test)]
fn test_task_time() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 8, 19, 0, 0, 0).unwrap()
}

#[cfg(test)]
fn new_test_task_attr(name: &str) -> TaskAttr {
    TaskAttr::with_identity(name, next_test_task_id(), test_task_time())
}

#[cfg(test)]
fn new_test_task_handle(name: &str) -> Result<TaskHandle, TaskTreeError> {
    TaskHandle::with_identity(name, next_test_task_id(), test_task_time())
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

    root.create_sequential_children("step", 60, 1, 2, "", (test_task_time(), next_test_task_id))
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

impl TaskHandle {
    // dendron::Node::try_detach_insert_subtree()は木そのものを消滅させることができない仕様のようなので、
    // ダミーのルートノードを用意することで、使いたいノードが全て子ノードになるようにする
    pub fn new(name: &str) -> Result<Self, TaskTreeError> {
        let dummy_attr = TaskAttr::new(format!("dummy-for-{}", name).as_str());
        let dummy_root = Node::new_tree(dummy_attr);

        let grant = dummy_root
            .tree()
            .grant_hierarchy_edit()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;
        let task_attr = TaskAttr::new(name);
        dummy_root.create_as_last_child(&grant, task_attr);

        let node = dummy_root
            .first_child()
            .ok_or(TaskTreeError::MissingDummyRootChild)?;

        Ok(Self { node })
    }

    pub fn with_identity(
        name: &str,
        id: Uuid,
        now: DateTime<Local>,
    ) -> Result<Self, TaskTreeError> {
        let dummy_attr =
            TaskAttr::with_identity(format!("dummy-for-{}", name).as_str(), Uuid::nil(), now);
        let dummy_root = Node::new_tree(dummy_attr);

        let grant = dummy_root
            .tree()
            .grant_hierarchy_edit()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;
        let task_attr = TaskAttr::with_identity(name, id, now);
        dummy_root.create_as_last_child(&grant, task_attr);

        let node = dummy_root
            .first_child()
            .ok_or(TaskTreeError::MissingDummyRootChild)?;

        Ok(Self { node })
    }

    pub fn get_attr(&self) -> Result<TaskAttr, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.clone())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn parent(&self) -> Result<Option<Self>, TaskTreeError> {
        if self.node.parent() == Some(self.node.root()) {
            return Ok(None);
        }

        Ok(self.node.parent().map(|node| Self { node }))
    }

    pub fn root(&self) -> Result<Self, TaskTreeError> {
        self.get_attr()?;
        let dummy_root = self.node.root();
        let mut children = dummy_root.children();
        let task_root = children
            .next()
            .ok_or(TaskTreeError::MissingDummyRootChild)?;
        if children.next().is_some() {
            return Err(TaskTreeError::MissingDummyRootChild);
        }

        let mut ancestor = Some(self.node.clone());
        while let Some(node) = ancestor {
            if node.ptr_eq(&task_root) {
                return Ok(Self { node: task_root });
            }
            ancestor = node.parent();
        }

        Err(TaskTreeError::MissingDummyRootChild)
    }

    pub fn get_children(&self) -> Result<Vec<Self>, TaskTreeError> {
        self.get_attr()?;
        Ok(self.node.children().map(|node| Self { node }).collect())
    }

    #[cfg(test)]
    pub(crate) fn with_exclusive_data_borrow_for_test<T>(&self, action: impl FnOnce() -> T) -> T {
        let _exclusive_borrow = self.node.borrow_data_mut();
        action()
    }

    #[cfg(test)]
    pub(crate) fn with_shared_data_borrow_for_test<T>(&self, action: impl FnOnce() -> T) -> T {
        let _shared_borrow = self.node.borrow_data();
        action()
    }

    pub fn snapshot(&self) -> Result<TaskSnapshot, TaskTreeError> {
        Ok(TaskSnapshot {
            attr: self.get_attr()?,
            children: self
                .node
                .children()
                .map(|node| Self { node }.snapshot())
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    pub fn create_child(&self, task_attr: TaskAttr) -> Result<Self, TaskTreeError> {
        let root = self.root()?;
        root.ensure_persistent_mutation_writable()?;
        let grant = self
            .node
            .tree()
            .grant_hierarchy_edit()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;
        let child_node = self.node.create_as_last_child(&grant, task_attr);
        root.mark_persistent_mutation()?;
        Ok(Self { node: child_node })
    }

    pub fn reparent_to(&mut self, parent_task: &Self) -> Result<(), TaskTreeError> {
        if self.node.ptr_eq(&parent_task.node)
            || parent_task
                .node
                .ancestors()
                .any(|ancestor| ancestor.ptr_eq(&self.node))
        {
            return Err(TaskTreeError::Cycle);
        }

        let source_root = self.root()?;
        let destination_root = parent_task.root()?;
        source_root.ensure_persistent_mutation_writable()?;
        if !source_root.node.ptr_eq(&destination_root.node) {
            destination_root.ensure_persistent_mutation_writable()?;
        }
        let self_grant = self
            .node
            .tree()
            .grant_hierarchy_edit()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;
        let parent_task_hot: HotNode<TaskAttr> = parent_task
            .node
            .clone()
            .bundle_new_hierarchy_edit_grant()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;

        self.node
            .try_detach_insert_subtree(&self_grant, InsertAs::LastChildOf(&parent_task_hot))
            .map_err(|_| TaskTreeError::Insert)?;

        source_root.mark_persistent_mutation()?;
        if !source_root.node.ptr_eq(&destination_root.node) {
            destination_root.mark_persistent_mutation()?;
        }
        Ok(())
    }

    /// Inserts a newly-created parent between this task and its current parent.
    pub fn create_parent(&mut self, task_attr: TaskAttr) -> Result<(), TaskTreeError> {
        let original_parent = self.parent()?.ok_or(TaskTreeError::RootOperation)?;
        let root = self.root()?;
        root.ensure_persistent_mutation_writable()?;
        let grant = self
            .node
            .tree()
            .grant_hierarchy_edit()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;
        let new_parent_node = original_parent.node.create_as_last_child(&grant, task_attr);
        let new_parent = Self {
            node: new_parent_node,
        };
        let parent_hot = new_parent
            .node
            .clone()
            .bundle_new_hierarchy_edit_grant()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;

        self.node
            .try_detach_insert_subtree(&grant, InsertAs::LastChildOf(&parent_hot))
            .map_err(|_| TaskTreeError::Insert)?;
        root.mark_persistent_mutation()?;
        Ok(())
    }

    /// Creates a chain of child tasks and returns the deepest child.
    pub fn create_sequential_children(
        &self,
        task_name: &str,
        estimated_work_seconds: i64,
        begin_index: u64,
        end_index: u64,
        task_name_suffix: &str,
        identity_source: (DateTime<Local>, impl FnMut() -> Uuid),
    ) -> Result<Self, TaskTreeError> {
        if begin_index > end_index {
            return Err(TaskTreeError::InvalidSequence);
        }
        let root = self.root()?;
        root.ensure_persistent_mutation_writable()?;
        let grant = self
            .node
            .tree()
            .grant_hierarchy_edit()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;
        let mut current_node = self.node.clone();
        let (now, mut next_id) = identity_source;

        for index in (begin_index..=end_index).rev() {
            let mut task_attr = TaskAttr::with_identity(
                &format!("{task_name} {index}{task_name_suffix}"),
                next_id(),
                now,
            );
            task_attr.set_estimated_work_seconds(estimated_work_seconds);
            current_node = current_node.create_as_last_child(&grant, task_attr);
        }

        root.mark_persistent_mutation()?;
        Ok(Self { node: current_node })
    }

    pub fn get_id(&self) -> Result<Uuid, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_id())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_id(&mut self, id: Uuid) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_id() == &id {
                false
            } else {
                attr.set_id(id);
                true
            }
        })
    }

    pub(crate) fn get_persistent_mutation_revision(&self) -> Result<u64, TaskTreeError> {
        self.root()?
            .node
            .try_borrow_data()
            .map(|attr| attr.persistent_mutation_revision)
            .map_err(|_| TaskTreeError::Borrow)
    }

    fn mark_persistent_mutation(&self) -> Result<(), TaskTreeError> {
        let root = self.root()?;
        let mut attr = root
            .node
            .try_borrow_data_mut()
            .map_err(|_| TaskTreeError::Borrow)?;
        attr.persistent_mutation_revision = attr.persistent_mutation_revision.wrapping_add(1);
        Ok(())
    }

    fn ensure_persistent_mutation_writable(&self) -> Result<(), TaskTreeError> {
        self.node
            .try_borrow_data_mut()
            .map(|_| ())
            .map_err(|_| TaskTreeError::Borrow)
    }

    fn update(&self, update: impl FnOnce(&mut TaskAttr) -> bool) -> Result<(), TaskTreeError> {
        let root = self.root()?;
        self.node
            .try_borrow_data()
            .map_err(|_| TaskTreeError::Borrow)?;
        root.ensure_persistent_mutation_writable()?;
        let changed = update(
            &mut *self
                .node
                .try_borrow_data_mut()
                .map_err(|_| TaskTreeError::Borrow)?,
        );
        if changed {
            root.mark_persistent_mutation()?;
        }
        Ok(())
    }

    pub fn get_name(&self) -> Result<String, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_name().to_string())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn get_status(&self) -> Result<Status, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_status())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn get_orig_status(&self) -> Result<Status, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_orig_status())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_orig_status(&self, orig_status: Status) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            let before = (*attr.get_orig_status(), *attr.get_pending_until());
            attr.set_orig_status(orig_status);
            before != (*attr.get_orig_status(), *attr.get_pending_until())
        })
    }

    pub fn get_is_on_other_side(&self) -> Result<bool, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_is_on_other_side())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_is_on_other_side(&self, is_on_other_side: bool) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_is_on_other_side() == &is_on_other_side {
                false
            } else {
                attr.set_is_on_other_side(is_on_other_side);
                true
            }
        })
    }

    pub fn get_atomic(&self) -> Result<bool, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_atomic())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_atomic(&self, atomic: bool) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_atomic() == atomic {
                false
            } else {
                attr.set_atomic(atomic);
                true
            }
        })
    }

    pub fn set_pending_until(&self, pending_until: DateTime<Local>) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            let before = *attr.get_pending_until();
            attr.set_pending_until(pending_until);
            before != *attr.get_pending_until()
        })
    }

    pub fn get_pending_until(&self) -> Result<DateTime<Local>, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_pending_until())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn sync_clock(&self, now: DateTime<Local>) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            let before = *attr.get_pending_until();
            attr.sync_clock(now);
            before != *attr.get_pending_until()
        })
    }

    pub fn get_last_synced_time(&self) -> Result<DateTime<Local>, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_last_synced_time())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_priority(&self, priority: i64) -> Result<(), TaskTreeError> {
        let root = self.root()?;
        root.update(|attr| {
            if attr.get_priority() == priority {
                false
            } else {
                attr.set_priority(priority);
                true
            }
        })
    }

    pub fn get_priority(&self) -> Result<i64, TaskTreeError> {
        self.root()?
            .node
            .try_borrow_data()
            .map(|attr| attr.get_priority())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_create_time(&self, create_time: DateTime<Local>) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_create_time() == &create_time {
                false
            } else {
                attr.set_create_time(create_time);
                true
            }
        })
    }

    pub fn get_create_time(&self) -> Result<DateTime<Local>, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_create_time())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_start_time(&self, start_time: DateTime<Local>) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            let before = (*attr.get_start_time(), *attr.get_pending_until());
            attr.set_start_time(start_time);
            before != (*attr.get_start_time(), *attr.get_pending_until())
        })
    }

    pub fn get_start_time(&self) -> Result<DateTime<Local>, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_start_time())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_end_time_opt(
        &self,
        end_time_opt: Option<DateTime<Local>>,
    ) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_end_time_opt() == &end_time_opt {
                false
            } else {
                attr.set_end_time_opt(end_time_opt);
                true
            }
        })
    }

    pub fn get_end_time_opt(&self) -> Result<Option<DateTime<Local>>, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_end_time_opt())
            .map_err(|_| TaskTreeError::Borrow)
    }

    // 親タスクと子タスクの〆切のうち、早いほうが子タスクの〆切となる
    // 〆切を設定する時には、子タスクに伝搬させていく
    // Noneの扱いが難しい。Noneを子に伝搬させても子の値に勝てないので、意味ないのでは?
    // 「親が〆切を持っている時は、子も必ず〆切を持っており、それは親より早いか等しい」という制約を維持させたい
    pub fn set_deadline_time_opt(
        &self,
        deadline_time_opt: Option<DateTime<Local>>,
    ) -> Result<(), TaskTreeError> {
        let root = self.root()?;
        let mut updates = Vec::new();
        self.collect_deadline_updates(deadline_time_opt, None, &mut updates)?;
        root.node
            .try_borrow_data_mut()
            .map_err(|_| TaskTreeError::Borrow)?;
        for (node, _) in &updates {
            node.try_borrow_data_mut()
                .map_err(|_| TaskTreeError::Borrow)?;
        }
        for (node, deadline) in &updates {
            node.try_borrow_data_mut()
                .map_err(|_| TaskTreeError::Borrow)?
                .set_deadline_time_opt(Some(*deadline));
        }
        if !updates.is_empty() {
            root.mark_persistent_mutation()?;
        }
        Ok(())
    }

    fn collect_deadline_updates(
        &self,
        inherited: Option<DateTime<Local>>,
        current_deadline_override: Option<Option<DateTime<Local>>>,
        updates: &mut Vec<(Node<TaskAttr>, DateTime<Local>)>,
    ) -> Result<(), TaskTreeError> {
        let attr = self
            .node
            .try_borrow_data()
            .map_err(|_| TaskTreeError::Borrow)?;
        if *attr.get_status() == Status::Done {
            return Ok(());
        }
        let current = current_deadline_override.unwrap_or(*attr.get_deadline_time_opt());
        drop(attr);
        let Some(inherited) = inherited else {
            return Ok(());
        };
        let effective = current
            .map(|current| current.min(inherited))
            .unwrap_or(inherited);
        if current != Some(effective) {
            updates.push((self.node.clone(), effective));
        }
        for child in self.node.children() {
            Self { node: child }.collect_deadline_updates(Some(effective), None, updates)?;
        }
        Ok(())
    }

    pub fn unset_deadline_time_opt(&self) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_deadline_time_opt().is_none() {
                false
            } else {
                attr.set_deadline_time_opt(None);
                true
            }
        })
    }

    pub fn get_deadline_time_opt(&self) -> Result<Option<DateTime<Local>>, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_deadline_time_opt())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_estimated_work_seconds(
        &self,
        estimated_work_seconds: i64,
    ) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_estimated_work_seconds() == estimated_work_seconds {
                false
            } else {
                attr.set_estimated_work_seconds(estimated_work_seconds);
                true
            }
        })
    }

    pub fn get_estimated_work_seconds(&self) -> Result<i64, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_estimated_work_seconds())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_actual_work_seconds(&self, actual_work_seconds: i64) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_actual_work_seconds() == actual_work_seconds {
                false
            } else {
                attr.set_actual_work_seconds(actual_work_seconds);
                true
            }
        })
    }

    pub fn get_repetition_interval_days_opt(&self) -> Result<Option<i64>, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_repetition_interval_days_opt())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn get_inherited_repetition_interval_days_opt(&self) -> Result<Option<i64>, TaskTreeError> {
        let mut current_parent_opt = self.parent()?;

        while let Some(parent) = current_parent_opt {
            if let Some(repetition_interval_days) = parent.get_repetition_interval_days_opt()? {
                return Ok(Some(repetition_interval_days));
            }

            current_parent_opt = parent.parent()?;
        }

        Ok(None)
    }

    pub fn set_repetition_interval_days_opt(
        &self,
        repetition_interval_days_opt: Option<i64>,
    ) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_repetition_interval_days_opt() == repetition_interval_days_opt {
                false
            } else {
                attr.set_repetition_interval_days_opt(repetition_interval_days_opt);
                true
            }
        })
    }

    pub fn get_repetition_anchor(&self) -> Result<RepetitionAnchor, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_repetition_anchor())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_repetition_anchor(
        &self,
        repetition_anchor: RepetitionAnchor,
    ) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_repetition_anchor() == repetition_anchor {
                false
            } else {
                attr.set_repetition_anchor(repetition_anchor);
                true
            }
        })
    }

    pub fn get_days_in_advance(&self) -> Result<i64, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_days_in_advance())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_days_in_advance(&self, days_in_advance: i64) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_days_in_advance() == days_in_advance {
                false
            } else {
                attr.set_days_in_advance(days_in_advance);
                true
            }
        })
    }

    pub fn get_project_category_opt(&self) -> Result<Option<ProjectCategory>, TaskTreeError> {
        self.root()?
            .node
            .try_borrow_data()
            .map(|attr| attr.get_project_category_opt())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_project_category_opt(
        &self,
        project_category_opt: Option<ProjectCategory>,
    ) -> Result<(), TaskTreeError> {
        self.root()?.update(|attr| {
            if attr.get_project_category_opt() == project_category_opt {
                false
            } else {
                attr.set_project_category_opt(project_category_opt);
                true
            }
        })
    }

    pub fn get_actual_work_seconds(&self) -> Result<i64, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_actual_work_seconds())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn make_appointment(
        &self,
        appointment_start_time: DateTime<Local>,
    ) -> Result<(), TaskTreeError> {
        // 〆切については、子タスク全体に掛かるようにする
        let deadline_time =
            appointment_start_time + Duration::seconds(self.get_estimated_work_seconds()?);

        let root = self.root()?;
        let mut deadline_updates = Vec::new();
        self.collect_deadline_updates(Some(deadline_time), Some(None), &mut deadline_updates)?;
        root.node
            .try_borrow_data_mut()
            .map_err(|_| TaskTreeError::Borrow)?;
        self.node
            .try_borrow_data_mut()
            .map_err(|_| TaskTreeError::Borrow)?;
        for (node, _) in &deadline_updates {
            node.try_borrow_data_mut()
                .map_err(|_| TaskTreeError::Borrow)?;
        }

        self.unset_deadline_time_opt()?;
        self.set_deadline_time_opt(Some(deadline_time))?;

        self.set_start_time(appointment_start_time)
    }

    pub fn num_children(&self) -> Result<usize, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map_err(|_| TaskTreeError::Borrow)?;
        Ok(self.node.num_children())
    }

    pub fn has_undone_children(&self) -> Result<bool, TaskTreeError> {
        for child in self.get_children()? {
            if child.get_status()? != Status::Done {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // 外から見て、ダミーノードのことは考慮させないように、ダミーノードの子で評価
    fn is_root(&self) -> Result<bool, TaskTreeError> {
        let root = self.root()?;
        Ok(self.node.ptr_eq(&root.node))
    }

    pub fn tree_debug_pretty_print(&self) -> Result<String, TaskTreeError> {
        self.get_attr()?;
        Ok(format!("{:?}", self.node.tree().debug_pretty_print()))
    }

    #[cfg(test)]
    pub(crate) fn eq_tree(&self, task: &TaskHandle) -> Result<bool, TaskTreeError> {
        self.node
            .tree()
            .try_eq(&task.node.tree())
            .map_err(|_| TaskTreeError::Borrow)
    }

    #[cfg(test)]
    pub(crate) fn detach_insert_as_last_child_of(
        &mut self,
        parent_task: TaskHandle,
    ) -> Result<(), String> {
        self.reparent_to(&parent_task)
            .map_err(|error| error.to_string())
    }

    #[cfg(test)]
    pub(crate) fn create_as_last_child(&self, task_attr: TaskAttr) -> Self {
        self.create_child(task_attr)
            .expect("test hierarchy child creation must succeed")
    }

    #[cfg(test)]
    pub(crate) fn create_as_parent(&mut self, task_attr: TaskAttr) -> Result<(), String> {
        self.create_parent(task_attr)
            .map_err(|error| error.to_string())
    }

    pub fn get_by_id(&self, id: Uuid) -> Result<Option<TaskHandle>, TaskTreeError> {
        let node_opt = Self::get_by_id_private(&self.node, id)?;

        Ok(node_opt.map(|node| Self { node }))
    }

    fn get_by_id_private(
        node: &Node<TaskAttr>,
        id: Uuid,
    ) -> Result<Option<Node<TaskAttr>>, TaskTreeError> {
        // ベースケース
        if node
            .try_borrow_data()
            .map_err(|_| TaskTreeError::Borrow)?
            .get_id()
            == &id
        {
            return Ok(Some(node.clone()));
        }

        // 子あり
        for child_node in node.children() {
            // let child_task = TaskHandle { node: child_node };

            // let child_task_found_opt =  child_task.get_by_id(id);
            // if  child_task_found_opt.is_some()   {
            //     return  child_task_found_opt;
            // }

            let child_task_found_opt = Self::get_by_id_private(&child_node, id)?;
            if child_task_found_opt.is_some() {
                return Ok(child_task_found_opt);
            }
        }

        Ok(None)
    }

    pub fn all_sibling_tasks_are_all_done(&self) -> Result<bool, TaskTreeError> {
        let mut ans = true;

        for sibling_node in self.node.siblings() {
            if sibling_node
                .try_borrow_data()
                .map_err(|_| TaskTreeError::Borrow)?
                .get_status()
                != &Status::Done
            {
                ans = false;
                break;
            }
        }

        Ok(ans)
    }

    // 親のタスクを考慮せずに、そのタスク単体で見た時に最速で着手できる時刻
    pub fn first_available_time(&self) -> Result<DateTime<Local>, TaskTreeError> {
        let dt_cand = if self.get_orig_status()? == Status::Pending {
            vec![self.get_start_time()?, self.get_pending_until()?]
        } else {
            vec![self.get_start_time()?]
        };

        // 1要素以上ありNoneになり得ないのでunwrap()してよい
        Ok(*dt_cand.iter().max().unwrap())
    }

    // 親を辿って、Todoのタスクを全て返す
    // タプルの1つ目は最速でTodo化するタイミング
    // ただし、〆切を守れるように、pending_untilよりも〆切を優先する
    pub fn list_all_parent_tasks_with_first_available_time(
        &self,
    ) -> Result<Vec<(DateTime<Local>, TaskHandle)>, TaskTreeError> {
        let mut ans: Vec<(DateTime<Local>, TaskHandle)> = vec![];
        let mut child_task_first_available_time: DateTime<Local> =
            DateTime::<Local>::MIN_UTC.into();

        // Phase1 子→親に辿って仮のfirst_available_timeを決定する
        //   max(子の最速着手時間 + 子の見積もり時間, 親のstart_time) = 親の最速着手時間

        // Phase2 〆切に対してオーバーしている時間を計算し、逆に親→子の順に時間を修正する

        // ここからPhase1: 子→親に辿って仮のfirst_available_timeを決定する
        let mut task_opt = Some(self.clone());
        while let Some(task) = task_opt {
            let first_available_time = task.first_available_time()?;
            let task_first_available_time =
                max(child_task_first_available_time, first_available_time);

            child_task_first_available_time = task_first_available_time
                + Duration::seconds(max(
                    0,
                    task.get_estimated_work_seconds()? - task.get_actual_work_seconds()?,
                ));

            let tpl = (task_first_available_time, task.clone());
            ans.push(tpl);

            // 再代入
            task_opt = task.parent()?;
        }

        // ここからPhase2: 〆切に対してオーバーしている時間を計算し、逆に親→子の順に時間を修正する

        // 親のfirst_available_timeは〆切を考慮済みなので、それを子でも考慮するために一時保存する
        // 別のメソッドでset_deadline_time()する際に各タスクの見積もりまで考慮して設定するのは、見積もりを変えるたびにdeadline_timeを設定し直さなければいけないため複雑になる
        // そのため、このメソッド内で行う
        let mut parent_required_start_time_for_deadline = DateTime::<Local>::MAX_UTC.into();

        for (rough_first_available_time, task) in ans.iter_mut().rev() {
            // 親から引き継いだ〆切か、自分の〆切のうち早いほう
            let mut required_start_time_for_deadline = parent_required_start_time_for_deadline;
            if let Some(deadline_time) = task.get_deadline_time_opt()? {
                if deadline_time < required_start_time_for_deadline {
                    required_start_time_for_deadline = deadline_time;
                }
            }

            let lateness_duration = *rough_first_available_time
                + Duration::seconds(max(
                    0,
                    task.get_estimated_work_seconds()? - task.get_actual_work_seconds()?,
                ))
                - required_start_time_for_deadline;

            if lateness_duration >= Duration::seconds(0) {
                *rough_first_available_time -= lateness_duration;
                parent_required_start_time_for_deadline = *rough_first_available_time;
            }
        }

        let mut child_task_finish_time: DateTime<Local> = DateTime::<Local>::MIN_UTC.into();
        for (first_available_time, task) in ans.iter_mut() {
            let earliest_time_for_task = min(*first_available_time, task.first_available_time()?);
            *first_available_time = max(earliest_time_for_task, child_task_finish_time);
            child_task_finish_time = *first_available_time
                + Duration::seconds(max(
                    0,
                    task.get_estimated_work_seconds()? - task.get_actual_work_seconds()?,
                ));
        }

        Ok(ans)
    }
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

    task.make_appointment(appointment_start_time).unwrap();

    assert_eq!(
        &task.get_start_time().unwrap(),
        &Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap()
    );
    assert_eq!(
        &task.get_deadline_time_opt().unwrap(),
        &Some(Local.with_ymd_and_hms(2023, 5, 19, 2, 23, 45).unwrap())
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

    let actual = root.create_sequential_children(
        "step",
        60,
        2,
        1,
        "",
        (test_task_time(), next_test_task_id),
    );

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
    let grand_child_task_result = task.create_sequential_children(
        "鎖タスク",
        600,
        1,
        2,
        "話",
        (test_task_time(), next_test_task_id),
    );

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
    let grand_child_task_result = task.create_sequential_children(
        "鎖タスク",
        600,
        10,
        1,
        "話",
        (test_task_time(), next_test_task_id),
    );

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

// 詳細な構造を知っていたほうが構築しやすいので、gatewayではなくtaskの中で定義する
pub fn task_to_yaml(task: &TaskHandle) -> Result<Yaml, TaskTreeError> {
    let default_attr = TaskAttr::with_identity(
        "デフォルト用",
        Uuid::nil(),
        DateTime::<Local>::MIN_UTC.into(),
    );

    let mut task_hash = LinkedHashMap::new();

    task_hash.insert(
        Yaml::String(String::from("name")),
        Yaml::String(task.get_name()?),
    );

    task_hash.insert(
        Yaml::String(String::from("id")),
        Yaml::String(task.get_id()?.to_string()),
    );

    let orig_status = task.get_orig_status()?;
    if orig_status != *default_attr.get_orig_status() {
        task_hash.insert(
            Yaml::String(String::from("status")),
            Yaml::String(orig_status.to_string()),
        );
    }

    let is_on_other_side = task.get_is_on_other_side()?;
    if is_on_other_side != *default_attr.get_is_on_other_side() {
        task_hash.insert(
            Yaml::String(String::from("is_on_other_side")),
            Yaml::Boolean(is_on_other_side),
        );
    }

    let atomic = task.get_atomic()?;
    if atomic != default_attr.get_atomic() {
        task_hash.insert(Yaml::String(String::from("atomic")), Yaml::Boolean(atomic));
    }

    let pending_until = task.get_pending_until()?;
    if pending_until != *default_attr.get_pending_until() {
        let pending_until_string = pending_until.format("%Y/%m/%d %H:%M:%S").to_string();
        task_hash.insert(
            Yaml::String(String::from("pending_until")),
            Yaml::String(pending_until_string),
        );
    }

    let priority = task.get_priority()?;
    if task.is_root()? && priority != default_attr.get_priority() {
        task_hash.insert(
            Yaml::String(String::from("priority")),
            Yaml::Integer(priority),
        );
    }

    if task.is_root()? {
        if let Some(project_category) = task.get_project_category_opt()? {
            task_hash.insert(
                Yaml::String(String::from("category")),
                Yaml::String(project_category.to_string()),
            );
        }
    }

    let create_time = task.get_create_time()?;
    let create_time_string = create_time.format("%Y/%m/%d %H:%M:%S").to_string();
    task_hash.insert(
        Yaml::String(String::from("create_time")),
        Yaml::String(create_time_string),
    );

    let start_time = task.get_start_time()?;
    let start_time_string = start_time.format("%Y/%m/%d %H:%M:%S").to_string();
    task_hash.insert(
        Yaml::String(String::from("start_time")),
        Yaml::String(start_time_string),
    );

    let end_time_opt = task.get_end_time_opt()?;
    if let Some(end_time) = end_time_opt {
        let end_time_string = end_time.format("%Y/%m/%d %H:%M:%S").to_string();
        task_hash.insert(
            Yaml::String(String::from("end_time")),
            Yaml::String(end_time_string),
        );
    }

    let deadline_time_opt = task.get_deadline_time_opt()?;
    if let Some(deadline_time) = deadline_time_opt {
        let deadline_time_string = deadline_time.format("%Y/%m/%d %H:%M:%S").to_string();
        task_hash.insert(
            Yaml::String(String::from("deadline_time")),
            Yaml::String(deadline_time_string),
        );
    }

    let estimated_work_seconds = task.get_estimated_work_seconds()?;
    if estimated_work_seconds != default_attr.get_estimated_work_seconds() {
        task_hash.insert(
            Yaml::String(String::from("estimated_work_seconds")),
            Yaml::Integer(estimated_work_seconds),
        );
    }

    let actual_work_seconds = task.get_actual_work_seconds()?;
    if actual_work_seconds != default_attr.get_actual_work_seconds() {
        task_hash.insert(
            Yaml::String(String::from("actual_work_seconds")),
            Yaml::Integer(actual_work_seconds),
        );
    }

    let repetition_interval_days_opt = task.get_repetition_interval_days_opt()?;
    if let Some(repetition_interval_days) = repetition_interval_days_opt {
        task_hash.insert(
            Yaml::String(String::from("repetition_interval_days")),
            Yaml::Integer(repetition_interval_days),
        );
    }

    let repetition_anchor = task.get_repetition_anchor()?;
    if repetition_anchor != default_attr.get_repetition_anchor() {
        task_hash.insert(
            Yaml::String(String::from("repetition_anchor")),
            Yaml::String(repetition_anchor.to_string()),
        );
    }

    let days_in_advance = task.get_days_in_advance()?;
    if days_in_advance != default_attr.get_days_in_advance() {
        task_hash.insert(
            Yaml::String(String::from("days_in_advance")),
            Yaml::Integer(days_in_advance),
        );
    }

    let mut children = vec![];
    for child_node in task.node.children() {
        let child_task = TaskHandle { node: child_node };
        let child_yaml = task_to_yaml(&child_task)?;
        children.push(child_yaml);
    }

    if !children.is_empty() {
        task_hash.insert(
            Yaml::String(String::from("children")),
            Yaml::Array(children),
        );
    }

    Ok(Yaml::Hash(task_hash))
}

#[test]
fn test_task_to_yaml_正常系1_デフォルトの値と同じ場合は出力しない() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

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
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let mut task = new_test_task_handle("親タスク1").unwrap();
    task.set_orig_status(Status::Pending).unwrap();
    task.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap())
        .unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();

    let mut task_attr_child_1 = new_test_task_attr("子タスク1");
    task_attr_child_1.set_orig_status(Status::Pending);
    task_attr_child_1.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    task_attr_child_1.set_create_time(now);
    task_attr_child_1.set_start_time(now);
    let id_child_1: Uuid = uuid!("0aaee735-3e22-4216-8b59-d56d5caf29ee");
    task_attr_child_1.set_id(id_child_1);

    let mut task_attr_child_2 = new_test_task_attr("子タスク2");
    task_attr_child_2.set_orig_status(Status::Pending);
    task_attr_child_2.set_pending_until(Local.with_ymd_and_hms(2023, 4, 1, 12, 0, 0).unwrap());
    task_attr_child_2.set_create_time(now);
    task_attr_child_2.set_start_time(now);
    let id_child_2: Uuid = uuid!("7ffcba2f-80e0-4a44-aee9-d68e0d2d1256");
    task_attr_child_2.set_id(id_child_2);

    task.create_as_last_child(task_attr_child_1);
    task.create_as_last_child(task_attr_child_2);

    let actual = task_to_yaml(&task).unwrap();

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
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

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
fn test_task_to_yaml_project_category() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_project_category_opt(Some(ProjectCategory::Sustaining))
        .unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
category: sustaining
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_project_categoryは子タスクには出力しない() {
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    let mut task = new_test_task_handle("親タスク").unwrap();
    task.set_id(uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8"))
        .unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();

    let mut task_attr_child = new_test_task_attr("子タスク");
    task_attr_child.set_id(uuid!("0aaee735-3e22-4216-8b59-d56d5caf29ee"));
    task_attr_child.set_create_time(now);
    task_attr_child.set_start_time(now);
    task_attr_child.set_project_category_opt(Some(ProjectCategory::Sustaining));

    task.create_as_last_child(task_attr_child);

    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: '親タスク'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
children:
  - name: '子タスク'
    id: 0aaee735-3e22-4216-8b59-d56d5caf29ee
    create_time: '2023/05/19 01:23:45'
    start_time: '2023/05/19 01:23:45'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_is_on_other_side() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_is_on_other_side(true).unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

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
fn test_task_to_yaml_atomic() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_atomic(true).unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
atomic: true
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_end_time_opt() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_is_on_other_side(true).unwrap();
    task.set_create_time(Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap())
        .unwrap();
    task.set_start_time(Local.with_ymd_and_hms(2023, 5, 19, 2, 34, 56).unwrap())
        .unwrap();
    task.set_end_time_opt(Some(Local.with_ymd_and_hms(2023, 5, 19, 3, 45, 6).unwrap()))
        .unwrap();
    let actual = task_to_yaml(&task).unwrap();

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
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_is_on_other_side(true).unwrap();
    task.set_create_time(Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap())
        .unwrap();
    task.set_start_time(Local.with_ymd_and_hms(2023, 5, 19, 2, 34, 56).unwrap())
        .unwrap();
    task.set_deadline_time_opt(Some(Local.with_ymd_and_hms(2023, 5, 19, 3, 45, 6).unwrap()))
        .unwrap();
    let actual = task_to_yaml(&task).unwrap();

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
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_is_on_other_side(true).unwrap();
    task.set_create_time(Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap())
        .unwrap();
    task.set_start_time(Local.with_ymd_and_hms(2023, 5, 19, 2, 34, 56).unwrap())
        .unwrap();
    task.set_estimated_work_seconds(1).unwrap();
    let actual = task_to_yaml(&task).unwrap();

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
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_is_on_other_side(true).unwrap();
    task.set_create_time(Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap())
        .unwrap();
    task.set_start_time(Local.with_ymd_and_hms(2023, 5, 19, 2, 34, 56).unwrap())
        .unwrap();
    task.set_actual_work_seconds(1).unwrap();
    let actual = task_to_yaml(&task).unwrap();

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
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_repetition_interval_days_opt(Some(7)).unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

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
fn test_task_to_yaml_repetition_anchor_completion() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_repetition_anchor(RepetitionAnchor::Completion)
        .unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

    let s = "
name: 'タスク1'
id: 67e55044-10b1-426f-9247-bb680e5fe0c8
create_time: '2023/05/19 01:23:45'
start_time: '2023/05/19 01:23:45'
repetition_anchor: completion
";
    let docs = YamlLoader::load_from_str(s).unwrap();
    let expected_yaml: &Yaml = &docs[0];

    assert_eq!(&actual, expected_yaml);
}

#[test]
fn test_task_to_yaml_repetition_anchor_deadlineは出力しない() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_repetition_anchor(RepetitionAnchor::Deadline)
        .unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

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
fn test_task_to_yaml_days_in_advance() {
    let mut task = new_test_task_handle("タスク1").unwrap();
    let id: Uuid = uuid!("67e55044-10b1-426f-9247-bb680e5fe0c8");
    task.set_id(id).unwrap();
    task.set_days_in_advance(1).unwrap();
    let now = Local.with_ymd_and_hms(2023, 5, 19, 1, 23, 45).unwrap();
    task.set_create_time(now).unwrap();
    task.set_start_time(now).unwrap();
    let actual = task_to_yaml(&task).unwrap();

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
            root.create_sequential_children(
                "子",
                60,
                1,
                2,
                "",
                (test_task_time(), next_test_task_id),
            ),
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
