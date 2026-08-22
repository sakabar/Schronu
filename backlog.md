# Schronu 技術的負債バックログ

- 監査日: 2026-08-15
- 対象revision: `ec43b68`
- 対象範囲: 追跡中のRustコード、shell script、Apps Script、設定、CI、README
- 評価方針: 現在の正確性とデータ保全への影響、障害時の回復性、変更時の波及範囲、検証容易性を優先して評価する

## 検証結果

監査時点では次の結果だった。

| 検証 | 結果 | 備考 |
| --- | --- | --- |
| `cargo test -q` | 成功 | 563件成功、1件ignored、失敗0件 |
| `cargo fmt --check` | 成功 | 差分なし |
| `cargo clippy --all-targets -- -D warnings` | 失敗 | testを中心にzero-prefixed literal、needless borrow、assertion on constant、option_env unwrapなどが残る |
| `git ls-files Cargo.lock` | 出力なし | 実行バイナリを配布する構成だが、`Cargo.lock`はignoreされ未追跡 |

テストが広く存在する点は強みである。一方、現在のCIはtestとformatしか実行せず、リポジトリガイドが要求するclippyを品質ゲートにしていない。

## 優先度

- `P0`: 正確性、データ保全、または通常入力に対するプロセス継続性へ直接影響する。最優先で契約テストを追加して修正する。
- `P1`: 障害リスクや変更コストが高く、今後の機能追加を阻害する。P0の安定化後に着手する。
- `P2`: 計画的に解消すべき設計、性能、検証容易性の問題。
- `P3`: 直ちに障害へつながりにくいが、意図の誤読や将来の不具合を招く整理不足。

概算規模は、既存テストの維持とRed/Greenの各commitを含む相対値である。

- `S`: 1日以内
- `M`: 2-4日
- `L`: 1-2週
- `XL`: 複数段階に分割すべき規模

## 一覧

| ID | 優先度 | 完了状況 | 概算 | 項目 |
| --- | --- | --- | --- | --- |
| TD-001 | P0 | 完了 | L | 毎週定期の行動不能時間が将来日・日跨ぎ計算へ正しく反映されない |
| TD-002 | P0 | 完了 | M | 行動不能時間YAMLの異常が回復可能なエラーではなくpanicになる |
| TD-003 | P0 | 完了| L | 永続化YAMLの不正値が黙って既定値や新規UUIDへ変換される |
| TD-004 | P1 | 完了 | XL | `Task`の木構造と内部可変性が暗黙の共有状態とpanic前提を作っている |
| TD-005 | P1 | 完了 | XL | CLIコントローラーへ責務が集中している |
| TD-006 | P1 | 完了 | L | CLIの入力・application・出力エラーが握り潰される |
| TD-007 | P1 | 完了 | L | CLIとMCPでrepository transactionが別々に組み立てられている |
| TD-008 | P1 | 完了 | M | CIがリポジトリ規約を満たさず、ビルド再現性も固定されていない |
| TD-009 | P2 | 未着手 | L | entity層がYAML形式へ依存している |
| TD-010 | P2 | 完了 | L | 現在時刻、UUID、業務日境界がドメイン内部へ埋め込まれている |
| TD-011 | P2 | 完了 | L | MCPのschema、入力検証、Rust入力型、JSON出力が重複している |
| TD-012 | P2 | 未着手 | L | flatten・pack・scheduleの再計算コストに性能上限が定義されていない |
| TD-013 | P2 | 完了 | M | Spreadsheetの列契約が複数言語・文書へ重複している |
| TD-014 | P2 | 完了 | M | 実環境計測で同期処理に有意な高速化の見込みがないことを確認した |
| TD-015 | P2 | 完了 | L | テストが巨大な製品ファイルへ混在し、fixtureも重複している |
| TD-016 | P3 | 未着手 | M | マジック値、未使用フィールド、古いコメントが意図を曖昧にしている |
| TD-017 | P1 | 完了 | XL | `TaskHandle`の既存infallible APIが内部不変条件の破れをpanicとして扱う |
| TD-018 | P1 | 未着手 | XL | CLI runtimeにcommand orchestrationと表示計算が残っている |

## 詳細

### TD-001: 毎週定期の行動不能時間が将来日・日跨ぎ計算へ正しく反映されない

- 優先度: `P0`
- 概算規模: `L`
- 完了日: 2026-08-15
- 対応: 曜日別の定期ruleを保持し、照会区間をローカル日付境界で分割して適用するようにした。70日限定の事前展開と未使用の`end_of_day_hour` / `end_of_day_minute`のdomain model依存を廃止した。
- 検証: `cargo fmt --check`と`cargo test -q`は成功した。`cargo clippy --all-targets -- -D warnings`はTD-008で記録済みの既存warning群により失敗するが、本項目由来のwarningは解消した。

#### 現状と根拠

- `src/adapter/gateway/free_time_manager.rs:44-68` は、毎週の定義を起動時点から70日分だけ日付別mapへ展開する。71日目以降は定義が存在しないため、空き時間として扱われる。
- `src/adapter/gateway/free_time_manager.rs:149-162` は、問い合わせが日を跨ぐと、最初の日の23:59以降をすべて自由時間として加算する。中間日と終了日の定期行動不能時間は参照されない。
- `src/adapter/gateway/free_time_manager.rs:98-99` では、`end_of_day_minute`を`end_of_day_hour`キーから読み込んでいる。
- `src/entity/busy_time_slot.rs:38-56` では曜日と終了時刻が`_`付きfieldへ格納され、その後の計算に使われない。
- READMEは`busy_time_slot`を毎週定期の行動不能時間として説明し、28日先まで扱う`平`などの計算にも反映される契約を示している。現在の実装は問い合わせ期間によって結果が変わる。

#### 影響

- 遠い将来の予定ほど利用可能時間が過大評価され、schedule、pack、flattenの配置が実際の生活時間と一致しなくなる。
- 日を跨ぐ同じ区間でも、分割して問い合わせた場合と一括で問い合わせた場合に結果が一致しない。
- 現在未使用の終了時刻を将来利用した際、誤って読み込まれた分値が潜在不具合として顕在化する。

#### 推奨する改善方針

- 曜日ごとの定期ルールをsource of truthとして保持し、問い合わせ区間をローカル日付境界で分割して各日にルールを適用する。
- 日付別mapは明示的に登録した例外や計算cacheだけに限定し、固定の70日展開へ正確性を依存させない。
- 半開区間`[start, end)`、23:59-翌00:00、複数日、70日超、業務日境界06:00の扱いを契約として固定する。
- `end_of_day_hour`と`end_of_day_minute`を利用するなら正しいkeyと有効範囲を検証し、不要ならschemaとdomain modelから同時に除去する。

#### 完了条件

- 1日、2日、3日以上、70日超の問い合わせで、各曜日の定期枠がすべて反映される。
- `get_free_minutes(a, c) == get_free_minutes(a, b) + get_free_minutes(b, c)`が日付境界を跨いでも成立する。
- 23:59、00:00、06:00付近の境界テストがある。
- 既存のschedule、pack、flatten契約テストが変更や緩和なしで通る。

#### 依存関係

- TD-002の型付き読込エラーと同じモデルを利用する。
- TD-010の日時ポリシーを先に設計すると境界処理の重複を避けられるが、正確性修正自体を待たせない。

### TD-002: 行動不能時間YAMLの異常が回復可能なエラーではなくpanicになる

- 優先度: `P0`
- 概算規模: `M`

#### 現状と根拠

- `src/adapter/gateway/free_time_manager.rs:32-40` はファイルopenとreadを`unwrap`する。
- 同ファイル`77-123`はYAML parse、document、曜日、配列、時刻、duration、nameを`panic!`、`unwrap`、`expect`で処理する。
- 同ファイル`49`は7曜日がすべて存在する前提でmapを`unwrap`する。
- `src/application/interface.rs:72-80` の`FreeTimeManagerTrait::load_busy_time_slots_from_file`は戻り値が`()`で、adapterから失敗理由を返せない。
- `register_busy_time_slot`も日跨ぎ入力でpanicし、入力契約を型として表現していない。

#### 影響

- 設定ファイルの欠落や1fieldのtypoでCLI全体が異常終了する。
- ユーザーはどのpath、曜日、fieldが不正かを機械的に判別できない。
- MCPや非対話CLIから構造化されたエラー応答を返せず、復旧手順を提示できない。

#### 推奨する改善方針

- path、YAML field path、値、原因を保持する`BusyTimeSlotLoadError`をadapter層へ導入する。
- `FreeTimeManagerTrait`のloadと、必要ならregisterを`Result`にし、情報量を落とさず呼出し元へ伝搬する。
- YAML全体を一時modelへ厳密に変換・検証してから、`FreeTimeManager`の状態を一括更新する。途中失敗時は既存状態を維持する。
- CLIは設定エラーとしてstderrへ表示し、MCPは構造化エラーへ変換する。

#### 完了条件

- file not found、permission denied、不正YAML、曜日欠落、未知曜日、不正時刻、負数duration、日跨ぎslotについてpanicしない。
- すべての失敗に対象pathとfield pathが含まれる。
- 読込途中の失敗で既存のfree-time状態が部分更新されない。
- production経路を通る異常系テストがある。

#### 依存関係

- TD-001と同じ変更系列で進められるが、Redテストとcommitは分離する。
- TD-007の共通transactionとは独立して先行できる。

### TD-003: 永続化YAMLの不正値が黙って既定値や新規UUIDへ変換される

- 優先度: `P0`
- 概算規模: `L`

#### 現状と根拠

- `src/adapter/gateway/yaml.rs:320-365`は`TaskAttr::new`を既定値生成に使い、不正または型違いのname、status、boolean、priority、category、work seconds、repetition設定、UUIDを既定値へ置換する。
- UUIDが不正な場合は`Task::new`で生成された新しいUUIDがそのまま残る。再読込ごとにidentityが変わり得る。
- `src/adapter/gateway/yaml.rs:333-395`は不正日時をエラーにせず、pendingは最小時刻、create/startは`Task::new`時の現在時刻、deadline/endは`None`として扱う。
- `read_repetition_anchor`は未知値を`Deadline`へ変換し、present-but-invalidとfield欠落を区別しない。
- `task_children_yaml`だけは型違いを`YamlConversionError`にしており、fieldごとに厳密さが不統一である。
- `src/adapter/gateway/task_repository.rs:424-460`はYAML文書自体とproject nodeのエラーを保持するが、field変換で黙って失われた情報は検知できない。

#### 影響

- typoや破損した永続データが一見正常にloadされ、次回saveで誤った既定値として確定する可能性がある。
- UUIDの変化により、Spreadsheet、MCP、focus状態など外部参照が切れる。
- create/start/deadlineの変化がschedule順序や反復タスク生成へ波及する。

#### 推奨する改善方針

- 「fieldが欠落した旧形式」と「fieldは存在するが不正」を明確に区別する。
- 互換性のため欠落fieldにはversionごとの既定値を許可し、present-but-invalidはtask path、field、原因付きの変換エラーにする。
- UUID、status、category、日時、非負秒数、反復間隔などを一時的な永続化DTOへ厳密にparseしてから`Task`を構築する。
- 既存データを走査する検証コマンドまたはdry-run migrationを用意し、厳格化前に不正データを発見できるようにする。

#### 完了条件

- 不正UUID、不正enum、型違い、曖昧・存在しないローカル日時、不正な数値がpath付きエラーになる。
- 欠落を許可するfieldと採用する互換既定値がテストと文書で一致する。
- 1projectでもloadに失敗した場合、repositoryのmemory状態とdisk状態を変更しない。
- 正常な既存fixtureをload-save-loadしてUUIDと全永続fieldが維持される。

#### 依存関係

- TD-009の層分離を同時に完了させようとせず、まずgateway内で厳密化する。
- migrationが必要な既存データを確認してから既定値契約を削除する。

### TD-004: `Task`の木構造と内部可変性が暗黙の共有状態とpanic前提を作っている

- 優先度: `P1`
- 概算規模: `XL`
- 完了日: 2026-08-15
- 対応: 共有可変な`Task`を`TaskHandle`へ全面移行し、独立した`TaskSnapshot`を追加した。create、reparent、親追加、連番生成をfallible APIへ集約し、`TaskTreeError`をapplication、controller、MCPまで保持した。strict YAML loaderもfallible tree operationを経由させ、deadline伝搬のmutation revisionを1操作につき1回へ一元化した。
- 検証: `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`に成功した。testは392件、controller testは219件、MCP binary testは2件、integration testは12件が成功し、1件はignored。

#### 現状と根拠

- `src/entity/task.rs:1147-1150`の`Task`は`dendron::Node<TaskAttr>`を保持し、deriveされた`Clone`は独立copyではなく同じ木を共有する。この契約は`src/entity/task.rs:2888`付近のテストで確認されるが、型名からは判別できない。
- `Task`のsetterは`&self`から内部を変更し、`borrow_data_mut`による実行時borrow検査へ依存する。
- `src/entity/task.rs:1222-1238`はdendronの制約を回避するためdummy rootを生成し、`root()`や`parent()`がその存在を暗黙に扱う。
- `src/entity/task.rs:1626-1653`はhierarchy grantとinsertを`expect`し、公開APIの`Result<String>`より先にpanicし得る。
- `src/entity/task.rs:1664-1675`の`create_as_parent`は`detach_insert_as_last_child_of`の戻り値を無視し、内部操作が失敗しても`Ok(())`を返す。
- 各setterと木構造操作が手作業で`mark_persistent_mutation`を呼ぶため、新しい変更経路でdirty判定を更新し忘れる余地がある。

#### 影響

- cloneした値の変更が別の呼出し元から観測され、所有権とmutation範囲の推論が難しい。
- borrow競合や木構造前提の破れが通常の`Result`ではなくpanicになる。
- dirty tracking漏れは変更がsaveされない障害へ直結する。
- tree libraryの都合がentity全体とテストfixtureへ漏れ、ライブラリ更新やモデル変更の費用が高い。

#### 推奨する改善方針

- まず`TaskHandle`など共有handleであることが明確な内部表現と、読み取り専用snapshotを区別する。
- 木構造のcreate、move、reparent、removeを少数の失敗原子的なdomain operationへ集約する。
- mutation revisionはoperation成功時に1か所で更新し、個々のsetterに責務を分散させない。
- `String`エラーを構造化し、循環、root操作、borrow、insert失敗を区別する。
- 最終的なtree実装の置換は別段階とし、最初に現在の共有・順序・deadline伝搬契約を固定する。

#### 完了条件

- 共有handleと独立snapshotの違いがAPI名と型で判別できる。
- 失敗するreparentで元と移動先の木が変化せず、成功時だけrevisionが進む。
- 公開domain operationに木構造由来の`expect`、`unwrap`、無視された`Result`がない。
- root、parent、children、順序、deadline伝搬、dirty trackingの契約テストが製品経路を通る。

#### 依存関係

- TD-003のstrict loaderが木を組み立てるため、現在の構築契約を先にテストで固定する。
- TD-009、TD-010は本項目を小さくするが、同一PRへまとめない。

### TD-017: `TaskHandle`の既存infallible APIが内部不変条件の破れをpanicとして扱う

- 優先度: `P1`
- 概算規模: `XL`
- 完了日: 2026-08-15
- 対応: `TaskHandle`の公開read、write、tree操作を`Result<_, TaskTreeError>`へ統一し、infallible APIと`try_*`互換APIを除去した。借用競合とdummy root不整合は構造化errorとしてapplication、CLI、MCPへ伝搬する。更新前のborrow可否検証により、attribute、tree、mutation revisionの原子性を保証した。
- 検証: 借用競合、dummy root欠落・複数child、tree操作、deadline伝搬、appointment、CLI/MCP error形式の契約testを追加した。`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`に成功した。

#### 現状と根拠

- TD-004で`try_get_attr`、`try_snapshot`、fallible tree operationを導入した一方、互換性のため`TaskHandle::new`、`root`、`snapshot`、`get_attr`はinfallible APIとして残っている。
- `new`と`root`はdummy rootの子ノード存在を`expect`し、内部不変条件が壊れた場合にpanicする。
- `root`をfallible化すると、getter、setter、dirty tracking、deadline伝搬、repository trait、application、CLI、MCPおよび多数のfixtureへ`Result`のerror contractが波及する。

#### 影響

- borrow競合やtree不変条件の破れを、呼出し元が構造化errorとして扱えない経路が残る。
- CLIとMCPでdomain内部エラーの表示・JSON error contractが統一されない。
- `try_*` APIと旧infallible APIが並存し、どちらを選ぶべきか利用者が判断する必要がある。

#### 推奨する改善方針

- `new`、`root`、read APIをfallible APIへ統一し、旧infallible APIを削除する。
- `TaskTreeError`をrepository/application errorへ保持したまま、CLI表示とMCP structured errorへ変換する。
- productionとfixtureを段階移行し、各層でerrorを握り潰さない契約テストを追加する。

#### 完了条件

- 公開`TaskHandle` APIにtree由来の`expect`、`unwrap`、panic前提のread/mutationがない。
- constructor、root探索、snapshot、mutationの失敗理由を型で判別できる。
- CLI、MCP、repository、applicationの各経路で構造化errorが保持される。
- 既存のYAML形式、CLI表示、MCP JSON契約、dirty trackingを維持する。

#### 依存関係

- TD-004の`TaskHandle`、`TaskSnapshot`、`TaskTreeError`を基盤として利用する。
- TD-006のerror分類と整合させ、同一PRでCLI分割を行わない。

### TD-005: CLIコントローラーへ責務が集中している

- 優先度: `P1`
- 概算規模: `XL`
- 完了日: 2026-08-18
- 対応: binary entrypointをprivate module宣言と`runtime::application()`呼び出しへ限定した。CLI入力をtyped commandへ変換するparser、typed context経由でcommandを処理するhandler、`DisplayModel`とwriterを扱うrenderer、対話入力とterminal制御を担うinteractive driver、実行結果・repository transaction・外部I/Oを調停するruntimeへ境界を分割した。Spreadsheet A-J列の出力はrendererの専用formatterへ集約した。
- 検証: 日本語・英語alias、typed fieldとparse error、各command群のdispatch、Spreadsheet A-J列、表示順・ANSI・改行・flush・broken pipe、interactive submit・refresh・終了、外部起動・保存時点・transaction errorの契約testを追加した。`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`に成功した。

#### 対応前の現状と根拠

- `src/adapter/controller/schronu.rs`は約10,500行あり、1つのbinary entrypointへコマンドparse、日時parse、application呼出し、表示整形、terminal raw mode、入力thread、外部URL起動、repository transaction、テストfixtureが同居する。
- `execute_with_config`は`src/adapter/controller/schronu.rs:6212`から始まる巨大な文字列matchで、日本語・英語alias、引数個数、parse、domain mutation、表示を同時に扱う。
- `main`は同ファイル`8276`付近にある一方、対話UIの実装とテストはその後も約2,000行続く。
- `全`の出力はSpreadsheetの列契約でもあり、表示変更がshell、Apps Script、READMEへ波及する。

#### 影響

- 小さなコマンド追加でも巨大なmatch、表示、transaction、対話・非対話テストを同時に理解する必要がある。
- parse失敗時の挙動がcommand branchごとに異なり、TD-006の握り潰しを生む。
- 製品コードとfixtureの境界が不明瞭で、clippyやreviewの信号対雑音比が低い。

#### 推奨する改善方針

- 先に各commandのalias、引数、出力、mutation有無をcharacterization testで固定する。
- `Command` enumへ変換するparser、application command handler、renderer、interactive terminal driverへ段階的に分割する。
- interactiveとnon-interactiveで同じparse・execute経路を使い、terminal固有処理だけを外側へ残す。
- Spreadsheet向け出力は人間向け表示から独立した明示的formatterとして扱う。

#### 完了条件

- 日本語・英語aliasと現在の有効入力が同じtyped commandへparseされる。
- command handlerはraw terminal、環境変数、外部browser、文字列tokenizeへ依存しない。
- rendererのgolden testで既存CLI出力とSpreadsheet出力の互換性が固定される。
- binary entrypointは依存構築、mode選択、終了code変換に限定される。

#### 依存関係

- TD-006のerror分類をparserとhandlerのinterfaceへ反映する。
- TD-007のtransaction境界をhandlerの外側へ置く。
- TD-013のSpreadsheet契約テストを分割前に用意する。

#### 残存負債

- runtimeにはcommand固有helper、domain orchestration、表示計算が残っている。handlerはruntimeをimportせずprivate context trait経由で処理するが、そのcontext実装はruntimeが担う。runtime縮小と意味的な表示modelへの移行はTD-018で扱う。
- `DisplayModel`はraw fragmentとwriter固有改行・flushを保持するmodelであり、tree、task list、calendar、band、focus、errorなどを意味的な型として表していない。
- CLIのtest fixtureとhelperは`runtime.rs`に残っている。製品コードの移動とは別commitに分け、TD-015で`test_support`へ分離する。

### TD-018: CLI runtimeにcommand orchestrationと表示計算が残っている

- 優先度: `P1`
- 概算規模: `XL`

#### 現状と根拠

- `src/adapter/controller/schronu/runtime.rs`は11,477行あり、repository transactionと外部I/Oの調停に加えて、command固有helper、日時解釈、domain operationの組み立て、tree・calendar・band・focusなどの表示計算、246件のruntime testとfixtureを保持する。
- `handler.rs`はruntimeをimportせず、typed `Command`とprivateな`ProjectCommandContext`、`TaskTreeCommandContext`、`TaskAttributeCommandContext`、`DeferCommandContext`、`FinishPlacementCommandContext`を介して処理する。一方、それらcontextの製品実装と多数のcommand helperはruntimeに残る。
- rendererの`DisplayModel`はraw fragment、writer固有改行、flushの順序を保持するrecording modelであり、表示対象の意味を型として表現していない。

#### 影響

- commandのdomain処理や表示内容を変更する際にruntimeの広い範囲を理解する必要があり、typed境界を導入しても変更範囲を十分に局所化できない。
- 表示互換性の検証がraw出力の記録に依存し、treeやcalendarなど意味的な表示model単位でrendererを検証できない。
- runtimeが調停層とcommand実装層を兼ね、依存構築・transaction・外部I/O以外にも複数の変更理由を持つ。

#### 推奨する改善方針

- commandごとのapplication呼出し、domain orchestration、focus変更判断をhandler側へ移し、再利用すべき処理だけをapplication層へ抽出する。
- tree、task list、calendar、band、focus、errorを表す意味的な表示modelを定義し、rendererはそのmodelから既存CLI出力を生成する。Spreadsheet A-J列の専用formatterは維持する。
- runtimeを依存構築、parse mode選択、repository transaction、外部I/O、interactive/non-interactive調停、終了code変換へ限定する。
- command名、alias、表示文言、Spreadsheet列、YAML、MCP契約を維持し、command単位の小さいRed/Greenで移行する。

#### 完了条件

- runtimeにcommand固有のdomain mutation、command引数の日時解釈、表示計算が残らず、handlerの製品経路をfake contextで検証できる。
- `DisplayModel`がtree、task list、calendar、band、focus、errorなどの意味を表し、raw fragment recordingをhandlerとrendererの主境界にしない。
- rendererのgolden testが意味的な表示modelから既存CLI出力を生成し、Spreadsheet A-J列の契約testも維持される。
- interactive/non-interactiveが同じparser・handler・renderer経路を通り、既存のtransaction、save、error分類を維持する。

#### 依存関係

- TD-006、TD-007、TD-013で確定したerror、transaction、Spreadsheet契約を維持する。
- test fixture/helperの`test_support`分離は製品コードの責務移動と独立しているため、TD-015として別commitで進める。

### TD-006: CLIの入力・application・出力エラーが握り潰される

- 優先度: `P1`
- 概算規模: `L`
- 完了日: 2026-08-15
- 対応: `CommandError`でparse、application、external open、outputの失敗を区別し、command実行の失敗を診断表示してrepository保存を抑止するようにした。stdoutの最初のI/O失敗を捕捉し、broken pipeは正常終了、それ以外は`CommandError::Output`として伝搬するようにした。
- 検証: CLI入力エラー、保存抑止、stdout error、broken pipe、改行出力の契約testを追加し、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`を成功させた。

#### 現状と根拠

- `src/adapter/controller/schronu.rs:2175`付近のtask作成、`3984`付近の延期、`4137-4248`付近のdeadline・estimate・category変更などでapplicationの`Result`が無視される。
- `execute_with_config`内にはparse失敗時に何も表示しないbranchや空の`Err(_)`があり、入力誤りと未実行を区別できない。
- `src/adapter/controller/schronu.rs:3554`と`3619`付近はbrowser・Obsidian起動結果を無視する。
- 多数の`writeln_newline(...).unwrap()`と`stdout.flush().unwrap()`があり、broken pipeやterminal I/O障害でpanicする。
- command branchによってエラー表示、無視、panicが混在している。

#### 影響

- ユーザーはcommandが成功したのか、入力が不正だったのか、保存前にdomain操作が拒否されたのかを判断できない。
- pipe先の終了やterminal障害がbacktraceを伴う異常終了になり得る。
- application errorがadapterで消えるため、MCPとCLIで同じ操作の意味が一致しない。

#### 推奨する改善方針

- `CommandParseError`、`ApplicationError`、`RepositoryError`、`OutputError`、`ExternalOpenError`を区別したCLI error modelを作る。
- typed command handlerは`Result<CommandOutcome, CommandError>`を返し、interactive driverが継続・再描画・終了を決定する。
- broken pipeは正常な出力終了として扱えるようにし、それ以外のI/O失敗はsource chainを保持する。
- 無視が意図的な副作用は明示的に記録し、少なくともdiagnosticを返す。

#### 完了条件

- 全commandの不正引数に一貫したfield付きメッセージが返る。
- domain拒否、load/save失敗、外部open失敗、stdout失敗をテストで区別できる。
- production codeにapplication `Result`を無条件で捨てる箇所がない。
- interactiveで継続できるエラーとprocessを終了するエラーの一覧が文書化される。

#### 依存関係

- TD-005のparser・handler分割と同じ設計を使うが、commandごとの小さいRed/Greenで移行する。
- TD-007より先にerror typeを定義すると共通transactionの戻り値を安定させられる。

### TD-007: CLIとMCPでrepository transactionが別々に組み立てられている

- 優先度: `P1`
- 概算規模: `L`
- 完了日: 2026-08-15
- 対応: lock、reload、operation、条件付きsaveを共通transaction実行器へ集約し、CLIとMCPから利用するようにした。read-only operationと実変更のないMCP更新はsaveを行わず、save失敗は`StateUncertain`としてMCPの既存`repository_state_uncertain` / `restart_server`契約へ変換する。
- 検証: read-only CLI transaction、MCPの更新・入力エラー・save失敗・同値更新、MCP stdioの契約testを追加・維持した。`cargo test --locked`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`に成功した。

#### 現状と根拠

- CLIは`src/adapter/controller/schronu.rs:8236`付近の`run_cli_repository_transaction`やreload helperでlock、load/reload、command、saveを管理する。
- MCPは`src/adapter/mcp.rs:37-180`のserver lifecycle内でlock、reload、dispatch、save失敗とstate uncertainを独自に管理する。
- 読取commandと更新commandのrepository要件がadapter側の分岐へ埋め込まれている。
- `TaskRepositoryTrait`はquery、mutation、clock同期、load、reload、saveを1つのtraitに持ち、application test doubleも永続化関心を実装する必要がある。

#### 影響

- 新しいadapterやcommand追加時に、lock順序、reload条件、save条件、失敗時状態を再実装する必要がある。
- CLIとMCPで同じapplication操作の一貫性を維持しにくい。
- 更新途中の失敗時に「memoryは変わったがdiskは変わらない」状態をadapterごとに扱う必要がある。

#### 推奨する改善方針

- application境界にread-only queryとmutating commandを明示した共通実行器を置く。
- 実行器がlock、freshness確認、operation、変更検出、save、失敗時のstate classificationを一貫して管理する。
- repositoryのdomain access interfaceとpersistence lifecycle interfaceを分離する。
- MCP lifecycleやinteractive redrawなどadapter固有状態は共通実行器の外側に残す。

#### 完了条件

- CLIとMCPが同じtransaction実行器を通る。
- read-only operationはsaveせず、mutating operationも実変更がない場合は不要なsaveをしない。
- load失敗、operation失敗、save失敗、lock競合の状態遷移が共通契約テストで固定される。
- save失敗後に再試行可能か、reload必須かを戻り値の型から判定できる。

#### 依存関係

- TD-006のerror分類を利用する。
- TD-004のmutation revisionを当面の変更検出として維持し、その置換は別変更にする。

### TD-008: CIがリポジトリ規約を満たさず、ビルド再現性も固定されていない

- 優先度: `P1`
- 概算規模: `M`
- 完了日: 2026-08-15
- 対応: Rust 1.97.1を`rust-toolchain.toml`とCIで固定し、`Cargo.lock`を追跡した。CIは`cargo test --locked`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`を実行する。既存のproduction codeとtest codeのClippy違反を挙動不変で解消し、ignored性能testの用途と手動実行方法をREADMEへ記載した。
- 検証: `cargo test --locked --offline`は615件成功、1件ignored、失敗0件。`cargo fmt --check`と`cargo clippy --locked --offline --all-targets -- -D warnings`は成功した。

#### 現状と根拠

- `AGENTS.md`は`cargo clippy -- -D warnings`を検証手順に含めるが、監査時の`cargo clippy --all-targets -- -D warnings`は失敗した。
- `.github/workflows/ci.yml:17-26`はstable Rustでtestとformatだけを実行し、clippy componentも導入しない。
- `Cargo.toml:28-34`は2つの実行バイナリを定義するが、`.gitignore:6-8`は`Cargo.lock`をignoreし、実際に未追跡である。
- Rust toolchainまたはMSRVの宣言がなく、stable更新日に新しいlintやcompiler挙動でCI結果が変わる。

#### 影響

- repository guide上は必須の品質基準がmainで継続的に検証されない。
- dependency解決結果が環境や実行時期で変化し、過去revisionのbuild再現性が低い。
- clippyを後から有効化するほど、機能変更と無関係な修正量が増える。

#### 推奨する改善方針

- 既存clippy違反をproductionとtestに分け、挙動を変えない機械的修正として小さいcommitで解消する。
- CIに`cargo clippy --all-targets -- -D warnings`を追加し、ローカルガイドと同じcommandを実行する。
- `Cargo.lock`を追跡し、dependency更新は意図したPRで行う。
- `rust-toolchain.toml`またはpackageの`rust-version`で採用方針を宣言し、更新手順をREADMEへ記載する。

#### 完了条件

- test、format、all-target clippyがローカルとCIの両方で成功する。
- clean checkoutが追跡済みlockfileを使って同じdependency graphを解決する。
- Rust version更新が通常の機能PRへ偶発的に混入しない。
- ignoredの性能testは用途と実行方法が文書化される。

#### 依存関係

- 他の大規模refactor前に完了させる。
- TD-015のテスト分離を待たず、現在の配置のままlintをGreenにする。

### TD-009: entity層がYAML形式へ依存している

- 優先度: `P2`
- 概算規模: `L`

#### 現状と根拠

- `src/entity/task.rs:1-8`が`yaml_rust::Yaml`と`LinkedHashMap`へ依存する。
- `src/entity/task.rs:2110`以降の`task_to_yaml`がfield省略規則、文字列形式、rootだけにcategoryを出力する規則をentity内で実装する。
- `src/adapter/gateway/task_repository.rs:325-343`はentityが返すYAMLへproject wrapperを追加して保存する。
- load側は`src/adapter/gateway/yaml.rs`にあるため、永続化のreadとwriteが別レイヤへ分裂している。

#### 影響

- entityのfield変更がYAML表現と直結し、別storage形式やversion migrationを導入しにくい。
- domain testとserialization testが同じ巨大ファイルへ混在する。
- adapterからentityへの依存方向というrepository guideline上の境界が曖昧になる。

#### 推奨する改善方針

- gatewayに永続化DTOとencode/decodeを集約する。
- entityは必要な状態をsnapshotとして公開し、YAML key、field省略、日付formatを知らないようにする。
- encodeとstrict decodeのround-trip契約を同じmoduleで管理する。
- format変更には明示的なschema versionまたはmigration方針を設ける。

#### 完了条件

- `src/entity`から`yaml_rust`と永続化keyへの依存がなくなる。
- encode/decodeの正常・互換・異常fixtureがgateway testsへ集約される。
- 既存YAMLの出力順、既定field省略、category配置が意図なく変化しない。

#### 依存関係

- TD-003のstrict decodeを先行する。
- TD-004のtree model全面変更とは分離し、現在のsnapshot APIから移動を始める。

### TD-010: 現在時刻、UUID、業務日境界がドメイン内部へ埋め込まれている

- 優先度: `P2`
- 概算規模: `L`
- 完了日: 2026-08-21
- 対応: entityのtask生成からsystem clockとUUID生成を除去し、operation固定時刻と注入可能なUUID生成器を持つ`TaskFactory`へ集約した。06:00の業務日境界、subjective date、日次終端offset、deadline bufferを`BusinessDateTimePolicy`へ統合し、曖昧・不存在local timeは情報付き`ApplicationError`として伝搬する。CLI、MCP、YAML decodeはoperation入口の同一時刻snapshotをreload、入力既定値、task生成へ共有する。
- 検証: `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功した。testは813件成功、1件ignored、失敗0件(entity/applicationを含むlib 465件、CLI 331件、MCP 2件、MCP stdio 12件、Spreadsheet 3件)。entity production codeの`Local::now()` / `Uuid::new_v4()`と旧暗黙constructorが0件であることも静的監査した。

#### 現状と根拠

- `src/entity/task.rs:898-924`の`TaskAttr::new`が`Local::now()`と`Uuid::new_v4()`を直接呼ぶ。
- `src/entity/task.rs:260-326`の`ImmutableTask`にも現在時刻依存constructorがある。
- `src/application/daily_capacity.rs:109-124`は日付を`Local::now().timezone()`でlocal datetimeへ変換する。
- `src/entity/datetime.rs:12-25`は06:00を直接埋め込み、`LocalResult`を`unwrap`する。
- MCPにもreload時刻や省略時の完了時刻として`Local::now()`があり、呼出し単位で時刻snapshotが統一されない可能性がある。
- deadline bufferの5分・60分や業務日終端offsetは複数層で扱われる。

#### 影響

- constructorとparseが実行時刻によって異なる結果を返し、再現可能なテストとmigrationが難しい。
- 1操作内で日付境界を跨ぐと、異なる`now`が混在し得る。
- timezoneや存在しない・曖昧なlocal timeの扱いが関数ごとに異なる。

#### 推奨する改善方針

- application operationの入口で`now`を1回取得し、entityとgatewayへ明示的に渡す。
- ID生成もapplication境界のfactoryへ置き、productionはUUID v4、testは固定列を使う。
- 06:00境界、subjective date start/end、deadline bufferを1つの日時ポリシーへ集約する。
- local datetime変換は`LocalResult`の`Single`のみを採用し、`Ambiguous`と`None`を情報付きエラーにする。

#### 完了条件

- entity constructorがsystem clockと乱数源を直接呼ばない。
- 1application operation内の全taskへ同一`now`が使われる。
- 06:00前後とlocal time変換失敗の契約テストがある。
- productionで現在時刻を取得する場所がadapter/application入口に限定される。

#### 依存関係

- TD-003のstrict decodeへ固定clockを提供する。
- TD-001の日付分割処理と日時ポリシーを共有する。

### TD-011: MCPのschema、入力検証、Rust入力型、JSON出力が重複している

- 優先度: `P2`
- 概算規模: `L`
- 完了日: 2026-08-21
- 対応: MCP adapterをprotocol、handler、registry、input、outputへ分割した。9 toolのSerde入力DTOをschema生成、decode、handler変換の契約源とし、schemaに基づくpreflight validationでも既存のJSON-RPC errorとtool-level structured errorを維持した。`TaskView`と`ScheduledTaskView`はSerde serializationへ統合した。
- 依存: `schemars 1.2.2`、`serde_path_to_error 0.1.20`を追加し、test専用の`jsonschema 0.49.3`は`default-features = false`とした。既存の`chrono`と`uuid`では`serde` featureを有効にした。
- 検証: `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`に成功した。MCPのprotocol contract 18件、tool contract 48件、output contract 3件、stdio integration 12件が成功した。既存の手動save性能計測1件は意図どおりignoredのまま維持した。
- 残存負債: `Local::now()`とrepository同期時刻のclock注入はTD-010へ残した。互換性維持のため、生成schemaを使うpreflight validationは意図的に残している。

#### 完了の証跡

- 9 toolすべてでtyped DTOからschemaを生成し、同じDTOをfield path付きdecodeとapplication inputへの変換に使用する契約testを追加した。
- `tools/list`のgolden fixtureとschema/decode matrixで、required、nullable、型違い、unknown field、境界値の可否を照合した。
- `-32602`とtool-level `invalid_input`の区別、field、reason、`structuredContent`、`content.text`をgoldenおよびbusiness testで固定した。
- `adapter::mcp::protocol_contract_tests`と`adapter::mcp::tool_contract_tests`を個別filterで実行でき、protocol lifecycleとtool business contractを独立して検証した。

#### 着手時の現状と根拠

- `src/adapter/mcp.rs:699-1243`はtoolごとの入力structに加え、required、optional、nullable、UUID、日時、非負整数、additional propertyを手作業で検証する。
- 同ファイル`1274-1310`は`TaskView`と`ScheduledTaskView`を手作業でJSONへ写像する。
- 同ファイル`1312-1491`は9 toolのJSON Schemaを別途手書きする。
- schemaで公開した制約、runtime validator、application inputの3表現を変更時に同期する必要がある。
- 製品部分だけで約1,500行、testsを含むmodule全体は5,000行を超える。

#### 影響

- schemaでは許可するがruntimeで拒否する、またはその逆のdriftが起きやすい。
- field追加のたびにvalidator、schema、serializer、test matrixの複数箇所を変更する。
- protocol lifecycleの検証と各toolのbusiness input検証が同じmoduleに混在する。

#### 推奨する改善方針

- serde対応のtool input/output型を契約源とし、deserialize errorをfield付きMCP errorへ変換する。
- schema生成を導入する場合も、現在のMCP client互換schemaをgolden testで固定してから移行する。
- JSON-RPC envelope/lifecycle、tool registry、tool handler、view serializationをmodule分割する。
- `TaskView`の公開fieldを追加した際にMCP JSONへ反映される契約テストを設ける。

#### 完了条件

- 各toolのfield定義と制約が1か所からvalidatorとschemaへ反映される。
- `tools/list` schemaと実際のdeserialize可否を同じcase集合で照合するテストがある。
- 既存のstructured error code、field、reason、structuredContentを維持する。
- protocol lifecycle testとtool business testを独立して実行できる。

#### 依存関係

- TD-015のtest fixture共通化を利用できる。
- 新しいtool追加前にregistry境界だけでも先行して分離する。

### TD-012: flatten・pack・scheduleの再計算コストに性能上限が定義されていない

- 優先度: `P2`
- 概算規模: `L`

#### 現状と根拠

- `src/application/flatten_use_case.rs:117-190`は過負荷が解消するまでloopし、候補ごとにoverride mapをcloneして全scheduleを再計算する。
- 同ファイルは再計算後に全taskのdeadline violationと日別usageを繰り返し走査する。
- `src/application/pack_use_case.rs`は候補配置ごとに最新schedule、日次余差、連続空き枠を評価し、空き枠探索を1分ずつ進める。
- `src/application/schedule_use_case.rs:232-355`は候補remove、occupied slot探索、追加後sortを繰り返す。
- repositoryには手動のsave性能testがあるが、schedule、pack、flattenの代表データ量と許容時間は定義されていない。

#### 影響

- task数、分割数、対象日数、過負荷候補が増えた際の劣化点をrelease前に検出できない。
- 根拠なしの早期最適化か、操作不能になるまで放置するかの二択になりやすい。
- 性能改善時にscheduleの決定順序を変えてしまう危険がある。

#### 推奨する改善方針

- 実データを匿名化したsmall、typical、stress fixtureを作り、schedule、pack、flattenを個別にbenchmarkする。
- task数、segment数、再schedule回数、候補試行数も計測し、時間だけでなく原因を追えるようにする。
- benchmarkで支配的と確認できた箇所から、interval index、差分usage、overrideのcopy削減、sort回数削減を行う。
- 最適化前後でtask順序、segment、deadline判定が同一であるcharacterization testを維持する。

#### 完了条件

- typicalとstressのデータ規模、測定環境、許容時間が文書化される。
- CIまたは定期測定で大幅な退行を検出できる。
- 最適化PRにbefore/afterとalgorithmic reasonが記録される。
- 性能のために既存のschedule契約テストが緩和されない。

#### 依存関係

- TD-008でbenchmarkの実行環境を固定する。
- TD-001のfree-time正確性を先に直し、誤った計算を高速化しない。

### TD-013: Spreadsheetの列契約が複数言語・文書へ重複している

- 優先度: `P2`
- 概算規模: `M`
- 完了日: 2026-08-15
- 対応: A-S列の列名、番号、同期対象、時刻書式を`spreadsheet_columns.tsv`へ集約した。Rust契約テストがmanifestを読み、shell script、Apps Script、文書の重要な列定義と照合する。CLI出力からSpreadsheet値を経由したコマンド生成もfixtureで固定した。
- 検証: `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`に成功した。

#### 現状と根拠

- `src/adapter/controller/schronu.rs`の`全`出力がA-J相当の列順を文字列formatで構築する。
- `shell/copy_for_spreadsheet.sh`は10列を前提にcutし、K-Q相当の式を列記号と行番号で生成する。
- `shell/generate_command_from_spreadsheet.sh`はB、J、N、P、Q、R、S列を数値indexで読む。
- `apps_script/main.js:1-7`はtask ID列、sync列、時刻format rangeを別の数値・A1表記で保持する。
- `README.md`、`apps_script/README.md`、`AGENTS.md`にも同じ列契約が自然言語で複製される。
- shellとApps Scriptを含むend-to-endの自動契約テストは存在しない。

#### 影響

- 1列追加・移動で、実行時エラーを出さず別fieldを読むsilent corruptionが起き得る。
- 変更者が全連動箇所を知っていることへ依存する。
- 列番号付きエラー文自体が実装とずれる可能性がある。

#### 推奨する改善方針

- 列名、index、用途、format、同期対象を1つの機械可読manifestへ定義する。
- Rust出力fixtureを`copy_for_spreadsheet.sh`へ通し、その結果を`generate_command_from_spreadsheet.sh`へ渡す契約テストを追加する。
- Apps Scriptと文書がmanifestから生成できない場合でも、同じfixture・定数一致を検証するscriptを用意する。
- 列追加は末尾追加を既定とし、既存列の意味と位置を互換契約として扱う。

#### 完了条件

- A-J出力、P/Q/R/Sの意味、L/N/P/R同期対象が自動テストで検証される。
- task nameに空白、日本語、tab相当の入力、同一taskの複数行、新規task行をfixtureに含める。
- 列変更時に連動箇所の更新漏れでCIが失敗する。
- READMEの列表と実装が同じ定義を参照または検証される。

#### 依存関係

- TD-005のCLI分割前に現行出力fixtureを作る。

### TD-014: Apps Scriptの同期処理が行数に比例してAPI呼出しを増やす

- 優先度: `P2`
- 概算規模: `M`

#### 現状と根拠

- 実際の運用ではL/N/P/R列を1セルずつ編集し、sheetは約1,000行、task数は50-100件である。
- 実Spreadsheetで安定してend-to-end 1.3-1.5秒を要した。
- 203行を走査した代表sampleでは、計測区間の合計が357msと463ms、target ID列readが134msと169ms、memory上のID検索が1msと0msだった。
- target ID列readはend-to-end時間の約9-13%であり、最適化候補とした50%を大きく下回った。
- 計測区間外の約0.8-1.1秒が支配的であり、ID列readや検索を最適化しても有意な短縮は見込めない。

#### 影響

- 現行のsimple `onEdit`を維持する。
- installable triggerやSheets batch APIの導入は複雑性に対する速度改善の根拠がないため行わない。
- 計測のために追加したlog、test、CI設定、計画文書はbranch内でrevertした。

#### 完了判断

- 今回の実測結果が同じ傾向で継続すると判断し、TD-014は「高速化の見込み無し」として完了する。
- lock競合、重複ID、同期失敗の検出は性能改善とは別の正確性課題として扱う。

### TD-015: テストが巨大な製品ファイルへ混在し、fixtureも重複している

- 優先度: `P2`
- 概算規模: `L`
- 完了日: 2026-08-22
- 対応: 巨大な製品moduleからtest bodyを挙動変更なしで別fileへ移し、CLI runtimeのunit test、contract test、test supportを分離した。application、gateway、MCPの対象testも外部化し、Task生成、repository、free-timeの同一目的fixtureをcrate境界に沿ったtest supportへ共通化した。製品API、CLI表示、YAML、MCP、Spreadsheet契約は変更していない。
- 実測test件数: lib 501 passed、1 ignored、CLI binary 333 passed、MCP binary 2 passed、MCP stdio 12 passed、Spreadsheet 4 passed。合計852 passed、1 ignoredを維持した。
- 品質ゲート: `git diff --check`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`が成功した。
- 意図的な対象外: 製品読解を妨げていない小規模inline testの`datetime`、`storage_lock`、`schronu_config`、`interactive`、testが1件だけの`flatten_use_case`はscope外として残した。これは未完了作業ではない。

#### 対応前の現状と根拠

- `src/entity/task.rs`は4,474行で、production type・operation・YAML出力と多数の個別`#[test]`が交互に配置される。
- `src/adapter/controller/schronu/runtime.rs`はproduction helperの間に大量のcommand test、repository stub、free-time stubを持つ。
- `src/adapter/mcp/input.rs`は2,895行、`src/adapter/mcp/handler.rs`は1,291行で、それぞれproduction codeに大量のtestが混在する。
- application contract testsにも類似の`TestTaskRepository`とtask builderが複数存在する。
- TD-008対応前にclippyが検出した違反の多くは古いテスト表現に由来した。現在のall-target clippyはGreenだが、production codeとtest cleanupの対象が同じ巨大fileに混在する構造は残る。

#### 影響

- productionの責務と規模を行数やreview diffから把握しにくい。
- fixtureの微妙な差により、同じ契約を検証しているようで初期値やclockが異なる。
- domain constructorが`Local::now()`へ依存するため、各testが独自に時刻を上書きする作業が増える。

#### 推奨する改善方針

- 挙動を変えず、まずtest moduleを対象subsystemごとの別fileへ移動する。
- fixed clock、fixed UUID、task tree builder、recording repository、free-time fakeを共通test supportへ集約する。
- unit test、application contract test、binary integration testの責務を明記する。
- test名とassertionは現在の契約を維持し、分離とリファクタを同じcommitに混ぜない。

#### 完了条件

- production moduleの主要type・operationをtest bodyの間から追わずに読める。
- 同じ目的のrepository/free-time fixtureが共通化される。
- 全既存test数とignored testの意図が維持される。
- all-target clippyがtestを含めて成功する。

#### 依存関係

- TD-008のclippy Green化を先行し、移動後に新旧lint差分を持ち込まない。
- CLI fixture/helperは`src/adapter/controller/schronu/runtime.rs`から`test_support`へ分離する。
- TD-004、TD-011、TD-018の大規模分割前にcharacterization testを安定させる。
- TD-018の製品コード移動とは独立して進め、test file移動とcommand実装移動を同じcommitへ混ぜない。

### TD-016: マジック値、未使用フィールド、古いコメントが意図を曖昧にしている

- 優先度: `P3`
- 概算規模: `M`

#### 現状と根拠

- 06:00の業務日境界、5分のsplit/deadline buffer、30分の日次終端offset、28日・35日のflatten範囲、70日のbusy-time展開、1400日のhobby延期などが複数moduleやcommand branchへ直接埋め込まれる。
- `src/entity/busy_time_slot.rs`の曜日と日次終了時刻は`_`付きfieldとして保持されるが利用されない。
- `src/entity/task.rs:580`付近などに大きなコメントアウト済み実装が残る。
- `src/entity/task.rs:1241`付近の「cloneして大丈夫か?」、`1559`付近の未完了テストコメント、controller内の重複を示すFIXMEなど、設計判断が未確定のまま残る。
- コメント内の`TODO`表記はrepositoryのtask status用語と衝突し、現在のAgent向け規約とも一致しない。

#### 影響

- 同じbusiness ruleを変更しても一部だけ古い値が残る。
- 未使用fieldが将来使う予定なのか、廃止済みなのか判断できない。
- コメントアウトコードが現在の候補実装に見え、reviewと検索の雑音になる。

#### 推奨する改善方針

- 値を単に共通定数へ移すのではなく、業務日、split、deadline、flatten horizonなど意味のあるpolicy単位へ集約する。
- 未使用fieldは契約と履歴を確認し、使用する項目と削除する項目を分ける。
- コメントアウトコードはversion controlへ委ねて除去する。
- 未解決コメントは背景、選択肢、完了条件を持つbacklog itemへ移し、コード内には現在の理由だけを残す。

#### 完了条件

- 同一のbusiness ruleを表す値に複数のsource of truthがない。
- private fieldが意図説明なしに`_`で抑制されていない。
- 大きなコメントアウト済み実装と古い未完了コメントが残らない。
- policy値変更時に影響する契約テストがある。

#### 依存関係

- TD-001、TD-005、TD-010でpolicy境界が確定してから段階的に整理する。
- 独立したcleanupを機能変更と同じcommitへ混ぜない。

## 推奨着手順

1. TD-008のうち`Cargo.lock`追跡と既存clippy違反の解消を行い、以後の変更に品質ゲートを設ける。
2. TD-001とTD-002を別々のRed/Green系列で直し、scheduleの入力となる自由時間を正確かつfallibleにする。
3. TD-003で永続化データのsilent fallbackを止める。厳格化前に既存データのdry-run検査を行う。
4. TD-006とTD-007でerror・transaction境界を整え、adapter間の挙動を統一する。
5. TD-013でSpreadsheet互換fixtureを固定してからTD-005のCLI境界分割を行う。その後、TD-015のCLI characterization testと`test_support`分離を先行し、残るruntime縮小と意味的表示model分離をTD-018で進める。
6. TD-010、TD-009、TD-004の順でdomain境界を狭める。tree実装の全面変更は最後の独立段階にする。
7. TD-011、TD-015を独立して進める。TD-015のCLI fixture分離はTD-018の製品コード移動とcommitを分ける。
8. TD-012はbenchmark結果を取得してから最適化範囲を決める。
9. TD-016は関連する上位項目の完了時に、小さいcleanup commitとして解消する。

## まとめて実施しない変更

- busy-timeの計算修正とYAML error API変更は、関連していてもRedテストと製品commitを分ける。
- 永続化のstrict化とentityからのYAML依存除去を同じ変更にしない。先に現在形式を厳密に守る。
- CLI分割時にcommand名、alias、表示文言、Spreadsheet列を変更しない。
- `Task`のtree実装変更とapplicationのschedule algorithm変更を同じ変更にしない。
- test file移動と既存testのassertion変更を同じcommitにしない。
- clippy cleanupへdomain挙動変更を混ぜない。
- 性能改善のためにscheduleの決定順序やdeadline契約を暗黙に変更しない。

各項目は、既存テストを削除・緩和せず、期待する契約を示すRedテスト、最小のGreen実装、全検証、レビューの順で進める。
