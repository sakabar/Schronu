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

## 3. 用語

| 用語 | 定義 |
| --- | --- |
| セッション | Web UIでtaskへの作業時間を計測する状態。Schronu本体のcurrent taskとは独立する。 |
| `work_session` | 内部で単一のセッションを表す名称。 |
| `work_sessions` | 内部で複数のセッションを表す名称。 |
| logical date | 06:00を日付境界とするSchronu上の日付。00:00から05:59までは前日として扱う。 |
| buffer | 現在logical dateの残り空き秒から、同日の予定残作業秒を引いた値。 |
| 開始時実績 | セッションを開始した時点のtaskの実績作業秒。 |
| 経過秒 | セッション開始時刻から現在またはserver操作時刻までに完了した整数秒。 |

## 4. 機能要件

### 4.1 共通画面

- **REQ-COMMON-001**: 画面上部に「セッション」「一覧」のtabを表示し、選択中の画面を識別できること。
- **REQ-COMMON-002**: tab切替はclient内だけで処理し、server通信を発生させないこと。
- **REQ-COMMON-003**: URL routingを必要とせず、単一ページ内で画面を切り替えられること。
- **REQ-COMMON-004**: 利用者に見える名称には「フォーカス」を使用せず、「セッション」を使用すること。既存core APIの`get_focus`は内部の選定処理として利用してよい。
- **REQ-COMMON-005**: 初回表示時に1度だけserverからsnapshotを取得し、bufferとlogical dateを初期化すること。
- **REQ-COMMON-006**: server操作に失敗した場合、直前の表示データと`work_sessions`を保持したまま、再試行可能なerrorを表示すること。

### 4.2 セッション状態

- **REQ-SESSION-001**: 複数のセッションを同時に保持し、それぞれの経過時間を独立して計測できること。
- **REQ-SESSION-002**: `work_sessions`をlocalStorageの`schronu_web.work_sessions.v1`へ保存すること。
- **REQ-SESSION-003**: 各`work_session`はtask UUID、task名、開始epoch milliseconds、開始時見積秒、開始時実績秒を保持すること。
- **REQ-SESSION-004**: reload後は保存した開始時刻と現在時刻との差から各セッションを復元し、reload中の経過時間も反映すること。
- **REQ-SESSION-005**: 同じtask UUIDのセッションは1件だけ保持し、重複追加しないこと。
- **REQ-SESSION-006**: localStorageの内容が不正または非対応versionの場合、不正な項目を実行状態として採用せず、taskを更新しないこと。
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

- **REQ-ACTION-001**: 各cardに「破棄して解除」「記録して解除」「完了」の3buttonを表示すること。
- **REQ-ACTION-002**: 「破棄して解除」は対象セッションをlocalStorageから削除するだけとし、taskの実績を加算せず、server通信を行わないこと。
- **REQ-ACTION-003**: 「記録して解除」は対象taskのUUIDを指定し、server操作時刻までの経過秒を開始時実績へ加算すること。
- **REQ-ACTION-004**: 「記録して解除」は開始時実績を期待値として検証し、現在実績と不一致の場合はtaskを保存せず、セッションを保持すること。
- **REQ-ACTION-005**: 「完了」は対象taskのUUIDを指定し、経過秒の加算とtask完了を同じrepository transactionで処理すること。
- **REQ-ACTION-006**: 「完了」も開始時実績を期待値として検証し、不一致の場合は実績加算、完了、終了時刻更新、反復task生成を一切保存せず、セッションを保持すること。
- **REQ-ACTION-007**: 「記録して解除」と「完了」はserver処理成功後だけ対象セッションを削除すること。
- **REQ-ACTION-008**: server処理中は同じセッションの操作buttonを無効化し、二重送信を防ぐこと。
- **REQ-ACTION-009**: 未知task、完了済みtask、不正な経過時間、overflow、repository errorではセッションを保持し、errorを表示すること。

### 4.6 buffer

- **REQ-BUFFER-001**: bufferを`現在logical dateの残り空き秒 - 同日の予定残作業秒`としてserver側で算出すること。
- **REQ-BUFFER-002**: serverからbuffer秒とその観測時刻を取得し、以後はbrowser側で経過秒を差し引いて1秒ごとに表示を更新すること。
- **REQ-BUFFER-003**: 0以上のbufferを`HH:MM:SS`でカウントダウン表示すること。
- **REQ-BUFFER-004**: 負のbufferを赤い文字の`-HH:MM:SS`でカウントアップ表示すること。
- **REQ-BUFFER-005**: logical dateが06:00境界で変化しても、それだけを理由にserverから再取得しないこと。
- **REQ-BUFFER-006**: 次の明示的server操作のresponseでlogical dateとbuffer snapshotを更新すること。

### 4.7 一覧画面

- **REQ-LIST-001**: serverから得た現在logical dateを起点として、連続する8 logical datesのbuttonを表示すること。
- **REQ-LIST-002**: 先頭のbuttonを`曜 今日`、2番目を`曜 明日`、3番目以降を曜日で表示すること。
- **REQ-LIST-003**: 日付取得では曜日名ではなく具体的なlogical dateをserverへ送ること。
- **REQ-LIST-004**: 選択したlogical dateのschedule segmentを開始時刻の昇順で表示すること。
- **REQ-LIST-005**: 各行に締切、予定時間、task名、「セッション」buttonを表示すること。
- **REQ-LIST-006**: 予定時間をlocal timeの`HH:MM-HH:MM`で表示すること。
- **REQ-LIST-007**: 現在時刻が締切を過ぎた場合、締切を赤色で表示すること。
- **REQ-LIST-008**: 葉taskのtask名を緑色で表示すること。
- **REQ-LIST-009**: 一覧の「セッション」buttonは対象taskをlocalの`work_sessions`へ追加するだけとし、server通信および画面遷移を行わないこと。
- **REQ-LIST-010**: 対象task UUIDのセッションが存在する場合、同じtaskを表すすべてのschedule segmentの「セッション」buttonを無効化すること。

### 4.8 通信制限と発火履歴

- **REQ-NET-001**: server通信を初回`bootstrap`、日付選択、`自動セッション`、`記録して解除`、`完了`に限定すること。
- **REQ-NET-002**: tab切替、毎秒tick、一覧の「セッション」、`破棄して解除`ではserver通信を行わないこと。
- **REQ-NET-003**: 画面上に開閉式の発火履歴を表示できること。
- **REQ-NET-004**: 発火履歴は直近100件をmemory内だけに保持し、reload時に消去すること。
- **REQ-NET-005**: 各履歴に操作時刻、操作種別、対象UUID、local/serverの別、成功・失敗を表示すること。
- **REQ-NET-006**: 実行していないCLI command名を履歴へ記録せず、実際のWeb操作を記録すること。

### 4.9 application操作と互換性

- **REQ-APP-001**: application層の実績加算は、UUID、追加実績秒、任意の期待実績秒を入力とする1つの操作へ集約し、CLIとWebで共用すること。
- **REQ-APP-002**: 実績加算は非負の追加秒だけを許可し、期待実績が指定された場合は現在値との一致を更新前に検証すること。
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
- **REQ-NFR-003**: client/server間ではUUID、epoch milliseconds、秒数、`YYYY-MM-DD`をwire形式として使用すること。
- **REQ-NFR-004**: errorは原因を識別可能な型付きerrorとし、競合と再試行可能なrepository errorを区別できること。
- **REQ-NFR-005**: server responseはserver観測時刻、現在logical date、buffer秒を含み、clientが同じ基準時刻から表示を更新できること。
- **REQ-NFR-006**: UIの自動更新はbrowser内の計算に限定し、意図しないtask dataの読み書きを発生させないこと。

## 6. 受入条件

| ID | 受入条件 |
| --- | --- |
| AC-001 | 画面上に「セッション」「一覧」が表示され、利用者向け文言に「フォーカス」が残っていない。 |
| AC-002 | 2件以上のセッションが同時に1秒ごとに進み、reload後も元の開始時刻から復元される。 |
| AC-003 | 15分見積、開始時実績5分のtaskはセッション開始直後に33%となり、100%および133%で指定どおりのbarを表示する。 |
| AC-004 | 見積0のtaskは`--%`と赤い超過時間を表示し、長時間の分表示は59を超えても欠落しない。 |
| AC-005 | 正、0、負のbufferがserver観測時刻を基準に毎秒変化し、負値は赤い符号付き表示になる。 |
| AC-006 | 06:00境界、tab切替、毎秒tick、一覧からのセッション追加、破棄ではserver requestが増えない。 |
| AC-007 | 初回、日付選択、自動セッション、記録、完了だけが仕様どおりのserver requestを発生させる。 |
| AC-008 | 一覧に8 logical datesが表示され、両端が同じ曜日でも具体日付で別の日として取得される。 |
| AC-009 | 一覧は開始時刻順で、締切超過は赤、葉task名は緑、セッション中taskのbuttonは全segmentで無効になる。 |
| AC-010 | 破棄では実績が変わらず、記録では完了済み整数秒だけが加算され、完了では加算とtask完了が1 transactionで保存される。 |
| AC-011 | 別processで実績が変化した後の記録・完了は競合となり、taskと反復taskを保存せず、Webセッションを保持する。 |
| AC-012 | CLI`働`は秒端数を保持し、引数なしは整数秒、明示指定は分から秒へ換算して加算し、失敗時はfocusを保持する。 |
| AC-013 | MCPのtool schemaと既存contract testの期待値が変更されず、CLI`働`以外のCLI contract testも変更なしで成功する。 |
| AC-014 | 発火履歴がlocal/serverと成否を区別して100件まで表示し、reload後は空になる。 |
