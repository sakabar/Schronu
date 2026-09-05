use schronu_web::client::carry_lock::{
    load_carry_lock, CarryLockMode, CARRY_LOCK_STORAGE_KEY, CARRY_LOCK_WINDOW_MS,
};
use schronu_web::client::state::load_client_state;

mod client_state_support;
use client_state_support::FakeStorage;

#[test]
fn 保存値なしと正常schemaは互換性を保って復元する() {
    let storage = FakeStorage::default();
    assert_eq!(load_carry_lock(&storage).mode(), CarryLockMode::Normal);
    assert_eq!(CARRY_LOCK_WINDOW_MS, 15_000);

    *storage.carry_lock_value.borrow_mut() = Some(r#"{"version":1,"enabled":true}"#.to_owned());
    assert_eq!(load_carry_lock(&storage).mode(), CarryLockMode::Locked);

    *storage.carry_lock_value.borrow_mut() = Some(r#"{"version":1,"enabled":false}"#.to_owned());
    assert_eq!(load_carry_lock(&storage).mode(), CarryLockMode::Normal);
}

#[test]
fn 不正値とread失敗は元値を変更せずlockedとwarningへ倒す() {
    for raw in [
        "not-json",
        r#"{"version":2,"enabled":false}"#,
        r#"{"version":1}"#,
        r#"{"version":1,"enabled":"false"}"#,
        r#"{"version":1,"enabled":false,"extra":true}"#,
    ] {
        let storage = FakeStorage::default();
        *storage.carry_lock_value.borrow_mut() = Some(raw.to_owned());

        let state = load_carry_lock(&storage);

        assert_eq!(state.mode(), CarryLockMode::Locked, "raw: {raw}");
        assert!(state.warning().is_some(), "raw: {raw}");
        assert_eq!(storage.carry_lock_value.borrow().as_deref(), Some(raw));
    }

    let storage = FakeStorage::default();
    storage.fail_carry_lock_reads.set(true);
    let state = load_carry_lock(&storage);
    assert_eq!(state.mode(), CarryLockMode::Locked);
    assert!(state.warning().is_some());
}

#[test]
fn enableはmemory優先でdisableはstorage優先にする() {
    let storage = FakeStorage::default();
    let mut state = load_carry_lock(&storage);
    storage.fail_carry_lock_writes.set(true);

    state.enable(&storage);
    assert_eq!(state.mode(), CarryLockMode::Locked);
    assert!(state.warning().is_some());
    assert!(storage.carry_lock_value.borrow().is_none());

    state.disable(&storage);
    assert_eq!(state.mode(), CarryLockMode::Locked);
    assert!(state.warning().is_some());

    storage.fail_carry_lock_writes.set(false);
    state.enable(&storage);
    assert_eq!(
        storage.carry_lock_value.borrow().as_deref(),
        Some(r#"{"version":1,"enabled":true}"#)
    );
    state.arm(1_000);
    assert_eq!(state.mode(), CarryLockMode::ArmedUntil(16_000));
    assert_eq!(load_carry_lock(&storage).mode(), CarryLockMode::Locked);

    state.disable(&storage);
    assert_eq!(state.mode(), CarryLockMode::Normal);
    assert_eq!(
        storage.carry_lock_value.borrow().as_deref(),
        Some(r#"{"version":1,"enabled":false}"#)
    );
}

#[test]
fn carry_lock_warningは既存storage_warningと併せて公開する() {
    let storage = FakeStorage::default();
    storage.fail_carry_lock_reads.set(true);

    let state = load_client_state(&storage, 0).unwrap();

    assert_eq!(state.carry_lock_mode(), CarryLockMode::Locked);
    assert_eq!(state.all_storage_warnings().len(), 1);
    assert!(state.all_storage_warnings()[0].contains("持ち歩きロック"));
    assert_eq!(CARRY_LOCK_STORAGE_KEY, "schronu_web.carry_lock.v1");
}
