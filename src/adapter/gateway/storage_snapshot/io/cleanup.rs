#[cfg(any(target_os = "macos", target_os = "linux"))]
use super::{c_name, StableDirectory};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::adapter::gateway::storage_snapshot::io::read_directory_names;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::adapter::gateway::storage_snapshot::manifest::MINIMUM_DIRECTORY_ENTRY_BYTES;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::adapter::gateway::storage_snapshot::DEFAULT_RESOURCE_LIMITS;

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[derive(Clone, Copy)]
struct CleanupLimits {
    max_depth: usize,
    max_entries: u64,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[derive(Default)]
struct CleanupUsage {
    entries: u64,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl StableDirectory {
    pub(super) fn remove_contents(&self) -> std::io::Result<()> {
        let directory_entries =
            DEFAULT_RESOURCE_LIMITS.manifest_bytes / MINIMUM_DIRECTORY_ENTRY_BYTES;
        let file_entries = u64::try_from(DEFAULT_RESOURCE_LIMITS.file_count).unwrap_or(u64::MAX);
        let max_entries = directory_entries
            .checked_add(file_entries)
            // Snapshot publication adds manifest.json and storage around the payload.
            .and_then(|entries| entries.checked_add(2))
            .unwrap_or(u64::MAX);
        self.remove_contents_with_limits(CleanupLimits {
            // Snapshot publication wraps payload paths in the storage directory.
            max_depth: DEFAULT_RESOURCE_LIMITS.depth.saturating_add(1),
            max_entries,
        })
    }

    fn remove_contents_with_limits(&self, limits: CleanupLimits) -> std::io::Result<()> {
        self.remove_contents_bounded(limits, 0, &mut CleanupUsage::default())
    }

    fn remove_contents_bounded(
        &self,
        limits: CleanupLimits,
        depth: usize,
        usage: &mut CleanupUsage,
    ) -> std::io::Result<()> {
        loop {
            let name = {
                let mut names = read_directory_names(self.raw_fd())?;
                names.next().transpose()?
            };
            let Some(name) = name else {
                break;
            };
            usage.entries = usage
                .entries
                .checked_add(1)
                .ok_or_else(cleanup_entry_limit_error)?;
            if usage.entries > limits.max_entries {
                return Err(cleanup_entry_limit_error());
            }
            let name = c_name(&name)?;
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            // SAFETY: stat is writable and name is live for the call.
            if unsafe {
                libc::fstatat(
                    self.raw_fd(),
                    name.as_ptr(),
                    stat.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            // SAFETY: fstatat initialized stat on success.
            let stat = unsafe { stat.assume_init() };
            if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
                let child_depth = depth.checked_add(1).ok_or_else(cleanup_depth_limit_error)?;
                if child_depth > limits.max_depth {
                    return Err(cleanup_depth_limit_error());
                }
                let child = Self::open_at(self.raw_fd(), &name)?;
                child.remove_contents_bounded(limits, child_depth, usage)?;
                // SAFETY: name is a child of the fixed directory handle.
                if unsafe { libc::unlinkat(self.raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
            } else {
                // SAFETY: name is a child of the fixed directory handle.
                if unsafe { libc::unlinkat(self.raw_fd(), name.as_ptr(), 0) } != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
        }
        Ok(())
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn cleanup_entry_limit_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "snapshot rollback cleanup entry limit exceeded",
    )
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn cleanup_depth_limit_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "snapshot rollback cleanup depth limit exceeded",
    )
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod cleanup_limit_tests {
    use super::*;
    use std::fs::{self, File};

    fn stable_directory(label: &str) -> (std::path::PathBuf, StableDirectory) {
        let root = std::env::temp_dir().join(format!(
            "schronu-cleanup-limit-{label}-{}",
            uuid::Uuid::new_v4().hyphenated()
        ));
        fs::create_dir(&root).unwrap();
        let directory = File::open(&root).unwrap();
        (root, StableDirectory { directory })
    }

    #[test]
    fn rollback_cleanupは処理entry上限で停止する() {
        let (root, directory) = stable_directory("entries");
        fs::write(root.join("first"), b"first").unwrap();
        fs::write(root.join("second"), b"second").unwrap();
        let limits = CleanupLimits {
            max_depth: 64,
            max_entries: 1,
        };

        let error = directory.remove_contents_with_limits(limits).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("cleanup entry limit"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rollback_cleanupは再帰depth上限で停止する() {
        let (root, directory) = stable_directory("depth");
        fs::create_dir_all(root.join("first/second")).unwrap();
        let limits = CleanupLimits {
            max_depth: 1,
            max_entries: 10,
        };

        let error = directory.remove_contents_with_limits(limits).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("cleanup depth limit"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rollback_cleanupはsnapshot_wrapper込みの最大depthを処理する() {
        let (root, directory) = stable_directory("default-depth");
        let mut deepest = root.join("storage");
        for _ in 0..DEFAULT_RESOURCE_LIMITS.depth {
            deepest.push("d");
        }
        fs::create_dir_all(deepest).unwrap();

        directory.remove_contents().unwrap();

        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir(root).unwrap();
    }
}
