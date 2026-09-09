use super::*;

#[test]
fn 不確実mutation確認後は固定された同一操作buttonだけを有効にする() {
    for (retrying, enabled_class) in [
        (
            SessionCardViewModel {
                retry_discard_release_only: true,
                ..card("release")
            },
            "session-action-discard",
        ),
        (
            SessionCardViewModel {
                retry_record_only: true,
                ..card("record")
            },
            "session-action-record",
        ),
        (
            SessionCardViewModel {
                retry_complete_only: true,
                ..card("complete")
            },
            "session-action-complete\"",
        ),
        (
            SessionCardViewModel {
                retry_discard_complete_only: true,
                ..card("discard-complete")
            },
            "session-action-complete-without-recording",
        ),
    ] {
        let (html, _) = render(vec![retrying], false);
        assert_eq!(html.matches("disabled").count(), 3, "{html}");
        let enabled_button = html
            .split_once(enabled_class)
            .unwrap()
            .1
            .split_once("</button>")
            .unwrap()
            .0;
        assert!(!enabled_button.contains("disabled"), "{html}");
    }
}
