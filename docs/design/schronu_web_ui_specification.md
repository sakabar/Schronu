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
- 曜日・時刻表示: epoch millisecondsをbrowserのlocal timezoneへ変換して生成する

browserとserverは同じlocal machine timezoneで動作することを実行前提とする。logical dateはbrowserでepochから再判定せず、serverの`ServerSnapshot.logical_date`を正とする。browserとserverのtimezone設定が異なる構成、または利用者がbrowserだけ異なるtimezoneで表示する構成は対象外とする。

### 3.2 Success and error envelope

成功responseだけがpayloadと併せて次のsnapshotを返す。

```text
ServerSnapshot {
    observed_at_epoch_ms: i64,
    logical_date: YYYY-MM-DD,
    buffer_seconds: i64,
}
```

`observed_at_epoch_ms`、`logical_date`、`buffer_seconds`は同じserver操作時刻を基準に算出する。clientはresponse受信時刻ではなく`observed_at_epoch_ms`を表示計算の基準とする。

`complete_session`の成功responseは`ServerSnapshot`だけとする。既存`complete_task`が返す次task情報はwireへ含めない。

payloadを持つ成功responseは次の形とする。

```text
WebSuccess<T> {
    snapshot: ServerSnapshot,
    data: T,
}
```

- `bootstrap`: `ServerSnapshot`
- `list_tasks`: `WebSuccess<Vec<ScheduledTaskRow>>`
- `auto_session`: `WebSuccess<Option<SessionTask>>`
- `record_session`: `WebSuccess<RecordSessionResult>`
- `complete_session`: `CompleteSessionResponse` (`ServerSnapshot`のtype alias)

error responseは成功型とは別の次の形とし、snapshotまたは部分的な成功payloadを含めない。

```text
WebError {
    code: String,
    message: String,
    retry_advice: RetryAdvice,
}

RetryAdvice = Retry | ManualCheck
```

`WebError.code`はopenな文字列とする。既知codeは`schronu-web`の`web_error_codes`定数を使って生成し、誤記を防ぐ。clientは未知codeを受信してもdeserializeを失敗させず、`code`、`message`、`retry_advice`をそのまま保持する。wire上の`retry_advice`は`retry`または`manual_check`とする。clientはerror responseを受けても直前の`ServerSnapshot`、一覧、`work_sessions`を置換しない。

Dioxus server functionの戻り値は次の二重`Result`とする。

```text
Result<Result<T, WebError>, ServerFnError>
```

内側はworkerまたはSchronuの操作が返すtyped `WebError`、外側はrequest、responseのserialize、network、server function contextなどのtransport `ServerFnError`を表す。clientは外側の失敗を再試行可能な表示errorへnormalizeするが、codeを`worker_unavailable`とはしない。`worker_unavailable`はworker commandの送信失敗またはresponse channel切断だけに使用する。

### 3.3 Task DTO

セッション開始に必要なtask snapshotは次を持つ。

```text
SessionTask {
    task_id: UUID,
    task_name: String,
    estimated_work_seconds: i64,
    actual_work_seconds: i64,
}
```

一覧の1行はschedule segmentを表し、次を持つ。

```text
ScheduledTaskRow {
    task: SessionTask,
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
2. top-level JSONのparseまたはschema検証に失敗した場合、memory上は空の`work_sessions`とし、warningを表示する。元のkeyは削除・上書きせず、そのpage lifetimeではstorageをwrite blockedとして扱う。
3. `version`が1以外の場合、内容を解釈せず、memory上は空の`work_sessions`とし、warningを表示する。将来versionのdataを失わないよう、元のkeyは削除・上書きせず、そのpage lifetimeではstorageをwrite blockedとして扱う。
4. version 1の個別entryでUUID不正、空のtask名、負の見積・実績、不正なepochがある場合、そのentryだけを除外し、valid entryは採用する。同一UUIDの2件目以降も不正entryとして除外する。
5. 個別entryを除外した初期化時点ではkeyを書き換えない。利用者が次にセッション追加・破棄などのlocal state変更を成功させた時、memory上のvalid entryだけをversion 1として1回で保存する。
6. いずれの復旧経路でもwarningを表示し、task更新を行わず、`bootstrap`を中止しない。
7. storageがwrite blockedでない通常のstate変更では、採用済みの全`work_sessions`を1回で書き戻す。write blockedまたは保存失敗の場合はmemory上の直前stateを維持し、手動でkeyを確認してreloadするようwarningと履歴へ残す。

`repository_state_uncertain`の再送防止状態は、`work_sessions` schemaを拡張せず、別keyの`schronu_web.mutation_safety.v1`へ保存する。

```json
{
  "version": 1,
  "mutation_blocked": true
}
```

このkeyが存在しない場合、またはversion 1の`mutation_blocked`が`false`の場合だけmutation可能な初期状態とする。未知version、JSON不正、schema不正は安全側へ倒し、mutation blockedとして復元する。

`record_session`または`complete_session`の送信前に、`mutation_blocked: true`をstorage-firstで保存する。保存失敗時はrequestを送信しない。成功、またはserverが未commitと確定できるerror responseの受信後、ほかに応答待ちのmutationがなく、repository状態も確定している場合だけ`false`へ戻す。browser crash、transport切断、`repository_state_uncertain`では`true`を残し、reload後も全mutationを停止する。解除はrepositoryを手動確認する明示操作だけが所有し、通常のread成功、session破棄、reloadでは解除しない。解除の保存に失敗した場合もblocked状態を維持する。

## 4. Server operations

専用workerは次の5 commandを順番に処理する。workerへの送信順が実行順となる。

各commandでは`operation_now`を1回だけ取得し、その時刻でrepositoryを`sync_clock`してからapplication操作とsnapshot生成を行う。実績を変更するcommandは、sync済みrepositoryへ変更を適用した後に同じ`operation_now`を基準としてscheduleを再生成し、更新後実績をbufferへ反映する。

### 4.1 `bootstrap`

- 入力: なし
- 成功出力: `ServerSnapshot`
- task dataは変更しない。

### 4.2 `list_tasks(date)`

- 入力: `logical_date: YYYY-MM-DD`
- 成功出力: `WebSuccess<Vec<ScheduledTaskRow>>`
- 指定日のscheduleを取得し、開始epoch milliseconds昇順で返す。
- 指定日を曜日へ変換してCLIの`全 曜日`文字列を実行する実装にはしない。
- task dataは変更しない。

### 4.3 `auto_session`

- 入力: なし
- 成功出力: `WebSuccess<Option<SessionTask>>`
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

成功出力は`WebSuccess<RecordSessionResult>`とし、`RecordSessionResult`は更新後の実績秒を持つ。current taskは参照・変更しない。

### 4.5 `complete_session`

入力は`RecordSessionRequest`と同じとする。

1. `record_session`と同じ規則で追加実績秒を算出する。
2. 既存`CompleteTaskInput`へtask UUID、`operation_now`、追加実績秒、`Some(expected_actual_work_seconds)`を渡す。
3. applicationは期待実績検証、実績加算、完了、終了時刻更新、反復task生成を1つの操作として準備する。
4. repository transactionは全変更を1回で保存する。

成功時は`ServerSnapshot`だけを返す。既存`complete_task`が返す次task情報はWebへ返さず、次taskをSchronu本体のcurrent taskへ設定せず、sessionも自動追加しない。失敗時は他のoperationと同じ`WebError`を返す。

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
scheduled_rows: Vec<ScheduledTaskRow>
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
               - sum(
                   segment.scheduled_work_seconds
                   where logical_date(segment.scheduled_start) == logical_date
                 )
```

`remaining_free_seconds`は06:00境界、Schronu設定、毎週固定の`busy_time_slot`を反映する。単発予定を`busy_time_slot`として追加しない。

予定作業秒の集計規則:

1. repositoryを`observed_at`へsyncした後、既存`get_schedule`から`Vec<ScheduledTaskView>`を生成する。
2. `ScheduledTaskView.scheduled_start`のlogical dateが対象logical dateと一致するsegmentだけを選ぶ。logical date判定は06:00境界を使う。
3. 選んだ各segmentの`scheduled_work_seconds`をchecked additionで1回ずつ合計する。
4. 同一taskが複数segmentを持つ場合もUUIDでまとめず、各segmentをそれぞれ加算する。
5. 進行中segmentも経過分を差し引かず、`scheduled_work_seconds`全量を加算する。
6. sync後のscheduleは通常`observed_at`より前に開始するsegmentを返さない。過去開始のsegmentが返った場合でも、開始時刻のlogical dateが一致すれば除外せず全量を加算する。
7. `record_session`または`complete_session`による実績変更後は、operation開始時にsync済みのrepositoryからscheduleを再生成する。これにより更新後の残作業が同じresponseのbufferへ反映される。

server snapshotのbufferでは進行中segmentから経過秒を引かない。browserは次の式でsnapshot後の経過秒を1回だけ差し引くため、serverとclientで二重減算しない。

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
| 初回表示 | `bootstrap` | なし | なし。復元時に元keyを書き換えない | なし |
| tab切替 | なし | なし | なし | なし |
| 毎秒tick | なし | なし | なし | なし |
| 日付button | `list_tasks` | なし | なし | なし |
| 自動セッション | `auto_session` | なし | session追加 | なし |
| 一覧の「セッション」 | なし | なし | session追加 | なし |
| 破棄して解除 | なし | なし | session削除 | なし |
| 記録して解除 | safety marker保存後に`record_session` | 実績保存1回 | 送信前marker設定。確定応答後marker解除。成功後session削除 | なし |
| 完了 | safety marker保存後に`complete_session` | 完了transaction 1回 | 送信前marker設定。確定応答後marker解除。成功後session削除 | なし |
| repository手動確認済み | なし | なし | safety marker解除 | なし |
| 06:00境界 | なし | なし | なし | なし |

## 9. Error contracts

server errorは少なくとも次を識別可能にする。`retry_advice`が`retry`の場合だけ同じ操作の再試行を案内する。`manual_check`では同一requestをそのまま再送しない。

| code | 条件 | `retry_advice` | client動作 |
| --- | --- | --- | --- |
| `invalid_input` | UUID、日付、epoch、負の経過秒、範囲外 | `manual_check` | 入力の修正、またはsessionの破棄を案内する。 |
| `task_not_found` | UUIDに対応するtaskがない | `manual_check` | task状態の確認、またはsessionの破棄を案内する。 |
| `task_already_completed` | 完了済みtaskを記録・完了しようとした | `manual_check` | task状態の確認、またはsessionの破棄を案内する。 |
| `actual_work_conflict` | 現在実績と期待実績が不一致 | `manual_check` | 実績の確認、またはsessionの破棄を案内する。 |
| `arithmetic_overflow` | 実績、進捗、日時計算が表現範囲外 | `manual_check` | task値または時刻の修正、またはsessionの破棄を案内する。 |
| `task_not_completable` | 未完了の子など既存完了条件を満たさない | `manual_check` | 未完了の子を含むtask状態の修正、またはsessionの破棄を案内する。 |
| `configuration_error` | 設定file、`busy_time_slot`、storage pathなどの設定不正 | `manual_check` | 設定を修正してworkerまたはserviceを再起動するよう案内する。 |
| `repository_unavailable` | lock競合またはrepository load失敗など、mutation開始前の一時的失敗 | `retry` | sessionを保持し、同じ操作の再試行を案内する。 |
| `operation_failed` | 上記codeへ分類できないtask tree、schedule、その他の操作失敗 | `manual_check` | sessionを保持し、task状態の手動確認を案内する。 |
| `repository_save_failed` | storageが未commitと確認できる保存失敗 | `retry` | sessionを保持し、同じ操作の再試行を案内する。 |
| `repository_state_uncertain` | storageへのcommit有無を確定できない保存失敗、またはその発生後に同じserviceがmutationを拒否した場合 | `manual_check` | sessionを保持してmutationを無効化し、repositoryの手動確認とworkerまたはserviceの再起動を要求する。 |
| `worker_unavailable` | worker停止またはresponse channel切断 | `retry` | 既存表示を保持し、接続回復後の再試行を案内する。 |

すべてのerror responseはcodeに対応した利用者向け`message`を持つ。validation、競合、task状態errorは`manual_check`であり、利用者が原因を修正するかsessionを破棄するまで同一requestを再送しない。`repository_state_uncertain`を1回返したserviceはpoisoned状態とし、read操作は許可しても、workerまたはserviceが再起動されるまで後続mutationをrepositoryへ到達させず同じcodeで拒否する。再起動後も、利用者がrepositoryを手動確認するまではclient側でmutationを再送しない。一時的なrepository利用不能、未commitと確定した保存失敗、worker停止だけをtyped errorとして`retry`とする。

未知の`WebError.code`を受信した場合もpayloadを保持し、serverが返した`retry_advice`に従って表示と再送可否を決める。外側の`ServerFnError`は`WebError`ではないため、この表のcodeへ変換せず、client固有のtransport表示errorとして扱う。

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
- `complete_session`: 成功responseが`ServerSnapshot`だけで、次task情報を含まないこと。
- 進捗計算: 開始時33%、100%、133%、見積0、長時間、乗算overflow回避。
- buffer: 正、0、負、06:00前後、固定`busy_time_slot`、隣接logical dateの除外を検証する。
- buffer segment集計: 単一segment、同一taskの複数segment、複数task、進行中segment全量、同一logical date内の過去segment、`scheduled_work_seconds`合計overflowを検証する。
- buffer更新: 実績変更後のschedule再生成と、clientでsnapshot経過秒を1回だけ減算することを検証する。
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

- localStorage round tripを検証する。
- top-level JSON不正とversion不一致では空state、warning、元key維持、storage write blocked、`bootstrap`継続になることを検証する。
- entry不正と同一UUID重複では不正entryだけを除外し、初期化時はkeyを維持し、次のlocal state変更時にvalid entryだけでversion 1を書き戻すことを検証する。
- reload、timer遅延、browser時計後退で開始時刻基準の経過秒になることを検証する。
- session追加・破棄がserver callを生成しないことを検証する。
- server mutation成功、競合、保存失敗、worker停止時のsession遷移を検証する。
- 各endpointの成功型がsnapshotを持ち、error型がsnapshotを持たず、clientがerror時に直前snapshotを維持することを検証する。
- error codeごとの`retry_advice`がerror表と一致し、`manual_check`では同一requestを再送しないことを検証する。
- 履歴の100件上限、local/server、成否、reload非永続化を検証する。

### 12.5 UI and integration

- 「セッション」「一覧」、8日button、card、一覧row、色、時刻形式をcomponent testとbrowser目視で確認する。
- 33%、100%、133%、見積0、buffer正負の表示を確認する。
- 通信matrixの各操作についてrequest件数を確認する。
- 2件以上の同時計測とreload復元を確認する。
- serverと同じlocal timezoneでepoch表示と曜日labelを確認し、logical dateがserver返却値を起点に生成されることを確認する。
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
| 2、3、4、8 | REQ-NFR-001..007、REQ-NET-001、REQ-APP-001 |
| 3.4、6.1、7.2 | REQ-SESSION-001..007、REQ-AUTO-001..004 |
| 5 | REQ-APP-001..004、REQ-COMPAT-001..005 |
| 6.2、6.3、7.2 | REQ-CARD-001..012 |
| 4.4、4.5、7.4、9 | REQ-ACTION-001..009 |
| 6.4 | REQ-BUFFER-001..006 |
| 6.5、7.3 | REQ-LIST-001..010 |
| 7.1、8、10 | REQ-COMMON-001..006、REQ-NET-001..006 |
| 11、12 | REQ-COMPAT-001..005、全受入条件 |
