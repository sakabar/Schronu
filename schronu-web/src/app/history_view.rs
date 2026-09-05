use dioxus::prelude::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntryViewModel {
    pub occurred_at_hh_mm_ss: String,
    pub operation: String,
    pub locality: String,
    pub outcome: String,
    pub summary: String,
    pub failed: bool,
}

#[component]
pub fn HistoryView(entries: Vec<HistoryEntryViewModel>) -> Element {
    rsx! {
        details { class: "history-panel",
            summary { "発火履歴" }
            if entries.is_empty() {
                p { class: "history-empty", "履歴はありません。" }
            } else {
                ol { class: "history-list",
                    for entry in entries {
                        li {
                            class: if entry.failed {
                                "history-entry is-failure"
                            } else {
                                "history-entry"
                            },
                            time { "{entry.occurred_at_hh_mm_ss}" }
                            code { class: "history-operation", "{entry.operation}" }
                            span { class: "history-locality", "{entry.locality}" }
                            span { class: "history-outcome", "{entry.outcome}" }
                            span { class: "history-summary", "{entry.summary}" }
                        }
                    }
                }
            }
        }
    }
}
