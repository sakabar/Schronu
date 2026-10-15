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
- 秘密情報をcommitせず、file書き込みを必要最小限のpathへ限定し、外部commandの引数を検証する。

## Agent固有規則

- 英語で思考し、日本語で表示する。
- `TODO`はタスクのステータス専用とし、将来実装する意味のcommentには`FIXME`を使う。
- カッコなどは半角記号を使う。ただし「」と【】は許容する。
- test caseを作成する際は、test関数名、fixtureのtask名・予定名、期待値に個人の生活を特定できる名称を使わず、検証する契約を表す中立的な名称にする。
- `busy_time_slot`は毎週定期的に時間固定で発生する行動不能時間だけを表す。単発の予定や外出予定には使わない。
- 同じhelperが複数箇所で必要なら共通化先を先に検討し、適切な層(例: `entity`)に1つ置く。
- deprecated APIの置換では戻り値の情報量を落とさない。`Result`を`Option`に潰すなど、error理由や分岐情報を失う変更を避ける。
- 標準libraryや利用中crateの型で意味を表せる場合はwrapper型を増やさない。chronoのlocal時刻変換では`LocalResult`を使い、呼び出し側で`Single`だけを採用する。
- 意図的に未使用の仮引数やprivate fieldは`_`始まりにする。保持や将来利用の意図がある値をwarning対応だけで削除しない。
- `全`commandのtask行またはSpreadsheet列を変える場合は、controller出力、`shell/copy_for_spreadsheet.sh`、`shell/generate_command_from_spreadsheet.sh`、`apps_script/main.js`、`README.md`を連動確認する。列定義の正本は[spreadsheet_columns.tsv](spreadsheet_columns.tsv)、詳細は[apps_script/README.md](apps_script/README.md)とする。
- `schronu-web`のUIを変える場合は[UI要件](docs/design/schronu_web_ui_requirements.md)と[UI実装仕様](docs/design/schronu_web_ui_specification.md)の該当箇所を確認し、影響するコード、test、記述を一致させる。

## Worktree build cache

- `target/`を分岐元からcopyせず、分岐元checkoutのabsolute pathを設定やcommandへ埋め込まない。
- Local environmentのsetupでは`cargo fetch --locked`だけを実行し、全worktree共通のcompileや`dx build`を行わない。
- `schronu-web`の製品コードを変更し、WASM buildが必要な場合は、作業範囲の確定後に次のcheckをbackgroundで先行実行してよい。

```bash
cargo check --locked -p schronu-web --no-default-features --features web --target wasm32-unknown-unknown
```

- background build中は同じworktreeで別のCargo commandを実行せず、終了codeと出力を必ず回収する。
- 先行checkはcache warmup専用とし、変更後の検証の代わりにしない。
- `~/.cargo/bin/dx build --locked --web --package schronu-web`はsetupやprewarmでは実行せず、Web製品コード変更の最終品質gateで1回だけ実行する。

## 開発workflow

### 通常の変更

- 変更する契約と影響範囲を確認し、必要なコード、既存test、文書だけを読む。複数の責務や境界にまたがる変更は、独立して検証・review・revertできる単位に分ける。
- 挙動を変える場合は製品経路を通る回帰testを追加または更新する。可能なら修正前に期待した理由で失敗することを確かめ、修正後に対象testを再実行する。旧契約を肯定する既存testも探し、維持・更新・削除を理由とともに判断する。
- 変更したコードと影響範囲を検証する。Rustコードでは通常`cargo fmt --check`、対象test、変更packageのtest・clippyを使う。共有契約や複数packageへ影響する場合はroot全体のgateを使う。文書だけの変更にはRustのbuild・testを要求しない。
- testやreviewで見つかった、今回の変更に起因する失敗を修正して再検証する。最初の実装だけで完了とせず、依頼された成果と検証結果を報告する。

root全体のgateが必要な場合のcommandは次のとおり。Web固有の検証は[README.md](README.md)の「schronu-web」を参照する。

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

### Commitとreview

- commitは1つの変更理由を説明でき、単体でrevertできる単位にする。機械的な移動・rename・formattingは挙動変更と分ける。Red test専用commitや指摘ごとのcommitは必須としない。
- commit前に受入条件ごとの変更理由を列挙し、各変更を単独でrevertしても他の受入条件を維持できるか確認する。独立してrevertできる変更は、同じ依頼・計画・PRに含まれていても別commitに分け、対応するtestと文書を各commitへ含める。機能削除と保存済みstateの復元仕様変更も別の変更理由として判定する。
- commit前に`git diff --check`と差分を確認する。既存testを理由なく削除・緩和したり、test専用分岐や不要な公開APIを製品コードへ加えたり、層の依存方向を悪化させたりしない。
- 差分の規模とリスクに応じてreviewを行う。公開API、schema、security、transaction、外部I/Oなどの境界を変える場合は、その契約と失敗経路を重点的に確認する。
- 大きなfile、fixtureやmockの肥大化、重複する検証・error変換、1moduleへの責務集中を見つけたら構造を再検討する。行数だけを停止条件にせず、分割の要否と残存負債を説明する。
- PRには目的、変更点、検証結果、互換性、関連Issueを記載する。公開前にはbranchの累積差分を確認する。履歴の再構成は未共有branchだけを対象とする。共有済みbranchではbackup branchを作り、影響を説明し、承認なしにforce-pushしない。

`backlog.md`のWaveを統括する場合は[専用skill](.codex/skills/orchestrating-backlog-waves/SKILL.md)の厳格な計画、Red/Green、review、品質gate、公開手順に従う。
