use super::work_sessions::{KeyValueStorage, StorageError};
use serde::{Deserialize, Serialize};

pub const CARRY_LOCK_STORAGE_KEY: &str = "schronu_web.carry_lock.v1";
pub const CARRY_LOCK_WINDOW_MS: i64 = 15_000;
const STORAGE_VERSION: u64 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CarryLockMode {
    Normal,
    Locked,
    ArmedUntil(i64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CarryLockState {
    mode: CarryLockMode,
    last_observed_epoch_ms: Option<i64>,
    warning: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredCarryLock {
    version: u64,
    enabled: bool,
}

pub fn load_carry_lock<S: KeyValueStorage>(storage: &S) -> CarryLockState {
    let raw = match storage.get(CARRY_LOCK_STORAGE_KEY) {
        Ok(Some(raw)) => raw,
        Ok(None) => return CarryLockState::normal(),
        Err(_) => {
            return CarryLockState::locked_with_warning(
                "持ち歩きロックの保存状態を読み取れません。安全のため操作をロックしました。",
            );
        }
    };
    let Ok(stored) = serde_json::from_str::<StoredCarryLock>(&raw) else {
        return CarryLockState::locked_with_warning(
            "持ち歩きロックの保存形式が不正です。localStorageを手動で確認してください。",
        );
    };
    if stored.version != STORAGE_VERSION {
        return CarryLockState::locked_with_warning(
            "持ち歩きロックのversionに対応していません。localStorageを手動で確認してください。",
        );
    }
    if stored.enabled {
        CarryLockState::locked()
    } else {
        CarryLockState::normal()
    }
}

impl CarryLockState {
    pub fn mode(&self) -> CarryLockMode {
        self.mode
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    pub fn enable<S: KeyValueStorage>(&mut self, storage: &S) {
        self.mode = CarryLockMode::Locked;
        self.last_observed_epoch_ms = None;
        match store_enabled(storage, true) {
            Ok(()) => self.warning = None,
            Err(_) => {
                self.warning = Some(
                    "持ち歩きロックを保存できません。再読み込み後はロックを維持できない可能性があります。"
                        .to_owned(),
                );
            }
        }
    }

    pub fn disable<S: KeyValueStorage>(&mut self, storage: &S) {
        match store_enabled(storage, false) {
            Ok(()) => {
                self.mode = CarryLockMode::Normal;
                self.last_observed_epoch_ms = None;
                self.warning = None;
            }
            Err(_) => {
                self.mode = CarryLockMode::Locked;
                self.last_observed_epoch_ms = None;
                self.warning = Some(
                    "持ち歩きロックの解除状態を保存できないため、操作ロックを維持しました。"
                        .to_owned(),
                );
            }
        }
    }

    pub fn arm(&mut self, now_epoch_ms: i64) {
        if self.mode == CarryLockMode::Locked {
            self.mode =
                CarryLockMode::ArmedUntil(now_epoch_ms.saturating_add(CARRY_LOCK_WINDOW_MS));
            self.last_observed_epoch_ms = Some(now_epoch_ms);
        }
    }

    pub(crate) fn expire(&mut self, now_epoch_ms: i64) {
        let CarryLockMode::ArmedUntil(deadline_epoch_ms) = self.mode else {
            return;
        };
        let clock_moved_back = self
            .last_observed_epoch_ms
            .is_some_and(|observed| now_epoch_ms < observed);
        if clock_moved_back || now_epoch_ms >= deadline_epoch_ms {
            self.mode = CarryLockMode::Locked;
            self.last_observed_epoch_ms = None;
        } else {
            self.last_observed_epoch_ms = Some(now_epoch_ms);
        }
    }

    #[cfg(any(test, all(feature = "web", target_arch = "wasm32")))]
    pub(crate) fn authorize_mutation(&mut self, now_epoch_ms: i64) -> bool {
        self.expire(now_epoch_ms);
        match self.mode {
            CarryLockMode::Normal => true,
            CarryLockMode::Locked => false,
            CarryLockMode::ArmedUntil(deadline_epoch_ms) => {
                self.mode = CarryLockMode::Locked;
                self.last_observed_epoch_ms = None;
                now_epoch_ms < deadline_epoch_ms
            }
        }
    }

    fn normal() -> Self {
        Self {
            mode: CarryLockMode::Normal,
            last_observed_epoch_ms: None,
            warning: None,
        }
    }

    fn locked() -> Self {
        Self {
            mode: CarryLockMode::Locked,
            last_observed_epoch_ms: None,
            warning: None,
        }
    }

    fn locked_with_warning(warning: &str) -> Self {
        Self {
            mode: CarryLockMode::Locked,
            last_observed_epoch_ms: None,
            warning: Some(warning.to_owned()),
        }
    }
}

fn store_enabled<S: KeyValueStorage>(storage: &S, enabled: bool) -> Result<(), StorageError> {
    let serialized = serde_json::to_string(&StoredCarryLock {
        version: STORAGE_VERSION,
        enabled,
    })
    .map_err(|_| StorageError::WriteFailed)?;
    storage.set(CARRY_LOCK_STORAGE_KEY, &serialized)
}
