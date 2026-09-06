use crate::client::state::{load_client_state_for_ui, ActiveTab, ClientEffect, ClientState};
use crate::client::work_sessions::KeyValueStorage;
use crate::SessionTask;

use super::effect_dispatcher::{apply_response, ClientResponse};
use super::session_view::{SessionAction, SessionActionKind};

pub(crate) enum ComponentAction {
    SwitchTab(ActiveTab),
    Tick { wall_now_epoch_ms: i64 },
    SelectDate(String),
    AutoSession,
    AddSession(SessionTask),
    DiscardSession(String),
    RecordSession(String),
    CompleteSession(String),
    CompleteSessionWithoutRecording(String),
    ConfirmRepositoryChecked,
    EnableCarryLock,
    ArmCarryLock,
    DisableCarryLock,
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
    wall_now_epoch_ms: i64,
) -> (ClientState, ClientEffect) {
    let mut state = load_client_state_for_ui(storage, wall_now_epoch_ms);
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

    pub fn mount<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        wall_now_epoch_ms: i64,
    ) -> ClientEffect {
        if self.mounted {
            return ClientEffect::None;
        }
        self.mounted = true;
        let (state, effect) = initialize_client(storage, wall_now_epoch_ms);
        self.state = Some(state);
        effect
    }

    pub fn action<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        monotonic_now_ms: u64,
        action: ComponentAction,
    ) -> ClientEffect {
        self.state.as_mut().map_or(ClientEffect::None, |state| {
            reduce_component_action_at(state, storage, monotonic_now_ms, action)
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

pub(crate) fn reduce_component_action_at<S: KeyValueStorage>(
    state: &mut ClientState,
    storage: &S,
    monotonic_now_ms: u64,
    action: ComponentAction,
) -> ClientEffect {
    state.observe_carry_lock_time(monotonic_now_ms);
    match action {
        ComponentAction::EnableCarryLock => return state.enable_carry_lock(storage),
        ComponentAction::ArmCarryLock => return state.arm_carry_lock(monotonic_now_ms),
        ComponentAction::DisableCarryLock => return state.disable_carry_lock(storage),
        _ => {}
    }
    if is_carry_lock_mutation(&action) && !state.authorize_carry_lock_mutation() {
        return ClientEffect::None;
    }
    match action {
        ComponentAction::SwitchTab(tab) => state.switch_tab(tab),
        ComponentAction::Tick { wall_now_epoch_ms } => state.tick(wall_now_epoch_ms),
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
        ComponentAction::EnableCarryLock
        | ComponentAction::ArmCarryLock
        | ComponentAction::DisableCarryLock => ClientEffect::None,
    }
}

fn is_carry_lock_mutation(action: &ComponentAction) -> bool {
    matches!(
        action,
        ComponentAction::AutoSession
            | ComponentAction::AddSession(_)
            | ComponentAction::DiscardSession(_)
            | ComponentAction::RecordSession(_)
            | ComponentAction::CompleteSession(_)
            | ComponentAction::CompleteSessionWithoutRecording(_)
            | ComponentAction::ConfirmRepositoryChecked
    )
}
