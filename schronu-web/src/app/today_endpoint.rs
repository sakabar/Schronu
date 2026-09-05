use dioxus::prelude::*;

#[cfg(feature = "server")]
use crate::TodayWorkerHandle;

#[cfg(feature = "server")]
use dioxus::fullstack::axum::Extension;

#[server(endpoint = "today_text")]
pub async fn today_text() -> Result<String, ServerFnError> {
    #[cfg(feature = "server")]
    {
        let Extension(worker) =
            dioxus::fullstack::FullstackContext::extract::<Extension<TodayWorkerHandle>, _>()
                .await
                .map_err(|error| ServerFnError::new(error.to_string()))?;
        return worker.request_async().await.map_err(ServerFnError::new);
    }

    #[cfg(not(feature = "server"))]
    unreachable!("server function body only runs on the server")
}
