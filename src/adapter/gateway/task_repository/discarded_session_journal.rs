use super::*;

const JOURNAL_DIRECTORY_NAME: &str = "discarded_sessions";

#[derive(Debug, Eq, PartialEq)]
pub struct DuplicateDiscardedSessionEventIdError {
    event_id: Uuid,
    first_path: PathBuf,
    first_index: usize,
    duplicate_path: PathBuf,
    duplicate_index: usize,
}

impl DuplicateDiscardedSessionEventIdError {
    pub fn event_id(&self) -> Uuid {
        self.event_id
    }
    pub fn first_path(&self) -> &Path {
        &self.first_path
    }
    pub fn first_index(&self) -> usize {
        self.first_index
    }
    pub fn duplicate_path(&self) -> &Path {
        &self.duplicate_path
    }
    pub fn duplicate_index(&self) -> usize {
        self.duplicate_index
    }
}

impl fmt::Display for DuplicateDiscardedSessionEventIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "duplicate discarded session event ID {} at {}:events[{}] and {}:events[{}]",
            self.event_id,
            self.first_path.display(),
            self.first_index,
            self.duplicate_path.display(),
            self.duplicate_index
        )
    }
}

impl Error for DuplicateDiscardedSessionEventIdError {}

pub(in crate::adapter::gateway) fn is_discarded_session_journal_path(
    storage_root: &Path,
    path: &Path,
) -> bool {
    let Ok(relative) = path.strip_prefix(storage_root) else {
        return false;
    };
    if relative.parent() != Some(Path::new(JOURNAL_DIRECTORY_NAME)) {
        return false;
    }
    let Some(file_name) = relative.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(stem) = file_name.strip_suffix(".yaml") else {
        return false;
    };
    stem.len() == 7
        && stem.as_bytes().get(4) == Some(&b'-')
        && stem[..4].bytes().all(|byte| byte.is_ascii_digit())
        && stem[5..].bytes().all(|byte| byte.is_ascii_digit())
        && stem[..4]
            .parse::<i32>()
            .ok()
            .zip(stem[5..].parse::<u32>().ok())
            .is_some_and(|(year, month)| NaiveDate::from_ymd_opt(year, month, 1).is_some())
}

pub(super) fn journal_directory(storage_root: &Path) -> PathBuf {
    storage_root.join(JOURNAL_DIRECTORY_NAME)
}

pub(super) struct PreparedJournalWrite {
    pub(super) path: PathBuf,
    pub(super) bytes: Vec<u8>,
}

impl TaskRepository {
    pub(in crate::adapter::gateway) fn load_captured_with_journals<'a, I, J>(
        &mut self,
        storage_revision: Option<(&Path, &[u8])>,
        project_files: I,
        journal_files: J,
    ) -> Result<(), TaskRepositoryError>
    where
        I: IntoIterator<Item = (&'a Path, &'a [u8])>,
        J: IntoIterator<Item = (&'a Path, &'a [u8])>,
    {
        let storage_revision = storage_revision
            .map(|(path, bytes)| parse_storage_revision(path, bytes))
            .transpose()
            .map_err(|error| {
                TaskRepositoryError::new(ApplicationRepositoryOperation::Load, error)
            })?;
        let mut project_files = project_files.into_iter().collect::<Vec<_>>();
        project_files.sort_by(|left, right| left.0.cmp(right.0));
        let mut builder = RepositoryLoadBuilder::new(self.last_synced_time);
        for (path, bytes) in project_files {
            builder.push(path.to_path_buf(), path.to_path_buf(), bytes)?;
        }
        let discarded_sessions = Self::parse_journal_files(journal_files)?;
        let loaded = builder.finish(storage_revision, discarded_sessions)?;
        self.apply_loaded_state(loaded);
        Ok(())
    }

    pub(super) fn validate_journal_directory(&self) -> Result<(), TaskRepositoryError> {
        let directory = journal_directory(Path::new(&self.project_storage_dir_name));
        match fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                Err(TaskRepositoryError::new(
                    ApplicationRepositoryOperation::Load,
                    FileRepositoryError::new(
                        FileRepositoryOperation::ReadMetadata,
                        directory,
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "discarded_sessions must be a non-symbolic-link directory",
                        ),
                    ),
                ))
            }
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(TaskRepositoryError::new(
                ApplicationRepositoryOperation::Load,
                FileRepositoryError::new(FileRepositoryOperation::ReadMetadata, directory, error),
            )),
        }
    }

    pub(super) fn parse_journal_files<'a, I>(
        journal_files: I,
    ) -> Result<Vec<DiscardedSessionEvent>, TaskRepositoryError>
    where
        I: IntoIterator<Item = (&'a Path, &'a [u8])>,
    {
        let mut events = Vec::new();
        let mut by_id = HashMap::<Uuid, (PathBuf, usize)>::new();
        for (path, bytes) in journal_files {
            for parsed in parse_journal(path, bytes).map_err(|error| {
                TaskRepositoryError::new(ApplicationRepositoryOperation::Load, error)
            })? {
                let event_id = parsed.event.event_id();
                if let Some((first_path, first_index)) =
                    by_id.insert(event_id, (path.to_path_buf(), parsed.index))
                {
                    return Err(TaskRepositoryError::new(
                        ApplicationRepositoryOperation::Load,
                        DuplicateDiscardedSessionEventIdError {
                            event_id,
                            first_path,
                            first_index,
                            duplicate_path: path.to_path_buf(),
                            duplicate_index: parsed.index,
                        },
                    ));
                }
                events.push(parsed.event);
            }
        }
        Ok(events)
    }

    fn journal_path(&self, year: i32, month: u32) -> PathBuf {
        journal_directory(Path::new(&self.project_storage_dir_name))
            .join(format!("{year:04}-{month:02}.yaml"))
    }

    pub(super) fn prepare_journal_writes(
        &self,
    ) -> Result<Vec<PreparedJournalWrite>, TaskRepositoryError> {
        let dirty_months = self
            .dirty_journal_months
            .borrow()
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let mut writes = Vec::new();
        for (year, month) in &dirty_months {
            let path = self.journal_path(*year, *month);
            let events = self
                .discarded_sessions
                .iter()
                .filter(|event| {
                    event.logical_date().year() == *year && event.logical_date().month() == *month
                })
                .cloned()
                .collect::<Vec<_>>();
            let bytes = serialize_journal(&path, &events).map_err(|error| {
                TaskRepositoryError::retryable_save(FileRepositoryError::new(
                    FileRepositoryOperation::SerializeJournal,
                    &path,
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                ))
            })?;
            if !fs::read(&path).is_ok_and(|existing| existing == bytes) {
                writes.push(PreparedJournalWrite { path, bytes });
            }
        }
        Ok(writes)
    }
}

impl DiscardedSessionJournalTrait for TaskRepository {
    fn append_discarded_session(
        &mut self,
        event: DiscardedSessionEvent,
    ) -> Result<AppendDiscardedSessionOutcome, DiscardedSessionConflictError> {
        if let Some(existing) = self
            .discarded_sessions
            .iter()
            .find(|existing| existing.event_id() == event.event_id())
        {
            return if existing == &event {
                Ok(AppendDiscardedSessionOutcome::AlreadyPresent)
            } else {
                Err(DiscardedSessionConflictError::new(event.event_id()))
            };
        }
        self.dirty_journal_months
            .borrow_mut()
            .insert((event.logical_date().year(), event.logical_date().month()));
        self.discarded_sessions.push(event);
        Ok(AppendDiscardedSessionOutcome::Appended)
    }

    fn discarded_sessions_on(
        &self,
        logical_date: NaiveDate,
    ) -> Result<DiscardedSessionDaySummary, DiscardedSessionSummaryError> {
        summarize_discarded_sessions(&self.discarded_sessions, logical_date)
    }
}
