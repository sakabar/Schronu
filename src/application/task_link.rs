use crate::application::interface::TaskRepositoryTrait;
use crate::application::task_use_case::ApplicationError;
use crate::entity::task::TaskHandle;
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};
use url::Url;
use uuid::Uuid;

const URL_ASCII_SET: &AsciiSet = &CONTROLS.add(b' ');

pub(crate) fn resolve_task_link_by_id(
    repository: &dyn TaskRepositoryTrait,
    task_id: Uuid,
) -> Result<Option<String>, ApplicationError> {
    let task = repository
        .get_by_id(task_id)
        .map_err(ApplicationError::TaskTree)?
        .ok_or(ApplicationError::TaskNotFound(task_id))?;
    resolve_task_link(&task)
}

pub(crate) fn resolve_task_link(task: &TaskHandle) -> Result<Option<String>, ApplicationError> {
    let mut task_opt = Some(task.clone());
    while let Some(task) = task_opt {
        let name = task.get_name().map_err(ApplicationError::TaskTree)?;
        if let Some(url) = extract_url(&name) {
            return Ok(Some(url));
        }
        task_opt = task.parent().map_err(ApplicationError::TaskTree)?;
    }
    Ok(None)
}

// タスク名からURLを抽出する規則はCLIとWebで共通にする。
pub(crate) fn extract_url(text: &str) -> Option<String> {
    let start = text.find("http")?;
    let chars: Vec<char> = (text[start..].to_owned() + "あ").chars().collect();
    let (mut ok, mut ng) = (0, chars.len());
    while ng - ok > 1 {
        let mid = (ok + ng) / 2;
        let candidate: String = chars[..mid].iter().collect();
        let encoded = percent_encode(candidate.as_bytes(), URL_ASCII_SET).to_string();
        if Url::parse(&candidate).is_ok() && candidate == encoded {
            ok = mid;
        } else {
            ng = mid;
        }
    }
    Some(chars[..ok].iter().collect())
}
