use crate::client::state::{load_client_state_for_ui, ActiveTab, ClientEffect, ClientState};
use crate::client::work_sessions::KeyValueStorage;
use crate::SessionTask;

use super::effect_dispatcher::{apply_response, ClientResponse};
use super::session_view::{SessionAction, SessionActionKind};

pub(crate) enum ComponentAction {
    SwitchTab(ActiveTab),
    Tick(i64),
    SelectDate(String),
    AutoSession,
    AddSession(SessionTask),
    DiscardSession(String),
    RecordSession(String),
    CompleteSession(String),
    CompleteSessionWithoutRecording(String),
    ConfirmRepositoryChecked,
}

pub(crate) fn component_action_from_session_action(action: SessionAction) -> ComponentAction {
    match action.kind {
        SessionActionKind::Discard => ComponentAction::DiscardSession(action.task_id),
        SessionActionKind::Record => ComponentAction::RecordSession(action.task_id),
        SessionActionKind::Complete => ComponentAction::CompleteSession(action.task_id),
        SessionActionKind::CompleteWithoutRecording => {
            ComponentAction::CompleteSessionWithoutRecording(action.task_id)
        }
    }
}

pub(crate) fn initialize_client<S: KeyValueStorage>(
    storage: &S,
    now_epoch_ms: i64,
) -> (ClientState, ClientEffect) {
    let mut state = load_client_state_for_ui(storage, now_epoch_ms);
    let effect = state.request_bootstrap();
    (state, effect)
}

pub(crate) struct ComponentOrchestrator {
    state: Option<ClientState>,
    mounted: bool,
}

impl ComponentOrchestrator {
    pub fn new() -> Self {
        Self {
            state: None,
            mounted: false,
        }
    }

    pub fn state(&self) -> Option<&ClientState> {
        self.state.as_ref()
    }

    pub fn mount<S: KeyValueStorage>(&mut self, storage: &S, now_epoch_ms: i64) -> ClientEffect {
        if self.mounted {
            return ClientEffect::None;
        }
        self.mounted = true;
        let (state, effect) = initialize_client(storage, now_epoch_ms);
        self.state = Some(state);
        effect
    }

    pub fn action<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        action: ComponentAction,
    ) -> ClientEffect {
        self.state.as_mut().map_or(ClientEffect::None, |state| {
            reduce_component_action(state, storage, action)
        })
    }

    pub fn apply_response<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        response: ClientResponse,
    ) -> ClientEffect {
        self.state.as_mut().map_or(ClientEffect::None, |state| {
            apply_response(state, storage, response)
        })
    }
}

pub(crate) fn reduce_component_action<S: KeyValueStorage>(
    state: &mut ClientState,
    storage: &S,
    action: ComponentAction,
) -> ClientEffect {
    match action {
        ComponentAction::SwitchTab(tab) => state.switch_tab(tab),
        ComponentAction::Tick(now_epoch_ms) => state.tick(now_epoch_ms),
        ComponentAction::SelectDate(logical_date) => state.request_list(&logical_date),
        ComponentAction::AutoSession => state.request_auto_session(),
        ComponentAction::AddSession(task) => state.add_session_from_task(storage, &task),
        ComponentAction::DiscardSession(task_id) => state.discard_session(storage, &task_id),
        ComponentAction::RecordSession(task_id) => state.begin_record_session(storage, &task_id),
        ComponentAction::CompleteSession(task_id) => {
            state.begin_complete_session(storage, &task_id)
        }
        ComponentAction::CompleteSessionWithoutRecording(task_id) => {
            state.begin_complete_session_without_recording(storage, &task_id)
        }
        ComponentAction::ConfirmRepositoryChecked => state.confirm_repository_checked(storage),
    }
}
