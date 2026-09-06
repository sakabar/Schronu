use crate::client::state::{load_client_state_for_ui, ActiveTab, ClientEffect, ClientState};
use crate::client::work_sessions::KeyValueStorage;
use crate::SessionTask;

use super::effect_dispatcher::{apply_response, ClientResponse};
use super::session_view::{SessionAction, SessionActionKind};

pub(crate) enum ComponentAction {
    SwitchTab(ActiveTab),
    Tick {
        wall_now_epoch_ms: i64,
    },
    SelectDate(String),
    AutoSession,
    AddSession {
        task: SessionTask,
        is_leaf: bool,
    },
    DiscardSession(String),
    RecordSession(String),
    CompleteSession(String),
    CompleteSessionWithoutRecording(String),
    ResumeCompletionConflict(String),
    ConfirmCompletionConflict(String),
    ConfirmRepositoryChecked,
    EnableCarryLock,
    ArmCarryLock,
    DisableCarryLock,
    #[cfg_attr(not(all(feature = "web", target_arch = "wasm32")), allow(dead_code))]
    RelockCarryLock,
}

pub(crate) fn component_action_from_session_action(action: SessionAction) -> ComponentAction {
    match action.kind {
        SessionActionKind::Discard => ComponentAction::DiscardSession(action.task_id),
        SessionActionKind::Record => ComponentAction::RecordSession(action.task_id),
        SessionActionKind::Complete => ComponentAction::CompleteSession(action.task_id),
        SessionActionKind::CompleteWithoutRecording => {
            ComponentAction::CompleteSessionWithoutRecording(action.task_id)
        }
        SessionActionKind::ResumeCompletionConflict => {
            ComponentAction::ResumeCompletionConflict(action.task_id)
        }
        SessionActionKind::ConfirmCompletionConflict => {
            ComponentAction::ConfirmCompletionConflict(action.task_id)
        }
    }
}

pub(crate) fn component_actions_from_session_action(
    action: SessionAction,
    ended_at_epoch_ms: i64,
) -> Vec<ComponentAction> {
    let stops_session = matches!(
        action.kind,
        SessionActionKind::Record
            | SessionActionKind::Complete
            | SessionActionKind::CompleteWithoutRecording
            | SessionActionKind::ResumeCompletionConflict
    );
    let mutation = component_action_from_session_action(action);
    if stops_session {
        vec![
            ComponentAction::Tick {
                wall_now_epoch_ms: ended_at_epoch_ms,
            },
            mutation,
        ]
    } else {
        vec![mutation]
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
    pending_server_effects: usize,
}

impl ComponentOrchestrator {
    pub fn new() -> Self {
        Self {
            state: None,
            mounted: false,
            pending_server_effects: 0,
        }
    }

    pub fn state(&self) -> Option<&ClientState> {
        self.state.as_ref()
    }

    pub fn server_effect_in_flight(&self) -> bool {
        self.pending_server_effects > 0
    }

    pub fn begin_server_effect(&mut self) {
        self.pending_server_effects = self
            .pending_server_effects
            .checked_add(1)
            .expect("pending server effect count must not overflow");
    }

    pub fn finish_server_effect(&mut self) {
        self.pending_server_effects = self
            .pending_server_effects
            .checked_sub(1)
            .expect("a pending server effect must exist before it finishes");
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
        ComponentAction::RelockCarryLock => return state.relock_carry_lock(),
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
        ComponentAction::AddSession { task, is_leaf } => {
            let session_count = state.sessions().len();
            let effect = state.add_session_from_list_task(storage, &task, is_leaf);
            if state.sessions().len() > session_count {
                state.switch_tab(ActiveTab::Session);
            }
            effect
        }
        ComponentAction::DiscardSession(task_id) => state.discard_session(storage, &task_id),
        ComponentAction::RecordSession(task_id) => state.begin_record_session(storage, &task_id),
        ComponentAction::CompleteSession(task_id) => {
            state.begin_complete_session(storage, &task_id)
        }
        ComponentAction::CompleteSessionWithoutRecording(task_id) => {
            state.begin_complete_session_without_recording(storage, &task_id)
        }
        ComponentAction::ResumeCompletionConflict(task_id) => {
            state.resume_completion_conflict(storage, &task_id)
        }
        ComponentAction::ConfirmCompletionConflict(task_id) => {
            state.confirm_completion_conflict(storage, &task_id)
        }
        ComponentAction::ConfirmRepositoryChecked => state.confirm_repository_checked(storage),
        ComponentAction::EnableCarryLock
        | ComponentAction::ArmCarryLock
        | ComponentAction::DisableCarryLock => ClientEffect::None,
        ComponentAction::RelockCarryLock => ClientEffect::None,
    }
}

fn is_carry_lock_mutation(action: &ComponentAction) -> bool {
    matches!(
        action,
        ComponentAction::AutoSession
            | ComponentAction::AddSession { .. }
            | ComponentAction::DiscardSession(_)
            | ComponentAction::RecordSession(_)
            | ComponentAction::CompleteSession(_)
            | ComponentAction::CompleteSessionWithoutRecording(_)
            | ComponentAction::ResumeCompletionConflict(_)
            | ComponentAction::ConfirmCompletionConflict(_)
            | ComponentAction::ConfirmRepositoryChecked
    )
}
