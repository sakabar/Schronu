#[cfg(any(feature = "web", feature = "server"))]
pub mod app;
mod refresh_state;
mod today_worker;

pub use refresh_state::{RefreshState, RefreshTrigger, REFRESH_INTERVAL};
pub use today_worker::{TodayTextQuery, TodayWorkerHandle};
