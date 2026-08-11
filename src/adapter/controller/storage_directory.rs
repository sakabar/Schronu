use std::ffi::OsString;
use std::path::PathBuf;

const DEFAULT_STORAGE_DIRECTORY: &str = "../Schronu-private/tasks/";

pub fn resolve_project_storage_directory(configured: Option<OsString>) -> Result<PathBuf, String> {
    let path = configured
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_STORAGE_DIRECTORY));
    if path.to_str().is_none() {
        return Err("storage directory path must be valid UTF-8".to_string());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::resolve_project_storage_directory;
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn storage_directoryは環境変数の設定値と既定値を区別する() {
        assert_eq!(
            resolve_project_storage_directory(Some(OsString::from("configured/tasks"))).unwrap(),
            PathBuf::from("configured/tasks")
        );
        assert_eq!(
            resolve_project_storage_directory(None).unwrap(),
            PathBuf::from("../Schronu-private/tasks/")
        );
    }

    #[cfg(unix)]
    #[test]
    fn storage_directoryは非utf8の環境変数を理由付きerrorにする() {
        use std::os::unix::ffi::OsStringExt;

        let error =
            resolve_project_storage_directory(Some(OsString::from_vec(vec![0xff]))).unwrap_err();

        assert!(!error.is_empty());
    }
}
