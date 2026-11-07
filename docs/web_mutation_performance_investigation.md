# Webの記録・完了・先送りの待ち時間調査

2026-11-07、修正前の基準commit `b9f2327e`。以下の初回調査結果に加え、保存成功後のrepository引継ぎを実装した結果を記載します。

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

この表は修正前の値です。更新直後の一覧と変更なしの一覧の差を改善余地として扱い、修正後の再測定を末尾に記載します。

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

## 実装した改善

`WebService::run_mutation_at`でtransaction結果が`Ok`の場合に限り、更新用repositoryを読取cacheへ移します。保存済みのtreeとrevisionを引き継ぐため、直後の一覧取得では外部更新がない限り`reload_if_changed`が`Cached`になります。更新前の独立repositoryの作成、lock取得、外部revisionの確認、保存結果不明時のpoisonは維持します。公開API、保存形式、payload、UIは変更しません。

privateなcache契約testは修正前にrevision不一致で3件失敗し、修正後に3件成功しました。記録・完了・先送り・作成・作業片完了の成功、外部保存後の再読込、更新エラー・commit前保存失敗時のcache維持と再試行、commit後保存失敗時のpoisonを確認しています。

検証済み: `cargo fmt --check`、rootの`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`(1,703 passed / 0 failed)、`cargo test --locked -p schronu-web --no-default-features --features server`(513 passed / 0 failed)。

Web server側の`cargo clippy --locked -p schronu-web --no-default-features --features server --all-targets -- -D warnings`も成功しています。Web製品コードを変更していないためWASM buildは実行していません。

## 修正後の性能測定

同じcommand・fixture・release設定で全27 sampleを完走しました(1 passed / 0 failed、準備・削除を含む全体1000.99秒)。各3回の中央値、単位はmsです。区間と合計の中央値は独立に計算しています。

| 規模 | 操作 | 更新 | 一覧再取得 | 負荷集計 | 合計 |
| --- | --- | ---: | ---: | ---: | ---: |
| Small | 記録 | 333.4 | 16.5 | 24.1 | 373.8 |
| Small | 完了(実績記録あり) | 259.2 | 14.2 | 18.3 | 334.6 |
| Small | 先送り | 211.3 | 10.2 | 8.3 | 375.8 |
| Typical | 記録 | 1654.8 | 80.6 | 81.8 | 1817.2 |
| Typical | 完了(実績記録あり) | 1098.1 | 48.8 | 48.9 | 1194.0 |
| Typical | 先送り | 1441.4 | 64.3 | 53.3 | 1559.0 |
| Stress | 記録 | 8579.4 | 356.4 | 393.2 | 9311.3 |
| Stress | 完了(実績記録あり) | 4737.4 | 224.5 | 223.3 | 5154.4 |
| Stress | 先送り | 2336.3 | 88.9 | 88.8 | 2510.4 |

| 規模 | 操作 | 一覧: 修正前 | 一覧: 修正後 | 修正前比 | 全体: 修正前 | 全体: 修正後 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Typical | 記録 | 797.8 | 80.6 | 10.1% | 1580.6 | 1817.2 |
| Typical | 完了(実績記録あり) | 807.0 | 48.8 | 6.0% | 1617.6 | 1194.0 |
| Typical | 先送り | 808.5 | 64.3 | 7.9% | 1611.9 | 1559.0 |
| Stress | 記録 | 3140.2 | 356.4 | 11.4% | 6284.3 | 9311.3 |
| Stress | 完了(実績記録あり) | 3232.6 | 224.5 | 6.9% | 6804.4 | 5154.4 |
| Stress | 先送り | 3084.5 | 88.9 | 2.9% | 6178.3 | 2510.4 |

全6条件で一覧再取得が修正前の25%以下となり、受入目安を満たしました。

### ばらつきと残る待ち時間

今回の測定中に`uptime`で観測したload averageは約11〜42で、前回との稼働負荷は揃っていません。環境負荷の変動を含む結果のため、全体時間が一律に短縮したとは判断しません。例えばTypicalの記録は全体1,580.6msから1,817.2msへ増えましたが、独立対照の全件ロードも647.6msから1,479.4ms、変更なしの一覧も29.3msから74.1msへ増えています。Stressの記録でも全件ロードの対照値は2,877.7msから9,216.2msへ増えました。今回の更新時間の悪化には測定環境の影響が示唆されます。

一覧の対照値は次のとおりです。修正後の一覧は同じ実行内の変更なしの一覧と同程度であり、回帰testで確認したcache利用と整合します。更新前の全件ロードは残ります。

| 規模 | 操作 | 修正後の一覧 | 変更なしの一覧 | 全件ロード単独 | schedule計算 |
| --- | --- | ---: | ---: | ---: | ---: |
| Typical | 記録 | 80.6 | 74.1 | 1479.4 | 71.0 |
| Typical | 完了(実績記録あり) | 48.8 | 47.0 | 1166.7 | 37.1 |
| Typical | 先送り | 64.3 | 51.7 | 1521.1 | 77.3 |
| Stress | 記録 | 356.4 | 355.8 | 9216.2 | 318.3 |
| Stress | 完了(実績記録あり) | 224.5 | 212.6 | 4028.7 | 114.1 |
| Stress | 先送り | 88.9 | 87.6 | 2141.3 | 70.3 |
