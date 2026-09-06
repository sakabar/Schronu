use std::{any::Any, rc::Rc, sync::Once};

use dioxus::dioxus_core::{ElementId, Mutation};
use dioxus::html::SerializedMouseData;
use dioxus::prelude::*;

pub fn rebuild_with_click_listeners(dom: &mut VirtualDom) -> Vec<ElementId> {
    rebuild_with_event_listeners(dom, "click")
}

pub fn render_with_click_listeners(dom: &mut VirtualDom) -> Vec<ElementId> {
    ensure_event_converter();
    click_listener_ids(dom.render_immediate_to_vec().edits)
}

fn click_listener_ids(mutations: Vec<Mutation>) -> Vec<ElementId> {
    event_listener_ids(mutations, "click")
}

pub fn rebuild_with_event_listeners(
    dom: &mut VirtualDom,
    event_name: &'static str,
) -> Vec<ElementId> {
    ensure_event_converter();
    event_listener_ids(dom.rebuild_to_vec().edits, event_name)
}

fn event_listener_ids(mutations: Vec<Mutation>, event_name: &str) -> Vec<ElementId> {
    mutations
        .into_iter()
        .filter_map(|mutation| match mutation {
            Mutation::NewEventListener { name, id } if name == event_name => Some(id),
            _ => None,
        })
        .collect()
}

pub fn dispatch_click(dom: &VirtualDom, element_id: ElementId) {
    dispatch_platform_event(
        dom,
        "click",
        element_id,
        Box::<SerializedMouseData>::default(),
    );
}

pub fn dispatch_platform_event(
    dom: &VirtualDom,
    event_name: &'static str,
    element_id: ElementId,
    data: Box<dyn Any>,
) {
    ensure_event_converter();
    let event = Event::new(Rc::new(PlatformEventData::new(data)) as Rc<dyn Any>, true);
    dom.runtime().handle_event(event_name, event, element_id);
}

fn ensure_event_converter() {
    static EVENT_CONVERTER: Once = Once::new();
    EVENT_CONVERTER.call_once(|| {
        set_event_converter(Box::new(dioxus::html::SerializedHtmlEventConverter));
    });
}
