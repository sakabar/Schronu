# Repository Guidelines

このドキュメントは Schronu リポジトリの貢献者向けガイドです。短く、具体的、再現可能な変更を心がけてください。

## プロジェクト構成
- 主要言語: Rust (`cargo`)
- コード: `src/`（レイヤ: `adapter/`, `application/`, `entity/`）
- バイナリ: `[[bin]] path = src/adapter/controller/schronu.rs`
- スクリプト: `shell/`
- アセット/ドキュメント: `README.md`, `LICENSE`

例: エンティティは `src/entity/*.rs`、アダプタは `src/adapter/**` に配置。

## ビルド・実行・テスト
- ビルド: `cargo build --release`
- 実行: `cargo run --bin schronu -- <args>`
- テスト: `cargo test`
- 静的解析: `cargo clippy -- -D warnings`
- フォーマット: `cargo fmt` / 検査は `cargo fmt --check`

## コーディング規約
- インデントは 4 スペース、`rustfmt` 準拠。
- 命名: 型は `UpperCamelCase`、関数/変数/モジュール/ファイルは `snake_case`、定数は `SCREAMING_SNAKE_CASE`。
- 公開 API は最小限に。`adapter` では副作用を隔離、`entity` は純粋なドメインロジックを維持。

## テスト指針
- フレームワーク: 標準の Rust テスト（`#[test]`）。
- 配置: 単体は各ファイルの `mod tests`、結合は `tests/` ディレクトリ（必要に応じて作成）。
- 目標: 変更行を中心にカバレッジを確保。再現手順と期待値を明記。
- 実行例: `cargo test -q entity::task`（モジュール単位の絞り込み）。

## コミット & PR ガイドライン
- コミット: 短い要約（命令形、約 50 文字）。必要なら本文に背景/方針/影響範囲を箇条書き。
- 例: `Task: 親子タスクの初期日付ずれを修正`
- PR: 目的、変更点、テスト方針、互換性、関連 Issue（`#123`）を記載。`cargo fmt --check && cargo clippy && cargo test` を通過させること。

## セキュリティ/設定
- 秘密情報はコミットしない（環境変数で注入）。
- ファイル書き込みは必要最小限のパスに限定。外部コマンド実行時は引数検証を徹底。

## Agent 向けメモ
- 英語で思考し、日本語で表示してください。
