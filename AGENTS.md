# Repository Guidelines

このドキュメントは Schronu リポジトリの貢献者向けガイドです。短く、具体的、再現可能な変更を心がけてください。

## プロジェクト構成
- 主要言語: Rust (`cargo`)
- コード: `src/`(レイヤ: `adapter/`, `application/`, `entity/`)
- バイナリ: `[[bin]] path = src/adapter/controller/schronu.rs`
- スクリプト: `shell/`
- アセット/ドキュメント: `README.md`, `LICENSE`

例: エンティティは `src/entity/*.rs`、アダプタは `src/adapter/**` に配置。

## ビルド・実行・テスト
- ビルド: `cargo build --release`
- 実行: `cargo run --bin schronu -- <args>`
- テスト: `cargo test`
- 静的解析: `cargo clippy --all-targets -- -D warnings`
- フォーマット: `cargo fmt` / 検査は `cargo fmt --check`

## コーディング規約
- インデントは 4 スペース、`rustfmt` 準拠。
- 命名: 型は `UpperCamelCase`、関数/変数/モジュール/ファイルは `snake_case`、定数は `SCREAMING_SNAKE_CASE`。
- 公開 API は最小限に。`adapter` では副作用を隔離、`entity` は純粋なドメインロジックを維持。

## テスト指針
- フレームワーク: 標準の Rust テスト(`#[test]`)。
- 配置: 単体は各ファイルの `mod tests`、結合は `tests/` ディレクトリ(必要に応じて作成)。
- 目標: 変更行を中心にカバレッジを確保。再現手順と期待値を明記。
- 実行例: `cargo test -q entity::task`(モジュール単位の絞り込み)。

## コミット & PR ガイドライン
- コミット: 短い要約(命令形、約 50 文字)。必要なら本文に背景/方針/影響範囲を箇条書き。
- 例: `Task: 親子タスクの初期日付ずれを修正`
- PR: 目的、変更点、テスト方針、互換性、関連 Issue(`#123`)を記載。`cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` を通過させること。

## セキュリティ/設定
- 秘密情報はコミットしない(環境変数で注入)。
- ファイル書き込みは必要最小限のパスに限定。外部コマンド実行時は引数検証を徹底。

## Agent 向け指示
- 英語で思考し、日本語で表示してください。
- "TODO" という言葉はタスクのステータスで使うので、今後実装するという意味合いのコメントとしては"FIXME"を使ってください。
- カッコなどは半角記号を用いてください。ただし「」と【】は許容します。
- `busy_time_slot` は、毎週定期的に時間固定で発生する行動不能時間を表すためのものです。単発の予定や外出予定を表す用途には使わないでください。
- 複数箇所で同じ helper が必要になった場合は、まず共通化先を検討してください。安易にファイルごとの同名 helper を増やさず、適切な層(例: `entity`)に1つ置いて参照してください。
- deprecated API の置換では、戻り値の情報量を落とさないでください。`Result` を `Option` に潰すなど、エラー理由や分岐情報を失う変更は避け、呼び出し側で無視する場合も型として情報を保持してください。
- 標準ライブラリや利用中のクレートの型で意味を表せる場合は、自前の wrapper 型を増やさないでください。例: chrono のローカル時刻変換では `LocalResult` を使い、`Single` のみ採用するなど呼び出し側で明示してください。
- 未使用 warning に対応するときは、値を保持する意図や将来使う意図があるものを安易に削除しないでください。意図的に未使用である場合は、仮引数や private field を `_` 始まりの名前にして明示してください。
- `全` コマンドのタスク行や Spreadsheet 連携の列構成を変更する場合は、連動して `src/adapter/controller/schronu.rs` の出力フォーマット、`shell/copy_for_spreadsheet.sh` の取り込み列数と計算列、`shell/generate_command_from_spreadsheet.sh` の読み取り列番号と列名付きエラー、`apps_script/main.js` の `syncCols` と `timeFormatRanges`、`apps_script/README.md` と `README.md` の説明を確認してください。現在の `全` 出力は Spreadsheet のA-J列相当で、I列が `category`、J列が `task_name` です。Spreadsheet 側の主要列はP列が完了時刻、Q列が抽出対象、S列が実作業時間、L/N/P/R列が同期対象です。

### Red/Green、commit分割、レビューの進め方

backlogの1項目を、そのまま1つの実装commit単位として扱わない。

backlog項目が複数の責務、module、境界を含む場合は、実装前に独立して検証・reviewできる単位へ分割する。ここでいう「機能」はbacklog項目全体ではなく、parser、handler、renderer、gateway、use caseなど、1つの契約または変更理由を持つ単位を指す。

#### 実装前のcommit計画

コードへ着手する前に、計画へ想定commit一覧を記載する。

各commitについて次を明記する。

- commit message
- 固定する契約
- 変更対象の責務またはmodule
- 先行commitへの依存
- 対象test
- Green確認方法

次のいずれかに該当する場合、1commitへまとめず分割する。

- 複数の責務を変更する
- parser、handler、renderer、runtime、gatewayなど複数の境界を同時に導入する
- 機械的なファイル移動と挙動変更が混在する
- 製品コードの変更理由を1文で説明できない
- review時に一部分だけrevertできない
- Redになる理由が複数ある
- 差分の大半を読まなければ1つの契約を確認できない

`git diff --stat`で差分量を確認し、機械的移動を除いて変更が800行を大きく超える場合は、commit前に分割可能性を再検討する。800行以下なら適切という意味ではなく、責務が1つであることを優先する。

例外的に大きなcommitが必要な場合は、次をすべて満たすこと。

- 内容が機械的な移動または自動生成だけである
- 挙動変更を含まない
- 移動前後で全testがGreenである
- 後続の挙動変更commitと分離されている
- commit本文へ大きくなる理由を記載する

#### 機械的移動と挙動変更

ファイル移動、module分割、rename、formattingと、挙動変更を同じcommitへ混ぜない。

推奨順序:

1. 現行挙動を固定するcharacterization test
2. 挙動を変えない機械的移動
3. 新しい境界のRed test
4. 最小のGreen実装
5. 旧経路の除去
6. cleanup

機械的移動commitでは、可能な限り実装内容を変更しない。移動と同時に型変更、error変更、公開API変更を行わない。

#### Red/Green cycle

各契約は次の単位で進める。

1. 期待する挙動を示すRed testを追加する
2. 対象testを実行し、期待した1つの理由でRedになることを確認する
3. Red testだけをcommitする
4. 最小限の製品コードでGreenにする
5. 対象testを実行する
6. 全品質ゲートを実行する
7. サブエージェントでreviewする
8. 基礎Green実装をcommitする
9. review指摘を1件ずつ修正・test・commitする
10. 全品質ゲートを再実行する

Red test commitもbacklog項目全体で1つにまとめない。parser、handler、rendererなど失敗理由が異なる契約は、それぞれ独立したRed/Green cycleにする。

明示的なRed test commitを除き、各commitはbuild・test可能なGreen状態にする。途中状態をGreenにできない場合は、巨大commitで回避せず、依存順序またはvertical sliceを見直す。

#### 基礎Green実装の定義

「基礎Green実装」はbacklog項目全体ではなく、直前のRed testが示す1つの契約を満たす最小実装を指す。

例えばCLI分割では、次を別々の基礎Green実装として扱う。

- entrypointの機械的分離
- typed parser
- typed handler
- renderer境界
- Spreadsheet formatter
- interactive driver
- transaction・外部I/O調停

これらすべてを1つの「基礎Green実装」commitへまとめない。

#### Review前後のcommit

review前に、review対象となる基礎Green実装をcommitする。複数の独立した機能を未commitのまま溜めてからreviewへ渡さない。

ただし、reviewで基礎設計そのものが完了条件を満たさないと判明した場合は、その機能をGreen完了扱いにしない。必要な修正も責務単位で分け、1つの巨大な差分へ吸収しない。

review指摘は次のように扱う。

- 1指摘だけ修正する
- 関連する対象testを実行する
- 指摘内容に対応する個別commitを作る
- 複数指摘を同じcommitへまとめない
- 判断が難しい指摘は保留一覧へ記録する
- 保留指摘を理由に、安全な指摘の修正を止めない

実装commit前に複数の重大な問題を発見した場合も、すべてを1commitへ畳み込まない。契約ごとのGreen状態を作り、独立したcommitとして残す。

#### 各commitの確認

commit前に次を確認する。

- このcommitの目的を1文で説明できる
- 変更理由が1つである
- 無関係なmodule変更がない
- 機械的移動と挙動変更が混在していない
- 既存testを削除・緩和・意味変更していない
- 新規testが製品経路を通る
- test専用分岐や不要な公開APIがない
- adapter・application・entityの依存方向を悪化させていない
- このcommitだけをrevertできる
- `git diff --check`が成功する

Green commitでは原則として次を実行する。

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

対象testを先に実行してよいが、基礎Green実装commit前には全品質ゲートを通す。

commit後に次を確認する。

```bash
git status --short --branch
git show --stat --oneline HEAD
```

意図より差分が大きい場合や、複数責務が含まれている場合は、次の作業へ進む前にcommit分割をやり直す。

#### 最終履歴レビュー

backlog項目を完了にする前に、コードだけでなくcommit履歴もreviewする。

確認項目:

- Red testとGreen実装が契約単位で対応している
- backlog項目全体が1つの巨大実装commitになっていない
- 機械的移動が独立している
- review修正が指摘単位で分かれている
- documentation変更が製品実装と分かれている
- 各commitの目的がmessageと差分から判断できる
- 各Green commitで品質ゲートを通した記録がある

履歴がreview不能な粒度になっている場合は、未mergeかつ安全に履歴を再構成できる段階でcommitを分割する。既に共有済みのbranchを書き換える場合は、backup branchを作り、force-pushの影響を明示してから行う。承認なしに共有branchをforce-pushしない。

#### レビュー観点

- それ以前のtestが削除・緩和・改変されていない
- 新規testが妥当な契約を表現している
- testが製品経路を通っている
- testを通すためだけの分岐や公開APIがない
- adapter・application・entityの依存方向が維持されている
- commitが単一目的でreview可能である
- 大規模な移動が実質的な責務分離に見せかけられていない
- 新しいmodule名と実際の責務が一致している
- 旧moduleから新moduleへ負債を移動しただけになっていない
- 完了条件を満たさない残存作業がある場合、完了扱いにせず別項目または残存負債として明示されている

判断が難しい指摘は作業を止める理由にせず、保留一覧へ記録する。安全な修正を完了してから、リスク・選択肢・推奨案をまとめて判断する。
