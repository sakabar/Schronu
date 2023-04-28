use crate::application::interface::TaskRepositoryTrait;
use crate::entity::task::{extract_leaf_tasks_from_project, Task, TaskAttr};
use chrono::Local;

pub fn update_task_sample(task_repository: &mut dyn TaskRepositoryTrait) {
    // 初期化
    task_repository.sync_clock(Local::now());
    task_repository.load();
    // let mut env_pokemon_repository = EnvPokemonRepository::new();

    let mut focused_task_opt: Option<&Task> = None;

    // 優先度の最も高いPJを一つ選ぶ
    // 一番下のタスクにフォーカスが自動的に当たる
    let highest_priority_leaf_project_opt = task_repository.get_highest_priority_project();
    match highest_priority_leaf_project_opt {
        Some(highest_priority_leaf_project) => {
            let leaf_tasks = extract_leaf_tasks_from_project(highest_priority_leaf_project);
            focused_task_opt = leaf_tasks.last();

            // 木を操作する
            match focused_task_opt {
                Some(focused_task) => {
                    focused_task.create_as_last_child(TaskAttr::new("新タスク with Schronu job"));
                }
                None => {}
            }

            // 保存して終わり
            task_repository.save();
        }
        None => {}
    }
}
