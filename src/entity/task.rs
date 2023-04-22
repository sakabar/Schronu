use chrono::{DateTime, Local};

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
pub struct Task {
    name: String,
    status: Status,
    pending_until: DateTime<Local>,
    children: Vec<Task>,
}

#[test]
#[allow(non_snake_case)]
pub fn test_new_with_current_time_現在時刻がpending_until以前でPending状態であること() {
    let pending_until = DateTime::<Local>::MAX_UTC.into();
    let actual =
        Task::new_with_current_time("タスク".to_string(), Status::Pending, pending_until, vec![]);
    let expected = Task::new("タスク".to_string(), Status::Pending, pending_until, vec![]);

    assert_eq!(actual, expected);
}

#[test]
#[allow(non_snake_case)]
pub fn test_new_with_current_time_現在時刻がpending_until以降の場合Todo状態となること() {
    let pending_until = DateTime::<Local>::MIN_UTC.into();
    let actual =
        Task::new_with_current_time("タスク".to_string(), Status::Pending, pending_until, vec![]);
    let expected = Task::new("タスク".to_string(), Status::Todo, pending_until, vec![]);

    assert_eq!(actual, expected);
}

impl Task {
    pub fn new(
        name: String,
        status: Status,
        pending_until: DateTime<Local>,
        children: Vec<Task>,
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
        children: Vec<Task>,
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
        children: Vec<Task>,
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

    pub fn new_with_name_children(name: String, children: Vec<Task>) -> Self {
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

    pub fn get_children(&self) -> &Vec<Task> {
        return &self.children;
    }
}

#[test]
fn test_extract_leaf_tasks_from_project_タスクのchildrenが空配列の場合() {
    let task = Task::new_with_name("タスク".to_string());
    let actual = extract_leaf_tasks_from_project(&task);

    let t = Task::new_with_name("タスク".to_string());

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

    let grand_child_task_1 = Task::new_with_name("孫タスク1".to_string());
    let child_task_1 =
        Task::new_with_name_children("子タスク1".to_string(), vec![grand_child_task_1]);
    let child_task_2 = Task::new_with_name("子タスク2".to_string());
    let parent_task_1 =
        Task::new_with_name_children("親タスク1".to_string(), vec![child_task_1, child_task_2]);

    let actual = extract_leaf_tasks_from_project(&parent_task_1);
    let t1 = Task::new_with_name("孫タスク1".to_string());
    let t2 = Task::new_with_name("子タスク2".to_string());
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

    let grand_child_task_1 = Task::new_with_name("孫タスク1".to_string());
    let child_task_1 = Task::new_with_name_status_children(
        "子タスク1".to_string(),
        Status::Done,
        vec![grand_child_task_1],
    );

    let child_task_2 = Task::new_with_name("子タスク2".to_string());

    let parent_task_1 =
        Task::new_with_name_children("親タスク1".to_string(), vec![child_task_1, child_task_2]);

    let actual = extract_leaf_tasks_from_project(&parent_task_1);
    let expected_child_task_2 = Task::new_with_name("子タスク2".to_string());
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

    let grand_child_task_1 = Task::new_with_name("孫タスク1".to_string());
    let child_task_1 = Task::new_with_name_status_children(
        "子タスク1".to_string(),
        Status::Pending,
        vec![grand_child_task_1],
    );

    let child_task_2 =
        Task::new_with_name_status_children("子タスク2".to_string(), Status::Pending, vec![]);

    let parent_task_1 =
        Task::new_with_name_children("親タスク1".to_string(), vec![child_task_1, child_task_2]);

    let actual = extract_leaf_tasks_from_project(&parent_task_1);
    let expected_grand_child_task_1 = Task::new_with_name("孫タスク1".to_string());
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
        Task::new_with_name_status_children("孫タスク1".to_string(), Status::Done, vec![]);
    let grand_child_task_2 =
        Task::new_with_name_status_children("孫タスク2".to_string(), Status::Done, vec![]);

    let child_task_1 = Task::new_with_name_status_children(
        "子タスク1".to_string(),
        Status::Todo,
        vec![grand_child_task_1, grand_child_task_2],
    );

    let expected_child_task_1 = child_task_1.clone();

    let child_task_2 = Task::new_with_name("子タスク2".to_string());

    let parent_task_1 =
        Task::new_with_name_children("親タスク1".to_string(), vec![child_task_1, child_task_2]);

    let actual = extract_leaf_tasks_from_project(&parent_task_1);
    let expected_child_task_2 = Task::new_with_name("子タスク2".to_string());
    let expected = vec![&expected_child_task_1, &expected_child_task_2];
    assert_eq!(actual, expected);
}

pub fn extract_leaf_tasks_from_project(task: &Task) -> Vec<&Task> {
    let children_are_all_done = task
        .get_children()
        .iter()
        .all(|task| task.status == Status::Done);

    if task.get_children().is_empty() || children_are_all_done {
        return vec![task];
    }

    let mut ans: Vec<&Task> = vec![];

    // 深さ優先
    for child in task.get_children() {
        if child.get_status() != &Status::Done {
            let leaves_with_pending: Vec<&Task> = extract_leaf_tasks_from_project(child);
            let mut leaves: Vec<&Task> = leaves_with_pending
                .iter()
                .filter(|&leaf| leaf.get_status() != &Status::Pending)
                .map(|&leaf| leaf)
                .collect::<Vec<_>>();
            ans.append(&mut leaves);
        }
    }

    return ans;
}
