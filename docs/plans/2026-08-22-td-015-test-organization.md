# TD-015 Test Organization Implementation Plan

**Goal:** 巨大な製品moduleからtestを挙動変更なしで分離し、同じ目的のtest fixtureを共通化する。

**Architecture:** 既存のinline `mod tests`は外部fileを指す同名moduleへ、top-level testは`include!`先へ機械的に移す。機械的移動とfixtureの意味的な共通化を別commitにし、製品APIと既存のtest名・assertion・fixture値を維持する。

**Tech Stack:** Rust、Cargo標準test、rustfmt、Clippy

---

## Baseline

- lib: 501 passed、1 ignored
- CLI binary: 333 passed
- MCP binary: 2 passed
- MCP stdio: 12 passed
- Spreadsheet: 4 passed
- 合計: 852 passed、1 ignored

新しい製品契約は導入しない。既存suiteをcharacterization contractとして使用し、Red test commitは作らない。

## Commit sequence

| 順序 | Commit message | 固定する契約・対象module | 依存 | 対象test |
| --- | --- | --- | --- | --- |
| 1 | `Docs: TD-015の実装計画を記録する` | baseline、対象file、commit境界を記録 | なし | full suite |
| 2 | `Test: Task単体testを別fileへ移す` | `entity/task.rs`の116 testを`task_tests.rs`へ機械的移動 | 1 | `cargo test --locked --lib entity::task` |
| 3 | `Test: CLI runtime単体testを別fileへ移す` | `runtime::tests`を`runtime_unit_tests.rs`へ移動 | 1 | `cargo test --locked --bin schronu runtime::tests::` |
| 4 | `Test: CLI runtime契約testを別fileへ移す` | top-level runtime testを`runtime_contract_tests.rs`へ移動 | 3 | `cargo test --locked --bin schronu runtime::` |
| 5 | `Test: CLI runtime fixtureをtest supportへ移す` | repository、writer、storage、free-time fakeを`runtime_test_support.rs`へ移動 | 4 | CLI runtime全test |
| 6 | `Test: Task use case testを別fileへ移す` | `task_use_case.rs`のinline test moduleを外部化 | 1 | `application::task_use_case::tests` |
| 7 | `Test: Pack use case testを別fileへ移す` | `pack_use_case.rs`のinline test moduleを外部化 | 1 | `application::pack_use_case::tests` |
| 8 | `Test: Schedule use case testを別fileへ移す` | `schedule_use_case.rs`のinline test moduleを外部化 | 1 | `application::schedule_use_case::tests` |
| 9 | `Test: Daily capacity testを別fileへ移す` | `daily_capacity.rs`のinline test moduleを外部化 | 1 | `application::daily_capacity::tests` |
| 10 | `Test: Task repository testを別fileへ移す` | repository testを`task_repository_tests.rs`へ移動 | 1 | `adapter::gateway::task_repository::tests` |
| 11 | `Test: YAML gateway testを別fileへ移す` | YAML testとtest専用helperを`yaml_tests.rs`へ移動 | 1 | `adapter::gateway::yaml` |
| 12 | `Test: Free time gateway testを別fileへ移す` | free-time testとtest専用helperを`free_time_manager_tests.rs`へ移動 | 1 | `adapter::gateway::free_time_manager` |
| 13 | `Test: MCP input単体testを別fileへ移す` | `mcp/input.rs`のunit testを`input_tests.rs`へ移動 | 1 | `adapter::mcp::input::tests` |
| 14 | `Test: MCP handler単体testを別fileへ移す` | `mcp/handler.rs`のunit testを`handler_tests.rs`へ移動 | 1 | `adapter::mcp::handler::tests` |
| 15 | `Test: Task fixture生成を共通化する` | entityはlibraryの`crate::test_support`へ、CLIはbinary privateな`runtime_test_support`へUUID・Task builderを統合 | 2、5 | entity、CLI runtime |
| 16 | `Test: List tasks repository fixtureを共通化する` | list contractのrepository stubを共通fixtureへ置換 | 1 | `application::list_tasks_contract_tests` |
| 17 | `Test: Schedule repository fixtureを共通化する` | project参照・save回数を共通fixtureで検証 | 16 | `application::schedule_use_case_contract_tests` |
| 18 | `Test: Task use case repository fixtureを共通化する` | focus候補、project状態、save回数を共通fixtureへ統合 | 6、17 | `application::task_use_case::tests` |
| 19 | `Test: CLI free-time fixtureを共通化する` | no-op/free-minutes用途のfakeを設定可能な1型へ統合 | 5 | CLI runtime全test |
| 20 | `Docs: test責務とfixture配置を説明する` | READMEへtest責務とfixture配置を記載 | 15-19 | full suite |
| 21 | `Docs: TD-015の完了を記録する` | backlogへ完了日、件数、品質ゲート、残存範囲を記録 | 20 | full suite |

機械的移動commitではtest関数名、assertion、fixture値、module pathを変更しない。fixture共通化commitでは製品向け公開APIを追加せず、test専用APIだけを`cfg(test)`配下へ置く。

対象外は、製品読解を妨げていない`datetime`、`storage_lock`、`schronu_config`、`interactive`、testが1件だけの`flatten_use_case`とする。

## Test-only interfaces

製品の公開interfaceは変更しない。library crateの`crate::test_support::TestTaskRepository`にはproject・save回数・highest-priority leaf IDを操作するtest専用APIだけを追加する。CLIは別binary crateであるためlibraryの`cfg(test)` moduleを公開せず、binary privateな`runtime_test_support`へUUID・Task builder、recording repository、writer、temporary storage、設定可能なfree-time fakeを置く。異なる失敗契約を表すfakeは別型のまま維持する。

## Verification

各Green commitで表の対象test、`git diff --check`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`を実行する。commit後に差分量と履歴を確認し、subagent reviewの指摘は1件ずつ独立commitで修正する。

最終的にbaselineのtest件数、`benchmark_save_2172project中1件変更を2秒未満で処理する`のignored指定、CLI・YAML・MCP・Spreadsheet契約を維持する。対象製品fileにはtest bodyとrepository/free-time stubを残さず、test専用分岐や製品向け公開APIを追加しない。
