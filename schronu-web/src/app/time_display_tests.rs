use super::super::component::{BufferPanel, BufferPanelProps, TimeDisplayMode, TIME_DISPLAY_MODE};
use dioxus::prelude::*;

fn render_panel(mode: TimeDisplayMode, value: Option<i128>) -> String {
    let mut dom = VirtualDom::new_with_props(BufferPanel, BufferPanelProps { value, mode });
    dom.rebuild_in_place();
    dioxus::ssr::render(&dom)
}

#[test]
fn 余裕時間はbufferの符号と不足状態を表示する() {
    for (value, expected) in [
        (3_661, "+01:01:01"),
        (0, "00:00:00"),
        (-60, "-00:01:00"),
        (-25_201, "-07:00:01"),
        (90_000, "+25:00:00"),
    ] {
        let html = render_panel(TimeDisplayMode::Buffer, Some(value));
        assert!(html.contains("id=\"schronu-buffer-ready\""), "{html}");
        assert!(html.contains(">余裕時間<"), "{html}");
        assert!(html.contains(&format!(">{expected}<")), "{html}");
        assert!(
            html.contains(&format!("aria-label=\"余裕時間 {expected}\"")),
            "{html}"
        );
        assert_eq!(html.contains("is-negative"), value < 0, "{html}");
        assert!(!html.contains("睡眠"), "{html}");
        assert!(!html.contains("基準より"), "{html}");
    }
}

#[test]
fn 未取得時間は選択した表示名で示す() {
    for (mode, label) in [
        (TimeDisplayMode::Sleep, "睡眠時間"),
        (TimeDisplayMode::Buffer, "余裕時間"),
    ] {
        let html = render_panel(mode, None);
        assert!(html.contains(&format!(">{label}<")), "{html}");
        assert!(html.contains(&format!("aria-label=\"{label}\"")), "{html}");
        assert!(html.contains(">未取得<"), "{html}");
        assert!(!html.contains("schronu-buffer-ready"), "{html}");
        assert!(!html.contains("is-negative"), "{html}");
    }
}

#[test]
fn 非表示では取得状態にかかわらず時間の要素を描画しない() {
    for value in [None, Some(3_661), Some(0), Some(-60), Some(-25_201)] {
        let html = render_panel(TimeDisplayMode::Hidden, value);
        for forbidden in [
            "section",
            "buffer-panel",
            "睡眠",
            "余裕",
            "未取得",
            "aria-label",
            "title=",
        ] {
            assert!(!html.contains(forbidden), "{html}");
        }
        for value in [
            "08:01:01",
            "07:00:00",
            "06:59:00",
            "-00:00:01",
            "+01:01:01",
            "00:00:00",
            "-00:01:00",
            "-07:00:01",
        ] {
            assert!(!html.contains(value), "{html}");
        }
    }
}

#[test]
fn 睡眠表示はbufferが0なら基準時間を通常色で表示する() {
    let html = render_panel(TimeDisplayMode::Sleep, Some(0));
    assert!(html.contains(">07:00:00<"), "{html}");
    assert!(html.contains("aria-label=\"睡眠時間\""), "{html}");
    assert!(!html.contains("is-negative"), "{html}");
}

fn configured_panel() -> Element {
    rsx! { BufferPanel { value: Some(0) } }
}

#[test]
fn 既定の時間表示はcompile_time定数に従う() {
    let mut dom = VirtualDom::new(configured_panel);
    dom.rebuild_in_place();
    assert_eq!(
        dioxus::ssr::render(&dom),
        render_panel(TIME_DISPLAY_MODE, Some(0))
    );
}
