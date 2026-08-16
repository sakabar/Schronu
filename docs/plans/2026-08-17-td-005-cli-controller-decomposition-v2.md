# TD-005 CLI境界分割再実装 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. 各Green実装のreviewにはsuperpowers:subagent-driven-development、完了判定にはsuperpowers:verification-before-completionを使用すること。

**Goal:** 既存CLIの外部契約を維持したまま、`schronu.rs`の責務をtyped command、handler、renderer、interactive driver、runtimeへ段階的に分割する。

**Architecture:** `src/adapter/controller/schronu.rs`は依存を構築して`runtime::application()`を呼ぶだけのentrypointとし、private moduleを`src/adapter/controller/schronu/`へ配置する。interactive/non-interactiveの両経路は共通parser、typed handler、rendererを通り、terminal、browser、repository transactionなどの副作用は境界moduleへ隔離する。

**Tech Stack:** Rust、Cargo、標準Rust test、termion、既存repository adapter、shell script、Google Apps Script

---

## 作業条件

- branchは`feature/td-005-cli-controller-decomposition-restart`を継続し、新しいbranchは作らない。
- 既存commit `83ce193`を保持する。
- baselineは`main == origin/main == 6e8e9dd`であり、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`の成功を確認済みである。
- 旧branchと途中のsplit系branchは参照専用とする。`ade9952`を含むcommitのcherry-pickや巨大patchの転用は行わない。
- 現在の変更を破棄しない。旧branchを削除、rebase、force-pushしない。
- PR作成、push、force-push、旧branch操作、TD-018およびTD-015の製品実装は対象外とする。

## 維持する契約

- 日本語・英語のcommand名とalias、既存の有効入力、CLI表示文言、ANSI対応、writer固有の改行規約を変更しない。
- broken pipeとその他出力errorの分類、repository transaction、reload、saveのタイミング、YAML形式、MCP契約を変更しない。
- Spreadsheet連携は`全`出力のA-J列を維持し、I列を`category`、J列を`task_name`とする。shell script、Apps Script、READMEが前提とする列番号も維持する。
- 既存testを削除、緩和、assertionの意味変更をしない。新規testは製品経路を通し、test専用分岐や不要な公開APIを追加しない。
- 公開APIは追加しない。標準ライブラリや既存crateの型で意味を表せる場合は独自wrapperを増やさない。
- 複数箇所で同じhelperが必要なら共通化先を検討し、ファイルごとの同名helperを増やさない。

## 最終architecture

- `command.rs`: `ParseMode`、typed `Command` / `CommandAction`、field・reason・usageを保持するparse error。
- `handler.rs`: typed commandを直接処理し、表示結果、外部起動要求、focus mode変更要求を返す。文字列やtoken列への逆変換は禁止する。
- `renderer.rs`: `DisplayModel`、writer抽象、ANSI・改行・flush・broken pipe、Spreadsheet A-J formatter。
- `interactive.rs`: raw terminal、入力thread、cursor編集、refresh、Ctrl-C / Ctrl-Dを隔離する。
- `runtime.rs`: repository transaction、reload / save、browser起動、interactive / non-interactive調停を担当する。
- `schronu.rs`: module宣言と依存構築後に`runtime::application()`を呼ぶだけのentrypointにする。

### Handlerの段階移行

- Commit 7で`renderer.rs`へ最小の`DisplayModel`、`DisplayFragment`、`DisplayRecorder`、`render_display_model`を導入し、最終形の`CommandOutcome { display: DisplayModel, external_request: Option<ExternalRequest>, focus_request: Option<FocusRequest> }`を定義する。
- Commit 7では`Open`、`Obsidian`、`FocusHighest`、`FocusLowest`を最初の移行済みcommandとしてhandlerへ移す。handlerはbrowserやfocus状態を直接操作せず、`external_request`または`focus_request`を返す。
- Commit 7からCommit 22まではruntimeが全`Command`を受け取り、移行済み`CommandKind`だけをhandlerへ渡す。移行済みcommandは`DisplayRecorder`へ書き、runtimeが直ちに`render_display_model`で実writerへ出力する。未移行variantだけをruntime内の既存writer付きtyped dispatchが所有し、handlerからruntimeを呼ばない。
- Commit 7ではinteractive / non-interactiveの各既存runtime経路が個別に`CommandOutcome`を解釈してbrowser起動とfocus変更を実行し、従来挙動をGreenに保つ。Commit 27で両経路を共通の`apply_command_outcome`へ集約し、browser実装とrepository transactionのruntime調停を完成させる。
- Commit 23では`DisplayModel`を初導入せず、残っている全handler出力とerror、flush、broken pipe分類をrenderer境界へ完全移行する。
- module依存は`runtime -> handler / renderer`、`handler -> command / renderer`とし、`handler -> runtime`を禁止する。

## 必須の互換性

### 検索fallback

- 未知入力は`全`検索fallbackとして扱い、複数語なら従来どおり先頭tokenだけを検索語にする。`foo bar`は`foo`検索となる。
- raw入力の先頭が`0`ならNo-opとする。`"0001 task"`はNo-op、`" 0001 task"`は`0001`検索となる。

### Interactive shortcut

- interactive時だけ`t`、`h`、`D`、`d`、`w`、`W`、`y`をtyped commandとして扱う。
- non-interactiveではinteractive専用解釈を行わない。

### Focus mode

- `高`、`低`および英語aliasはinteractive時だけfocus mode commandとする。non-interactiveの`high`や`低`などは検索fallbackとする。
- `高`は引数なしだけを受理する。
- `低`は引数なし、またはASCII数字による非負整数1個だけを受理する。
- `高 1`、`低 -1`、`低 1 2`はparse errorとする。

### Extrude

- `押` / `extrude`の引数省略はNo-opとする。
- 不正な第1引数は従来どおり1日として扱い、新しいparse errorへ変更しない。

## Commit計画

各Red commitでは対象testを追加し、指定した対象test commandが期待した1つの理由で失敗することを確認してから、testだけをcommitする。各Green commitでは最小限の製品実装を行い、対象test、全品質ゲート、サブエージェントreviewの順に検証してからcommitする。

### Commit 1: `Docs: TD-005再実装計画を記録`

- **固定する契約:** branch、baseline、禁止事項、全実装順序と検証方法を実装前に固定する。
- **対象:** `docs/plans/2026-08-17-td-005-cli-controller-decomposition-v2.md`を作成する。
- **依存:** `83ce193`およびbaseline確認。
- **検証:** `git diff --check`。

### Commit 2: `Test: CLI entrypointの分割契約を固定する`

- **固定する契約:** binary entrypointが依存構築先を呼ぶだけであり、`src/adapter/controller/schronu/`配下に分割moduleが存在する。
- **対象:** `src/adapter/controller/schronu.rs`内のtest、または同binaryを対象にする構造test。
- **依存:** Commit 1。
- **Red確認:** `cargo test --locked --bin schronu entrypoint`が構造未分割だけを理由に失敗する。

### Commit 3: `CLI: entrypointからruntimeを分離する`

- **固定する契約:** 現行CLI挙動を変えず、entrypointとruntimeの責務だけを機械的に分離する。
- **対象:** 現行実装とtestを`src/adapter/controller/schronu/runtime.rs`へ移し、`src/adapter/controller/schronu.rs`を薄くする。
- **依存:** Commit 2。
- **Green確認:** entrypoint対象testと全品質ゲート。command解釈、型、表示内容は変更しない。

### Commit 4: `Test: typed command parser契約を固定する`

- **固定する契約:** 全command名・alias、typed field、field・reason・usageを持つparse error、空入力、comment、検索fallback、先頭空白付き`0`、interactive shortcut、focus mode、extrude互換を固定する。
- **対象:** `src/adapter/controller/schronu/command.rs`のunit testとnon-interactive製品経路test。
- **依存:** Commit 3。
- **Red確認:** 全table testと製品経路testは共通の`command::parse`を直接呼ぶ構成にし、`command::parse`が存在しないことだけを理由にcompile errorで失敗させる。製品経路への接続有無を別の失敗理由にしない。

### Commit 5: `CLI: typed command parserを導入する`

- **固定する契約:** interactive / non-interactiveの両入口が`ParseMode`を指定して同一parserを使用する。
- **対象:** `command.rs`を追加し、runtimeの両入口を接続する。
- **依存:** Commit 4。
- **Green確認:** 未知複数語、raw先頭`0`、interactive限定alias、focus mode、extrudeを含むparser対象testと全品質ゲート。

### Commit 6: `Test: typed command handler境界を固定する`

- **固定する契約:** handler入力はtyped `Command`、出力は最終形の`CommandOutcome { display: DisplayModel, external_request: Option<ExternalRequest>, focus_request: Option<FocusRequest> }`である。最初の移行対象は`Open`、`Obsidian`、`FocusHighest`、`FocusLowest`とし、handlerが対応する構造化要求を返す。runtimeが全commandを受け、移行済み`CommandKind`だけをhandlerへ渡し、未移行variantをruntime内のtyped dispatchが所有する。
- **対象:** `src/adapter/controller/schronu/handler.rs`の境界test。
- **依存:** Commit 5。
- **Red確認:** handler境界が未導入である1つの理由で対象testが失敗する。

### Commit 7: `CLI: typed command handler境界を導入する`

- **固定する契約:** handlerはterminal、環境変数、browser、transaction、runtimeへ依存せず、command文字列やtoken列を再生成しない。依存方向は`runtime -> handler / renderer`、`handler -> command / renderer`とする。
- **対象:** `handler.rs`、最終形の`CommandOutcome`、`renderer.rs`の最小`DisplayModel` / `DisplayFragment` / `DisplayRecorder` / `render_display_model`を追加する。`Open`、`Obsidian`、`FocusHighest`、`FocusLowest`をhandlerへ移し、runtimeが返却されたexternal / focus要求を既存browser / focus実装で実行する。runtimeが全`Command`を受け、移行済み`CommandKind`だけをhandlerへ渡して`DisplayModel`を直ちに実writerへ出力し、未移行variantだけはruntime内の既存writer付きtyped dispatchで処理する。handlerからruntimeを呼ばない。
- **依存:** Commit 6。
- **Green確認:** handler境界test、Open / Obsidianの外部要求test、FocusHighest / FocusLowestのfocus要求test、runtimeによる各要求の実行test、全品質ゲート。

### Commit 8: `Test: project作成commandのtyped dispatchを固定する`

- **固定する契約:** 新規、趣味、未計画、連番、反復、予定、開始がtyped fieldから処理される。
- **対象:** handlerのproject作成command test。
- **依存:** Commit 7。
- **Red確認:** 対象commandが未移行である1つの理由で失敗する。

### Commit 9: `CLI: project作成commandをhandlerへ移す`

- **固定する契約:** project作成系commandの既存挙動をtyped dispatchで維持する。
- **対象:** `handler.rs`と必要最小限のruntime接続。
- **依存:** Commit 8。
- **Green確認:** project作成command対象testと全品質ゲート。

### Commit 10: `Test: task tree表示commandのtyped dispatchを固定する`

- **固定する契約:** 木、祖先、根、葉、全、尾、今日、非反復、暦、帯、focus、pick、親、子、深、次がtyped fieldから処理される。
- **対象:** handlerのtask tree表示command test。
- **依存:** Commit 9。
- **Red確認:** 対象commandが未移行である1つの理由で失敗する。

### Commit 11: `CLI: task tree表示commandをhandlerへ移す`

- **固定する契約:** task tree表示系commandの既存表示と選択挙動をtyped dispatchで維持する。
- **対象:** `handler.rs`と必要最小限のruntime接続。
- **依存:** Commit 10。
- **Green確認:** task tree表示command対象testと全品質ゲート。

### Commit 12: `Test: breakdownとsplitのtyped dispatchを固定する`

- **固定する契約:** 分解、分割、待機がtyped fieldから処理される。
- **対象:** handlerのbreakdown / split test。
- **依存:** Commit 11。
- **Red確認:** 対象commandが未移行である1つの理由で失敗する。

### Commit 13: `CLI: breakdownとsplitをhandlerへ移す`

- **固定する契約:** 分解、分割、待機の既存挙動をtyped dispatchで維持する。
- **対象:** `handler.rs`と必要最小限のruntime接続。
- **依存:** Commit 12。
- **Green確認:** breakdown / split対象testと全品質ゲート。

### Commit 14: `Test: task属性更新commandのtyped dispatchを固定する`

- **固定する契約:** 締切、見積、均、実績、優先度、category、作業がtyped fieldから処理される。
- **対象:** handlerのtask属性更新command test。
- **依存:** Commit 13。
- **Red確認:** 対象commandが未移行である1つの理由で失敗する。

### Commit 15: `CLI: task属性更新commandをhandlerへ移す`

- **固定する契約:** task属性更新commandの入力検証と既存更新挙動をtyped dispatchで維持する。
- **対象:** `handler.rs`と必要最小限のruntime接続。
- **依存:** Commit 14。
- **Green確認:** task属性更新command対象testと全品質ゲート。

### Commit 16: `Test: defer系commandのtyped dispatchを固定する`

- **固定する契約:** 延期、定期延期、空、集、逃、押がtyped fieldから処理され、extrude互換を維持する。
- **対象:** handlerのdefer系command test。
- **依存:** Commit 15。
- **Red確認:** 対象commandが未移行である1つの理由で失敗する。

### Commit 17: `CLI: defer系commandをhandlerへ移す`

- **固定する契約:** defer系commandの既存挙動とextrudeの省略・不正値処理をtyped dispatchで維持する。
- **対象:** `handler.rs`と必要最小限のruntime接続。
- **依存:** Commit 16。
- **Green確認:** defer系command対象test、extrude互換test、全品質ゲート。

### Commit 18: `Test: 完了と配置commandのtyped dispatchを固定する`

- **固定する契約:** 完了、詰、平詰、clear系がtyped fieldから処理される。
- **対象:** handlerの完了・配置command test。
- **依存:** Commit 17。
- **Red確認:** 対象commandが未移行である1つの理由で失敗する。

### Commit 19: `CLI: 完了と配置commandをhandlerへ移す`

- **固定する契約:** 完了・配置commandの既存挙動をtyped dispatchで維持し、handler内のtoken index dispatchと旧command文字列再生成を完全に除去する。
- **対象:** `handler.rs`と必要最小限のruntime接続。
- **依存:** Commit 18。
- **Green確認:** 完了・配置command対象test、handler境界test、全品質ゲート。

### Commit 20: `Test: Spreadsheet A-J列のCLI契約を固定する`

- **固定する契約:** 製品formatterがA-J列を生成し、I列は`category`、J列は`task_name`である。
- **対象:** `全`の製品実行経路、既存fixture、`shell/copy_for_spreadsheet.sh`、`shell/generate_command_from_spreadsheet.sh`、`apps_script/main.js`、`apps_script/README.md`、`README.md`の列契約test。
- **依存:** Commit 19。
- **Red確認:** formatter単体testと`全`の製品経路testは共通の`renderer::format_spreadsheet_row`を参照し、この関数が存在しないことだけを理由にcompile errorで失敗させる。列値の不一致や製品配線の不在を同時の失敗理由にしない。

### Commit 21: `CLI: Spreadsheet formatterを分離する`

- **固定する契約:** test専用formatterを作らず、`全`の製品経路とtestが同じformatterを使用する。
- **対象:** `renderer.rs`へSpreadsheet formatterを追加し、製品経路を接続する。
- **依存:** Commit 20。
- **Green確認:** A-J列、I列、J列、shell・Apps Script・READMEとの整合testと全品質ゲート。

### Commit 22: `Test: CLI表示結果とwriter契約を固定する`

- **固定する契約:** 全handler出力が既存の`DisplayModel` / `DisplayRecorder`を通り、tree、list、calendar、band、focus、error、ANSI、writer固有改行、write順、flush、broken pipe分類を維持する。
- **対象:** rendererへの完全移行、writer、error分類test。
- **依存:** Commit 21。
- **Red確認:** runtime writer spyを使う製品経路testが、残存するhandler出力の直接writeを1件検出することだけを理由に失敗する。`DisplayModel`自体はCommit 7からGreenである。

### Commit 23: `CLI: rendererへの出力境界を分離する`

- **固定する契約:** 残っている全handler出力とerror、flush、broken pipe分類をrenderer境界へ完全移行する。handlerは実writerへ書かず、raw fragmentを保持する既存の`DisplayRecorder`へ書き、runtimeが`DisplayModel`を`render_display_model`へ渡す。
- **対象:** `renderer.rs`、`handler.rs`、runtimeの残存出力接続。依存方向を`runtime -> handler / renderer`、`handler -> command / renderer`とし、`handler -> runtime`を禁止する。
- **依存:** Commit 22。
- **Green確認:** 表示とwriter契約test、全品質ゲート。意味的表示modelへの全面移行は行わない。

### Commit 24: `Test: interactive共通実行経路を固定する`

- **固定する契約:** submitとrefreshが共通parser・handler・renderer経路を通り、focus、Ctrl-C、Ctrl-D、切断、read error、raw mode開始前errorを維持する。
- **対象:** interactive driverとruntime調停test。
- **依存:** Commit 23。
- **Red確認:** driverとruntime調停のtestは共通の`interactive::run`入口を呼び、この入口が存在しないことだけを理由にcompile errorで失敗させる。parser・handler・rendererの既存testはGreenのまま維持する。

### Commit 25: `CLI: interactive terminal driverを分離する`

- **固定する契約:** `termion`、raw mode、入力thread、cursor、refresh timer、prompt編集を`interactive.rs`へ隔離する。
- **対象:** `interactive.rs`を追加し、runtimeへ接続する。
- **依存:** Commit 24。
- **Green確認:** interactive共通経路と終了・error契約test、全品質ゲート。

### Commit 26: `Test: CLI runtimeの外部I/O境界を固定する`

- **固定する契約:** interactive / non-interactiveの両経路が共通の`apply_command_outcome` runtime調停関数を使い、browser / focus要求、`CommandError::ExternalOpen`、repository transaction、reload / saveの既存分類と実行時点を維持する。
- **対象:** `apply_command_outcome`を通るinteractive / non-interactiveのruntime調停test。
- **依存:** Commit 25。
- **Red確認:** 両経路の契約testは共通の`apply_command_outcome`を直接呼び、この関数が未導入であることだけを理由にcompile errorで失敗させる。各既存経路によるbrowser / focus要求の個別実行はCommit 7からGreenのまま維持する。

### Commit 27: `CLI: 外部I/Oとtransactionをruntimeへ集約する`

- **固定する契約:** Commit 7から各runtime経路が個別に担うoutcome解釈を共通の`apply_command_outcome`へ集約し、browser error reason、transaction、reload / saveの分類と実行時点を維持する。
- **対象:** `runtime.rs`のinteractive / non-interactive個別処理を共通調停関数へ置き換え、browser実装、focus変更、repository transactionを同じruntime境界から呼ぶ。`handler.rs`の構造化要求契約は変更しない。
- **依存:** Commit 26。
- **Green確認:** runtime外部I/O境界test、interactive / non-interactive保存時点test、全品質ゲート。

### Commit 28: `Docs: TD-005の完了を記録`

- **固定する契約:** TD-005の対応内容、検証結果、完了状態を履歴として残す。
- **対象:** `backlog.md`のTD-005項目。
- **依存:** Commit 27および最終品質ゲート。
- **確認:** `git diff --check`と文書内容のself-review。

### Commit 29: `Docs: CLI分割の残存負債を記録`

- **固定する契約:** 今回実装しない残存負債を完了範囲と混同しない。
- **対象:** `backlog.md`のTD-018へruntime縮小、command orchestration、意味的表示modelを記録し、TD-015へruntime内test fixture / helperの`test_support`分離を記録する。
- **依存:** Commit 28。
- **確認:** TD-018 / TD-015の製品実装が差分に含まれないこと、`git diff --check`、文書内容のself-review。

## Red / Green・review手順

各契約は次の順序で進める。

1. 期待する挙動を示すRed testを追加する。
2. 対象testを実行し、期待した1つの理由でRedになることを確認する。
3. Red testだけをcommitする。
4. 最小限の製品コードでGreenにする。
5. 対象testを実行する。
6. 全品質ゲートを実行する。
7. サブエージェントでreviewする。
8. 基礎Green実装をcommitする。
9. review指摘を1件ずつ修正し、対象testを実行して個別commitにする。
10. 全品質ゲートを再実行する。

reviewでは、既存testの維持、新規testが製品経路を通ること、adapter / application / entityの依存方向、typed fieldの直接参照、I/O隔離、Spreadsheet列、表示互換、commitの単一目的性を確認する。P0 / P1が残るphaseは完了扱いにしない。判断が難しい指摘は保留一覧へ記録し、安全な指摘の修正を止めない。

## 品質ゲート

各Green commit前に次を実行する。

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
git diff --check
```

各commit後に次を実行する。

```bash
git status --short --branch
git show --stat --oneline HEAD
```

- 対象testを先に実行してよいが、基礎Green実装commit前には全品質ゲートを通す。
- 差分が機械的移動を除いて800行を大きく超える場合、commit前に責務単位の再分割を検討する。
- 機械的移動と挙動変更、formattingと挙動変更、documentationと製品実装を同じcommitへ混ぜない。
- 各commitは目的を1文で説明でき、単独でrevert可能な状態にする。明示的なRed test commitを除き、各commitをGreenに保つ。
- commit後に差分量と責務を確認し、意図より大きい場合は次へ進む前に分割をやり直す。

## 最終検証と完了報告

- 最終品質ゲートを新しい出力で再実行し、`superpowers:verification-before-completion`に従って結果を確認する。
- 旧branchとの差分は`src/adapter/controller/schronu.rs`、`src/adapter/controller/schronu/`、`backlog.md`に限定して読み取り専用で比較し、構造差、旧branch不具合修正、formatting、欠落のいずれかへ分類する。
- commit履歴をreviewし、Red testとGreen実装の対応、機械的移動の独立、review修正の指摘単位、documentation分離、各commitの単一目的性を確認する。
- 完了報告には最終品質ゲート、全commit一覧、review結果、旧branchとの差分分類、TD-005 / TD-018 / TD-015の状態、保留指摘を含める。
- TD-018、意味的表示modelへの全面移行、runtime内test fixture / helperのTD-015対応は残存負債として記録するだけで、今回の製品実装へ追加しない。
