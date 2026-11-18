use super::super::{offline_storage, offline_sync, web_endpoint};
use crate::client::offline::OfflineState;
use crate::client::safety_state::load_mutation_safety;
use crate::client::work_sessions::{
    load_work_sessions, BrowserLocalStorage, KeyValueStorage, WorkSession,
};
use crate::{OfflineBundle, OfflineOperation};
use serde::{Deserialize, Serialize};

pub(super) const MODE_NOTICE: &str = "schronu_web.offline_mode_notice.v1";
const UNREACHABLE: &str = "サーバーに接続できません。接続を確認してから同期してください。";

#[derive(Clone)]
pub(super) struct Saved {
    pub token: String,
    pub data: Document,
}

#[derive(Serialize, Deserialize, Clone)]
pub(super) struct Document {
    pub state: OfflineState,
    initial_sessions: Vec<WorkSession>,
    pub returning: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "version", content = "data")]
enum Versioned<T> {
    #[serde(rename = "1")]
    V1(T),
}

pub(super) async fn read() -> Result<Option<Saved>, String> {
    let Some((token, raw)) = offline_storage::load().await? else {
        return Ok(None);
    };
    let Versioned::V1(data) = serde_json::from_str(&raw).map_err(|_| {
        "保存したオフラインデータを読み取れません。対応するバージョンで開いてください。".to_owned()
    })?;
    Ok(Some(Saved { token, data }))
}

async fn write(expected: Option<&str>, data: &Document) -> Result<String, String> {
    let raw = serde_json::to_string(&Versioned::V1(data))
        .map_err(|_| "オフラインの作業記録を保存する形式に変換できませんでした。".to_owned())?;
    offline_storage::save(expected, &raw).await
}

pub(super) fn notify_mode(mode: &str) {
    let _ = BrowserLocalStorage.set(MODE_NOTICE, mode);
}

async fn fetch_bundle() -> Result<OfflineBundle, String> {
    web_endpoint::prepare_offline()
        .await
        .map_err(|_| UNREACHABLE.to_owned())?
        .map_err(|error| error.message)
}

fn online_sessions() -> Result<Vec<WorkSession>, String> {
    let safety = load_mutation_safety(&BrowserLocalStorage)
        .map_err(|_| "セッションの保存状態を確認できません。".to_owned())?;
    if safety.mutation_blocked() {
        return Err("オンライン操作の保存結果を確認してから切り替えてください。".into());
    }
    let sessions = load_work_sessions(&BrowserLocalStorage)
        .map_err(|_| "作業中のセッションを読み取れません。".to_owned())?;
    if sessions.write_blocked() || sessions.needs_repair() {
        return Err(
            "作業中のセッションに読み取れない項目があります。保存状態を確認してください。".into(),
        );
    }
    Ok(sessions.sessions().to_vec())
}

pub(super) async fn prepare(sessions: Vec<WorkSession>) -> Result<Saved, String> {
    let bundle = fetch_bundle().await?;
    offline_storage::prepare_shell().await?;
    let data = Document {
        initial_sessions: sessions.clone(),
        state: OfflineState::new(bundle, sessions),
        returning: false,
    };
    let publication = async {
        if online_sessions()? != data.initial_sessions {
            return Err(
                "準備中にセッションが変更されました。一覧を開き直して再度準備してください。".into(),
            );
        }
        write(None, &data).await
    }
    .await;
    let token = match publication {
        Ok(token) => token,
        Err(error) => {
            let _ = offline_storage::cancel_preparation().await;
            return Err(error);
        }
    };
    notify_mode("offline");
    Ok(Saved { token, data })
}

impl Saved {
    pub async fn update(&mut self, state: OfflineState) -> Result<(), String> {
        self.data.state = state;
        self.token = write(Some(&self.token), &self.data).await?;
        Ok(())
    }

    pub async fn sync(&mut self) -> Result<usize, String> {
        let mut state = self.data.state.clone();
        let count = offline_sync::send_pending(&mut state, self).await?;
        // A successful final read both checks connectivity for an empty queue and
        // refreshes all seven dates and both load reports at the same observation time.
        let bundle = fetch_bundle().await?;
        state.rebase(bundle);
        self.data.state = state;
        if self.data.state.pending.is_empty() {
            self.merged_sessions()?;
            self.data.returning = true;
        }
        self.token = write(Some(&self.token), &self.data).await?;
        if self.data.returning {
            self.handoff().await?;
        }
        Ok(count)
    }

    fn merged_sessions(&self) -> Result<Vec<WorkSession>, String> {
        offline_sync::merge_sessions(
            &self.data.initial_sessions,
            &self.data.state.sessions,
            &online_sessions()?,
        )
    }

    /// The durable returning flag closes this version to other edits. If the tab
    /// exits between stores, the next mount repeats the same session handoff.
    pub async fn handoff(&self) -> Result<(), String> {
        let sessions = self.merged_sessions()?;
        let mut live = load_work_sessions(&BrowserLocalStorage)
            .map_err(|_| "オンライン画面のセッションを読み取れません。".to_owned())?;
        if live.sessions() != sessions {
            live.replace_sessions(&BrowserLocalStorage, sessions)
                .map_err(|_| {
                    "オンライン画面へセッションを引き継げません。端末の保存容量を確認してください。"
                        .to_owned()
                })?;
        }
        if let Err(error) = offline_storage::remove(&self.token).await {
            // Another tab can finish this same handoff after a restart.
            if read().await?.is_some() {
                return Err(error);
            }
        }
        notify_mode("online");
        Ok(())
    }
}

impl offline_sync::SyncIo for Saved {
    async fn push(&mut self, operation: OfflineOperation) -> Result<(), offline_sync::PushError> {
        web_endpoint::sync_offline_operation(operation)
            .await
            .map_err(|_| offline_sync::PushError::Unavailable(UNREACHABLE.to_owned()))?
            .map_err(Into::into)
    }

    async fn checkpoint(&mut self, state: &OfflineState) -> Result<(), String> {
        self.update(state.clone()).await
    }
}
