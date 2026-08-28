# MCP `defer_routine_task` 設計

## 1. Summary

- 目的: `defer_task`(絶対時刻延期)に加えて、CLIの`W`と同等の「ルーチン延期」をMCPの明示APIとして提供する。
- 方針: raw CRUD ではなく、既存 CLI の意味論を保持した高レベル操作を公開する。
- 契約: `task_id` を受けて対象 task が「ルーチン延期可能」であれば次周期へずらす。

## 2. Public API / Interface changes

- MCP Tool 名: `defer_routine_task`
- 入力: `task_id`: `uuid`。必須で、unknown fieldを拒否する。
- 成功レスポンス: `{"task_id": "<uuid>"}`
- 失敗:
  - `task_not_found`
  - `invalid_input`。`field`は`task_id`に統一し、reasonは`task must have a deadline`、`task must have a parent`、`parent task must have a repetition interval`で区別する。
  - 既存 MCP と同様の `repository_save_failed` / `repository_state_uncertain`
- `tools/list` と契約 fixture を更新
- 対象条件はtask自身のdeadline、直接の親、親自身の`repetition_interval_days`とする。祖先から継承せず、leaf制約は追加しない。

## 3. 実装方針

- `src/application/task_use_case.rs`
  - `pub fn defer_routine_task(...)` を追加
  - 既存 `execute_defer_routine` の意味論を use case 化して移植
  - エラー:
    - 取得失敗: `TaskNotFound`
    - `deadline_time` 未設定: `InvalidInput`
    - 親なし: `InvalidInput`
    - `repetition_interval_days` 未設定: `InvalidInput`
  - 締切計算:
    - 親 `deadline_time` あり:
      - `try_next_business_day_start(orig_deadline)` を基点に、`repetition_interval_days - 1` 日進めた日付へ
      - 時刻は親の `deadline_time.time()` を採用
    - 親 `deadline_time` なし:
      - `orig_deadline + repetition_interval_days`
  - `start_time`: `(new_deadline - orig_deadline).num_days()` 日分を加算し、元の時刻を維持
  - 全日時を計算してからmutationする。日時範囲外、曖昧・存在しないlocal日時、task tree errorは`ApplicationError`の情報を保持して伝播する。
  - 対象 task:
    - `orig_status` を `Todo`
    - `replace_deadline_time`で対象deadlineと既存の子への伝播を原子的に更新
    - `start_time` を更新
    - `pending_until`は変更しない

- `src/adapter/mcp/input.rs`
  - `DeferRoutineTaskInput { task_id: UuidValue }` を追加
- `src/adapter/mcp/registry.rs`
  - ツール定義を追加
- `src/adapter/mcp/handler.rs`
  - dispatch 追加
  - `call_defer_routine_task` を追加
  - 成功時レスポンスに `task_id` を返却
  - `tool_call_succeeded_with_mutation` に `defer_routine_task` を追加

- CLI 側 `W` との一貫性:
  - `execute_defer_routine` は application use case 呼び出しへ集約する。
  - focus未設定・対象なし・非ルーチン時のno-opと、成功時のfocus解除はCLI adapterで維持する。

## 4. テスト設計

- `src/application/task_use_case_tests.rs`
  - 成功:
    - 親 `deadline_time` あり・なしの両ケース
  - 失敗:
    - 非ルーチン条件(親なし/締切なし/反復間隔なし)で`InvalidInput`
    - 日時範囲外とdeadline伝播失敗でtask snapshotとmutation revisionを維持
- `src/adapter/mcp/input_tests.rs`
  - スキーマ検証: required、型、UUID、unknown field
- `src/adapter/mcp/protocol_contract_tests.rs`
  - `tools/list` と `defer_routine_task` の名前・properties・required の一致
- `src/adapter/mcp/tool_contract_tests.rs`
  - 成功1件(保存1回、deadline/start/status更新、`pending_until`維持)
  - schema違反、不正UUID、未知ID、対象不成立(save 0)
  - 保存失敗(`repository_save_failed` + uncertain)
- `tests/mcp_stdio.rs`
  - 10 toolを実filesystem repositoryで実行
  - 専用testでprocess再起動後のdeadline/start/status永続化を検証
- `tests/fixtures/mcp/tools-list.json`
  - ツール定義を追加
- `README.md`
  - MCP tool 一覧に `defer_routine_task` を追記

## 5. 前提

- `focused` の概念は MCP では扱わず、`task_id` を明示指定する。
- ルール成立しない場合は MCP 側で明示 error を返却する。
- raw CRUD、`pending_until`入力、Spreadsheet、YAML、Apps Script、CLI command構文の変更は行わない。
