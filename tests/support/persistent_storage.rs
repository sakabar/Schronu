use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub fn persistent_storage_bytes_excluding_process_lock(
    storage: &Path,
) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    collect_directory_bytes(storage, storage, &mut files);
    files
}

fn collect_directory_bytes(
    storage: &Path,
    directory: &Path,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
) {
    let mut entries = fs::read_dir(directory)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type().unwrap();
        if file_type.is_dir() {
            collect_directory_bytes(storage, &path, files);
        } else if file_type.is_file() {
            let relative_path = path.strip_prefix(storage).unwrap().to_path_buf();
            if relative_path != Path::new(".lock") {
                files.insert(relative_path, fs::read(path).unwrap());
            }
        }
    }
}
