# TD-015 Test Organization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

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

1. `Docs: TD-015の実装計画を記録する`
2. `Test: Task単体testを別fileへ移す`
3. `Test: CLI runtime単体testを別fileへ移す`
4. `Test: CLI runtime契約testを別fileへ移す`
5. `Test: CLI runtime fixtureをtest supportへ移す`
6. `Test: Task use case testを別fileへ移す`
7. `Test: Pack use case testを別fileへ移す`
8. `Test: Schedule use case testを別fileへ移す`
9. `Test: Daily capacity testを別fileへ移す`
10. `Test: Task repository testを別fileへ移す`
11. `Test: YAML gateway testを別fileへ移す`
12. `Test: Free time gateway testを別fileへ移す`
13. `Test: MCP input単体testを別fileへ移す`
14. `Test: MCP handler単体testを別fileへ移す`
15. `Test: Task fixture生成を共通化する`
16. `Test: List tasks repository fixtureを共通化する`
17. `Test: Schedule repository fixtureを共通化する`
18. `Test: Task use case repository fixtureを共通化する`
19. `Test: CLI free-time fixtureを共通化する`
20. `Docs: test責務とfixture配置を説明する`
21. `Docs: TD-015の完了を記録する`

機械的移動commitではtest関数名、assertion、fixture値、module pathを変更しない。fixture共通化commitでは製品向け公開APIを追加せず、test専用APIだけを`cfg(test)`配下へ置く。

## Verification

各Green commitで対象test、`git diff --check`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`を実行する。commit後に差分量と履歴を確認し、subagent reviewの指摘は1件ずつ独立commitで修正する。

最終的にbaselineのtest件数、ignored testの意図、CLI・YAML・MCP・Spreadsheet契約を維持する。
