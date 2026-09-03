# Schronu 技術的負債バックログ

- 初回監査日: 2026-08-15
- 再監査日: 2026-09-02
- 並列開発計画更新日: 2026-09-03
- 対象revision: `8ce90d7`
- 対象範囲: 追跡中のRustコード、shell script、Apps Script、設定、CI、README
- 評価方針: 現在の正確性とデータ保全への影響、障害時の回復性、変更時の波及範囲、検証容易性を優先して評価する

## 検証結果

再監査時点では次の結果だった。既存の品質ゲートはGreenであり、今回追加した項目はlint違反の列挙ではなく、正常入力で到達する不整合、失敗原子性、未検証の境界、運用上の制約を対象とする。

| 検証 | 結果 | 備考 |
| --- | --- | --- |
| `cargo test --locked` | 成功 | 1060件成功、2件ignored、失敗0件 |
| `cargo test --locked --features benchmarking --test scheduling_benchmark_contract` | 成功 | 16件成功、失敗0件 |
| `cargo fmt --check` | 成功 | 差分なし |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | 成功 | default buildと`benchmarking` featureを含めwarningなし |
| `git ls-files Cargo.lock` | 成功 | `Cargo.lock`を追跡済み |

テストが広く存在し、CIもtest、benchmark contract、format、clippyを実行している点は強みである。一方、Greenである既存testの一部が誤挙動を明示的に固定しているため、修正時は既存assertionを安易に緩和せず、READMEとdomain contractのどちらを正とするかをRed testで先に示す必要がある。

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
| TD-009 | P2 | 完了 | L | entity層がYAML形式へ依存している |
| TD-010 | P2 | 完了 | L | 現在時刻、UUID、論理日境界がドメイン内部へ埋め込まれている |
| TD-011 | P2 | 完了 | L | MCPのschema、入力検証、Rust入力型、JSON出力が重複している |
| TD-012 | P2 | 完了 | L | flatten・pack・scheduleの再計算コストに性能上限が定義されていない |
| TD-013 | P2 | 完了 | M | Spreadsheetの列契約が複数言語・文書へ重複している |
| TD-014 | P2 | 完了 | M | 実環境計測で同期処理に有意な高速化の見込みがないことを確認した |
| TD-015 | P2 | 完了 | L | テストが巨大な製品ファイルへ混在し、fixtureも重複している |
| TD-016 | P3 | 完了 | M | マジック値、未使用フィールド、古いコメントが意図を曖昧にしている |
| TD-017 | P1 | 完了 | XL | `TaskHandle`の既存infallible APIが内部不変条件の破れをpanicとして扱う |
| TD-018 | P1 | 完了 | XL | CLI runtimeにcommand orchestrationと表示計算が残っている |
| TD-019 | P2 | 完了 | L | scheduling性能計測の状態がapplicationの業務ロジックへ伝播している |
| TD-020 | P0 | 完了 | M | 同日・同名またはsanitize後に同名となるprojectが同じ保存先を共有し、再読込時に1件消失する |
| TD-021 | P1 | 未着手 | M | repositoryが重複UUIDを受理し、ID指定操作の対象が走査順に依存する |
| TD-022 | P1 | 未着手 | XL | 複数project保存でrevisionだけが先行し、失敗時にdisk snapshotが部分更新される |
| TD-023 | P1 | 完了 | S | `終`が不正時刻と一部application errorを成功扱いで握り潰す |
| TD-024 | P1 | 完了 | M | CLI parserが不正な数値や余分な引数を黙って受理し、更新commandを実行する |
| TD-025 | P1 | 未着手 | M | 対話CLIのterminal I/O失敗がpanicまたは未検査結果になる |
| TD-026 | P1 | 未着手 | L | task名をCLI・YAML・MCP・Spreadsheet間で安全にround-tripできない |
| TD-027 | P1 | 完了 | S | 残作業時間の補正計算が合法な大値入力で整数overflowする |
| TD-028 | P1 | 完了 | M | 論理日境界を跨ぐschedule segmentの容量が開始日に全量計上される |
| TD-029 | P1 | 未着手 | L | 反復task完了の後段失敗で完了状態と親見積もりだけが部分更新される |
| TD-030 | P1 | 未着手 | S | 00:00以降の日次残容量計算がbusy timeを無視する |
| TD-031 | P1 | 未着手 | S | Spreadsheet変換がrank 1000以降のtask行を黙って破棄する |
| TD-032 | P1 | 完了 | S | macOS標準環境でSpreadsheet変換の`tac`依存が空出力の成功になる |
| TD-033 | P1 | 未着手 | M | 同一taskの複数segmentをApps Scriptが別行へ同期する |
| TD-034 | P1 | 一部完了(W1-J) | M | Spreadsheet入力が存在しない日付と不正な時分秒をcommandへ変換する |
| TD-035 | P2 | 未着手 | M | 反復延期がDST境界で開始時刻とdeadlineの壁時計時刻をずらす |
| TD-036 | P2 | 未着手 | L | source textを独自parseするarchitecture testがRust構文と実装名へ強く結合している |
| TD-037 | P2 | 完了 | M | 未使用のlenient YAML変換APIがstrict loaderと並存している |
| TD-038 | P2 | 未着手 | L | MCPのtask一覧に検索・paginationがなく、大規模storageで応答が無制限に増える |
| TD-039 | P2 | 未着手 | L | 稼働中processを止めずに整合したbackupを作成・検証・restoreする手段がない |

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
- 半開区間`[start, end)`、23:59-翌00:00、複数日、70日超、論理日境界06:00の扱いを契約として固定する。
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
- 完了日: 2026-08-27
- 対応: 通常commandの統合入口を`handler::handle_command`へ一本化し、privateな`CommandContext`の製品実装、日時解釈、domain mutationを`command_context.rs`へ分離した。tree、task list、calendar、band、focusの表示計算を`view.rs`へ移し、pack、flattenを含む意味的な`DisplayModel`の組み立てをhandlerへ集約した。rendererはそのmodelから既存出力とflushを生成し、`DisplayFragment`と`DisplayRecorder`を削除した。runtimeは依存構築、repository transaction、`Verify`のread-only検査、外部URL起動、interactive/non-interactive調停、focus変更と描画要求の適用、終了code変換だけを担う。
- 実測: TD-018実装開始時に4,892行だった`runtime.rs`は1,377行になった。lib 501 passed、1 ignored、CLI binary 434 passed、MCP binary 2 passed、MCP stdio 12 passed、Spreadsheet 4 passedで、合計953 passed、1 ignoredとなった。ignoredは既存の`benchmark_save_2172project中1件変更を2秒未満で処理する`のみである。
- 品質ゲート: `git diff --check`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`が成功した。製品公開API、command名・alias、CLI文言、YAML、MCP、shell・Apps Scriptを含むSpreadsheet A-J列連携の契約は変更していない。

#### 起票時の現状と根拠

- TD-015によるtest分離前の起票時点では、`src/adapter/controller/schronu/runtime.rs`は11,477行あり、repository transactionと外部I/Oの調停に加えて、command固有helper、日時解釈、domain operationの組み立て、tree・calendar・band・focusなどの表示計算、246件のruntime testとfixtureを保持していた。TD-015完了後のTD-018実装開始時点では4,892行だった。
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
- 完了日: 2026-08-28
- 対応: YAML encodeとその契約testをgatewayへ集約した。repositoryは`TaskHandle`から`TaskSnapshot`を1回だけ取得し、pure encoderがsnapshotからproject YAMLを生成する。entityから`yaml_rust`、`LinkedHashMap`、永続化encoderを除去し、既存format、key順、既定field省略、root限定の`priority` / `category`を維持した。保存bytesとstrict decodeとのround-tripも契約testで固定した。
- 検証: `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`に成功した。testはlib 519件成功、1件ignored、CLI binary 443件、MCP binary 2件、MCP stdio 13件、Spreadsheet 4件が成功し、失敗は0件だった。`rg -n 'yaml_rust|LinkedHashMap|task_to_yaml' src/entity`が0件であることも静的監査した。

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

### TD-010: 現在時刻、UUID、論理日境界がドメイン内部へ埋め込まれている

- 優先度: `P2`
- 概算規模: `L`
- 完了日: 2026-08-21
- 対応: entityのtask生成からsystem clockとUUID生成を除去し、operation固定時刻と注入可能なUUID生成器を持つ`TaskFactory`へ集約した。06:00の論理日境界、logical date、日次終端offset、deadline bufferを`LogicalDateTimePolicy`へ統合し、曖昧・不存在local timeは情報付き`ApplicationError`として伝搬する。CLI、MCP、YAML decodeはoperation入口の同一時刻snapshotをreload、入力既定値、task生成へ共有する。
- 検証: `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功した。testは813件成功、1件ignored、失敗0件(entity/applicationを含むlib 465件、CLI 331件、MCP 2件、MCP stdio 12件、Spreadsheet 3件)。entity production codeの`Local::now()` / `Uuid::new_v4()`と旧暗黙constructorが0件であることも静的監査した。

#### 現状と根拠

- `src/entity/task.rs:898-924`の`TaskAttr::new`が`Local::now()`と`Uuid::new_v4()`を直接呼ぶ。
- `src/entity/task.rs:260-326`の`ImmutableTask`にも現在時刻依存constructorがある。
- `src/application/daily_capacity.rs:109-124`は日付を`Local::now().timezone()`でlocal datetimeへ変換する。
- `src/entity/datetime.rs:12-25`は06:00を直接埋め込み、`LocalResult`を`unwrap`する。
- MCPにもreload時刻や省略時の完了時刻として`Local::now()`があり、呼出し単位で時刻snapshotが統一されない可能性がある。
- deadline bufferの5分・60分や論理日終端offsetは複数層で扱われる。

#### 影響

- constructorとparseが実行時刻によって異なる結果を返し、再現可能なテストとmigrationが難しい。
- 1操作内で日付境界を跨ぐと、異なる`now`が混在し得る。
- timezoneや存在しない・曖昧なlocal timeの扱いが関数ごとに異なる。

#### 推奨する改善方針

- application operationの入口で`now`を1回取得し、entityとgatewayへ明示的に渡す。
- ID生成もapplication境界のfactoryへ置き、productionはUUID v4、testは固定列を使う。
- 06:00境界、logical date start/end、deadline bufferを1つの日時ポリシーへ集約する。
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
- 完了日: 2026-08-29
- 対応: 指定storageをread-only集計し、識別情報と実日付を含まないsmall、typical、stress固定seed fixtureを追加した。通常APIを変えず、schedule、pack、flattenの内部処理をbenchmarking featureで計数する。occupied intervalの二分探索と隣接区間union、packのschedule snapshot再利用、flattenのoverride挿入・復元とcandidate context再利用、依存候補のready heap化により支配的な再計算を削減した。
- 性能契約: 通常CIはtypical/stressの決定論的counter上限を検査する。packはprofile全体に固定small配置・atomic cursor probeをtypicalで1組、stressで4組加える。週次・手動CIはRust 1.97.1、release build、`Asia/Tokyo`、GitHub Actions Ubuntu runnerで3回medianを測り、typical 500ms、stress 5,000msを上限とする。初回ローカルbaseline(Darwin arm64)はtypicalがschedule 6.930ms、pack 8.046ms、flatten 72.900ms、stressがschedule 29.172ms、pack 35.296ms、flatten 403.322msだった。
- 検証: fixtureはtypical 2,213 project・26,378 task・691 active leaf、stress 8,852 project・105,512 task・2,764 active leafを固定する。通常経路と診断経路の結果、task状態、deadline、segment、`PackResult`、`FlattenResult`を照合し、既存schedule契約を緩和していない。`cargo fmt --check`、default/benchmarking双方のClippyと全test、6つのrelease wall-clock gate、`git diff --check`に成功した。

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
- 完了日: 2026-08-29
- 対応: TD-001、TD-010等で既に06:00の論理日境界、5分・60分のdeadline buffer、30分の日次終端offset、28日・35日のflatten範囲をpolicy化し、busy-timeの70日限定展開を解消していた。今回、1日・1400日のproject初期延期、一覧表示の28日・幅70・fallback日付、日次1440分を意味付きpolicyへ集約した。`BusyTimeSlot`をcrate内部APIへ限定して未使用のname保持を除去し、YAMLの`name`必須・文字列validationは維持した。古いcommented code、疑問形コメント、FIXME、task statusを指す`TODO`表記を整理した。CLI、YAML、MCP、Spreadsheetの挙動は変更していない。
- 検証: `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功した。testは982件成功、1件ignored、失敗0件だった。

#### 対応前の現状と根拠

- 06:00の論理日境界、5分のsplit/deadline buffer、30分の日次終端offset、28日・35日のflatten範囲、70日のbusy-time展開、1400日のhobby延期などが複数moduleやcommand branchへ直接埋め込まれる。
- `src/entity/busy_time_slot.rs`の曜日と日次終了時刻は`_`付きfieldとして保持されるが利用されない。
- `src/entity/task.rs:580`付近などに大きなコメントアウト済み実装が残る。
- `src/entity/task.rs:1241`付近の「cloneして大丈夫か?」、`1559`付近の未完了テストコメント、controller内の重複を示すFIXMEなど、設計判断が未確定のまま残る。
- コメント内の`TODO`表記はrepositoryのtask status用語と衝突し、現在のAgent向け規約とも一致しない。

#### 影響

- 同じbusiness ruleを変更しても一部だけ古い値が残る。
- 未使用fieldが将来使う予定なのか、廃止済みなのか判断できない。
- コメントアウトコードが現在の候補実装に見え、reviewと検索の雑音になる。

#### 推奨する改善方針

- 値を単に共通定数へ移すのではなく、論理日、split、deadline、flatten horizonなど意味のあるpolicy単位へ集約する。
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

### TD-019: scheduling性能計測の状態がapplicationの業務ロジックへ伝播している

- 優先度: `P2`
- 概算規模: `L`
- 完了日: 2026-09-02
- 対応: `scheduling_instrumentation`へ中立なevent記録境界を設け、default featureではno-op、`benchmarking` featureだけがthread-local sessionとcounter stateを持つ構成へ置換した。schedule、pack、flattenの通常entrypointからconcrete metricsの生成・引数伝播と`*_with_metrics`/`*_and_metrics`経路を除去し、診断entrypointは各session内で同じ通常algorithmを実行する。公開metrics型はsession側の型を直接再公開し、重複型と旧`scheduling_metrics` moduleを削除した。
- 検証: `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`cargo test --locked --features benchmarking --test scheduling_benchmark_contract`、`git diff --check`に成功した。通常testは失敗0件、benchmark contractは16件成功した。3 use case内のconcrete metrics名と`_with_metrics`/`_and_metrics`が0件であり、3回の責務別subagent reviewとcommit履歴reviewでも残存指摘がないことを確認した。
- 調査revision: `91afef6`

#### 現状と根拠

- concreteな`ScheduleMetrics`、`PackMetrics`、`FlattenMetrics`が、`schedule_use_case.rs`の12関数、`pack_use_case.rs`の4関数、`flatten_use_case.rs`の7関数、合計23関数の引数へ伝播している。
- 3つの通常entrypointも空のmetricsを生成し、`*_with_metrics`または`*_and_metrics`経路を呼んでいる。
- `benchmarking` featureで除外されるのは診断entrypointとcounter更新本体であり、metrics型、関数引数、呼出経路、38か所の計測参照は通常buildにも残る。
- 計測値は判定、戻り値、task変更内容には使用されておらず、現在の業務結果との意味的な結合はない。
- `application/benchmarking.rs`は公開用metrics型と内部用metrics型を重複定義し、診断結果を変換している。
- `benches/`、benchmark fixture、CI、feature限定の診断APIは適切に分離されている。CLIの`RhoMetrics`と保存時間のignored testは製品表示または独立したtestであり、本項目の対象外とする。

#### 影響

- schedulingの業務規則を変更する際にも計測用引数とcounter更新箇所を追従させる必要があり、use caseの可読性と変更局所性を損なう。
- benchmarkの都合で`*_with_metrics`と`*_and_metrics`という内部APIが増え、通常経路と診断経路の対応関係を追いにくい。
- default featureでも不要な計測stateを生成して渡す構造になり、最適化による除去を前提にしている。
- 計測経路だけを分離しようとしてalgorithmを複製すると、通常経路とbenchmark結果が乖離する危険がある。

#### 推奨する改善方針

- concreteなbenchmark metricsを業務関数の引数から除去し、scheduling実行contextと中立な非公開instrumentation境界を設ける。
- 通常経路は結果へ影響しないno-op実装を使い、`benchmarking` featureだけが計数実装と診断用metrics型を提供する。
- 診断経路は通常経路と同じalgorithmを通し、benchmark専用algorithmを複製しない。
- schedule、pack、flattenを契約単位に分け、通常経路と診断経路の同値性を固定してから段階的に境界を置換する。

#### 完了条件

- schedule、pack、flattenのuse caseがconcreteなbenchmark metricsをimportまたは生成しない。
- `_with_metrics`または`_and_metrics`という計測都合の並行経路が残らない。
- 公開API、task順序、segment、deadline判定、`PackResult`、`FlattenResult`、反映後のtask変更内容が置換前後で一致する。
- TD-012で導入した決定論的counter契約とwall-clock gateが維持される。
- default featureの製品buildへ計数用stateを含めない。

#### 依存関係

- TD-012で性能契約と診断経路が固定された後の負債として扱う。
- 今後のscheduling algorithm変更より先に計測境界を整理し、境界変更とalgorithm変更を同じcommitへ混ぜない。

### TD-020: 同日・同名またはsanitize後に同名となるprojectが同じ保存先を共有し、再読込時に1件消失する

- 分類: `バグ / データ保全`
- 優先度: `P0`
- 概算規模: `M`
- 完了日: 2026-09-04
- 対応: 新規project directoryを`YYYYMMDD-{sanitize済みproject名}-{root UUID}`形式にして完全なUUIDをidentityとし、長い表示名はUTF-8境界でcomponent上限内へ短縮した。登録前に既存task UUIDと保存先pathをtyped errorで拒否し、load時はcanonical pathと実際に開くfileを同じtargetへ固定した上で、同一実体の重複を両path付きerrorにする。旧形式directoryはrenameせず読み取り・再保存できる互換性を維持した。
- 検証: 同名、`a/b`と`a-b`、URL除去衝突、長いUTF-8名、旧形式非migration、登録失敗原子性、canonical path重複、symlink差し替え後のfile identityを製品repository経路で確認した。`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功し、libraryは600件成功・1件ignore、CLIは446件、MCP binaryは2件、stdio integrationは13件、fixtureは5件成功・1件ignore、Spreadsheetは4件成功した。責務別3回とbranch全体1回のsubagent reviewを行い、2件のP2指摘を個別commitで修正後、コードと履歴に残存指摘がないことを確認した。

#### 現状と根拠

- `src/application/task_use_case.rs:298-333`はproject名や保存先の重複を確認せず、正常なcreate操作として`start_new_project`を呼ぶ。
- `src/adapter/gateway/task_repository.rs:743-755`は保存directoryを`YYYYMMDD-{project_name_for_dir}`だけから生成する。root UUIDはpathへ含まれない。
- 同箇所はURL以降を除去し、`/`を`-`へ置換するため、完全な同名だけでなく`a/b`と`a-b`、異なるURL suffixを持つ名前も同じpathになり得る。
- `src/adapter/gateway/task_repository.rs:598-602`は各projectを順番に同じ`project.yaml`へatomic renameし、path重複を検出しない。memoryでは2件ともclean扱いになるが、diskには最後の1件しか残らない。
- 既存testは「新規projectのdirectoryを作る」「start時点ではfilesystemを変更しない」を固定するが、同一repository内のpath一意性とsave-load後の件数を検証していない。

#### 影響

- 正常なCLI/MCP入力だけで先に作成したprojectが失われる。次回process起動までmemory上では2件見えるため、消失の発見も遅れる。
- sanitize規則を変更すると新旧のpath衝突条件が変わり、migrationなしでは別の上書きを作り得る。

#### 推奨する改善方針

- 保存directoryのidentityへroot task UUIDを含める。表示用の名前部分は可読性の補助とし、一意性を担わせない。
- 既存directory形式は読み取り互換を維持し、新規作成分だけ新形式を使う。load時にはcanonicalized path重複も拒否する。
- `start_new_project`はmemoryへ追加する前に、既存project pathとUUIDの双方を検査し、失敗時にrepositoryを変更しない。

#### 完了条件

- 同日・同名projectを2件作成し、save-load後も異なるUUIDの2件が残る。
- `a/b`と`a-b`、URL除去後に同名となる名前でも衝突しない。
- 旧directory名の既存storageをrenameせず読み込める。
- 新形式の命名規則と互換方針をREADMEへ記載する。

#### 推奨commit分割

1. `Test: project保存先の一意性契約を固定する`: 同名・sanitize衝突のsave-load Red testだけを追加する。
2. `Repository: project保存先へidentityを付与する`: path割当と重複拒否を最小実装し、全品質ゲートを通す。
3. `Docs: project directory互換規則を記載する`: READMEだけを更新する。

### TD-021: repositoryが重複UUIDを受理し、ID指定操作の対象が走査順に依存する

- 分類: `バグ / データ整合性`
- 優先度: `P1`
- 概算規模: `M`

#### 現状と根拠

- strict YAML decodeは`src/adapter/gateway/yaml.rs:344-353`で各UUIDの形式だけを検査し、tree内・project間の一意性を検査しない。
- `src/adapter/gateway/task_repository.rs:263-270`は`HashMap::insert`の置換結果を無視する。重複IDがあると後からcacheしたtaskが先のentryを上書きする。
- `src/adapter/gateway/task_repository.rs:483-493`は全projectをcacheした後、そのままload成功とする。`get_by_id`を使うCLI、MCP、Spreadsheetの対象はfile名と走査順へ依存する。
- `src/application/task_use_case_tests.rs:1155-1160`には同一UUIDのsiblingを作るfixtureがあるが、repositoryのload/lookup一意性は検証していない。
- READMEはtask IDを一意なUUIDとして外部連携キーにしている。

#### 影響

- ID指定の完了・延期・更新が別taskへ適用され、保存後に意図したtaskと異なるデータが確定する。
- `検証`commandが成功してもUUID一意性は保証されない。

#### 推奨する改善方針

- load中に`UUID -> project.yaml path + task path`を構築し、重複時は最初と2件目の両位置を持つtyped validation errorで全loadを失敗させる。
- cache構築を一時mapで完了してからrepository stateへcommitし、失敗時のmemory原子性を維持する。
- `TaskFactory`のID生成器が衝突値を返した場合も、新規project/child追加を拒否する。

#### 完了条件

- 同一tree内と別project間の重複UUIDが、両方のfile/task pathを含むerrorになる。
- load失敗時に既存projects、cache、revision、clockを変更しない。
- `検証`commandも同じvalidatorを通る。
- CLI/MCP/Spreadsheetの外部ID契約を変更しない。

#### 推奨commit分割

1. `Test: repository UUID一意性を固定する`: tree内、project間、memory原子性のRed testを追加する。
2. `Repository: 重複UUIDをpath付きで拒否する`: 一時indexとtyped errorを実装する。
3. `Test: 検証commandへUUID一意性を通す`: 製品経路のcontract testを追加してGreenにする。

### TD-022: 複数project保存でrevisionだけが先行し、失敗時にdisk snapshotが部分更新される

- 分類: `技術的負債 / 障害回復性`
- 優先度: `P1`
- 概算規模: `XL`

#### 現状と根拠

- `src/adapter/gateway/task_repository.rs:523-575`は変更projectをserializeし、directoryを準備する。
- 同ファイル`576-602`は`.revision`を先にatomic更新し、その後で各`project.yaml`を個別に置換する。2件目以降のwrite失敗時は、revisionと先行projectだけが新しく、残りは古いsnapshotになる。
- `src/adapter/gateway/task_repository_tests.rs:890-914`は「project失敗時はdisk revisionだけを先に進める」ことを明示的にGreen契約として固定している。
- READMEは複数projectをまたぐatomic transactionを初版対象外とし、save失敗後のMCP process再起動を要求する。しかし再起動してもdisk上の部分更新自体はrollbackされない。

#### 影響

- `flatten`、`pack`、反復task完了など複数projectを変更する操作が、障害時に業務上ひとまとまりでないsnapshotを残す。
- revisionが新しいため別processは部分更新済みsnapshotを正規の最新版としてloadする。
- `StateUncertain`はmemory継続を防ぐだけで、disk整合性を回復しない。

#### 推奨する改善方針

- storage直下にtransaction staging directoryとmanifestを作り、全projectのtemporary fileをwrite+syncした後にcommit markerを切り替える。
- 起動時に未完了transactionを検出し、旧snapshotへ戻すかcommitを完了するrecovery protocolを定義する。
- directory fsync、renameの同一filesystem制約、削除project、permission維持、crash pointを明示する。単なるrevision更新順の後置だけでは、project間atomicityを満たさない。

#### 完了条件

- prepare中、1件目rename後、最終rename前、marker切替前後の各failure injectionで、再起動後に旧snapshotまたは新snapshotのどちらか一方だけを読む。
- revisionと全project内容が同じtransaction IDへ対応する。
- temporary/staging fileが通常load対象にならず、recovery後に残骸を安全に除去できる。
- 単一projectの「同一内容なら書かない」最適化とpermission維持を保つ。

#### 推奨commit分割

1. `Test: 複数project saveのcrash contractを固定する`: failure point別のRed integration testを追加する。
2. `Repository: save staging phaseを導入する`: 挙動を変えずprepare境界を分離する。
3. `Repository: snapshot commit markerを導入する`: recovery可能なcommit protocolを実装する。
4. `Repository: 未完了saveを起動時に回復する`: recoveryとcleanupを独立実装する。
5. `Docs: repository recovery protocolを記載する`: READMEを更新する。

### TD-023: `終`が不正時刻と一部application errorを成功扱いで握り潰す

- 分類: `バグ / エラー契約`
- 優先度: `P1`
- 概算規模: `S`
- 完了日: 2026-09-04
- 対応: `終`の不正な完了時刻を`finished_at`付きの`CommandParseError`へ変換し、`HasUndoneChildren`だけは既存のtree表示へfallbackしつつ、その他の`ApplicationError`はvariantを保持してruntimeへ伝搬するようにした。error時はcompletion後のfocus更新を行わず、対話CLIでは診断を表示して既存focus状態を維持する。
- 検証: 構文不正、不正な秒、存在しない日付、task不明、実績加算overflow、tree errorのhandler contract testを追加した。`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功し、サブエージェントreviewでも指摘がないことを確認した。
- 関連既存項目: TD-006の完了条件に対する残存不具合。

#### 現状と根拠

- `src/adapter/controller/schronu/handler.rs:733-771`で、`decide_finish_time_values`が`None`を返す不正時刻は空の成功outcomeになる。
- 同ファイル`761-770`は`HasUndoneChildren`だけをtree表示へ変換し、それ以外の`ApplicationError`を`Err(_) => {}`で破棄する。
- `src/adapter/controller/schronu/handler_contract_tests.rs:1893-1928`は、不正時刻とその他completion errorを「handled no-op」とする誤挙動を明示的に固定している。
- READMEは不正commandと操作拒否を診断し、非対話では非0終了・未保存にすると説明している。

#### 影響

- taskが完了していないのにCLIは成功終了し、automationや利用者が完了済みと誤認する。
- Spreadsheetから生成された不正な`終`commandでも、実績更新だけが先に行われる運用事故へつながる。

#### 推奨する改善方針

- 不正時刻をfield付き`CommandParseError`または`ApplicationError::InvalidInput`へ変換する。
- `HasUndoneChildren`の既存tree表示だけを明示branchとして残し、その他のerrorは`?`でruntimeへ伝搬する。

#### 完了条件

- 不正時刻、task不明、算術overflow、tree errorが診断され、非対話実行は非0で終了する。
- error時にfocus、task snapshot、mutation revision、diskを変更しない。
- `HasUndoneChildren`のtree表示契約は維持する。

#### 推奨commit分割

1. `Test: 終commandのerror伝搬を固定する`: 現行no-op assertionをREADME契約に沿うRed testへ置換する。
2. `CLI: 終commandのerrorを保持する`: handlerの最小修正でGreenにする。

### TD-024: CLI parserが不正な数値や余分な引数を黙って受理し、更新commandを実行する

- 分類: `バグ / 入力検証`
- 優先度: `P1`
- 概算規模: `M`
- 完了日: 2026-09-04
- 対応: command定義へcanonical name、usage、最小・最大argument数を集約し、typed fieldの変換前に全既知commandのarityを検証するようにした。`extrude`はargument省略時だけ既存動作を維持し、不正値と`u16`範囲外をfield付きerrorにする。`arrange`の任意flagは`全`または`all`だけを受理する。parse errorはbusy timeとrepositoryの読込、task変更、保存より先に返す。
- 検証: `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked -q`、`git diff --check`に成功した。通常testは失敗0件で、全commandのarity table、alias、省略時既定値、canonical error、非対話runtimeの未変更・未保存契約を確認した。契約単位のspec reviewとcode quality review、最終実装・commit履歴reviewでも残存指摘がないことを確認した。
- 関連既存項目: TD-006の完了条件に対する残存不具合。

#### 現状と根拠

- `src/adapter/controller/schronu/command.rs:649`付近は`extrude invalid`の数値変換失敗を既定の1日へfallbackする。
- `src/adapter/controller/schronu/command_contract_tests.rs:278`付近はこのsilent fallbackを現在の契約として固定している。
- `src/adapter/controller/schronu/command.rs:698`付近の引数なしcommand群は余分なargumentを検査せず捨てるため、`flatten extra`や`pack extra`が実行され得る。
- READMEは不正入力時に状態変更もsaveもしないと明記する。

#### 影響

- typoが意図しない延期・平坦化・前倒しとして実行・保存される。
- commandごとにarity検証方法が異なり、新command追加時に同じsilent fallbackを再発させやすい。

#### 推奨する改善方針

- `CommandAction`定義に最小・最大argument数とfield parseを集約し、未知・余分・型違いを統一`CommandParseError`にする。
- shorthandの既定値は「argument省略」の場合だけ適用し、「argumentはあるが不正」と区別する。

#### 完了条件

- `extrude invalid`、`flatten extra`、`pack extra`、引数なしcommandへの任意の余分な値を拒否し、保存しない。
- 全commandの最小・最大argument数をtable-driven testで固定する。
- 正常なaliasと省略時既定値は維持する。

#### 推奨commit分割

1. `Test: CLI command arityを固定する`: command別のRed table testを追加する。
2. `CLI: 不正argumentのsilent fallbackを除去する`: parserをGreenにする。
3. `Test: 不正argumentでrepositoryを保存しない`: runtime製品経路を固定する。

### TD-025: 対話CLIのterminal I/O失敗がpanicまたは未検査結果になる

- 分類: `バグ / 障害回復性`
- 優先度: `P1`
- 概算規模: `M`
- 関連既存項目: TD-006は非対話command出力のerror捕捉を完了したが、interactive driverに同じ契約が届いていない。

#### 現状と根拠

- `src/adapter/controller/schronu/interactive.rs:163-209`はprompt描画、flush、raw mode移行、cursor設定を`unwrap()`する。
- 同ファイル`246-378`もerror表示、cursor移動、文字出力、終了描画を多数の`unwrap()`と直接`println!`で処理する。
- `src/adapter/controller/schronu/runtime.rs:1`はmodule全体へ`allow(unused_must_use)`を付け、interactive描画helperの戻り値を一部未検査にしている。
- READMEはBrokenPipeを正常な出力終了として扱うと説明するが、interactive経路ではpanicし得る。

#### 影響

- terminal切断、pipe終了、raw mode移行失敗がbacktrace付きpanicになり、`RunError`の診断・終了code契約を迂回する。
- raw mode復元前のpanicはterminal状態を壊す可能性がある。

#### 推奨する改善方針

- interactive driverを`Result<_, InteractiveIoError>`にし、描画・cursor・flush・raw mode初期化をすべてfallibleにする。
- raw terminalをRAII guardで管理し、途中errorでも復元する。
- BrokenPipe分類は非対話の`captured_output_result`と共通化し、module levelの`allow(unused_must_use)`を除去する。

#### 完了条件

- prompt、refresh、cursor、error表示、終了描画、raw mode移行の各failure injectionでpanicしない。
- BrokenPipeは正常終了、それ以外はsource付き出力errorとして非0終了する。
- error後もterminal復元処理が走り、task save方針が明示される。

#### 推奨commit分割

1. `Test: interactive I/O failure contractを固定する`: failure writerとraw mode failureのRed testを追加する。
2. `CLI: interactive driverをfallibleにする`: I/O result伝搬を実装する。
3. `CLI: raw terminal lifecycleをguardへ移す`: lifecycleだけを独立変更する。

### TD-026: task名をCLI・YAML・MCP・Spreadsheet間で安全にround-tripできない

- 分類: `バグ / 境界契約`
- 優先度: `P1`
- 概算規模: `L`

#### 現状と根拠

- `tests/fixtures/spreadsheet/generated-commands.txt`は空白を含む`新 新規 タスク`を期待する一方、`src/adapter/controller/schronu/command.rs:483`付近は第1 tokenをname、第2 tokenを見積分として解釈する。生成済みcommandを実際のCLI parserへ通すtestがない。
- `src/application/task_use_case.rs:580-594`のname validationはblankと整数だけを拒否し、tab、改行、ANSI escape、その他control characterを許可する。
- `src/adapter/gateway/yaml.rs:337-343`もblankだけを拒否する。MCP JSONからはCLIで入力できない制御文字を渡せる。
- `src/adapter/controller/schronu/renderer.rs:24-37`はtask名をSpreadsheet行へ未escapeで連結するため、tab/改行は列・行構造を壊す。repository directory名にも未定義のcontrol characterが入り得る。

#### 影響

- Spreadsheetで空白入りtaskを仮登録しても生成commandを実行できない。
- control characterを含む名前がterminal表示を偽装し、Spreadsheetの列ずれやcommand injectionに似た誤操作を起こす。
- YAMLでは保存できるがCLIでは再現・修正できない値が生まれる。

#### 推奨する改善方針

- task名のcanonical contractをapplication層で定義する。少なくともNUL、改行、tab、terminal controlは拒否または明示escapeする。
- CLIに引用符・backslashを扱う単一lexerを導入し、Spreadsheet generatorも同じescape規則で名前を出力する。
- 永続YAMLの既存名を検査するdry-runを用意し、厳格化前に互換性を確認する。

#### 完了条件

- 空白、日本語、引用符、backslashを含む許可名がSpreadsheet生成からCLI parseまで同一文字列でround-tripする。
- tab、改行、ESC、NULの扱いがCLI/MCP/YAMLで一致し、Spreadsheetの行列構造を壊さない。
- 既存不正名にはfile/task path付き診断が出る。
- A-J列とshell取込を変更する場合、AGENTS.mdが列挙する全連携箇所を同時確認する。

#### 推奨commit分割

1. `Test: task名の境界契約を固定する`: application validationとCLI lexerのRed testを追加する。
2. `CLI: 引用可能なtask名lexerを導入する`: parserだけをGreenにする。
3. `Spreadsheet: task名をCLI形式へescapeする`: shell fixtureとend-to-end testを更新する。
4. `Repository: control character名を診断する`: strict loaderと検証commandを更新する。

### TD-027: 残作業時間の補正計算が合法な大値入力で整数overflowする

- 分類: `バグ / 算術安全性`
- 優先度: `P1`
- 概算規模: `S`
- 完了日: 2026-09-04
- 対応: 残作業補正の減算と倍化をchecked演算へ置換し、中間値が`i64`で表現不能な場合はtask ID、見積秒、実績秒を保持する`RemainingWorkCalculationOverflow`を返すようにした。通常の残作業規則とpack・flattenへのerror伝搬は維持した。
- 検証: `i64::MAX`近傍かつ60の倍数の見積とその1秒超過の実績による境界testをdebug・release双方で実行し、同一errorとtask view・mutation revision・save回数の不変を確認した。`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功し、subagent reviewの指摘は0件だった。

#### 現状と根拠

- `src/application/schedule_use_case.rs:327`付近の`calculate_remaining_work_seconds`は、実績が見積もりを超えた場合に`estimated_work_seconds * 2 - actual_work_seconds`を未検査で計算する。
- CLI/MCPの見積分は秒変換時にoverflowを検査するが、`i64::MAX`近傍までの非負値は型上有効である。実績もchecked add後の大値を保持できる。
- 見積が`i64::MAX / 2`を超え、実績が見積を少し超えた入力では、debug/test buildはpanicし、release buildはwrapする。

#### 影響

- 同じstorageでもbuild profileによりpanicまたは誤ったscheduleになる。
- `get_schedule`だけでなく、その結果を使う`pack`、`flatten`へ波及する。

#### 推奨する改善方針

- 数式をchecked演算で表現し、表現不能時はfieldと値を保持するtyped errorにする。saturatingを採る場合は業務上の意味を先に契約化する。
- estimated/actual secondsの上限をdomain invariantとして制限する案も比較し、YAML/MCP/CLIで同じ上限を使う。

#### 完了条件

- `E=i64::MAX`近傍の60の倍数、`A=E+1`でもpanicせず、debug/releaseで同じ結果または同じerrorになる。
- 通常の「見積60分、実績90分なら残30分」を維持する。
- error時にschedule候補やtask stateを変更しない。

#### 推奨commit分割

1. `Test: 残作業補正のoverflow契約を固定する`: boundary Red testを追加する。
2. `Schedule: 残作業補正をchecked演算にする`: 最小実装でGreenにする。

### TD-028: 論理日境界を跨ぐschedule segmentの容量が開始日に全量計上される

- 分類: `バグ / schedule正確性`
- 優先度: `P1`
- 概算規模: `M`
- 完了日: 2026-09-04
- 対応: schedule segmentと06:00境界の交差区間を時系列で返すapplication共通helperを追加した。fixedは予約windowの実時間、flexibleは作業秒を交差時間比で配賦し、整数除算の丸め差を最後の区間へ集約して総量を保存する。packの通常・反復容量とflattenの使用量・延期候補日を同じhelperへ移し、開始日の翌日にだけ過負荷があるsegmentも未解消理由と代表taskへ関連付けるようにした。
- 検証: 05:30-06:30、複数日、fixed、flexible、分割済みsegment、丸め差、zero容量、日時範囲errorの単体契約と、pack・flatten・CLI製品経路のRed/Greenを確認した。`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功し、main取り込み後の通常testは601件+449件ほか失敗0件だった。3回の責務別subagent reviewと最終履歴reviewの指摘を個別commitで解消した。

#### 現状と根拠

- `src/application/pack_use_case.rs:346`付近の日次集計は`scheduled_start`の論理日を1つだけ求め、segment全体の容量をその日に加算する。
- `src/application/flatten_use_case.rs:520`付近も同じ集計方法を持つ。
- `src/application/scheduled_capacity.rs:8`付近はfixedなら予約window全体、flexibleならsegment全作業秒を返すが、06:00の論理日境界で分割しない。
- 05:30-06:30の1時間segmentは、前日30分・翌日30分ではなく、開始側の論理日へ60分すべて計上される。

#### 影響

- 前日の過負荷を過大評価し、翌日の過負荷を見落とす。
- `flatten`が誤った日から延期し、`pack`が翌日の余差へtaskを詰めすぎる。

#### 推奨する改善方針

- schedule segmentと各論理日区間のintersectionを返す共通helperをapplication層へ置き、packとflattenで共有する。
- fixedはintersectionの実時間、flexibleはsegmentの実作業秒を各区間へ欠損なく配賦する。丸め差は最後の区間へ集約するなど決定規則を定める。

#### 完了条件

- 05:30-06:30を30分ずつ別論理日へ計上する。
- 2日以上、fixed、flexible、既に分割済みのsegmentで、日別合計の総和が元segment容量と一致する。
- packとflattenが同じhelperを使い、同一fixtureの日別容量が一致する。

#### 推奨commit分割

1. `Test: schedule容量の日跨ぎ配賦を固定する`: 共通helperのRed contractを追加する。
2. `Application: segmentを論理日別に配賦する`: helperを実装する。
3. `Pack: 日別容量集計を共通helperへ移す`: packだけをGreenにする。
4. `Flatten: 日別容量集計を共通helperへ移す`: flattenだけをGreenにする。

### TD-029: 反復task完了の後段失敗で完了状態と親見積もりだけが部分更新される

- 分類: `バグ / 失敗原子性`
- 優先度: `P1`
- 概算規模: `L`

#### 現状と根拠

- `src/application/task_use_case.rs:500`付近の`complete_task`は、対象taskの実績、status、完了時刻を更新した後に次回反復taskを生成する。
- `src/application/task_use_case.rs:679`付近の`create_prepared_repetition_task`は、親見積もりを先に更新し、その後`create_child`する。
- child insertやhierarchy grant、borrowが後段で失敗すると、対象taskはDone、親見積もりは変更済みだが次回taskがない状態で`Err`になる。
- transaction層はsaveを抑止するが、同一processのmemoryをrollbackしない。既存testは準備段階のoverflow等を検証するが、最初のwrite後にchild生成を失敗させる契約がない。

#### 影響

- 反復taskが途切れ、再試行で実績・親見積もりが重複更新され得る。
- CLIは処理失敗を表示しても、その後のinteractive操作が部分更新状態を観測する。

#### 推奨する改善方針

- 対象更新、親見積もり更新、次回child追加を1つのdomain operationとして、変更計画の構築と全borrow/hierarchy事前検証後にcommitする。
- rollbackより「最初のwrite前に失敗条件を解消する」方式を優先し、成功時のmutation revision増分も明示する。

#### 完了条件

- hierarchy edit禁止、borrow競合、insert失敗を注入しても、対象snapshot、親見積もり、children、focus、mutation revisionがすべて不変。
- 成功時は現在の完了値と次回反復taskの契約を維持する。
- CLI/MCP双方が同じapplication operationを通る。

#### 推奨commit分割

1. `Test: 反復完了の後段失敗原子性を固定する`: failure injection Red testを追加する。
2. `Task: 反復完了の変更計画を導入する`: domain側のprepareを実装する。
3. `Application: 反復完了を一括commitする`: use caseを新operationへ移してGreenにする。

### TD-030: 00:00以降の日次残容量計算がbusy timeを無視する

- 分類: `バグ / 日次容量`
- 優先度: `P1`
- 概算規模: `S`
- 関連既存項目: TD-001はFreeTimeManager自体の日跨ぎを修正したが、application側がmanagerを呼ばない経路が残る。

#### 現状と根拠

- `src/application/daily_capacity.rs:62-71`は、現在時刻が06:00未満かつ日次終端より前の場合、`FreeTimeManagerTrait::get_free_minutes`を呼ばず、単純な`eod - last_synced_time`を自由時間として返す。
- 既定の日次終端は翌00:30である。00:10-00:30がweekly busy slotでも、00:10実行時には0分でなく20分の自由時間になる。
- `end_of_day_offset_minutes`を大きくすると、busy timeを無視する時間帯も広がる。

#### 影響

- 深夜実行時にpackの余差とflattenの日次容量を過大評価し、過負荷を見落とす。
- 同じ区間をFreeTimeManagerへ直接照会した結果とdaily capacity結果が一致しない。

#### 推奨する改善方針

- `last_synced_time < eod`なら時刻帯にかかわらず`get_free_minutes(last_synced_time, eod)`を使う。
- EOD後は0、対象日が現在の論理日でなければ全日計算、という条件へ単純化する。

#### 完了条件

- 00:10-00:30を全てbusyにした場合は残容量0分、一部busyならその分だけ控除する。
- EODちょうど、EOD後、05:59、06:00、正負のEOD offsetを固定する。
- daily capacity、pack、flattenの製品経路で同じ結果になる。

#### 推奨commit分割

1. `Test: 深夜の日次容量へbusy timeを反映する`: recording fakeを使うRed testを追加する。
2. `Capacity: 深夜もfree time managerを照会する`: 条件分岐を最小修正する。

### TD-031: Spreadsheet変換がrank 1000以降のtask行を黙って破棄する

- 分類: `バグ / Spreadsheet export`
- 優先度: `P1`
- 概算規模: `S`

#### 現状と根拠

- `shell/copy_for_spreadsheet.sh:9`は`/^0/`で始まる行だけをtask行として処理する。
- `src/adapter/controller/schronu/renderer.rs:968`付近のrank表示は`format!("{:04}", rank)`という最小幅であり、rank 999は`0999`、rank 1000は`1000`になる。
- 実行確認では`0999`は変換され、`1000`と`10000`は0行になった。errorやwarningは出ない。
- `tests/spreadsheet_contract.rs:116`付近のfixtureはrank `0000`と`0001`だけで境界を覆わない。

#### 影響

- 1001件以上を表示する大規模storageで、Spreadsheetへ貼るtaskが途中から黙って欠落する。
- 欠落後もpadding行が出るため、空きtaskとして気付きにくい。

#### 推奨する改善方針

- 行頭文字でなく、rank、UUID、scheduled timeなどA-J列のtask row grammarを検査する。
- より堅牢にはCLIへ`--format tsv`等のmachine-readable出力を追加し、human displayの見た目へshellを依存させない。

#### 完了条件

- `0999`、`1000`、`10000`をすべて保持し、見出し・集計・warning行は除外する。
- 不完全なtask行は黙ってskipせず、line番号付きerrorにする。
- 既存A-J列順とtask名中の空白を維持する。

#### 推奨commit分割

1. `Test: Spreadsheet rank上限境界を固定する`: 999/1000/10000のRed fixtureを追加する。
2. `Spreadsheet: task行判定を列grammarへ変更する`: shellだけをGreenにする。

### TD-032: macOS標準環境でSpreadsheet変換の`tac`依存が空出力の成功になる

- 分類: `バグ / portability`
- 優先度: `P1`
- 概算規模: `S`
- 完了日: 2026-09-04
- 対応: task行の逆順処理をPOSIX AWK内の配列へ統合し、GNU `tac`依存を除去した。`pipefail`とpipeline出力の一時保持を導入し、前段command失敗と必須command欠落で非0終了しつつtask行とpaddingを公開しないようにした。`yes | head`は`pipefail`下のSIGPIPEを避けるためzsh builtinの50回loopへ置換した。
- 検証: `PATH=/usr/bin:/bin`の正常変換、途中失敗するfake AWK、必須command欠落を製品script経路で確認する専用test 3件と既存Spreadsheet contract 4件に成功した。`/bin/zsh -n shell/copy_for_spreadsheet.sh`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功し、旧新出力のbyte一致とsubagent reviewで追加指摘がないことを確認した。

#### 現状と根拠

- READMEはmacOSを正式対応対象とするが、`shell/copy_for_spreadsheet.sh:31`はmacOS標準にないGNU系`tac`を使う。
- 同scriptは`set -ue`だけで`pipefail`を設定しない。`PATH=/usr/bin:/bin`で`tac: command not found`になっても、後段のwhile/paddingが終了code 0を返し得る。
- CIはUbuntuだけで、`tests/spreadsheet_contract.rs`も通常PATHを使うため、開発機にHomebrew `tac`があると欠落を検出しない。

#### 影響

- documented workflowが空のtask dataを正常結果としてclipboardへ渡す。
- 必須commandの存在が非明示で、環境差がdata欠落に直結する。

#### 推奨する改善方針

- 逆順処理をAWK内へ統合するか、macOS標準のportableな実装へ置換する。
- `set -o pipefail`を追加し、必須command欠落や前段failureを非0で伝搬する。

#### 完了条件

- HomebrewなしのmacOS標準PATHで正常fixtureを変換できる。
- 前段command failureと必須command欠落が非0終了し、task/paddingを一切出さない。
- CIにportable PATHを固定したcontract testがある。

#### 推奨commit分割

1. `Test: Spreadsheet変換をmacOS標準PATHで固定する`: 現在失敗するtestを追加する。
2. `Spreadsheet: tac依存をportableな逆順処理へ置換する`: scriptをGreenにする。

### TD-033: 同一taskの複数segmentをApps Scriptが別行へ同期する

- 分類: `バグ / Spreadsheet同期`
- 優先度: `P1`
- 概算規模: `M`

#### 現状と根拠

- scheduleは1taskを複数segmentへ分割でき、Spreadsheet fixtureとREADMEも同一task IDの複数行を扱う。
- `apps_script/main.js:87-107`は編集行のtask IDだけを取得して同期先を探す。
- `apps_script/main.js:111-128`の`findRowByTaskId_`は最初の一致行を即座に返す。2番目以降のsegmentを編集しても、相手sheetの先頭segmentへL/N/P/R値を書き込む。
- `tests/spreadsheet_contract.rs:205`以降はApps Scriptの定数と文書文字列を確認するだけで、関数挙動を実行しない。
- `apps_script/README.md`はL/N/P/Rを同期すると説明する一方、`applyTimeFormat`実装は日付保持のため`実ログ`を対象外にしており、時刻formatの説明にも軽微な乖離がある。

#### 影響

- segment別の開始・完了時刻が別segmentへ転記され、実績command生成と表示順が壊れる。
- 同期先が見つかったためerrorにならず、誤同期を利用者が見落とす。

#### 推奨する改善方針

- row identityを`task_id`だけでなく予定segmentを識別する複合keyにする。既存列で一意にできない場合は明示的なsegment ID列を追加する。
- L/Pのsegment固有値とN/Rのtask全体値を分け、同一taskの全行へ同期すべき列と対応segmentだけへ同期すべき列を明文化する。
- duplicate/ambiguous keyは先頭を選ばず診断する。

#### 完了条件

- 同じtask IDを持つ2行以上を両sheetに置き、各segmentのL/P編集が対応segmentだけへ反映される。
- N/Rの同期単位がtestと文書で一致する。
- key欠落・重複時にsilent returnせず、利用者が確認できる診断または再試行情報を残す。
- 列追加時は`spreadsheet_columns.tsv`、CLI、両shell、Apps Script、両READMEを同時更新する。

#### 推奨commit分割

1. `Test: Spreadsheet segment同期の契約を固定する`: Apps Script関数を実行するRed testを追加する。
2. `Spreadsheet: segment identityを定義する`: manifestとfixtureだけを更新する。
3. `Apps Script: segment単位の同期へ変更する`: 実装をGreenにする。
4. `Docs: sheet別の同期と時刻formatを訂正する`: 文書だけを更新する。

### TD-034: Spreadsheet入力が存在しない日付と不正な時分秒をcommandへ変換する

- 分類: `バグ / Spreadsheet import`
- 優先度: `P1`
- 概算規模: `M`
- W1-J完了日: 2026-09-04
- 対応: S列のminute/secondを00-59へ限定し、P列をGregorian暦の実在日まで検証するようにした。専用import contract testで、入力全体の検証が成功するまでstdoutへcommandを出さない契約を固定した。
- 検証: `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功した。専用import contract test 5件と既存Spreadsheet contract test 4件が成功し、仕様・code quality reviewの指摘を解消した。
- 残存: 正常生成commandをCLI parserへ通すcross-boundary contract testはWave 2へ残す。このintegration gateがGreenになるまでTD-034全体は完了扱いにしない。

#### 着手時の現状と根拠

- `shell/generate_command_from_spreadsheet.sh:37-59`の完了日時parseは月1-12、日1-31しか検査せず、`2026/02/31`や非閏年の`02/29`を受理する。
- 同script`21-28`の実作業時間parseは形だけを見て、minute/secondが00-59であることを検査しない。`0:99:99`を99分として受理する。
- 実行確認では`2026/02/31 9:10:00`から`終 9:10:00 2026/02/31`、`0:99:99`から`働 99`を生成した。
- 正常fixtureの文字列比較だけで、異常時にoutputが完全に空か、生成commandをCLIが受理するかを検証していない。

#### 影響

- `働`だけが反映され、`終`はTD-023のsilent no-opになるなど、1回のSpreadsheet取込が部分適用される。
- 行番号・列名の段階で検出できる入力誤りが、後段CLIの別errorとして現れる。

#### 推奨する改善方針

- P列は実在する暦日まで検査し、S列はminute/secondを00-59へ制限する。
- 中期的にはSpreadsheet importをRustのtyped境界へ移し、chronoとCLI parserを再利用する。shellで維持する場合も全行validationを先に完了し、1件でも不正ならcommandを1行も出さない。

#### 完了条件

- `2026/02/31`、非閏年`02/29`、`0:60:00`、`0:00:60`を列名・line番号付きで拒否する。
- 正しい閏日、`23:59:59`、24時間超のhour表現という現行契約を明示して受理する。
- error時はstdoutが空で、task単位の部分commandを出さない。
- 正常生成commandを実際のCLI parserへ通すend-to-end testがある。

#### 推奨commit分割

1. `Test: Spreadsheet日時検証を固定する`: calendar/time境界のRed testを追加する。
2. `Spreadsheet: P列とS列を厳密検証する`: shellをGreenにする。
3. `Test: Spreadsheet生成commandをCLI parserへ接続する`: cross-boundary contractを追加する。

### TD-035: 反復延期がDST境界で開始時刻とdeadlineの壁時計時刻をずらす

- 分類: `バグ / timezone`
- 優先度: `P2`
- 概算規模: `M`
- 関連既存項目: TD-010はlocal datetime変換errorを統一したが、反復間隔をelapsed durationとして扱う経路が残る。

#### 現状と根拠

- `src/application/task_use_case.rs:425`付近の親deadlineなし経路は`orig_deadline + Duration::days(interval)`とし、暦日でなく24時間単位を加算する。
- 親deadlineあり経路は新deadlineを暦日で構築した後、`new_deadline - orig_deadline`の`num_days()`を開始時刻へ適用する。
- DST開始週の7暦日は6日23時間なので、前者は壁時計時刻が1時間ずれ、後者は`num_days()`切り捨てで開始日が1日不足し得る。DST終了時は逆方向にずれる。
- 既存testとCI timezoneはAsia/Tokyo中心で、DST境界を覆わない。

#### 影響

- DST地域で週次routineの開始・deadlineが1時間または1日ずれる。
- `defer_routine_task`と通常の次回反復生成が異なる周期意味を持つ。

#### 推奨する改善方針

- repetition interval daysを暦日として定義し、元のlocal dateへchecked addした後、元のlocal timeを既存のfallible local変換で解決する。
- startとdeadlineをelapsed秒差から相互導出せず、それぞれ同じ暦日数だけ移動する。
- `AmbiguousLocalDateTime`と`NonexistentLocalDateTime`を保持し、error時は変更しない。

#### 完了条件

- DST開始・終了を跨ぐ1日/7日周期でstart/deadlineの壁時計時刻を維持する。
- 親deadlineあり/なしの結果が同じ暦日規則に従う。
- 曖昧・不存在時刻は構造化errorになり、snapshotとrevisionが不変である。
- timezone別testをsubprocessまたはtimezoneを明示できる型で決定論的に実行する。

#### 推奨commit分割

1. `Test: 反復延期のDST契約を固定する`: timezone別Red testを追加する。
2. `Task: 反復日数を暦日加算する`: 共通helperを実装する。
3. `Application: routine延期を暦日helperへ移す`: use caseをGreenにする。

### TD-036: source textを独自parseするarchitecture testがRust構文と実装名へ強く結合している

- 分類: `技術的負債 / test保守性`
- 優先度: `P2`
- 概算規模: `L`

#### 現状と根拠

- `src/adapter/controller/schronu/interactive_contract_tests.rs:11-51`はcontroller配下のRust sourceをfilesystemから収集する。
- 同ファイル`158`以降はcomment、string、raw string、`cfg(test)`、braceを独自scannerで除外し、関数・trait・implの領域を文字列として抽出する。
- 同ファイル`950-1080`などは移動済み関数名、禁止識別子、正規化したsignature文字列を列挙し、architectureを検査する。scanner自体のtestも数百行必要になっている。
- `schedule_use_case_contract_tests.rs:518`以降、`handler_contract_tests.rs`、`command_contract_tests.rs`にも`include_str!`と`contains`による構造検査がある。
- Rustとして等価なrename、format、generic表現、macro利用でもtestが壊れ得る一方、文字列scannerが理解しない新構文ではfalse negativeになり得る。

#### 影響

- 製品挙動を変えないrefactorでも、独自parserと禁止symbol一覧の追従が必要になる。
- compilerが既に保証できる依存方向を文字列でも二重検証し、test suiteの規模と実行・review費用を増やす。

#### 推奨する改善方針

- module visibility、trait境界、戻り型はcompile-fail testまたは通常の型検査で固定する。
- source architecture lintが必要なら`syn`等のRust parserを使う独立`xtask`/test helperへ限定し、製品関数名のdeny listを縮小する。
- 移行中はscanner testと置換testを同時に削除せず、1契約ずつcompiler-backed testへ置換する。

#### 完了条件

- comment/string/raw stringを自前parseするhelperがcontroller contract testからなくなる。
- parser/handler/renderer/runtimeの依存方向をcompiler-backed testまたは正式なRust ASTで検証する。
- 製品関数のrenameだけでbehavior testが壊れない。
- test移動と製品挙動変更を同じcommitへ混ぜない。

#### 推奨commit分割

1. `Test: controller境界をcompile-time contractへ固定する`: 1境界だけ新testを追加する。
2. `Test: 対応するsource scannerを除去する`: 同じ契約の旧scannerだけを削除する。
3. 上記をparser、handler、view、renderer、runtimeごとに繰り返す。

### TD-037: 未使用のlenient YAML変換APIがstrict loaderと並存している

- 分類: `技術的負債 / API整理`
- 優先度: `P2`
- 概算規模: `M`
- 完了日: 2026-09-04
- 対応: 未使用`yaml_to_immutable_task`と専用test、`ImmutableTask`・`extract_leaf_immutable_tasks_from_project`と専用test、未知値を`Deadline`へfallbackする`read_repetition_anchor`と専用testを削除。strict `yaml_to_task`、repository load、YAML encoder/保存形式、CLI、MCP JSONは変更なし。不正nameのstrict validation testも追加。
- 検証: `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`成功。通常testは1038件成功、2件ignored、失敗0件。`src/`と`tests/`の削除対象3 API参照0件、repository loadは`yaml_to_task`のみ、2段階reviewと横断再reviewで残存指摘なし。
- 関連既存項目: TD-003とTD-009の完了後に残った旧経路。

#### 現状と根拠

- `src/adapter/gateway/yaml.rs:191-225`の公開`yaml_to_immutable_task`は、欠落/不正nameを空文字、未知statusをTodo、不正日時を最小時刻、型違いchildrenを空配列へ黙って変換する。
- production検索では同関数、`ImmutableTask`、`extract_leaf_immutable_tasks_from_project`にmain repository loadからのcallerがなく、testと自己再帰だけが残る。
- `src/entity/task.rs:73-77`の公開`read_repetition_anchor`も未知値をDeadlineへfallbackし、strict loaderは別途独自に未知値を拒否する。
- strict loaderの「present-but-invalidはerror」という現在の契約と逆のAPIが同じcrateで公開され、将来のcallerが誤って選択できる。

#### 影響

- dead pathのtest・型・fallback仕様を保守し続ける必要がある。
- 新しいimport機能が旧helperを再利用すると、TD-003で解消したsilent data coercionを再導入する。

#### 推奨する改善方針

- `rg`と外部利用有無を確認し、未使用なら`ImmutableTask`系とlenient converterを機械的削除する。
- 必要なread-only projectionならstrict DTOから生成し、YAML parseとdomain projectionを分離する。
- enum parseは`Option`/`Result`で未知値情報を保持し、fallbackは互換version policy内だけで行う。

#### 完了条件

- production APIに不正YAMLをsilent fallbackするconverterがない。
- repository load、検証command、migrationが同じstrict parse規則を使う。
- 削除前後で公開CLI、YAML保存形式、MCP JSONが変わらない。

#### 推奨commit分割

1. `Test: lenient YAML APIにproduction callerがないことを確認する`: 利用調査と必要ならcharacterizationを行う。
2. `YAML: 未使用のlenient immutable変換を除去する`: 機械的削除だけを行う。
3. `Entity: 未使用immutable projectionを除去する`: entity cleanupを別commitにする。

### TD-038: MCPのtask一覧に検索・paginationがなく、大規模storageで応答が無制限に増える

- 分類: `機能提案 / scalability`
- 優先度: `P2`
- 概算規模: `L`

#### 現状と根拠

- README記載の`list_tasks`入力はperiod、statuses、categoriesだけで、name query、project root、limit、cursorを持たない。
- `src/application/task_use_case.rs:283-291`は全project treeをpre-orderで`Vec`へ収集し、その後filterする。
- MCPは結果全体を1つのJSON-RPC responseへserializeする。repositoryには2172 project規模の手動性能fixtureがあり、task数に比例してresponse、memory、client contextを消費する。
- CLIにはtask名を含む表示検索があるが、MCP clientはUUID不明時に小さい応答で対象を探せない。

#### 期待する機能

- `list_tasks`へ任意の`query`、`root_task_id`、`limit`、opaque `cursor`を追加し、安定順序でpage取得できるようにする。
- cursorはsort keyとfilter条件を検証し、途中でrepository revisionが変わった場合の扱いを明示する。

#### 完了条件

- default limitと最大limitをschema・READMEへ明記し、pageを連結するとfilter済み全件と重複・欠損なく一致する。
- 同一deadline/name等の同値keyでもUUID tie-breakで順序が安定する。
- queryはUnicodeと大文字小文字規則を文書化する。
- 旧clientの引数なしcallに対する互換方針を決め、無制限応答を残す場合は明示opt-inにする。
- typical/stress fixtureでresponse sizeと走査回数の上限を測定する。

#### 推奨commit分割

1. `Test: task一覧の安定順序を固定する`: 現行順序のcharacterization testを追加する。
2. `Application: task queryとpage型を導入する`: adapter非依存のpaginationを実装する。
3. `MCP: list_tasksへlimitとcursorを追加する`: schema/input/outputを更新する。
4. `MCP: task名とroot filterを追加する`: 検索機能を別commitで追加する。
5. `Docs: task一覧paginationを記載する`: READMEとgolden schemaを更新する。

### TD-039: 稼働中processを止めずに整合したbackupを作成・検証・restoreする手段がない

- 分類: `機能提案 / 運用安全性`
- 優先度: `P2`
- 概算規模: `L`

#### 現状と根拠

- READMEは一貫したbackupのためCLIと全MCP serverを停止し、`.lock`を除くstorage全体を手動copyするよう求める。
- CLI/MCPは既にstorage advisory lockとstrict検証commandを持つが、lock保持中にsnapshotを作るuser-facing commandはない。
- TD-022のsave failureや手動復旧時に、どのrevisionの全projectを退避したかを記録するmanifestがない。

#### 期待する機能

- read-onlyの`backup`操作がstorage lockを取得し、revision、全project、schema/tool version、作成時刻、file digestをmanifest付きsnapshotへ保存する。
- `backup verify`がsource storageなしでもdigestとstrict YAMLを検査する。
- `restore`は別directoryへの展開を既定とし、稼働中storageへの上書きは明示確認・lock・事前backupを要求する。

#### 完了条件

- MCP/CLIがidleまたは別操作待ちの状態でも、lock取得後の一貫したsnapshotを作れる。
- saveとbackupが競合しても旧/新の混合snapshotにならない。
- `.lock`、temporary、staging fileを除外し、`.revision`とproject digestの対応をmanifestで検査できる。
- restore failureで既存storageを部分上書きせず、別directoryへのrestore後に`検証`を通して切替できる。
- backup format、retention、permission、機密情報の扱いをREADMEへ記載する。

#### 推奨commit分割

1. `Test: repository snapshot manifestを固定する`: read-only snapshot contractのRed testを追加する。
2. `Repository: lock下のbackup readerを実装する`: archive形式に依存しないsnapshot生成を追加する。
3. `CLI: backupとverify commandを追加する`: user-facing境界を実装する。
4. `Repository: 別directory restoreを実装する`: overwriteなしのrestoreを追加する。
5. `Docs: backupとrestore運用を記載する`: READMEを更新する。

## 推奨着手順

1. TD-020を最優先で修正し、正常なproject作成によるデータ消失を止める。同じ系列でTD-021のidentity検証を進めるが、path一意性とUUID一意性は別のRed/Green cycleにする。
2. TD-023、TD-024、TD-025を順に修正し、CLIが失敗を成功扱いする経路とpanic経路を閉じる。これにより後続項目の異常系testが正しい終了codeを観測できる。
3. TD-027とTD-030は小さく独立した正確性修正として先行できる。その後、TD-028の日別配賦とTD-029の反復完了原子性を、それぞれ別のapplication契約として進める。
4. Spreadsheet系列はTD-031とTD-032でexport欠落を先に止め、TD-034でimport validationを厳格化する。TD-026のtask名lexerを確定してから空白名のend-to-endをGreenにし、列identity変更を伴うTD-033は最後に行う。
5. TD-022はstorage formatとcrash recoveryを伴うため、P1の中でも独立projectとして進める。先にTD-020/TD-021でidentity不変条件を固定し、transaction manifestへ曖昧なprojectを持ち込まない。
6. TD-035はtimezone別test harnessを先に用意する。TD-036とTD-037は挙動変更と分けたtest/API cleanupとして、schedulingやCLIの機能修正と同じcommitへ混ぜない。
7. TD-038とTD-039は互換設計を先に文書化する将来機能である。MCP paginationは既存client互換、backup/restoreはTD-022のsnapshot protocolとの共有可能性を確認してから実装する。

## 未着手項目の並列開発計画

この節は、TD-020〜TD-039を複数worktree・複数担当で実装する際の統合計画である。担当数には上限を置かず、意味上の先行条件と主要write範囲が衝突しない限り並列化する。ここでいうwaveは全laneを待つglobal barrierではない。各laneは、自身の先行条件がmainへmergeされ、rebase後の品質ゲートがGreenになった時点で次wave相当の作業へ進んでよい。

### 判断基準

- `hard dependency`: 先行項目が契約・identity・永続化protocolを確定しないと、後続実装を正しく設計できない依存である。先行項目のGreen commitがmainへmergeされるまで製品実装を開始しない。
- `serialization dependency`: 意味上は独立しているが、同じ製品file・巨大test file・fixtureを変更するため、merge順を固定する依存である。Red testの設計は並行できるが、同じwrite範囲へ同時に実装しない。
- `integration gate`: 実装は並行できるが、両方をmergeした状態で追加のcross-boundary testを通すまで完了扱いにしない関係である。
- 文書だけの衝突は製品実装を止める理由にしない。各TDのdocumentation commitを製品Green commitから分け、`README.md`と`apps_script/README.md`はwave内の製品commitが揃った後にrebaseして順番にmergeする。

### 主要な依存関係

`H`はhard dependency、`S`はserialization dependency、`I`はintegration gateを表す。

```text
TD-020 ─H─┐
           ├─> TD-022 ─H─> TD-039
TD-021 ─H─┘

TD-037 ─S─> TD-021
TD-021 ─S─> TD-029 ─S─> TD-026 ─S─> TD-035 ─S─> TD-038

TD-024 ─H─┐
TD-034 ─S─┼─> TD-026 ─S─> TD-033
TD-037 ─S─┘                ^
TD-032 ─S─> TD-031 ────────┘

TD-021 ─H─┐
TD-022 ─H─┼─> TD-038
TD-026 ─H─┘

TD-028 ─I─ TD-030

TD-023〜TD-026、TD-033、TD-039 ─S─> TD-036
```

補足:

- TD-020とTD-021は論理的には独立だが、どちらも`task_repository.rs`とrepository testを変更する。TD-022のtransaction manifestへ安定したproject pathと一意なUUIDを格納するため、merge順はTD-020→TD-021→TD-022とする。
- TD-037は独立したdead API cleanupだが、`yaml.rs`を触るTD-021やTD-026より先に終える。古いlenient APIの削除と新しいvalidationを同じ差分にしない。
- TD-032をTD-031より先に置く。先にportableな実行環境とpipeline failure契約を確立すると、rank境界testをmacOS標準PATHでも信頼できるためである。
- TD-023とTD-029は製品fileが異なるためRed test作成までは並行できる。ただし反復完了のend-to-end error契約はTD-023のerror伝搬をmergeした後に確定する。
- TD-028とTD-030は別moduleなので並行実装する。両方のmerge後に、論理日境界を跨ぎbusy timeとも重なるsegmentの統合testを追加する。
- TD-038のcursorはtask identityとrepository revisionへ結び付く。TD-021とTD-022が確定する前にcursor形式を公開しない。またname queryの正規化はTD-026のtask名契約を再利用する。
- TD-036は挙動修正ではなくarchitecture testの置換である。先に実施するとTD-023〜TD-026、TD-033、TD-039の製品変更とtest変更が衝突するため、対象境界の製品変更後に回す。

### Wave 0: 並列作業の準備

コード変更を含まない調整段階である。

1. 各TDを別worktree・別branchへ割り当てる。branch名は`feature/td-0xx-<short-name>`とし、1 branchへ複数TDのGreen実装を混ぜない。
2. 各laneは着手前に「変更予定の製品file」「新規または変更予定のtest file」「fixture」「文書」を宣言する。同じ製品fileを予約したlaneは同時実装しない。異なる関数だからという理由だけで同一fileの同時所有を許可しない。
3. `tests/spreadsheet_contract.rs`へ複数laneが同時追記しない。TDごとの新規contract testと専用fixture directoryを作り、既存testの機械的移動は挙動変更と別commitにする。
4. `task_use_case.rs`を共有するTD-021、TD-029、TD-026、TD-035、TD-038は既定では直列化する。並列化する場合は、先に挙動非変更のmodule分割だけを独立commitで完了し、全品質ゲートを通してから各laneへ引き渡す。test fileを分けただけでは製品fileの競合を解消したことにしない。
5. 各Red/Green cycleとreview修正は、前節のcommit分割とリポジトリ規約に従う。未commit差分を別laneへ手渡さない。

### Wave 1: 独立した正確性修正(最大10レーン)

| Lane | 項目 | 主なwrite範囲 | このwaveで固定する契約 |
| --- | --- | --- | --- |
| W1-A | TD-020 | `task_repository.rs`、repository test、project作成経路 | project directory identityと旧形式の読み取り互換 |
| W1-B | TD-037 | `yaml.rs`、`task.rs`と各test | 未使用lenient APIの除去。strict loaderの挙動は変更しない |
| W1-C | TD-023 | `handler.rs`、handler contract test | `終`の不正入力・application errorを成功扱いしない |
| W1-D | TD-024 | `command.rs`、command/runtime contract test | command arityと「省略」と「不正値」の区別 |
| W1-E | TD-025 | `interactive.rs`、`runtime.rs`、interactive I/O test | terminal I/Oのfallible化とraw mode復元 |
| W1-F | TD-027 | `schedule_use_case.rs`と算術境界test | 残作業補正のchecked演算 |
| W1-G | TD-028 | `scheduled_capacity.rs`、`pack_use_case.rs`、`flatten_use_case.rs`と各test | segmentの日別分割規則 |
| W1-H | TD-030 | `daily_capacity.rs`と専用test | 深夜帯でもbusy timeを控除する規則 |
| W1-I | TD-032 | `copy_for_spreadsheet.sh`とportable PATH test | `tac`非依存とpipeline failure伝搬 |
| W1-J | TD-034 | `generate_command_from_spreadsheet.sh`と専用import test | 暦日・時分秒の全行事前validation |

W1-GとW1-Hは同時実装してよいが、どちらも相手の製品fileを変更しない。統合testは両方のmerge後にWave 2の独立laneで追加する。W1-IとW1-Jはshell fileが異なるため並行できるが、共通fixtureと`tests/spreadsheet_contract.rs`は予約しない。W1-C、W1-D、W1-Eも製品実装は並行できるが、`runtime_contract_tests.rs`と共通test supportを使うend-to-end検証はWave 2へ後置する。

### Wave 2: identity・Spreadsheet基盤・統合test(最大5レーン)

| Lane | 項目 | 先行条件 | 主なwrite範囲・注意事項 |
| --- | --- | --- | --- |
| W2-A | TD-021 | TD-020、TD-037 | `task_repository.rs`と一意性test。UUID生成時の衝突対応が`task_use_case.rs`へ及ぶ場合はW3-B開始前にmergeする |
| W2-B | TD-031 | TD-032 | `copy_for_spreadsheet.sh`とrank境界専用test。TD-032のportable implementationを維持する |
| W2-C | TD-036のschedule境界だけ | TD-027 | `schedule_use_case_contract_tests.rs`に限定してcompiler/AST-backed testへ置換する。controller/view scannerには触れない |
| W2-D | TD-023〜TD-025のCLI統合gate | TD-023、TD-024、TD-025 | `runtime_contract_tests.rs`と共通test supportをこのlaneだけが所有し、error、終了code、未保存、raw mode復元を製品経路で確認する |
| W2-E | TD-028・TD-030の容量統合gate | TD-028、TD-030 | 論理日境界を跨ぎbusy timeとも重なるsegmentをpack/flatten製品経路で確認する。製品algorithmは変更しない |

TD-036全体はこのwaveで完了扱いにしない。schedule境界の旧scannerを新しい契約がGreenになった範囲だけ除去し、残りはWave 7へ送る。

### Wave 3: repository transactionと反復完了原子性(最大2レーン)

| Lane | 項目 | 先行条件 | 主なwrite範囲・統合順 |
| --- | --- | --- | --- |
| W3-A | TD-022 | TD-020、TD-021 | repository save/recoveryと新規transaction module。storage write範囲をこのlaneが単独所有する |
| W3-B | TD-029 | TD-021、TD-023 | `task_use_case.rs`の反復完了経路と専用原子性test。memory原子性を担当する |

TD-039はW3-Aと同時に、manifest項目、crash point、backup/restore受け入れtestの設計だけを進めてよい。ただしTD-022のcommit markerとrecovery protocolがGreenになるまでrepository製品コードへ着手しない。

### Wave 4: backup coreとtask名横断契約(最大2レーン)

| Lane | 項目 | 先行条件 | 主なwrite範囲・注意事項 |
| --- | --- | --- | --- |
| W4-A | TD-039のrepository phase | TD-022 | 新規backup/snapshot module、manifest、verify、別directory restore。CLI controllerは変更しない |
| W4-B | TD-026 | TD-021、TD-024、TD-029、TD-031、TD-032、TD-034、TD-037 | task名validation、CLI lexer、YAML/MCP、Spreadsheet escapeを記載済みcommit順で実装する |

W4-AとW4-Bは新規backup moduleとtask名境界へwrite範囲を分ける。W4-Aが`task_repository.rs`または`yaml.rs`の保存・読込契約を変更する必要が生じた場合は、TD-026のYAML commitと同時実装せず、W4-Aを先にmergeしてTD-026をrebaseする。

### Wave 5: Spreadsheet identityと反復暦日修正(最大2レーン)

| Lane | 項目 | 先行条件 | 主なwrite範囲・注意事項 |
| --- | --- | --- | --- |
| W5-A | TD-035 | TD-029、TD-026 | `task_use_case.rs`の反復暦日helperとtimezone別test |
| W5-B | TD-033 | TD-026、TD-031、TD-032、TD-034 | segment identity、列契約、renderer/view、両shell、Apps Script。Spreadsheet write範囲を単独所有する |

W5-AとW5-Bはapplication task操作とSpreadsheet表示で製品fileが分かれるため並行できる。W5-Bの文書commitとTD-039 repository phaseの文書commitは製品commitの後に順番にmergeする。

### Wave 6: backup CLIとMCP pagination(最大2レーン)

| Lane | 項目 | 先行条件 | 主なwrite範囲・注意事項 |
| --- | --- | --- | --- |
| W6-A | TD-039のCLI phaseと完了 | TD-039 repository phase、TD-026、TD-033 | `command.rs`、`handler.rs`または`runtime.rs`、`view.rs`、`renderer.rs`、CLI contract test、README |
| W6-B | TD-038 | TD-021、TD-022、TD-026、TD-035 | application query/page型、MCP schema/input/output、pagination test |

W6-AとW6-BはCLI controllerとMCP/applicationにwrite範囲を分ける。backup、verify、restoreの各commandを別のRed/Green cycleにする。TD-039はrepository phaseだけでは完了にせず、CLI製品経路、failure injection、文書までGreenにして完了とする。

### Wave 7: architecture test cleanup(原則1レーン)

| Lane | 項目 | 先行条件 | 主なwrite範囲 |
| --- | --- | --- | --- |
| W7-A | TD-036の残りと完了 | TD-023〜TD-026、TD-033、TD-039 | controller、handler、runtime、view、rendererのsource scannerを契約単位でcompiler/AST-backed testへ置換する |

TD-036はparser、handler、runtime、renderer/viewの各契約を別commitにする。各置換が完全に別test fileへ閉じる場合だけ内部laneを増やしてよい。共通scanner helper、`Cargo.toml`、共通compile-fail harnessを複数laneが変更するなら1レーンへ戻す。

### 任意の並列化accelerator

既定計画は追加のmodule移動を前提にしない。critical pathをさらに短縮する価値がある場合だけ、次の挙動非変更commitを先に実施してよい。

- `task_use_case.rs`からlist/query責務を独立moduleへ機械的分離し、移動前後で全testをGreenにする。このcommitが独立してreview・revert可能なら、TD-035とTD-038をWave 5で並列化できる。
- task名validationをapplicationの独立moduleへ機械的分離できる場合、TD-029とTD-026の製品file競合を除ける。ただしCLI・YAML・Spreadsheetまで同時に移動せず、validationだけを分離する。
- accelerator自体に公開API変更、error変更、挙動変更を含めない。差分が大きくなる場合は、並列度向上よりreview容易性を優先して既定の直列計画へ戻す。

### 共有write範囲の所有順

| Hotspot | 所有順 | 並列化条件 |
| --- | --- | --- |
| `task_repository.rs`とrepository test | TD-020→TD-021→TD-022→TD-039 | 並列化しない。storage identityとtransaction protocolの主直列レーンとする |
| `yaml.rs`とYAML test | TD-037→TD-021→TD-026 | TD-021がrepository aggregateだけで完結する場合はYAML変更を省略できるが、merge順は維持する |
| `task_use_case.rs`とapplication test | TD-021→TD-029→TD-026→TD-035→TD-038 | 既定では直列化する。挙動非変更のmodule分割を先にGreenでmergeした場合だけ分離後のmodule単位で並行する |
| CLI parser/driver/表示 | TD-023・TD-024・TD-025を並列→TD-026→TD-039→TD-036 | 初動3件は製品fileとtest fileを分ける。TD-036は最後に置く |
| `copy_for_spreadsheet.sh` | TD-032→TD-031→TD-033 | 同じscriptを触るため直列化する |
| Spreadsheet import/fixture | TD-034→TD-026→TD-033 | TD-034は専用test/fixtureを使えばexport laneと並行可能 |
| Spreadsheet列・Apps Script | TD-026→TD-033→TD-036のrenderer/view部分 | TD-033中はmanifest、両shell、Apps Script、renderer/viewを1laneが所有する |
| schedule容量 | TD-027、TD-028、TD-030を並列 | TD-028とTD-030のmerge後に論理日境界×busy timeの統合testを追加する |
| `README.md`類 | 各waveの製品commit後 | docs commitをrebaseして1件ずつmergeし、製品実装の並列度を下げない |

### 各laneのmerge gate

1. 先行TDが完了statusになったことではなく、必要な契約commitがmainに存在することを確認する。
2. mainへrebaseし、競合解消で他TDのtest・error情報・戻り値を削除していないことをreviewする。
3. 対象testのRed理由が1つだった記録と、最小Green後の対象test結果を残す。
4. Green commit前に`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`を実行する。benchmarking境界を触る場合は`cargo test --locked --features benchmarking --test scheduling_benchmark_contract`も実行する。
5. shell、Apps Script、Spreadsheet列を触るlaneは`cargo test --locked --test spreadsheet_contract`に加え、そのTD専用contract testを実行する。
6. storage laneはfailure injection後の再起動testと`検証`commandの製品経路を通す。memoryだけ、またはdiskだけの単体testで完了扱いにしない。
7. wave内の全merge後にfull quality gateとcommit履歴reviewを行う。integration gateがある組み合わせはcross-boundary testを追加してから次へ進む。

### 並列計画を中断・組み替えする条件

- 実装中に予約外のHotspotを変更する必要が生じた。
- 共有error enum、公開trait、factory API、Spreadsheet列数、storage formatの変更が新たに必要になった。
- rebase時に同じ既存testのassertionを複数laneが異なる意味へ変更していた。
- 一方のlaneが他方のRed testを意図せずGreenまたは別理由のRedへ変えた。
- 品質ゲート失敗の原因を単一TDへ帰属できなくなった。

この場合、差分を強引にまとめず、依存元を先にmergeし、依存先をrebaseしてRed理由を再確認する。すでに複数責務が1commitへ混ざった場合は、共有前であればcommitを契約単位へ分割し直す。

## まとめて実施しない変更

- busy-timeの計算修正とYAML error API変更は、関連していてもRedテストと製品commitを分ける。
- 永続化のstrict化とentityからのYAML依存除去を同じ変更にしない。先に現在形式を厳密に守る。
- CLI分割時にcommand名、alias、表示文言、Spreadsheet列を変更しない。
- `Task`のtree実装変更とapplicationのschedule algorithm変更を同じ変更にしない。
- test file移動と既存testのassertion変更を同じcommitにしない。
- clippy cleanupへdomain挙動変更を混ぜない。
- 性能改善のためにscheduleの決定順序やdeadline契約を暗黙に変更しない。
- project directory命名変更、UUID一意性検証、複数project atomic saveを1つの巨大なrepository commitへまとめない。
- CLIの`終`error修正、command arity修正、interactive I/O修正は失敗理由と対象moduleが異なるため、別々のRed/Green cycleにする。
- task名lexer導入とSpreadsheet列追加を同じcommitへ混ぜない。lexerのCLI契約を先にGreenにし、その後generatorとfixtureを移行する。
- pack/flattenの日別容量修正とscheduling algorithmの選択順変更を同時に行わない。
- 反復完了の失敗原子性とDST暦日計算を同時に変更しない。
- source scanner削除時に、対応するarchitecture契約を検証なしで失わない。compiler-backedな置換testを先に追加する。

各項目は、既存テストを削除・緩和せず、期待する契約を示すRedテスト、最小のGreen実装、全検証、レビューの順で進める。

## Codexへの依頼方法の例

backlog.mdのWave 1、W1-A〜W1-Jを統括してください。各laneは内部subagentではなく、サイドバーから個別に確認できる新しいCodex taskとして作成し、それぞれ別branch・別worktreeで実装してください。親taskは各taskの進捗を監視し、backlog記載のwrite範囲と依存関係を管理してください。各laneでは契約単位のRed/Green commit、各Green後のsubagent review、品質ゲート、履歴reviewを実施してください。親taskでも各branchのmain...branch差分とcommit履歴を個別reviewし、問題があれば該当taskへ修正を依頼してください。Wave 2は実装せず、残存作業として報告してください。
各lane taskの初回turnでは、ファイル編集・実装・commitを一切行わず、backlog.md、write範囲、依存関係を調査し、契約単位のRed/Green commit計画のみを提示して終了してください。親taskから計画承認と実装開始の指示を受けるまで待機してください。
