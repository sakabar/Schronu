mod component;
#[cfg(all(feature = "web", target_arch = "wasm32"))]
mod component_dispatch;
#[cfg(all(feature = "web", target_arch = "wasm32"))]
mod component_models;
#[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
mod component_runtime;
#[cfg(test)]
mod component_tests;
#[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
mod effect_dispatcher;
#[cfg(test)]
mod effect_dispatcher_tests;
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
#[cfg(test)]
mod view_test_support;
mod web_endpoint;

pub use component::app;
#[cfg(feature = "server")]
pub use environment_web_operations::web_worker_from_environment;
pub use web_endpoint::{
    auto_session, bootstrap, complete_session, list_tasks, record_session, WebOperationResult,
};
