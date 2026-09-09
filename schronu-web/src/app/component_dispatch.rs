use super::component_models::{browser_monotonic_now_ms, browser_now_epoch_ms};
use super::component_runtime::{
    component_actions_from_session_action, ComponentAction, ComponentOrchestrator,
};
use super::effect_dispatcher::{execute_tracked_effect, ServerFunctionGateway};
use super::session_view::SessionAction;
use crate::client::state::ClientEffect;
use crate::client::work_sessions::BrowserLocalStorage;
use dioxus::prelude::*;

pub(crate) fn dispatch_session_action(
    client: Signal<ComponentOrchestrator>,
    action: SessionAction,
) {
    let actions = component_actions_from_session_action(action, browser_now_epoch_ms());
    for action in actions {
        dispatch_action(client, action);
    }
}

pub(crate) fn dispatch_action(mut client: Signal<ComponentOrchestrator>, action: ComponentAction) {
    let effect = client
        .write()
        .action(&BrowserLocalStorage, browser_monotonic_now_ms(), action);
    dispatch_action_effect(client, effect);
}

pub(crate) fn dispatch_action_effect(
    mut client: Signal<ComponentOrchestrator>,
    effect: ClientEffect,
) {
    if effect == ClientEffect::None {
        return;
    }
    let background = client.read().effect_is_background(&effect);
    spawn(async move {
        let response = execute_tracked_effect(&ServerFunctionGateway, effect, |pending| {
            if !background {
                if pending {
                    client.write().begin_server_effect();
                } else {
                    client.write().finish_server_effect();
                }
            }
        })
        .await;
        let Some(response) = response else {
            return;
        };
        let follow_up = client
            .write()
            .apply_response(&BrowserLocalStorage, response);
        dispatch_action_effect(client, follow_up);
    });
}
