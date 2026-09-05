mod component;
#[cfg(feature = "server")]
mod environment_query;
#[cfg(feature = "server")]
mod environment_web_operations;
pub(crate) mod history_view;
#[cfg(test)]
mod history_view_tests;
pub(crate) mod list_view;
#[cfg(test)]
mod list_view_tests;
#[cfg(test)]
mod projection_boundary_tests;
pub(crate) mod session_view;
#[cfg(test)]
mod session_view_tests;
mod today_endpoint;
#[cfg(test)]
mod view_test_support;
mod web_endpoint;

pub use component::app;
#[cfg(feature = "server")]
pub use environment_query::worker_from_environment;
#[cfg(feature = "server")]
pub use environment_web_operations::web_worker_from_environment;
pub use today_endpoint::today_text;
pub use web_endpoint::{
    auto_session, bootstrap, complete_session, list_tasks, record_session, WebOperationResult,
};
