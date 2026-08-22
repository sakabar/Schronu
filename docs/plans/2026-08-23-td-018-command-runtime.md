# TD-018 CLI Runtime Responsibility Separation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** CLIの既存契約を変えずに、巨大な`runtime.rs`からcommand contextと表示計算を分離し、handlerを唯一のcommand orchestration入口、runtimeをI/O調停境界にする。

**Architecture:** privateな`CommandContext`と`CliCommandContext`へrepository、free-time、TaskFactory、focus、設定、日時解釈、domain mutationを集約する。handlerはtyped commandを処理して`CommandOutcome`を返し、`view.rs`は意味的な`DisplayModel`を構築し、rendererが既存の文字列・ANSI・改行・flush契約へ変換する。runtimeにはrepository transaction、external URL、storage lock、interactive event loopなどの外部I/O調停だけを残す。

**Tech Stack:** Rust、Cargo、標準Rust test、既存CLI parser/handler/renderer、既存repository・free-time gateway、Clippy、rustfmt

---

## Summary

`feature/td-018-command-runtime`を`main`から作成し、本計画に従って責務単位のRed/Green cycleを進める。

現在の`src/adapter/controller/schronu/runtime.rs`は4,892行である。typed command dispatchはすでに`handler.rs`へ移っているが、command context、日時解釈、domain mutation、tree・task list・calendar・band・focusの表示計算が残っている。これらをprivateな`command_context.rs`と`view.rs`へ分離し、handlerを唯一のcommand orchestration入口にする。

### Baseline

- lib: 501 passed、1 ignored
- CLI binary: 333 passed
- MCP binary: 2 passed
- MCP stdio: 12 passed
- Spreadsheet: 4 passed
- 合計: 852 passed、1 ignored

## Private Interface Changes

製品公開API、command名・alias、CLI文言、Spreadsheet A-J列、YAML、MCP契約は変更しない。

- `handler::handle_command`を全通常commandの統合入口にする。
- privateな`CommandContext`を既存5 context traitの合成境界として定義し、`CliCommandContext`がrepository、free-time、TaskFactory、focus、設定を提供する。
- `CommandOutcome`は`DisplayModel`、外部I/O要求、`FocusChange`を返す。既存の`FocusRequest`は`FocusChange::{Keep, Clear, Set(Uuid), SelectionMode(FocusSelection)}`へ置換し、runtimeは要求の適用だけを担当する。
- `HandlerError`は`Parse(CommandParseError)`と`Application(ApplicationError)`として原因情報を保持する。runtimeは両者をvariantを潰さず対応する`CommandError`へ変換する。`Output`と`ExternalOpen`のerrorはruntimeが所有する。
- `DisplayModel`を`Message`、`Tree`、`TaskList`、`Calendar`、`Band`、`Focus`、`Pack`、`Flatten`、`Sequence`で構成する。
- `DisplayFragment`と`DisplayRecorder`は最終的に削除する。ANSIとwriter固有改行はrenderer内部の責務とする。
- privateな`RenderMode::{Flushed, Unflushed}`をrenderer APIへ渡す。runtimeはnon-interactiveのnon-`Noop`を`Flushed`、interactiveを`Unflushed`として選び、rendererが実際のflushを行う。`DisplayModel`にはflush指定を含めない。
- `view.rs`はtyped row・集計値・alert状態を生成し、rendererが既存文字列へ変換する。
- `Verify`のread-only repository検査と外部URL起動はruntimeに残すが、成功・error表示は意味的model経由にする。

## Commit Sequence

| 順序 | Commit message | 固定する契約・変更対象 | 依存 | 対象test | Green確認方法 |
| --- | --- | --- | --- | --- | --- |
| 1 | `Docs: TD-018の実装計画を記録する` | baseline、private interface、commit境界を記録 | なし | full suite | 全品質ゲート |
| 2 | `Test: Handler所有権を挙動契約で固定する` | runtime source文字列依存を、typed commandとfake contextによるdispatch検証へ置換 | 1 | handler、runtime | 対象test後に全品質ゲート |
| 3 | `Test: Renderer error契約を挙動検証へ置換する` | source検査をerror modelからの出力・I/O error検証へ置換 | 2 | renderer | 対象test後に全品質ゲート |
| 4 | `Test: Runtime調停契約を挙動検証へ置換する` | outcome、flush、broken pipe、external request、`FocusChange`の4 variant適用をtraceで固定 | 3 | runtime | 対象test後に全品質ゲート |
| 5 | `Test: Interactive共通経路を挙動検証へ置換する` | interactive/non-interactiveが同じparser・handlerを通ることを固定 | 4 | interactive、runtime | 対象test後に全品質ゲート |
| 6 | `Refactor: CLI表示計算をview moduleへ移す` | tree/list/calendar/band/focus builderと計算helperを`view.rs`へ機械移動 | 5 | runtime 291件 | 対象test後に全品質ゲート |
| 7 | `Refactor: CLI command contextを別moduleへ移す` | context実装、日時解釈、command mutation helperを`command_context.rs`へ機械移動 | 6 | handler、runtime | 対象test後に全品質ゲート |
| 8 | `Test: CLI handler統合経路を追加する` | `handle_command`未実装を理由にRed。全通常commandが1入口で処理される契約 | 7 | handler | 期待した1理由のRedを確認してtestだけcommit |
| 9 | `CLI: composite command contextを実装する` | `CommandContext`、`CliCommandContext`、`HandlerError`、`FocusChange`を導入し、旧経路と併存させる | 8 | handler | 対象test後に全品質ゲート |
| 10 | `CLI: Runtimeをhandler統合入口へ切り替える` | runtimeの全通常commandを`handle_command`経由へ切り替え、legacy dispatchは到達不能な旧実装として残す | 9 | handler、runtime | 対象test後に全品質ゲート |
| 11 | `CLI: legacy command dispatchを除去する` | handler統合入口への切替後にlegacy dispatchを削除する | 10 | handler、runtime | 対象test後に全品質ゲート |
| 12 | `Test: 意味的message modelを追加する` | plain/info/warn/critical/errorと複数messageのRed contract | 11 | renderer | 期待した1理由のRedを確認してtestだけcommit |
| 13 | `CLI: messageとerror表示を意味的modelへ移す` | prefix、改行、error分類を維持し、単純出力をmodel化 | 12 | renderer、handler、runtime | 対象test後に全品質ゲート |
| 14 | `Test: Tree表示modelを追加する` | tree、ancestor、leaf rowと空行配置のRed contract | 13 | renderer | 期待した1理由のRedを確認してtestだけcommit |
| 15 | `CLI: Tree表示を意味的modelへ移す` | writerをcontext traitから除き、typed tree modelを返す | 14 | tree関連runtime契約 | 対象test後に全品質ゲート |
| 16 | `Test: Task list表示modelを追加する` | row順、give-up icon、category集計、A-J列のRed contract | 15 | renderer、Spreadsheet | 期待した1理由のRedを確認してtestだけcommit |
| 17 | `CLI: Task list表示を意味的modelへ移す` | task rowと集計値をtyped model化し、A-J formatterをrendererに維持 | 16 | list/today/tail全契約 | 対象test後に全品質ゲート |
| 18 | `Test: Calendar表示modelを追加する` | 日付逆順、週区切り、footer、alertのRed contract | 17 | renderer | 期待した1理由のRedを確認してtestだけcommit |
| 19 | `CLI: Calendar表示を意味的modelへ移す` | 日別数値とalert状態をmodel化し、文字列整形をrendererへ移す | 18 | calendar契約 | 対象test後に全品質ゲート |
| 20 | `Test: Band表示modelを追加する` | 96 segment、7色ANSI、非terminal、overflow、凡例のRed contract | 19 | renderer | 期待した1理由のRedを確認してtestだけcommit |
| 21 | `CLI: Band表示を意味的modelへ移す` | duration分類をtyped model化し、色と記号の選択をrendererへ移す | 20 | band契約 | 対象test後に全品質ゲート |
| 22 | `Test: Pack表示modelを追加する` | packed row、空結果、summary、skip件数のRed contract | 21 | renderer、pack | 期待した1理由のRedを確認してtestだけcommit |
| 23 | `CLI: Pack表示を意味的modelへ移す` | `PackResult`からprivate `PackDisplay`を構築 | 22 | pack契約 | 対象test後に全品質ゲート |
| 24 | `Test: Flatten表示modelを追加する` | overload、未解消理由、代表task、warning順のRed contract | 23 | renderer、flatten | 期待した1理由のRedを確認してtestだけcommit |
| 25 | `CLI: Flatten表示を意味的modelへ移す` | `FlattenResult`をtyped displayへ変換 | 24 | flatten契約 | 対象test後に全品質ゲート |
| 26 | `Test: Focus表示modelを追加する` | ancestor、category、attr、残時間、progress、overflowのRed contract | 25 | renderer、runtime | 期待した1理由のRedを確認してtestだけcommit |
| 27 | `CLI: Focus表示を意味的modelへ移す` | focus計算をviewへ、terminal描画をrendererへ移す | 26 | interactive画面契約 | 対象test後に全品質ゲート |
| 28 | `Test: Interactive再描画判断をtyped commandで固定する` | raw先頭文字ではなく`CommandKind`で再描画を判断するRed contract | 27 | interactive | 期待した1理由のRedを確認してtestだけcommit |
| 29 | `CLI: Interactive再描画をtyped commandへ統一する` | command文字列の再解析・先頭文字判定を除去 | 28 | interactive、runtime | 対象test後に全品質ゲート |
| 30 | `Test: Verify表示を意味的modelで固定する` | Verifyの成功・error表示を意味的modelから生成するRed contract | 29 | renderer、runtime | 期待した1理由のRedを確認してtestだけcommit |
| 31 | `CLI: Verify表示を意味的modelへ移す` | Verifyのread-only検査をruntimeに保ち、成功・error表示をrenderer経由へ統一 | 30 | renderer、runtime | 対象test後に全品質ゲート |
| 32 | `Test: Runtime責務境界を固定する` | context、日時解釈、mutation、表示計算、raw recorderがruntimeにないことをRedで固定 | 31 | runtime architecture | 期待した1理由のRedを確認してtestだけcommit |
| 33 | `Refactor: legacy display recorderを除去する` | legacy fragment/recorderと不要helper/importだけを挙動変更なしで削除し、runtimeをI/O調停へ限定 | 32 | CLI全test | 対象test後に全品質ゲート |
| 34 | `Docs: CLI command境界を説明する` | READMEへparser→handler→view model→renderer→runtimeの責務を記録 | 33 | full suite | 全品質ゲート |
| 35 | `Docs: TD-018の完了を記録する` | backlogへ完了日、実測件数、runtime行数、品質ゲートを記録 | 34 | full suite | 全品質ゲート |

計画本体は全35 commitとする。

順序6・7は機械移動のみとし、commit本文へ大規模になる理由、挙動変更なし、実行した品質ゲートを記録する。Red commit以外の全commitも、本文へ対象testと全品質ゲートの結果を残す。

各commitは表の依存順で進める。独立した契約を未commitのまま溜めず、Green commit後にreviewする。review指摘は1件ずつ`Fix: ...` commitへ分け、関連する対象testと全品質ゲートを再実行する。

## Verification and Acceptance

各Red commitでは対象testが新しいinterfaceまたはmodelの欠如という1理由だけで失敗することを確認する。対応するGreen commitでは対象testに続いて以下を実行する。

```bash
git diff --check
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

各Green commit後に以下を確認する。

```bash
git status --short --branch
git show --stat --oneline HEAD
```

最終受入条件:

- baselineの既存test名と保護対象契約を維持し、852 passed、1 ignoredの既存suiteを全件維持する。最終passed総数は新規contract testの追加分だけ852より増加する。commit 2-5では、脆いsource依存assertionだけを同等以上の挙動assertionへ置換する。
- ignored testは既存benchmarkのまま維持する。
- `runtime.rs`にcommand context実装、command引数の日時解釈、domain mutation、表示計算、`DisplayFragment`、`DisplayRecorder`を残さない。
- handlerの製品経路をfake `CommandContext`で検証できる。
- rendererが意味的modelから既存出力を生成する。
- interactive/non-interactiveのtransaction、save、error分類を維持する。
- Spreadsheet A-J列、YAML、MCP、shell、Apps Scriptの契約を変更しない。
- 最終コード・履歴reviewで重大指摘がない。

## Assumptions

- CLI固有のview計算はadapter層に置き、今回新しいapplication公開APIは追加しない。
- treeの既存debug表現は互換性のため保持するが、`TreeDisplay`として他の出力と区別する。
- external URL起動、storage lock、repository transaction、interactive event loopはruntime責務として残す。
- TD-018と無関係なcommand構文、domain algorithm、YAML形式、MCP schemaのcleanupは行わない。
