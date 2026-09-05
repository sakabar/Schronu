use std::{any::Any, rc::Rc, sync::Once};

use dioxus::dioxus_core::{ElementId, Mutation};
use dioxus::html::SerializedMouseData;
use dioxus::prelude::*;

pub fn rebuild_with_click_listeners(dom: &mut VirtualDom) -> Vec<ElementId> {
    ensure_event_converter();
    dom.rebuild_to_vec()
        .edits
        .into_iter()
        .filter_map(|mutation| match mutation {
            Mutation::NewEventListener { name, id } if name == "click" => Some(id),
            _ => None,
        })
        .collect()
}

pub fn dispatch_click(dom: &VirtualDom, element_id: ElementId) {
    ensure_event_converter();
    let event = Event::new(
        Rc::new(PlatformEventData::new(Box::<SerializedMouseData>::default())) as Rc<dyn Any>,
        true,
    );
    dom.runtime().handle_event("click", event, element_id);
}

fn ensure_event_converter() {
    static EVENT_CONVERTER: Once = Once::new();
    EVENT_CONVERTER.call_once(|| {
        set_event_converter(Box::new(dioxus::html::SerializedHtmlEventConverter));
    });
}
