#[cfg(test)]
mod tests {
    use super::resolve_project_storage_directory;
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn 設定値と既定値を区別する() {
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
    fn 非utf8設定値を理由付きerrorにする() {
        use std::os::unix::ffi::OsStringExt;

        let error =
            resolve_project_storage_directory(Some(OsString::from_vec(vec![0xff]))).unwrap_err();

        assert!(!error.is_empty());
    }
}
