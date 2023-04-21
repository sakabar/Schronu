use crate::entity::task::extract_leaf_tasks_from_project;
use crate::entity::task::Task;

pub fn show_leaf_tasks(projects: &Vec<Task>) {
    let mut all_leaf_tasks = vec![];

    for project in projects.iter() {
        let root_task_name = project.get_name();
        let leaf_tasks = extract_leaf_tasks_from_project(&project);
        let p = (root_task_name, leaf_tasks);
        all_leaf_tasks.push(p);
    }

    for (root_task_name, leaf_tasks) in all_leaf_tasks.iter() {
        for leaf_task in leaf_tasks.iter() {
            println!("{},{:?}", root_task_name, leaf_task);
        }
    }
}
