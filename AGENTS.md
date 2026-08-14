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

### Red/Greenとレビューの進め方

各機能は次の単位で進める。

1. 期待する挙動を示すRedテストを追加
2. Redになる理由を確認
3. Redテストだけをcommit
4. 最小限の製品コードでGreenにする
5. 全検証を実行
6. サブエージェントでレビュー
7. 基礎Green実装をcommit
8. リスクなく直せる指摘は、1件ずつ次を行う
   - 1指摘だけ修正
   - 関連する対象テストを実行
   - 指摘内容に対応する個別commitを作成
9. 全検証を再実行

レビュー観点:

- それ以前のテストが削除・緩和・改変されていない
- 新規テストが妥当な契約を表現している
- テストが製品経路を通っている
- テストを通すためだけの分岐や公開APIがない
- adapter・application・entityの依存方向が維持されている

判断が難しい指摘は作業を止める理由にせず、保留一覧へ記録する。安全な修正を完了してから、リスク・選択肢・推奨案をまとめて判断する。
