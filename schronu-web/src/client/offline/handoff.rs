use super::OfflineState;

impl OfflineState {
    pub fn effective_leaf(&self, id: &str, saved_leaf: bool) -> bool {
        self.bundle
            .handoff_children
            .get(id)
            .map_or(saved_leaf, |children| {
                !children.is_empty()
                    && children
                        .iter()
                        .all(|child| self.completed(child) && !self.needs_review(child))
            })
    }

    pub(super) fn start_parent_after_completion(&mut self, child: &str, now: i64) {
        let Some((parent, children)) = self
            .bundle
            .handoff_children
            .iter()
            .find(|(_, children)| children.contains(child))
        else {
            return;
        };
        if !self.effective_leaf(parent, false) {
            return;
        }
        let dependencies = self
            .pending
            .iter()
            .filter(|op| op.kind.is_completion() && children.contains(&op.task_id))
            .map(|op| op.task_id.clone())
            .collect();
        let parent = parent.clone();
        self.unconfirmed_handoffs
            .insert(parent.clone(), dependencies);
        self.start_if_ready(&parent, now);
    }

    pub fn confirm_handoff_child(&mut self, child: &str) {
        self.unconfirmed_handoffs.retain(|_, children| {
            children.remove(child);
            !children.is_empty()
        });
    }

    /// Keep dependent work for manual reconciliation even if a later bundle moves
    /// a task outside the saved dates. Propagate through multiple local handoffs.
    pub fn mark_sync_conflict(&mut self, task: String, reason: String) {
        self.conflicts.insert(task.clone(), reason);
        let mut blocked = vec![task];
        while let Some(child) = blocked.pop() {
            for (parent, children) in &self.unconfirmed_handoffs {
                if children.contains(&child)
                    && !self.conflicts.contains_key(parent)
                    && (self.sessions.iter().any(|s| &s.task_id == parent)
                        || self.pending.iter().any(|op| &op.task_id == parent))
                {
                    let name = self.review_task_name(&child).unwrap_or(&child);
                    self.conflicts.insert(
                        parent.clone(),
                        format!("子タスク「{name}」の完了結果を確認してください。"),
                    );
                    blocked.push(parent.clone());
                }
            }
        }
    }
}
