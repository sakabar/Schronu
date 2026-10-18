use crate::application::interface::TaskRepositoryTrait;
use crate::application::task_use_case::ApplicationError;
use crate::entity::task::TaskHandle;
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};
use uuid::Uuid;

const MY_ASCII_SET: &AsciiSet = &CONTROLS.add(b' ');
const OBSIDIAN_VAULT_ASCII_SET: &AsciiSet = &MY_ASCII_SET.add(b'&').add(b'=');

pub(crate) fn make_obsidian_search_url_with_vault(query: &str, vault_name: &str) -> String {
    format!(
        "obsidian://search?vault={}&query={}",
        percent_encode(vault_name.as_bytes(), OBSIDIAN_VAULT_ASCII_SET),
        percent_encode(query.as_bytes(), MY_ASCII_SET)
    )
}

pub(crate) fn make_obsidian_root_task_search_url_with_vault(
    focused_task: &TaskHandle,
    vault_name: &str,
) -> Result<String, ApplicationError> {
    let root_task_id = focused_task
        .root()
        .and_then(|root| root.get_id())
        .map_err(ApplicationError::TaskTree)?;
    Ok(make_obsidian_search_url_with_vault(
        &root_task_id.hyphenated().to_string(),
        vault_name,
    ))
}

pub(crate) fn resolve_obsidian_search_uri(
    repository: &dyn TaskRepositoryTrait,
    task_id: Uuid,
    vault_name: &str,
) -> Result<String, ApplicationError> {
    let task = repository
        .get_by_id(task_id)
        .map_err(ApplicationError::TaskTree)?
        .ok_or(ApplicationError::TaskNotFound(task_id))?;
    make_obsidian_root_task_search_url_with_vault(&task, vault_name)
}
