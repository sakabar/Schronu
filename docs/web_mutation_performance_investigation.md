# Webの記録・完了・先送りの待ち時間調査

2026-11-07、基準commit `b9f2327e`。製品コードの変更前に実施した調査です。

## 方法

`tests/web_mutation_performance.rs`で、実際の`WebService`とファイルrepositoryを使い、
記録・完了・先送りそれぞれの成功後に予定一覧、負荷帯を順番に取得します。
実データや稼働中のserverには接続しません。

```sh
TZ=Asia/Tokyo cargo test --locked --release --test web_mutation_performance -- --ignored --nocapture
```

- Rust 1.97.1、Darwin arm64、release、`Asia/Tokyo`。
- 既存の固定seed付き`SchedulingFixture`を再利用し、独立した操作対象を1件追加します。
- smallは7 task / 3 project、typicalは26,379 task / 2,214 project、stressは105,513 task / 8,853 projectです。完了済み、未完了、保留、階層、締切、atomic taskを含みます。
- 各規模の初期データを一時directoryへ製品repositoryで保存し、各sampleへ複製します。初期保存・複製・削除は計測外です。正常終了時に一時データを削除します。
- sampleごとに新しいserviceを作り、bootstrap・予定一覧・負荷帯の初回取得を終えてから計測します。各操作は同じ初期データから3回測ります。
- `mutation_ms`は読込・更新・snapshot計算・保存を含み、`list_ms`は直後の予定一覧取得、`load_ms`は続く負荷帯・繰返負荷集計です。`total_ms`は3区間の和です。
- 比較用に変更なしの一覧取得、別repositoryでの全件ロード、ロード済みrepositoryでのschedule計算を測ります。これらは`total_ms`に含めません。

## 修正前の実測結果

各3回の中央値、単位はmsです。各列を独立に集計するため、区間の中央値の和と合計の中央値は必ずしも一致しません。

| 規模 | 操作 | 更新 | 一覧再取得 | 負荷集計 | 合計 |
| --- | --- | ---: | ---: | ---: | ---: |
| Small | 記録 | 54.1 | 7.7 | 5.5 | 67.3 |
| Small | 完了(実績記録あり) | 66.6 | 7.4 | 6.3 | 82.4 |
| Small | 先送り | 52.1 | 7.0 | 5.6 | 64.0 |
| Typical | 記録 | 751.7 | 797.8 | 31.1 | 1580.6 |
| Typical | 完了(実績記録あり) | 773.9 | 807.0 | 35.2 | 1617.6 |
| Typical | 先送り | 769.4 | 808.5 | 34.7 | 1611.9 |
| Stress | 記録 | 2990.1 | 3140.2 | 135.0 | 6284.3 |
| Stress | 完了(実績記録あり) | 3437.6 | 3232.6 | 134.2 | 6804.4 |
| Stress | 先送り | 2965.1 | 3084.5 | 126.1 | 6178.3 |

対照測定も各3回の中央値、単位はmsです。全件ロードは別repositoryで測った独立の対照値であり、更新時間から直接抽出した内訳ではありません。

| 規模 | 操作後 | 変更なしの一覧 | 全件ロード | schedule計算 |
| --- | --- | ---: | ---: | ---: |
| Small | 記録 | 6.3 | 1.5 | 0.1 |
| Small | 完了(実績記録あり) | 6.1 | 1.0 | 0.0 |
| Small | 先送り | 5.1 | 0.9 | 0.0 |
| Typical | 記録 | 29.3 | 647.6 | 22.2 |
| Typical | 完了(実績記録あり) | 33.9 | 681.0 | 23.3 |
| Typical | 先送り | 34.8 | 702.4 | 23.5 |
| Stress | 記録 | 131.9 | 2877.7 | 102.8 |
| Stress | 完了(実績記録あり) | 133.4 | 2934.0 | 104.9 |
| Stress | 先送り | 121.2 | 2782.6 | 96.8 |

Typicalの操作全体は約1.6秒、Stressは約6.2〜6.8秒でした。Typicalで負荷集計は約31〜35msに対し、更新と一覧再取得はそれぞれ約0.75〜0.81秒です。全件ロード単独は約0.65〜0.70秒、変更なしの一覧は約29〜35msであり、下記の二重ロード経路と整合します。負荷集計を背景へ移すだけでは、この合成データの待ち時間の大部分は残ります。

更新直後の一覧と変更なしの一覧の差は改善余地の目安であり、そのまま改善保証値にはできません。

検証: 全27 sampleを完走し、ignored testの明示実行は1 passed / 0 failedでした。fixture準備・削除を含む実行全体は660.22秒です。

## 修正前のコード上の原因

1. `schronu-web/src/client/state/session_state.rs`の記録・完了成功と、`read_state.rs`の先送り成功は、`apply_mutation_snapshot_and_request_list`から一覧再取得へ進みます。一覧応答後には負荷取得が続きます。
2. `src/adapter/controller/schronu/web_service.rs`の`run_mutation_at`は、毎回新しい`TaskRepository`を作ります。`run_repository_transaction`の`reload_if_changed`で全件ロードし、更新・snapshot計算後に保存します。
3. 成功した更新用repositoryはserviceの読取用repositoryへ引き継がれません。保存でrevisionが変わるため、後続の一覧取得でも`reload_if_changed`が全件ロードします。
4. `src/adapter/gateway/task_repository.rs`の`load_state`は全`project.yaml`を列挙して読み込みます。YAML解析、task tree再構築、UUID重複検査・索引作成も含みます。単純なファイル転送時間だけではありません。

## 初回調査での改善の優先順位

1. **更新成功後のrepositoryを読取cacheへ引き継ぎ、直後の一覧再取得での全件ロードをなくす。** 独立した更新用repositoryによる失敗時の隔離は維持できます。保存失敗・保存結果不明時には引き継がず、外部CLI/MCPの更新は従来どおりrevisionで検出する必要があります。
2. 更新前の全件ロードについて、失敗時に未保存の変更を再利用しない仕組みを維持しながら、読込済みstateの再利用または差分ロードを検討します。これは1と別の変更理由です。
3. 負荷集計の背景更新と「保存中」「一覧更新中」の表示分離は操作感の改善として検討します。今回の合成データでは全件ロードの改善を先に評価します。

## 結果の範囲

これはserverの製品service経路の測定であり、HTTP、workerの順番待ち、JSON転送、WASM、DOM描画は含みません。OSのfile cacheは温まっています。競合するCLI/MCP、外部変更、遅いdisk、debug build、繰返taskの完了・先送り、完了一覧modeは未測定です。実環境の原因を断定するものではなく、合成データで再現できたボトルネックを示します。

fixtureの初期生成は`start_new_project`の既存taskとのUUID重複検査と、多数のfileへの永続化を伴うため、計測本体より長くかかることがあります。wall-clockの固定しきい値は設けず、通常testではignoredです。
