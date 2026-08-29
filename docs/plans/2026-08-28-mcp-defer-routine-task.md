# MCP defer_routine_task Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** CLI `W` と同じ次周期へのルーチン延期を、`task_id` 指定の10個目のMCP toolとして追加する。

**Architecture:** 日時計算とtask mutationはapplication use caseへ集約する。CLI adapterは従来のno-opとfocus解除を維持し、MCP adapterはtyped入力、structured error、transaction保存へ接続する。

**Tech Stack:** Rust、chrono、serde、schemars、JSON-RPC/MCP、標準Rust test

---

## 実装順序

1. applicationへ厳格な`defer_routine_task`契約testを追加し、Redを確認してcommitする。
2. 現行`execute_defer_routine`の日時計算とmutationをapplicationへ移し、Green・全品質ゲート・review後にcommitする。
3. CLIのno-op characterization testを追加し、計算をapplication呼び出しへ置換する。
4. MCP typed入力、handler、dispatch・保存判定を、それぞれ独立したRed/Green cycleで追加する。
5. `tools/list` fixture、protocol contract、stdio永続化testを追加してからregistryを公開する。
6. READMEと設計書を実装に合わせ、全品質ゲートと最終履歴reviewを行う。

## 公開契約

- Tool名: `defer_routine_task`
- 入力: `{"task_id":"<uuid>"}`。`task_id`は必須でunknown fieldを拒否する。
- 成功: `{"task_id":"<uuid>"}`
- 対象条件: task自身のdeadline、直接の親、親自身の`repetition_interval_days`が存在すること。
- MCPの対象不成立: `task_not_found`または`invalid_input`。CLIでは従来どおりno-op。
- 親deadlineありの場合は次論理日境界から`interval - 1`日後の日付へ親deadline時刻を適用する。なしの場合は元deadlineへ`interval`日を加える。
- startは新旧deadline差の整数日だけ移動し、`orig_status`を`Todo`へ戻す。`pending_until`は変更しない。

## 品質ゲート

各Green commit前と最終確認で次を実行する。

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
git diff --check
```

各commit後に`git status --short --branch`と`git show --stat --oneline HEAD`を確認する。
