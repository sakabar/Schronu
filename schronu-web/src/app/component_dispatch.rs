use super::component_models::{browser_monotonic_now_ms, browser_now_epoch_ms};
use super::component_runtime::{
    component_actions_from_session_action, ComponentAction, ComponentOrchestrator,
};
use super::effect_dispatcher::{execute_tracked_effect, ServerFunctionGateway};
use super::session_view::SessionAction;
use crate::client::state::ClientEffect;
use crate::client::state::ServerFailure;
use crate::client::work_sessions::BrowserLocalStorage;
use crate::{AllTaskRow, ListAllTasksPageRequest};
use dioxus::prelude::*;
use std::collections::HashSet;

pub(crate) fn dispatch_select_all_tasks(mut client: Signal<ComponentOrchestrator>) {
    let Some(request_id) = client.write().select_all_tasks() else {
        return;
    };
    client.write().begin_server_effect();
    spawn(async move {
        let result = load_all_task_pages(client).await;
        client.write().finish_server_effect();
        client.write().apply_all_task_result(request_id, result);
    });
}

async fn load_all_task_pages(
    mut client: Signal<ComponentOrchestrator>,
) -> Result<Vec<AllTaskRow>, ()> {
    let mut rows = Vec::new();
    let mut cursor = None;
    let mut seen_cursors = HashSet::new();
    loop {
        let request = ListAllTasksPageRequest {
            cursor: cursor.clone(),
        };
        let result = super::list_all_tasks_page(request.clone()).await;
        let page = match result {
            Ok(Ok(page)) => {
                client.write().record_all_task_page_result(request, Ok(()));
                page
            }
            Ok(Err(error)) => {
                client
                    .write()
                    .record_all_task_page_result(request, Err(ServerFailure::Operation(error)));
                return Err(());
            }
            Err(error) => {
                client.write().record_all_task_page_result(
                    request,
                    Err(ServerFailure::Transport(error.to_string())),
                );
                return Err(());
            }
        };
        rows.extend(page.rows);
        match page.next_cursor {
            Some(next) if seen_cursors.insert(next.clone()) => cursor = Some(next),
            Some(_) => return Err(()),
            None => return Ok(rows),
        }
    }
}

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
