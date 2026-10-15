use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Eq, PartialEq)]
enum LinkStatus {
    Checking(u64),
    Available(String),
    Missing,
    Failed(String),
}

#[derive(Default)]
pub(super) struct SessionLinks {
    statuses: HashMap<String, LinkStatus>,
    next_request_id: u64,
    was_active: bool,
}

impl SessionLinks {
    pub(super) fn reconcile(&mut self, active: bool, task_ids: &[String]) -> Vec<(String, u64)> {
        let visible: HashSet<&str> = task_ids.iter().map(String::as_str).collect();
        self.statuses
            .retain(|task_id, _| visible.contains(task_id.as_str()));
        let reentered = active && !self.was_active;
        self.was_active = active;
        if !active {
            return Vec::new();
        }

        let pending: Vec<_> = task_ids
            .iter()
            .filter(|task_id| reentered || !self.statuses.contains_key(task_id.as_str()))
            .cloned()
            .collect();
        pending
            .into_iter()
            .map(|task_id| {
                self.next_request_id += 1;
                let request_id = self.next_request_id;
                self.statuses
                    .insert(task_id.clone(), LinkStatus::Checking(request_id));
                (task_id, request_id)
            })
            .collect()
    }

    pub(super) fn apply_result(
        &mut self,
        task_id: &str,
        request_id: u64,
        result: Result<Option<String>, String>,
    ) {
        if !matches!(self.statuses.get(task_id), Some(LinkStatus::Checking(id)) if *id == request_id)
        {
            return;
        }
        let status = match result {
            Ok(Some(url)) if url.starts_with("https://") || url.starts_with("http://") => {
                LinkStatus::Available(url)
            }
            Ok(Some(_)) | Ok(None) => LinkStatus::Missing,
            Err(message) => LinkStatus::Failed(message),
        };
        self.statuses.insert(task_id.to_owned(), status);
    }

    pub(super) fn url_for(&self, task_id: &str) -> Option<&str> {
        match self.statuses.get(task_id) {
            Some(LinkStatus::Available(url)) => Some(url.as_str()),
            _ => None,
        }
    }

    pub(super) fn linkable_task_ids(&self) -> HashSet<String> {
        self.statuses
            .iter()
            .filter(|(_, status)| matches!(status, LinkStatus::Available(_)))
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub(super) fn errors(&self) -> Vec<String> {
        self.statuses
            .values()
            .filter_map(|status| match status {
                LinkStatus::Failed(message) => Some(message.clone()),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::SessionLinks;

    #[test]
    fn urlの有無と失敗で操作可否を切り替える() {
        let mut links = SessionLinks::default();
        let ids = ["direct", "inherited", "missing", "failed"].map(str::to_owned);
        let requests = links.reconcile(true, &ids);
        assert!(links.linkable_task_ids().is_empty());
        links.apply_result(
            "direct",
            requests[0].1,
            Ok(Some("https://example.com/direct".into())),
        );
        links.apply_result(
            "inherited",
            requests[1].1,
            Ok(Some("https://example.com/parent".into())),
        );
        links.apply_result("missing", requests[2].1, Ok(None));
        links.apply_result(
            "failed",
            requests[3].1,
            Err("リンクを取得できませんでした。".into()),
        );
        assert_eq!(
            links.linkable_task_ids(),
            ["direct".into(), "inherited".into()].into()
        );
        assert_eq!(links.url_for("direct"), Some("https://example.com/direct"));
        assert_eq!(
            links.url_for("inherited"),
            Some("https://example.com/parent")
        );
        assert_eq!(links.url_for("missing"), None);
        assert_eq!(links.url_for("failed"), None);
        assert_eq!(links.errors(), ["リンクを取得できませんでした。"]);
        assert!(links.reconcile(true, &ids).is_empty());
        links.reconcile(false, &ids);
        assert_eq!(links.reconcile(true, &ids).len(), ids.len());
        assert!(links.linkable_task_ids().is_empty());
    }

    #[test]
    fn 削除後の遅延応答を無視し再追加時に再確認する() {
        let mut links = SessionLinks::default();
        let first = links.reconcile(true, &["task".into()])[0].1;
        links.reconcile(true, &[]);
        let second = links.reconcile(true, &["task".into()])[0].1;
        links.apply_result("task", first, Ok(Some("https://example.com/old".into())));
        assert!(links.linkable_task_ids().is_empty());
        links.apply_result("task", second, Ok(Some("https://example.com/new".into())));
        assert!(links.linkable_task_ids().contains("task"));
    }

    #[test]
    fn urlが未確認または存在しなければ開く先を返さない() {
        let mut links = SessionLinks::default();
        let request_id = links.reconcile(true, &["task".into()])[0].1;
        assert_eq!(links.url_for("task"), None);
        links.apply_result("task", request_id, Ok(Some("https://example.com".into())));
        assert_eq!(links.url_for("task"), Some("https://example.com"));
        links.reconcile(true, &[]);
        assert_eq!(links.url_for("task"), None);
    }
}
