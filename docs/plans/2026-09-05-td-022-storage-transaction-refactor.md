# TD-022 Storage Transaction Refactoring Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** TD-022で追加したstorage transactionの機能と互換性を維持したまま、責務を分離し、branch全体を保守可能な構造へ整理する。

**Architecture:** 現在の`storage_transaction.rs`をprivate facadeと責務別moduleへ分割する。manifest検証、path layout、filesystem I/O、prepare、commit、recovery、cleanupの境界を明確にし、marker前後とmanifest検証済み状態を内部型で表す。testは既存scenarioをcharacterizationとして維持しつつ、責務別moduleと共通failure-injection harnessへ整理する。

**Tech Stack:** Rust、Cargo、serde/serde_json、fs2、標準libraryのfilesystem API、標準Rust test framework。

## Summary

PR #412の現branch `feature/td-022-repository-transaction`へ、既存履歴を書き換えずにリファクタリングcommitを追加する。同一PR上で作業し、push、PR作成、mainへのmergeは行わない。

公開API、manifest v1、disk上のfile・directory構成、error情報、crash recovery、性能特性を維持する。既存の32件のtransaction testと18件のrepository経路testをcharacterizationとして扱い、挙動変更を含めない。

すべての新規fileと大幅拡張fileを800行以下にする。各Green commitで対象testと全品質gateを実行し、commit後に内部subagent reviewを行う。review指摘は1件1commitで修正し、対象testと再reviewを行う。

## Target Architecture

製品コードは次の責務へ分離する。

```text
storage_transaction.rs
└─ private facade、error、gateway向け再公開、protocol概要

storage_transaction/
├─ layout.rs       transaction内のpath生成とstorage相対path検証
├─ manifest.rs     manifest v1 schema、serialize、decode、意味検証、checksum
├─ io.rs           filesystem I/O、advisory lock、sync、Unix境界
├─ prepare.rs      staging、manifest永続化、marker前の処理
├─ commit.rs       marker公開、preflight、target適用、revision更新
├─ recovery.rs     marker判定、破棄、committed transaction復元
└─ cleanup.rs      tombstoneへのhandoffとbest-effort再清掃
```

内部状態は次の型で区別する。

- `RawTransactionManifest`: 現行JSON schemaをそのまま表現する。
- `ValidatedManifest`: version、path、entry整合性を検証済みであることを表す。
- `ValidatedEntry::Write { target, staged_file, integrity }`
- `ValidatedEntry::Delete { target }`
- `PreparedTransaction`: marker公開前だけを表現する。
- `CommittedTransaction`: marker公開済みでroll-forwardだけを許可する。
- `EntryDisposition`: `AlreadyApplied`、`Write`、`Delete`を表す。

製品コード内の検証済みfieldに対する`Option + expect`と`allow(clippy::too_many_arguments)`を除去する。

全filesystem syscallを`io.rs`へ集約する。`StorageTransactionIo`はprivateのまま維持し、directory列挙とmetadata確認も同じ境界を通す。新しい依存、unsafe、公開traitは追加しない。

## Commit Plan

新しい機能契約を導入しないためRed commitは作らず、既存testと追加するmanifest byte characterizationを回帰検証に使う。すべてのcommitはGreen状態で作成する。

### Task 1: 計画を記録する

**Commit:** `Docs: transaction refactor計画を記録する`

- 固定する契約: 本計画、非対象、完了条件をrepositoryへ保存する。
- 変更対象: `docs/plans/2026-09-05-td-022-storage-transaction-refactor.md`
- 依存: `5dcf0fa`
- 対象test: なし。
- Green確認: `git diff --check`

### Task 2: manifest v1の保存bytesを固定する

**Commit:** `Test: manifest v1の保存bytesを固定する`

- 固定する契約: write/deleteのfield名、順序、省略規則、checksum表現をbyte単位でcharacterizationする。
- 変更対象: transaction manifest test。
- 依存: Task 1。
- 対象test: manifest serialization test。
- Green確認: 対象testと全品質gate。

### Task 3: manifest責務を分離する

**Commit:** `Repository: transaction manifestを分離する`

- 固定する契約: raw schema、checksum、decode・validation helperの挙動を維持する。
- 変更対象: `manifest.rs`への機械的移動。
- 依存: Task 2。
- 対象test: manifest/recovery test。
- Green確認: 対象testと全品質gate。

### Task 4: filesystem境界を分離する

**Commit:** `Repository: transaction filesystem境界を分離する`

- 固定する契約: I/O trait、実filesystem、lock、syncの挙動を維持する。
- 変更対象: `io.rs`への機械的移動。
- 依存: Task 3。
- 対象test: lock、failure、symlink test。
- Green確認: 対象testと全品質gate。

### Task 5: path責務を分離する

**Commit:** `Repository: transaction path責務を分離する`

- 固定する契約: layout定数、path生成、storage相対path検証を維持する。
- 変更対象: `layout.rs`への機械的移動。
- 依存: Task 4。
- 対象test: path escape、reserved namespace test。
- Green確認: 対象testと全品質gate。

### Task 6: prepare責務を分離する

**Commit:** `Repository: transaction prepare責務を分離する`

- 固定する契約: stagingとmanifest永続化の順序・失敗挙動を維持する。
- 変更対象: `prepare.rs`への機械的移動。
- 依存: Task 5。
- 対象test: prepare failure/order test。
- Green確認: 対象testと全品質gate。

### Task 7: committed snapshot適用を分離する

**Commit:** `Repository: committed snapshot適用を分離する`

- 固定する契約: marker、preflight、atomic replace、revision-lastを維持する。
- 変更対象: `commit.rs`への機械的移動。
- 依存: Task 6。
- 対象test: commit order/failure test。
- Green確認: 対象testと全品質gate。

### Task 8: recoveryを分離する

**Commit:** `Repository: transaction recoveryを分離する`

- 固定する契約: marker判定とroll-forwardを維持する。
- 変更対象: `recovery.rs`への機械的移動。
- 依存: Task 7。
- 対象test: recovery test。
- Green確認: 対象testと全品質gate。

### Task 9: cleanupを分離する

**Commit:** `Repository: transaction cleanupを分離する`

- 固定する契約: tombstoneへのhandoffとbest-effort再清掃を維持する。
- 変更対象: `cleanup.rs`への機械的移動。
- 依存: Task 8。
- 対象test: cleanup test。
- Green確認: 対象testと全品質gate。

### Task 10: transaction testを責務別moduleへ分ける

**Commit:** `Test: transaction contractを責務別moduleへ分ける`

- 固定する契約: 既存32 scenarioとすべてのassertion、failure pointを維持する。
- 変更対象: manifest、prepare、commit、recovery、delete、security testへの機械的分割。
- 依存: Task 9。
- 対象test: transaction test全33 scenario(既存32件とmanifest保存bytes characterization 1件)。
- Green確認: 移動前後で全32 scenario Green、全品質gate。

### Task 11: repository transaction testを分離する

**Commit:** `Test: repository transaction契約を専用moduleへ分ける`

- 固定する契約: TD-022由来18 scenarioとすべてのassertion、failure pointを維持する。
- 変更対象: save、recovery、support testへの機械的分割。
- 依存: Task 10。
- 対象test: repository経路test全18 scenario。
- Green確認: 移動前後で全18 scenario Green、全品質gate。

### Task 12: transaction layoutを一元化する

**Commit:** `Repository: transaction layoutを一元化する`

- 固定する契約: `.active`、manifest、marker、staged、cleanup、revision pathを従来と同じdisk layoutで生成する。
- 変更対象: `TransactionLayout`。
- 依存: Task 11。
- 対象test: transaction/repository test全件。
- Green確認: 対象testと全品質gate。

### Task 13: manifest検証済み状態を導入する

**Commit:** `Repository: manifest検証済み状態を導入する`

- 固定する契約: raw manifestを一度だけ検証し、write/deleteをdiscriminatedなvalidated entryとして扱う。
- 変更対象: manifest decode・validationと利用側。
- 依存: Task 12。
- 対象test: malformed、integrity、delete test。
- Green確認: 対象testと全品質gate。検証済みentryに対する`expect`が0件。

### Task 14: marker前後の状態を型で分離する

**Commit:** `Repository: marker前後の状態を型で分離する`

- 固定する契約: `PreparedTransaction::commit()`がmarkerを公開した後、`CommittedTransaction::roll_forward()`だけがsnapshotを適用する。
- 変更対象: transaction内部状態とcommit flow。
- 依存: Task 13。
- 対象test: marker、crash、revision-last test。
- Green確認: 対象testと全品質gate。

### Task 15: transaction I/O経路を統一する

**Commit:** `Repository: transaction I/O経路を統一する`

- 固定する契約: metadata、directory列挙を含む全filesystem操作をI/O境界経由にし、挙動とerror情報を維持する。
- 変更対象: `io.rs`と各transaction moduleのI/O呼び出し。
- 依存: Task 14。
- 対象test: I/O failure、cleanup、symlink test。
- Green確認: 対象testと全品質gate。raw filesystem syscallが`io.rs`以外のtransaction製品moduleに0件。

### Task 16: prepare failure injectionを共通化する

**Commit:** `Test: prepare failure injectionを共通化する`

- 固定する契約: prepare時のfailure pointと記録内容を維持する。
- 変更対象: operation、path matcher、発生回数で故障させる`RecordingIo`。
- 依存: Task 15。
- 対象test: prepare test全件。
- Green確認: 対象testと全品質gate。

### Task 17: commit/recovery failure injectionを共通化する

**Commit:** `Test: commit recovery failure injectionを共通化する`

- 固定する契約: commit order、crash、delete、cleanupのfailure pointと記録内容を維持する。
- 変更対象: event logとfault ruleを使う共通harness。barrier使用mockだけは専用で残す。
- 依存: Task 16。
- 対象test: commit/recovery/delete test全件。
- Green確認: 対象testと全品質gate。

### Task 18: repository transaction fixtureを共通化する

**Commit:** `Test: repository transaction fixtureを共通化する`

- 固定する契約: repository製品経路の既存failure scenarioを維持する。
- 変更対象: repository側5種のI/O mock、snapshot作成・検証helper。
- 依存: Task 17。
- 対象test: TD-022 repository test全18件。
- Green確認: 対象testと全品質gate。

### Task 19: TD-022の保守性検証を記録する

**Commit:** `Docs: TD-022の保守性検証を記録する`

- 固定する契約: module責務、累積差分、file行数、test維持、残存非対象を記録する。
- 変更対象: `backlog.md`。
- 依存: Task 18。
- 対象test: final gateと最終履歴review。
- Green確認: 全品質gate、正しさreview、保守性review、clean status。

## Per-Commit Workflow

機械的移動commitではsymbol名、型構造、処理順序を変更しない。型・I/O設計の変更はTask 12以降へ分離する。

各Green commitで次の順序を守る。

1. 対象testを実行する。
2. 全品質gateを実行する。
3. `git diff --check`を実行する。
4. 基礎Green実装をcommitする。
5. `git status --short --branch`と`git show --stat --oneline HEAD`を確認する。
6. 内部subagent reviewを実施する。
7. 指摘がある場合は1件ずつ修正し、対象test、個別commit、再reviewを行う。
8. 全品質gateを再実行する。

全品質gateは次のとおり。

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
git diff --check
```

## Final Acceptance

- transaction test 33件(既存32件とmanifest保存bytes characterization 1件)とrepository経路test 18件の全scenario、assertion、failure pointが維持されている。
- markerだけがcommit pointであり、marker前は旧snapshot、marker後はnew snapshotへroll-forwardする。
- 全entryのpreflight後にlive変更し、`.revision`を最後に更新する。
- manifest v1のJSON表現とdisk上のfile・directory構成が変わっていない。
- error operation、path、source chainが維持されている。
- static symlink拒否、advisory lock、permission維持、private delete、cleanup retryが維持されている。
- `storage_transaction.rs`と配下の全製品・test file、および新規repository transaction test fileが各800行以下である。
- transaction製品コードで`allow(clippy::too_many_arguments)`が0件である。
- 検証済みentryに対する`expect`が0件である。
- raw filesystem syscallが`io.rs`以外のtransaction製品moduleにない。
- 専用failure mockはmarker競合用barrier mockだけであり、残りは共通fault harnessを使う。
- `main...HEAD`の累積差分について、正しさreviewと保守性reviewを別々に完了する。
- working treeがcleanである。

## Interfaces and Constraints

- 公開API、CLI、MCP、Spreadsheet、application/entity層は変更しない。
- `TaskRepository`から見えるprivate transaction入口を維持し、`PreparedTransaction::commit(&revision_path)`だけをprivateな`commit()`へ整理する。
- manifest version、field名、checksum algorithm、target適用順、memoryへ全staged bytesを保持するpreflight方式を維持する。
- 重複targetの新規拒否、memory最適化、Windows対応、checksum変更、symlink TOCTOU完全耐性、製品delete APIは対象外とする。
- 既存履歴を保持し、force-pushしない。
- local commit完了後の通常pushは明示指示を受けてから行い、mainへmergeしない。
