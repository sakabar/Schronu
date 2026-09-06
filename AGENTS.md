# Repository Guidelines

Schronu リポジトリでは、短く、具体的、再現可能な変更を心がける。

## 概要と基本コマンド

- 主要言語: Rust (`cargo`)
- コード: `src/`(`adapter/`, `application/`, `entity/`)
- バイナリ: `src/adapter/controller/schronu.rs`
- スクリプト: `shell/`
- ドキュメント: `README.md`, `docs/`, `apps_script/README.md`

```bash
cargo build --release
cargo run --bin schronu -- <args>
cargo test -q entity::task
```

## 実装規則

- Rustコードは4スペース、`rustfmt`準拠とする。
- 型は`UpperCamelCase`、関数・変数・module・fileは`snake_case`、定数は`SCREAMING_SNAKE_CASE`とする。
- 公開APIは最小限にし、`adapter`で副作用を隔離し、`entity`を純粋なdomain logicに保つ。
- 単体testは各fileの`mod tests`、結合testは必要に応じて`tests/`へ置く。変更した契約を製品経路で検証する。
- commit messageは命令形の短い要約(約50文字)とし、必要なら本文へ背景・方針・影響範囲を書く。
- PRには目的、変更点、test方針、互換性、関連Issueを記載する。
- 秘密情報をcommitせず、file書き込みを必要最小限のpathへ限定し、外部commandの引数を検証する。

## Agent固有規則

- 英語で思考し、日本語で表示する。
- `TODO`はタスクのステータス専用とし、将来実装する意味のcommentには`FIXME`を使う。
- カッコなどは半角記号を使う。ただし「」と【】は許容する。
- `busy_time_slot`は毎週定期的に時間固定で発生する行動不能時間だけを表す。単発の予定や外出予定には使わない。
- 同じhelperが複数箇所で必要なら共通化先を先に検討し、適切な層(例: `entity`)に1つ置く。
- deprecated APIの置換では戻り値の情報量を落とさない。`Result`を`Option`に潰すなど、error理由や分岐情報を失う変更を避ける。
- 標準libraryや利用中crateの型で意味を表せる場合はwrapper型を増やさない。chronoのlocal時刻変換では`LocalResult`を使い、呼び出し側で`Single`だけを採用する。
- 意図的に未使用の仮引数やprivate fieldは`_`始まりにする。保持や将来利用の意図がある値をwarning対応だけで削除しない。
- `全`commandのtask行またはSpreadsheet列を変える場合は、controller出力、`shell/copy_for_spreadsheet.sh`、`shell/generate_command_from_spreadsheet.sh`、`apps_script/main.js`、`README.md`を連動確認する。列定義の正本は[spreadsheet_columns.tsv](spreadsheet_columns.tsv)、詳細は[apps_script/README.md](apps_script/README.md)とする。

## 開発workflow

### 経路選択

1つの契約または変更理由を1つのcycleとして扱う。複数の責務や境界を含むbacklog項目は、独立して検証・review・revertできる単位へ分ける。

次をすべて満たす場合だけfast pathを使う。判断に迷う場合や、途中で条件を外れた場合は標準経路へ切り替える。

- 製品コードの変更理由と固定する契約を、それぞれ1文で説明できる。
- 製品コードが3file以下で、機械的変更を除く追加・削除の合計が100行以下である。
- storage schema、wire format、公開API、security境界、transaction、並行制御、外部I/O protocolを変えない。
- file移動、module分割、dependency追加、migrationを含まない。
- 既存test構造の延長で回帰を固定できる。

### Fast path

1. 回帰testを追加し、対象testが期待した1つの理由でRedになることを確認する。Red test単独commitは必須としない。
2. 最小実装でGreenにする。同じ利用者向け契約を保つ小さなvertical sliceは層ごとに分割しなくてよい。
3. `cargo fmt --check`、対象test、変更packageのtestとclippyを実行する。
4. 関連test、実装、契約に直結するdocumentationを1つのGreen commitにまとめてよい。
5. 契約全体がGreenになった後、サブエージェントreviewを1回実施する。
6. 同じ原因と契約に属し、個別revertの意味がない指摘はまとめて修正し、対象testを再実行する。

root全体の品質gateは、PR作成または実装完了報告の直前に1回実行する。

### 標準経路

実装前にcommit計画を作り、各commitのmessage、契約、責務・module、依存、対象test、Green確認方法を明記する。

各契約を次の順序で進める。

1. 期待する挙動を示すRed testを追加する。
2. 対象testを実行し、期待した1つの理由でRedになることを確認する。
3. Red testだけをcommitする。
4. 最小実装で対象testをGreenにする。
5. 全品質gateを実行する。
6. 基礎Green実装をcommitする。
7. commit済みの基礎Green実装をサブエージェントでreviewする。
8. 指摘を1件ずつ修正し、関連testを実行して個別commitにする。
9. 全品質gateを再実行する。

「基礎Green実装」は、直前のRed testが示す1つの契約を満たす最小実装を指す。parser、handler、renderer、gateway、use case、transaction・外部I/O調停など、独立した境界を一括りにしない。

### 共通commit規則

次に該当する変更はcommitを分ける。

- 複数の責務、境界、変更理由またはRedになる理由がある。
- 一部分だけを安全にrevertできない。
- 差分の大半を読まなければ契約を確認できない。
- file移動、module分割、rename、formattingなどの機械的変更と挙動変更が混在する。

機械的移動は挙動変更と分離し、移動と同時に型、error、公開APIを変えず、移動前後で全testをGreenに保つ。例外的な大規模commitは、機械的移動または自動生成だけで、挙動変更がなく、後続の変更と分離できる場合に限り、その理由をcommit本文へ記載する。

Red test commitを除く各commitはbuild・test可能なGreen状態にする。commit前に次を確認する。

- 目的と変更理由を1文で説明でき、無関係なmodule変更がない。
- 既存testを削除・緩和・意味変更していない。
- 新規testが製品経路を通り、test専用分岐や不要な公開APIを導入していない。
- `adapter`、`application`、`entity`の依存方向を悪化させていない。
- commit単体でrevertでき、`git diff --check`が成功する。

reviewで完了条件を満たさないと判明した変更はGreen完了扱いにしない。判断が難しい指摘は保留一覧へ記録し、安全な修正を止めない。将来へ残す負債は影響と回避策をbacklogへ記録し、userの明示承認を得る。

## 品質gateとPR前確認

標準経路のGreen commit前と最終確認では、次を実行する。fast pathでは前述のpackage単位検証後、最終確認時に1回実行する。

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

commit後は次を確認する。

```bash
git status --short --branch
git show --stat --oneline HEAD
```

契約群の完了時、変更が計画より増えた時、PR作成、backlog完了、または「実装完了」「PR作成可能」と報告する前にbranch累積差分を確認する。

```bash
git diff --stat main...HEAD
git diff --numstat main...HEAD
git diff --name-only main...HEAD
```

累積差分について、責務境界、依存方向、状態遷移、I/Oとerrorの所有場所、test helperの共通化、残存負債を説明できる状態にする。次に該当する場合は実装を止め、module・責務・test構造の分割を再検討する。

- 新規または大幅拡張した1fileが800行を超える、または1fileへの累積追加が800行を超える。
- fixtureやmock準備がtest本体より支配的である。
- 1moduleで複数の独立した状態遷移やI/O protocolを追う必要がある。
- `allow(clippy::too_many_arguments)`、多数の`expect`、同型の`map_err`やmockなど、構造上の圧力が増えている。
- 計画外の補助責務、互換処理、security処理、cleanupが同じmoduleへ累積している。

800行は不合格基準ではなく設計再検討の閾値とする。分割しない場合は責務が1つである根拠、代替案、保守方法、残存負債を報告し、userの明示承認を得る。

最終reviewでは、正しさに加えて次を確認する。

- module名と責務が一致し、同じ検証、path計算、error変換、fixtureが重複せず、旧moduleの負債を移動しただけになっていない。
- testの網羅性と変更容易性を保ち、行数削減のために可読性、error情報、failure coverageを落としていない。
- Red testとGreen実装、review修正が契約単位で対応し、機械的移動が独立している。
- 各commitの目的と検証記録が判断でき、巨大module、巨大fixture、責務集中が残っていない。

履歴を再構成する場合、未共有branchだけを対象とする。共有済みbranchではbackup branchを作り、影響を説明し、承認なしにforce-pushしない。
