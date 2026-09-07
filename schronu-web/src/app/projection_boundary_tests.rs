#![cfg(feature = "server")]

use super::list_view::ListView;
use super::session_view::SessionView;
use crate::client::view_projection::{ListRowViewModel, SessionCardViewModel};
use dioxus::prelude::*;

#[test]
fn client_projectionの型をviewへ直接渡せる() {
    let mut dom = VirtualDom::new(|| {
        let sessions: Vec<SessionCardViewModel> = Vec::new();
        let rows: Vec<ListRowViewModel> = Vec::new();
        rsx! {
            SessionView {
                sessions,
                global_blocked: false,
                on_auto_session: move |_| {},
                on_action: move |_| {},
            }
            ListView {
                dates: Vec::new(),
                rows,
                active_task_ids: Vec::new(),
                date_input_text: String::new(),
                date_input_error: None,
                filter_text: String::new(),
                on_select_date: move |_| {},
                on_date_input_change: move |_| {},
                on_submit_date_input: move |_| {},
                on_start_session: move |_| {},
                on_filter_change: move |_| {},
            }
        }
    });
    dom.rebuild_in_place();
}
