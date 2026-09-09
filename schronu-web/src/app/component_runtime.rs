use crate::client::date_input::DateInputState;
use crate::client::state::{load_client_state_for_ui, ActiveTab, ClientEffect, ClientState};
use crate::client::view_state::{load_view_state, store_view_state, StoredListView, ViewState};
use crate::client::work_sessions::KeyValueStorage;
use crate::SessionTask;

use super::effect_dispatcher::{apply_response, ClientResponse};
use super::session_view::{SessionAction, SessionActionKind};

pub(crate) enum ComponentAction {
    RetryRefresh,
    SwitchTab(ActiveTab),
    Tick { wall_now_epoch_ms: i64 },
    SelectDate(String),
    AutoSession,
    AddSession { task: SessionTask, is_leaf: bool },
    DiscardSession(String),
    RecordSession(String),
    CompleteSession(String),
    CompleteSessionWithoutRecording(String),
    ResumeCompletionConflict(String),
    ConfirmCompletionConflict(String),
    ConfirmRepositoryChecked,
    EnableCarryLock,
    ArmCarryLock,
    RelockCarryLock,
    DisableCarryLock,
}

pub(crate) fn component_action_from_date_input(
    date_input: &mut DateInputState,
    current_logical_date: &str,
) -> Option<ComponentAction> {
    date_input
        .submit(current_logical_date)
        .map(ComponentAction::SelectDate)
}

#[cfg(test)]
pub(crate) fn component_action_from_date_button(
    date_input: &mut DateInputState,
    logical_date: String,
) -> ComponentAction {
    date_input.clear();
    ComponentAction::SelectDate(logical_date)
}

pub(crate) fn reset_task_name_filter_after_session_add(
    task_name_filter: &mut String,
    previous_session_count: usize,
    current_session_count: usize,
) {
    if current_session_count > previous_session_count {
        task_name_filter.clear();
    }
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
    refresh_state: RefreshState,
    date_input: DateInputState,
    task_name_filter: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RefreshState {
    Bootstrap(u64),
    List(u64),
    Failed,
    Fresh,
}

impl ComponentOrchestrator {
    pub fn new() -> Self {
        Self {
            state: None,
            mounted: false,
            pending_server_effects: 0,
            refresh_state: RefreshState::Bootstrap(0),
            date_input: DateInputState::default(),
            task_name_filter: String::new(),
        }
    }

    pub fn state(&self) -> Option<&ClientState> {
        self.state.as_ref()
    }

    pub fn server_effect_in_flight(&self) -> bool {
        self.pending_server_effects > 0
    }

    pub fn background_refreshing(&self) -> bool {
        matches!(
            self.refresh_state,
            RefreshState::Bootstrap(_) | RefreshState::List(_)
        )
    }

    pub fn refresh_failed(&self) -> bool {
        self.refresh_state == RefreshState::Failed
    }

    pub fn server_actions_blocked(&self) -> bool {
        self.refresh_state != RefreshState::Fresh
    }

    pub fn date_input(&self) -> &DateInputState {
        &self.date_input
    }

    pub fn task_name_filter(&self) -> &str {
        &self.task_name_filter
    }

    pub fn edit_date_input<S: KeyValueStorage>(&mut self, storage: &S, text: String) {
        self.date_input.edit(text);
        self.persist_view_state(storage);
    }

    pub fn clear_date_input<S: KeyValueStorage>(&mut self, storage: &S) {
        self.date_input.clear();
        self.persist_view_state(storage);
    }

    pub fn submit_date_input<S: KeyValueStorage>(
        &mut self,
        storage: &S,
    ) -> Option<ComponentAction> {
        let current_logical_date = self
            .state()
            .and_then(ClientState::snapshot)
            .map(|snapshot| snapshot.logical_date.as_str())
            .map(str::to_owned)?;
        let action = component_action_from_date_input(&mut self.date_input, &current_logical_date);
        self.persist_view_state(storage);
        action
    }

    pub fn edit_task_name_filter<S: KeyValueStorage>(&mut self, storage: &S, text: String) {
        self.task_name_filter = text;
        self.persist_view_state(storage);
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
        let loaded_view_state = load_view_state(storage);
        let warning = loaded_view_state.warning().map(str::to_owned);
        let restored_view_state = loaded_view_state.into_state();
        let (mut state, effect) = initialize_client(storage, wall_now_epoch_ms);
        if let Some(view_state) = restored_view_state {
            self.date_input.edit(view_state.date_input_text.clone());
            self.task_name_filter = view_state.task_name_filter.clone();
            state.restore_view_state(&view_state);
        }
        state.set_view_state_warning(warning);
        self.state = Some(state);
        self.refresh_state = background_state_for_effect(&effect).unwrap_or(RefreshState::Failed);
        effect
    }

    pub fn action<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        monotonic_now_ms: u64,
        action: ComponentAction,
    ) -> ClientEffect {
        if matches!(action, ComponentAction::RetryRefresh) {
            if self.refresh_state != RefreshState::Failed {
                return ClientEffect::None;
            }
            let effect = self
                .state
                .as_mut()
                .map_or(ClientEffect::None, ClientState::request_bootstrap);
            self.refresh_state =
                background_state_for_effect(&effect).unwrap_or(RefreshState::Failed);
            return effect;
        }
        if self.server_actions_blocked() && action_requires_server(&action) {
            return ClientEffect::None;
        }
        let should_persist = !matches!(action, ComponentAction::Tick { .. });
        let Some(state) = self.state.as_mut() else {
            return ClientEffect::None;
        };
        let effect = reduce_component_action_at(state, storage, monotonic_now_ms, action);
        if should_persist {
            self.persist_view_state(storage);
        }
        effect
    }

    pub(crate) fn start_session_from_list<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        monotonic_now_ms: u64,
        task: SessionTask,
        is_leaf: bool,
    ) -> ClientEffect {
        let previous_session_count = self.state().map_or(0, |state| state.sessions().len());
        let effect = self.action(
            storage,
            monotonic_now_ms,
            ComponentAction::AddSession { task, is_leaf },
        );
        let current_session_count = self.state().map_or(0, |state| state.sessions().len());
        reset_task_name_filter_after_session_add(
            &mut self.task_name_filter,
            previous_session_count,
            current_session_count,
        );
        self.persist_view_state(storage);
        effect
    }

    pub fn apply_response<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        response: ClientResponse,
    ) -> ClientEffect {
        let refresh_result = match (&self.refresh_state, &response) {
            (
                RefreshState::Bootstrap(expected),
                ClientResponse::Bootstrap { request_id, result },
            ) if expected == request_id => Some(result.is_ok()),
            (
                RefreshState::List(expected),
                ClientResponse::ListTasks {
                    request_id, result, ..
                },
            ) if expected == request_id => Some(result.is_ok()),
            _ => None,
        };
        let effect = self.state.as_mut().map_or(ClientEffect::None, |state| {
            let previous_session_count = state.sessions().len();
            let effect = apply_response(state, storage, response);
            switch_to_list_after_last_session_removed(state, previous_session_count);
            effect
        });
        if let Some(succeeded) = refresh_result {
            self.refresh_state = if succeeded {
                if matches!(effect, ClientEffect::ListTasks { .. }) {
                    background_state_for_effect(&effect).unwrap_or(RefreshState::Failed)
                } else {
                    RefreshState::Fresh
                }
            } else {
                RefreshState::Failed
            };
        }
        self.persist_view_state(storage);
        effect
    }

    pub fn effect_is_background(&self, effect: &ClientEffect) -> bool {
        matches!(
            (self.refresh_state, effect),
            (RefreshState::Bootstrap(expected), ClientEffect::Bootstrap { request_id })
                if expected == *request_id
        ) || matches!(
            (self.refresh_state, effect),
            (RefreshState::List(expected), ClientEffect::ListTasks { request_id, .. })
                if expected == *request_id
        )
    }

    fn persist_view_state<S: KeyValueStorage>(&mut self, storage: &S) {
        let Some(state) = self.state.as_ref() else {
            return;
        };
        let Some(snapshot) = state.snapshot().cloned() else {
            return;
        };
        let list = if state.has_scheduled_list() {
            state
                .selected_logical_date()
                .map(|logical_date| StoredListView {
                    logical_date: logical_date.to_owned(),
                    rows: state.scheduled_rows().to_vec(),
                })
        } else {
            None
        };
        let view_state = ViewState {
            snapshot,
            list,
            active_tab: state.active_tab(),
            task_name_filter: self.task_name_filter.clone(),
            date_input_text: self.date_input.text().to_owned(),
        };
        let warning = store_view_state(storage, &view_state)
            .err()
            .map(|_| "前回の画面状態を保存できませんでした。".to_owned());
        if let Some(state) = self.state.as_mut() {
            state.set_view_state_warning(warning);
        }
    }
}

fn background_state_for_effect(effect: &ClientEffect) -> Option<RefreshState> {
    match effect {
        ClientEffect::Bootstrap { request_id } => Some(RefreshState::Bootstrap(*request_id)),
        ClientEffect::ListTasks { request_id, .. } => Some(RefreshState::List(*request_id)),
        _ => None,
    }
}

fn action_requires_server(action: &ComponentAction) -> bool {
    matches!(
        action,
        ComponentAction::SelectDate(_)
            | ComponentAction::AutoSession
            | ComponentAction::DiscardSession(_)
            | ComponentAction::RecordSession(_)
            | ComponentAction::CompleteSession(_)
            | ComponentAction::CompleteSessionWithoutRecording(_)
            | ComponentAction::ConfirmCompletionConflict(_)
    )
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
        ComponentAction::RelockCarryLock => return state.relock_carry_lock(),
        ComponentAction::DisableCarryLock => return state.disable_carry_lock(storage),
        _ => {}
    }
    if is_carry_lock_mutation(&action) && !state.authorize_carry_lock_mutation(monotonic_now_ms) {
        return ClientEffect::None;
    }
    let previous_session_count = state.sessions().len();
    let effect = match action {
        ComponentAction::RetryRefresh => ClientEffect::None,
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
        | ComponentAction::RelockCarryLock
        | ComponentAction::DisableCarryLock => ClientEffect::None,
    };
    switch_to_list_after_last_session_removed(state, previous_session_count);
    effect
}

fn switch_to_list_after_last_session_removed(
    state: &mut ClientState,
    previous_session_count: usize,
) {
    if state.active_tab() == ActiveTab::Session
        && previous_session_count > state.sessions().len()
        && state.sessions().is_empty()
    {
        state.switch_tab(ActiveTab::List);
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
