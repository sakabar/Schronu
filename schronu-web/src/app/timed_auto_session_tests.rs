use super::session_view::SessionView;
use super::view_test_support::{
    dispatch_click, dispatch_platform_event, rebuild_with_named_event_listeners,
};
use dioxus::html::SerializedFormData;
use dioxus::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct InputProps {
    requests: Arc<Mutex<Vec<Option<i64>>>>,
}

fn input_root(props: InputProps) -> Element {
    rsx! {
        SessionView {
            sessions: Vec::new(),
            global_blocked: false,
            on_auto_session: move |minutes| props.requests.lock().unwrap().push(minutes),
            on_action: move |_| {},
        }
    }
}

#[test]
fn 分数入力から自動セッションへ空欄と正整数だけを送る() {
    for (input, expected) in [
        ("", Some(None)),
        ("30", Some(Some(30))),
        ("0", None),
        ("-1", None),
        ("1.5", None),
        ("text", None),
        ("9223372036854775807", None),
    ] {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut dom = VirtualDom::new_with_props(
            input_root,
            InputProps {
                requests: Arc::clone(&requests),
            },
        );
        let listeners = rebuild_with_named_event_listeners(&mut dom);
        let input_id = listeners
            .iter()
            .find(|(name, _)| name == "input")
            .unwrap()
            .1;
        let click_id = listeners
            .iter()
            .find(|(name, _)| name == "click")
            .unwrap()
            .1;
        dispatch_platform_event(
            &dom,
            "input",
            input_id,
            Box::new(SerializedFormData::new(input.to_owned(), Vec::new())),
        );
        dom.render_immediate_to_vec();
        dispatch_click(&dom, click_id);
        dom.render_immediate_to_vec();

        assert_eq!(
            *requests.lock().unwrap(),
            expected.into_iter().collect::<Vec<_>>(),
            "input: {input}"
        );
        let html = dioxus::ssr::render(&dom);
        assert_eq!(
            html.contains("role=\"alert\""),
            expected.is_none(),
            "input: {input}, html: {html}"
        );
        assert!(html.contains("指定分数未満"));
    }
}
