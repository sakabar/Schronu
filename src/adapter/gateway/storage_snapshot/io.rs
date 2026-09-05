use std::ffi::CString;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[cfg(unix)]
fn c_path(path: &Path) -> std::io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "filesystem path contains a NUL byte",
        )
    })
}

#[cfg(target_os = "macos")]
pub(in crate::adapter::gateway) fn rename_no_replace(
    from: &Path,
    to: &Path,
) -> std::io::Result<()> {
    let from = c_path(from)?;
    let to = c_path(to)?;
    // SAFETY: both pointers are backed by live CStrings and renamex_np does not retain them.
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
pub(in crate::adapter::gateway) fn rename_no_replace(
    from: &Path,
    to: &Path,
) -> std::io::Result<()> {
    let from = c_path(from)?;
    let to = c_path(to)?;
    // SAFETY: both pointers are backed by live CStrings and renameat2 does not retain them.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(in crate::adapter::gateway) fn rename_no_replace(
    _from: &Path,
    _to: &Path,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic no-replace rename is supported only on macOS and Linux",
    ))
}
