#[path = "schronu/command.rs"]
mod command;

#[path = "schronu/cli_syntax.rs"]
mod cli_syntax;

#[path = "schronu/command_context.rs"]
mod command_context;

#[path = "schronu/handler.rs"]
mod handler;

#[path = "schronu/interactive.rs"]
mod interactive;

#[path = "schronu/renderer.rs"]
mod renderer;

#[path = "schronu/runtime.rs"]
mod runtime;

mod storage_directory;

#[path = "schronu/web_service.rs"]
mod web_service;

#[path = "schronu/web_session_write.rs"]
mod web_session_write;

#[path = "schronu/view.rs"]
mod view;

#[cfg(test)]
#[path = "schronu/command_test_support.rs"]
mod command_test_support;

#[cfg(test)]
#[path = "schronu/command_contract_tests.rs"]
mod command_contract_tests;

#[cfg(test)]
#[path = "schronu/cli_syntax_contract_tests.rs"]
mod cli_syntax_contract_tests;

#[cfg(test)]
#[path = "schronu/task_name_spreadsheet_contract_tests.rs"]
mod task_name_spreadsheet_contract_tests;

#[cfg(test)]
#[path = "schronu/handler_contract_tests.rs"]
mod handler_contract_tests;

#[cfg(test)]
#[path = "schronu/renderer_contract_tests.rs"]
mod renderer_contract_tests;

#[cfg(test)]
#[path = "schronu/interactive_contract_tests.rs"]
mod interactive_contract_tests;

#[cfg(test)]
#[path = "schronu/web_read_buffer_contract_tests.rs"]
mod web_read_buffer_contract_tests;

#[cfg(test)]
#[path = "schronu/web_read_model_contract_tests.rs"]
mod web_read_model_contract_tests;

#[cfg(test)]
#[path = "schronu/web_read_service_contract_tests.rs"]
mod web_read_service_contract_tests;

/// CLI applicationを起動する。
pub fn run_cli() {
    runtime::application();
}

pub use storage_directory::resolve_project_storage_directory;
pub use web_service::{
    ScheduledTaskRowDto, ServerSnapshot, SessionTaskDto, WebReadError, WebReadOverflowError,
    WebService, WebSuccess,
};
pub use web_session_write::{
    CompleteSessionRequest, RecordSessionRequest, RecordSessionResult, WebSessionInputError,
};

#[cfg(test)]
mod entrypoint_contract_tests {
    #[test]
    fn binary_entrypoint_delegates_to_library_cli() {
        const EXPECTED_SOURCE: &str =
            "fn main() {\n    schronu::adapter::controller::run_cli();\n}";

        assert_eq!(include_str!("schronu.rs").trim(), EXPECTED_SOURCE);
    }
}
