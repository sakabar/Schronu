mod component;
#[cfg(feature = "server")]
mod environment_query;
mod today_endpoint;

pub use component::app;
#[cfg(feature = "server")]
pub use environment_query::worker_from_environment;
pub use today_endpoint::today_text;
