# TD-018 Test Integrity Remediation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** TD-018の独立reviewで見つかったtest専用Task list icon経路と、製品経路から切断された表示assertionを、製品契約を変えずに是正する。

**Architecture:** Task listのtest fixtureも製品と同じtyped `TaskListRow::Task`を生成し、製品のrow変換とformatterを通して検証する。日時error testはwriterを持たない現在のcontext interfaceに合わせ、`Result::Err`、repository、focusの契約だけを正確に表現する。

**Tech Stack:** Rust、標準test framework、Cargo

---

## Baseline

- lib: 501 passed、1 ignored
- CLI binary: 434 passed
- MCP binary: 2 passed
- MCP stdio: 12 passed
- Spreadsheet: 4 passed
- 合計: 953 passed、1 ignored
- ignored test: `benchmark_save_2172project中1件変更を2秒未満で処理する`

## Private Interface Changes

- `TaskListDisplayRow::into_display_row`を`pub(super)`にし、testからも製品と同じtyped row変換を通せるようにする。外部公開APIにはしない。
- test fixtureのreal taskは`TaskListRow::Message`ではなく、製品と同じ`TaskListRow::Task(TaskListTaskRow)`を生成する。
- `#[cfg(test)]`の`render_message`と`replace_task_list_icon`を製品moduleから削除する。
- error時の表示非変更は、切断されたwriterではなく`Result::Err`によって成功用`DisplayModel`が返らない契約として表現する。

## Commit Sequence

| 順序 | Commit message | 固定する契約・変更 | 依存 | 対象test・Green確認 |
| --- | --- | --- | --- | --- |
| 1 | `Docs: TD-018 test integrity是正計画を記録する` | 2指摘、commit境界、既存契約の不変条件を記録 | なし | full suite |
| 2 | `Fix: Task list icon testを製品typed row経路へ戻す` | `new_task` fixtureをtyped task rowへ変更し、mark後のrowを製品の`into_display_row`とformatterで検証する。既存icon testも`TaskListTaskRow`から元iconとgive-up iconのA-J列を比較する。test専用helperと文字列書換え分岐を削除 | 1 | `mark_give_up_candidate_rows`、`test_replace_task_list_icon`、`task_list_icon_mode`、Spreadsheet contract、full suite |
| 3 | `Fix: Task list error testの無効なwriter assertionを除く` | 既存2 testから未接続`TestWriter`を削除。test名を「成功表示を返さず状態を変更しない」へ正確化し、詳細error、repository snapshot、focus不変を維持 | 2 | `test_show_task_list_`、full suite |
| 4 | `Docs: TD-018 test integrity是正完了を記録する` | 実測件数、品質ゲート、独立review結果、例外となるtest名変更理由を記録 | 3 | full suite |

製品不具合ではなくtest harnessの問題であるため、新しいRed test commitは作らない。review指摘ごとに独立したGreenの`Fix:` commitとする。

## Quality Gates

各Green commitで以下を実行する。

```bash
git diff --check
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

対象testは次のfilterで先に確認する。

```bash
cargo test --locked --bin schronu mark_give_up_candidate_rows
cargo test --locked --bin schronu test_replace_task_list_icon
cargo test --locked --bin schronu task_list_icon_mode
cargo test --locked --bin schronu test_show_task_list
cargo test --locked --test spreadsheet_contract
```

## Acceptance

- `view.rs`に`#[cfg(test)]`のicon置換helper・描画分岐が残らない。
- real task fixtureが必ず`TaskListRow::Task`を生成し、製品formatter経路を通る。
- 切断された`TestWriter`への常時成功assertionが残らない。
- error variant、repository snapshot、focus、give-up判定、Spreadsheet A-J列のassertionを維持する。
- test総数は953 passed、1 ignoredを維持する。
- 製品出力、CLI、Spreadsheet A-J列、YAML、MCP、公開APIを変更しない。
- 独立subagentがコード差分とcommit履歴をreviewし、test専用分岐・無効assertion・契約緩和の重大指摘がない。

既存2 testの改名は契約緩和ではなく、writerを受け取らない現在のinterfaceへ名称を正確化するための明示的な例外とする。

## Completion (2026-08-27)

- `2ec408f`: real task fixtureをtyped rowへ移し、製品のrow変換・formatter経路へ統一した。test専用`render_message`と`replace_task_list_icon`を削除した。
- `fcb85dd`: 未接続`TestWriter`と常時成功する2 assertionを削除し、具体的な`Err`、repository snapshot、focus不変を維持した。
- 計画時の`test_show_task_list_` filterは1件だけに一致したため、実行時は`test_show_task_list`へ補正して2件を検証した。
- 実測: lib 501 passed・1 ignored、CLI 434 passed、MCP binary 2 passed、MCP stdio 12 passed、Spreadsheet 4 passed。合計953 passed・1 ignored。
- `git diff --check`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`を通過した。
- コード差分とcommit履歴を別々のsubagentが監査し、test専用分岐、無効assertion、契約緩和、commit分割に残存findingがないことを確認した。
