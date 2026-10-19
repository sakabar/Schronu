use std::borrow::Cow;

/// Removes the first lowercase `http` and the rest of that line.
///
/// Matches the historical CLI replacement of the first `http.*` match:
/// whitespace is preserved, and a newline ends the removed suffix.
pub fn remove_url_suffix(name: &str) -> Cow<'_, str> {
    let Some(start) = name.find("http") else {
        return Cow::Borrowed(name);
    };
    let Some(line_end) = name[start..].find('\n') else {
        return Cow::Borrowed(&name[..start]);
    };
    let mut result = String::with_capacity(name.len());
    result.push_str(&name[..start]);
    result.push_str(&name[start + line_end..]);
    Cow::Owned(result)
}

#[cfg(test)]
mod tests {
    use super::remove_url_suffix;

    #[test]
    fn removes_first_lowercase_http_suffix_with_cli_whitespace_and_newline_rules() {
        for (name, expected) in [
            ("task http://example.com", "task "),
            ("task https://example.com/path", "task "),
            ("task", "task"),
            ("task  ", "task  "),
            ("taskhttp-text", "task"),
            ("task http://example.com https://example.org", "task "),
            ("https://example.com", ""),
            ("task HTTP://example.com", "task HTTP://example.com"),
            ("検証 https://example.com", "検証 "),
            (
                "task http://example.com\nnext http://example.org",
                "task \nnext http://example.org",
            ),
            ("task http://example.com\r\nnext", "task \nnext"),
            ("task\nnext https://example.com", "task\nnext "),
            ("", ""),
        ] {
            assert_eq!(remove_url_suffix(name), expected, "{name:?}");
        }
    }
}
