mod component;
#[cfg(feature = "server")]
mod environment_query;
#[cfg(feature = "server")]
mod environment_web_operations;
pub mod session_view;
mod today_endpoint;
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
