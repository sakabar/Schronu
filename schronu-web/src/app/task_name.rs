/// Formats a name only at the rendering boundary; stored names remain intact.
pub(super) fn display_task_name(name: &str) -> String {
    let stripped = schronu_shared::task_name::remove_url_suffix(name);
    if stripped == name {
        return name.to_owned();
    }
    let display = stripped.trim_end();
    if display.trim().is_empty() {
        "(タスク名なし)".to_owned()
    } else {
        display.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::display_task_name;

    #[test]
    fn display_trims_only_after_removal_and_supplies_an_empty_name_label() {
        for (name, expected) in [
            ("task https://example.com", "task"),
            ("task\t http://example.com", "task"),
            ("https://example.com", "(タスク名なし)"),
            (" \t https://example.com", "(タスク名なし)"),
            ("  task  ", "  task  "),
            ("task HTTP://example.com ", "task HTTP://example.com "),
            ("<3/6>task https://example.com", "<3/6>task"),
        ] {
            assert_eq!(display_task_name(name), expected);
        }
    }
}
