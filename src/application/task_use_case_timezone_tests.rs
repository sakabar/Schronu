use chrono::{Local, TimeZone};
use std::process::Command;

const TIMEZONE_TEST_CHILD: &str = "SCHRONU_TIMEZONE_TEST_CHILD";

fn in_new_york_timezone(test: impl FnOnce()) {
    if std::env::var_os(TIMEZONE_TEST_CHILD).is_some() {
        assert_eq!(
            Local
                .with_ymd_and_hms(2026, 1, 15, 12, 0, 0)
                .unwrap()
                .offset()
                .local_minus_utc(),
            -5 * 60 * 60
        );
        assert_eq!(
            Local
                .with_ymd_and_hms(2026, 7, 15, 12, 0, 0)
                .unwrap()
                .offset()
                .local_minus_utc(),
            -4 * 60 * 60
        );
        test();
        return;
    }

    let current_thread = std::thread::current();
    let test_name = current_thread
        .name()
        .expect("timezone test thread must have a name");
    let output = Command::new(std::env::current_exe().expect("test executable must exist"))
        .arg(test_name)
        .arg("--exact")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env("TZ", "America/New_York")
        .env(TIMEZONE_TEST_CHILD, "1")
        .output()
        .expect("timezone test subprocess must start");

    assert!(
        output.status.success(),
        "timezone test subprocess failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn timezone_subprocess_harnessはnew_yorkのoffsetを適用する() {
    in_new_york_timezone(|| {});
}
