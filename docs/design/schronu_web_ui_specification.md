# Schronu-web UI実装仕様

## 1. Summary

`schronu-web`を、localStorageに保持する複数の`work_sessions`と、Schronuのscheduleを表示するDioxus single-page UIへ置換する。

WebセッションはSchronu本体のcurrent taskと独立させる。Schronuの`get_focus`は自動選定の内部実装としてだけ利用し、UI上の名称は「セッション」に統一する。

本仕様は[UI要件定義](./schronu_web_ui_requirements.md)の実装契約を定める。

## 2. 構成と責務

| 層 | 責務 |
| --- | --- |
| Dioxus component | tab、button、card、一覧、error、履歴の描画と利用者操作の受付。 |
| client state | `work_sessions`、snapshot、選択日、一覧、処理中操作、履歴、1秒tickを管理する。 |
| localStorage adapter | `work_sessions`のversion付きserialize、読込、検証、保存を行う。 |
| server function | wire DTOを検証し、専用workerへ型付きcommandを送る。 |
| Web operation worker | 1 thread上でWeb操作を直列実行し、environment、repository、free-time資源を所有する。 |
| Web controller service | application use caseを組み合わせ、snapshot、一覧、自動選定、記録、完了を提供する。 |
| application | schedule、focus選定、UUID指定実績加算、task完了のdomain操作を提供する。 |
| repository transaction | task treeの読込、変更、保存、rollback、状態不確実性の契約を維持する。 |

clientはCLI command文字列を生成・submitせず、型付きserver functionを呼ぶ。serverもCLI renderer出力をparseしない。

## 3. Data contracts

### 3.1 Wire primitive

- task ID: UUID文字列
- 時刻: Unix epoch millisecondsの整数
- 時間量: 秒の整数
- logical date: `YYYY-MM-DD`文字列
- 曜日・時刻表示: clientがlocal timezoneで生成する

### 3.2 Common snapshot

すべてのserver responseはpayloadと併せて次のsnapshotを返す。

```text
ServerSnapshot {
    observed_at_epoch_ms: i64,
    logical_date: YYYY-MM-DD,
    buffer_seconds: i64,
}
```

`observed_at_epoch_ms`、`logical_date`、`buffer_seconds`は同じserver操作時刻を基準に算出する。clientはresponse受信時刻ではなく`observed_at_epoch_ms`を表示計算の基準とする。

### 3.3 Task DTO

セッション開始に必要なtask snapshotは次を持つ。

```text
SessionTaskDto {
    task_id: UUID,
    task_name: String,
    estimated_work_seconds: i64,
    actual_work_seconds: i64,
}
```

一覧の1行はschedule segmentを表し、次を持つ。

```text
ScheduledTaskRowDto {
    task: SessionTaskDto,
    schedule_start_epoch_ms: i64,
    schedule_end_epoch_ms: i64,
    deadline_epoch_ms: Option<i64>,
    is_leaf: bool,
}
```

同じtaskが複数segmentに分かれる場合、同じ`task_id`を持つrowを複数返してよい。serverは`get_schedule`の結果を開始時刻昇順に安定sortする。

### 3.4 localStorage schema

keyは`schronu_web.work_sessions.v1`とする。valueはversion付きobjectとし、配列だけを直接保存しない。

```json
{
  "version": 1,
  "work_sessions": [
    {
      "task_id": "UUID",
      "task_name": "task name",
      "started_at_epoch_ms": 1788565500000,
      "estimated_work_seconds_at_start": 900,
      "actual_work_seconds_at_start": 300
    }
  ]
}
```

読込規則:

1. keyがなければ空配列とする。
2. JSON不正、version不一致、UUID不正、空のtask名、負の見積・実績、不正なepochは該当データを採用しない。
3. 同一UUIDが複数ある場合は配列中の先頭1件だけを採用する。
4. 採用できないデータがあればclient errorとして表示するが、server通信やtask更新は行わない。
5. state変更後は採用済みの全`work_sessions`を1回で書き戻す。保存失敗時はmemory上の直前stateを維持し、操作失敗として履歴へ残す。

## 4. Server operations

専用workerは次の5 commandを順番に処理する。workerへの送信順が実行順となる。

### 4.1 `bootstrap`

- 入力: なし
- 出力: `ServerSnapshot`
- task dataは変更しない。

### 4.2 `list_tasks(date)`

- 入力: `logical_date: YYYY-MM-DD`
- 出力: `ServerSnapshot`と`Vec<ScheduledTaskRowDto>`
- 指定日のscheduleを取得し、開始epoch milliseconds昇順で返す。
- 指定日を曜日へ変換してCLIの`全 曜日`文字列を実行する実装にはしない。
- task dataは変更しない。

### 4.3 `auto_session`

- 入力: なし
- 出力: `ServerSnapshot`と`Option<SessionTaskDto>`
- applicationの`get_focus`相当の選定を呼ぶが、current taskの設定処理は呼ばない。
- 候補がなければ`None`を正常結果として返す。
- task dataは変更しない。

### 4.4 `record_session`

入力:

```text
RecordSessionRequest {
    task_id: UUID,
    started_at_epoch_ms: i64,
    expected_actual_work_seconds: i64,
}
```

処理:

1. 1回だけ取得したserver操作時刻を`operation_now`とする。
2. `floor((operation_now - started_at) / 1000)`を追加実績秒とする。
3. 差が負、日時変換不能、秒数変換不能なら入力errorとし、保存しない。
4. UUID、追加実績秒、期待実績秒をapplicationの共通実績加算操作へ渡す。
5. repository transactionの保存成功後に`ServerSnapshot`を返す。

出力は`ServerSnapshot`と更新後の実績秒を持つ。current taskは参照・変更しない。

### 4.5 `complete_session`

入力は`RecordSessionRequest`と同じとする。

1. `record_session`と同じ規則で追加実績秒を算出する。
2. 既存`CompleteTaskInput`へtask UUID、`operation_now`、追加実績秒、`Some(expected_actual_work_seconds)`を渡す。
3. applicationは期待実績検証、実績加算、完了、終了時刻更新、反復task生成を1つの操作として準備する。
4. repository transactionは全変更を1回で保存する。

出力は`ServerSnapshot`と既存`complete_task`の次task情報を必要な範囲で持つ。Webは次taskをSchronu本体のcurrent taskへ設定せず、sessionも自動追加しない。

## 5. Application contracts

### 5.1 共通実績加算

application層にはCLI用とWeb用を分けず、実績加算操作を1つだけ設ける。

```text
AddActualWorkInput {
    task_id: UUID,
    additional_actual_work_seconds: i64,
    expected_actual_work_seconds: Option<i64>,
}
```

処理順:

1. 追加実績秒が非負であることを検証する。
2. UUIDでtaskを取得する。
3. taskが未完了であることを検証し、完了済みなら`task_already_completed`を返す。
4. 期待実績が`Some`なら現在実績と完全一致することを検証する。
5. checked additionで更新後実績を計算する。
6. すべての検証後にtaskを変更する。

未知task、完了済みtask、負数、期待値不一致、overflowではtask treeのmutation revisionを含めて状態を変更しない。

呼び出し側の差は入力の決定と成功後のUI状態だけに限定する。

- CLI`働`: adapterがcurrent task UUIDと追加秒を決め、期待実績は`None`とする。
- Web`record_session`: session UUID、経過秒、`Some(開始時実績)`を渡す。
- task完了: 別責務である既存`complete_task`を使う。ただし、内部の実績加算規則は共通実績加算と同じprivate helperへ集約し、検証を複製しない。

### 5.2 CLI `働`

- 引数なし:
  - command処理で固定した実行時刻からfocus開始時刻を引く。
  - 完了済み整数秒を追加実績秒として共通操作へ渡す。
- `働 <minutes>`:
  - parserは非負整数だけを受理する。
  - adapterでchecked multiplicationした`minutes * 60`を追加実績秒とする。
- 共通操作は現在実績へ秒を加算するため、既存の秒端数を保持する。
- task未選択は従来どおりno-opとする。
- 保存成功後だけCLI focusを解除する。
- 負数、時計後退、overflow、repository errorでは実績とfocusを変更しない。
- command名、alias、引数個数、正常時renderer出力、lock・保存境界は変更しない。

### 5.3 `complete_task`

既存`CompleteTaskInput`へ次を追加する。

```text
expected_actual_work_seconds: Option<i64>
```

- `None`: 現在と同じ完了処理を行う。
- `Some(expected)`: taskの現在実績と一致した場合だけ後続処理を行う。
- 不一致時は実績、status、終了時刻、親子状態、反復task、mutation revisionを変更しない。
- CLIとMCP adapterは常に`None`を設定する。
- Webだけが`Some(開始時実績)`を設定する。
- MCPの入力structおよび生成JSON schemaへこのfieldを追加しない。

## 6. Client state and calculations

### 6.1 State

clientは最低限、次を保持する。

```text
active_tab: Session | List
work_sessions: Vec<WorkSession>
server_snapshot: Option<ServerSnapshot>
date_buttons: Vec<LogicalDateButton>
selected_logical_date: Option<YYYY-MM-DD>
scheduled_rows: Vec<ScheduledTaskRowDto>
in_flight_session_ids: Set<UUID>
page_error: Option<DisplayError>
operation_history: VecDeque<OperationHistoryEntry>
tick_now_epoch_ms: i64
```

`operation_history`は最大100件とし、101件目の追加前に最古のentryを削除する。localStorageへ保存しない。

### 6.2 経過時間

```text
elapsed_seconds = max(0, floor((tick_now_epoch_ms - started_at_epoch_ms) / 1000))
```

毎秒tickは`tick_now_epoch_ms`だけを更新する。経過秒をincrementして保持しないため、tab非表示、timer遅延、reloadを挟んでも開始時刻基準で復元できる。

### 6.3 完了予定、進捗、残り時間

```text
remaining_at_start = max(estimated_at_start - actual_at_start, 0)
estimated_completion = started_at + remaining_at_start
worked_seconds = actual_at_start + elapsed_seconds
progress_percent = floor(worked_seconds * 100 / estimated_at_start)
remaining_seconds = remaining_at_start - elapsed_seconds
```

- 見積秒が0なら除算せず`--%`とする。
- `remaining_seconds >= 0`は通常色の`MM:SS`、負なら絶対値を赤い`MM:SS`で表示する。
- `MM`は総分数とし、2桁へ制限しない。`SS`は常に2桁とする。
- `worked_seconds * 100`はoverflowしない計算方法を用いる。
- 通常bar幅は`min(progress, 100)%`。
- 超過bar幅は`max(progress - 100, 0)%`で、100%位置の右側へ赤色で連結する。card内で切り捨てず、必要な横方向の表示領域を確保する。

### 6.4 buffer

server側:

```text
buffer_seconds = remaining_free_seconds(logical_date, observed_at)
               - scheduled_remaining_work_seconds(logical_date, observed_at)
```

`remaining_free_seconds`は06:00境界、Schronu設定、毎週固定の`busy_time_slot`を反映する。単発予定を`busy_time_slot`として追加しない。`scheduled_remaining_work_seconds`は同じlogical dateのscheduleに割り当てられた未実施作業秒を用いる。

client側:

```text
snapshot_elapsed = max(0, floor((tick_now - observed_at) / 1000))
display_buffer = buffer_seconds - snapshot_elapsed
```

- `display_buffer >= 0`: 通常色の`HH:MM:SS`
- `display_buffer < 0`: 赤色の`-HH:MM:SS`
- hourは総時間とし、24以上もそのまま表示する。
- clientの06:00到達を監視してserver requestを送らない。

### 6.5 logical date buttons

最新の`ServerSnapshot.logical_date`をindex 0として8日分を生成する。

- index 0: `曜 今日`
- index 1: `曜 明日`
- index 2..7: `曜`

各buttonは表示labelとは別に具体的な`YYYY-MM-DD`を保持する。新しいserver responseでlogical dateが変わった場合はbuttonを再生成し、既存一覧をclearする。追加の`list_tasks`は自動実行しない。

## 7. UI behavior

### 7.1 初期化とtab

1. localStorageを読み、`work_sessions`を復元する。
2. `bootstrap`を1回送る。
3. responseからbufferと8日buttonを表示する。
4. 初期tabは「セッション」とする。
5. tab切替だけでは一覧取得を含むserver操作を行わない。

### 7.2 セッション画面

- セッション0件では「自動セッション」buttonを表示する。
- 1件以上ではbuttonを隠し、各`work_session`をcard表示する。
- cardはtask名、開始`HH:MM`、完了予定`HH:MM`、進捗率、bar、残り・超過`MM:SS`、3操作buttonを持つ。
- 「自動セッション」成功時はresponseのtask snapshotから現在時刻を開始時刻とするsessionを追加する。
- 自動選定結果が`None`なら空状態と案内を表示する。

### 7.3 一覧画面

- 日付button click時だけ`list_tasks(date)`を送る。
- rowは締切、予定`HH:MM-HH:MM`、task名、「セッション」buttonを表示する。
- 締切は選択logical date内なら`HH:MM`、それ以外は`MM/DD HH:MM`とする。現在epochが締切epochを超えた場合に赤くする。
- `is_leaf`がtrueのtask名を緑にする。
- 「セッション」click時はrowのtask snapshotとclient現在時刻からsessionを作り、localStorageへ保存する。active tabは変更しない。
- `work_sessions`に同一UUIDがあれば、そのUUIDの全rowでbuttonをdisabledにする。

### 7.4 操作結果

- localStorage更新は、memory state確定前に保存成功を確認する。
- server mutationは、response成功後にlocalStorageからsessionを削除する。
- server errorまたはlocalStorage削除失敗ではsessionを残す。server保存成功後にlocalStorage削除だけが失敗した場合、responseの更新後実績を反映した競合案内を表示し、再送による二重加算を防ぐため対象buttonを無効化する。
- in-flight中は対象sessionの3buttonを無効化する。他sessionの計測は継続する。

## 8. Communication and persistence matrix

| 操作 | server通信 | task保存 | localStorage変更 | current task変更 |
| --- | --- | --- | --- | --- |
| 初回表示 | `bootstrap` | なし | 復元時の正規化だけ | なし |
| tab切替 | なし | なし | なし | なし |
| 毎秒tick | なし | なし | なし | なし |
| 日付button | `list_tasks` | なし | なし | なし |
| 自動セッション | `auto_session` | なし | session追加 | なし |
| 一覧の「セッション」 | なし | なし | session追加 | なし |
| 破棄して解除 | なし | なし | session削除 | なし |
| 記録して解除 | `record_session` | 実績保存1回 | 成功後session削除 | なし |
| 完了 | `complete_session` | 完了transaction 1回 | 成功後session削除 | なし |
| 06:00境界 | なし | なし | なし | なし |

## 9. Error contracts

server errorは少なくとも次を識別可能にする。

| code | 条件 | client動作 |
| --- | --- | --- |
| `invalid_input` | UUID、日付、epoch、負の経過秒、範囲外 | sessionと既存表示を保持して内容を表示する。 |
| `task_not_found` | UUIDに対応するtaskがない | sessionを保持し、破棄可能にする。 |
| `task_already_completed` | 完了済みtaskを記録・完了しようとした | 保存せずsessionを保持する。 |
| `actual_work_conflict` | 現在実績と期待実績が不一致 | 保存せずsessionを保持し、競合を明示する。 |
| `arithmetic_overflow` | 実績、進捗、日時計算が表現範囲外 | 保存せずsessionを保持する。 |
| `task_not_completable` | 未完了の子など既存完了条件を満たさない | 保存せずsessionを保持する。 |
| `repository_save_failed` | 保存失敗、rollback成功 | sessionを保持して再試行可能と表示する。 |
| `repository_state_uncertain` | rollbackを保証できない | sessionを保持して再送を無効化し、手動確認を要求する。 |
| `worker_unavailable` | worker停止またはresponse channel切断 | 既存表示を保持してserver操作失敗を表示する。 |

server操作ごとに`operation_now`は1回だけ取得し、経過秒算出、完了時刻、snapshotに共通利用する。

## 10. Operation history

```text
OperationHistoryEntry {
    occurred_at_epoch_ms: i64,
    operation: Bootstrap | ListTasks | AutoSession | AddSession | DiscardSession
             | RecordSession | CompleteSession,
    task_id: Option<UUID>,
    locality: Local | Server,
    outcome: Success | Failure,
    summary: String,
}
```

- panelは初期状態で閉じ、利用者が開閉できる。
- requestを送るserver操作はresponse受信時に成否を1件記録する。
- local操作はlocalStorage結果を含む最終成否を1件記録する。
- summaryへ秘密情報、repository path、stack traceを出さない。
- 実行していない`見`、`働`、`終`、`外`などのCLI commandを履歴へ記録しない。

## 11. Compatibility

### 11.1 維持する契約

- CLI`働`以外のcommand文法、renderer出力、current task遷移
- CLIのtask未選択時no-opと成功時だけfocus解除する規則
- MCPのtool名、tool数、JSON schema、required field、default、response、error
- MCP `complete_task`の`task_id`、`finished_at`、`additional_actual_work_seconds`というwire入力
- YAMLを含むtask storage schema
- repository lock、transaction、rollback、state uncertainの区別

### 11.2 意図的に変更する契約

- CLI`働`の記録精度を分単位の更新から秒単位の加算へ変更する。
- CLI`働`は既存実績の秒端数を保持する。
- CLI`働 <minutes>`は負数を拒否する。

## 12. Test specification

### 12.1 Application

- 共通実績加算: 正常加算、0秒、未知UUID、完了済みtask、負数、期待値一致・不一致、加算overflow、失敗時無変更。
- `complete_task`: 期待値一致、競合、負の追加秒、overflow、未完了の子、完了済み、反復task生成、各失敗時の全状態不変。
- 進捗計算: 開始時33%、100%、133%、見積0、長時間、乗算overflow回避。
- buffer: 正、0、負、06:00前後、固定`busy_time_slot`、schedule残作業の反映。
- read model: 指定日、開始時刻順、複数segment、葉判定、締切、候補なしの自動選定。

### 12.2 CLI互換性

- `働`引数なしでfocus開始からの完了済み整数秒を加算する。
- `働 <minutes>`で分の60倍を加算し、既存秒端数を保持する。
- task未選択no-op、0分、負数、時計後退、乗算・加算overflow、保存失敗を検証する。
- 成功時だけfocusを解除し、失敗時は保持する。
- command名、alias、引数個数、正常時出力を既存contract testで固定する。

### 12.3 MCP互換性

- `tools/list` fixtureとgenerated schemaを変更しない。
- `complete_task`の入力default、成功response、application error、repository errorを既存contract testで確認する。
- MCP adapterが`expected_actual_work_seconds: None`を渡すことを確認する。

### 12.4 Client state

- localStorage round trip、version不一致、不正JSON、不正field、同一UUID重複を検証する。
- reload、timer遅延、browser時計後退で開始時刻基準の経過秒になることを検証する。
- session追加・破棄がserver callを生成しないことを検証する。
- server mutation成功、競合、保存失敗、worker停止時のsession遷移を検証する。
- 履歴の100件上限、local/server、成否、reload非永続化を検証する。

### 12.5 UI and integration

- 「セッション」「一覧」、8日button、card、一覧row、色、時刻形式をcomponent testとbrowser目視で確認する。
- 33%、100%、133%、見積0、buffer正負の表示を確認する。
- 通信matrixの各操作についてrequest件数を確認する。
- 2件以上の同時計測とreload復元を確認する。
- UI表示文字列を検索し、「フォーカス」が存在しないことを確認する。
- server featureのtest・clippy、wasm32 check、Dioxus web buildを実行する。
- rootで`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`を実行する。

## 13. 現行UIの置換対象

次は互換性を維持せず、削除または置換してよい。

- `today_text`をそのまま表示する単一画面
- 60秒ごとの自動server refresh
- today text専用のclient refresh state
- today text専用worker commandおよびendpoint
- 現行component階層とCSS

専用worker threadというrepository操作の直列化方針は維持し、today text専用interfaceを5つの型付きWeb操作へ置換する。旧経路を互換目的で残さず、未使用APIとtestを整理する。

## 14. 要件対応表

| 仕様箇所 | 対応要件 |
| --- | --- |
| 2、3、4、8 | REQ-NFR-001..006、REQ-NET-001、REQ-APP-001 |
| 3.4、6.1、7.2 | REQ-SESSION-001..007、REQ-AUTO-001..004 |
| 5 | REQ-APP-001..004、REQ-COMPAT-001..005 |
| 6.2、6.3、7.2 | REQ-CARD-001..012 |
| 4.4、4.5、7.4、9 | REQ-ACTION-001..009 |
| 6.4 | REQ-BUFFER-001..006 |
| 6.5、7.3 | REQ-LIST-001..010 |
| 7.1、8、10 | REQ-COMMON-001..006、REQ-NET-001..006 |
| 11、12 | REQ-COMPAT-001..005、全受入条件 |
