# Schronu

Schronu (スロン) : タスクの抵抗感を減らして前に進んでいくための1人用タスク管理ツール

## 動作環境

* macOSを正式な動作確認対象としています。
* その他のUnix系OSでは動作する可能性がありますが、継続的な動作確認は行っていません。
* Windowsには現在対応していません。

## 開発と検証

Rust toolchainは[`rust-toolchain.toml`](rust-toolchain.toml)で1.97.1に固定しています。依存関係は追跡済みの`Cargo.lock`を使い、通常の検証では次を実行します。

```shell
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

Rust versionまたは依存関係を更新する場合は、`rust-toolchain.toml`と`Cargo.lock`を同じ専用PRで更新し、上記の全検証を通してください。

### testの責務とfixture配置

unit testはmodule privateな振る舞いを検証します。大規模なtest suiteは製品fileと同じdirectoryの`*_tests.rs`へ置き、`#[cfg(test)]`付きの`#[path = "..."] mod tests;`または`include!("...");`から読み込んで、従来のtest module pathを維持します。

application contract testはfilesystemやterminalを介さずuse caseの契約を検証し、library共通fixtureは`src/test_support.rs`を利用します。CLI・MCP contract testはadapter境界を検証し、processやfilesystemを含むend-to-end testは`tests/`へ置きます。CLI binaryのfixtureは`src/adapter/controller/schronu/runtime_test_support.rs`、MCPのfixtureは`src/adapter/mcp/test_support.rs`へ配置します。

test supportは`#[cfg(test)]`の範囲内またはprivateに保ち、製品向けpublic APIを追加しません。

### CLI command境界

CLI commandは、入力から副作用までを次のprivate境界で処理します。

1. `command.rs`がinteractive/non-interactive共通のparserで入力をtyped `Command`へ変換する。
2. `handler.rs`の`handle_command`がVerify以外の唯一のcommand orchestration入口となる。`handler.rs`が定義するprivate context traitを`command_context.rs`の製品contextが実装し、日時解釈とdomain operationを提供する。
3. `view.rs`がtree、task list、calendar、band、focusなどのtyped表示値を組み立て、handlerが意味的な`DisplayModel`を返す。
4. `renderer.rs`が`DisplayModel`を既存CLI文字列へ変換し、writer固有改行、ANSI、Spreadsheet A-J列の整形、flush modeを扱う。
5. `runtime.rs`は依存構築、repository transaction、Verifyのread-only repository検査、外部URL起動、interactive/non-interactive調停、focus変更と描画要求の適用、終了code変換だけを担う。

この境界はprivateな実装構造です。command名とalias、CLI文言、YAML、MCP、Spreadsheetの公開契約は変更しません。

保存性能を測定するignored testは、2172 projectを含むtask storageのコピー元を`SCHRONU_BENCHMARK_STORAGE`へ指定して手動実行します。外部fixtureと実行環境に依存するため、CIでは実行しません。

```shell
SCHRONU_BENCHMARK_STORAGE=/absolute/path/to/task-storage-copy \
  cargo test benchmark_save_2172project中1件変更を2秒未満で処理する -- --ignored --nocapture
```

### scheduling性能契約

`schedule`、`pack`、`flatten`は、匿名化した固定seed fixtureで性能退行を検出します。元storageは集計時だけread-onlyで開き、名前、UUID、本文、実日付、絶対pathはfixtureへ保存しません。`SCHRONU_BENCHMARK_STORAGE`はアプリケーションの恒久設定ではなく、次の手動集計だけにinline指定します。

```shell
SCHRONU_BENCHMARK_STORAGE=/absolute/path/to/task-storage \
  cargo test --locked --test scheduling_fixture_contract \
  指定storageの匿名化集計はtypical_fixture契約と一致する \
  -- --ignored --nocapture
```

fixture規模は次のとおりです。stressはtypicalのproject、task、active leaf、競合候補を固定seedで4倍にします。pack benchmarkはprofile全体の走査に加え、typicalで1組、stressで4組の固定small probeを同じ計測区間内で実行し、通常配置とatomicの1分cursor前進を必ず通します。flatten benchmarkの固定capacityは1,500分で、過負荷経路を決定論的に発生させるための合成負荷係数です。実際の1日の空き時間を表す設定ではありません。

| fixture | project | task | active leaf |
| --- | ---: | ---: | ---: |
| typical | 2,213 | 26,378 | 691 |
| stress | 8,852 | 105,512 | 2,764 |

通常CIはwall-clockではなく、candidate、segment、occupied slot探索、依存候補走査、sort、schedule再構築、配置試行、cursor前進、overload反復、override clone、全schedule走査の上限を検査します。週次・手動CIはRust 1.97.1、release build、`Asia/Tokyo`、GitHub Actions Ubuntu runnerで3回のmedianを測り、typical 500ms、stress 5,000msを上限とします。ただし、全scheduleを333回再構築するstress flattenは8,000msを上限とします。

```shell
cargo test --locked --features benchmarking --test scheduling_benchmark_contract
cargo bench --locked --features benchmarking --bench scheduling -- typical schedule check
cargo bench --locked --features benchmarking --bench scheduling -- typical pack check
cargo bench --locked --features benchmarking --bench scheduling -- typical flatten check
cargo bench --locked --features benchmarking --bench scheduling -- stress schedule check
cargo bench --locked --features benchmarking --bench scheduling -- stress pack check
cargo bench --locked --features benchmarking --bench scheduling -- stress flatten check
```

初回ローカルbaselineはRust 1.97.1、release build、`Asia/Tokyo`、Darwin arm64で次のmedianでした。CI上限にはrunner差を見込んだ余裕を持たせています。

| use case | typical | stress |
| --- | ---: | ---: |
| schedule | 6.930ms | 29.172ms |
| pack | 8.046ms | 35.296ms |
| flatten | 72.900ms | 403.322ms |

探索時の旧実装ではtypical scheduleが約1,070ms、packが20秒超でした。支配要因はoccupied intervalの反復走査、packの未変更schedule再構築、flattenのoverride全cloneとcandidate再構築、依存待ちcandidateの反復走査でした。最適化後も通常API、task順序、segment、deadline判定、`PackResult`、`FlattenResult`、`pending_until`のcharacterization contractを維持します。

## Schronuが対象とすること

* あなた1人が持っているタスクの抵抗感を小さくし、スムーズに進めるようにすること
* あなた1人のタスクの完了時間を見積もること
* あなたが、今、新たな仕事を引き受けてよいのかを判断しやすくすること

## Schronuの範囲外であること

あるいは、実装予定がない機能。

* チームでのタスク管理
  * チームでのタスク管理を行うツールは既に多数存在する
  * Schronuは、チームでのタスク管理を行った結果としてあなたにアサインされたタスクを、上手く素早く遂行するために用いる
* 通知機能
  * うるさく思ってオフにされるのがオチ。あるいは、通知に気付いても無視するか。
* 複数の端末でのタスク情報の同期
  * ネットワークによる通信は[crates.io](https://crates.io/)からRustのライブラリをダウンロードしてくることのみ
  * その他は一切ネットワークを通した送受信を行わないため、セキュリティ的に安全

## MCP server

Schronuは、ローカルのMCP clientから10個のtask toolを利用できるstdio serverを提供します。network transportや認証機能は持ちません。

### buildと起動

```shell
cargo build --release --bin schronu-mcp
SCHRONU_STORAGE_DIR=/absolute/path/to/tasks SCHRONU_CONFIG_PATH=/absolute/path/to/schronu.yaml ./target/release/schronu-mcp
```

`schronu-mcp`はstdinからnewline区切りのJSON-RPC messageを読み、stdoutにはMCP protocol responseだけを出力します。起動時は保存先pathの検証だけを行い、lock取得やrepository loadは行いません。通常はterminalから直接操作せず、MCP clientからprocessを起動してください。

保存先は`SCHRONU_STORAGE_DIR`で指定します。未指定時は起動時のworking directoryから見た`../Schronu-private/tasks/`です。相対pathの解釈違いを避けるため、MCP clientでは絶対pathを推奨します。同じ環境変数はCLIの`schronu`にも適用されます。

### 保存データの検証

`schronu 検証`は、すべての`project.yaml`を読取専用で検査します。成功時は`検証: OK`を表示し、不正値がある場合はファイルpath、task path、field、原因を表示して失敗します。YAMLを書き換えず、free time設定も読み込みません。

旧形式との互換性のため、fieldの欠落は既定値として読みます。`status`は`todo`、booleanは`false`、秒数・日数は0、見積時間は900秒、反復anchorは`deadline`、日時は`now`または未設定として扱います。`fixed_start`だけは後述する旧data推定を適用します。`id`の欠落時だけは新規UUIDを生成します。一方、fieldが存在する場合の型違い、不正UUID、不正enum、負の秒数、0以下の反復間隔、不正または曖昧なローカル日時はエラーです。

### 設定ファイル

`SCHRONU_CONFIG_PATH`には任意の設定YAMLのabsolute pathを指定できます。CLIの`schronu`とMCP serverの`schronu-mcp`は、ともに起動時に同じ設定を読み込みます。未指定時は従来の既定値で動作します。指定したfileが読めない、YAMLが壊れている、未知のキーや不正な値がある場合は、意図しない既定値でtaskを操作しないよう起動を停止します。

仕事用設定の例です。

```yaml
obsidian_vault_name: Obsidian-Work
busy_time_slots_yaml_path: busy_time_slots.yaml
end_of_day_offset_minutes: -120
calendar_blank_line_weekday: Mon
extrude_skip_weekdays: [Sat, Sun]
default_deadline_time: "19:00"
```

すべてのキーは任意です。相対`busy_time_slots_yaml_path`は、実行時のworking directoryではなく設定YAMLの親directoryから解釈します。

編集用の雛形は[`config/schronu.sample.yaml`](config/schronu.sample.yaml)です。コピーして値を環境に合わせて変更し、`SCHRONU_CONFIG_PATH`でabsolute pathを指定します。

```shell
cp config/schronu.sample.yaml /absolute/path/to/schronu.yaml
SCHRONU_CONFIG_PATH=/absolute/path/to/schronu.yaml cargo run --bin schronu
```

コピー後は`busy_time_slots_yaml_path`を、設定ファイルの親directoryを基準に実在するbusy time slots YAMLへのpathへ変更してください。

| キー | 既定値 | 効果 |
| --- | --- | --- |
| `obsidian_vault_name` | `Obsidian-Work` | `黒`(または`obs`)コマンドのObsidian検索先vault名です。空白や記号を含む名前も利用できます。 |
| `busy_time_slots_yaml_path` | `../Schronu-private/busy_time_slots.yaml` | 毎週定期の行動不能時間を定義するYAMLへのpathです。 |
| `end_of_day_offset_minutes` | `30` | 当日24:00からの符号付き分オフセットです。`-120`は22:00、`30`は翌日00:30を表し、日次容量・`全`・`暦`・`帯`・`平`・`詰`で使います。論理日の開始境界である06:00は変更しません。 |
| `calendar_blank_line_weekday` | `Mon` | `暦`の出力で、その曜日の直後に空行を入れます。 |
| `extrude_skip_weekdays` | `[]` | `押`で次の割当日として飛ばす曜日です。例の`[Sat, Sun]`では土日を飛ばします。7曜日すべては指定できません。 |
| `default_deadline_time` | `23:59:59` | `〆`の`今`・`明`・曜日・日付指定で使う締切時刻です。時刻を明示した`〆 19:00`と`〆 消`には適用しません。 |

曜日は`Mon`、`Tue`、`Wed`、`Thu`、`Fri`、`Sat`、`Sun`のいずれかです。`end_of_day_offset_minutes`は`-1079`から`1439`までの整数、`default_deadline_time`は`HH:MM`または`HH:MM:SS`で指定します。

### MCP client設定例

clientごとの設定形式に合わせて、commandと環境変数を次のように指定します。

```json
{
  "mcpServers": {
    "schronu": {
      "command": "/absolute/path/to/Schronu/target/release/schronu-mcp",
      "env": {
        "SCHRONU_STORAGE_DIR": "/absolute/path/to/Schronu-private/tasks",
        "SCHRONU_CONFIG_PATH": "/absolute/path/to/schronu.yaml"
      }
    }
  }
}
```

### 利用可能なtool

日時はRFC 3339、task IDはUUIDで指定します。categoryは`earning`、`sustaining`、`recovery`、`investment`、`consumption`のいずれかです。

| tool | 主な入力 | 動作 |
| --- | --- | --- |
| `get_focus` | なし | 現在着手すべきtaskを返す。候補がなければ`task: null` |
| `get_task` | `task_id` | task詳細を返す |
| `list_tasks` | optional: `period`、`statuses`、`categories` | taskを絞り込んでpre-orderで返す |
| `get_schedule` | optional: `from`、`until` | 日付範囲でSchronuの予定計算結果を返す |
| `create_task` | `name`、optional: `estimated_work_minutes`、`pending_until` | 新規projectを作成する |
| `breakdown_task` | `parent_id`、`names`、optional: `pending_until` | 入力順に子taskを追加する |
| `defer_task` | `task_id`、`pending_until` | 絶対時刻までtaskを延期する |
| `defer_routine_task` | `task_id` | 親の反復間隔に従ってtaskを次周期へ延期する |
| `complete_task` | `task_id`、optional: `finished_at`、`additional_actual_work_seconds` | taskを完了する |
| `update_task` | `task_id`と、`estimated_work_minutes`、`deadline_time`、`category`のうち1つ以上 | 見積もり・締切・categoryを更新する |

`deadline_time`と`category`は`null`で解除できます。`list_tasks.period.field`は`scheduled_start`、`created_at`、`deadline`、`completed_at`のいずれかで、`from`以上`until`未満の半開区間です。`statuses`は`todo`、`pending`、`done`、`categories`は上記categoryまたは`null`を配列で指定します。同じ`statuses`内と同じ`categories`内はOR、period・status・categoryの間はANDです。statusは現在時刻を反映した実効statusで判定します。配列の省略または空配列は、その項目で絞り込みません。`get_schedule.from`と`get_schedule.until`は`YYYY-MM-DD`の日付で、`from`以上`until`未満の範囲を指定します。`from`のみはその日、`until`のみは現在から指定日までです。両方省略時は、現在からSchronuの次の論理日境界までを返します。

例:

```json
{
  "name": "create_task",
  "arguments": {
    "name": "MCPから作成したtask",
    "estimated_work_minutes": 30,
    "pending_until": "2026-08-12T09:00:00+09:00"
  }
}
```

```json
{
  "name": "list_tasks",
  "arguments": {
    "statuses": ["todo", "pending"],
    "categories": ["recovery", null]
  }
}
```

入力schema違反はJSON-RPC `-32602`、実行時の入力error・task不明・未完了child・保存失敗は`isError: true`のstructured tool resultとして返ります。`tools/call`中のrepository load失敗は`repository_load_failed`、lock競合は`repository_lock_contended`、その他のlock取得失敗は`repository_lock_failed`として返ります。これらの失敗はsessionをpoisonせず、修復または競合解消後に同じsessionから再試行できます。

write toolの保存に失敗すると、memory上のrepositoryとfileの状態が一致している保証がありません。失敗したrequestには`repository_save_failed`、同一sessionの後続`tools/call`には`repository_state_uncertain`と`recovery: "restart_server"`を返します。そのsessionを継続利用せず、MCP serverを再起動してrepositoryをfileから読み直してください。

### CLIとの排他lock

CLIとMCP serverは保存先直下の`.lock`へ同じOS advisory lockを取得します。CLIは起動時、60秒ごとの再描画、command実行時だけlockを取得します。command実行時はrepository cacheの確認、command実行、saveまで保持してから解放し、成功したcommandは即時保存します。MCP serverは`tools/call`ごとにlockを取得し、repository cacheの確認、tool実行、必要ならsave、response構築まで保持してから解放します。CLIと複数のMCP processはidle中に共存でき、storage操作だけが直列化されます。`.lock`には`pid`、`started_at`、`mode`(`cli`または`mcp`)が記録され、`started_at`はそのstorage操作がlockを取得した時刻です。

実際に`project.yaml`を変更する保存では、保存先直下の`.revision`を先にatomic更新してから、変更されたprojectだけを保存します。`.revision`はCLI・MCP間でcacheを無効化するための補助metadataで、task dataや`project.yaml`のschemaではありません。既存storageに`.revision`がない場合もそのまま起動でき、最初の変更保存時に作成されます。

各processは起動後の最初のstorage操作では必ず全projectをloadします。2回目以降は`.revision`が前回値と一致すればmemory上のtask treeを再利用し、現在時刻へのclock同期だけを行います。他processが保存して`.revision`が変わった場合は、次のCLI command、MCP `tools/call`、またはCLIの60秒ごとの再描画で全projectを1回loadし直します。稼働中の`project.yaml`直接編集は`.revision`を更新しないため検出対象外です。

CLIはlock競合時に最大1秒、10ms間隔で取得を再試行します。timeoutしたcommandは実行も保存もせず、入力を保持するため、競合解消後にEnterで再試行できます。MCP callは競合時に待機せず`repository_lock_contended`と`recovery: "retry"`を返します。競合中のstorage操作が終わった後に再試行してください。`.lock` fileはprocess終了後も残りますが、fileの存在だけではlock中を意味しません。OS lockを取得できるかどうかで、実際のlock状態を判定します。取得成功時にmetadataは上書きされます。

CLIのCtrl-Cは未送信の入力だけを破棄します。既に成功したcommandは保存済みであり、session全体をrollbackしません。CLI commandのsaveに失敗した場合は、memoryとfileの状態が一致している保証がないためCLIを終了します。保存先を確認・修復してからCLIを再起動してください。

稼働中のprocessがある状態で`.lock`や`.revision`を削除・編集すると、排他やcache無効化が破れる可能性があります。どちらも手動変更しないでください。`.revision`が壊れた場合はCLIと全MCP serverを停止し、`.revision`だけを削除してから再起動すると、次の変更保存時に再作成されます。異常終了後は、まず通常どおり再起動してOS lockが解放済みか確認してください。

### backupと安全上の注意

一貫したbackupを取る場合はCLIを終了し、全MCP serverを停止した状態で、`.lock`を除く保存先directoryの内容をdirectory構造ごとcopyしてください。`.lock`はtask dataではないためbackup・restore対象外です。`project.yaml`の直接編集や復元もCLI・MCP停止中に行い、完了後にprocessを再起動してください。

stdio接続を許可したMCP clientはtaskの作成・変更・完了とfile保存を実行できます。信頼できるローカルclientだけに設定し、保存先のfilesystem permissionとbackupを管理してください。初版の対象外は、team共有、端末間同期、network transport、複数projectをまたぐatomic transactionです。

## CLI

``` shell
schronu
```

起動すると、デフォルトでは最も優先度が高いタスクにフォーカスが当たり、それが表示されます。
対話モードで最後のキー入力から60秒間操作がない場合、進捗表示を含む画面全体を自動更新します。入力途中のコマンドとカーソル位置は維持されます。

### タスクを選択してTodoへ戻す

``` shell
schronu> 選
schronu> 選 00000000-0000-0000-0000-000000000001
schronu> pick
schronu> pick 00000000-0000-0000-0000-000000000001
```

`選 [task_id]`または`pick [task_id]`は、対象タスクを選択してTodo状態へ戻します。
task_idを省略した場合は、現在フォーカスが当たっているタスクを対象にします。フォーカスしているタスクがなければ何も変更しません。

### タスクを細分化する

``` shell
schronu> breakdown 新しいタスク名
```

今フォーカスが当たっているタスクを細分化し、子タスクとして新しいタスクを作成します。  
フォーカスは新しく作成したタスクに移ります。

### 今フォーカスしているタスクをしばらく先送りにする
``` shell
schronu> defer 5 minutes
```

今フォーカスが当たっているタスクを指定した期間Pending状態とします。

### 指定日の予定枠を空ける・集める

``` shell
schronu> 空 13:00
schronu> 集 120
schronu> clear 13:00 8/15
schronu> gather 24:00 8/15
schronu> 空 13:00 明
schronu> 集 24:00 月
```

2引数の`空|clear <時刻または分>`と`集|gather <時刻または分>`は従来どおり、現在のタスク状態を対象に処理します。

3引数の`空|clear <時刻> <月/日|明|曜日>`と`集|gather <時刻> <月/日|明|曜日>`は、指定日の予定表に含まれる未完了の葉タスクだけを対象にします。予定が分割されている場合も、指定日のセグメントを1つでも持つタスクを対象にします。

`空`は、Todoタスクでは予定開始時刻が論理日開始以上・終点未満のもの、Pendingタスクでは`pending_until`が同じ範囲のものを、終点までPendingにします。`集`は、Pendingかつ`pending_until`が終点以下の対象タスクについて、Pendingのまま`pending_until`を指定論理日の開始へ移します。いずれも`start_time`は変更しません。

`<月/日>`が過去なら翌年として解釈します。`明`は次の論理日、曜日は`月`から`日`のうち明日以降で最も近い論理日です。当日と同じ曜日を指定した場合は翌週になります。Schronu論理日は06:00開始です。指定日の06:00より前の時刻は翌暦日の時刻として扱い、`24:00`以降も受け付けます。たとえば`8/15`の`03:00`は8月16日03:00、`24:00`は8月16日00:00です。不正な時刻や、06:00ちょうどのような空区間は何も変更しません。

### 今フォーカスしているプロジェクトのカテゴリを設定する

``` shell
schronu> 類 資
schronu> 類 _
```

今フォーカスが当たっているタスクが属するプロジェクトにカテゴリを設定します。

`獲`、`維`、`回`、`資`、`消`、または `earning`、`sustaining`、`recovery`、`investment`、`consumption` を指定できます。

`_`、`none`、`clear` を指定すると未分類に戻します。

### 繰り返しタスクの見積もりを揃える

``` shell
schronu> 揃 15
schronu> 揃 15 全
```

繰り返しタスクにフォーカスした状態で `揃 15` を実行すると、未完了の直下の子タスクの見積もりを15分に揃えます。見積もりが0の子タスクは変更しません。

指定できる見積もりは0分以上1439分以下です。範囲外または数値でない場合、見積もりは変更されません。

`揃 15 全` と指定すると、見積もりが0の子タスクも15分に変更します。英語形では `arr 15`、`arr 15 all` を使用します。いずれの場合も、完了済みの子タスクは変更しません。

### 今フォーカスしているタスクを完了する

``` shell
schronu> 終
```

今フォーカスが当たっているタスクをDone状態とします。  
引数なしの `終` は、フォーカス開始から現在までの時間を実作業時間に加算し、現在時刻を完了時刻として記録します。

実作業時間を自動加算せずに完了時刻だけを指定する場合は、以下のように入力します。

``` shell
schronu> 終 今
schronu> 終 14:30
schronu> 終 14:30:45
schronu> 終 14:30 明
schronu> 終 14:30 7/4
schronu> 終 14:30 2026/7/4
schronu> 終 14:30 月
schronu> 終 14:30:45 2026/7/4
```

`終 今` は実作業時間を加算せず、現在時刻を完了時刻として記録します。  
`終 14:30` は実作業時間を加算せず、今日の14:30を完了時刻として記録します。秒まで指定したい場合は `終 14:30:45` のように入力します。日付指定は `始` コマンドと同じ形式で解釈されます。

### 低優先度タスクを連続して先送りにする

``` shell
schronu> 低
schronu> defer 7 days
schronu> defer 7 days
schronu> 高
```

起動中に `低` または `low` を入力すると、フォーカスの自動選択を低優先度モードに切り替えます。
`低 0` のように日数を指定すると、低優先度モードで最近扱いする着手可能日の範囲を変更できます。
日数を省略した場合は0日として扱います。
`defer` 後も低優先度モードが維持されるため、低優先度タスクを順に後ろ倒しできます。
`高` または `high` で通常の高優先度モードに戻ります。

### 現在のタスクを一時的に伏せる

```shell
schronu> tuck
schronu> 伏
schronu> t
```

`TuckAway`は、現在のタスクを現在の高・低優先度モードの自動フォーカス候補から一時的に外し、次の候補へ進む対話モード専用コマンドです。canonical inputは`tuck`、aliasは`伏`と`t`です。taskのstatus、優先度、着手可能時刻、`pending_until`は変更しません。

伏せたtask IDは同じモードの間だけCLI process内に保持されます。`高`または`低`を切り替えた場合だけでなく、同じコマンドを再入力した場合も伏せた一覧をresetします。CLIを再起動した場合も伏せた状態は残りません。task一覧やscheduleからはtaskを隠しません。

`見 <task_id>`を使うと、伏せたtaskにも明示的にフォーカスできます。この明示フォーカスは元の高・低モードと伏せた一覧を変更しません。`外`、`tuck`(`伏`または`t`)、または成功した`終`で明示フォーカスが終了すると、元のモードと伏せた一覧から自動選択し直します。

### 先送り中のタスクを余差へ前倒しする

```shell
schronu> 詰
schronu> pack
```

`詰`は、現在のSchronu日(06:00区切り)から7日間を対象に、先送り中で着手可能な葉タスクを日ごとの余差へ前倒しします。余差は反復タスクを除いた可処分時間に対する目標負荷率ρ=0.7までの時間です。空き時間をすべて埋めるコマンドではありません。

候補は優先度が高い順です。同じ優先度では現在の予定日時が早い順、予定日時も同じ場合はUUIDの昇順に扱います。タスクの残作業時間が丸ごと収まる最初の日へ配置し、複数日の余差は合算しません。候補が7日間のどの日にも収まらない場合は、そのタスクをスキップして次の低優先度候補へ進みます。`atomic`タスクは、実際の連続した空き枠にも全量が収まる場合だけ前倒しします。

前倒しでは`pending_until`だけを早めます。元の`start_time`、締切、依存関係、反復設定は変更しません。

### 過負荷を翌日へ平坦化する

``` shell
schronu> 平
```

`平`は、現在のSchronu日(06:00区切り)から28日後までを調べ、日次の確保時間を100%超えている日のタスクを翌日へ押し出します。押し出し先が過負荷になった場合も同じ処理を繰り返すため、途中の日を飛び越えずに負荷が後方へ伝播します。`busy_time_slot`で表される毎週定期的な行動不能時間も確保時間へ反映されます。

延期候補は、葉よりも着手可能になるのが遅い親タスクを優先します。同じrankでは期限がない、または期限が遅いタスク、優先度が低いタスク、現在の予定開始が遅いタスクの順です。親を延期しても子孫の状態は変更しません。タスクは分割せず、残作業時間0、待っていることを表すタスク、1日の最大容量を超えるタスク、現在の予定が論理日境界をまたぐタスクは対象外です。

`end_of_day_offset_minutes`で定めた日次終端より後でも、次の論理日境界である06:00より前の予定は直前の論理日に属する延期候補として扱います。日次終端の前後に分割されたタスクも、全ての作業が同じ論理日内ならタスク単位で翌日へ押し出します。

28日後の過負荷からあふれたタスクは、29日後から34日後を使わず、35日後の06:00へ退避します。この退避先には日次容量の上限を適用しないため、実際の予定は35日後以降へ展開されます。通常延期と退避のどちらでも、仮予定によって親子や既存タスクに新たな期限超過が生じないことを確認します。

延期可能なタスクがない過負荷日は未解消として残し、翌日以降の平坦化を続けます。一部の日を解消できなくても、成立した移動は保存します。未解消日については、超過時間、延期不能となった理由別の件数、その理由を代表するtaskを1件表示します。全ての過負荷日が延期不能で移動が0件の場合も、警告付きで正常終了します。

同じタスクが複数日押し出された場合も、出力と保存は最初の延期元から最終延期先への1回にまとめます。移動件数に固定上限はありません。

移動結果の各行は`平 <延期元> <延期先> <見積時間> <優先度> <task ID> <task名>`の列順で表示します。延期元はコマンド実行開始時の予定日、延期先は連鎖後の最終日です。

英語形では`flatten`または`flat`を使用します。

### タスクツリーを表示する
``` shell
schronu> tree
```

今フォーカスが当たっているタスクのタスクツリー全体を表示します。


### タスク一覧を表示する

```shell
schronu> all
schronu> 全 9/26
schronu> 全 2026/09/26
```

タスクの一覧を以下のように表示します。
`全 9/26` のように年を省略した日付を指定すると、`後 9/26` と同じく、現在から未来方向で直近の9月26日を年付きの日付へ補完して、その日の予定を表示します。例えば現在が2026年8月なら `全 2026/09/26`、現在が2026年10月なら `全 2027/09/26` として扱います。英語形では `all 9/26` を使用します。
年を固定したい場合は `全 2026/09/26` のように指定してください。

末尾側から犠牲候補を確認したい場合は、以下のように表示します。

```shell
schronu> 尾
```

`尾` は `尾 今` として扱われます。予定計算は `all` と同じまま、今日のタスクについて低優先度のタスクほど下側に表示します。同じ優先度の場合は、作業予定時刻が遅いタスクほど下側に表示します。見積もりより長くなったり割り込みタスクが発生したりした場合に、後ろへ押し出されやすい候補を確認するために使います。
`尾 週` のように指定すると、`全 週` と同じ絞り込みをこの順序で表示します。

`今`、`today`、`全 今`、`尾`、`尾 今` のように今日のタスクを絞る表示では、一覧の末尾に「残り拘束時間」「完了見込み日時」、`rep ρ` と `Lq`、`one ρ` と `Lq` を、`暦`・`帯` と同じ形式で表示します。

### 直近の負荷を帯で表示する

```shell
schronu> 帯
schronu> band
```

`帯` は `暦` と同じ直近28日の日次集計を、1文字15分、96文字で24時間を表すASCIIの積み上げ棒として表示します。日付の後ろには余差累と空差累をこの順に、符号付きの `HH:MM` 形式で表示します。時と分は2桁にゼロ埋めされます。タスクが24時間に収まらない場合は、超過時間を15分単位に丸め、閉じ括弧の右側へ `>` を表示します。標準出力が端末の場合、帯と凡例の記号はANSI 256色で表示されます。

```text
# 固定  x 経過済み  = 繰返  - 単発  : 余差  . 空き  > 超過  (1文字=15分)
```

色は順に、固定が淡青(110)、経過済みが灰(244)、繰返が青(33)、単発が橙(208)、余差が暗緑(28)、空きが明緑(34)、超過が赤(196)です。標準出力が端末の場合にだけANSIエスケープシーケンスを含め、パイプ・リダイレクト時は無色で出力します。

`#` は24時間からその日の全日空き時間を引いた固定時間です。`x` は当日だけに表示され、同日の全日空き時間から現在以降の空き時間を引いた、既に経過した空き時間を表します。`:` はrho 0.7に対する当日の余裕です。

15分未満の端数は区分ごとに丸めず、先頭からの累積時間を15分単位へ四捨五入して各区分の境界を決めます。このため短い区分が0文字になる場合はありますが、閉じ括弧内は常に96文字になります。

対話モードを起動せずに、1つのコマンドだけを実行して標準出力へ出すこともできます。

```shell
schronu 今
schronu 尾
schronu 尾 週
```

この非対話実行では、引数全体を1つのコマンドとして扱います。結果は標準出力へ出し、成功した更新コマンドはタスクファイルへ保存されます。
`tuck`、`伏`、`t`は対話モード専用のため、非対話実行では入力errorになり、taskの状態もfileも変更しません。

コマンド入力が不正な場合は、`[Error] 入力エラー: <field>: <理由>`を表示します。対話モードではエラーを表示して入力待ちへ戻り、非対話実行では標準エラーへ表示して非0で終了します。不正入力ではタスクの状態を変更せず、保存も行いません。タスクが見つからない、未完了の子があるなどの操作拒否も診断として表示されます。browserまたはObsidianの起動に失敗した場合は外部起動エラーとして表示されます。

標準出力の受け手が先に終了した場合(`BrokenPipe`)は、パイプライン利用時の正常終了として扱います。
スプレッドシートに貼る形式へ整形する場合は、以下のように使います。

```shell
schronu 尾 | ~/projects/sakabar/Schronu/shell/copy_for_spreadsheet.sh | pbcopy
```

`copy_for_spreadsheet.sh`は、06:00を論理日の境界として、入力内で新しい論理日へ移った最初のタスクのP列へ睡眠時間420分を算入します。00:00から05:59開始のタスクは前の論理日として扱います。コピー対象の先頭行には算入しません。G列の見積もり分数は変更せず、P列の予定完了時刻の計算だけに420分を加えます。対象行のN列には`F`を設定するため、Spreadsheetから戻す際に`終`コマンドは生成されません。R列が`W`または`d`の行は、タスクの見積もりと睡眠時間のどちらもP列へ加算しません。

スプレッドシートから作業実績をコマンドへ戻す場合は、シートをコピーしてから以下のように使います。

```shell
~/projects/sakabar/Schronu/shell/generate_command_from_spreadsheet.sh
```

R列には、タスクの処理時期を変更するときに実行するコマンド、またはSpreadsheet上の表示制御に使う値を入力します。

- 空白: 通常どおりQ列の抽出対象を処理する
- `W`: `W` コマンドで延期する
- `d`: `d` コマンドで翌朝まで延期する
- `t`: Spreadsheet上の表示制御にのみ使い、コマンド生成では無視する(CLIの`TuckAway`短縮入力`t`とは別機能)

`W` または `d` が入力された行は、Q列が `TRUE` でなくても抽出されます。`t` を含むそれ以外の値はR列としては無視され、Q列が `TRUE` の場合だけ通常処理されます。同じタスクが複数行に分かれている場合、R列の `W` と `d` を混在させないでください。

完了対象の行では、P列に `2026/07/04 9:23:45` のような完了時刻、S列に `0:23:45` のような実作業時間を入れてください。生成される `終` コマンドにはP列の完了時刻が渡されます。

新規タスクを仮登録する場合は、B列を空欄のままJ列にタスク名を入力してください。Q列の抽出フラグに関係なく、`新 <タスク名>`、仮の説明、見積もり3分のコマンドが生成されます。

SpreadsheetのA-S列は[spreadsheet_columns.tsv](spreadsheet_columns.tsv)を正本とします。A-J列はSchronuの`全`出力、K-S列はSpreadsheet上の補助列です。B列は`task_id`、J列は`task_name`、L/N/P/R列はシート間の同期対象、P列は完了時刻、Q列は抽出対象、S列は実作業時間です。

(例)

```
0001 ff0a6947-cefb-4401-917c-7766035c4aa3 ! ____-01:20 06/21(土)-18:40~19:20 0 40 01 維 夕食
0000 7942713f-f9c2-4a3a-b251-b3f384e3f820 ! ____-00:00 06/21(土)-18:00~18:40 0 40 01 維 夕方の料理
```

各列はスペース区切りです。内容は以下の通りです。

#### 通し番号

0000から始まる通し番号です。

#### タスクID

タスクを指し示す一意のUUIDです。

#### アイコン

タスクの状況を示すアイコンです。

* `!` : 今日が締切である
* `v` : タスクの完了予定時刻が〆切を過ぎている
* `A` : そのタスクを諦めないと、その日までの累積作業量が時間枠に収まらない
* `/` : すぐ着手できるタスクであり、かつ、今日着手する予定のタスク
* `-` : その他 (特徴なし)

#### 締切

3種類の表現方法があります。

* `____/__/__` : 締切なし
* `YYYY/MM/DD` : YYYY年MM月DD日のどこかの時間が締切である
  * (例) `2025/06/21` : 2025年6月21日のどこかの時間が締切である
* `____-HH:MM` : 今日中に締切があり、完了予定時刻はその締切のHH時間MM分前である
  * (例) `____-00:30` : 今日中に締切があり、完了予定時刻はその締切の30分前である
* `+HH:MM____` : 今日中に締切があり、完了予定時刻はその締切をHH時間MM分オーバーしているため予定を調整する必要がある
  * (例) `+01:30____` : 今日中に締切があり、完了予定時刻はその締切を1時間30分オーバーしている

#### 作業予定時刻

`MM/DD(a)-HH:MM~HH:MM`

(例) `06/21(土)-18:00~18:40` : 6月21日(土曜日)の18時から18時40分まで作業予定

作業予定時刻は、単に着手可能時刻の早い順には並びません。

各タスクには、`start_time`、`pending_until`、親子関係を考慮した「最速着手可能時刻」があります。タスク一覧では、この時刻を「それ以前には開始できない」という制約として扱います。

着手可能なタスクは通常、優先度が高いものから配置します。ただし、ある締切までに必要な残作業量に対して空き時間が尽きる時点では、その締切を守るために必要なタスクへ切り替えます。したがって、締切が近いという理由だけで重要な長期タスクが常に後回しになることはありません。同時に、締切を守るための時間も使い切る前に保護されます。

`約`または`appointment`で開始時刻を設定したタスクはfixed予定となり、priorityや締切にかかわらず指定時刻から動きません。`始`または`start`で開始時刻を設定し直すとflexibleへ戻ります。fixed予定同士が重なる場合は重複したまま表示し、その他のタスクはその予約時間を避けます。

旧dataに`fixed_start`がない場合だけ、`deadline_time == start_time + estimated_work_seconds`と完全一致するタスクを従来の予定としてfixed扱いします。明示的な`fixed_start: false`は推定で上書きしません。

選択規則、slackの定義、fixed・atomic・依存関係の扱いは[予定配置policyの設計](docs/design/scheduling_policy.md)を参照してください。

親タスクは、子タスクが実際に割り当てられた作業予定時刻で完了した後にのみ配置されます。つまり、子タスクが他のタスクとの衝突回避によって後ろにずれた場合、親タスクもその子タスクの実際の完了予定時刻以降にずれます。

長いタスクは、fixed予定の開始、別taskがreleaseされて実際に選択が切り替わる時刻、またはslackが0になる時刻で再選択するため、表示上の複数セグメントに分かれる場合があります。現在taskに勝たない候補の着手可能時刻は分割境界にならず、60分ごとに分割する規則もありません。分割された各セグメントの合計時間は元タスクの残作業時間と一致し、fixed予約や実際に保護が必要になった締切taskを押し出しません。原則として、分割後の前半または後半のどちらかが15分以下になる分割は避け、次の十分な空き時間で再評価します。ただし、slack guardで保護するtaskがその境界で着手可能になる場合は、締切容量を失わないために境界までの短いsegmentを残します。

#### タスクの深さ

* 深さ0 : すぐに着手できる
* 深さ1 : 深さ0のタスクが終わったら着手できる
* 深さ(N+1) : 深さNのタスクが終わったら着手できる

#### 見積もり

そのタスクの推定所要時間 (分)

#### 累計時間 (hour)

タスクリストの先頭から見て、そのタスクまでは多くとも何時間あれば終わるか

#### カテゴリ

プロジェクトカテゴリを表す1文字です。

* `獲` : 金銭獲得
* `維` : 生活維持
* `回` : 体調回復
* `資` : 自己投資
* `消` : 消費
* `_` : 未分類

#### タスク名

そのタスクの内容を表す文字列。
