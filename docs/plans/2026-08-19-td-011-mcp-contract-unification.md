# TD-011 MCP契約統合 Implementation Plan

**Goal:** MCPの9 toolについてSerde対応の入力型を契約源とし、JSON Schema、入力検証、Rust入力型、JSON出力の重複を解消する。

**Architecture:** `McpServer`にはrepository transaction調停を残し、protocol、handler、registry、input、outputを`src/adapter/mcp/`へ分離する。Serde入力DTOからschemaを生成し、application型への変換で既存のschema errorとsemantic errorを維持する。

**Tech Stack:** Rust 2021、Serde、serde_json、Schemars、serde_path_to_error、jsonschema(testのみ)

---

## 公開契約

- 9 toolの名前、description、field、required、nullable、format、minimum、additionalPropertiesを変更しない。
- JSON-RPC `-32602`とtool-level structured errorのcode、field、reason、structuredContentを維持する。
- `McpServer`のpublic APIは変更しない。
- `TaskView`と`ScheduledTaskView`はSerde serializationを公開JSONの契約源とする。

## Architecture

- `src/adapter/mcp.rs`には`McpServer`とrepository transaction調停のみを残す。
- `src/adapter/mcp/protocol.rs`はJSON-RPC envelope、lifecycle、response/error生成を担う。
- `src/adapter/mcp/handler.rs`はtool dispatchとapplication use case呼び出しを担う。
- `src/adapter/mcp/registry.rs`はtool名、description、入力型、生成schemaの対応を担う。
- `src/adapter/mcp/input.rs`はSerde入力DTO、field-path付きdecode、application型への変換を担う。
- application/entityのviewとenumがSerialize表現を定義し、`src/adapter/mcp/output.rs`は`serde_json::to_value`によるresponse変換だけを担って手書きmapperを廃止する。
- 入力DTOは`Deserialize + JsonSchema + deny_unknown_fields`を基本とする。UUID、RFC 3339日時、ISO日付、非負`i64`、非空文字列、nullable patchを共通型として1か所に定義する。
- `update_task`の「更新fieldを1つ以上指定」は共有field定数をschema transformとruntime検証の双方から参照する。
- `schemars = "1.2.2"`、`serde_path_to_error = "0.1.20"`をproduction dependencyに追加する。schema検証用の`jsonschema = { version = "0.49.3", default-features = false }`はdev-dependencyに限定する。

## Commit計画

| No. | Commit message | 固定する契約 | 対象責務 | 依存 | 対象test / Green確認 |
| --- | --- | --- | --- | --- | --- |
| 1 | `Docs: TD-011実装計画を記録` | 本計画とcommit依存 | 計画文書 | なし | `git diff --check` |
| 2 | `Test: MCP公開schemaとJSON出力を固定する` | 9 toolのfield、nullable、required、additional propertyとTask/Schedule全field JSON | 契約test / `tests/fixtures/mcp/` | 1 | 現行実装のままgoldenと一致 |
| 3 | `Test: MCP protocolとtool testを分離する` | protocolとbusiness testの独立実行 | test module | 2 | 両filterと全test |
| 4 | `MCP: protocol lifecycle境界を分離する` | lifecycleとJSON-RPC envelope | protocol | 3 | protocol / stdio |
| 5 | `MCP: tool handler境界を分離する` | dispatch、transaction、save判定 | handler | 4 | tool / stdio |
| 6 | `MCP: tool registry境界を分離する` | 現行tool metadataとschemaを挙動変更なしで移動 | registry | 5 | tools/listが移動前と同じJSON値 |
| 7 | `MCP: tool input境界を分離する` | 現行validatorを挙動変更なしで移動 | input | 6 | 全入力error test |
| 8 | `MCP: tool output境界を分離する` | 現行mapperを挙動変更なしで移動 | output | 7 | Task/Schedule golden |
| 9 | `Test: MCP共通入力制約の生成契約を追加する` | 共通scalarのschema/decode一致 | input test | 8 | 想定理由でRed |
| 10 | `MCP: Serde入力契約の共通基盤を追加する` | field pathとerror区分 | input | 9 | input unit / schema検証 |
| 11 | `Test: 参照toolのtyped入力契約を追加する` | get_focus / get_task DTOとschema | input contract | 10 | 想定理由でRed |
| 12 | `MCP: 参照toolのtyped入力を実装する` | 空入力、UUID、unknown field | input / registry metadata | 11 | 対象schema/decode matrix |
| 13 | `Test: 参照toolのtyped handler契約を追加する` | handlerがtyped入力だけを受け取る | handler contract | 12 | 想定理由でRed |
| 14 | `MCP: 参照tool handlerをtyped入力へ接続する` | application変換と既存response | handler | 13 | get_focus/get_task business test |
| 15 | `Test: 検索toolのtyped入力契約を追加する` | list_tasks / get_schedule DTOとschema | input contract | 14 | 想定理由でRed |
| 16 | `MCP: 検索toolのtyped入力を実装する` | nested path、nullable category、period/date | input / registry metadata | 15 | 対象schema/decode matrix |
| 17 | `Test: 検索toolのtyped handler契約を追加する` | typed filterと既定期間のapplication変換 | handler contract | 16 | 想定理由でRed |
| 18 | `MCP: 検索tool handlerをtyped入力へ接続する` | 逆転期間、structured error、検索結果 | handler | 17 | list/schedule business test |
| 19 | `Test: 作成toolのtyped入力契約を追加する` | create_task / breakdown_task DTOとschema | input contract | 18 | 想定理由でRed |
| 20 | `MCP: 作成toolのtyped入力を実装する` | name、names、見積、日時 | input / registry metadata | 19 | 対象schema/decode matrix |
| 21 | `Test: 作成toolのtyped handler契約を追加する` | typed作成入力のapplication変換 | handler contract | 20 | 想定理由でRed |
| 22 | `MCP: 作成tool handlerをtyped入力へ接続する` | 空白error、overflow、原子性、save回数 | handler | 21 | create/breakdown business test |
| 23 | `Test: 状態変更toolのtyped入力契約を追加する` | defer / complete / update DTOとschema | input contract | 22 | 想定理由でRed |
| 24 | `MCP: 状態変更toolのtyped入力を実装する` | default、nullable patch、更新field共有定数 | input / registry metadata | 23 | 対象schema/decode matrix |
| 25 | `Test: 状態変更toolのtyped handler契約を追加する` | typed状態変更入力のapplication変換 | handler contract | 24 | 想定理由でRed |
| 26 | `MCP: 状態変更tool handlerをtyped入力へ接続する` | finished_at、解除、partial update順序、save回数 | handler | 25 | defer/complete/update business test |
| 27 | `Test: MCP viewのSerde出力契約を追加する` | application viewとJSONの同一性 | output test | 26 | 想定理由でRed |
| 28 | `MCP: view出力をSerde serializationへ統合する` | UUID、日時、lowercase enum、null、全field | application / entity serialization | 27 | output / integration |
| 29 | `MCP: 旧契約経路を除去する` | 手書きvalidator・schema・mapperとregistry外のtool定義経路を廃止 | MCP全体 | 28 | `rg`で旧helper不在 / 全品質ゲート |
| 30 | `Docs: TD-011の完了を記録` | 対応内容と検証証跡 | backlog | 29 / 最終review | `git diff --check` |

各Red commitでは対象testが期待した1つの未実装理由で失敗することを確認する。各基礎Green後に対象test、全品質ゲート、subagent reviewを行い、review指摘は1件ずつ個別commitにする。

## 品質ゲート

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

追加でprotocol contract、tool contract、`tests/mcp_stdio.rs`を個別実行し、schema validatorとtyped decodeを正常・欠落・型違い・unknown field・境界値の同じcase集合で照合する。`tests/mcp_stdio.rs`ではfilesystem、lock、再起動、9 toolの経路を維持する。JSON-RPC errorとstructured errorのcode、field、reasonに加え、`structuredContent`と`content.text`の一致も確認する。既存testの削除・緩和・意味変更、test専用製品分岐、不要なpublic API追加は行わず、既存のignored test 1件とその理由を維持する。

最終段階で`git diff --stat`とcommit履歴をreviewし、Red/Greenの対応、機械的移動の独立、review修正の指摘単位、documentation分離、各commitの単一目的性を確認する。

## 前提

- UUID・日時の形式不正はtool-level structured error、JSON型・required・minimum・additional property違反はJSON-RPC `-32602`を維持する。
- `complete_task.finished_at`の`Local::now()`と`get_schedule`の既定期間は変更しない。
- application/entityへの変更は汎用的な`Serialize`追加だけに限定する。
- Commit 28で`chrono`と`uuid`の`serde` featureを有効化し、`Status`、`RepetitionAnchor`、`ProjectCategory`は`lowercase`のSerde表現を明示する。
- schema生成結果は事前に固定したgolden fixtureとJSON値として一致させる。
