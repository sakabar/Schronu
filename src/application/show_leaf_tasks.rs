use crate::entity::task::extract_leaf_tasks_from_project;
use crate::entity::task::Task;

pub fn show_leaf_tasks(projects: &Vec<Task>) {
    let mut all_leaf_tasks = vec![];

    for project in projects.iter() {
        let mut leaf_tasks = extract_leaf_tasks_from_project(&project);
        all_leaf_tasks.append(&mut leaf_tasks);
    }

    for leaf_task in all_leaf_tasks.iter() {
        println!("{:?}", leaf_task);
    }
}
