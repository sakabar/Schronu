# TD-019 Scheduling Instrumentation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** scheduling性能計測stateをapplicationの業務関数引数から除去し、通常経路と診断経路が同一algorithmを通るfeature限定の計測境界へ置換する。

**Architecture:** `application::scheduling_instrumentation`へ中立なevent記録APIを置き、default featureではno-op、`benchmarking` featureではthread-localな計測sessionへ記録する。診断entrypointはsession内で通常の`get_schedule`、`pack_tasks`、`flatten_tasks`を呼び、公開metrics型はinstrumentation側の型をそのまま再公開する。

**Tech Stack:** Rust 2021、標準thread-local storage、Cargo feature、標準Rust test。

---

### Task 1: feature限定instrumentation session

**想定commit 1 (Red test):** `Test: scheduling計測sessionの契約を固定する`

- 固定する契約: schedule、pack、flattenの各sessionが対応eventを決定論的counterへ集約し、pack/flatten session内のschedule eventも内包する。
- 対象責務/module: `src/application/scheduling_instrumentation.rs`、`src/application.rs`。
- 先行commitへの依存: なし。
- 対象test: `cargo test --locked --features benchmarking scheduling_instrumentation::tests`。
- Green確認方法: 新module未実装のため、Redは未解決moduleまたは未定義APIの1理由で失敗することを確認する。

**想定commit 2 (Green implementation):** `Benchmark: scheduling計測sessionを導入する`

- 固定する契約: feature時だけ計測stateを生成し、scope終了時にmetricsを返す。default featureは計測stateを定義・生成しない。
- 対象責務/module: `src/application/scheduling_instrumentation.rs`、`src/application.rs`。
- 先行commitへの依存: commit 1。
- 対象test: instrumentation unit test、default/benchmarking build。
- Green確認方法: `cargo test --locked --features benchmarking scheduling_instrumentation::tests`、`cargo check --locked`、`cargo check --locked --features benchmarking`。

**Steps:**

1. `ScheduleEvent`、`PackEvent`、`FlattenEvent`とsession capture APIを前提にしたunit testを追加する。
2. 対象testを実行し、未実装APIだけを理由にRedになることを確認する。
3. Red testだけをcommitする。
4. default featureのno-op記録関数と、feature限定のmetrics型・thread-local sessionを最小実装する。
5. panic時にもsession stackを破棄するguardを実装する。
6. 対象test、default/benchmarking checkを実行してGreenを確認する。
7. `git diff --check`を実行し、commitする。

### Task 2: schedule経路の移行

**想定commit 3:** `Schedule: 計測引数をinstrumentationへ移す`

- 固定する契約: scheduleの公開結果、task順序、segment、deadline判定と既存counter値を維持し、schedule関数群から`ScheduleMetrics`引数と`*_with_metrics`名を除去する。
- 対象責務/module: `src/application/schedule_use_case.rs`、`src/application/scheduling_policy.rs`、関連unit test、`src/application/benchmarking.rs`。
- 先行commitへの依存: commit 2。
- 対象test: schedule use case/policy test、`tests/scheduling_benchmark_contract.rs`のschedule契約。
- Green確認方法: `cargo test --locked --features benchmarking --test scheduling_benchmark_contract schedule`と関連unit test、全品質ゲート。

**Steps:**

1. 既存の通常経路/診断経路同値testと決定論的counter testを対象testとして実行し、characterizationがGreenであることを確認する。
2. `ScheduleMetrics`引数を除き、計測更新を`ScheduleEvent`記録へ機械的に置換する。
3. `get_schedule_with_metrics`等を通常名の単一路径へ統合する。
4. 診断entrypointをschedule session内の通常`get_schedule`呼出しへ変更する。
5. 対象testと全品質ゲートを実行する。
6. サブエージェントreviewを受け、基礎Green実装をcommitする。

### Task 3: pack経路の移行

**想定commit 4:** `Pack: 計測引数をinstrumentationへ移す`

- 固定する契約: `PackResult`、task変更内容、schedule counter、pack counterを維持し、pack関数群から`PackMetrics`引数と`*_with_metrics`/`*_and_metrics`名を除去する。
- 対象責務/module: `src/application/pack_use_case.rs`、`src/application/benchmarking.rs`。
- 先行commitへの依存: commit 3。
- 対象test: pack unit test、benchmark contractのpack同値/counter test。
- Green確認方法: 関連testと全品質ゲート。

**Steps:**

1. packの既存characterization testを実行する。
2. metrics引数を除き、pack event記録へ置換する。
3. 診断entrypointをpack session内の通常`pack_tasks`呼出しへ変更する。
4. 対象testと全品質ゲートを実行する。
5. サブエージェントreviewを受け、commitする。

### Task 4: flatten経路の移行と旧計測型の除去

**想定commit 5:** `Flatten: 計測引数をinstrumentationへ移す`

- 固定する契約: `FlattenResult`、task変更内容、schedule counter、flatten counterを維持し、flatten関数群から`FlattenMetrics`引数と計測専用並行経路を除去する。
- 対象責務/module: `src/application/flatten_use_case.rs`、`src/application/benchmarking.rs`、`src/application/scheduling_metrics.rs`。
- 先行commitへの依存: commit 4。
- 対象test: flatten unit test、benchmark contractのflatten同値/counter/wall-clock test。
- Green確認方法: 関連test、`rg`構造検査、全品質ゲート。

**Steps:**

1. flattenの既存characterization testを実行する。
2. metrics引数を除き、flatten event記録へ置換する。
3. 診断entrypointをflatten session内の通常`flatten_tasks`呼出しへ変更する。
4. 公開metrics型を再公開へ統一し、旧`scheduling_metrics.rs`を除去する。
5. `rg`でuse caseのconcrete metrics import/生成と`_with_metrics`/`_and_metrics`が0件であることを確認する。
6. 対象testと全品質ゲートを実行する。
7. サブエージェントreviewを受け、commitする。

### Task 5: backlog更新と最終履歴review

**想定commit 6:** `Docs: TD-019を完了にする`

- 固定する契約: 完了日、解決内容、検証根拠をbacklogへ記録する。
- 対象責務/module: `backlog.md`。
- 先行commitへの依存: commit 5と最終検証。
- 対象test: 文書差分、履歴review。
- Green確認方法: 全品質ゲート、benchmark contract、構造検査、`git diff --check`。

**Steps:**

1. `cargo fmt --check`を実行する。
2. `cargo clippy --locked --all-targets -- -D warnings`を実行する。
3. `cargo test --locked`を実行する。
4. `cargo test --locked --features benchmarking --test scheduling_benchmark_contract`を実行する。
5. default/benchmarkingのbuild構造と完了条件を再確認する。
6. サブエージェントにコードとcommit履歴の最終reviewを依頼する。
7. 指摘があれば1件ずつ修正、対象test、個別commitを行う。
8. `backlog.md`を更新し、文書commitを作成する。
9. `git status --short --branch`と各commitの`git show --stat --oneline`を確認する。
