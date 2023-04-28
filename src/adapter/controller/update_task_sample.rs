use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::application::update_task_sample::update_task_sample;

fn main() {
    // env_logger::init();

    let mut task_repository = TaskRepository::new("../Schronu-alpha/tasks/");
    update_task_sample(&mut task_repository);
}
