use super::storage_transaction::{FileSystemStorageTransactionIo, StorageTransactionIo};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RecordingOperation {
    CreateDirectory,
    ReadTargetMetadata,
    ReadFile,
    CreateFile,
    SetPermissions,
    WriteFile,
    SyncFile,
    Rename,
    RemoveDirectory,
    RemoveFile,
    SyncDirectory,
}

#[derive(Debug)]
pub(super) enum PathMatcher {
    Any,
    Exact(PathBuf),
    FileName(&'static str),
    FileNamePrefix(&'static str),
    FileNameContains(&'static str),
}

pub(super) struct FaultRule {
    pub(super) operation: RecordingOperation,
    pub(super) path_matcher: PathMatcher,
    pub(super) occurrence: usize,
    pub(super) error_kind: std::io::ErrorKind,
    pub(super) error_message: &'static str,
}

#[derive(Clone, Debug)]
pub(super) struct IoEvent {
    operation: RecordingOperation,
    path: PathBuf,
}

pub(super) struct RecordingIo {
    faults: Vec<FaultRule>,
    events: Mutex<Vec<IoEvent>>,
}

impl RecordingIo {
    pub(super) fn new(faults: Vec<FaultRule>) -> Self {
        Self {
            faults,
            events: Mutex::new(Vec::new()),
        }
    }

    fn record(&self, operation: RecordingOperation, path: &Path) -> std::io::Result<()> {
        let mut events = self.events.lock().unwrap();
        events.push(IoEvent {
            operation,
            path: path.to_path_buf(),
        });
        for fault in &self.faults {
            if fault.operation != operation || !fault.path_matcher.matches(path) {
                continue;
            }
            let occurrence = events
                .iter()
                .filter(|event| {
                    event.operation == fault.operation && fault.path_matcher.matches(&event.path)
                })
                .count();
            if occurrence == fault.occurrence {
                return Err(std::io::Error::new(fault.error_kind, fault.error_message));
            }
        }
        Ok(())
    }

    pub(super) fn events(&self) -> Vec<IoEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl PathMatcher {
    fn matches(&self, path: &Path) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(expected) => path == expected,
            Self::FileName(expected) => path.file_name().is_some_and(|name| name == *expected),
            Self::FileNamePrefix(expected) => path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(expected)),
            Self::FileNameContains(expected) => path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains(expected)),
        }
    }
}

pub(super) fn event_position(
    events: &[IoEvent],
    operation: RecordingOperation,
    path_matcher: &PathMatcher,
    occurrence: usize,
) -> usize {
    events
        .iter()
        .enumerate()
        .filter(|(_, event)| {
            event.operation == operation && path_matcher.matches(event.path.as_path())
        })
        .nth(occurrence - 1)
        .map(|(index, _)| index)
        .unwrap_or_else(|| {
            panic!("missing event {operation:?} matching {path_matcher:?} occurrence {occurrence}")
        })
}

pub(super) fn last_event_position(
    events: &[IoEvent],
    operation: RecordingOperation,
    path_matcher: &PathMatcher,
) -> usize {
    events
        .iter()
        .rposition(|event| {
            event.operation == operation && path_matcher.matches(event.path.as_path())
        })
        .unwrap_or_else(|| panic!("missing event {operation:?} matching {path_matcher:?}"))
}

impl StorageTransactionIo for RecordingIo {
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        self.record(RecordingOperation::CreateDirectory, path)?;
        FileSystemStorageTransactionIo.create_dir_all(path)
    }

    fn target_permissions(&self, path: &Path) -> std::io::Result<Option<fs::Permissions>> {
        self.record(RecordingOperation::ReadTargetMetadata, path)?;
        FileSystemStorageTransactionIo.target_permissions(path)
    }

    fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.record(RecordingOperation::ReadFile, path)?;
        FileSystemStorageTransactionIo.read_file(path)
    }

    fn create_new_file(&self, path: &Path) -> std::io::Result<()> {
        self.record(RecordingOperation::CreateFile, path)?;
        FileSystemStorageTransactionIo.create_new_file(path)
    }

    fn set_permissions(&self, path: &Path, permissions: fs::Permissions) -> std::io::Result<()> {
        self.record(RecordingOperation::SetPermissions, path)?;
        FileSystemStorageTransactionIo.set_permissions(path, permissions)
    }

    fn write_file(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        self.record(RecordingOperation::WriteFile, path)?;
        FileSystemStorageTransactionIo.write_file(path, bytes)
    }

    fn sync_file(&self, path: &Path) -> std::io::Result<()> {
        self.record(RecordingOperation::SyncFile, path)?;
        FileSystemStorageTransactionIo.sync_file(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        self.record(RecordingOperation::Rename, to)?;
        FileSystemStorageTransactionIo.rename(from, to)
    }

    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        self.record(RecordingOperation::RemoveDirectory, path)?;
        FileSystemStorageTransactionIo.remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        self.record(RecordingOperation::RemoveFile, path)?;
        FileSystemStorageTransactionIo.remove_file(path)
    }

    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        self.record(RecordingOperation::SyncDirectory, path)?;
        FileSystemStorageTransactionIo.sync_directory(path)
    }
}
