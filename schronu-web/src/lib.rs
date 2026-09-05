use std::sync::mpsc;
use std::thread;
use tokio::sync::oneshot;

#[cfg(any(feature = "web", feature = "server"))]
pub mod app;
mod refresh_state;

pub use refresh_state::{RefreshState, RefreshTrigger, REFRESH_INTERVAL};
const TODAY_WORKER_STACK_SIZE_BYTES: usize = 32 * 1024 * 1024;

pub trait TodayTextQuery: 'static {
    fn today_text(&mut self) -> Result<String, String>;
}

#[derive(Clone, Debug)]
pub struct TodayWorkerHandle {
    commands: mpsc::Sender<TodayWorkerCommand>,
}

#[derive(Debug)]
struct TodayWorkerCommand {
    response: oneshot::Sender<Result<String, String>>,
}

impl TodayWorkerHandle {
    pub fn spawn<F, Q>(factory: F) -> Self
    where
        F: FnOnce() -> Q + Send + 'static,
        Q: TodayTextQuery,
    {
        let (commands, receiver) = mpsc::channel::<TodayWorkerCommand>();
        thread::Builder::new()
            .name("schronu-today-text".to_owned())
            .stack_size(TODAY_WORKER_STACK_SIZE_BYTES)
            .spawn(move || {
                let mut query = factory();
                while let Ok(command) = receiver.recv() {
                    let _ = command.response.send(query.today_text());
                }
            })
            .expect("today text worker thread must start");
        Self { commands }
    }

    pub async fn request_async(&self) -> Result<String, String> {
        let receiver = self.send_request()?;
        receiver
            .await
            .map_err(|_| "today text worker stopped before responding".to_owned())?
    }

    fn send_request(&self) -> Result<oneshot::Receiver<Result<String, String>>, String> {
        let (response, receiver) = oneshot::channel();
        self.commands
            .send(TodayWorkerCommand { response })
            .map_err(|_| "today text worker is not available".to_owned())?;
        Ok(receiver)
    }
}
