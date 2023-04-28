use chrono::{DateTime, Local};
use core::cell::BorrowError;
use dendron::{HotNode, InsertAs, Node};

#[cfg(test)]
use chrono::TimeZone;

#[cfg(test)]
use dendron::{tree, Tree};

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
fn test_extract_leaf_tasks_from_project_タスクのchildrenが空配列の場合() {
    let task = ImmutableTask::new_with_name("タスク".to_string());
    let actual = extract_leaf_tasks_from_project(&task);

    let t = ImmutableTask::new_with_name("タスク".to_string());

    let expected = vec![&t];
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

    let grand_child_task_1 = ImmutableTask::new_with_name("孫タスク1".to_string());
    let child_task_1 =
        ImmutableTask::new_with_name_children("子タスク1".to_string(), vec![grand_child_task_1]);
    let child_task_2 = ImmutableTask::new_with_name("子タスク2".to_string());
    let parent_task_1 = ImmutableTask::new_with_name_children(
        "親タスク1".to_string(),
        vec![child_task_1, child_task_2],
    );

    let actual = extract_leaf_tasks_from_project(&parent_task_1);
    let t1 = ImmutableTask::new_with_name("孫タスク1".to_string());
    let t2 = ImmutableTask::new_with_name("子タスク2".to_string());
    let expected = vec![&t1, &t2];
    assert_eq!(actual, expected);
}

#[test]
fn test_extract_leaf_tasks_from_project_done状態のタスクとその子孫は全て無視されること() {
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

    let actual = extract_leaf_tasks_from_project(&parent_task_1);
    let expected_child_task_2 = ImmutableTask::new_with_name("子タスク2".to_string());
    let expected = vec![&expected_child_task_2];
    assert_eq!(actual, expected);
}

#[test]
fn test_extract_leaf_tasks_from_project_途中にpending状態のタスクがあった場合は子孫を辿るが_葉がpending状態の場合は結果に入らないこと(
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

    let actual = extract_leaf_tasks_from_project(&parent_task_1);
    let expected_grand_child_task_1 = ImmutableTask::new_with_name("孫タスク1".to_string());
    let expected = vec![&expected_grand_child_task_1];
    assert_eq!(actual, expected);
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

    let actual = extract_leaf_tasks_from_project(&parent_task_1);
    let expected_child_task_2 = ImmutableTask::new_with_name("子タスク2".to_string());
    let expected = vec![&expected_child_task_1, &expected_child_task_2];
    assert_eq!(actual, expected);
}

#[test]
fn test_extract_leaf_tasks_from_project_子が全てdoneのタスクで親がpendingの時は空配列を返すこと() {
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

    let actual = extract_leaf_tasks_from_project(&parent_task_1);
    let expected: Vec<&ImmutableTask> = vec![];
    assert_eq!(actual, expected);
}

pub fn extract_leaf_tasks_from_project(task: &ImmutableTask) -> Vec<&ImmutableTask> {
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
            let leaves_with_pending: Vec<&ImmutableTask> = extract_leaf_tasks_from_project(child);
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

#[derive(Clone, Debug, PartialEq)]
pub struct TaskAttr {
    name: String,
    status: Status,
    pending_until: DateTime<Local>,
}

impl TaskAttr {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            status: Status::Todo,
            pending_until: DateTime::<Local>::MIN_UTC.into(),
        }
    }

    pub fn set_status(&mut self, status: Status) {
        self.status = status;
    }

    pub fn get_status(&self) -> &Status {
        &self.status
    }

    pub fn set_pending_until(&mut self, pending_until: DateTime<Local>) {
        self.pending_until = pending_until;
    }

    pub fn get_pending_until(&self) -> &DateTime<Local> {
        &self.pending_until
    }
}

#[test]
fn test_task_attr_set_status() {
    let mut attr = TaskAttr::new("タスク");
    attr.set_status(Status::Done);
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

#[derive(Debug, PartialEq)]
pub struct Task {
    // task_attr: TaskAttr,
    node: Node<TaskAttr>,
}

impl Task {
    pub fn new(name: &str) -> Self {
        let task_attr = TaskAttr::new(name);
        let node = Node::new_tree(task_attr);

        Self { node }
    }

    pub fn set_status(&self, status: Status) {
        self.node.borrow_data_mut().set_status(status);
    }

    pub fn set_pending_until(&self, pending_until: DateTime<Local>) {
        self.node.borrow_data_mut().set_pending_until(pending_until);
    }

    pub fn parent(&self) -> Option<Self> {
        match self.node.parent() {
            Some(node) => Some(Task { node }),
            None => None,
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
    pub fn detach_insert_as_last_child_of(&mut self, parent_task: Task) {
        // taskのsubtreeをコピーしてselfを親から切り離して、parent_taskに結合する
        // という挙動を期待しているが、ライブラリの不具合により実現できていない
        // let self_grant = &self.node.tree().grant_hierarchy_edit().expect("self grant");

        let parent_task_hot: HotNode<TaskAttr> = parent_task
            .node
            .bundle_new_hierarchy_edit_grant()
            .expect("parent hot node");

        // let parent_task_grant = &parent_task.node.tree().grant_hierarchy_edit().expect("parent grant");

        // let parent_task_hot: HotNode<TaskAttr> = parent_task
        //     .node
        //     .bundle_hierarchy_edit_grant(&parent_task_grant);

        // self.node
        //     .try_detach_insert_subtree(&self_grant, InsertAs::LastChildOf(&parent_task_hot))
        //     .expect("creating valid hierarchy");

        self.node = self
            .node
            .try_clone_insert_subtree(InsertAs::LastChildOf(&parent_task_hot))
            .expect("creating valid hierarchy")
            .plain();
    }

    pub fn create_as_last_child(&self, task_attr: TaskAttr) -> Self {
        let self_grant = &self.node.tree().grant_hierarchy_edit().expect("self grant");

        let child_node = self.node.create_as_last_child(&self_grant, task_attr);
        Self { node: child_node }
    }
}

#[test]
fn test_new_detach_insert_as_last_child_of_正常系1() {
    let parent_task = Task::new("親タスク");
    let mut child_task = Task::new("子タスク");

    child_task.detach_insert_as_last_child_of(parent_task);
    assert_eq!(*child_task.node.borrow_data(), TaskAttr::new("子タスク"));
    assert_eq!(
        *child_task.node.root().borrow_data(),
        TaskAttr::new("親タスク")
    );
}

#[test]
fn test_new_detach_insert_as_last_child_of_正常系2() {
    let parent_task = Task::new("親タスク");
    let mut child_task = Task::new("子タスク");
    child_task.create_as_last_child(TaskAttr::new("孫タスク"));

    child_task.detach_insert_as_last_child_of(parent_task);

    let expected_tree = tree! {
        TaskAttr::new("親タスク"), [
        /(TaskAttr::new("子タスク"), [
            TaskAttr::new("孫タスク")
        ]),
    ]};

    assert_task_and_tree(&child_task, &expected_tree)
}

#[test]
fn test_create_as_last_child_正常系1() {
    let actual_task = Task::new("親タスク");
    actual_task.create_as_last_child(TaskAttr::new("子タスク"));

    let expected_tree = tree! {
    TaskAttr::new("親タスク"), [
        TaskAttr::new("子タスク")
    ]};

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
