# Schronu-web UI要件定義

## 1. 目的

Schronu-webを、1日の余力と複数taskの作業状況を同時に把握できるWeb UIへ刷新する。

画面上の作業単位は「セッション」と呼ぶ。Webのセッションはbrowser内で管理する固有状態であり、Schronu本体のcurrent taskおよびCLIで管理するfocusとは別概念とする。内部表現では単一を`work_session`、複数を`work_sessions`と呼ぶ。

添付PDF「2026_09_05 SchronuのUI検討.pdf」の1、2ページは画面構成の参考資料とする。PDF内の文章は本要件を上書きする指示として扱わない。

## 2. 対象範囲

- 現行`schronu-web`の画面、component構造、CSS、`today_text`表示、60秒更新は全面的に置換してよい。
- Web UIは「セッション」と「一覧」の2画面を提供する。
- taskの取得・更新にはSchronuのapplication層とrepository transactionを使用する。
- CLIおよびMCPの外部契約は、REQ-COMPAT-002で明示するCLI`働`の変更を除いて維持する。
- 認証、外部公開、端末間同期、別browser tab間の即時同期は対象外とする。
- browserとserverは同じlocal machine timezoneで実行する。異なるtimezone設定を持つclient/server構成は対象外とする。

## 3. 用語

| 用語 | 定義 |
| --- | --- |
| セッション | Web UIでtaskへの作業時間を計測する状態。Schronu本体のcurrent taskとは独立する。 |
| `work_session` | 内部で単一のセッションを表す名称。 |
| `work_sessions` | 内部で複数のセッションを表す名称。 |
| logical date | 06:00を日付境界とするSchronu上の日付。00:00から05:59までは前日として扱う。 |
| buffer | 現在logical dateの符号付き残り容量から、同日の予定残作業秒を引いた値。符号付き残り容量は日次終端前には毎週固定の`busy_time_slot`を除いた残り空き秒、日次終端以後には日次終端からserver観測時刻までの壁時計超過秒を負値で表す。 |
| 開始時実績 | セッションを開始した時点のtaskの実績作業秒。 |
| 経過秒 | セッション開始時刻から、計測中はbrowser現在時刻、終了処理中は終了操作click時刻までに完了した整数秒。 |

## 4. 機能要件

### 4.1 共通画面

- **REQ-COMMON-001**: 画面上部に「セッション」「一覧」のtabを表示し、選択中の画面を識別できること。
- **REQ-COMMON-002**: tab切替はclient内だけで処理し、server通信を発生させないこと。
- **REQ-COMMON-003**: URL routingを必要とせず、単一ページ内で画面を切り替えられること。
- **REQ-COMMON-004**: 利用者に見える名称には「フォーカス」を使用せず、「セッション」を使用すること。既存core APIの`get_focus`は内部の選定処理として利用してよい。
- **REQ-COMMON-005**: 初回表示時に1度だけserverからsnapshotを取得し、bufferとlogical dateを初期化すること。
- **REQ-COMMON-006**: server操作に失敗した場合、直前の表示データと`work_sessions`を保持したまま、errorの再試行可否を識別し、再試行または手動確認を案内すること。repository状態が不確実な場合は再送を案内しないこと。
- **REQ-COMMON-007**: 34rem以下ではbuffer領域と日付buttonの余白を圧縮し、日付buttonの44px以上の操作高と8日分の横スクロールを維持すること。
- **REQ-COMMON-008**: 全buttonのhover配色はhover可能なfine pointerでだけ適用し、タッチ操作後に残留させないこと。desktopで選択済み日付buttonへhoverした場合は、緑背景と白文字を維持すること。`:active`と`:focus-visible`の操作feedbackはpointer種別によらず維持すること。

### 4.2 セッション状態

- **REQ-SESSION-001**: 複数のセッションを同時に保持し、それぞれの経過時間を独立して計測できること。
- **REQ-SESSION-002**: `work_sessions`をlocalStorageの`schronu_web.work_sessions.v1`へ保存すること。
- **REQ-SESSION-003**: 各`work_session`はtask UUID、task名、開始epoch milliseconds、開始時見積秒、開始時実績秒を保持すること。
- **REQ-SESSION-004**: reload後は保存した開始時刻と現在時刻との差から各セッションを復元し、reload中の経過時間も反映すること。初期loadでlocalStorageから復元した計測中セッションについては、server snapshotのbuffer秒から、server観測時刻以前に存在する復元セッション計測区間の和集合を差し引くこと。終了操作中の区間は終了click時刻で閉じること。
- **REQ-SESSION-005**: 同じtask UUIDのセッションは1件だけ保持し、重複追加しないこと。
- **REQ-SESSION-006**: localStorageのtop-level JSONが不正またはversionが非対応の場合は空の`work_sessions`で表示し、元のkeyを自動上書きせずwarningを表示すること。個別entryだけが不正な場合はそのentryだけを除外し、次のlocal state変更時にvalid entryだけをversion 1として保存すること。いずれの場合もtaskを更新せず、初回`bootstrap`を継続すること。
- **REQ-SESSION-007**: Webセッションの追加・削除・復元によってSchronu本体のcurrent taskを変更しないこと。

### 4.3 セッションがない場合

- **REQ-AUTO-001**: セッションが0件の場合だけ「自動セッション」buttonを表示すること。
- **REQ-AUTO-002**: 「自動セッション」を押すと、Schronuの`get_focus`相当の規則で選定したtaskをセッションへ追加すること。
- **REQ-AUTO-003**: 自動選定によってSchronu本体のcurrent taskを変更しないこと。
- **REQ-AUTO-004**: 選定対象がない場合、セッションを追加せず、その結果を利用者へ表示すること。

### 4.4 セッションcard

- **REQ-CARD-001**: 各cardにtask名を表示すること。
- **REQ-CARD-002**: セッション開始時刻をlocal timeの`HH:MM`で表示すること。
- **REQ-CARD-003**: 完了予定時刻をlocal timeの`HH:MM`で表示すること。
- **REQ-CARD-004**: 完了予定時刻は`開始時刻 + max(見積秒 - 開始時実績秒, 0)`で算出すること。
- **REQ-CARD-005**: 進捗率は`(開始時実績秒 + 経過秒) * 100 / 見積秒`で算出し、整数%で表示すること。
- **REQ-CARD-006**: 見積秒が0の場合、進捗率を`--%`と表示すること。
- **REQ-CARD-007**: 進捗率は100%を超過できること。
- **REQ-CARD-008**: progress barは100%までを通常色、100%を超えた部分をbarの右側へ伸長する赤色領域として表示すること。
- **REQ-CARD-009**: 完了までの残り時間を`MM:SS`で表示し、1秒ごとに更新すること。分は59を超えてよい。
- **REQ-CARD-010**: 見積時間を超過した場合、超過時間を赤い文字の`MM:SS`で増加表示すること。
- **REQ-CARD-011**: 見積秒が0の場合、セッション開始直後から経過秒を超過時間として表示すること。
- **REQ-CARD-012**: browser時計がセッション開始時刻より前になった場合、表示用経過秒を0として扱い、負の実績を生成しないこと。

### 4.5 セッション操作

- **REQ-ACTION-001**: 各cardに「破棄して解除」「記録して解除」「計測を破棄して完了」「記録して完了」の4buttonを表示すること。
- **REQ-ACTION-002**: 「破棄して解除」は対象セッションをlocalStorageから削除するだけとし、taskの実績を加算せず、server通信を行わないこと。削除成功後は残存する計測中セッションからbufferを再計算し、削除したセッションだけが覆っていた時間を未作業時間として減算すること。
- **REQ-ACTION-003**: 「記録して解除」は対象taskのUUIDと終了操作click時刻を指定し、その時刻までの経過秒を開始時実績へ加算すること。server処理中の通信待ち時間を加算しないこと。
- **REQ-ACTION-004**: 「記録して解除」は開始時実績を期待値として検証し、現在実績と不一致の場合はtaskを保存せず、セッションを保持すること。
- **REQ-ACTION-005**: 「記録して完了」は対象taskのUUIDと終了操作click時刻を指定し、その時刻までの経過秒加算、task完了、終了時刻更新を同じrepository transactionで処理すること。
- **REQ-ACTION-006**: 「計測を破棄して完了」は開始時刻を実績計算に使わず、追加実績秒を0とし、確定click時刻を完了時刻としてtaskを完了すること。既存の実績秒は変更しないこと。
- **REQ-ACTION-007**: 2種類の完了は開始時実績を期待値として検証し、不一致の場合は実績加算、完了、終了時刻更新、反復task生成を一切保存せず、セッションを保持すること。
- **REQ-ACTION-008**: server処理中は同じセッションの操作buttonを無効化し、二重送信を防ぐこと。終了操作ではclick時刻でcardの進捗、残り・超過時間を停止し、serverが未commitと確定できるerror時は現在時刻基準の計測へ自動復帰すること。transport切断またはrepository状態が不確実な場合は、repository確認完了までclick時刻で停止し続けること。
- **REQ-ACTION-009**: 未知task、完了済みtask、不正な経過時間、overflow、repository errorではセッションを保持し、errorを表示すること。
- **REQ-ACTION-010**: 「記録して解除」「計測を破棄して完了」「記録して完了」はserver処理成功後だけ対象セッションを削除すること。
- **REQ-ACTION-011**: 「計測を破棄して完了」の最初のclickではserver requestを送らず、当該card内に「このセッションの計測時間は記録されません。タスクを完了しますか?」と「キャンセル」「計測を破棄して完了」を表示すること。キャンセルは元の4操作へ戻し、確定時だけrequestを1回送ること。
- **REQ-ACTION-012**: 操作buttonは`nth-child`ではなく意味別classで記録・完了・破棄の役割を表し、通常幅では解除系と完了系をそれぞれまとまりとして配置し、狭い画面では1列にすること。

### 4.6 buffer

- **REQ-BUFFER-001**: bufferを`現在logical dateの符号付き残り容量 - 同日のschedule segmentごとのscheduled_work_seconds合計`としてserver側で算出すること。server観測時刻が日次終端より前なら、符号付き残り容量は観測時刻から日次終端までの毎週固定`busy_time_slot`控除後の空き秒とする。日次終端以後なら、符号付き残り容量は`日次終端 - server観測時刻`の0以下の秒数とし、日次終端後の全壁時計超過時間を反映する。同一taskの複数segment、進行中segment、同じlogical date内の過去segmentをそれぞれ1回ずつ全量で集計すること。
- **REQ-BUFFER-002**: serverからbuffer秒とその観測時刻を取得し、以後はbrowser側で計測中セッションが1件も存在しない経過秒だけを差し引いて1秒ごとに表示を更新すること。終了操作処理中の対象はclick時刻以後を計測中とみなさないこと。加えて、初期loadでlocalStorageから復元し、現在も保持するセッションのserver観測時刻までの計測区間はserver bufferへ未反映として、複数セッションの重複を除いた継続秒を1回だけ差し引くこと。この規則はserver観測時刻が日次終端以後の場合も同じとする。
- **REQ-BUFFER-003**: 0以上のbufferを`HH:MM:SS`でカウントダウン表示すること。
- **REQ-BUFFER-004**: 負のbufferを赤い文字の`-HH:MM:SS`でカウントアップ表示すること。
- **REQ-BUFFER-005**: logical dateが06:00境界で変化しても、それだけを理由にserverから再取得しないこと。
- **REQ-BUFFER-006**: 次の明示的server操作のresponseでlogical dateとbuffer snapshotを更新すること。
- **REQ-BUFFER-007**: snapshot後に計測中セッションが1件以上存在する時間はbufferを停止すること。snapshot後に最初のセッションを開始した場合は、その開始前のセッション不在時間だけを減算すること。最後の計測中セッションが終了操作に入った場合は、click時刻からbuffer更新を再開すること。snapshot以前から継続する復元セッションは、REQ-BUFFER-002の復元補正を適用したうえでsnapshot後のbufferを停止すること。
- **REQ-BUFFER-008**: 複数の計測中セッションが重なる時間は和集合として扱い、重複時間を二重に補正しないこと。復元セッションの観測時刻以前の計測区間も和集合として1回だけ差し引くこと。終了処理中のセッションはclick時刻を計測区間の終端とし、server commit済みでlocalStorage削除失敗により残ったセッションはsnapshot後の停止と復元補正の双方から除外すること。serverが未commitと確定できるerror時は対象を計測中へ戻し、transport切断またはrepository状態が不確実な場合はrepository確認完了までclick時刻で終了した区間として扱うこと。
- **REQ-BUFFER-009**: 「破棄して解除」のlocalStorage削除成功後は残存セッションからbufferを再計算し、全セッションを破棄した場合はsnapshot後の全経過秒を減算すること。保存失敗時はmemory上のセッションを維持し、buffer表示を変化させないこと。
- **REQ-BUFFER-010**: 新しいserver responseを受信した場合は、そのbuffer秒と観測時刻を新たな表示計算の基準とし、観測時点で計測中のセッションがあれば観測直後からbufferを停止すること。初期loadで復元したセッションが残っている間は、新しい基準にもREQ-BUFFER-002の復元補正を適用すること。

### 4.7 一覧画面

- **REQ-LIST-001**: serverから得た現在logical dateを起点として、連続する8 logical datesのbuttonを表示すること。
- **REQ-LIST-002**: 先頭のbuttonを`曜 今日`、2番目を`曜 明日`、3番目以降を曜日で表示すること。
- **REQ-LIST-003**: 日付取得では曜日名ではなく具体的なlogical dateをserverへ送ること。
- **REQ-LIST-004**: 選択したlogical dateのschedule segmentを開始時刻の昇順で表示すること。
- **REQ-LIST-005**: 各行に締切、予定時間、task名、「セッション」buttonを表示すること。
- **REQ-LIST-006**: 予定時間をlocal timeの`HH:MM-HH:MM`で表示すること。
- **REQ-LIST-007**: 現在時刻が締切を過ぎた場合、締切を赤色で表示すること。
- **REQ-LIST-008**: schedule rankが0であるtask(未完了の子を持たないtask)のtask名を緑色で表示すること。
- **REQ-LIST-009**: 一覧の「セッション」buttonは対象taskをlocalの`work_sessions`へ追加するだけとし、server通信および画面遷移を行わないこと。
- **REQ-LIST-010**: 対象task UUIDのセッションが存在する場合、同じtaskを表すすべてのschedule segmentの「セッション」buttonを無効化すること。
- **REQ-LIST-011**: schedule rankが0でないtaskは「セッション」buttonを表示せず、client stateが手動追加要求を受けても`work_sessions`へ追加しないこと。
- **REQ-LIST-012**: 「計測を破棄して完了」または「記録して完了」のserver処理成功後は、追加の`list_tasks`を送らず、表示中の一覧から対象task UUIDを持つ全schedule segmentを即時に除去すること。別taskのrowと選択logical dateを維持し、responseがlogical date境界を跨いだ場合もsnapshotと日付buttonは更新すること。完了成功response受理時点でin-flightの`list_tasks` requestを無効化し、その後に到着したresponseは適用しないこと。完了成功response受理後に開始した`list_tasks` responseは通常どおり適用すること。完了失敗、「記録して解除」、「破棄して解除」では一覧を変更しないこと。server commit成功後に対象sessionのlocalStorage削除だけが失敗した場合も、一覧からは除去すること。反復完了で生成された次回taskは自動追加せず、次の明示的な一覧取得で表示すること。
- **REQ-LIST-013**: 46rem以下では一覧の横スクロールをなくし、各rowをtask名、label付きの締切・予定、横幅100%の「セッション」buttonの順にcard表示すること。締切と予定は2列とし、tableの列header semanticsを維持すること。

### 4.8 通信制限と発火履歴

- **REQ-NET-001**: server通信を初回`bootstrap`、日付選択、`自動セッション`、`記録して解除`、`計測を破棄して完了`の確定、`記録して完了`に限定すること。
- **REQ-NET-002**: tab切替、毎秒tick、一覧の「セッション」、`破棄して解除`、`計測を破棄して完了`の確認表示とキャンセルではserver通信を行わないこと。
- **REQ-NET-003**: 画面上に開閉式の発火履歴を表示できること。
- **REQ-NET-004**: 発火履歴はserver通信結果の直近100件をmemory内だけに保持し、reload時に消去すること。localStorage操作は記録しないこと。
- **REQ-NET-005**: 各履歴に操作時刻、実際に呼び出したserver action名と全送信引数、成功・失敗を表示すること。引数は関数呼出し形式で表示し、client内部の`request_id`は含めないこと。
- **REQ-NET-006**: 実行していないCLI command名を履歴へ記録せず、実際にresponseを受信したserver操作を記録すること。
- **REQ-NET-007**: 「記録して完了」と「計測を破棄して完了」は、実際の`complete_session`呼出しと`record_elapsed_seconds`の真偽を履歴へ記録し、失敗時もどちらを試みたか識別できること。
- **REQ-NET-008**: 初回`bootstrap`を含む全server通信で、request開始からresponseの成否を適用するまで画面全体に待機表示を出し、pointerとkeyboardによる背面操作を無効にすること。複数requestの並行時は全responseの受理まで維持し、成功、operation error、transport errorのいずれでも対応するresponseの受理時に当該request分を解除すること。待機表示は「通信中…」をstatusとして通知し、動きを減らすOS設定では回転animationを停止すること。

### 4.9 持ち歩きロック

- **REQ-LOCK-001**: 画面上部へ常時表示されるstickyな持ち歩きロックbarを設け、通常モードでは「持ち歩きロック」の1 clickで即時にロックできること。持ち歩きロックだけを理由に画面を覆うoverlayや画面内容を非表示にする方式は使用しないこと。server通信中の待機表示は`REQ-NET-008`を優先すること。
- **REQ-LOCK-002**: ロック中もbufferとセッションの表示・更新、scroll、tab切替、日付選択、一覧取得を利用可能とすること。
- **REQ-LOCK-003**: ロック中は「自動セッション」、一覧からの「セッション」追加、「破棄して解除」、「記録して解除」、「計測を破棄して完了」、「記録して完了」、「repository確認済み」の7変更操作をclientの共通guardで遮断すること。
- **REQ-LOCK-004**: ロックbarをpointerまたはSpace・Enterで1.2秒長押しすると、15秒間かつ1操作だけ変更操作を許可すること。pointerup、pointerleave、pointercancel、buttonのblur、window scroll、または1.2秒未満の入力終了では長押しを成立させないこと。
- **REQ-LOCK-005**: 一時許可は、変更操作の成功可否や実際の変更有無にかかわらず最初のdispatch時に消費し、即座に再ロックすること。15秒経過時にも再ロックし、期限判定は壁時計と分離した単調時計を用いること。
- **REQ-LOCK-006**: 「計測を破棄して完了」は確認表示では一時許可を消費せず、確定dispatchで消費すること。確認のキャンセルまたは一時許可の期限切れでは確認表示を閉じて再ロックすること。
- **REQ-LOCK-007**: ロックbarは通常、ロック中、一時許可中を識別可能に表示し、一時許可中は残り秒数を表示すること。`aria-live`では状態遷移だけを通知し、毎秒変化する残り秒数を通知対象にしないこと。
- **REQ-LOCK-008**: 通常モードへ戻す操作はロックbarの`details`内へ分離し、確認操作の成功後だけ永続的に解除すること。
- **REQ-LOCK-009**: 持ち歩きロックはrepository不確実性を扱うmutation safetyとは独立したclient stateとして、localStorageの`schronu_web.carry_lock.v1`へ`version: 1`と`enabled`を保存すること。keyなしは通常モード、正常値は保存状態を復元し、ロック状態のreloadでは一時許可を復元せずロック状態とすること。
- **REQ-LOCK-010**: 持ち歩きロックのJSON不正、未知version、読込失敗は元のvalueを上書きせず、warning付きのロック状態とすること。ロック開始はmemory-firstで反映し、保存失敗時も現在のpageではロックを維持してreload後の危険を警告すること。通常モードへの復帰はstorage-firstとし、保存失敗時はロックを維持すること。

### 4.10 application操作と互換性

- **REQ-APP-001**: application層の実績加算は、UUID、追加実績秒、任意の期待実績秒を入力とする1つの操作へ集約し、CLIとWebで共用すること。
- **REQ-APP-002**: 実績加算は未完了taskと非負の追加秒だけを許可し、期待実績が指定された場合は現在値との一致を更新前に検証すること。完了済みtaskは実績を変更せず、明示的なerrorにすること。
- **REQ-APP-003**: 実績加算時のoverflowおよび期待実績競合では、taskを変更しないこと。
- **REQ-APP-004**: `complete_task`は任意の期待実績秒を受け取り、実績加算と完了処理を原子的に実行できること。
- **REQ-COMPAT-001**: CLIのcommand名、alias、引数個数、正常時出力、task未選択時のno-op、成功時だけfocusを解除する挙動、保存・lock境界は維持すること。
- **REQ-COMPAT-002**: CLIの`働`は次の秒単位契約へ変更すること。
  - 引数なしは、focus開始からcommand実行時までの完了済み整数秒を既存実績へ加算する。
  - `働 <minutes>`は非負整数の`minutes * 60`秒を既存実績へ加算する。
  - 既存実績の秒端数を保持する。
  - 負数、時間逆行、乗算または加算overflowをerrorにし、実績とfocusを変更しない。
- **REQ-COMPAT-003**: MCPはtool一覧、JSON schema、入力既定値、response、error、保存結果を変更しないこと。
- **REQ-COMPAT-004**: MCPおよびCLIから`complete_task`を呼ぶ場合は期待実績を指定せず、従来の完了契約を維持すること。
- **REQ-COMPAT-005**: task storage schemaおよび既存repository transactionの安全性契約を変更しないこと。

## 5. 非機能要件

- **REQ-NFR-001**: 時刻、秒数、UUID、logical dateを型付きデータとしてclient/server間で受け渡し、CLI出力文字列をparseしないこと。
- **REQ-NFR-002**: server側のtask操作は専用workerで直列化し、repositoryへの同時操作を避けること。
- **REQ-NFR-003**: client/server間ではUUID、開始・終了epoch milliseconds、秒数、`YYYY-MM-DD`をwire形式として使用すること。
- **REQ-NFR-004**: errorは原因を識別可能な型付きerrorとし、競合、再試行可能なrepository error、repository状態が不確実で再送できないerrorを区別できること。
- **REQ-NFR-005**: serverの成功responseはserver観測時刻、現在logical date、buffer秒を含み、clientが同じ基準時刻から表示を更新できること。error responseはsnapshotを含めず、code、message、再試行方針を含むこと。
- **REQ-NFR-006**: UIの自動更新はbrowser内の計算に限定し、意図しないtask dataの読み書きを発生させないこと。
- **REQ-NFR-007**: epoch millisecondsの表示はbrowserのlocal timezoneを使用し、logical dateの判定はserverが返す値を使用すること。browserとserverは同じlocal machine timezoneで動作することを実行前提とする。

## 6. 受入条件

| ID | 受入条件 |
| --- | --- |
| AC-001 | 画面上に「セッション」「一覧」が表示され、利用者向け文言に「フォーカス」が残っていない。 |
| AC-002 | 2件以上のセッションが同時に1秒ごとに進み、reload後も元の開始時刻から復元される。server buffer表示はserver観測時刻以前に存在する復元セッション計測区間の和集合を1回だけ差し引き、終了操作中の区間は終了click時刻で閉じる。 |
| AC-003 | 15分見積、開始時実績5分のtaskはセッション開始直後に33%となり、100%および133%で指定どおりのbarを表示する。 |
| AC-004 | 見積0のtaskは`--%`と赤い超過時間を表示し、長時間の分表示は59を超えても欠落しない。 |
| AC-005 | 日次終端前は毎週固定`busy_time_slot`控除後の空き秒、日次終端ちょうどは予定作業がなければ0、日次終端後は壁時計超過秒を負値とするbufferがserver観測時刻を基準に変化する。計測中セッションが0件なら毎秒減り、1件以上なら停止する。localStorageから復元したセッションの観測時刻以前の継続秒は重複を除いてserver bufferから差し引き、最後のセッションの終了操作後はclick時刻から再開し、負値は赤い符号付き表示になる。 |
| AC-006 | 06:00境界、tab切替、毎秒tick、一覧からのセッション追加、破棄して解除、破棄完了の確認とキャンセルではserver requestが増えない。 |
| AC-007 | 初回、日付選択、自動セッション、記録、2種類の完了確定だけが仕様どおりのserver requestを発生させる。 |
| AC-008 | 一覧に8 logical datesが表示され、両端が同じ曜日でも具体日付で別の日として取得される。 |
| AC-009 | 一覧は開始時刻順で、締切超過は赤、schedule rank 0のtask名は緑になる。rank非0ではセッションbuttonを表示せず、セッション中のrank 0 taskでは全segmentのbuttonが無効になる。 |
| AC-010 | 破棄して解除ではtaskが変わらず、記録では終了操作clickまでの完了済み整数秒だけが加算される。記録して完了では同じclick時刻をtask終了時刻として加算と完了が1 transactionで保存され、計測を破棄して完了では既存実績を変えずtaskだけが完了する。 |
| AC-011 | 別processで実績が変化した後の記録・2種類の完了は競合となり、taskと反復taskを保存せず、Webセッションを保持する。 |
| AC-012 | CLI`働`は秒端数を保持し、引数なしは整数秒、明示指定は分から秒へ換算して加算し、失敗時はfocusを保持する。 |
| AC-013 | MCPのtool schemaと既存contract testの期待値が変更されず、CLI`働`以外のCLI contract testも変更なしで成功する。 |
| AC-014 | 発火履歴が実際のserver action名、全送信引数、成否を区別して100件まで表示し、localStorage操作を表示せず、reload後は空になる。 |
| AC-015 | 各cardに4操作が表示され、計測を破棄して完了はcard内の確認を経た確定時だけ1回送信され、キャンセルでは送信されない。3終了操作はclick時刻でcardの計測を停止し、通信待ちで表示や実績を増やさない。2種類の完了は`record_elapsed_seconds`の真偽を含む発火履歴で区別される。 |
| AC-016 | 2種類の完了が成功すると追加通信なしで対象task UUIDの全schedule segmentが一覧から消え、別taskのrowと選択logical dateは維持される。logical date境界を跨ぐ完了responseではsnapshotと日付buttonが更新される。完了成功response受理時点でin-flightだった一覧requestは無効化され、その後にresponseが到着しても対象taskが復活しない。完了成功response受理後に開始した明示的一覧取得は通常どおり反映される。完了失敗、記録して解除、破棄して解除では一覧が変化せず、server commit成功後にlocalStorage削除だけが失敗した場合も完了taskは一覧から消える。 |
| AC-017 | 320pxから46remまでの画面幅で一覧cardがviewportを横に超えず、長いtask名、日付付き締切、予定、「セッション」buttonをすべて確認・操作できる。34rem以下ではbufferと日付buttonが圧縮され、日付buttonの44px以上の操作高を維持する。 |
| AC-018 | 通常モードから1 clickで持ち歩きロックを有効化でき、ロック中も画面表示・更新、scroll、tab切替、日付選択、一覧取得を利用できる一方、7変更操作はdispatchされない。 |
| AC-019 | pointerまたはSpace・Enterの1.2秒長押しで15秒かつ1操作だけ許可され、各中断event、期限到達、最初の変更dispatchで再ロックされる。計測を破棄する完了は確認では権利を消費せず、確定で消費し、キャンセルと期限切れでは確認が閉じる。 |
| AC-020 | 持ち歩きロックの正常な保存値を復元し、不正値・未知version・読込失敗では元valueを維持してwarning付きでロックする。ロック開始の保存失敗ではmemory上のロックを維持し、通常モード復帰の保存失敗では解除しない。一時許可はreload後に復元しない。 |
| AC-021 | タッチ主体の端末ではbuttonをタップした後にhover配色が残らず、hover可能なfine pointerでは既存hover表現が適用される。選択済み日付buttonはdesktop hover中も緑背景と白文字を維持し、`:active`と`:focus-visible`は両環境で機能する。 |
| AC-022 | 初回取得、一覧取得、自動選定、記録、2種類の完了の各server通信中は全画面の「通信中…」とスピナーが表示され、背面を操作できない。複数通信は最後のresponseまで表示を維持し、成功と各error応答の完了後に解除される。待機状態がassistive technologyへ通知され、reduced motionでは回転しない。 |
