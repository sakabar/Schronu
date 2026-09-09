use super::*;

impl ClientState {
    /// Removes a local session without a server journal mutation.
    ///
    /// The product UI uses `begin_discard_session`; this operation remains for
    /// callers that explicitly manage only the local session cache.
    pub fn discard_session<S: KeyValueStorage>(
        &mut self,
        storage: &S,
        task_id: &str,
    ) -> ClientEffect {
        if self.sessions.in_flight_task_ids.contains(task_id)
            || self.sessions.committed_blocked_task_ids.contains(task_id)
        {
            return ClientEffect::None;
        }
        let candidate: Vec<_> = self
            .sessions()
            .iter()
            .filter(|session| session.task_id != task_id)
            .cloned()
            .collect();
        if candidate.len() == self.sessions().len() {
            return ClientEffect::None;
        }
        let result = self
            .sessions
            .work_sessions
            .replace_sessions(storage, candidate);
        if result.is_ok() {
            self.sessions.manual_check_blocked_task_ids.remove(task_id);
            self.sessions.completion_conflicts.remove(task_id);
            self.sessions.uncertain_stopped_at_epoch_ms.remove(task_id);
        }
        self.record_local_result(Some(task_id), result.is_ok());
        if result.is_ok() {
            self.request_selected_or_current_list()
        } else {
            ClientEffect::None
        }
    }
}
