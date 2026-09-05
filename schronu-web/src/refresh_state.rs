use std::time::Duration;

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
