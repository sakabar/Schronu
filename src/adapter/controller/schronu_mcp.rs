use schronu::adapter::gateway::storage_lock::{LockMode, StorageLock};
use schronu::adapter::gateway::task_repository::TaskRepository;
use schronu::adapter::mcp::McpServer;
use schronu::application::interface::TaskRepositoryTrait;
use serde_json::json;
use std::error::Error;
use std::io::{self, BufRead, Write};
use std::process;

mod storage_directory;
use storage_directory::resolve_project_storage_directory;

fn main() {
    if let Err(error) = run() {
        eprintln!("[Error] {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let storage_directory =
        resolve_project_storage_directory(std::env::var_os("SCHRONU_STORAGE_DIR"))?;
    let _storage_lock = StorageLock::acquire(&storage_directory, LockMode::Mcp)?;
    let storage_directory_text = storage_directory
        .to_str()
        .ok_or("storage directory path must be valid UTF-8")?;
    let mut repository = TaskRepository::new(storage_directory_text);
    repository.load()?;
    serve_stdio(
        McpServer::new(repository),
        io::stdin().lock(),
        io::stdout().lock(),
    )
}

fn serve_stdio<R: TaskRepositoryTrait>(
    mut server: McpServer<R>,
    input: impl BufRead,
    mut output: impl Write,
) -> Result<(), Box<dyn Error>> {
    for line in input.lines() {
        let line = line?;
        let response = match serde_json::from_str(&line) {
            Ok(request) => server.handle_request(request),
            Err(_) => Some(json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {
                    "code": -32700,
                    "message": "Parse error"
                }
            })),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response)?;
            writeln!(output)?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::prepare_repository;
    use chrono::{DateTime, Local, TimeZone};
    use schronu::application::interface::{TaskRepositoryError, TaskRepositoryTrait};
    use schronu::entity::task::Task;
    use std::cell::RefCell;
    use std::rc::Rc;
    use uuid::Uuid;

    struct RecordingRepository {
        calls: Rc<RefCell<Vec<&'static str>>>,
        synced_time: Option<DateTime<Local>>,
    }

    impl RecordingRepository {
        fn new(calls: Rc<RefCell<Vec<&'static str>>>) -> Self {
            Self {
                calls,
                synced_time: None,
            }
        }
    }

    impl TaskRepositoryTrait for RecordingRepository {
        fn get_project_storage_dir_name(&self) -> &str {
            "unused"
        }

        fn get_all_projects(&self) -> Vec<&Task> {
            Vec::new()
        }

        fn load(&mut self) -> Result<(), TaskRepositoryError> {
            self.calls.borrow_mut().push("load");
            Ok(())
        }

        fn save(&self) -> Result<(), TaskRepositoryError> {
            Ok(())
        }

        fn sync_clock(&mut self, now: DateTime<Local>) {
            self.calls.borrow_mut().push("sync_clock");
            self.synced_time = Some(now);
        }

        fn get_last_synced_time(&self) -> DateTime<Local> {
            self.synced_time.unwrap()
        }

        fn get_highest_priority_project(&mut self) -> Option<&Task> {
            None
        }

        fn get_highest_priority_leaf_task_id(&mut self) -> Option<Uuid> {
            None
        }

        fn get_defer_candidate_leaf_task_id(&mut self, _recent_days: i64) -> Option<Uuid> {
            None
        }

        fn get_by_id(&self, _id: Uuid) -> Option<Task> {
            None
        }

        fn start_new_project(&mut self, _root_task: Task) {}
    }

    #[test]
    fn prepare_repositoryは指定時刻を同期してからloadする() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut repository = RecordingRepository::new(Rc::clone(&calls));
        let now = Local.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap();

        prepare_repository(&mut repository, now).unwrap();

        assert_eq!(&*calls.borrow(), &["sync_clock", "load"]);
        assert_eq!(repository.synced_time, Some(now));
    }
}
