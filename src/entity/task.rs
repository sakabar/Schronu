#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Status {
    Todo,
    Doing,
    Done,
}

pub fn read_status(s: &str) -> Option<Status> {
    let lc = s.to_lowercase();

    if lc == "todo" {
        return Some(Status::Todo);
    } else if lc == "doing" {
        return Some(Status::Doing);
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
fn test_read_status_doingの文字列を変換する() {
    let s = "doing";
    let actual = read_status(s);
    assert_eq!(actual, Some(Status::Doing));
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
    children: Vec<Task>,
}

impl Task {
    pub fn new(name: String, status: Status, children: Vec<Task>) -> Self {
        Self {
            name,
            status,
            children,
        }
    }

    pub fn new_with_name(name: String) -> Self {
        Self {
            name,
            status: Status::Todo,
            children: vec![],
        }
    }

    pub fn new_with_name_children(name: String, children: Vec<Task>) -> Self {
        Self {
            name,
            status: Status::Todo,
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
    let child_task_1 = Task::new(
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

pub fn extract_leaf_tasks_from_project(task: &Task) -> Vec<&Task> {
    if task.get_children().is_empty() {
        return vec![task];
    }

    let mut ans = vec![];

    // 深さ優先
    for child in task.get_children() {
        if child.get_status() != &Status::Done {
            let mut leaves = extract_leaf_tasks_from_project(child);
            ans.append(&mut leaves);
        }
    }

    return ans;
}
