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
