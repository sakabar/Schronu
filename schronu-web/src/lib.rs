use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tokio::sync::oneshot;

#[cfg(any(feature = "web", feature = "server"))]
pub mod app;

pub const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshTrigger {
    Initial,
    Manual,
    Interval,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct RefreshState {
    text: Option<String>,
    error: Option<String>,
    is_refreshing: bool,
}

impl RefreshState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin_refresh(&mut self, _trigger: RefreshTrigger) -> bool {
        if self.is_refreshing {
            return false;
        }
        self.is_refreshing = true;
        true
    }

    pub fn complete_refresh(&mut self, result: Result<String, String>) {
        self.is_refreshing = false;
        match result {
            Ok(text) => {
                self.text = Some(text);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn is_refreshing(&self) -> bool {
        self.is_refreshing
    }
}

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
