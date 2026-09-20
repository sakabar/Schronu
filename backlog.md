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
| TD-022 | P1 | 完了 | XL | 複数project保存でrevisionだけが先行し、失敗時にdisk snapshotが部分更新される |
| TD-023 | P1 | 完了 | S | `終`が不正時刻と一部application errorを成功扱いで握り潰す |
| TD-024 | P1 | 完了 | M | CLI parserが不正な数値や余分な引数を黙って受理し、更新commandを実行する |
| TD-025 | P1 | 完了 | M | 対話CLIのterminal I/O失敗がpanicまたは未検査結果になる |
| TD-026 | P1 | 完了 | L | task名をCLI・YAML・MCP・Spreadsheet間で安全にround-tripできない |
| TD-027 | P1 | 完了 | S | 残作業時間の補正計算が合法な大値入力で整数overflowする |
| TD-028 | P1 | 完了 | M | 論理日境界を跨ぐschedule segmentの容量が開始日に全量計上される |
| TD-029 | P1 | 完了 | L | 反復task完了の後段失敗で完了状態と親見積もりだけが部分更新される |
| TD-030 | P1 | 未着手 | S | 00:00以降の日次残容量計算がbusy timeを無視する |
| TD-031 | P1 | 未着手 | S | Spreadsheet変換がind 1000以降のtask行を黙って破棄する |
| TD-032 | P1 | 完了 | S | macOS標準環境でSpreadsheet変換の`tac`依存が空出力の成功になる |
| TD-033 | P1 | 完了 | M | 同一taskの複数segmentをApps Scriptが別行へ同期する |
| TD-034 | P1 | 一部完了(W1-J) | M | Spreadsheet入力が存在しない日付と不正な時分秒をcommandへ変換する |
| TD-035 | P2 | 完了 | M | 反復延期が夏時間の切り替え境界で開始時刻とdeadlineの壁時計時刻をずらす |
| TD-036 | P2 | 完了 | L | source textを独自parseするarchitecture testがRust構文と実装名へ強く結合している |
| TD-037 | P2 | 完了 | M | 未使用のlenient YAML変換APIがstrict loaderと並存している |
| TD-038 | P2 | 完了 | L | MCPのtask一覧に検索・paginationがなく、大規模storageで応答が無制限に増える |
| TD-039 | P2 | 完了 | L | 稼働中processを止めずに整合したbackupを作成・検証・restoreする手段がない |
| TD-040 | P0 | 完了 | S | 小数秒付き現在時刻でslack indexとschedulerの論理時刻が乖離する |
| TD-041 | P2 | 未着手 | L | task treeとscheduleの再帰処理が大規模storageでlarge stackを必要とする |
| TD-042 | P2 | 未着手 | S | Schronu Webのcomponent testがpart1/part2という無意味な単位で分割されている |
| TD-043 | P1 | 未着手 | M | Pending期限上限が実施済み時間を考慮せず見積全体から計算される |

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
- 完了日: 2026-09-05
- 対応: 変更projectをimmutable manifestとstaged fileへwrite・syncしてから、独立markerをatomicに公開・directory syncするrepository transactionを導入した。markerだけをcommit pointとし、markerなしはlive targetを変更せず破棄して旧snapshotを維持し、markerありは全materialの長さとchecksumを事前検証後に新snapshotへidempotentにroll-forwardして`.revision`を最後に揃える。既にmanifestどおりの内容であるlive targetは適用済みとして扱い、対応staged fileが欠落しても残りを回復する。同一内容のskip、permission維持、静止したsymlinkの拒否、advisory lockによる直列化を維持し、deleteは公開APIを増やさないprivate protocolだけを実装した。path検証後に外部processがfilesystem entryを悪意的に差し替えるsymlink TOCTOUは、TD-022の脅威model外でありrepository全体に残る独立したsecurity debtとした。
- 検証: prepare、commit marker、markerなしdiscard、markerありroll-forward、内容検証付きmaterial preflight、適用済みtarget判定、revision後置、deleteの各契約をRed/Green cycleで固定し、各Green後のsubagent review指摘を個別commitで修正・再reviewした。`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功し、testはlibrary 645件成功・1件ignored、controller 475件成功、integration test全件成功を確認した。
- 保守性: `storage_transaction.rs`をprivate facade、error、request/state定義、module wiringとgateway向け再公開に限定し、`layout.rs`はpath生成と相対path検証、`manifest.rs`はmanifest v1 schema・serialize・decode・意味検証・checksum、`io.rs`はfilesystem I/O・advisory lock・sync・Unix境界、`prepare.rs`はstagingとmarker前処理、`commit.rs`はmarker公開・全entry preflight・live target適用・revision後置と`CommittedTransaction::roll_forward`、`recovery.rs`はmarker判定・未commit transaction破棄・`CommittedTransaction`復元と`roll_forward`呼出、`cleanup.rs`はtombstone handoffと再清掃を所有する構造へ分割した。
- 内部状態: disk由来の`RawTransactionManifest`を意味・path・integrity検証済みの`ValidatedManifest` / `ValidatedEntry`へ一度だけ変換し、marker前の`PreparedTransaction`とmarker公開済みの`CommittedTransaction`を型で分離した。transaction製品moduleのfilesystem syscallは`io.rs`へ集約した。
- test構造: transaction 33件(元32件とmanifest v1保存bytes characterization 1件)をmanifest、prepare、commit、recovery、delete、securityへ、repository経路18件をsave、recovery、supportへ分割してscenario名・assertion・failure pointを維持した。failure injectionは両suite共通の`RecordingIo`へ集約し、専用I/O mockはmarker競合用barrierだけを残した。
- 保守性検証: `storage_transaction.rs`と配下の製品・test file、共通transaction test support、新規repository transaction test fileは各800行未満で、最大製品fileは`commit.rs`の395行、最大対象test fileはrepository recoveryの690行である。transaction製品コードの`allow(clippy::too_many_arguments)`、検証済みentryに対する`expect`、`io.rs`以外のraw filesystem syscallはいずれも0件である。`main...HEAD`の累積差分は27 files、4,815 insertions、417 deletionsであり、正しさreviewと独立した保守性reviewで責務境界、依存方向、重複、test fixture、残存負債を再確認した。
- 残存非対象: symlink TOCTOU完全耐性、Windows対応、重複targetの新規拒否、全staged bytesを保持するpreflightのmemory最適化、製品delete公開API、TD-039のbackup設計は本対応へ含めていない。

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
- 完了日: 2026-09-04
- 対応: interactive driverへ注入可能なinput sourceとterminal factoryを設け、prompt、refresh、cursor、error表示、終了描画、flushをすべてfallibleにした。raw terminalはRAII guardで所有し、初期化失敗と出力失敗をsource付き`InteractiveIoError`として区別する。runtimeのinteractive描画helperも`Result`を返し、非対話経路と共通の分類で`BrokenPipe`だけを正常終了として扱う。module・関数単位の`allow(unused_must_use)`を除去し、成功commandのtransaction内即時保存、terminal・raw mode失敗時の追加保存なし、Ctrl-D・stdin異常時の既存reload後saveを維持した。
- 検証: prompt、refresh、cursor、retry error、submit後prompt、終了描画、flushのfailure writer testと、raw mode初期化失敗、通常終了・handler failure・output failure時のguard drop testを追加した。`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功した。W2-Dのprocess-level統合gateは後続laneとして残す。
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
- 完了日: 2026-09-05
- 対応: application層へcanonical task名validatorを置き、原文をtrim・正規化せず保持したまま、validation時だけtrimしてblankとoptional sign付きASCII整数だけの名前を拒否し、全Unicode control characterも拒否する契約へ統一した。interactive CLIにはsingle quote内をliteral、double quote内とquote外のbackslashを次の1文字のescapeとして扱い、quoteを除去して隣接fragmentを連結する共通lexerを導入した。non-interactive CLIはOS argvをjoin・再lexerせずtoken列parserへ直接渡す。Spreadsheet generatorはJ列の原文をdouble-quoted argvへ変換してbackslashとdouble quoteをescapeし、A-Sの19列と全task名を全行事前検証して不正時のstdoutを空に保つ。strict YAMLはtask path、repository loadは実file pathを保持して診断し、MCPは既存の`invalid_input` error形状を保ったまま同validatorを使用してschema descriptionも同期した。A-J列、A-S manifest、Apps Script同期列、storage schemaは変更していない。
- 検証: application validation、interactive lexer、non-interactive argv、Spreadsheet escape・構造保護、strict YAML・repository診断、横断integrationを契約単位のRed/Green commitで固定し、各Greenで`git diff --check`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`を成功させた。Spreadsheet、MCP、YAML、CLIの専用testも成功した。各cycleのspec・quality reviewと最終の履歴・累積差分の保守性reviewを行い、P1/P2/P3の残存指摘がないことを確認した。
- 残存: blockingな新規負債はない。OS argvのNULはprocess起動前に`InvalidInput`となってapplicationへ到達しないOS境界をtestで固定しており、保存前に拒否されるため横断契約を妨げない。SpreadsheetはAWK上に同じ規則を表現する必要があるが、実generatorから各公開境界を通す横断testでdriftを検出する。独立dry-runは追加せず通常のrepository loadが既存不正名をfile/task path付きで診断するため、利用中のstorageを変更せず発見できる。

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
- 完了日: 2026-09-05
- 対応: 対象taskの実績・status・完了時刻、反復親の見積もり、次回child追加を1つのentity operationへ集約した。root・対象task・親の全mutable borrowとhierarchy grantを最初のwrite前に取得し、commit phaseは失敗不能としてrootのmutation revisionを1回だけ進める。`complete_task`の反復あり経路とtest helperを同operationへ接続し、反復なし経路は変更していない。
- 検証: hierarchy edit禁止とroot・対象task・親の各borrow競合でsnapshot、親見積もり、children、revisionが不変であること、成功時の既存完了値と次回反復taskの契約、root revisionの1増分をRed/Green testで固定した。`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功し、各Green後の内部review指摘を1件1commitで修正して再review済み。

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

### TD-031: Spreadsheet変換がind 1000以降のtask行を黙って破棄する

- 分類: `バグ / Spreadsheet export`
- 優先度: `P1`
- 概算規模: `S`

#### 現状と根拠

- `shell/copy_for_spreadsheet.sh:9`は`/^0/`で始まる行だけをtask行として処理する。
- `src/adapter/controller/schronu/renderer.rs:968`付近のind表示は`format!("{:04}", ind)`という最小幅であり、ind 999は`0999`、ind 1000は`1000`になる。
- 実行確認では`0999`は変換され、`1000`と`10000`は0行になった。errorやwarningは出ない。
- `tests/spreadsheet_contract.rs:116`付近のfixtureはind `0000`と`0001`だけで境界を覆わない。

#### 影響

- 1001件以上を表示する大規模storageで、Spreadsheetへ貼るtaskが途中から黙って欠落する。
- 欠落後もpadding行が出るため、空きtaskとして気付きにくい。

#### 推奨する改善方針

- 行頭文字でなく、ind、UUID、estimated_datetimeなどA-J列のtask row grammarを検査する。
- より堅牢にはCLIへ`--format tsv`等のmachine-readable出力を追加し、human displayの見た目へshellを依存させない。

#### 完了条件

- `0999`、`1000`、`10000`をすべて保持し、見出し・集計・warning行は除外する。
- 不完全なtask行は黙ってskipせず、line番号付きerrorにする。
- 既存A-J列順とtask名中の空白を維持する。

#### 推奨commit分割

1. `Test: Spreadsheet ind上限境界を固定する`: 999/1000/10000のRed fixtureを追加する。
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
- 完了日: 2026-09-06
- 対応: A-Sの19列とCLI A-Jを維持し、同一export snapshot内のA列`ind`とB列`task_id`をsegment複合keyにした。L/Pは相手sheetの対応segmentだけへ、N/Rはsource/target両sheetの同一task全segmentへ同期する。全対象をwrite前に検証し、identity欠落、対応なし、複合key重複、N/Rの競合一括編集を無書込の1回Toastで診断する。数値`0`として読み取られた先頭`ind`も保持する。
- 検証: Node 24の標準test runnerとfake Spreadsheet APIで`apps_script/main.js`を`node:vm`評価し、`onEdit`製品入口を実行する16件に成功した。CIへ同testを追加し、Spreadsheet contract 5件、renderer contract、両shell構文、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功した。内部reviewのP1 1件・P2 2件を個別修正し、再reviewと親branch reviewで未解消P1/P2/P3がないことを確認した。
- 残存: なし。

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

### TD-035: 反復延期が夏時間の切り替え境界で開始時刻とdeadlineの壁時計時刻をずらす

- 分類: `バグ / timezone`
- 優先度: `P2`
- 概算規模: `M`
- 完了日: 2026-09-06
- 対応: `repetition_interval_days`をOS local timezone上の暦日として扱い、通常の次回反復生成とroutine延期を、local dateのchecked加算後に元の壁時計時刻をfallibleに解決する共通helperへ統一した。startとdeadlineは互いのelapsed差分から導出せず、それぞれ独立して移動し、全日時の解決後にmutationする。
- 検証: `America/New_York`を設定したfresh subprocessで冬・夏のUTC offsetをcanary検証し、夏時間の開始・終了を跨ぐ1日/7日周期、親deadlineあり/なし、`days_in_advance`、曖昧・不存在時刻の構造化errorと失敗時のsnapshot・親aggregate revision・保存回数・focus・子一覧不変を固定した。`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功し、subagent reviewと親branch reviewで未解消指摘なし。
- 残存: なし。公開API、error enum、gateway、storage schemaは変更していない。
- 関連既存項目: TD-010はlocal datetime変換errorを統一したが、反復間隔をelapsed durationとして扱う経路が残る。

#### 現状と根拠

- `src/application/task_use_case.rs:425`付近の親deadlineなし経路は`orig_deadline + Duration::days(interval)`とし、暦日でなく24時間単位を加算する。
- 親deadlineあり経路は新deadlineを暦日で構築した後、`new_deadline - orig_deadline`の`num_days()`を開始時刻へ適用する。
- 夏時間の開始週における7暦日は6日23時間なので、前者は壁時計時刻が1時間ずれ、後者は`num_days()`切り捨てで開始日が1日不足し得る。夏時間の終了時は逆方向にずれる。
- 既存testとCI timezoneはAsia/Tokyo中心で、夏時間の切り替え境界を覆わない。

#### 影響

- 夏時間を採用する地域で週次routineの開始・deadlineが1時間または1日ずれる。
- `defer_routine_task`と通常の次回反復生成が異なる周期意味を持つ。

#### 推奨する改善方針

- repetition interval daysを暦日として定義し、元のlocal dateへchecked addした後、元のlocal timeを既存のfallible local変換で解決する。
- startとdeadlineをelapsed秒差から相互導出せず、それぞれ同じ暦日数だけ移動する。
- `AmbiguousLocalDateTime`と`NonexistentLocalDateTime`を保持し、error時は変更しない。

#### 完了条件

- 夏時間の開始・終了を跨ぐ1日/7日周期でstart/deadlineの壁時計時刻を維持する。
- 親deadlineあり/なしの結果が同じ暦日規則に従う。
- 曖昧・不存在時刻は構造化errorになり、snapshotとrevisionが不変である。
- timezone別testをsubprocessまたはtimezoneを明示できる型で決定論的に実行する。

#### 推奨commit分割

1. `Test: 反復延期の夏時間契約を固定する`: timezone別Red testを追加する。
2. `Task: 反復日数を暦日加算する`: 共通helperを実装する。
3. `Application: routine延期を暦日helperへ移す`: use caseをGreenにする。

### TD-036: source textを独自parseするarchitecture testがRust構文と実装名へ強く結合している

- 分類: `技術的負債 / test保守性`
- 優先度: `P2`
- 概算規模: `L`
- 状態: 完了。[PR #467](https://github.com/sakabar/Schronu/pull/467)へpush・PR本文更新済み。

#### W2-C schedule境界の既存証跡

- `6f888bfa`で`get_schedule`を`fn(&dyn TaskRepositoryTrait) -> Result<Vec<ScheduledTaskView>, ApplicationError>`へ固定する型契約を追加し、`22315c98`で対応するschedule source scannerを除去済み。
- `src/application/schedule_use_case_contract_tests.rs`の型契約と製品経路の挙動testは今回変更していない。Wave 2のschedule完了範囲を維持し、残っていたcontroller側だけをW7-Aで置換した。

#### W7-A controller側5境界の最終的な保証分担

独自Rust scannerを除去し、compilerと製品経路のbehavior testを主な保証とする。test用`syn`による`tests/controller_architecture.rs`は、具体的な禁止依存と宣言所有を検査する。製品挙動・公開API・出力契約は変更していない。簡素化による製品fileの差分は、再描画testを接続する`runtime.rs`の`cfg(test)` includeだけである。

| 境界 | compiler・製品経路のbehavior test | 残すAST検査 |
| --- | --- | --- |
| parser | 実argvのtoken境界、3入口のmode、引用符・alias、typed field/error、maintenance errorの優先順 | handlerからcommand内のparser関数への依存を宣言に基づいて禁止し、typed validatorを許容 |
| handler | typed contextのtrait整合、全command groupのoutcome・context呼び出し、finish値とplacementのalias非依存 | 外部I/O・writer依存禁止、contextの宣言所有 |
| runtime | 両実行入口の状態更新・transaction traceと失敗経路、不正入力時のI/O不実行、Verify保存不実行、終了・保存・出力error。実runtime driverで表示/更新command後の葉描画、focus、flushを検証 | I/O調停・terminal操作の所有、handler context・domain更新capability・Naive系日時型への依存禁止 |
| view | 実TaskTreeCommandContextの通常順/低優先度末尾順、対象行・typed model・metrics、focus表示 | writerfree、具体的な表示data型のruntime依存禁止、Focus sourceの宣言所有 |
| renderer | 診断本文と末尾改行の完全一致、flush回数・部分出力・error情報、progressの秒数・境界・負値・未算定・超過表示 | semantic表示のlegacy禁止、RenderModeの宣言所有 |

- AST policyは`tests/controller_architecture/`配下。`source.rs`が製品moduleとtest用cfg除外、`imports.rs`がimport/glob解決、`paths.rs`が参照・宣言・writer capabilityを扱う。alias・macro・nested helper・関数ポインタ・UFCSの回帰例を維持し、未対応構文は無視せずerrorにする。
- `parser.rs`・`dispatch.rs`・`progress.rs`を削除し、「共有関数を直接1回呼ぶ」「引数が直接Call式」「callback内の実行を静的追跡する」という形状契約を製品挙動へ置換した。日時の戻り型、f64/float literalからの役割推測、型aliasの役割伝播、finish/placementの文字列・index構文禁止も除去した。Naive系と具体modelの依存禁止は残している。
- call記録・scope追跡・callback whitelist・`and_then`特別扱い・member/index/signature蓄積を除去し、I/O依存規則を共通化した。新しい汎用解析器・fixture基盤・公開APIは追加していない。
- 対象5 contract_test fileの自作scannerは除去済み。behavior testを補強してから対応するAST形状検査を別commitで削除し、既存testでmutationを検出できる契約にはtestを追加していない。legacy helperの非対話経路は実argv入口へ接続し、`0001`の文字列入力とargvで異なる既存仕様をそれぞれ固定した。

#### W7-A簡素化後のlane検証

- 検証対象code HEAD: `ca42f6e8`。`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`はすべてexit 0。22 suites / 1,459 passed / 0 failed / 2 ignored、ASTは73 passed。ignoredは既存のまま。
- 27種類の製品mutationを拒否した(compilerによるwriter付きtrait変更の拒否1件を含む)。診断への余分な出力、flush欠落・重複・error握り潰し、再描画の常時実行/常時省略、parser mode・token境界、dispatch・状態更新・保存、数値・model順序・禁止依存を確認し、mutationはすべて復元してcommitから除外した。
- AST関連は簡素化終了時に4,360行から2,415行へ削減し、親reviewのraw module回帰修正後は2,470行(当初比1,890行減)。最大fileは334行、新規の`runtime_redraw_contract_tests.rs`は53行で既存runtime helperとterminal fixtureを再利用した。`syn`の`full`・`visit`・`visit-mut`は残すmodule/cfg・依存検査で使用する。
- 内部reviewと累積reviewの指摘は解消済み。親の具体依存維持の指摘を`93184542`、内部reviewのcontext関連UFCS依存の指摘を`42cf09c1`で個別修正した。親独立reviewのraw module名による検査漏れは`ca42f6e8`で論理module名と暗黙pathを`unraw()`へ統一し、通常file・mod.rs・inline・`#[path]`とhandler関数ポインタ禁止を回帰検証した。親はscope・log・累積差分・実装差分を確認し、`ca42f6e8`で独立reviewのP2解消を確認済み。未解消P1/P2なしで保守性も受入れ済み。文書commit `976e61c8`もscope内として承認された。
- 証跡: `/private/tmp/w7-simplification-record.md`にcommit対応とmutation一覧、`/private/tmp/w7-raw-module-{red,clippy,green}.log`に最終修正のRedとlane gateを記録した。

#### 簡素化後の親再統合gate

- `origin/main=10fba05d`から107commitを使い捨て統合worktreeへ適用した統合HEAD `95d793ce057e4d7dc9b1511e5a509566050b61ed`で通過した。tree `6609a01fbefd8cc1a04747971a8e18adf50e2fe7`はlane HEAD `976e61c8`と一致する。
- `git diff --check`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`はすべてexit 0。22 suites / 1,459 passed / 0 failed / 2 ignored、AST 73 passed。
- ログは`/private/tmp/wave7-simplified-integration-gate-{0..3}.log`、結果は`/private/tmp/wave7-simplified-integration-gate-results.json`。この証跡追記は文書のみで、検証対象の製品・test codeを変更しない。

#### 簡素化前の親統合gate(過去証跡)

- `origin/main=10fba05d`から当時の87commitを使い捨てworktreeへcherry-pickした統合HEAD `18b80583`でgateを通過した。tree `f61919dc2ffb951a8f49ac7345e6089b4ce5a6c6`は当時のlane HEAD `1a6add89`と一致する。
- 当時の`git diff --check`、fmt、clippy、全testはexit 0。22 suites / 1,494 passed / 0 failed / 2 ignored、AST 111件で、CLI runtime・storage backup・Spreadsheetを含む。ログは`/private/tmp/wave7-integration-gate-{0..4}.log`。これは簡素化前の過去証跡であり、簡素化後の結果は上記の親再統合gateに記録した。

#### 残存範囲・別契約

- W2-CとW7-Aのsource scanner置換と簡素化は完了。親再統合gate通過済み。
- `src/adapter/controller/mod.rs`の`binary_entrypoint_delegates_to_library_cli`は、binary入口がlibraryの`run_cli`だけへ委譲する薄いwrapperであることをsource一致で固定する別契約であり、今回の独自Rust scanner置換の対象外として維持した。

#### 対応前の現状と根拠

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
- 状態: `完了`
- W6-B対応日: 2026-09-06

#### 対応と根拠

- `list_tasks`へ`query`、`root_task_id`、`limit`、opaque `cursor`、`unbounded`を追加した。既定limitは100、最大500で、無制限取得は`unbounded: true`だけを明示的に受理する。
- applicationに既存の全件取得APIを残したままpaged APIを追加し、task全件の事前`Vec`化を避けるiterative pre-order DFSでpage境界まで走査する。
- cursorはversion、repository revision、canonical filter fingerprint、DFS再開位置、直前task UUIDを保持する。形式、version、filter、revision、再開位置の不一致は情報を失わず既存の`ApplicationError::InvalidInput { field: "cursor", .. }`へ分類する。
- `query`はUnicode lowercaseの部分一致とし、Unicode正規化を行わない。`root_task_id`は指定task自身を含むsubtreeを選択する。

#### 検証

- 0、1、100、101、500件のcardinality境界で全pageを連結し、pre-orderの重複・欠損がなく、終端が`next_cursor: null`になることを固定した。
- queryのUnicode lowercase・正規化なし・空文字、root subtree、limit変更、filter canonical化、malformed/version/revision/resume mismatch、unbounded競合をApplication/MCP契約testで固定した。
- 実storageを使うstdio testでcursor継続と別processの保存後のrevision失効を確認した。
- canonical typical/stress fixtureで走査数と保持数の上限を固定し、MCP JSON-RPC response全体が128KiB以下かつ2秒以内であることを確認した。
- `cargo fmt --check`、feature有無の`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功した。

#### 完了条件

- default limitと最大limitをschema・READMEへ明記し、pageを連結するとfilter済み全件と重複・欠損なく一致する。完了。
- project treeのpre-orderを維持し、cursorで直前task UUIDとDFS位置を照合する。完了。
- queryのUnicode lowercase部分一致と正規化なしを文書化する。完了。
- 引数なしcallを既定100件とし、無制限応答を`unbounded: true`へ限定する。完了。
- typical/stress fixtureでMCP response size、走査数、保持数、応答時間の上限を測定する。完了。

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
- 状態: `完了`
- W4-A repository phase対応日: 2026-09-05
- W6-A CLI phase対応日: 2026-09-06

#### W4-A repository phase対応

- exclusive storage lock取得後にtransaction recoveryとstrict repository validationを行い、revisionと全project fileが同一時点に揃ったsnapshotを作成するrepository APIを実装した。
- manifest v1へnullable revision、directory、permission、全fileのbyte lengthと長さ付きFNV-1a 64bit digestを記録し、source storage非依存のverifyを実装した。
- 未存在の別directoryへstaging経由でatomic restoreし、rename後のparent sync失敗もbounded rollbackと再syncを行うようにした。
- `.lock`、transaction、既知temporary/staging artifactを物理的に除外し、digest、revision、strict YAML、重複UUID、path traversal、symlink、reserved path、欠落・余剰fileを検証する。
- manifest 8 MiB、file entry 10,000件、1 file 64 MiB、payload合計256 MiB、相対path 4,096 byte、path depth 64の共通private上限と、rollback cleanupのdepth・総entry上限を実装した。
- 別directoryへのrestore後に既存の製品`検証`経路を通るintegration contractを追加した。

#### W4-A検証

- manifest、lock/recovery、create、verify、security、restore、resource limit、rollback cleanupを契約単位のRed/Green commitへ分割した。
- 各Green後の内部review、親task review、累積spec/security・保守性・履歴reviewを実施し、blockingなP1/P2を解消した。
- `git diff --check`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`を通過した。Linux CIのdevice ID型差によるclippy失敗もplatform別変換へ修正し、Linux CIとmacOSの意味を一致させた。

#### W6-A CLI phase対応

- `backup <snapshot_dir>`、`backup verify <snapshot_dir>`、`restore <snapshot_dir> <destination_dir>`、`restore current <snapshot_dir> <pre_backup_dir> REPLACE_CURRENT_STORAGE`を対話・非対話CLIの共通parse経路へ追加した。
- backupはexclusive lock内でsnapshot作成からrepository reloadまでを調停し、strict verifyはsource storage非依存、通常restoreは別directoryのみ、current restoreは明示確認・事前backup・transaction置換を必須とした。
- current restoreでfileとdirectoryのentry種別変更、commit marker後のrecovery、snapshot permission復元を保持し、directory permissionのsetとsync失敗は個別のphaseとpathで診断する。
- READMEに推奨運用、offline fallback、retention、permission、機密情報、default resource limits、FNV-1a 64bit digestの非暗号学的性質を記録した。

#### W6-A検証

- CLI製品経路18件で成功、引数不正、strict検証失敗、current storage alias、既存destination・pre-backup、lock競合、restore原子性を固定した。
- storage snapshotとtransactionのfailure injectionで、prepare・commit中断、entry種別変更後のrecovery、file・directory permissionの復元、削除順序とpath安全性を検証した。
- `cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`を通過した。

### TD-040: 小数秒付き現在時刻でslack indexとschedulerの論理時刻が乖離する

- 分類: `バグ / プロセス継続性`
- 優先度: `P0`
- 概算規模: `S`
- 完了日: 2026-09-05
- 対応: 作業量とslack需要は従来どおり整数秒で更新し、`SlackDemandIndex.current_time`は実際のsegment境界時刻へ直接同期するようにした。
- 検証: 障害ログと同じ718.250msの乖離を再現するRed testを追加し、修正後のrelease再選択、segment境界、整数作業秒数を固定した。rootの全品質gateに成功した。

#### 現状と根拠

- 2026-09-05、`schronu-web`の60秒ごとの更新またはその近傍の「今」再取得中に、`schronu-today-text`専用threadが`slack index time diverged`でpanicした。
- panic時の`SlackDemandIndex.current_time`は`2026-09-05T15:59:59.281750+09:00`、schedulerの`now`は`2026-09-05T16:00:00+09:00`で、差は718.250msだった。
- `schedule_selected_segment`は境界までの作業量を`num_seconds()`で切り捨てた整数秒とし、scheduler本体の`now`は実際の境界時刻へ設定していた。
- `SlackDemandIndex::record_work`が同じ整数作業秒数だけを`current_time`へ加算していたため、小数秒付きの開始時刻から整数秒境界へ進むとindex側に端数が残った。

#### 影響

- debug buildでは次の候補選択時の不変条件検査で実行threadがpanicする。workerを再生成しないWeb構成では、server processが残っていても以後の「今」queryは成功しない。
- release buildで不変条件検査が無効でも、同じ論理時刻を持つべきstateが乖離し、後続のslack判定が異なる時刻を前提とする。
- この障害はstack overflowではなく、32MiB stack、`.zshrc`、`RUST_MIN_STACK`、Dioxus componentとは独立している。

#### 完了条件

- 小数秒付きの開始時刻からfixed、release、deadline guardなどの境界へ進んでも、`SlackDemandIndex.current_time`とschedulerの`now`が一致する。
- segmentの`scheduled_work_seconds`、taskの残作業秒数、slack需要は従来の整数秒契約を維持する。
- `debug_assert_eq!`を残し、内部不変条件の再検出を継続する。
- Web branchで別途追跡する再帰処理とstack使用量の改善は、本修正と独立した変更として扱う。

### TD-041: task treeとscheduleの再帰処理が大規模storageでlarge stackを必要とする

- 分類: `設計 / プロセス継続性`
- 優先度: `P2`
- 概算規模: `L`

#### 現状と根拠

- task treeの読込、走査、schedule・view生成には再帰処理が残っており、大規模storageでは通常のthread stackを使い切る。
- `schronu-web`はbootstrap、一覧取得、自動セッション選定、実績記録、完了を処理する専用worker threadへ32MiBのstackを予約し、実データ処理時のstack overflowを防御している。
- この予約はWeb serverのprocess abortを防ぐための局所的な対策であり、再帰処理そのもののstack消費量や入力規模に対する上限は解消していない。

#### 影響

- task数やtree深度が現在の代表値を超えると、32MiBを予約したWeb workerでもstack overflowへ再到達する可能性がある。
- CLIなど別の実行経路は各threadのstack policyへ依存し、同じstorageに対するprocess継続性が実行環境ごとに異なり得る。
- Web固有のstack予約が根本原因を覆い隠すため、再帰箇所を追加すると必要stack量を予測しにくい。

#### 推奨する改善方針

- repository load、task tree traversal、schedule生成、view生成を個別に計測し、stack消費が支配的な再帰箇所を特定する。
- 現行の走査順序とerror契約をcharacterization testで固定してから、対象ごとに明示的な`Vec` stackを使う反復処理へ段階的に置換する。
- Web workerの32MiB指定は移行中の防御として維持し、各置換後に2MiB固定stackでcommit管理済みfixtureを検証する。実データ確認は補助acceptanceとして別に行う。

#### 完了条件

- `SchedulingFixture::build(FixtureSize::Typical)`の26,378 tasks・最大depth 210と、その4倍のtask規模を持つ`FixtureSize::Stress`が、2MiB固定stackの専用threadでstack overflowせず処理できる。
- repository load、tree traversal、schedule・view生成の順序、出力、error情報が置換前後で一致する。
- `schronu-web`固有の32MiB stack指定を削除し、root/Webの全品質gateと実データacceptanceがGreenになる。
- 実データはfixture契約の代替にせず、Web初回表示と手動更新が成功する補助acceptanceとして確認する。

#### 依存関係

- 今回の`schronu-web` module分割には混ぜず、再帰処理ごとに独立したRed/Green cycleで進める。
- 公開API、storage schema、lock metadataを変更する必要が生じた場合は、互換性設計を別commitで先に固定する。

### TD-042: Schronu Webのcomponent testを責務別moduleへ再編する

- 分類: `設計 / 検証容易性`
- 優先度: `P2`
- 概算規模: `S`

#### 現状と根拠

- `schronu-web/src/app/component_tests.rs`が`component_tests_part1.rs`と`component_tests_part2.rs`を`include!`している。
- `part1`、`part2`という名前はfileの順序と大きさしか表さず、どの製品契約を検証するmoduleか判断できない。
- action変換、reload復元、background refresh、session操作、shell描画、持ち歩きlock、および共通fixtureが2fileへ混在している。
- `include!`によって同じ字句scopeへ展開されるため、importとtest helperのfile間依存が表面化しない。

#### 影響

- 変更した契約に対応するtestを探しにくく、追加先が責務ではなくfile sizeで決まりやすい。
- 共通helperの所有場所が曖昧になり、重複または暗黙の依存を増やしやすい。
- review時に、どの契約群へ影響した差分かをfile単位で判別できない。

#### 推奨する改善方針

- `part1`、`part2`を廃止し、少なくとも次の責務を名前で表すtest moduleへ分割する。
  - action変換とbackground refresh中の操作guard
  - reload時のview state復元とbackground refresh
  - orchestratorのsession操作とserver effect
  - shell、overlay、stale表示の描画model
  - 持ち歩きlock
- `MemoryStorage`、snapshot・task row builderなど複数moduleで必要なfixtureは、test専用の`component_test_support`へ1回だけ定義する。
- test名、assertion、製品挙動、公開API、storage schemaを変えず、最初のcommitは機械的なmodule移動だけに限定する。
- helperの重複整理やtest API改善が必要になった場合は、移動後の独立したGreen commitとして扱う。

#### 完了条件

- `component_tests_part1.rs`、`component_tests_part2.rs`およびそれらへの`include!`が残っていない。
- 各test moduleの名前と内容が1つの契約境界に対応している。
- 複数moduleで使うfixtureがtest support moduleへ集約され、隠れたfile間依存がない。
- 再編前後でtest名、test数、assertionの意味、および製品コードの挙動が維持されている。
- `cargo fmt --check`、`cargo test --locked -p schronu-web`、該当clippy、WASM check、root品質gate、`git diff --check`が成功する。

#### 依存関係

- 現在進行中のview state復元変更と同じtest fileを触るため、その変更をGreenで確定した後に独立着手する。
- 完了済みの`TD-015`は再openせず、その後に導入されたSchronu Web固有の残存負債として扱う。
- test moduleの機械的移動と、製品挙動・storage schema・UI契約の変更を同じcommitへ含めない。

### TD-043: Pending期限上限が実施済み時間を考慮せず見積全体から計算される

- 分類: `正確性 / domain logic`
- 優先度: `P1`
- 概算規模: `M`

#### 現状と根拠

- `LogicalDateTimePolicy::deadline_pending_limit`は、deadlineから見積全体と5分のbufferを引いてPending期限上限を算出する。
- `TaskAttr::set_orig_status`は実績時間を参照せず、この上限で`pending_until`を短縮する。また、deadline接近時のstatus判定にも同じ見積全体ベースの上限を使う。
- `flatten_use_case`も見積全体ベースの同じhelperで実効`pending_until`を制限する。
- WebとCLIの`d`相当は共通の`defer_task`を経由して`TaskAttr::set_orig_status(Status::Pending)`を呼ぶため、両方がこの影響を受ける。
- 一方、scheduleは実績を考慮した残作業時間を使っており、見積超過時の補正と算術overflowを扱う別の計算がapplication内に存在する。

#### 影響

- 作業済みのtaskほど実際の残作業より大きな時間がdeadline前に必要だと判定され、必要以上に早くPendingを解除される。
- Web・CLIの延期、直接のPending変更、flatten、scheduleで残作業の解釈が一致せず、同じtaskでも操作経路によりdeadline余裕の判断が変わる。
- 先送り可否をdeadline余裕で分岐する機能が既存helperを利用すると、この過剰に保守的な判定を引き継ぐ。

#### 推奨する改善方針

- Pending期限上限を`deadline - 残作業時間 - 5分`へ変更する。
- 残作業時間を単純な減算として重複実装せず、scheduleで使われている見積超過時の補正規則とoverflow処理を共通domain logicへ集約する。
- deadline接近によるTodo化、Pending期限上限、flatten、Web・CLIの延期が同じ残作業契約を利用するよう統一する。
- 5分のbufferと、既存errorが保持するtask ID・見積秒・実績秒の情報量を維持する。

#### 完了条件

- 実績0、見積未満、見積と同値、見積超過の各ケースで、共通の残作業規則からPending期限上限が算出される。
- WebとCLIの`d`、直接のPending変更、deadline接近によるstatus判定、flatten、scheduleで残作業の解釈が一致する。
- 5分のbufferが維持され、上限と要求時刻が同値の場合を含む境界testがある。
- 合法な大値入力でもpanicやwraparoundを起こさず、overflow時の型付きerror情報を失わない。
- 既存のWeb、CLI、flatten、schedule契約testとroot品質gateがGreenになる。

#### 依存関係

- 現在のWeb先送り機能では既存計算を共通利用し、本項目の修正を同じcommitへ混ぜない。
- `TD-027`で導入した残作業補正のoverflow契約を維持し、共通化に伴うerror型や公開APIの変更は独立したRed/Green cycleで固定する。

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

#### Wave 6完了記録(2026-09-06)

- 固定基点`4d87ed77`からW6-A、W6-Bの順に統合し、TD-039とTD-038を完了した。
- W6-Aは整合snapshotのbackup・verify、別directory restore、事前backup付きcurrent restoreをCLI製品経路へ公開し、transaction recovery、安全なpath境界、lock、error情報を維持した。
- W6-Bは`list_tasks`へquery・root subtree filterと既定100件・最大500件のcursor paginationを追加し、pre-order、repository revisionによるcursor失効、明示的な無制限取得を維持した。
- 独立reviewのP1/P2として、W6-Aではentry種別・permissionを含むcurrent restore、lexer統一、failure phase保持、storage maintenance責務分割を、W6-Bではcardinality境界、iterative走査の保持数上限、実MCP JSON-RPCのtypical/stress測定を解消・固定した。
- Integration gateは`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功した。主要結果はunit 1,328 passed / 1 ignored、backup CLI 18 passed、MCP stdio 14 passed、benchmarking対象1 passedである。

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
- 反復完了の失敗原子性と夏時間を跨ぐ暦日計算を同時に変更しない。
- source scanner削除時に、対応するarchitecture契約を検証なしで失わない。compiler-backedな置換testを先に追加する。

各項目は、既存テストを削除・緩和せず、期待する契約を示すRedテスト、最小のGreen実装、全検証、レビューの順で進める。

## Codexへの依頼方法の例

backlog.mdのWave 1、W1-A〜W1-Jを統括してください。各laneは内部subagentではなく、サイドバーから個別に確認できる新しいCodex taskとして現在のproject内に作成し、それぞれ別branch・別worktreeで実装してください。親taskは各taskの進捗を監視し、backlog記載のwrite範囲と依存関係を管理してください。各laneでは契約単位のRed/Green commit、各Green後のsubagent review、品質ゲート、履歴reviewを実施してください。親taskでも各branchのmain...branch差分とcommit履歴を個別reviewし、問題があれば該当taskへ修正を依頼してください。Wave 2は実装せず、残存作業として報告してください。
各lane taskは初回turnで、backlog.md、write範囲、依存関係を調査し、契約単位のRed/Green commit計画を提示してください。ファイル編集より前に、計画がbacklog契約、repository規則、依存関係、write予約と整合することを自己検査し、通過したready laneは同turnで専用branchの確認から実装へ進んでください。計画提示後に停止したり、親taskの事前承認や再開指示を待ったりしないでください。未充足依存、write競合、契約の曖昧さがあるlaneは何も編集せず、理由を報告して対応を求めてください。

## 追加監査(2026-09-19)

- 監査日: 2026-09-19
- 対象revision: `10fba05df60c04f7fde888874a747ceff0c66c65` (`10fba05d`)
- 更新範囲: 本節の追記だけ。TD-001〜TD-043の完了状況、本文、過去の検証記録、着手順、Wave計画は変更していない。
- 評価基準: 本文冒頭のP0〜P3とS〜XLを引き継ぐ。コードの行数や`unwrap`の存在だけで不具合と判定せず、到達する入力、失敗後の状態、変更時の責務境界、検証の欠落を根拠にする。
- 証拠の区分: `再現済み`は一時storageの製品MCP経路または製品Apps Scriptへの障害注入で観測した結果、`静的確認`はコード・設定・既存testの照合結果を指す。性能劣化、OOM、実Google Sheets上の障害発生を実測したとは扱わない。

### 調査範囲と限界

| 領域 | 調査した境界 | 今回の扱い |
| --- | --- | --- |
| entity / application | task状態・日時・残作業時間、反復、schedule / pack / flatten、一覧・pagination、transaction調停 | 日時演算とschedulerの責務集中を追加。再帰処理、Pending期限計算はTD-041 / TD-043を参照 |
| gateway | YAML decode / encode、repository load / save / revision、transaction prepare / commit / recovery、snapshot / restore、lock、busy time / config | 保存前後の情報保持、日時の再読込、transactionの保持bytesを追加 |
| CLI / MCP | 入力解析、handler・runtime・viewの分担、JSON-RPC lifecycle、入力型、エラー変換、保存経路 | 一時storageでMCPからの読込・更新・再起動を確認。既存のCLI責務・source scanner問題は重複登録しない |
| Web | worker・endpoint、server adapter、clientのrequest / response・mutation safety・localStorage、表示projection、毎秒tick、UI仕様 | workerの障害分類・負荷制御、診断、表示再計算の検証不足を追加 |
| shell / Apps Script | export / import、列定義、task名と日時、segment / task同期、lockと書込 | 実scriptと既存fakeで同期失敗を確認。列契約・日時検証等の既存項目は重複登録しない |
| test / 設定 / CI / 文書 | Cargo featureとworkspace、toolchain、workflow、fixture・contract test、READMEとWeb両仕様 | WebのCI漏れを追加。巨大test、独自source解析、part1 / part2分割はTD-015 / TD-036 / TD-042との重複を避ける |

本監査は上記の境界ごとの静的確認、既存test、限定した異常系再現を組み合わせたものであり、全入力・全分岐の網羅証明ではない。依存crateの最新脆弱性DB照合、fuzzing、実Google Sheetsのquota・通信障害、実browserの描画時間・端末別挙動、Linux上の今回の再実行、release性能・ピークRSS計測、`dx build`は実施していない。外部公開・認証・端末間同期・browser tab間の即時同期など、Web要件が明示的に対象外とする機能は欠陥として登録しない。実運用storageは変更していない。

### 今回の検証結果

過去の監査表とは別の実行記録である。環境はmacOS arm64、repository指定のRust 1.97.1。依存解決は`--locked --offline`を用いた。通常testがGreenでも、後述の異常系を保証するものではない。

| 検証コマンド | 結果 | 確認範囲 |
| --- | --- | --- |
| `cargo test --locked --offline -q` | 成功 | 合計1,416 passed / 2 ignored / 失敗0。libraryは1,343 passed / 1 ignored、scheduling fixtureに別途1件ignoredあり |
| `cargo test --locked --offline -q -p schronu-web --features server` | 成功 | 合計276 passed / 失敗0、うちlibrary 124 passed |
| `node --test apps_script/main.test.mjs` | 成功 | 16 passed、失敗0 |
| `cargo fmt --check` | 成功 | format差分なし |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | 成功 | rootの既定packageを検証 |
| `cargo clippy --locked --offline -q -p schronu-web --all-targets --features server -- -D warnings` | 成功 | Web server featureを明示して検証 |
| `cargo check --locked --offline -q -p schronu-web --no-default-features --features web --target wasm32-unknown-unknown` | 成功 | browser専用のWASMコンパイル境界を確認。実browser実行ではない |

#### 異常系再現の共通条件

TD-044〜TD-047は`cargo build --locked --offline --bin schronu-mcp`で用意できる製品binaryを使う。各case専用の空の一時directoryを`SCHRONU_STORAGE_DIR`へ設定し、その下の`project/project.yaml`へ記載のfixtureを置く。`SCHRONU_CONFIG_PATH`は未指定にし、TD-044〜TD-046は`TZ=Asia/Tokyo`、TD-047は`TZ=America/New_York`で実行する。MCPへ次の2行を送って初期化した後、各項目の`tools/call`を1行JSONとして送り、stdinを閉じてstdout、stderr、終了code、保存fileを確認する。

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"audit","version":"1"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
```

各caseの基本fixtureは次のとおり。項目ごとの追加フィールドは`project`の直下へ同じindentで置く。再起動とは、同じ一時storageを指定して新しいMCP processを起動し、同じ初期化を行うことを指す。

```yaml
project:
  name: audit-task
  id: 00000000-0000-4000-8000-000000000001
```

TD-048 / TD-049はNodeの`vm`で`apps_script/main.js`を実行し、`apps_script/test_support/fake_spreadsheet.mjs`と同じSpreadsheet / LockService境界へ失敗を注入した。Google API自体のtransaction保証や障害頻度の実測ではない。

### 新規負債一覧

| ID | 優先度 | 完了状況 | 概算 | 証拠 | 項目 |
| --- | --- | --- | --- | --- | --- |
| TD-044 | P1 | 未着手 | M | 再現済み | 締切計算が受理済みの大きな見積秒数でpanicする |
| TD-045 | P1 | 未着手 | S | 再現済み | 複数文書のproject YAMLを受理し、更新保存で後続文書を消失させる |
| TD-046 | P1 | 未着手 | M | 再現済み | 未知のYAMLフィールドを無視し、更新保存で消失させる |
| TD-047 | P1 | 未着手 | L | 再現済み | 日時保存でoffsetを失い、夏時間終了時の日時を再読込できない |
| TD-048 | P1 | 未着手 | S | 障害注入で再現済み | Apps Scriptがlock競合時の同期中止を通知しない |
| TD-049 | P1 | 未着手 | M | 障害注入で再現済み | Apps Scriptの書込例外で部分同期が残り、失敗範囲の診断と明示的な復旧案内がない |
| TD-050 | P1 | 未着手 | M | 静的確認 | CIがWebのfeature別test・clippy・WASM境界を検証しない |
| TD-051 | P1 | 未着手 | M | 静的確認・既存test | Web workerが未実行と結果不確実を同じretry可能errorにする |
| TD-052 | P2 | 未着手 | M | 静的確認 | Web workerのqueue容量と待機時間が無制限である |
| TD-053 | P2 | 未着手 | M | 静的確認 | Webの一般化されたエラーからserver側の原因を追跡できない |
| TD-054 | P2 | 未着手 | M | 静的確認・性能未計測 | Webの毎秒tickで非表示画面のmodelも再構築し、性能基準がない |
| TD-055 | P2 | 未着手 | L | 静的確認 | scheduling policyに複数の状態管理責務が集中している |
| TD-056 | P2 | 未着手 | L | 静的確認・RSS未計測 | transaction preflightが適用待ちの全write bytesを保持する |

### TD-044: 締切計算が受理済みの大きな見積秒数でpanicする

- 分類: `バグ / プロセス継続性`
- 優先度: `P1`
- 概算規模: `M`
- 証拠: `再現済み`

#### 現状と根拠・再現条件

- `src/entity/datetime.rs:54-71`は`Duration::seconds`と日時の減算operatorを直接使う。整数として表現可能でもchronoのduration / datetime範囲に収まる保証がない。
- `src/adapter/gateway/yaml.rs:330-336,428`は見積秒数の非負だけを検査し、`src/entity/task.rs:323-359`のstatus再計算が締切計算を呼ぶ。
- 基本fixtureへ`estimated_work_seconds: 9223372036854775807`、`start_time: 2026/01/01 00:00:00`、`deadline_time: 2026/12/31 23:59:59`を追加して`{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_tasks","arguments":{}}}`を実行した。終了codeは101、stderrは`TimeDelta::seconds out of bounds`であり、tool error応答を返さない。

#### 影響

- 形式検証を通る1taskがrepository全体の読込を中断する。Webでも同じdomain処理を使うため、worker停止へ波及する可能性がある。

#### 推奨する改善方針

- duration生成と加減算をchecked化し、task / field / 演算の情報を持つerrorとしてloadとapplication境界へ伝搬する。巨大値を丸めて受理しない。
- duration範囲内でも日時範囲外になるケースを別途扱う。setterの後段で失敗する場合は変更前に検証し、部分状態を残さない。

#### 完了条件

- 上記fixture、duration境界、日時の上下限、CLI / MCPからの見積・締切更新がpanicせず、対象を特定できるerrorを返す。
- 失敗後のtask属性・revision・diskを変更せず、同一MCP processが後続requestへ応答できる。

#### 依存関係

- TD-027の残作業時間の整数演算、TD-043のPending上限の計算式とは別契約。修正を一括りにしない。TD-051は停止原因を直しても必要なworker側の防御である。

### TD-045: 複数文書のproject YAMLを受理し、更新保存で後続文書を消失させる

- 分類: `バグ / データ保全`
- 優先度: `P1`
- 概算規模: `S`
- 証拠: `再現済み`

#### 現状と根拠・再現条件

- `src/adapter/gateway/task_repository/load.rs:139-166`は全YAML文書をparseした後、`docs.first()`だけを採用する。文書数を検証しない。
- 基本fixtureの末尾へ、indentなしの`---`に続けて別の`project` mappingを追加する。2件目の名前は`second-document`、UUIDは`00000000-0000-4000-8000-000000000002`とする。`list_tasks`は成功して先頭の1件だけを返す。
- 先頭taskへ`{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"update_task","arguments":{"task_id":"00000000-0000-4000-8000-000000000001","estimated_work_minutes":20}}}`を実行すると成功するが、保存fileから2文書目が消える。`src/adapter/gateway/task_repository.rs:396,472-489`は読み込んだprojectだけを再serializeする。

#### 影響

- 読込成功が保存時の情報保持を保証せず、手動結合等で混入した別projectを更新時に消失させる。

#### 推奨する改善方針

- project fileは1文書という契約をload時に検証し、余分な文書をfile pathと文書位置付きで拒否する。空文書も黙って無視しない。
- 複数projectの自動分割やmigrationはこの修正へ混ぜず、元fileを保持したまま修復を案内する。

#### 完了条件

- 1文書は従来どおり読め、複数文書・末尾の空文書はread / 検証 / backupのstrict検証で一貫して拒否される。
- load失敗でmemoryや元fileを変更せず、更新commandが保存へ進まない。

#### 依存関係

- TD-003の既知fieldの値検証、TD-021の読込済みtaskのUUID一意性とは別の文書構造契約。TD-046と同じYAML周辺を触るが、Red理由とcommitを分離する。

### TD-046: 未知のYAMLフィールドを無視し、更新保存で消失させる

- 分類: `バグ / データ保全・schema境界`
- 優先度: `P1`
- 概算規模: `M`
- 証拠: `再現済み`

#### 現状と根拠・再現条件

- `src/adapter/gateway/yaml.rs:291-459`のstrict decodeは既知キーを個別に読むが、mapping全体の未知キーを検査しない。`task_snapshot_to_yaml_recursive`は既知属性だけを出力する。
- 基本fixtureのproject直下へ`custom_metadata: keep-me`を追加する。`list_tasks`は成功し、TD-045と同じ`update_task`で見積を20分にすると、保存fileから`custom_metadata`が消える。

#### 影響

- field名のtypoや将来形式のdataを読めたように見せ、次の無関係な更新で失う。現在のserializerが扱えないdataを黙って受理している。

#### 推奨する改善方針

- document rootとtask mappingの許可キーを明示し、未知キーと非文字列キーをfile / task path付きで拒否する。既存の省略可能field・legacy形状を誤って拒否しないようinventoryを先に固定する。
- 現在未定義の拡張fieldを黙って保持・解釈する仕組みは追加せず、読み取れない形式は保存前に止める。既存fileの検査と修復案内を用意する。

#### 完了条件

- root / childの未知キー、typo、非文字列キーを検出し、元fileのbytesを保持する。
- 既存の正常・legacy fixtureが通り、CLI検証、MCP load、snapshotのstrict検証が同じ規則を使う。

#### 依存関係

- TD-003 / TD-037は既知fieldの値とlenient APIの問題であり、本項目はmappingの受理範囲を扱う。TD-045とは文書数とfield集合の別cycleにする。

### TD-047: 日時保存でoffsetを失い、夏時間終了時の日時を再読込できない

- 分類: `バグ / 永続化round-trip`
- 優先度: `P1`
- 概算規模: `L`
- 証拠: `再現済み`

#### 現状と根拠・再現条件

- `src/adapter/gateway/yaml.rs:78,103-127`は各日時をoffsetも小数秒もない`%Y/%m/%d %H:%M:%S`へ変換する。`strict_datetime`(`257-289`)はローカル日時変換の`LocalResult::Single`だけを受理する。
- `TZ=America/New_York`で基本fixtureへ`{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"update_task","arguments":{"task_id":"00000000-0000-4000-8000-000000000001","deadline_time":"2026-11-01T01:30:00-04:00"}}}`を実行すると成功する。
- 同じstorage・TZで再起動して`list_tasks`を呼ぶと、`project.deadline_time: must be a valid local datetime`を含む`repository_load_failed`になる。保存値`2026/11/01 01:30:00`からは夏時間終了前後のどちらかを復元できない。

#### 影響

- 公開APIが受理した正しい日時を保存するだけで、そのprojectを含むrepositoryが次回から読込不能になる。運用timezone変更時にもoffsetを失った形式の解釈が変わる。

#### 推奨する改善方針

- RFC 3339等のoffsetと必要な精度を保持する表現を読み書きへ導入する。新形式のdecodeを先に追加し、旧形式の一意に解釈できる値の読込互換を維持した後にwriterを切り替える。
- 曖昧な旧値を勝手に早い方・遅い方へ丸めない。file / fieldと修復に必要な情報を返し、schema互換・backup / restore・運用手順を同時に文書化する。

#### 完了条件

- 上記offsetと同日の`-05:00`を、それぞれ別のinstantとしてsave / reloadできる。各永続日時fieldと小数秒の保持方針もtestで固定する。
- 旧形式の正常fileは読み込め、曖昧・存在しないローカル時刻は理由を保ったerrorになる。MCP製品経路とbackup / restoreで同じ結果になる。

#### 依存関係

- TD-035は反復の日付移動で壁時計時刻を保つ契約。本項目は保存表現の情報保持であり別件。TD-044〜TD-046とYAML周辺の変更順を調整するが、契約とcommitは分ける。

### TD-048: Apps Scriptがlock競合時の同期中止を通知しない

- 分類: `バグ / 同期の信頼性`
- 優先度: `P1`
- 概算規模: `S`
- 証拠: `障害注入で再現済み`

#### 現状と根拠・再現条件

- `apps_script/main.js:60-63`は`tryLock(1000)`がfalseなら即returnし、通知・再試行・未同期記録を行わない。元sheetの手動編集は既に行われている。
- `vm`へ対象sheet名`実ログ`、row 3・1行のrangeを渡し、DocumentLockの`tryLock`をfalseにした。`onEdit`は例外なく終了し、toast呼出は0回だった。既存fakeの`tryLock`(`apps_script/test_support/fake_spreadsheet.mjs:133-138`)は常にtrueで、この境界を検証しない。

#### 影響

- 編集が短時間に重なると両sheetが不一致のまま残り、利用者にはどちらの値が同期済みか分からない。

#### 推奨する改善方針

- 同期対象の競合時に未同期を通知し、対象task / segmentと手動再同期の方法を示す。再同期は最新のidentityと値を再検証し、古い編集内容を無条件に再送しない。
- lock待ちを無制限に延長せず、未同期時の扱いをApps Scriptの運用文書へ記載する。

#### 完了条件

- lock失敗時に同期先へ書き込まず、利用者が対象と復旧手順を確認できる。
- 成功・失敗・再同期のtestを実`onEdit`経路で固定し、lock解放を保証する。

#### 依存関係

- TD-014の速度最適化ではなく、同項目が別課題としたlock競合の正確性を扱う。TD-033のidentity規則を維持し、TD-049の途中書込失敗とは分ける。

### TD-049: Apps Scriptの書込例外で部分同期が残り、失敗範囲の診断と明示的な復旧案内がない

- 分類: `技術的負債 / 同期の障害回復性`
- 優先度: `P1`
- 概算規模: `M`
- 証拠: `障害注入で再現済み`

#### 現状と根拠・再現条件

- `apps_script/main.js:181-188`はidentityの事前検証後、計画した各cellへ順に`setValue`する。外側の`finally`はlockだけを解放し、書込失敗の分類や再同期の状態を保持しない。
- 両sheetに同一taskのind `0000` / `0001`を置き、実ログのrow 3のR列を`W`へ編集する。既存fakeを読み込み、`優先度低い順`の`writeCell`を例外へ差し替えると、実ログrow 4のR列だけが先に更新された状態で例外となり、toastは0回だった。
- 障害を除去して元cellを再編集すれば既存経路でも全segmentを同期できることを確認した。再同期が不可能なのではなく、失敗範囲の診断と復旧案内がないことが本項目の対象である。

#### 影響

- task単位で一致すべきN / R列やsegment単位の値が部分反映される。入力検証が成功しても同期の完了は保証されず、再試行の対象も分からない。

#### 推奨する改善方針

- 同期計画、書込実行、結果診断を分け、例外時は失敗位置と反映確認が必要な範囲を提示する。I/O例外だけから未反映と断定しない。
- 最新値を読み直して未同期を確認できる再同期経路を用意する。他の手動編集を上書きする無条件rollbackは避け、再実行時もidentityと競合を検証する。

#### 完了条件

- 最初・途中・最後のwriteと結果不明の失敗を注入し、全件成功と部分反映を区別できる。
- 再同期で意図した全segmentが揃い、途中の別編集やidentity変更は診断される。失敗時もlockが解放される。

#### 依存関係

- TD-033の複合keyとtask / segment列の契約を使う。TD-048と復旧UIを共有できるが、lock未取得と書込途中の失敗は別cycleにする。TD-014の実測を覆す根拠なしにbatch化を速度改善として導入しない。

### TD-050: CIがWebのfeature別test・clippy・WASM境界を検証しない

- 分類: `技術的負債 / 検証漏れ`
- 優先度: `P1`
- 概算規模: `M`
- 証拠: `静的確認`

#### 現状と根拠

- `Cargo.toml:6-9`はworkspaceへWebを含める一方、`default-members = ["."]`とする。`.github/workflows/ci.yml:37-47`のtest / clippyはpackageもworkspaceも指定しない。
- `schronu-web/Cargo.toml`のdefault featureは空で、`server` / `web`を明示しなければ両方の製品境界を検証できない。`schronu-web/src/app.rs`にはWASM targetでのみ有効なbrowser moduleもある。
- 今回のWeb server test・clippy・WASM checkは明示的に追加実行して成功したが、これらは現在の通常CIにない。

#### 影響

- root CIがGreenでもWebのコンパイル・挙動の回帰をmerge前に検出できない。

#### 推奨する改善方針

- root gateを維持し、Webのdefault / server / webのfeature境界に対応したtest・clippyと`wasm32-unknown-unknown`のcheckを明示する。nativeの`--all-features`だけでbrowser専用コードを検証したことにしない。
- WASM targetと必要なtool versionを固定し、既存の`dx build`最終gateをCIへ置く場合はcheckとの役割を明示して重複実行を避ける。実browserのsmoke確認はコンパイルとは別に扱う。

#### 完了条件

- WebだけのPRも対象になり、server endpoint testとbrowser専用moduleのコンパイル失敗がCI失敗になる。
- root・Apps Script・benchmarkingの既存gateを維持し、feature間の依存混入を検知できる。

#### 依存関係

- TD-008のroot品質gate整備後に増えたWeb packageの検証範囲を扱う。新規Web修正より先行でき、製品の挙動変更を伴わない。

### TD-051: Web workerが未実行と結果不確実を同じretry可能errorにする

- 分類: `技術的負債 / 障害分類・再送安全性`
- 優先度: `P1`
- 概算規模: `M`
- 証拠: `静的確認・既存test`。commit後のpanicによる実データの二重更新は今回再現していない。

#### 現状と根拠

- `schronu-web/src/web_worker.rs:75-129`はcommand送信失敗と、送信後のresponse channel切断の双方を同じ`unavailable_error`にする。同file`158-164`は常に`RetryAdvice::Retry`を返す。
- workerは`spawn`時に一度生成されるだけで、停止したthreadを同じhandleから復旧する経路はない。`schronu-web/tests/web_worker_contract.rs:108-115`はpanicしたworkerからretry可能errorを返すことを固定している。
- `schronu-web/src/client/state/session_state.rs:741-748`で安全markerを残すのはtransport失敗と`repository_state_uncertain`であり、workerの応答喪失はその分類へ入らない。受理後にworkerが消えたことだけではmutation未実行を保証できない。

#### 影響

- 時間をおいても回復しない障害に再試行を案内する。mutationの結果が不明なケースを安全に再送できるケースと区別できない。

#### 推奨する改善方針

- queueへ送れなかった未実行と、受理後に結果が得られない状態を区別する。後者のmutationでは既存の安全markerを維持し、自動再送しない。
- 停止したworkerにはserver再起動とrepository確認を含む復旧手順を返す。安易なcatch / 再spawnで途中の変更状態を継続しない。
- 現在のretryを肯定する既存testは、未実行と結果不確実の2契約へ置換する。Webのerror契約とUI両文書も同じ変更で揃える。

#### 完了条件

- 送信前停止、受理後停止、mutation後の応答喪失、read失敗を別々に固定する。
- 不確実なmutationではreload後もmarkerが残り、明示確認まで再送されない。恒久停止に単なる待機再試行を案内しない。

#### 依存関係

- TD-041はstack消費の根本原因、本項目は停止時の契約。TD-044でpanic原因を減らしても独立して必要。TD-052の待機期限設計に先行する。

### TD-052: Web workerのqueue容量と待機時間が無制限である

- 分類: `技術的負債 / 資源制御`
- 優先度: `P2`
- 概算規模: `M`
- 証拠: `静的確認`。負荷によるOOMや待機時間超過は未計測。

#### 現状と根拠

- `schronu-web/src/web_worker.rs:66`は無制限の`std::sync::mpsc::channel`を使う。同file`75-129`の各要求はresponseを期限なしで待つ。
- `run_worker`(`133-155`)は全操作を1本のthreadで逐次処理し、呼出元が応答待ちを中止してもqueue内のcommandの破棄方針を持たない。

#### 影響

- 重いscheduleやI/Oが1件停滞すると後続のread / mutationも待ち続ける。複数接続や連続requestで受付済み要求を制限できない。

#### 推奨する改善方針

- 代表負荷でqueue容量と待機時間の基準を定め、async handlerのthreadをblockingさせずに有界queueへ受付する。満杯時は未受付と分かるerrorを返す。
- queued readの取消と、受付済みmutationの継続・結果照会を分ける。timeoutをrollbackや未実行の証拠として扱わず、順序と既存の直列化を維持する。

#### 完了条件

- 停滞するfake operationと連続送信でqueue上限・受付拒否・順序・取消を決定論的に検証する。
- mutation待機切れを安全に再送可能と誤分類せず、後続操作の復旧条件を説明できる。

#### 依存関係

- TD-051の成否分類を先に固定する。stack上限のTD-041、画面再計算のTD-054とは別の資源を扱う。

### TD-053: Webの一般化されたエラーからserver側の原因を追跡できない

- 分類: `技術的負債 / 障害診断`
- 優先度: `P2`
- 概算規模: `M`
- 証拠: `静的確認`

#### 現状と根拠

- `schronu-web/src/app/environment_web_operations.rs:67-75`は設定読込とstorage path解決のerrorを捨て、同じ一般的な設定errorへ変換する。
- `schronu-web/src/controller_error.rs:5-33`もbusy time、path、lock、repository、save等の詳細を公開用messageへ変換するが、その境界に元のsource chainを保存する処理がない。
- 利用者へprivate pathを公開しない既存testは妥当である一方、server側の原因記録を保証するtestはない。

#### 影響

- permission、壊れたYAML、一時的なlock競合などの区別が表示から失われ、利用者へ同じ再試行を案内する。障害時に操作と原因を突き合わせられない。

#### 推奨する改善方針

- adapter境界で操作名・追跡ID・失敗phase・原因をserverの診断sinkへ記録し、その後に公開errorへ変換する。wireへ詳細を直接露出することで解決しない。
- task名・payload・private path等の出力方針を定め、必要な情報だけを記録する。診断sink失敗で元のerrorを上書きしない。

#### 完了条件

- 設定不正、load、lock、保存前失敗、状態不確実の各ケースをfake sinkで識別でき、公開応答へprivate detailが漏れない。
- 1操作の記録を対応付けられ、sink失敗時にも本来のerrorとretry分類を維持する。

#### 依存関係

- TD-051とerror分類を揃えるが、診断導入とmutation安全性は別cycle。CLI / MCPの公開error形式を同時に変更しない。

### TD-054: Webの毎秒tickで非表示画面のmodelも再構築し、性能基準がない

- 分類: `技術的負債 / 表示性能・検証容易性`
- 優先度: `P2`
- 概算規模: `M`
- 証拠: `静的確認・性能未計測`

#### 現状と根拠

- `schronu-web/src/app/component/browser.rs:22,37-53`は毎秒stateを更新し、`BrowserPageModel::from_state_at`を構築する。表示するtabの分岐はmodel構築後にある。
- `schronu-web/src/app/component_models.rs:34-64`はactive tabによらずsession、全一覧row、日付、履歴をprojectする。`schronu-web/src/client/view_projection.rs:157-205`は取得済みrowを走査し、表示用文字列等を生成する。
- rootのscheduling benchmarkはbrowserでのprojection、DOM更新、localStorageの保存費用を対象にしない。

#### 影響

- row数や履歴量による再構築費用が毎秒発生する。現時点で利用者が体感する遅延や具体的な許容上限は未計測であり、最適化の優先度を判断できない。

#### 推奨する改善方針

- small / typical / stressの表示fixtureで、clockだけの更新、一覧受信、検索、tab切替の費用を分けて測る。
- 時計依存の進捗・bufferと静的な一覧・履歴を分離し、該当dataが変わった時だけ後者を再構築する。最適化前に再計算回数と実browserの所要時間の基準を記録する。

#### 完了条件

- clockだけのtickでは一覧・履歴を不要に再projectせず、data変更時は確実に更新することを製品経路のtestで固定する。
- 進捗、buffer、carry lock期限、timezone表示、選択状態、一覧復元を維持し、実browser測定条件と結果を残す。

#### 依存関係

- TD-012はserver側のschedule / pack / flatten費用であり対象が異なる。TD-042のtest配置変更とは独立させる。TD-050のCI整備を先行すると回帰を検知しやすい。

### TD-055: scheduling policyに複数の状態管理責務が集中している

- 分類: `技術的負債 / 保守性・責務境界`
- 優先度: `P2`
- 概算規模: `L`
- 証拠: `静的確認`

#### 現状と根拠

- `src/application/scheduling_policy.rs`は対象revisionで2,570行ある。行数だけでなく、`SchedulerFrontier`(`331`)、`SlackDemandIndex`(`371`)、`SlackRangeTree`(`397`)が別々の状態管理を同居させている。
- 同fileの候補選択(`2000`)、先読み選択(`2083`)、`PolicyState`(`2123`)、segment生成(`2431`)までが同一moduleにあり、privateな状態の変更範囲が広い。引数数のlint抑制も残る。

#### 影響

- 区間木の更新、先読みの一時変更と復元、業務上の選択規則を同時に追う必要があり、局所的な修正でもreview範囲が広がる。

#### 推奨する改善方針

- interval / slack index、frontier、先読みの変更・復元、policy orchestrationを責務別のprivate moduleへ分ける。機械的移動では型・error・公開API・選択順を変更しない。
- 同型のhelperやfixtureを複製せず、状態更新を所有する側へ閉じ込める。アルゴリズム変更や高速化は移動後の別cycleにする。

#### 完了条件

- 同時刻のtie-break、deadline、atomic / fixed task、分割segment、先読みの復元が移動前後で一致する。
- 通常testとbenchmarkingのcounter / 同値性契約がGreenであり、各moduleの所有状態と依存方向を説明できる。行数だけを減らすtest削除は行わない。

#### 依存関係

- TD-018はCLI orchestration、TD-019は計測境界、TD-041は再帰stackの問題。本項目は現在のscheduler内部の責務分離であり、それらの完了状況を変更しない。

### TD-056: transaction preflightが適用待ちの全write bytesを保持する

- 分類: `技術的負債 / 永続化の資源制御`
- 優先度: `P2`
- 概算規模: `L`
- 証拠: `静的確認・RSS未計測`

#### 現状と根拠

- `src/adapter/gateway/storage_transaction/commit.rs:177-303`は各staged fileを読み、`PreflightEntry::Write { bytes: Vec<u8>, ... }`として全件をcollectしてから適用する。検証途中にはlive targetのbytesも読む。
- `src/adapter/gateway/task_repository.rs:483-548`のsave経路はserialize済みの全変更projectも`prepared_writes`に保持する。通常saveと再起動recoveryでは保持物が異なるため、別々の計測が必要である。
- TD-022の残存非対象に記載された「全staged bytesを保持するpreflightのmemory最適化」を、独立して着手できる詳細項目として具体化した。TD-022の内容・完了状況は変更しない。

#### 影響

- 保存対象総量に比例した追加memoryを必要とする。大規模restore / recoveryでのピークRSSと許容範囲は今回未計測であり、OOMを再現済みとは扱わない。

#### 推奨する改善方針

- 通常save、未適用transactionのrecovery、部分適用済みrecoveryを分け、file数・総bytes・最大file sizeに対する保持量を計測する。
- 全entryのintegrity検証が完了する前にlive dataを変更しない契約を維持する。immutable staged materialの同一性を保証した二段階処理等で保持量を抑え、適用時に別bytesへ差し替わる問題を持ち込まない。
- 受付上限を設ける場合はmarker公開前に検証する。既にcommit済みのtransactionを新上限だけで回復不能にせず、旧transactionのrecovery互換も設計する。

#### 完了条件

- aggregate sizeに対する保持bytes / RSSを測定し、設計した上限または最大単一file等に基づく保持量を検証できる。
- 全件preflight、checksum不一致、適用済みtarget、staged欠落、permission、revision後置、crash後のidempotentなroll-forwardの既存契約を維持する。

#### 依存関係

- TD-022のtransaction protocolを前提とする。TD-039のsnapshot固有resource limitとは別境界であり、TD-041のstack消費とも分ける。既存failure injectionを再利用する。

### 追加項目の推奨着手順と検証方針

1. TD-044〜TD-047のデータ保全・再読込可能性を優先する。YAMLに集中するが、日時演算、文書数、field集合、保存形式を別々のRed / Green cycleにする。
2. TD-048 / TD-049を同期失敗の契約として個別に進める。TD-050は独立して先行し、TD-051のWeb安全停止をCIで検証できるようにする。
3. TD-051の未実行 / 結果不確実の分類を固定してからTD-052の負荷制御へ進む。TD-053の診断は同じ分類に対応付ける。
4. TD-054 / TD-056は計測と上限設定を先に行い、未測定の性能改善を目的に契約を緩めない。TD-055は機械的移動を独立させ、機能変更と同時に行わない。

この順序は本節の追加項目だけを対象とし、既存Wave計画の更新ではない。各修正では製品経路の回帰test、期待した単一理由のRed、最小Green、品質gate、reviewを契約単位で実施する。旧挙動を肯定する既存testは検索し、維持・反転・置換の判断をcommit計画へ記録する。今回の追記自体は製品testや仕様を変更せず、修正完了を示すものではない。

## 追加監査項目の並列開発計画(2026-09-19)

対象は追加監査のTD-044〜TD-056、計画基準はrevision `10fba05d`とする。目的は、データ保全と障害時の契約を先に確定し、file所有権を分けて独立領域を並列開発することである。Rust本体、Dioxus / TokioによるWeb、Apps Script、GitHub Actionsを対象とする。既存のTD本文・完了状況・監査記録・Wave 0〜7は変更しない。本節は将来の実行計画であり、task作成、実装開始、修正完了の記録ではない。

### 依存関係と実行単位

- `H`: 契約上の依存。前提となる契約commitが`main`へmergeされるまで後続を実装しない。
- `S`: 変更fileまたは契約の伝搬先の競合を避ける直列化。意味上の必須依存とは区別し、先行merge後に後続を最新`main`へ合わせる。
- `G`: 品質gateの前提。Webの実装laneはTD-050のfeature別CIが`main`で利用できる状態から始める。
- `I`: 両変更を合わせた状態で確認する境界横断契約。個別testの成功だけでは代替しない。

```text
TD-044 ─S─> TD-045 ─S─> TD-046 ─S─> TD-047
TD-044 ─S─> TD-055
TD-045 ─S─> TD-056
TD-046 ─I─ TD-056

TD-048 ─S─> TD-049

TD-050 ─G─> TD-051 ─H─> TD-052 ─S─> TD-053 ─S─> TD-054
                └─────────H────────> TD-053
```

TD-044は日時helperだけで閉じず、task setter、use case、YAML読込へのerror伝搬が必要になり得る。YAML系とschedulerの構造変更は、この伝搬先を確定してから始める。TD-045〜TD-047はloader / YAML / 回帰testの所有権、TD-048→TD-049は`apps_script/main.js`とtestの所有権による直列化である。TD-045→TD-056はrepositoryと保存・再読込testの変更範囲を整理するための直列化である。

TD-051→TD-052は「未受付」と「受付後の結果不確実」を負荷制御に適用するための依存、TD-051→TD-053は同じ分類で診断を対応付けるための依存である。TD-052以降のWeb laneは共通のmodule配線、test、UI仕様を順番に所有する。TD-054に診断機能そのものの意味上の依存はないが、本計画では安全停止・待機方針が安定した後に性能改善する。

各Wave内に`H` / `S` / `G`の待ち合わせは置かず、開始条件を満たしたlaneを並列実行する。最大並列数は4であり、少ない担当数ならlane単位で順番に実行できる。既存Waveの未完了表示だけを理由に待機したり、完了表示だけで前提充足を判断したりせず、特にTD-022 / TD-033の必要な契約が`main`にあることを確認する。

### 共通の予約・開発規則

1. 実行対象として明示されたWaveだけを扱う。実行時は保存済みproject内の別task・別worktree・`feature/<lane>-<short-name>`へ各laneを割り当てる。初回turnはread-only調査とcommit計画だけとし、編集、branch作成、commit、push、PR作成を行わない。親が依存とwrite予約を確認してから実装を開始する。
2. 所有権はfile全体を単位にする。下表の予約は初期範囲であり、初回調査で実際の製品・test・fixture・文書fileを列挙して確定する。新規fileも所有laneを予約する。予約外の変更が必要なら該当範囲を止め、親が予約と依存を再確認する。行が異なることや別worktreeであることは同じfileの同時編集を許す理由にしない。
3. `Cargo.toml`、`Cargo.lock`、`schronu-web/Cargo.toml`、`src/lib.rs`、`src/entity.rs`、`src/application.rs`、`src/adapter/gateway.rs`、共通test helperは暗黙の共有編集対象にしない。下記で明示された配線file以外は親が排他予約する。複数laneで必要と判明した場合は実装前に直列化する。並列化のためだけにhelperを複製しない。
4. `README.md`、`apps_script/README.md`、`docs/design/schronu_web_ui_requirements.md`、`docs/design/schronu_web_ui_specification.md`は文書leaseで一度に1 laneだけが編集する。製品実装と同じPR内に必要な仕様変更を含め、文書commitは分ける。Web改修前には両UI文書を読み、error、待機、復旧、tickの利用者向け契約を同期する。
5. `backlog.md`も排他leaseとし、現時点の指示では既存項目と本計画を更新しない。各laneの証跡は末尾の実行記録へ追記する。既存statusの変更は別途明示された場合だけ行う。共有のWave検証summaryは下記の担当laneが統合gate後に末尾へ追記し、他laneは重複して書かない。
6. 各laneは契約単位でcommit message、責務・module、先行commit、対象test、想定する単一のRed理由、Green確認commandを計画する。原則としてRed test commit→最小Greenと全gate→Green commit→内部subagent review→指摘別修正commitと再検証の順にする。旧挙動を肯定するtestは維持・反転・置換の判断を記録する。fast pathは`AGENTS.md`の条件を実際に満たす場合だけ使う。
7. 挙動を変えないmodule移動や計測準備は、無理にRedを作らず独立したGreen commitにする。API・error・保存形式の変更と機械的移動を混在させない。800行を超えるfileや巨大fixtureは行数だけで判断せず、所有状態・責務・変更容易性をreviewする。

### Wave 8: 読込panic、同期中止通知、Web CIの基盤(最大3レーン)

開始条件: 対象revision以後の`main`差分と関連契約を再確認し、各laneの予約を確定する。Wave 8内のlane間依存はない。

| lane / 項目 | 予定branch | 製品・設定の初期write予約 | test・文書の初期予約 | 契約とcycle |
| --- | --- | --- | --- | --- |
| W8-A / TD-044 | `feature/w8-a-checked-deadlines` | `src/entity/datetime.rs`、`src/entity/task.rs`、`src/application/task_use_case.rs`、`src/application/flatten_use_case.rs`、`src/adapter/gateway/yaml.rs`。error伝搬先は初回調査で追加予約 | 同file内test、`src/application/task_use_case_tests.rs`、`src/adapter/gateway/yaml_tests.rs`、`tests/mcp_stdio.rs` | Duration生成の範囲外と日時演算の範囲外を別cycleで固定する。読込・更新ともpanicせず理由付きerrorを返し、失敗時の保存dataが不変である |
| W8-B / TD-048 | `feature/w8-b-sheet-lock-feedback` | `apps_script/main.js` | `apps_script/main.test.mjs`、`apps_script/README.md` | lock取得失敗の通知を先に固定し、続いて明示的な再同期操作を追加する。再同期時は現在のidentityと値を再検証し、古い編集値を遅延適用しない |
| W8-C / TD-050 | `feature/w8-c-web-ci-gates` | `.github/workflows/ci.yml`。必要なら専用Web workflowを新設 | workflow設定そのもの、`schronu-web/tests/web_feature_boundary_contract.rs`、必要なCI説明文書 | default / server / webを明示してtest・lintし、別jobでWASM checkする。rootの既存gateを残す。feature境界の失敗がCI失敗になることを確認する |

- W8-Aの対象確認: `cargo test --locked entity::datetime`、`cargo test --locked entity::task`、`cargo test --locked --test mcp_stdio`。巨大な受理可能i64、日時境界、通常値を含め、単に値を丸めて通過させない。
- W8-Bの対象確認: `node --test apps_script/main.test.mjs`と`cargo test --locked --test spreadsheet_contract`。lockを取得できないfakeと、再同期のidentity不一致を含める。
- W8-Cの対象確認: 下記のWeb gateをworkflowの各jobへ対応付ける。実行環境に必要なtarget・toolchain・cacheを設定する。公開前はlocalで各commandとworkflow設定を検証し、push / PR後に実CI結果を記録する。設定追加やlocal成功だけでCI成功と扱わない。
- 統合gate: root gate + Apps Script gate + Web feature / WASM gate。日時errorの伝搬がWeb server側のbuildを壊していないことも確認する。
- PR merge / 文書lease順: W8-A→W8-B→W8-C。共有summary担当はW8-C。次段階は各前提が`main`へ入ってから着手する。

### Wave 9: YAML文書数、部分同期、worker安全停止、scheduler分割(最大4レーン)

開始条件: W9-A / W9-DはW8-A、W9-BはW8-B、W9-CはW8-Cが`main`へmerge済みであること。W9-CはW8-A後の日時error伝搬も取り込んでbuild可能であることを確認する。

| lane / 項目 | 予定branch | 製品の初期write予約 | test・文書の初期予約 | 契約とcycle |
| --- | --- | --- | --- | --- |
| W9-A / TD-045 | `feature/w9-a-single-yaml-document` | `src/adapter/gateway/task_repository/load.rs`、必要な`src/adapter/gateway/task_repository.rs`のtest配線 | `src/adapter/gateway/task_repository_tests.rs`、新規`tests/yaml_document_contract.rs`、`tests/storage_snapshot_contract.rs` | 通常の複数文書と空の後続文書を読込段階で拒否する。CLI / MCP / snapshot入口の実製品経路で、error後に元bytesが不変である |
| W9-B / TD-049 | `feature/w9-b-sheet-partial-recovery` | `apps_script/main.js` | `apps_script/main.test.mjs`、`apps_script/README.md` | 第n書込の例外と完了範囲の診断を固定し、W8-Bの再同期経路へ接続する。再編集での回復も維持し、競合編集を盲目的なrollbackで上書きしない |
| W9-C / TD-051 | `feature/w9-c-worker-outcome-safety` | `schronu-web/src/web_worker.rs`、`schronu-web/src/controller_error.rs`、`schronu-web/src/wire.rs`、`schronu-web/src/client/state/session_state.rs`、`schronu-web/src/client/safety_state.rs`、`schronu-web/src/lib.rs`、`schronu-web/src/app.rs`、`schronu-web/src/app/environment_web_operations.rs` | `schronu-web/tests/web_worker_contract.rs`、`schronu-web/tests/wire_contract.rs`、`schronu-web/tests/client_state_response_contract.rs`、両UI文書 | enqueue前の停止と受付後の応答喪失を別cycleで固定する。mutation結果不確実ではsafety markerを保持し、確認・復旧まで変更を止める。無条件Retryを肯定する既存testを見直す |
| W9-D / TD-055 | `feature/w9-d-scheduling-modules` | `src/application/scheduling_policy.rs`、新規`src/application/scheduling_policy/`配下、必要な`src/application.rs`の配線 | `src/application/scheduling_policy_tests.rs`、`tests/scheduling_benchmark_contract.rs`、`tests/scheduling_fixture_contract.rs`。`tests/support/scheduling_*`はまずread-only | frontier、slack index / tree、先読みの変更・復元、segment生成を状態所有者ごとの機械的移動commitに分ける。型・error・公開API・選択順を維持する |

- W9-Aの対象確認: `cargo test --locked --test yaml_document_contract`、`cargo test --locked --test storage_snapshot_contract`。新規test fileは通常のCargo統合testとして自動検出させ、他laneのtest配線fileを借りない。
- W9-Bの対象確認: Apps Script gate。最初・途中・最後の書込失敗、lock解放、通知、再実行、再同期までに別編集が入る場合を検証する。APIの一括置換は性能計測なしで含めない。
- W9-Cの対象確認: `cargo test --locked -p schronu-web --features server --test web_worker_contract`とWeb gate。非受付、worker panic、処理完了後の応答喪失でmutationを重複実行しないことを確認する。
- W9-Dの対象確認: `cargo test --locked --test scheduling_fixture_contract`、`cargo test --locked --features benchmarking --test scheduling_benchmark_contract`、`cargo test --locked --test capacity_integration_gate`。移動前後の出力・tie-break・counterを比較し、アルゴリズム改善は含めない。
- 統合gate: root + Apps Script + Web + schedulingの各gate。W9-Aの読込errorをW9-CのWeb経路が安全に扱うこと、W8-A後のdeadline計算とW9-Dのschedule結果が整合することを確認する。
- PR merge / 文書lease順: W9-A→W9-B→W9-C→W9-D。共有summary担当はW9-D。

### Wave 10: 未知field、worker負荷制御、transaction保持量(最大3レーン)

開始条件: W10-A / W10-CはW9-A、W10-BはW9-Cが`main`へmerge済みであること。W10-CではTD-022の全件preflight・roll-forward契約を確認する。

| lane / 項目 | 予定branch | 製品の初期write予約 | test・文書の初期予約 | 契約とcycle |
| --- | --- | --- | --- | --- |
| W10-A / TD-046 | `feature/w10-a-yaml-field-validation` | `src/adapter/gateway/yaml.rs`、`src/adapter/gateway/task_repository/load.rs` | `src/adapter/gateway/yaml_tests.rs`、`src/adapter/gateway/task_name_yaml_contract_tests.rs`、新規`tests/yaml_unknown_field_contract.rs` | document root / project / task / childの許可キーと旧形式を整理し、未知キーを位置付きで拒否する。mapping境界ごとにRed / Greenを分け、既知fieldの読込互換を維持する |
| W10-B / TD-052 | `feature/w10-b-worker-admission` | `schronu-web/src/web_worker.rs`、`schronu-web/src/controller_error.rs`、`schronu-web/src/wire.rs`、`schronu-web/src/client/state/session_state.rs`、`schronu-web/src/lib.rs`、`schronu-web/src/app.rs`、`schronu-web/Cargo.toml`、`Cargo.lock` | `schronu-web/tests/web_worker_contract.rs`、`schronu-web/tests/client_state_response_contract.rs`、両UI文書 | bounded受付とbusy拒否を先に固定し、次に待機期限を定義する。受付前拒否と受付後timeoutを混同せず、read取消とmutation結果不確実を分離する |
| W10-C / TD-056 | `feature/w10-c-transaction-memory` | `src/adapter/gateway/storage_transaction.rs`、`src/adapter/gateway/storage_transaction/`配下、`src/adapter/gateway/task_repository.rs`。`task_repository/load.rs`は予約外 | `src/adapter/gateway/storage_transaction_tests.rs`と同名directory、`src/adapter/gateway/storage_transaction_test_support.rs`、`src/adapter/gateway/task_repository_tests/transaction/`配下、新規`tests/storage_transaction_memory_contract.rs` | 通常saveとrecoveryを計測してから保持量の目標を決める。immutableな検証対象の同一性、全件検証後の適用、旧transactionのrecovery互換を別cycleで固定する |

- **Wave 10の排他境界**: W10-Aは`task_repository.rs`、`task_repository_tests.rs`、transaction配下を編集しない。W10-Cは`yaml.rs`、`yaml_tests.rs`、`task_repository/load.rs`、W10-Aの新規統合testを編集しない。既存testの登録や共通helperに両laneの変更が必要なら、そのfileを初回計画で直列化してから実装する。Web manifest / lockfileはW10-Bだけが所有し、W10-Cのdependency追加が必要なら同時編集を止めて再計画する。
- W10-Aの対象確認: `cargo test --locked --test yaml_unknown_field_contract`とYAML単体test。位置診断、nested child、既知のlegacy field、未知キー拒否後の元bytes保持を確認する。
- W10-Bの対象確認: Web gate。決定的にworkerを停止・待機させるtestで容量境界、受付順序、queue待機・実行中の期限、応答喪失、終了時の待機者を確認する。実時間sleepだけに依存する不安定なtestを避ける。
- W10-Cの対象確認: `cargo test --locked storage_transaction`、`cargo test --locked --test storage_transaction_memory_contract`、`cargo test --locked --test storage_snapshot_contract`、`cargo test --locked --test storage_backup_cli_contract`。破損が末尾entryにある場合もlive dataを変更せず、部分適用済み状態からの再開を維持する。RSSと保持bytesは区別し、file数・総量・最大file sizeを記録する。
- `I` / 統合gate: root + Web + storage gate。W10-Aの未知field拒否とW10-Cのsave / recoveryを合成する。未commitの通常load / saveでは未知field拒否による非更新を確認する。marker公開済みtransactionでは既存契約どおりintegrity検証・roll-forward・revision反映を先に行い、回復後のbytesをstrict loadが拒否する場合も回復処理を巻き戻さない。正常なprojectの保存・再読込、snapshot / restoreの整合性も確認する。検証用の追加testが必要なら新規`tests/yaml_transaction_integration_contract.rs`をW10-Cが所有し、W10-Aの予約を侵さない。
- PR merge / 文書lease順: W10-A→W10-B→W10-C。共有summary担当はW10-C。

### Wave 11: 日時の保存互換とWebの障害診断(最大2レーン)

開始条件: W11-AはW10-A、W11-BはW9-C / W10-Bが`main`へmerge済みであること。W11-AはW10-C後の保存経路も取り込んだ状態で互換testを行う。

| lane / 項目 | 予定branch | 製品の初期write予約 | test・文書の初期予約 | 契約とcycle |
| --- | --- | --- | --- | --- |
| W11-A / TD-047 | `feature/w11-a-persist-datetime-offset` | `src/adapter/gateway/yaml.rs`、必要な`src/entity/datetime.rs`とrepositoryの日時decode / encode呼出箇所 | `src/adapter/gateway/yaml_tests.rs`、`tests/mcp_stdio.rs`、`tests/storage_snapshot_contract.rs`、`tests/storage_backup_cli_contract.rs`、新規`tests/yaml_datetime_roundtrip_contract.rs`、保存形式の関連文書 | 旧形式とoffset付き形式を読むreaderを先にGreenにし、別cycleでoffsetを保持するwriterへ切り替える。全永続化日時の往復、端数秒、旧形式の曖昧時刻を勝手に選ばない方針を固定する |
| W11-B / TD-053 | `feature/w11-b-web-error-diagnostics` | `schronu-web/src/controller_error.rs`、`schronu-web/src/app/environment_web_operations.rs`、`schronu-web/src/web_worker.rs`、`schronu-web/src/lib.rs`、`schronu-web/src/app.rs`、必要なWeb manifest / lockfile | Web server側test、新規`schronu-web/tests/server_diagnostics_contract.rs`、両UI文書 | 操作・phase・原因chainの対応をserver側に記録する。外向き表示を一般化したまま、相関情報で診断できる契約を固定する。秘密・task本文等の記録方針とsink失敗時の挙動を検証する |

- W11-Aの対象確認: `cargo test --locked --test yaml_datetime_roundtrip_contract`、MCPとsnapshot / backupのtest。`TZ=America/New_York`のDST終了時の2つのoffset、通常日、旧形式、別TZ再読込を独立processで検証する。reader更新前の旧binaryへ戻す際の制約も文書化する。
- W11-Bの対象確認: `cargo test --locked -p schronu-web --features server --test server_diagnostics_contract`とWeb gate。config / repository / workerの原因対応、未受付 / 結果不確実の分類、診断sink失敗が元errorを隠さないことを確認する。
- 統合gate: root + Web + storage gate。日時読込拒否をWeb経路で発生させ、利用者への安全な表示とserver側の原因特定を両立する。W11-Bが境界横断testを所有し、日時fixtureの仕様をW11-Aと照合する。
- PR merge / 文書lease順: W11-A→W11-B。共有summary担当はW11-B。

### Wave 12: Web描画の計測と時計依存更新の分離(1レーン)

開始条件: W11-Bが`main`へmerge済みであり、先行Web変更のclient安全状態とUI仕様が固定されていること。TD-047後の日時往復契約も統合検証に含める。

| lane / 項目 | 予定branch | 製品の初期write予約 | test・文書の初期予約 | 契約とcycle |
| --- | --- | --- | --- | --- |
| W12-A / TD-054 | `feature/w12-a-clock-projection-cost` | `schronu-web/src/app/component/browser.rs`、`schronu-web/src/app/component_models.rs`、`schronu-web/src/app/component_runtime.rs`、`schronu-web/src/client/view_projection.rs`、`schronu-web/src/app.rs`、関連view module | `schronu-web/src/app/projection_boundary_tests.rs`、`schronu-web/src/app/component_tests*.rs`、`schronu-web/tests/view_projection_contract.rs`、必要なview fixture、両UI文書 | baseline計測を先に記録する。clockだけのtick、data受信、検索、tab切替を別々に測り、静的projectionの再利用と時計依存部分の更新を分離する |

- 計測準備、不要な再計算の回帰test、最小の再計算制御、実browser比較を別cycleにする。毎秒tickを止めるだけの修正や、古い一覧・履歴を表示し続けるcacheは完了としない。
- 対象確認: Web gate、`cargo test --locked -p schronu-web --features web --test view_projection_contract`、component / projectionの製品経路test。small / typical / stressについて件数、browser、build mode、計測回数、所要時間と再計算回数を記録する。
- 統合gate: root + Web gateと実browser検証。進捗、buffer、carry lock期限、timezone、選択・一覧復元、操作失敗後の安全停止を確認し、変更時には一覧・履歴が更新されることを検証する。
- PR / 文書lease / 共有summary担当はW12-A。完了後はTD-044〜TD-056の契約・検証証跡を一覧で追記し、未検証・残存作業を明記する。既存項目のstatusは変更しない。

### 実行時の共通品質gate

root gateは各実装の`AGENTS.md`に従うタイミングと、使い捨てintegration worktreeで実行する。

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
git diff --check
```

Apps Script gate:

```bash
node --test apps_script/main.test.mjs
cargo test --locked --test spreadsheet_contract
```

Web feature / WASM gate:

```bash
cargo test --locked -p schronu-web
cargo test --locked -p schronu-web --no-default-features --features server
cargo test --locked -p schronu-web --no-default-features --features web
cargo clippy --locked -p schronu-web --all-targets --no-default-features -- -D warnings
cargo clippy --locked -p schronu-web --all-targets --no-default-features --features server -- -D warnings
cargo clippy --locked -p schronu-web --all-targets --no-default-features --features web -- -D warnings
cargo check --locked -p schronu-web --no-default-features --features web --target wasm32-unknown-unknown
```

Web製品変更のあるlaneでは、範囲確定後のWASM checkをcache warmupとしてbackgroundで1回先行実行し、終了codeと出力を回収する。同じworktreeではその間に別Cargo commandを実行しない。変更後の最終gateは別途実行し、`~/.cargo/bin/dx build --locked --web --package schronu-web`を最終品質gateで1回行う。target / dxが未準備、browser未検証、CI未実行等は成功扱いせず、結果と未検証範囲を残す。

### 統合・公開・実行記録

各Waveの全laneが契約単位のreview、品質gate、親による`main...branch`の累積差分と履歴reviewを通過してから、現在の`main`を基点とする使い捨てintegration worktreeへ表記順で合成する。Wave内にhard dependencyがないため、未完了laneを残してWave全体の成功を宣言しない。semantic conflictは統合側だけで直さず、所有laneに戻して修正・再reviewし、統合状態を作り直す。

各laneの実行記録にはtask / worktree / branch、基点と最終revision、予約file、Red / Green・review履歴、実行commandと結果、互換性、未検証・残存作業、依存commitを記載する。共有summary担当は統合結果だけを最後に追記する。文書追記後にも差分・文書検査と文書を入力とするgateを確認する。

公開前の親reviewとlocal・統合gateの成功後にだけ、実行依頼の範囲内で通常pushと`main`向けPR作成へ進む。push / pull_requestで起動する実CIは公開後のgateとし、CI成功確認前にはmerge可能と報告しない。CI失敗は所有laneへ戻して修正・再検証する。mergeはユーザーが行う。先行PRのmergeで後続branchが古くなった場合はrebase、影響する検証と全gate、親review、最新headのCI確認を済ませてからmerge可能と報告する。共有`backlog.md`への追記も上記のmerge順で維持する。次Waveの開始条件は報告するが、実行を明示されていないWaveのtask、branch、実装、PRを先回りして作成しない。

## 追加監査項目実行記録

### W8-A / TD-044: checked deadline calculations

- 状態: local実装・内部review・親review完了。PR #470は作成済み。CI修正commitは未push、最新CI未実行、未merge。
- task / worktree / branch: W8-A / `/Users/sakakibaratakafumi/.codex/worktrees/9355/Schronu` / `feature/w8-a-checked-deadlines`。
- revision: 基点`566a5e238a6c9e9cd923b18f7571308acb3de6b0`、CI修正を含む実装最終revision`a9544c9c`。
- 予約file: `src/entity/datetime.rs`、`src/entity/task.rs`、`src/application/task_use_case.rs`、`src/application/flatten_use_case.rs`、`src/adapter/gateway/yaml.rs`、同file内test、`src/application/task_use_case_tests.rs`、`src/adapter/gateway/yaml_tests.rs`、`tests/mcp_stdio.rs`。親reviewの指摘対応として`tests/support/persistent_storage.rs`、`tests/cli_runtime_contract.rs`、`tests/task_name_cli_contract.rs`を追加予約した。

#### 固定した契約

- 受理済みの巨大な見積秒数について、chronoのDuration生成範囲外とDateTime加減算範囲外を別errorとして扱い、値を丸めたり黙って受理したりしない。
- YAML読込、CLI / MCPの見積更新、予約、親から子孫へのdeadline伝搬はpanicせず、task ID、field、operation、失敗理由を保持したerrorを返す。
- 失敗時はtask属性、persistent mutation revision、storage bytesを変更しない。MCPはerror後も同一processで後続requestへ応答する。
- 予約による自己・子孫のdeadline候補は最初のwrite前に全件検証し、完了済みtaskを伝搬境界とする既存契約を維持する。
- disk不変検証はstorage配下のregular fileを任意深度まで相対pathで収集し、process用のroot `.lock`だけを除外する。`.transactions/<id>/...`等の深い変更も検出する。

#### Red / Green履歴

- `f018c576`でDuration範囲外のYAML / MCP panicをRed化し、`eafef017`で理由付きerrorへ変更した。
- `4c5d0728`でdeadline日時減算overflowをRed化し、`a2e56de5`でchecked subtractionへ変更した。
- `020f5570`で親deadline伝搬時の未検証子孫をRed化し、`a74ea8ed`で全候補の事前検証へ変更した。
- `847a29bd`でchrono Durationの受理境界、`50811c66`でCLIのDuration範囲外更新を固定した。
- `5d8e2c69`で予約時のDuration panicをRed化し、`bc11e663`でDuration生成をchecked化した。
- `21d484ec`で予約時のDateTime加算panicをRed化し、`2c99bec9`でchecked additionと自己・子孫の事前検証へ変更した。`6e2a886e`で子孫伝搬の失敗原子性を追加固定した。
- 親reviewのP2指摘に対し、`d366fcb5`で3箇所に重複していたstorage snapshot helperを再帰的な共通helperへ集約し、深いtransaction階層を検出する回帰を追加した。
- PR #470の初回CI run `35456014875`では、`tests/mcp_stdio.rs`の新規CLI test 4件がLinuxで失敗した。CLI子processが`SCHRONU_CONFIG_PATH`を指定せず、localのprivate busy-time fileへ暗黙依存していたことを、欠損busy fileを指すconfigの継承で4件ともRedとして再現した。`a9544c9c`で専用configと7曜日の空busy-time fixtureを用意し、CLI子processへ明示して環境依存を除去した。

#### Reviewと検証

- 内部spec reviewで、子孫伝搬の事前検証、chrono境界、CLI経路を確認した。内部quality reviewの予約経路に関するP1を修正し、再reviewはblocking 0件、non-blocking 0件だった。
- 親reviewのP2を修正後、共通helperの任意深度収集、相対pathの決定性、root `.lock`限定除外、既存before / after比較の維持を内部再reviewし、blocking 0件、non-blocking 0件だった。親branch reviewも通過した。
- `a9544c9c`は空の専用`CARGO_TARGET_DIR`と欠損busy fileを指す外部configの継承条件でbuildから再検証し、対象CLI testは4 passedだった。同じ外部config条件の`tests/mcp_stdio.rs`は23 passedだった。修正後の内部reviewと親reviewはいずれも追加P1 / P2なしだった。
- `cargo fmt --check`: 成功。
- `cargo clippy --locked --all-targets -- -D warnings`: 成功。
- `cargo test --locked`: 成功。root libraryは1326 passed、0 failed、1 ignored、`tests/mcp_stdio.rs`は23 passed。
- `git diff --check main...HEAD`: 成功。最終確認時のworktreeはcleanだった。
- TD-027 / TD-043の契約、既存TD一覧・status、Wave計画、共有Wave summaryは変更していない。

#### 互換性と残存作業

- 通常範囲のdeadline計算、既存YAML、CLI / MCPの成功経路、完了済みtaskの予約境界を維持する。新しいerrorは従来panicしていた範囲外入力だけを明示的に拒否する。
- Wave 8統合gateと共有summary後の文書gateは成功済みであり、初回branch pushとPR #470作成まで実施した。`a9544c9c`と本記録commitの外部push、それらを含む最新CI再実行、mainへのmergeは未実施。

### W8-B / TD-048 実行記録(2026-09-20)

- 状態: lane実装、内部review、親review、local品質gateまで完了。push、PR作成、実CI、統合、mergeは未実施。
- task / worktree / branch: Codex task `01a0ba5b-356d-7ec1-a365-34f73a8362c2`、`/Users/sakakibaratakafumi/.codex/worktrees/1983/Schronu`、`feature/w8-b-sheet-lock-feedback`。
- revision: 基点`566a5e238a6c9e9cd923b18f7571308acb3de6b0`、製品・test・利用者文書のreview済みrevision`67405c91717f707ba53d037d2e013e06d54f273a`。本項の文書commitはこの後続。
- 予約file: `apps_script/main.js`、`apps_script/main.test.mjs`、`apps_script/test_support/fake_spreadsheet.mjs`、`apps_script/README.md`、本実行記録用の`backlog.md` lease。Spreadsheet列定義、Rust製品file、Web、TD-049の途中書込失敗処理は変更していない。
- 固定契約: DocumentLockを取得できない編集は同期先へ書き込まず、元sheet・行・taskまたはsegment identityと手動復旧手順をToastへ表示する。明示的な`選択範囲を再同期`は、失敗時のevent値を保持せず、実行時の選択範囲から最新identityと値を読み直して既存の全件事前検証を通す。identity不一致では書き込まず診断し、取得済みlockは成功・検証失敗とも`finally`で解放する。未取得lockは解放しない。
- Red / Green履歴: `a820fb8a`でlock失敗通知をRedにし、17件中その1件だけがToast未通知で失敗した。`6b007b56`で通知とlock lifecycleをGreenにした。`209d709b`でmenu再同期、最新値再読込、identity不一致をRedにし、20件中新規2件だけがmenu / handler未実装で失敗した。`1ee72b67`で共通lock・検証経路を使う再同期をGreenにし、`41dd264c`で復旧手順を文書化した。
- review: 内部subagent reviewはtask列のlock競合通知分岐が未固定であるP2を1件指摘した。`67405c91`でN列の実`onEdit`回帰testを追加し、再reviewで解消・追加指摘なしとなった。親reviewでは`main...branch`の責務境界、commit履歴、TD-049 / batch化 / 列契約変更の非混入を確認した。
- 検証: `node --test apps_script/main.test.mjs`は21件成功、`cargo test --locked --test spreadsheet_contract`は5件成功。`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`も成功した。Rust主要unit testは1313件成功、1件manual計測としてignoredで、integration testも成功した。
- 互換性: L/P列のsegment同期、N/R列のtask全segment同期、複合identity検証、command出力pasteの同期除外、列構成A-Sを維持する。lock対象外の編集は従来どおり無処理で、再同期menuは既存menuへ追加する。
- 未検証・残存作業: local Node fakeではToastの実表示、Google Sheetsのmenu配置とactive range取得を完全には再現できないため、実sheetでの表示・操作確認を公開後の確認対象とする。書込途中例外の失敗範囲診断と復旧はTD-049で扱い、W8-Bには含めない。

### W8-C / TD-050: Web CI feature gate

- 状態: 実装、内部review修正、親review、local gateまで完了。push、PR作成、実CI、Wave統合gateは未実施。
- task / worktree / branch: `W8-C / TD-050` / `/Users/sakakibaratakafumi/.codex/worktrees/a5c6/Schronu` / `feature/w8-c-web-ci-gates`。
- revision: 基点`566a5e238a6c9e9cd923b18f7571308acb3de6b0`。設定・test・READMEの最終revisionは`2184cf9483958a2c3a92ae560accc0ee8b2086ce`。
- 予約file: `.github/workflows/ci.yml`、`schronu-web/tests/web_feature_boundary_contract.rs`、`README.md`。製品code、共有manifest、UI文書は変更していない。
- 契約: 既存のroot / Apps Script / benchmarking gateを維持したまま、`schronu-web`のdefault / server / web featureをnative targetで個別にtest・Clippyする。browser専用moduleは別jobの`wasm32-unknown-unknown` checkで検証し、native all-featuresで代替しない。Rust 1.97.1、WASM target、job別cacheを明示する。
- Red / Green: commit `6e659cbb`でworkflow契約4件のうち既存2件だけが成功し、`web-native` / `web-wasm` job不在という同じ理由で新規2件がRedになった。commit `7da61197`で2jobとREADMEを追加してGreenにした。
- 内部review: active commandのraw部分一致ではroot testとdefault Web testの欠落を別commandのprefixで見逃すP2を検出した。commit `2184cf94`でjobごとのactiveな`run:`を抽出し、完全一致で検証するよう修正した。修正後のP1 / P2 / P3残存指摘はない。
- 親review: `main...branch`の3fileの累積差分、Red / Green履歴、予約範囲、製品挙動を変更しない責務分離を確認し、lane専用の`backlog.md`文書leaseを取得した。
- Web local gate: default test、既知のfeature限定未使用codeだけを許可するdefault Clippy、server test / Clippy、web全integration test、web製品library Clippy、WASM checkに成功した。server featureではendpoint unit test 2件を含む124件、feature boundary contractは4件が成功した。
- 既存・全体local gate: YAML parse、`node --test apps_script/main.test.mjs`(16件)、`cargo test --locked --features benchmarking --test scheduling_benchmark_contract`(16件)、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`、`git diff --check`に成功した。
- 互換性: 製品挙動、公開API、wire format、storage schema、error分類を変更していない。root、Apps Script、benchmarkingの既存CI commandを同じ`quality` jobに維持する。
- 未検証・残存作業: GitHub Actionsの実CIはpush / PR公開後にだけ確認する。local成功を実CI成功として扱わない。Web製品code変更がないためWASM prewarmと`dx build`の必須条件には該当しない。Wave共有summaryは統合gate後の別leaseで追記する。
- 依存commit: Wave 8内のhard dependencyはない。W8-A / W8-Bの未merge文書commitは取り込んでいない。

### Wave 8共有統合summary

- 統合状態: W8-A / TD-044、W8-B / TD-048、W8-C / TD-050を表記順に使い捨てintegration worktreeへ合成し、統合HEAD `1eb9efc2`で確認した。このrevisionは検証専用であり、各lane branchへ取り込まない。
- conflict: 製品codeのconflictは0件だった。`backlog.md`は各laneの末尾追記を内容不変のままW8-A→W8-B→W8-Cの順に配置した。
- root gate: `git diff --check`、`cargo fmt --check`、`cargo clippy --locked --all-targets -- -D warnings`、`cargo test --locked`に成功した。root libraryは1,326件成功・1件ignoredで、MCPは23件成功した。
- Apps Script / Spreadsheet gate: `node --test apps_script/main.test.mjs`は21件、`cargo test --locked --test spreadsheet_contract`は5件が成功した。実spreadsheetでの動作確認は未実施である。
- benchmarking gate: `cargo test --locked --features benchmarking --test scheduling_benchmark_contract`は16件成功した。
- Web gate: default test、default Clippy(`dead-code`だけを許可)、server test、server Clippy、web integration test、web library Clippy、`wasm32-unknown-unknown` checkに成功した。server testはlibrary 124件とmain 1件に加え、各integration testが成功した。
- 互換性: 3 laneを合成した状態でもroot、Apps Script、benchmarking、Web native / WASMの既存契約を維持した。統合側だけの製品修正はない。
- 公開・外部検証: push、PR作成、merge、GitHub Actionsの実CIは未実施であり、local / 統合gate成功を実CI成功として扱わない。実spreadsheet確認も未実施である。
- merge順: 共有文書末尾の競合を避けるため、PRはW8-A→W8-B→W8-Cの順でmergeする。先行PRのmerge後、後続branchは最新`main`へrebaseし、先行laneの文書記録を維持したうえで関連gate、全gate、親review、最新HEADの実CIを再確認する。
