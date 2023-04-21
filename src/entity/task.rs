#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Status {
    Todo,
    Doing,
    Done,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    name: String,
    status: Status,
    children: Vec<Task>,
}

impl Task {
    pub fn new(name: String, children: Vec<Task>) -> Self {
        Self {
            name,
            status: Status::Todo,
            children,
        }
    }

    pub fn get_children(&self) -> &Vec<Task> {
        return &self.children;
    }
}

#[test]
fn test_extract_leaves__タスクのchildrenが空配列の場合() {
    let task = Task::new("タスク".to_string(), vec![]);
    let actual = extract_leaves(&task);

    let t = Task::new("タスク".to_string(), vec![]);

    let expected = vec![&t];
    assert_eq!(actual, expected);
}

#[test]
fn test_extract_leaves__タスクのchildrenが空配列ではない場合は再帰して結果を返す() {
    let grand_child_task_1 = Task::new("孫タスク1".to_string(), vec![]);
    let child_task_1 = Task::new("子タスク1".to_string(), vec![grand_child_task_1]);
    let child_task_2 = Task::new("子タスク2".to_string(), vec![]);
    let parent_task_1 = Task::new("親タスク1".to_string(), vec![child_task_1, child_task_2]);

    let actual = extract_leaves(&parent_task_1);
    let t1 = Task::new("孫タスク1".to_string(), vec![]);
    let t2 = Task::new("子タスク2".to_string(), vec![]);
    let expected = vec![&t1, &t2];
    assert_eq!(actual, expected);
}

pub fn extract_leaves(task: &Task) -> Vec<&Task> {
    if task.get_children().is_empty() {
        return vec![task];
    }

    let mut ans = vec![];

    // 深さ優先
    for child in task.get_children() {
        let mut leaves = extract_leaves(child);
        ans.append(&mut leaves);
    }

    return ans;
}
