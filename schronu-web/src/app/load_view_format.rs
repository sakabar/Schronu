pub(super) fn format_unsigned(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}
