# Schronu-web UI実装仕様

## 1. Summary

`schronu-web`を、localStorageに保持する複数の`work_sessions`と、Schronuのscheduleを表示するDioxus single-page UIへ置換する。

WebセッションはSchronu本体のcurrent taskと独立させる。Schronuの`get_focus`は自動選定の内部実装としてだけ利用し、UI上の名称は「セッション」に統一する。

本仕様は[UI要件定義](./schronu_web_ui_requirements.md)の実装契約を定める。

## 2. 構成と責務

| 層 | 責務 |
| --- | --- |
| Dioxus component | tab、button、card、一覧、error、履歴、持ち歩きロックbarの描画と利用者操作の受付。 |
| client state | `work_sessions`、snapshot、選択日、一覧、処理中操作、履歴、1秒tick、持ち歩きロックを管理する。 |
| localStorage adapter | `work_sessions`、mutation safety、持ち歩きロックの独立したversion付きstateを読込・検証・保存する。 |
| server function | wire DTOを検証し、専用workerへ型付きcommandを送る。 |
| Web operation worker | 1 thread上でWeb操作を直列実行し、environment、repository、free-time資源を所有する。 |
| Web controller service | application use caseを組み合わせ、snapshot、一覧、自動選定、記録、計測を記録する完了、計測を破棄する完了を提供する。 |
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
    current_actual_work_seconds: Option<i64>,
}

RetryAdvice = Retry | ManualCheck
```

`WebError.code`はopenな文字列とする。既知codeは`schronu-web`の`web_error_codes`定数を使って生成し、誤記を防ぐ。`current_actual_work_seconds`は`actual_work_conflict`だけが設定し、それ以外ではwireから省略する。clientはfieldのない旧payloadと未知codeを受信してもdeserializeを失敗させず、`code`、`message`、`retry_advice`を保持する。wire上の`retry_advice`は`retry`または`manual_check`とする。clientはerror responseを受けても直前の`ServerSnapshot`、一覧、`work_sessions`を置換しない。

Dioxus server functionの戻り値は次の二重`Result`とする。

```text
Result<Result<T, WebError>, ServerFnError>
```

内側はworkerまたはSchronuの操作が返すtyped `WebError`、外側はrequest、responseのserialize、network、server function contextなどのtransport `ServerFnError`を表す。clientは外側の失敗をcodeなしのtransport表示errorへnormalizeする。read操作のtransport失敗は再試行可能とするが、mutationではserver commitの成否が分からないため再試行不可とし、repositoryの手動確認を要求する。`worker_unavailable`はworker commandの送信失敗またはresponse channel切断だけに使用する。

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

`is_leaf`はwire互換のため名称を維持するが、task tree上の`child_ids`の空否ではなく、schedule計算結果の`ScheduledTaskView.rank == 0`を表す。rank 0は未完了の子を持たないtaskである。

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
7. storageがwrite blockedでない通常のstate変更では、採用済みの全`work_sessions`を1回で書き戻す。write blockedまたは保存失敗の場合はmemory上の直前stateを維持し、手動でkeyを確認してreloadするようwarningを表示する。

`repository_state_uncertain`の再送防止状態は、`work_sessions` schemaを拡張せず、別keyの`schronu_web.mutation_safety.v1`へ保存する。

```json
{
  "version": 1,
  "mutation_blocked": true,
  "committed_task_ids": ["task UUID"]
}
```

このkeyが存在しない場合、またはversion 1の`mutation_blocked`が`false`の場合だけmutation可能な初期状態とする。`committed_task_ids`はserver commit成功後にlocal session削除だけが失敗したtask UUIDを保持し、省略された既存dataは空配列として読み込む。未知version、JSON不正、schema不正は安全側へ倒し、mutation blockedとして復元する。

`record_session`または`complete_session`の送信前に、`mutation_blocked: true`をstorage-firstで保存する。保存失敗時はrequestを送信しない。成功、またはserverが未commitと確定できるerror responseの受信後、ほかに応答待ちのmutationがなく、repository状態も確定している場合だけ`false`へ戻す。browser crash、transport切断、`repository_state_uncertain`では`true`を残し、reload後も全mutationを停止する。解除はrepositoryを手動確認する明示操作だけが所有し、通常のread成功、session破棄、reloadでは解除しない。server commit後にlocal session削除だけが失敗している場合はtask UUIDを`committed_task_ids`へstorage-firstで追加し、reload後も再送とbuffer補正の対象外にする。明示解除は該当sessionを`work_sessions`からstorage-firstで削除してからmarkerと`committed_task_ids`を解除する。session削除に失敗した場合はmarkerを解除しない。session削除後のmarker解除に失敗した場合もblocked状態を維持するが、該当sessionは既に永続層から消えているため二重送信できない。transportまたは`repository_state_uncertain`由来の未確定sessionは、手動確認結果に基づく再操作のため残す。

持ち歩きロックは`MutationSafetyState`とは目的と解除条件が異なるため、独立した`CarryLockState`とkey `schronu_web.carry_lock.v1`を使用する。

```json
{
  "version": 1,
  "enabled": true
}
```

- keyがなければ通常モードとする。version 1の正常値は`enabled`を復元するが、一時許可の期限は保存せず、`enabled: true`のreload後はロック状態とする。
- JSON・schema不正、未知version、読込失敗はwarning付きのロック状態とし、元のvalueを削除・上書きしない。
- 有効化はmemory-firstとし、保存に失敗しても現在のpageではロックを維持し、reload後に維持できない可能性をwarning表示する。
- 通常モードへの復帰はstorage-firstとし、`enabled: false`の保存成功後だけmemory stateを通常モードへ変える。保存失敗時はロックを維持する。

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
    ended_at_epoch_ms: Option<i64>,
    expected_actual_work_seconds: i64,
}
```

処理:

1. 1回だけ取得したserver操作時刻を`operation_now`とする。
2. `ended_at_epoch_ms`があればbrowserの終了操作click時刻、なければ旧client互換のため`operation_now`を計測終了時刻とする。
3. `floor((ended_at - started_at) / 1000)`を追加実績秒とする。
4. `started_at <= ended_at`を満たさない場合、日時変換不能、秒数変換不能なら入力errorとし、保存しない。browser時計とserver時計の差により`ended_at > operation_now`でも、browser内の時刻差を計測へ使用する。
5. UUID、追加実績秒、期待実績秒をapplicationの共通実績加算操作へ渡す。
6. repository transactionの保存成功後に`ServerSnapshot`を返す。

成功出力は`WebSuccess<RecordSessionResult>`とし、`RecordSessionResult`は更新後の実績秒を持つ。current taskは参照・変更しない。

### 4.5 `complete_session`

入力:

```text
CompleteSessionRequest {
    task_id: UUID,
    started_at_epoch_ms: i64,
    ended_at_epoch_ms: Option<i64>,
    expected_actual_work_seconds: i64,
    record_elapsed_seconds: bool,
}
```

1. `ended_at_epoch_ms`があればbrowserの終了操作click時刻、なければ旧client互換のため`operation_now`を計測終了時刻とする。保存用の`finished_at`は`min(ended_at, operation_now)`とし、browser時計が進んでいても未来時刻を保存しない。
2. `record_elapsed_seconds`が`true`なら、`record_session`と同じ規則で追加実績秒を算出する。
3. `record_elapsed_seconds`が`false`なら、`started_at_epoch_ms`を実績計算やvalidationに使用せず、追加実績秒を0とする。
4. 既存`CompleteTaskInput`へtask UUID、保存用の終了時刻、追加実績秒、`Some(expected_actual_work_seconds)`を渡す。
5. applicationは期待実績検証、実績加算、完了、終了時刻更新、反復task生成を1つの操作として準備する。
6. repository transactionは全変更を1回で保存する。repository同期とresponse snapshot生成は`operation_now`を維持し、click後の通信待ちを未作業時間として反映する。

`record_elapsed_seconds`の値にかかわらず期待実績競合、未完了child、反復task生成、保存、安全停止は同じ完了経路で処理する。成功時は`ServerSnapshot`だけを返す。既存`complete_task`が返す次task情報はWebへ返さず、次taskをSchronu本体のcurrent taskへ設定せず、sessionも自動追加しない。失敗時は他のoperationと同じ`WebError`を返す。

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
- Webの`complete_session`は、`record_elapsed_seconds`が`true`なら経過秒、`false`なら0を追加実績秒として渡す。
- MCPの入力structおよび生成JSON schemaへこのfieldを追加しない。

## 6. Client state and calculations

### 6.1 State

clientは最低限、次を保持する。

```text
active_tab: Session | List | History
work_sessions: Vec<WorkSession>
server_snapshot: Option<ServerSnapshot>
date_buttons: Vec<LogicalDateButton>
selected_logical_date: Option<YYYY-MM-DD>
scheduled_rows: Vec<ScheduledTaskRow>
task_name_filter: String
date_input: DateInputState { text, error }
in_flight_session_ids: Set<UUID>
pending_session_ended_at: Map<UUID, epoch_ms>
completion_conflicts: Map<UUID, (original CompleteSessionRequest, original ended_at, latest actual)>
page_error: Option<DisplayError>
operation_history: VecDeque<OperationHistoryEntry>
tick_now_epoch_ms: i64
```

`operation_history`は最大100件とし、101件目の追加前に最古のentryを削除する。localStorageへ保存しない。

### 6.2 経過時間

```text
display_now = pending_session_ended_at.get(task_id).unwrap_or(tick_now_epoch_ms)
elapsed_seconds = max(0, floor((display_now - started_at_epoch_ms) / 1000))
```

毎秒tickは`tick_now_epoch_ms`だけを更新する。経過秒をincrementして保持しないため、tab非表示、timer遅延、reloadを挟んでも開始時刻基準で復元できる。終了操作はbrowser壁時計を同期的に取得し、pending中はそのclick時刻で表示を停止する。完了の実績競合は元requestと終了時刻を専用stateへ移してcardとbufferを停止し続ける。それ以外でserverが未commitと確定できるerror時はpending終了時刻を破棄し、現在時刻基準へ戻す。transport切断または`repository_state_uncertain`では終了時刻をmemory上に保持し、repository確認完了まで表示を停止する。

完了競合の再送は元requestから期待実績だけを最新値へ置換し、新しいrequest IDで同じ`complete_session`を送る。再競合時は元requestを維持して最新実績だけを更新する。計測再開時は`measured_ms = max(0, original_ended_at - original_started_at)`、`new_started_at = tick_now - measured_ms`をchecked arithmeticで求め、開始時実績を最新値へ置換した全session候補をlocalStorageへ保存する。memory state、競合、errorは保存成功後だけ更新する。

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
- 残り・超過`MM:SS`は「残り」または「超過」と組み合わせた大きな主表示とする。開始`HH:MM`、矢印、完了予定`HH:MM`、開始時実績`MM:SS`はその下の補助情報領域へ置き、必要なら項目単位で折り返して320px幅でもcardを横へ超過させない。開始と完了予定は`time`要素とし、残り・超過および開始時実績を含む各値へ意味を識別できるARIA labelを付ける。完了予定を算出できない場合も同じ位置へ`--:--`を表示する。
- `worked_seconds * 100`はoverflowしない計算方法を用いる。
- 通常bar幅は`min(progress, 100) / 150 * 100%`とし、100%進捗をtrack全幅の3分の2に置く。
- 超過bar幅は`max(progress - 100, 0) / 150 * 100%`で、100%位置の右側へ赤色で連結する。150%でtrack全幅へ到達し、それを超えた分はcard内で切り捨てず、横scroll可能な表示領域を確保する。
- 100%位置には、通常bar、未塗り領域、超過barのいずれの上でも常時視認できる2pxの縦線を装飾要素として置き、assistive technologyからは隠す。

### 6.4 buffer

server側:

```text
end_of_day = end_of_day(logical_date)

remaining_capacity_seconds =
  observed_at < end_of_day:
    free_seconds(observed_at, end_of_day, weekly_busy_time_slots)
  observed_at >= end_of_day:
    seconds(end_of_day - observed_at) // 0以下

buffer_seconds = remaining_capacity_seconds
               - sum(
                   segment.scheduled_work_seconds
                   where logical_date(segment.scheduled_start) == logical_date
                 )
```

`remaining_capacity_seconds`は現在logical dateに対する符号付き残り容量である。`observed_at`が日次終端より前なら、06:00境界、Schronu設定、毎週固定の`busy_time_slot`を反映した残り空き秒とする。単発予定を`busy_time_slot`として追加しない。`observed_at`が日次終端ちょうどなら0、日次終端より後なら`end_of_day - observed_at`の負の秒数とし、`busy_time_slot`に関係なく日次終端後の全壁時計超過時間を含める。

例えば日次終端が00:30、`observed_at`が01:10、同じlogical dateの予定残作業が62分なら、`remaining_capacity_seconds`は-40分、`buffer_seconds`は`-40分 - 62分 = -01:42:00`となる。

予定作業秒の集計規則:

1. repositoryを`observed_at`へsyncした後、既存`get_schedule`から`Vec<ScheduledTaskView>`を生成する。
2. `ScheduledTaskView.scheduled_start`のlogical dateが対象logical dateと一致するsegmentだけを選ぶ。logical date判定は06:00境界を使う。
3. 選んだ各segmentの`scheduled_work_seconds`をchecked additionで1回ずつ合計する。
4. 同一taskが複数segmentを持つ場合もUUIDでまとめず、各segmentをそれぞれ加算する。
5. 進行中segmentも経過分を差し引かず、`scheduled_work_seconds`全量を加算する。
6. sync後のscheduleは通常`observed_at`より前に開始するsegmentを返さない。過去開始のsegmentが返った場合でも、開始時刻のlogical dateが一致すれば除外せず全量を加算する。
7. `record_session`または`complete_session`による実績変更後は、operation開始時にsync済みのrepositoryからscheduleを再生成する。これにより更新後の残作業が同じresponseのbufferへ反映される。

server snapshotのbufferでは進行中segmentから経過秒を引かない。browserはserver snapshot後の壁時計経過秒を1回減算し、serverへ未送信の各セッション進捗秒を個別に加算する。この規則はserver snapshotの観測時刻が日次終端の前後かどうかとbufferの正負に依存しない。

client側:

```text
snapshot_elapsed = max(0, floor((tick_now - observed_at) / 1000))
buffer_sessions = work_sessions - server commit済みでlocal削除に失敗したsessions
stopped_at(session) = 終了処理中またはrepository状態不確実なsessionのclick時刻
estimated_completion(session) =
  session.started_at + max(session.estimated_at_start - session.actual_at_start, 0)
protected_until(session) = min(
  estimated_completion(session),
  session.stopped_atがあればその時刻
)
credit_until = max(tick_now, observed_at)
normal_session_credit = sum(
  max(0, floor((min(credit_until, protected_until(session)) - session.started_at) / 1000))
  for each completion conflict中ではないbuffer_session
)
conflict_session_credit(session) =
  初回click <= estimated_completionまたはestimated_completionを算出不能:
    max(0, floor((credit_until - session.started_at) / 1000))
  初回click > estimated_completion:
    max(0, floor((estimated_completion - session.started_at) / 1000))
    + max(0, floor((credit_until - 初回click) / 1000))
session_credit = normal_session_credit + sum(conflict_session_credit)

display_buffer = buffer_seconds - snapshot_elapsed + session_credit
```

`protected_until`は定義できる終端のうち最も早い時刻とする。完了実績競合の確認中と再送中は、初回clickが見積到達以前なら開始から競合解消までを連続して加算し、見積到達後なら通常の未送信進捗と初回click以後の相殺時間を分けて加算する。これにより初回click時点のbuffer表示を固定し、click以前に減算済みのbufferを巻き戻さない。見積到達時刻がepoch範囲外で算出不能かつ`stopped_at`もない場合は終端なしとし、終了まで未送信進捗を加算する。

- `snapshot_elapsed`はserver観測時刻からbrowser現在時刻までのbuffer減算として1回だけ差し引く。
- 新しいserver responseを受信した場合は、その`buffer_seconds`と`observed_at`を新たな表示計算の基準とする。page内で開始したセッションとlocalStorageから復元したセッションを区別せず、観測時刻以前も含む未送信進捗秒を新基準へ加算する。
- `record_session`または`complete_session`のmutation responseは、対象実績を反映した`buffer_seconds`をそのまま新たな基準とする。server commit済みでlocalStorage削除だけに失敗した対象sessionは、以後のbuffer計算上の計測中sessionから除外する。
- 終了操作をdispatchしたsessionはclick時刻と見積到達時刻の早い方で未送信進捗の加算を打ち切る。未commitが確定するerrorでは対象を計測中へ戻し、transport切断または`repository_state_uncertain`ではrepository確認完了までclick時刻の終端を保持する。完了実績競合では初回click後の壁時計減算を相殺し、確認中と再送中の表示を固定する。成功時はresponseのsnapshotを新たな基準とするため、通信待ち時間を未送信進捗へ加算しない。
- 複数の計測中セッションは重複区間を除かず、各セッションの完了済み整数秒を個別に合算する。2件が10分ずつ同時計測された場合は20分を加算する。
- 「計測を破棄して解除」成功後は残存セッションから式全体を再計算し、破棄したセッション分の未送信進捗を加算しない。全件破棄した場合はsnapshot後の全経過秒を減算する。localStorage保存失敗時はmemory stateを確定しないため、buffer表示も変化させない。
- browser時計が後退した区間は0秒へclampする。時刻差と加減算は`i64`境界でもoverflowしない計算を用いる。
- `display_buffer >= 0`: 通常色の`HH:MM:SS`
- `display_buffer < 0`: 赤色の`-HH:MM:SS`
- hourは総時間とし、24以上もそのまま表示する。

### 6.5 logical date buttons

最新の`ServerSnapshot.logical_date`をindex 0として8日分を生成する。

- index 0: `曜 今日`
- index 1: `曜 明日`
- index 2..7: `曜`

各buttonは表示labelとは別に具体的な`YYYY-MM-DD`を保持する。新しいserver responseでlogical dateが変わった場合はbuttonを再生成する。4種類のセッション終了成功後は選択中のlogical dateを維持し、未選択なら最新snapshotのlogical dateを選んで`list_tasks`を自動実行する。

日付文字列入力はpage内だけの`DateInputState`に保持し、`M/D`または`YYYY/M/D`のASCII数字とslashだけを受け付ける。前後空白は除去する。年省略時は`ServerSnapshot.logical_date`の年から候補日を探し、現在logical date以上となる最初の有効なcalendar日付を採用する。同じ月日は当日として扱い、`2/29`は必要なら次の閏年まで進める。妥当な入力は表示用`YYYY/M/D`とwire用`YYYY-MM-DD`へ正規化し、不正な形式・calendar日付・範囲overflowは型付きerrorとする。

### 6.6 持ち歩きロックstate

`CarryLockState`は`Normal`、`Locked`、`ArmedUntil(monotonic_deadline_ms)`を持つ。`ArmedUntil`は`Performance.now()`相当の単調時計を基準に15秒後を期限とし、セッション経過時間などに使う壁時計とは分離する。一時許可の残り秒数も単調時計から算出する。単調時計が後退した場合も安全側へ倒して`Locked`へ戻す。

すべてのcomponent actionは同じreducerを通し、reducerはaction処理前に期限を観測する。`AutoSession`、`AddSession`、`DiscardSession`、`RecordSession`、`CompleteSession`、`CompleteSessionWithoutRecording`、`ResumeCompletionConflict`、`ConfirmCompletionConflict`、`ConfirmRepositoryChecked`を変更操作とする。`Locked`ではこれらをeffectなしで拒否し、`ArmedUntil`ではdispatch時点の単調時刻から15秒後へ期限を更新してから処理する。成功、失敗、local stateが実際に変化したかには依存しない。tab切替、tick、日付選択とresponse適用では期限を更新しない。

## 7. UI behavior

### 7.1 初期化とtab

1. localStorageを読み、`work_sessions`を復元する。
2. `bootstrap`を1回送る。
3. responseからbufferと8日buttonを表示する。bufferは復元した各セッションの未送信進捗秒をserver bufferへ個別に加算し、見積到達時刻またはそれより早い終了click時刻で加算を打ち切る。
4. 初期tabは「セッション」とする。
5. viewport下端へ「セッション」「一覧」「発火履歴」の3tabを固定し、選択中だけ上端の緑indicatorと`aria-pressed: true`を付ける。各buttonは均等幅とし、操作高はdesktopで44px以上、46rem以下で40px以上とする。
6. tab barはsafe areaをpaddingへ含め、全幅かつ最大82remで中央配置する。本文末尾にはbar高、safe area、余白の合計を確保し、通信中overlayより低い`z-index`にする。
7. tab切替だけでは一覧取得を含むserver操作を行わず、選択中の1画面だけをDOMへ描画する。タイトルとtoolbarは描画せず、持ち歩きロックbarとbufferはセッションtabだけに表示する。持ち歩きロックstateとmutation guardはtabにかかわらず有効にする。
8. セッションtab表示中にセッション件数が実際に減少して0件になった場合は、既存のtab切替処理で一覧tabへ移る。件数不変、セッションが残る場合、一覧または発火履歴tab表示中は強制遷移しない。

client componentは非`None`の`ClientEffect`をserverへdispatchする直前に実行中通信数を1増やし、response受理後に成否にかかわらず1減らす。実行中通信数が1以上の間は、viewport全体を覆う半透明overlay、スピナー、「通信中…」を表示する。背面の`main`に`inert`と`aria-busy`を設定し、pointerとkeyboard操作を無効にする。overlayのstatusは`aria-live=polite`で通知する。`prefers-reduced-motion: reduce`ではスピナーの回転を停止するが、待機表示自体は維持する。

初回SSR、browser初期化前、`bootstrap`応答待ちは`schronu-web-loading` IDの専用rootだけを描画し、BUFFER要素を含めない。snapshot取得前に`bootstrap`が失敗した場合はoverlayを外し、errorを持つ`schronu-web-load-error` rootへ切り替えるが、BUFFER要素は追加しない。snapshot取得成功後は`schronu-web-ready` root内に、確定値だけを受け取る`schronu-buffer-ready` IDのBUFFER要素を描画する。以後のserver通信中はready rootと最後の確定BUFFERを維持したままoverlayを重ねる。

34rem以下ではbuffer領域を圧縮する。46rem以下の一覧画面では日付buttonと日付入力・表示buttonを高さ36px、日付領域の上下paddingを`0.125rem`と`0.25rem`へ圧縮し、8日分の横スクロールを維持する。日付入力はtask名検索の上へ積み、320px幅でもviewportを超えないようにする。

全buttonの`:hover`装飾は`@media (hover: hover) and (pointer: fine)`内だけに定義し、タッチ主体の端末ではタップ後にhover配色を残さない。`:active`と`:focus-visible`はmedia query外に置き、pointer種別にかかわらず操作feedbackを維持する。hover可能なfine pointerではtab、primary action、session startを含む既存hover表現を維持し、選択済み日付buttonのhover中は緑背景と白文字を上書き規則で維持する。

### 7.2 セッション画面

- 初期化時など、削除を伴わずセッション0件でセッションtabを表示している場合は「自動セッション」buttonを表示する。
- 1件以上ではbuttonを隠し、各`work_session`をcard表示する。
- cardはtask名、開始`HH:MM`、完了予定`HH:MM`、開始時実績`MM:SS`、進捗率、bar、残り・超過`MM:SS`、「計測を破棄して解除」「記録して解除」「計測を破棄して完了」「記録して完了」の4操作buttonを持つ。timing領域は残り・超過を大きな主表示、開始、矢印、完了予定、開始時実績を折り返し可能な補助表示とし、mobileのgridをtask名、timing、progress、操作の順にする。
- 操作buttonは意味別classを持ち、通常幅では解除系2つと完了系2つをそれぞれ同じ段に配置し、狭い画面では1列にする。
- 「計測を破棄して完了」をclickすると当該cardだけを確認表示へ切り替え、「このセッションの計測時間は記録されません。タスクを完了しますか?」と「キャンセル」「計測を破棄して完了」を表示する。最初のclickとキャンセルではserver requestを送らず、確定時だけ`record_elapsed_seconds: false`の`complete_session`を1回送る。
- 「記録して完了」は確認を挟まず、`record_elapsed_seconds: true`の`complete_session`を送る。
- 「記録して解除」と2種類の完了確定ではclick時刻を`ended_at_epoch_ms`として送信し、応答待ち中は対象cardの進捗、bar、残り・超過時間をその時刻で停止する。「計測を破棄して完了」の確認表示だけでは停止しない。
- 「自動セッション」成功時はresponseのtask snapshotから現在時刻を開始時刻とするsessionを追加する。
- 自動選定結果が`None`なら空状態と案内を表示する。

### 7.3 一覧画面

- 日付button click時と4種類のセッション終了成功後に`list_tasks(date)`を送る。
- 日付button直下、task table直上へ日付入力とtask名検索の操作領域を置く。desktopでは固定幅の日付入力を検索の左、46rem以下では検索の上に配置する。
- 日付入力は`M/D`と`YYYY/M/D`を受け、Enterと「表示」のどちらでも送信する。空または空白だけなら何もせず、不正入力は`aria-invalid`と説明要素でfieldに関連付けたerrorを表示する。妥当な入力は`YYYY/M/D`へ正規化してtab切替後も保持し、日付buttonを選択した場合は入力とerrorを消去する。入力編集・不正送信・clearはserver通信、localStorage更新、発火履歴追加を行わず、妥当な送信だけ既存の日付選択経路へ正規化済み`YYYY-MM-DD`を渡す。
- task名検索文字列はpage内だけに保持し、日付・tab切替では維持、reloadでは空へ戻す。一覧からセッションをlocalStorageへ追加できた場合だけ空へ戻し、選択日と日付入力は維持する。追加の保存失敗、重複、rank非0、持ち歩きロックによる拒否時は検索文字列も維持する。localStorageへは保存しない。
- 入力の前後空白を除外して小文字化し、task名を小文字化した文字列への部分一致で取得済みrowを即時に絞り込む。空または空白だけなら全rowを表示し、Unicode正規化と全角・半角変換は行わない。同一taskの複数segmentは一致する全rowを残し、新しい日付のresponseにも保持中の条件を適用する。
- 生の入力が空でない間だけ「×」のclear buttonを表示し、`aria-label`を「検索文字列をクリア」とする。46rem以下では検索欄を高さ36px、clear buttonを36px四方、曜日・検索・table間を8pxにする。clearは検索文字列を空にして全rowを再表示し、DOMから消えるclear buttonにあったkeyboard focusを検索欄へ戻す。検索条件が空でなく一致rowが0件なら、tableの代わりに`role=status`で「一致するタスクがありません。」と表示する。
- 検索入力とclearはclient component内だけで処理し、server通信、task更新、localStorage更新、発火履歴追加を行わない。持ち歩きロック中も利用できるが、通信中overlayの`inert`はほかの背面操作と同様に適用する。
- rowは締切、予定`HH:MM-HH:MM`、task名を表示し、開始可能なrowにはセッション追加buttonも表示する。46remを超える画面では表示labelを「セッション」、46rem以下では「＋」とし、ARIA labelはtask名とセッション追加操作を表す。左スワイプは追加操作として扱わず、buttonのclickだけで追加する。
- 締切は選択logical date内なら`HH:MM`、それ以外は`MM/DD HH:MM`とする。現在epochが締切epochを超えた場合に赤くする。
- schedule rankが0のとき`is_leaf`をtrueとし、そのtask名を緑にする。
- `is_leaf == true`のrowだけに「セッション」buttonを表示する。`is_leaf == false`のrowではbuttonとclick listenerを生成せず、client stateへ手動追加要求が直接渡されても拒否する。
- 「セッション」click時はrowのtask snapshotと`is_leaf`、client現在時刻からsessionを作り、localStorageへ保存する。追加成功後はtask名検索文字列を空にして取得済みrowへの絞り込みを解除し、セッションtabへ切り替える。追加前後のsession件数が増えた場合だけ成功とし、検索解除でserver通信、localStorage更新、発火履歴追加を発生させない。
- `work_sessions`に同一UUIDがあれば、そのUUIDの全rowでbuttonをdisabledにする。46rem以下では追加済みを「✓」で示し、ARIA labelも追加済みであることを表す。
- 4種類のセッション終了成功後は選択中、または未選択なら最新snapshotのlogical dateを再取得し、表示中の一覧をresponse全体で置換する。
- 完了成功response受理時点でin-flightの`list_tasks` requestを無効化する。その後に到着した無効化済みrequestのresponseは適用せず、完了taskのrowが復活することを防ぐ。完了成功response後に開始した再取得と、さらに後から利用者が明示した日付取得は通常どおり適用する。
- 完了によって生成された反復taskは、終了成功後の一覧再取得responseに含まれる場合に表示する。
- 全幅でheaderとrowをセッション追加、予定、締切、task名の順に置き、可視headerとtable semanticsを維持する。46rem以下ではtable全体の横スクロールを解除し、`44px 5.75rem 5.5rem minmax(0, 1fr)`の4列gridにする。行の文字はtask名を`0.75rem`、予定と締切を`0.68rem`とする。
- mobile rowは32px以上の1行とし、row間を罫線だけで区切る。cellの上下paddingは`0.125rem`とし、card用の行間、角丸、影は使用しない。セッション追加cellは未追加のrank 0で幅44px・高さ32pxの「＋」、追加済みで同寸法かつdisabledの「✓」、rank非0で空cellとする。
- 締切と予定は小さい等幅数字の固定列として折り返さず、既存formatを省略しない。task名だけを`min-width: 0`、`white-space: nowrap`、`overflow-x: auto`、`overflow-y: hidden`としてcell内で横スクロール可能にし、縦scrollbarを生成せず全文をDOMへ保持する。task名のscroll領域はkeyboard focusとfocus-visible表示を持ち、横panがpage全体の横移動へ伝播しないようにする。

### 7.4 操作結果

- localStorage更新は、memory state確定前に保存成功を確認する。
- 「計測を破棄して解除」はlocalStorage削除成功後だけmemory stateを確定し、残存する計測中セッションからbufferを再計算する。task実績を更新するserver mutationは行わず、成功後の一覧再取得だけを行う。
- server mutationは、response成功後にlocalStorageからsessionを削除する。
- server errorまたはlocalStorage削除失敗ではsessionを残す。server保存成功後にlocalStorage削除だけが失敗した場合、responseの更新後実績を反映した競合案内を表示し、再送による二重加算を防ぐため対象buttonを無効化し、対象sessionをbuffer計算上の計測中sessionから除外する。
- component orchestratorはactionまたはresponse適用前後のsession件数を共通判定へ渡す。active tabがセッションで、件数が実際に減少して0件になった場合だけ一覧tabへ切り替える。即時削除、server成功後の削除、repository確認済みの削除を同じ判定へ通し、tab切替自体はeffectを生成しない。
- serverが未commitと確定できるerrorではpending終了時刻を破棄し、対象sessionの表示と未送信進捗の加算を現在時刻基準で自動再開する。transport切断または`repository_state_uncertain`では終了時刻を保持し、repository確認完了時に破棄して再開する。
- 4種類の終了成功では一覧再取得effectを生成し、response全体で一覧を置換する。server errorでは再取得せず、server commit成功後のlocalStorage削除失敗では安全状態を維持しつつ再取得する。
- 完了responseの`ServerSnapshot`は通常どおり適用し、logical dateが変わった場合は日付buttonを再生成する。一覧再取得には選択中のlogical dateを維持して用い、未選択なら最新snapshotのlogical dateを用いる。
- in-flight中は対象sessionの4buttonを無効化する。他sessionの計測は継続する。globalまたはmanual safety block中はserver mutationの3buttonを無効化し、「計測を破棄して解除」は利用可能とする。

### 7.5 持ち歩きロックbar

- page上部へstickyなbarを常時表示する。持ち歩きロックだけを理由に画面を覆うoverlayや内容の非表示は行わず、ロック中もbuffer・セッション・一覧の表示と更新、scroll、tab切替、日付選択、一覧取得を維持する。一覧取得を含むserver通信のdispatch後は、response受理まで通信中overlayによる全面操作遮断を優先する。
- `Normal`では「持ち歩きロック」を1 clickすると即時に有効化する。`Locked`では独立した状態blockを生成せず、44px以上の長押しbutton内へ主文言「操作ロック中」と補足「1.2秒長押しで15秒間操作可能」を横並びで集約し、通常モードへ戻す`details`だけを次の行へ置く。barのpaddingとgapを抑え、34rem以下でもbar全体を汎用的な縦積みに切り替えない。`ArmedUntil`では「操作可能」と残り秒数を表示する。
- `Locked`の長押しbuttonはprimary pointer、Space、Enterを受け付ける。pointerup、pointerleave、pointercancel、buttonのblur、window scroll、または1.2秒未満のkeyupでtimerを破棄し、stale timerが発火しても許可しない。keyboard auto-repeatは新しい長押しを開始しない。
- 状態名だけを`aria-live=polite`で通知する。`ArmedUntil`の残り秒数はlive regionの外へ置き、毎秒読み上げない。
- 「計測を破棄して完了」の確認表示は変更操作に含めず、確定dispatchだけが無操作期限を更新する。キャンセルは`ArmedUntil`と期限を維持し、期限切れでは確認表示を閉じる。
- 完了実績競合の確認は元の完了dispatch後も維持する。「加算して完了」「実績を維持して完了」「計測を再開」は共通guardを通る別の変更操作とし、一時許可中は各dispatchから15秒後へ無操作期限を更新する。
- 通常モードへの復帰はbar内の`details`に置き、「確認して解除」の操作だけが永続解除を要求する。

## 8. Communication and persistence matrix

| 操作 | server通信 | task保存 | localStorage変更 | 表示中一覧 | current task変更 |
| --- | --- | --- | --- | --- | --- |
| 初回表示 | `bootstrap` | なし | なし。復元時に元keyを書き換えない | なし | なし |
| tab切替 | なし | なし | なし | なし | なし |
| 毎秒tick | なし | なし | なし。client stateからbufferを再計算 | なし | なし |
| 一覧検索の入力・clear | なし | なし | なし | 取得済みrowをclient内で絞り込み | なし |
| 日付button | `list_tasks` | なし | なし | responseのrowへ置換 | なし |
| 自動セッション | `auto_session` | なし | session追加 | なし | なし |
| 一覧の「セッション」 | なし | なし | session追加 | 追加成功後にセッションtabへ切替 | なし |
| 計測を破棄して解除 | session削除成功後に`list_tasks` | なし | session削除。成功後にbuffer再計算 | 一覧再取得responseで置換 | なし |
| 記録して解除 | click時刻付きでsafety marker保存後に`record_session`。成功後に`list_tasks` | clickまでの実績保存1回 | 送信前marker設定とtimer停止。確定応答後marker解除。成功後session削除 | 一覧再取得responseで置換 | なし |
| 計測を破棄して完了の確認・キャンセル | なし | なし | card内の一時的な確認状態だけを変更 | なし | なし |
| 計測を破棄して完了の確定 | click時刻付きでsafety marker保存後に`complete_session(record_elapsed_seconds: false)`。成功後に`list_tasks` | 追加実績0、click時刻で完了するtransaction 1回 | 送信前marker設定とtimer停止。確定応答後marker解除。成功後session削除 | 一覧再取得responseで置換 | なし |
| 記録して完了 | click時刻付きでsafety marker保存後に`complete_session(record_elapsed_seconds: true)`。成功後に`list_tasks` | clickまでの経過秒を加算し、click時刻で完了するtransaction 1回 | 送信前marker設定とtimer停止。確定応答後marker解除。成功後session削除 | 一覧再取得responseで置換 | なし |
| 完了実績競合の再完了 | 元requestの期待実績だけを最新値へ変更し、safety marker保存後に`complete_session`。成功後に`list_tasks` | 最新実績とのCAS成功時だけ元の記録方針で完了 | 初回click時刻の停止を維持。再競合は最新値を更新し、成功後session削除 | 一覧再取得responseで置換 | なし |
| 完了実績競合の計測再開 | なし | なし | 確認待ちを除いた開始時刻と最新実績でsessionを原子的に置換 | なし | なし |
| repository手動確認済み | なし | なし | commit済みで削除失敗したsessionを先に削除し、safety marker解除 | なし | なし |
| 持ち歩きロック有効化 | なし | なし | `enabled: true`を保存。失敗時もmemory上はロック | なし | なし |
| 持ち歩きロック一時許可 | なし | なし | なし。15秒の期限はmemoryだけ | なし | なし |
| 持ち歩きロック解除 | なし | なし | `enabled: false`の保存成功後だけ解除 | なし | なし |
| 06:00境界 | なし | なし | なし | なし | なし |

## 9. Error contracts

server errorは少なくとも次を識別可能にする。`retry_advice`が`retry`の場合だけ同じ操作の再試行を案内する。`manual_check`では同一requestをそのまま再送しない。

| code | 条件 | `retry_advice` | client動作 |
| --- | --- | --- | --- |
| `invalid_input` | UUID、日付、epoch、負の経過秒、範囲外 | `manual_check` | 入力の修正、またはsessionの破棄を案内する。 |
| `task_not_found` | UUIDに対応するtaskがない | `manual_check` | task状態の確認、またはsessionの破棄を案内する。 |
| `task_already_completed` | 完了済みtaskを記録・完了しようとした | `manual_check` | task状態の確認、またはsessionの破棄を案内する。 |
| `actual_work_conflict` | 現在実績と期待実績が不一致 | `manual_check` | 記録は従来どおり手動確認。完了で現在実績があれば、保持した計測の再完了または計測再開をcardで案内する。現在実績がなければ手動確認。 |
| `arithmetic_overflow` | 実績、進捗、日時計算が表現範囲外 | `manual_check` | task値または時刻の修正、またはsessionの破棄を案内する。 |
| `task_not_completable` | 未完了の子など既存完了条件を満たさない | `manual_check` | 未完了の子を含むtask状態の修正、またはsessionの破棄を案内する。 |
| `configuration_error` | 設定file、`busy_time_slot`、storage pathなどの設定不正 | `manual_check` | 設定を修正してworkerまたはserviceを再起動するよう案内する。 |
| `repository_unavailable` | lock競合またはrepository load失敗など、mutation開始前の一時的失敗 | `retry` | sessionを保持し、同じ操作の再試行を案内する。 |
| `operation_failed` | 上記codeへ分類できないtask tree、schedule、その他の操作失敗 | `manual_check` | sessionを保持し、task状態の手動確認を案内する。 |
| `repository_save_failed` | storageが未commitと確認できる保存失敗 | `retry` | sessionを保持し、同じ操作の再試行を案内する。 |
| `repository_state_uncertain` | storageへのcommit有無を確定できない保存失敗、またはその発生後に同じserviceがmutationを拒否した場合 | `manual_check` | sessionを保持してmutationを無効化し、repositoryの手動確認とworkerまたはserviceの再起動を要求する。 |
| `worker_unavailable` | worker停止またはresponse channel切断 | `retry` | 既存表示を保持し、接続回復後の再試行を案内する。 |

すべてのerror responseはcodeに対応した利用者向け`message`を持つ。validation、競合、task状態errorは`manual_check`であり、利用者が原因を修正するかsessionを破棄するまで同一requestをそのまま再送しない。例外として、完了の実績競合で非負の現在実績が返った場合だけ、利用者の明示確認後に期待実績を最新値へ置換した新requestを送れる。`repository_state_uncertain`を1回返したserviceはpoisoned状態とし、read操作は許可しても、workerまたはserviceが再起動されるまで後続mutationをrepositoryへ到達させず同じcodeで拒否する。再起動後も、利用者がrepositoryを手動確認するまではclient側でmutationを再送しない。一時的なrepository利用不能、未commitと確定した保存失敗、worker停止だけをtyped errorとして`retry`とする。

未知の`WebError.code`を受信した場合もpayloadを保持し、serverが返した`retry_advice`に従って表示と再送可否を決める。外側の`ServerFnError`は`WebError`ではないため、この表のcodeへ変換せず、client固有のtransport表示errorとしてoperationとtask scopeを保持する。readのtransport失敗だけを再試行可能とし、mutationのtransport失敗はsafety markerを維持してrepositoryの手動確認を要求する。

server操作ごとに`operation_now`は1回だけ取得し、経過秒算出、完了時刻、snapshotに共通利用する。

## 10. Operation history

```text
OperationHistoryEntry {
    occurred_at_epoch_ms: i64,
    invocation: Bootstrap
              | ListTasks(ListTasksRequest)
              | AutoSession
              | RecordSession(RecordSessionRequest)
              | CompleteSession(CompleteSessionRequest),
    outcome: Success | Failure,
    summary: String,
}
```

- 「発火履歴」tabの選択時だけ独立したsectionとしてDOMへ描画し、ほかのtabでは履歴内容を描画しない。
- requestを送るserver操作はresponse受信時に成否を1件記録する。
- action名と実際に送信した全引数を`action_name(field: value, ...)`形式で表示する。引数のないactionも`bootstrap()`のように括弧を表示し、client内部の`request_id`は表示しない。
- 2種類の完了は実際のserver actionである`complete_session`として表示し、`record_elapsed_seconds: true`と`false`で、成功・失敗のどちらも区別して記録する。
- localStorage操作とrepository手動確認済み操作は履歴へ記録しない。
- summaryへ秘密情報、repository path、stack traceを出さない。
- 実行していない`見`、`働`、`終`、`外`などのCLI commandを履歴へ記録しない。

## 11. Compatibility

### 11.1 維持する契約

- CLI`働`以外のcommand文法、renderer出力、current task遷移
- CLIのtask未選択時no-opと成功時だけfocus解除する規則
- MCPのtool名、tool数、JSON schema、required field、default、response、error
- MCP `complete_task`の`task_id`、`finished_at`、`additional_actual_work_seconds`というwire入力
- Schronu-webのserver operationと`ServerSnapshot`を含むclient/server wire形式
- 既存の`work_sessions`とmutation safetyのlocalStorage schema。持ち歩きロックは独立keyとして追加する
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
- `record_session`と`complete_session`: 注入した開始0秒、click 60秒、server操作65秒で、記録実績とtask終了時刻がclick時点に固定されることを待機なしで検証する。終了時刻省略、開始前、epoch範囲外も検証する。browser時計がserver時計より進む場合はbrowser内の開始・終了差を実績へ反映し、完了時刻をserver操作時刻で上限化することも検証する。
- `complete_session`: 記録ありではclickまでの経過整数秒を加算し、記録なしでは開始時刻を使用せず追加実績0でclick時刻に完了すること。どちらも期待実績競合、反復task、未完了child、保存失敗の契約を維持し、成功responseが`ServerSnapshot`だけで次task情報を含まないこと。
- 進捗計算: 開始時33%、100%、133%、見積0、長時間、乗算overflow回避。
- buffer: 正、0、負、06:00前後、日次終端前の固定`busy_time_slot`控除、隣接logical dateの除外を検証する。日次終端10分前で予定作業なしなら`+00:10:00`、日次終端ちょうどで予定作業なしなら`00:00:00`、日次終端40分後で予定作業なしなら`-00:40:00`、日次終端40分後で予定残作業62分なら`-01:42:00`となることを検証する。
- buffer segment集計: 単一segment、同一taskの複数segment、複数task、進行中segment全量、同一logical date内の過去segment、`scheduled_work_seconds`合計overflowを検証する。
- buffer更新: 実績変更後のschedule再生成と、日次終端の前後を問わず開始時見積内のセッションが1件以上存在する間はbufferを停止し、セッション0件または全セッションが時間超過した間は実時間と同速で減算することを検証する。
- read model: 指定日、開始時刻順、複数segment、schedule rank 0判定(task tree上の子の有無に非依存)、締切、候補なしの自動選定。

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
- session追加はserver callを生成せず、破棄はlocalStorage削除成功後だけ一覧再取得を生成し、削除失敗時は生成しないことを検証する。
- 一覧の手動session追加は`is_leaf == false`でlocalStorage、memory state、発火履歴を変更しないことを検証する。
- bufferはsession 0件、snapshot以前からの復元session、snapshot後の途中開始、複数sessionの同時計測、単一・複数sessionの見積到達、開始時に見積到達済みのsession、sessionの個別破棄、全session破棄で、server bufferからsnapshot後の壁時計経過秒を1回減算し、各sessionの未送信進捗秒を個別に合算することを検証する。
- 破棄のlocalStorage保存失敗ではsessionとbuffer表示を維持し、server commit済みでlocal削除に失敗したsessionはbuffer計算上の計測中sessionから除外することを検証する。
- 同じlogical dateのread snapshot、実績反映済みmutation snapshot、06:00を跨ぐlogical date更新を新たなbuffer基準とし、page内開始と復元を区別せず各sessionの未送信進捗秒を新snapshotへ加算することを検証する。一覧再取得の繰り返しと正・0・負のbufferを含める。
- 記録と2種類の完了についてserver mutation成功、競合、保存失敗、worker停止、多重送信防止、global・manual safety block時のsession遷移を検証する。
- 3終了操作でclick時刻をrequestへ保持し、pending中のcard停止、見積到達時刻とclick時刻の早い方で打ち切る未送信進捗、単一・複数sessionのbuffer遷移、未commit確定error後の自動再開、transport切断・repository状態不確実時の確認完了までの打ち切りを注入epochだけで検証する。実時間のsleepやtimer待機は使用しない。
- 完了実績競合について、記録方針別の確認、初回click時刻でのcard・buffer停止、元requestを保った最新実績での再送、新request ID、再競合更新、成功cleanup、旧payloadのmanual block、計測再開の待ち時間除外とstorage失敗時の原子性を検証する。記録操作の競合は従来どおりmanual blockとなることを検証する。
- 4種類のセッション終了成功後に選択中または最新snapshotのlogical dateを再取得し、response全体で一覧を置換することを検証する。終了errorでは再取得せず、server commit成功後のlocalStorage削除失敗では安全状態を維持して再取得することも検証する。
- 完了成功response受理時点でin-flightだった`list_tasks` requestを無効化し、その後にresponseが到着しても完了taskが復活しないことを検証する。完了成功後の再取得responseと、それより後に開始した明示的な`list_tasks` responseは適用されることを検証する。logical date境界を跨ぐ完了responseではsnapshotと日付buttonを更新し、反復taskは再取得responseに従うことを検証する。
- 各endpointの成功型がsnapshotを持ち、error型がsnapshotを持たず、clientがerror時に直前snapshotを維持することを検証する。
- error codeごとの`retry_advice`がerror表と一致し、`manual_check`では同一requestをそのまま再送しないことを検証する。完了実績競合だけは明示確認後に期待実績を置換した新requestを送る。
- `History`へのtab切替がeffectを生成しないこと、履歴がserver通信結果だけを対象とすること、100件上限、成否、reload非永続化を検証する。
- 持ち歩きロックのkeyなし・正常値・不正JSON・未知version・読込失敗、元value維持、memory-first有効化、storage-first解除、一時許可非永続化を検証する。
- 単調時計による15秒境界と時計後退、閲覧操作と確認キャンセルでは期限を維持し、全変更操作のdispatchごとに成否を問わず期限を15秒後へ更新することを検証する。完了実績競合の再完了と計測再開も同じguardを通す。
- actionとresponseの製品orchestrator経路で、最後のsession削除成功時だけセッションtabから一覧tabへ移ることを検証する。複数session、server失敗、localStorage削除失敗、他tab表示中では遷移しないことも固定する。

### 12.5 UI and integration

- 固定された「セッション」「一覧」「発火履歴」の3tab、選択状態、callback、desktopで44px以上・46rem以下で40px以上の操作高、safe area、本文との非重複、通信中overlayとの重なり順をcomponent test、CSS contract test、browser目視で確認する。
- 各tabで選択中の画面だけがDOMへ存在し、タイトルは存在せず、持ち歩きロックbarとbufferはセッションtabだけに存在することを確認する。barを隠した一覧・発火履歴でも持ち歩きロックのmutation guardが有効であることを確認する。
- rank 0の一覧rowだけにセッションbuttonとclick listenerがあり、rank非0にはどちらもないことを確認する。
- 日付parserは同日、未来、過去、年境界、完全日付、前後空白、不正形式、不正calendar日付、範囲overflowをcontract testで確認する。component testでは日付入力と検索のDOM順、入力・submit callback、正規化値の保持、曜日buttonでのclear、inline errorとARIA関連付けを確認する。
- 一覧検索は日本語の部分一致、ASCII大小無視、前後空白、空白だけ、不一致、同一taskの複数segmentをcomponent testで確認する。検索欄が日付buttonとtableの間にあること、入力callback、入力中だけのclear button、clear callback、空結果のstatus、非表示rowの操作listener不在を確認する。keyboardでclearした後に検索欄へfocusが戻ることをbrowserで確認する。
- 検索文字列が日付・tab切替で保持され、reloadで破棄されることと、検索入力・clearでserver通信、localStorage更新、発火履歴追加がないことをbrowserで確認する。
- 一覧は320px、360px、46rem、1024pxで確認する。全幅で操作、予定、締切、taskの順を確認し、46rem以下では可視header、32px以上の1行row、左端の幅44px・高さ32pxの「＋」・disabledの「✓」・rank非0の空cell、固定された日付付き予定と締切、task名cellだけの横scrollを確認する。長いtask名と複数segmentでもtask名cellの縦scrollbarとviewport全体の横scrollが発生しないことを確認する。
- 320px以上で高さ36pxの日付入力・表示button、検索欄、36px四方のclear buttonがviewportを超えないことをCSS contract testとbrowser目視で確認する。
- 46rem以下で日付button、検索欄、clear button、各section間隔が圧縮され、日付buttonの横スクロールが維持されることを確認する。34rem以下ではbufferも圧縮されることを確認する。
- touch/mobile emulationでは全buttonのタップ後にhover配色が残らず、`:active`と`:focus-visible`が機能することを確認する。desktopのhover可能なfine pointerでは既存hover表現と、選択済み日付buttonの緑背景・白文字が維持されることを確認する。
- 4操作buttonのlabel、ARIA名、意味別class、通常幅の2列配置、狭幅の1列配置を確認する。
- 「計測を破棄して完了」の最初のclickでは通信せず、card単位の確認表示、キャンセル、確定時の1回だけのtyped callbackを確認する。
- 完了実績競合では通常の4操作をaccessibility付き確認groupへ置換し、記録方針ごとの正確な文言、`HH:MM:SS`、計測再開と再完了のtyped callbackを確認する。
- 確認表示ではtimerが進み、3終了操作のdispatch後は注入したclick時刻でcardが停止することを確認する。
- 33%、100%、133%、見積0、buffer正負の表示を確認する。開始、完了予定、残り・超過が同じtiming領域にあり、semanticな`time`要素と識別可能なARIA labelを維持することをcomponent testで確認する。
- 320px、360px、46rem、1024pxでsession cardのtiming領域が折り返さず、task名、timing、progress、操作の順序とdesktop layoutを維持することをCSS contract testとbrowser目視で確認する。
- 通信matrixの各操作についてrequest件数を確認する。
- 全5server通信のdispatchで全画面待機表示と背面の`inert`が即時に有効になり、最後のresponseまで維持されることを確認する。成功、operation error、transport errorの各応答で解除され、`ClientEffect::None`では表示されないことを確認する。SSR初期表示のstatusとARIA属性、viewport全面のCSS、reduced motionを確認する。
- 持ち歩きロックbarのsticky表示、3状態、残り秒表示、`aria-live`対象、通常モードへの確認付き復帰を確認する。`Locked`では状態文言が44px以上の長押しbutton内にあり、独立した状態blockがなく、解除`details`だけが次の行にあることと、34rem以下でも汎用縦積み規則を適用しないことをcomponent testとCSS contract testで固定する。
- pointer・Space・Enterの1.2秒長押し成立と、pointerup・leave・cancel・blur・window scroll・短いkeyupでの中断を確認する。
- ロック中も画面表示・更新、scroll、tab切替、日付選択、一覧取得が機能し、全変更操作が無効になることを確認する。破棄完了の確認は一時許可を消費せず確定時に消費し、完了実績競合の再完了・計測再開は新たな許可を消費することを確認する。
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
| 6.4 | REQ-BUFFER-001..010 |
| 6.1、6.5、7.3、7.4、8、12.5 | REQ-LIST-001..014 |
| 6.1、7.1、8、10、12.4、12.5 | REQ-COMMON-001..007、REQ-NET-001..006 |
| 3.4、6.6、7.5、8、12.4、12.5 | REQ-LOCK-001..010 |
| 11、12 | REQ-COMPAT-001..005、全受入条件 |
