use std::io::{IsTerminal, Stdout, Write};
use termion::raw::RawTerminal;

pub(super) const MAX_COL: u16 = 999;

pub(super) trait SchronuWriter: Write {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error>;

    fn supports_ansi_color(&self) -> bool {
        true
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum DisplayFragment {
    Raw(Vec<u8>),
    Newline(String),
    Flush,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct DisplayModel {
    fragments: Vec<DisplayFragment>,
}

impl DisplayModel {
    pub(super) fn newline(message: impl Into<String>) -> Self {
        Self {
            fragments: vec![DisplayFragment::Newline(message.into())],
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.fragments.is_empty()
    }

    pub(super) fn fragments(&self) -> &[DisplayFragment] {
        &self.fragments
    }
}

#[derive(Default)]
pub(super) struct DisplayRecorder {
    model: DisplayModel,
}

impl DisplayRecorder {
    pub(super) fn model(&self) -> &DisplayModel {
        &self.model
    }
}

impl Write for DisplayRecorder {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.model
            .fragments
            .push(DisplayFragment::Raw(buffer.to_vec()));
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.model.fragments.push(DisplayFragment::Flush);
        Ok(())
    }
}

impl SchronuWriter for DisplayRecorder {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        self.model
            .fragments
            .push(DisplayFragment::Newline(message.to_string()));
        Ok(())
    }
}

pub(super) fn render_display_model(
    writer: &mut dyn SchronuWriter,
    model: &DisplayModel,
) -> Result<(), std::io::Error> {
    for fragment in model.fragments() {
        match fragment {
            DisplayFragment::Raw(buffer) => writer.write_all(buffer)?,
            DisplayFragment::Newline(message) => writer.writeln_newline(message)?,
            DisplayFragment::Flush => writer.flush()?,
        }
    }
    Ok(())
}

pub(super) struct ErrorCapturingWriter<'a> {
    inner: &'a mut dyn SchronuWriter,
    first_error: Option<std::io::Error>,
}

impl<'a> ErrorCapturingWriter<'a> {
    pub(super) fn new(inner: &'a mut dyn SchronuWriter) -> Self {
        Self {
            inner,
            first_error: None,
        }
    }

    pub(super) fn take_error(&mut self) -> Option<std::io::Error> {
        self.first_error.take()
    }

    fn capture(&mut self, error: std::io::Error) {
        if self.first_error.is_none() {
            self.first_error = Some(error);
        }
    }
}

impl Write for ErrorCapturingWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        match self.inner.write(buffer) {
            Ok(written) => Ok(written),
            Err(error) => {
                self.capture(error);
                Ok(buffer.len())
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Err(error) = self.inner.flush() {
            self.capture(error);
        }
        Ok(())
    }
}

impl SchronuWriter for ErrorCapturingWriter<'_> {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        if let Err(error) = self.inner.writeln_newline(message) {
            self.capture(error);
        }
        Ok(())
    }

    fn supports_ansi_color(&self) -> bool {
        self.inner.supports_ansi_color()
    }
}

impl SchronuWriter for RawTerminal<Stdout> {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        writeln!(self, "{}{}", termion::cursor::Left(MAX_COL), message)
    }

    fn supports_ansi_color(&self) -> bool {
        true
    }
}

impl SchronuWriter for Stdout {
    fn writeln_newline(&mut self, message: &str) -> Result<(), std::io::Error> {
        writeln!(self, "{}", message)
    }

    fn supports_ansi_color(&self) -> bool {
        self.is_terminal()
    }
}

pub(super) fn writeln_newline(
    writer: &mut dyn SchronuWriter,
    message: &str,
) -> Result<(), std::io::Error> {
    writer.writeln_newline(message)
}
