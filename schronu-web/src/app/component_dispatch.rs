use super::component_runtime::{ComponentAction, ComponentOrchestrator};
use super::effect_dispatcher::{execute_effect, ServerFunctionGateway};
use super::session_view::{SessionAction, SessionActionKind};
use crate::client::state::ClientEffect;
use crate::client::work_sessions::BrowserLocalStorage;
use dioxus::prelude::*;

pub(crate) fn dispatch_session_action(
    client: Signal<ComponentOrchestrator>,
    action: SessionAction,
) {
    let action = match action.kind {
        SessionActionKind::Discard => ComponentAction::DiscardSession(action.task_id),
        SessionActionKind::Record => ComponentAction::RecordSession(action.task_id),
        SessionActionKind::Complete => ComponentAction::CompleteSession(action.task_id),
        SessionActionKind::CompleteWithoutRecording => {
            ComponentAction::CompleteSessionWithoutRecording(action.task_id)
        }
    };
    dispatch_action(client, action);
}

pub(crate) fn dispatch_action(mut client: Signal<ComponentOrchestrator>, action: ComponentAction) {
    let effect = client.write().action(&BrowserLocalStorage, action);
    dispatch_action_effect(client, effect);
}

pub(crate) fn dispatch_action_effect(
    mut client: Signal<ComponentOrchestrator>,
    effect: ClientEffect,
) {
    if effect == ClientEffect::None {
        return;
    }
    spawn(async move {
        let Some(response) = execute_effect(&ServerFunctionGateway, effect).await else {
            return;
        };
        let follow_up = client
            .write()
            .apply_response(&BrowserLocalStorage, response);
        dispatch_action_effect(client, follow_up);
    });
}
