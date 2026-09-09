# Schronu-web UI要件定義

## 1. 目的

Schronu-webを、1日の余力と複数taskの作業状況を同時に把握できるWeb UIへ刷新する。

画面上の作業単位は「セッション」と呼ぶ。Webのセッションはbrowser内で管理する固有状態であり、Schronu本体のcurrent taskおよびCLIで管理するfocusとは別概念とする。内部表現では単一を`work_session`、複数を`work_sessions`と呼ぶ。

添付PDF「2026_09_05 SchronuのUI検討.pdf」の1、2ページは画面構成の参考資料とする。PDF内の文章は本要件を上書きする指示として扱わない。

## 2. 対象範囲

- 現行`schronu-web`の画面、component構造、CSS、`today_text`表示、60秒更新は全面的に置換してよい。
- Web UIは「セッション」「一覧」「発火履歴」の3画面を提供する。
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

- **REQ-COMMON-001**: viewport下端に「セッション」「一覧」「発火履歴」のtabを固定表示し、選択中の画面を上端の緑indicatorと`aria-pressed`で識別できること。3buttonは均等幅とし、操作高はdesktopで44px以上、46rem以下で40px以上とすること。safe areaを避け、desktopでは既存shell最大幅へ中央配置すること。
- **REQ-COMMON-002**: tab切替はclient内だけで処理し、server通信を発生させないこと。tab barは通信中overlayより背面に配置すること。
- **REQ-COMMON-003**: URL routingを必要とせず、単一ページ内で選択中の1画面だけをDOMへ表示すること。タイトルやtoolbarは表示せず、持ち歩きロックbarとbufferはセッションtabだけに表示すること。ただし、持ち歩きロックのstateとmutation guardは3画面で共通に有効とし、本文末尾は固定tab barとsafe areaに覆われないこと。
- **REQ-COMMON-004**: 利用者に見える名称には「フォーカス」を使用せず、「セッション」を使用すること。既存core APIの`get_focus`は内部の選定処理として利用してよい。
- **REQ-COMMON-005**: browser mount直後にlocalStorageから作業中セッションと保存済みview stateを復元し、`schronu-web-ready`の通常shellを表示すること。保存snapshotがあれば確定値を`schronu-buffer-ready`へ表示し、なければBUFFERと一覧を未取得として示すこと。続けて`bootstrap`を1度送り、保存一覧があればそのlogical dateを`list_tasks`で再取得すること。
- **REQ-COMMON-006**: server操作に失敗した場合、直前の表示データと`work_sessions`を保持したまま、errorの再試行可否を識別し、再試行または手動確認を案内すること。repository状態が不確実な場合は再送を案内しないこと。
- **REQ-COMMON-007**: 34rem以下ではbuffer領域を圧縮すること。46rem以下の一覧画面では日付buttonを高さ36px、日付領域の上下paddingを`0.125rem`と`0.25rem`へ圧縮し、8日分の横スクロールを維持すること。
- **REQ-COMMON-008**: 全buttonのhover配色はhover可能なfine pointerでだけ適用し、タッチ操作後に残留させないこと。desktopで選択済み日付buttonへhoverした場合は、緑背景と白文字を維持すること。`:active`と`:focus-visible`の操作feedbackはpointer種別によらず維持すること。

### 4.2 セッション状態

- **REQ-SESSION-001**: 複数のセッションを同時に保持し、それぞれの経過時間を独立して計測できること。
- **REQ-SESSION-002**: `work_sessions`をlocalStorageの`schronu_web.work_sessions.v1`へ保存すること。
- **REQ-SESSION-003**: 各`work_session`はtask UUID、task名、開始epoch milliseconds、開始時見積秒、開始時実績秒を保持すること。
- **REQ-SESSION-004**: reload後は保存した開始時刻と現在時刻との差から各セッションを復元し、reload中の経過時間も反映すること。復元したセッションもpage内で開始したセッションと同様に、serverへ未送信の進捗秒をbufferへ加算すること。
- **REQ-SESSION-005**: 同じtask UUIDのセッションは1件だけ保持し、重複追加しないこと。
- **REQ-SESSION-006**: localStorageのtop-level JSONが不正またはversionが非対応の場合は空の`work_sessions`で表示し、元のkeyを自動上書きせずwarningを表示すること。個別entryだけが不正な場合はそのentryだけを除外し、次のlocal state変更時にvalid entryだけをversion 1として保存すること。いずれの場合もtaskを更新せず、初回`bootstrap`を継続すること。
- **REQ-SESSION-007**: Webセッションの追加・削除・復元によってSchronu本体のcurrent taskを変更しないこと。
- **REQ-SESSION-008**: セッションtab表示中にセッション終了操作またはrepository確認によってセッション件数が実際に減少して0件になった場合は、一覧tabへ切り替えること。削除失敗、件数不変、1件以上残る場合、一覧または発火履歴tabの表示中はtabを変更しないこと。

### 4.3 セッションがない場合

- **REQ-AUTO-001**: セッションが0件の場合だけ「自動セッション」buttonを表示すること。
- **REQ-AUTO-002**: 「自動セッション」を押すと、Schronuの`get_focus`相当の規則で選定したtaskをセッションへ追加すること。
- **REQ-AUTO-003**: 自動選定によってSchronu本体のcurrent taskを変更しないこと。
- **REQ-AUTO-004**: 選定対象がない場合、セッションを追加せず、その結果を利用者へ表示すること。

### 4.4 セッションcard

- **REQ-CARD-001**: 各cardにtask名を表示すること。
- **REQ-CARD-002**: セッション開始時刻をlocal timeの`HH:MM`で表示し、完了予定時刻および開始時実績と同じ補助情報領域へ置くこと。
- **REQ-CARD-003**: 完了予定時刻をlocal timeの`HH:MM`で開始時刻の右に表示し、算出不能時は`--:--`とすること。開始時刻と完了予定時刻はsemanticな`time`要素を維持し、assistive technologyが両者を識別できるlabelを持つこと。
- **REQ-CARD-004**: 完了予定時刻は`開始時刻 + max(見積秒 - 開始時実績秒, 0)`で算出すること。
- **REQ-CARD-005**: 進捗率は`(開始時実績秒 + 経過秒) * 100 / 見積秒`で算出し、整数%で表示すること。
- **REQ-CARD-006**: 見積秒が0の場合、進捗率を`--%`と表示すること。
- **REQ-CARD-007**: 進捗率は100%を超過できること。
- **REQ-CARD-008**: progress barは150%をcard内の全幅として縮尺し、100%までを通常色、100%を超えた部分をbarの右側へ伸長する赤色領域として表示すること。track全幅の3分の2にあたる100%位置には、進捗量にかかわらず常時視認できる縦の境界線を表示すること。150%を超えた部分は切り捨てず、横scrollで確認可能にすること。
- **REQ-CARD-009**: 完了までの残り時間を「残り」と大きな`MM:SS`の主表示として示し、1秒ごとに更新すること。分は59を超えてよく、320px幅でもcardを横へ超過させないこと。
- **REQ-CARD-010**: 見積時間を超過した場合、同じ主表示を「超過」と赤い文字の`MM:SS`へ切り替えて増加表示し、assistive technologyが残り時間と超過時間を識別できるlabelを持つこと。
- **REQ-CARD-011**: 見積秒が0の場合、セッション開始直後から経過秒を超過時間として同じtiming領域へ表示すること。
- **REQ-CARD-012**: browser時計がセッション開始時刻より前になった場合、表示用経過秒を0として扱い、負の実績を生成しないこと。
- **REQ-CARD-013**: セッション開始までに記録済みだった実績を、保存した開始時実績から「開始時実績 MM:SS」として補助情報領域へ表示すること。分は59を超えてよく、assistive technology向けlabelを持つこと。

### 4.5 セッション操作

- **REQ-ACTION-001**: 各cardに「計測を破棄して解除」「記録して解除」「計測を破棄して再開」「計測を破棄して完了」「記録して完了」の順で5buttonを表示すること。通常幅では解除系2buttonを1段目、再開buttonを全幅の2段目、完了系2buttonを3段目とし、34rem以下では同じ順序の1列にすること。
- **REQ-ACTION-002**: 「計測を破棄して解除」は対象セッションをlocalStorageから削除し、taskの実績を加算するserver mutationを行わないこと。削除成功後は残存セッションの未送信進捗秒からbufferを再計算して削除したセッション分の加算を取り消し、表示一覧を更新するため`list_tasks`を送ること。
- **REQ-ACTION-003**: 「記録して解除」は対象taskのUUIDと終了操作click時刻を指定し、その時刻までの経過秒を開始時実績へ加算すること。server処理中の通信待ち時間を加算せず、browser時計がserver時計より進んでいても開始・終了click時刻の差を維持すること。
- **REQ-ACTION-004**: 「記録して解除」は開始時実績を期待値として検証し、現在実績と不一致の場合はtaskを保存せず、セッションを保持すること。
- **REQ-ACTION-005**: 「記録して完了」は対象taskのUUIDと終了操作click時刻を指定し、その時刻までの経過秒加算、task完了、終了時刻更新を同じrepository transactionで処理すること。browser時計がserver時計より進んでいる場合、経過秒はclick時刻差を使い、保存する完了時刻はserver操作時刻を上限とすること。
- **REQ-ACTION-006**: 「計測を破棄して完了」は開始時刻を実績計算に使わず、追加実績秒を0とし、確定click時刻を完了時刻としてtaskを完了すること。既存の実績秒は変更しないこと。browser時計がserver時計より進んでいる場合、保存する完了時刻はserver操作時刻を上限とすること。
- **REQ-ACTION-007**: 2種類の完了は開始時実績を期待値として検証し、不一致の場合は実績加算、完了、終了時刻更新、反復task生成を一切保存せず、セッションを保持すること。
- **REQ-ACTION-008**: server処理中は同じセッションの操作buttonを無効化し、二重送信を防ぐこと。終了操作ではclick時刻でcardの進捗、残り・超過時間を停止し、serverが未commitと確定できるerror時は現在時刻基準の計測へ自動復帰すること。transport切断またはrepository状態が不確実な場合は、repository確認完了までclick時刻で停止し続けること。
- **REQ-ACTION-009**: 未知task、完了済みtask、不正な経過時間、overflow、repository errorではセッションを保持し、errorを表示すること。
- **REQ-ACTION-010**: 「記録して解除」「計測を破棄して完了」「記録して完了」はserver処理成功後だけ対象セッションを削除すること。
- **REQ-ACTION-011**: 「計測を破棄して完了」の最初のclickではserver requestを送らず、当該card内に「このセッションの計測時間は記録されません。タスクを完了しますか?」と「キャンセル」「計測を破棄して完了」を表示すること。キャンセルは元の5操作へ戻し、確定時だけrequestを1回送ること。
- **REQ-ACTION-012**: 操作buttonは`nth-child`ではなく意味別classで記録・完了・破棄の役割を表すこと。「計測を破棄して再開」と「計測を破棄して解除」は灰、「記録して解除」は青、「計測を破棄して完了」は赤、「記録して完了」は緑とすること。通常幅では解除系と完了系をそれぞれまとまりとして配置し、狭い画面では1列にすること。
- **REQ-ACTION-013**: 2種類の完了が現在実績付きの実績競合になった場合、通常の手動確認blockとは分離し、元request、初回終了click時刻、最新実績をmemoryに保持すること。cardとbufferを初回click時刻で停止し、現在実績と保持した計測を`HH:MM:SS`で示す確認groupへ通常操作を置換すること。記録ありは「計測を再開」「加算して完了」、記録なしは「計測を再開」「実績を維持して完了」を提供すること。
- **REQ-ACTION-014**: 競合確認後の完了は元requestのtask UUID、開始時刻、終了時刻、記録方針を維持し、期待実績だけを最新値へ差し替え、新しいrequest IDとsafety markerで1回送信すること。再競合では勝手に加算せず最新実績を更新して確認を続け、成功時は通常の完了cleanupを行うこと。現在実績がない旧errorまたは不正な現在実績は従来の手動確認blockへ移すこと。
- **REQ-ACTION-015**: 競合確認の「計測を再開」は、確認待ちを除外しつつ初回clickまでの計測millisecondを保持するよう開始時刻を現在時刻から補正し、最新実績を開始時実績として`WorkSession`全体をstorage-firstで置換すること。保存成功時だけ競合とerrorを解除し、失敗時は保存済みsessionと停止中の競合確認を維持してlocalStorage errorを表示すること。
- **REQ-ACTION-016**: 通常操作の「計測を破棄して再開」は、task UUID、task名、開始時見積秒、開始時実績秒を維持し、開始時刻だけをclick時刻へstorage-firstで更新すること。成功時は対象の旧経過秒をbufferから除外してセッションtabに留まり、task更新、server通信、発火履歴追加を行わないこと。保存失敗時はmemory上のsessionとbufferを維持すること。in-flight、server commit済み、global・manual safety block、完了競合中は拒否すること。

### 4.6 buffer

- **REQ-BUFFER-001**: bufferを`現在logical dateの符号付き残り容量 - 同日のschedule segmentごとのscheduled_work_seconds合計`としてserver側で算出すること。server観測時刻が日次終端より前なら、符号付き残り容量は観測時刻から日次終端までの毎週固定`busy_time_slot`控除後の空き秒とする。日次終端以後なら、符号付き残り容量は`日次終端 - server観測時刻`の0以下の秒数とし、日次終端後の全壁時計超過時間を反映する。同一taskの複数segment、進行中segment、同じlogical date内の過去segmentをそれぞれ1回ずつ全量で集計すること。
- **REQ-BUFFER-002**: serverからbuffer秒とその観測時刻を取得し、browser側で`server buffer - snapshot後の壁時計経過秒 + 各セッションのserverへ未送信の進捗秒の合計`を1秒ごとに再計算すること。未送信進捗秒は開始時刻から開始時見積到達時刻までとし、終了操作中は終了click時刻がそれより早ければその時刻で打ち切ること。ただし、完了実績競合の確認中と再送中は、見積到達後も競合を解消するまで初回click時点のbuffer表示を維持すること。この規則はbufferの正負とserver観測時刻が日次終端の前後かどうかに依存しない。
- **REQ-BUFFER-003**: 0以上のbufferを`HH:MM:SS`でカウントダウン表示すること。
- **REQ-BUFFER-004**: 負のbufferを赤い文字の`-HH:MM:SS`でカウントアップ表示すること。
- **REQ-BUFFER-005**: logical dateが06:00境界で変化しても、それだけを理由にserverから再取得しないこと。
- **REQ-BUFFER-006**: 次の明示的server操作のresponseでlogical dateとbuffer snapshotを更新すること。
- **REQ-BUFFER-007**: 開始時見積内のセッションは、snapshotの前後にかかわらずsnapshot後の壁時計減算を1秒ずつ相殺する進捗秒をbufferへ加算すること。見積到達またはより早い終了click後は対象セッションの加算を止め、ほかに加算対象がなければbufferを実時間と同速で減算すること。ただし、完了実績競合の確認中と再送中はREQ-ACTION-013を優先し、初回click後の壁時計減算も相殺して競合解消まで表示を固定すること。
- **REQ-BUFFER-008**: 複数セッションの未送信進捗秒は重複を除かずセッションごとに合算すること。2セッションを同時に10分計測した場合は20分を加算すること。server commit済みでlocalStorage削除失敗により残ったセッションは加算対象から除外し、未commitが確定できるerror時は計測を再開し、transport切断またはrepository状態が不確実な場合はrepository確認完了まで終了click時刻で加算を打ち切ること。完了実績競合では通常の未送信進捗に加え、初回clickから競合解消までの壁時計減算を相殺し、click以前に減算済みのbufferを巻き戻さないこと。
- **REQ-BUFFER-009**: 「計測を破棄して解除」のlocalStorage削除成功後は、残存セッションの未送信進捗秒だけでbufferを再計算すること。全セッションを破棄した場合はsnapshot後の全経過秒を減算すること。保存失敗時はmemory上のセッションを維持し、buffer表示を変化させないこと。
- **REQ-BUFFER-010**: 新しいserver responseを受信した場合は、そのbuffer秒と観測時刻を新たな表示計算の基準とし、page内開始とreload復元を区別せず、現在保持する各セッションの開始時刻から観測時刻までの未送信進捗秒も加算すること。一覧の再取得でserver bufferが進んだ観測時刻分減っても、同じ進捗秒を新基準へ加算すること。

### 4.7 一覧画面

- **REQ-LIST-001**: serverから得た現在logical dateを起点として、連続する8 logical datesのbuttonを表示すること。
- **REQ-LIST-002**: 先頭のbuttonを`曜 今日`、2番目を`曜 明日`、3番目以降を曜日で表示すること。
- **REQ-LIST-003**: 日付取得では曜日名ではなく具体的なlogical dateをserverへ送ること。
- **REQ-LIST-004**: 選択したlogical dateのschedule segmentを開始時刻の昇順で表示すること。
- **REQ-LIST-005**: 各行に締切、予定時間、task名、セッション追加buttonを表示すること。46remを超える画面ではbuttonの表示を「セッション」、46rem以下では左端の幅44pxかつ高さ32pxのbuttonを「＋」とし、いずれもassistive technologyがtask名とセッション追加操作を識別できるlabelを持つこと。左スワイプによる直接発火は行わないこと。
- **REQ-LIST-006**: 予定時間をlocal timeの`HH:MM-HH:MM`で表示すること。
- **REQ-LIST-007**: 現在時刻が締切を過ぎた場合、締切を赤色で表示すること。
- **REQ-LIST-008**: schedule rankが0であるtask(未完了の子を持たないtask)のtask名を緑色で表示すること。
- **REQ-LIST-009**: 一覧の「セッション」buttonは対象taskをlocalの`work_sessions`へ追加し、追加に成功した場合はセッションtabへ切り替えること。server通信は行わないこと。
- **REQ-LIST-010**: 対象task UUIDのセッションが存在する場合、同じtaskを表すすべてのschedule segmentの「セッション」buttonを無効化すること。
- **REQ-LIST-011**: schedule rankが0でないtaskは「セッション」buttonを表示せず、client stateが手動追加要求を受けても`work_sessions`へ追加しないこと。
- **REQ-LIST-012**: 4種類のセッション終了操作が成功した場合は、選択中のlogical date、または未選択なら最新snapshotのlogical dateを指定して`list_tasks`を送り、response全体で表示一覧を置換すること。完了成功response受理時点でin-flightの古い`list_tasks` requestを無効化し、その後に到着したresponseは適用しないこと。server errorでは追加取得せず、server commit成功後に対象sessionのlocalStorage削除だけが失敗した場合は安全状態を維持したまま一覧を再取得すること。
- **REQ-LIST-013**: 全幅で一覧を`セッション追加、予定、締切、task名`の順に表示し、可視の列headerを維持すること。46rem以下では罫線区切りの1行tableとし、列幅は`44px 5.75rem 5.5rem minmax(0, 1fr)`、rowの高さは32px以上とする。締切と予定は固定列で折り返さず、task名だけを1行のままcell内で横スクロール可能にし、task名cellの縦overflowとpage全体の横scrollを発生させないこと。セッション追加済みのrank 0 taskは同一UUIDの全segmentでdisabledの「✓」、未追加なら「＋」、rank非0なら空の操作cellを表示すること。
- **REQ-LIST-014**: 日付buttonの直下にtask名検索欄を表示し、前後空白を除外した英字大小無視の部分一致で取得済みrowを即時に絞り込むこと。空または空白だけなら全rowを表示し、一致しない場合は空結果を案内すること。検索文字列は日付・tab切替とreloadで保持すること。一覧からセッションを追加できた場合は検索文字列と絞り込みを解除し、追加の保存失敗または拒否時は保持すること。この解除で選択日、日付入力、日付入力errorを変更しないこと。46rem以下では検索欄を高さ36px、入力中だけ表示するclear buttonを36px四方とし、clear後は検索欄へkeyboard focusを戻すこと。検索入力とclearではserver通信、task更新、発火履歴追加を行わず、view stateだけをlocalStorageへ保存すること。
- **REQ-LIST-015**: 日付buttonの下に`M/D`または`YYYY/M/D`を入力してEnterまたは「表示」で一覧取得できること。年省略時はserver snapshotの現在logical dateを含む未来方向の直近日へ解決し、同じ月日は当日、過ぎた月日は翌年とすること。妥当な入力は`YYYY/M/D`へ正規化してpage内に保持し、serverへは`YYYY-MM-DD`を送ること。不正入力はfieldと関連付けたerrorを表示して通信せず、日付button選択時は入力とerrorを消去すること。desktopでは日付入力をtask名検索の左、46rem以下では検索の上に配置し、狭幅でもviewportを超えないこと。

### 4.8 通信制限と発火履歴

- **REQ-NET-001**: server通信を初回`bootstrap`、日付選択、`自動セッション`、`記録して解除`、`計測を破棄して完了`の確定、`記録して完了`、完了実績競合の再完了、および4種類のセッション終了成功後の`list_tasks`に限定すること。
- **REQ-NET-002**: tab切替、毎秒tick、一覧検索の入力・clear、一覧の「セッション」と追加成功後の検索解除・tab切替、「計測を破棄して再開」、`計測を破棄して完了`の確認表示とキャンセルではserver通信を行わないこと。
- **REQ-NET-003**: 「発火履歴」tabを選択した場合だけ、発火履歴を独立したsectionとして表示できること。
- **REQ-NET-004**: 発火履歴はserver通信結果の直近100件をmemory内だけに保持し、reload時に消去すること。localStorage操作は記録しないこと。
- **REQ-NET-005**: 各履歴に操作時刻、実際に呼び出したserver action名と全送信引数、成功・失敗を表示すること。引数は関数呼出し形式で表示し、client内部の`request_id`は含めないこと。
- **REQ-NET-006**: 実行していないCLI command名を履歴へ記録せず、実際にresponseを受信したserver操作を記録すること。
- **REQ-NET-007**: 「記録して完了」と「計測を破棄して完了」は、実際の`complete_session`呼出しと`record_elapsed_seconds`の真偽を履歴へ記録し、失敗時もどちらを試みたか識別できること。
- **REQ-NET-008**: 利用者起点のserver通信ではrequest開始からresponse適用まで画面全体に待機表示を出し、背面操作を無効にすること。SSR初期HTMLとhydration前の表示は全面overlayを出さず「画面を復元しています…」を表示すること。localStorage復元後の`bootstrap`と保存日付の`list_tasks`は背景更新とし、保存一覧があれば「前回の表示です。最新状態を確認中…」、なければ「最新状態を確認中…」を表示すること。この背景更新statusは全tab共通でbottom navigation直上に浮遊表示し、表示・消去で本文の位置を変えないこと。背景更新中は「計測を破棄して再開」を含むlocal操作を許可し、日付選択・送信、自動セッション、記録・完了・競合再送、「計測を破棄して解除」はUIとreducerの両方で拒否すること。失敗時は保存一覧を維持して同じ浮遊表示に再試行buttonを表示すること。
- **REQ-NET-010**: `schronu_web.view_state.v1`へ最後に成功した`ServerSnapshot`、最後に表示した1日分のlogical dateと全`ScheduledTaskRow`、選択tab、検索文字列、日付入力文字列をversion付きでatomicに保存すること。空一覧の成功も保存し、破損・未知version・不正行・read/write失敗はwarningにしてwork sessionやserver mutationをwrite-blockしないこと。発火履歴、通信中state、error、確認dialogは保存しないこと。
- **REQ-NET-009**: 完了実績競合とその再完了は、それぞれ実際に送信した全引数と失敗・成功を通常どおり履歴へ記録すること。「計測を再開」はlocalStorage操作なので履歴へ記録しないこと。

### 4.9 持ち歩きロック

- **REQ-LOCK-001**: 画面上部へ常時表示されるstickyな持ち歩きロックbarを設け、通常モードでは「持ち歩きロック」の1 clickで即時にロックできること。持ち歩きロックだけを理由に画面を覆うoverlayや画面内容を非表示にする方式は使用しないこと。server通信中の待機表示は`REQ-NET-008`を優先すること。
- **REQ-LOCK-002**: ロック中もbufferとセッションの表示・更新、scroll、tab切替、日付選択、一覧取得を利用可能とすること。
- **REQ-LOCK-003**: ロック中は「自動セッション」、一覧からの「セッション」追加、「計測を破棄して再開」、「計測を破棄して解除」、「記録して解除」、「計測を破棄して完了」、「記録して完了」、完了実績競合の再完了・計測再開、「repository確認済み」の変更操作をclientの共通guardで遮断すること。
- **REQ-LOCK-004**: ロックbarをpointerまたはSpace・Enterで1.2秒長押しすると、15秒間変更操作を許可すること。pointerup、pointerleave、pointercancel、buttonのblur、window scroll、または1.2秒未満の入力終了では長押しを成立させないこと。
- **REQ-LOCK-005**: 一時許可中に封印対象の変更操作をdispatchするたび、成功可否や実際の変更有無にかかわらず、単調時計による無操作期限をdispatch時点から15秒後へ更新すること。ただし、最初のsession追加成功後は`REQ-LOCK-013`の即時再ロックを優先すること。封印対象外の操作では期限を更新せず、期限到達または単調時計の後退で再ロックすること。
- **REQ-LOCK-006**: 「計測を破棄して完了」は確認表示では無操作期限を更新せず、確定dispatchで更新すること。確認のキャンセルでは一時許可と期限を維持し、一時許可の期限切れでは確認表示を閉じて再ロックすること。
- **REQ-LOCK-007**: ロックbarは通常、ロック中、一時許可中を識別可能に表示し、一時許可中は残り秒数を表示すること。ロック中は独立した状態表示を置かず、44px以上の長押しbutton内へ「操作ロック中」と「1.2秒長押しで15秒間操作可能」を集約し、通常モードへ戻す`details`だけを次の行へ置くこと。一時許可中は「操作可能」と表示し、`aria-live`では状態遷移だけを通知して、毎秒変化する残り秒数を通知対象にしないこと。
- **REQ-LOCK-008**: 通常モードへ戻す操作はロックbarの`details`内へ分離し、確認操作の成功後だけ永続的に解除すること。
- **REQ-LOCK-009**: 持ち歩きロックはrepository不確実性を扱うmutation safetyとは独立したclient stateとして、localStorageの`schronu_web.carry_lock.v1`へ`version: 1`と`enabled`を保存すること。keyなしは通常モード、正常値は保存状態を復元し、ロック状態のreloadでは一時許可を復元せずロック状態とすること。
- **REQ-LOCK-010**: 持ち歩きロックのJSON不正、未知version、読込失敗は元のvalueを上書きせず、warning付きのロック状態とすること。ロック開始はmemory-firstで反映し、保存失敗時も現在のpageではロックを維持してreload後の危険を警告すること。通常モードへの復帰はstorage-firstとし、保存失敗時はロックを維持すること。
- **REQ-LOCK-011**: 完了実績競合の確認表示は、元の完了dispatch後も保持すること。再完了と計測再開はそれぞれ独立した変更dispatchとして共通guardを通し、一時許可中なら成否にかかわらず無操作期限を各dispatch時点から15秒後へ更新すること。
- **REQ-LOCK-012**: 一時許可中は残り秒数の横へ「今すぐロック」buttonを表示し、1 clickで即時にロック状態へ戻せること。この操作はserver通信、発火履歴追加、localStorage更新を行わず、実行後の変更操作を共通guardで直ちに遮断すること。
- **REQ-LOCK-013**: 一時許可中に一覧または自動選定からsessionの追加が成功し、件数が0件から1件になった場合は、無操作期限の延長より優先して即時再ロックすること。追加失敗、重複、自動選定結果なし、server error、および1件以上からの追加では、この再ロックを行わないこと。

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
| AC-001 | viewport下端に「セッション」「一覧」「発火履歴」の固定tabが表示され、desktopで44px以上、46rem以下で40px以上の均等幅button、safe area、本文との非重複、選択indicatorと`aria-pressed`を維持し、利用者向け文言に「フォーカス」が残っていない。 |
| AC-002 | 2件以上のセッションが同時に1秒ごとに進み、reload後も元の開始時刻から復元される。server buffer表示は各セッションの未送信進捗秒を個別に加算し、終了操作中の加算は見積到達時刻と終了click時刻の早い方で打ち切る。 |
| AC-003 | 15分見積、開始時実績5分のtaskはセッション開始直後に33%となり、100%および133%で指定どおりのbarを表示する。いずれの進捗でもtrack全幅の3分の2に100%境界線を表示する。 |
| AC-004 | 残り・超過`MM:SS`がtiming領域の主表示となり、開始`HH:MM`、完了予定`HH:MM`、開始時実績`MM:SS`が補助情報として表示され、320px幅でもcardが横へ超過しない。各値をassistive technologyが識別でき、見積0のtaskは`--%`と赤い超過時間を表示し、長時間の分表示は59を超えても欠落しない。 |
| AC-005 | 日次終端前は毎週固定`busy_time_slot`控除後の空き秒、日次終端ちょうどは予定作業がなければ0、日次終端後は壁時計超過秒を負値とするbufferがserver観測時刻を基準に変化する。browserはsnapshot後の壁時計経過秒を1回減算し、各セッションの未送信進捗秒を重複ごと個別に加算する。1セッションの見積内では通常停止し、同時計測ではセッションごとの進捗が加算され、見積到達またはより早い終了click後は対象の加算を止める。一覧を再取得しても新server bufferへ同じ未送信進捗を足し、正負どちらのbufferも符号どおり表示する。 |
| AC-006 | 06:00境界、3画面のtab切替、毎秒tick、一覧からのセッション追加、計測を破棄して再開、破棄完了の確認とキャンセルではserver requestが増えない。 |
| AC-007 | 初回、日付選択、自動セッション、記録、2種類の完了確定、および4種類のセッション終了成功後の一覧再取得だけが仕様どおりのserver requestを発生させる。 |
| AC-008 | 一覧に8 logical datesが表示され、両端が同じ曜日でも具体日付で別の日として取得される。 |
| AC-009 | 一覧は開始時刻順で、締切超過は赤、schedule rank 0のtask名は緑になる。rank非0ではセッションbuttonを表示せず、セッション中のrank 0 taskでは全segmentのbuttonが無効になる。 |
| AC-010 | 計測を破棄して解除ではtaskが変わらず、記録では終了操作clickまでの完了済み整数秒だけが加算される。記録して完了では同じclick時刻をtask終了時刻として加算と完了が1 transactionで保存され、計測を破棄して完了では既存実績を変えずtaskだけが完了する。 |
| AC-011 | 別processで実績が変化した後の記録・2種類の完了は競合となり、taskと反復taskを保存せず、Webセッションを保持する。2種類の完了は初回clickまでの計測を失わず、最新実績での明示的な再完了または確認待ちを除外した計測再開を選べる。 |
| AC-012 | CLI`働`は秒端数を保持し、引数なしは整数秒、明示指定は分から秒へ換算して加算し、失敗時はfocusを保持する。 |
| AC-013 | MCPのtool schemaと既存contract testの期待値が変更されず、CLI`働`以外のCLI contract testも変更なしで成功する。 |
| AC-014 | 発火履歴tabの選択時だけ独立sectionがDOMへ表示され、実際のserver action名、全送信引数、成否を区別して100件まで表示し、localStorage操作を表示せず、reload後は空になる。 |
| AC-015 | 各cardに5操作が指定順で表示され、計測を破棄して再開は解除系2操作の後で経過秒だけを破棄し、計測を破棄して完了はcard内の確認を経た確定時だけ1回送信され、キャンセルでは送信されない。3終了操作はclick時刻でcardの計測を停止し、通信待ちで表示や実績を増やさない。2種類の完了は`record_elapsed_seconds`の真偽を含む発火履歴で区別される。 |
| AC-016 | 4種類のセッション終了が成功すると選択中または最新snapshotのlogical dateで一覧を再取得し、実績変更後の再schedule、完了taskの除去、反復taskを含むresponse全体で置換する。終了失敗では再取得せず一覧とsessionを保持し、server commit成功後にlocalStorage削除だけが失敗した場合は安全状態を維持して一覧を再取得する。 |
| AC-017 | 320pxから46remまでの画面幅で一覧が可視header付きの高さ32px以上の1行tableとなり、左端の幅44px・高さ32pxの「＋」またはdisabledの「✓」、固定された締切・予定、cell内だけを横スクロールできる長いtask名を表示する。task名cellに縦scrollbarを表示せず、viewport全体は横に超えず、rank非0の操作cellは空になる。曜日button、日付入力・表示button、検索欄は高さ36px、検索clear buttonは36px四方、曜日・入力・検索・table間は8pxとする。46remを超える画面では従来のdesktop tableを維持し、34rem以下ではbufferを圧縮する。 |
| AC-018 | 通常モードから1 clickで持ち歩きロックを有効化でき、ロック中は状態と説明を長押しbutton内へ集約した2行以内のbarを表示する。ロック中も画面表示・更新、scroll、tab切替、日付選択、一覧取得を利用できる一方、10変更操作はdispatchされない。 |
| AC-019 | 44px以上のbuttonをpointerまたはSpace・Enterで1.2秒長押しすると15秒間許可され、封印対象操作のdispatchごとに成否を問わず無操作期限が15秒後へ延長される。ただし、最初のsession追加成功時は即時再ロックが優先される。閲覧操作と確認キャンセルでは延長せず、各中断event、期限到達、単調時計の後退で安全側へ戻る。一時許可中は残り秒数の横に44px以上の「今すぐロック」を表示し、1 clickで通信・履歴・保存なしに即時再ロックする。 |
| AC-020 | 持ち歩きロックの正常な保存値を復元し、ロック状態では圧縮したbarを表示する。不正値・未知version・読込失敗では元valueを維持してwarning付きで同じロック表示にする。ロック開始の保存失敗ではmemory上のロックを維持し、通常モード復帰の保存失敗では解除しない。一時許可はreload後に復元しない。 |
| AC-021 | タッチ主体の端末ではbuttonをタップした後にhover配色が残らず、hover可能なfine pointerでは既存hover表現が適用される。選択済み日付buttonはdesktop hover中も緑背景と白文字を維持し、`:active`と`:focus-visible`は両環境で機能する。 |
| AC-022 | 利用者起点の一覧取得、自動選定、記録、2種類の完了の各server通信中は全画面の「通信中…」とスピナーが表示され、背面を操作できない。reload直後の背景更新では全面overlayを出さず、前回表示または未取得状態と更新statusを示す。複数の通常通信は最後のresponseまでoverlayを維持し、成功と各error応答の完了後に解除される。 |
| AC-023 | 曜日button直下の検索欄へtask名の一部を入力すると、前後空白を除外した英字大小無視の部分一致で取得済みrowだけが即時表示され、同一taskの複数segmentはすべて残る。日付・tab切替とreloadでは検索文字列を保持し、clearで全rowへ戻って検索欄へkeyboard focusが戻る。一覧からセッションを正常に追加すると検索と絞り込みだけが解除され、選択日と日付入力は保持される。保存失敗または追加拒否時は検索を保持する。入力、clear、追加成功後の検索解除はserver通信と発火履歴追加を行わず、view stateだけを保存する。 |
| AC-024 | セッションtabで最後のセッションを正常に削除すると一覧tabへ移り、複数セッション中の1件削除、server失敗、localStorage削除失敗、完了競合の計測再開、通常操作の計測を破棄して再開、一覧・発火履歴tab表示中の削除では強制遷移しない。既存の一覧再取得以外にtab遷移由来のserver通信を追加しない。 |
| AC-025 | 一覧の日付欄へ`9/16`を入力すると、現在logical dateが9月16日なら当日、9月17日以後なら翌年の9月16日を`YYYY-MM-DD`で取得する。`2026/9/16`は指定年を維持する。入力はreload後も復元され、曜日button選択で消去される。不正値と空白だけの入力はserver通信と発火履歴追加を行わない。 |
| AC-026 | reload後に最後のsnapshot、1日分の一覧、選択tab、検索、日付入力、作業中セッションを復元し、保存日付の最新取得が成功した時だけ一覧全体を置換する。日跨ぎ、bootstrap失敗、一覧取得失敗でも前回一覧を維持し、失敗時は再試行できる。背景更新中は「計測を破棄して再開」を含むlocal操作が成功し、server依存操作と「計測を破棄して解除」はUIとreducerの両方で拒否される。 |
| AC-027 | 持ち歩きロックの一時許可中に一覧または自動選定から最初のsessionを追加すると即時再ロックする。sessionが増えない場合と1件以上からの追加では一時許可を維持する。 |
