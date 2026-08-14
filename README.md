# Schronu

Schronu (スロン) : タスクの抵抗感を減らして前に進んでいくための1人用タスク管理ツール

## 動作環境

* macOSを正式な動作確認対象としています。
* その他のUnix系OSでは動作する可能性がありますが、継続的な動作確認は行っていません。
* Windowsには現在対応していません。

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

Schronuは、ローカルのMCP clientから9個のtask toolを利用できるstdio serverを提供します。network transportや認証機能は持ちません。

### buildと起動

```shell
cargo build --release --bin schronu-mcp
SCHRONU_STORAGE_DIR=/absolute/path/to/tasks SCHRONU_CONFIG_PATH=/absolute/path/to/schronu.yaml ./target/release/schronu-mcp
```

`schronu-mcp`はstdinからnewline区切りのJSON-RPC messageを読み、stdoutにはMCP protocol responseだけを出力します。起動時は保存先pathの検証だけを行い、lock取得やrepository loadは行いません。通常はterminalから直接操作せず、MCP clientからprocessを起動してください。

保存先は`SCHRONU_STORAGE_DIR`で指定します。未指定時は起動時のworking directoryから見た`../Schronu-private/tasks/`です。相対pathの解釈違いを避けるため、MCP clientでは絶対pathを推奨します。同じ環境変数はCLIの`schronu`にも適用されます。

### 設定ファイル

`SCHRONU_CONFIG_PATH`には任意の設定YAMLのabsolute pathを指定できます。CLIの`schronu`とMCP serverの`schronu-mcp`は、ともに起動時に同じ設定を読み込みます。未指定時は従来の既定値で動作します。指定したfileが読めない、YAMLが壊れている、未知のキーや不正な値がある場合は、意図しない既定値でtaskを操作しないよう起動を停止します。

仕事用設定の例です。

```yaml
obsidian_vault_name: Obsidian-Moica
busy_time_slots_yaml_path: busy_time_slots.yaml
end_of_day_duration: "00:30"
calendar_blank_line_weekday: Mon
extrude_skip_weekdays: [Sat, Sun]
default_deadline_time: "19:00"
```

すべてのキーは任意です。相対`busy_time_slots_yaml_path`は、実行時のworking directoryではなく設定YAMLの親directoryから解釈します。

| キー | 既定値 | 効果 |
| --- | --- | --- |
| `obsidian_vault_name` | `Obsidian-Moica` | `黒`(または`obs`)コマンドのObsidian検索先vault名です。空白や記号を含む名前も利用できます。 |
| `busy_time_slots_yaml_path` | `../Schronu-private/busy_time_slots.yaml` | 毎週定期の行動不能時間を定義するYAMLへのpathです。 |
| `end_of_day_duration` | `00:30` | 日次容量・`全`・`暦`・`帯`・`平`・`詰`で使う1日の終端時刻です。業務日の開始境界である06:00は変更しません。 |
| `calendar_blank_line_weekday` | `Mon` | `暦`の出力で、その曜日の直後に空行を入れます。 |
| `extrude_skip_weekdays` | `[]` | `押`で次の割当日として飛ばす曜日です。例の`[Sat, Sun]`では土日を飛ばします。7曜日すべては指定できません。 |
| `default_deadline_time` | `23:59:59` | `〆`の`今`・`明`・曜日・日付指定で使う締切時刻です。時刻を明示した`〆 19:00`と`〆 消`には適用しません。 |

曜日は`Mon`、`Tue`、`Wed`、`Thu`、`Fri`、`Sat`、`Sun`のいずれかです。`end_of_day_duration`は`HH:MM`、`default_deadline_time`は`HH:MM`または`HH:MM:SS`で指定します。

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
| `complete_task` | `task_id`、optional: `finished_at`、`additional_actual_work_seconds` | taskを完了する |
| `update_task` | `task_id`と、`estimated_work_minutes`、`deadline_time`、`category`のうち1つ以上 | 見積もり・締切・categoryを更新する |

`deadline_time`と`category`は`null`で解除できます。`list_tasks.period.field`は`scheduled_start`、`created_at`、`deadline`、`completed_at`のいずれかで、`from`以上`until`未満の半開区間です。`statuses`は`todo`、`pending`、`done`、`categories`は上記categoryまたは`null`を配列で指定します。同じ`statuses`内と同じ`categories`内はOR、period・status・categoryの間はANDです。statusは現在時刻を反映した実効statusで判定します。配列の省略または空配列は、その項目で絞り込みません。`get_schedule.from`と`get_schedule.until`は`YYYY-MM-DD`の日付で、`from`以上`until`未満の範囲を指定します。`from`のみはその日、`until`のみは現在から指定日までです。両方省略時は、現在からSchronuの次の業務日境界までを返します。

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

延期候補は、葉よりも着手可能になるのが遅い親タスクを優先します。同じrankでは期限がない、または期限が遅いタスク、優先度が低いタスク、現在の予定開始が遅いタスクの順です。親を延期しても子孫の状態は変更しません。タスクは分割せず、残作業時間0、待っていることを表すタスク、1日の最大容量を超えるタスク、現在の予定が業務日境界をまたぐタスクは対象外です。

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

この非対話実行では、引数全体を1つのコマンドとして扱います。結果は標準出力へ出し、タスクファイルは保存しません。
スプレッドシートに貼る形式へ整形する場合は、以下のように使います。

```shell
schronu 尾 | ~/projects/sakabar/Schronu/shell/copy_for_spreadsheet.sh | pbcopy
```

スプレッドシートから作業実績をコマンドへ戻す場合は、シートをコピーしてから以下のように使います。

```shell
~/projects/sakabar/Schronu/shell/generate_command_from_spreadsheet.sh
```

R列には、タスクの処理時期を変更するときに実行するコマンド、またはSpreadsheet上の表示制御に使う値を入力します。

- 空白: 通常どおりQ列の抽出対象を処理する
- `W`: `W` コマンドで延期する
- `d`: `d` コマンドで翌朝まで延期する
- `t`: Spreadsheet上の表示制御にのみ使い、コマンド生成では無視する

`W` または `d` が入力された行は、Q列が `TRUE` でなくても抽出されます。`t` を含むそれ以外の値はR列としては無視され、Q列が `TRUE` の場合だけ通常処理されます。同じタスクが複数行に分かれている場合、R列の `W` と `d` を混在させないでください。

完了対象の行では、P列に `2026/07/04 9:23:45` のような完了時刻、S列に `0:23:45` のような実作業時間を入れてください。生成される `終` コマンドにはP列の完了時刻が渡されます。

新規タスクを仮登録する場合は、B列を空欄のままJ列にタスク名を入力してください。Q列の抽出フラグに関係なく、`新 <タスク名>`、仮の説明、見積もり3分のコマンドが生成されます。

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

各タスクには、`start_time`、`pending_until`、親子関係、締切から逆算した制約を考慮した「最速着手可能時刻」があります。タスク一覧では、この時刻を「それ以前には開始できない」という制約として扱い、実際の作業予定時刻は以下の順序で割り当てます。

* まず締切があるタスクを配置する
  * 締切があるタスク同士では、締切が早いものを先に配置する
  * 締切が同じ場合は、優先度が高いものを先に配置する
* 次に締切がないタスクを配置する
  * 優先度が高いものを先に配置する
  * 優先度が同じ場合は、最速着手可能時刻が早いものを先に配置する

タスクを配置する時は、すでに配置済みのタスクの作業予定時刻と重ならない最も早い時刻に置きます。そのため、後の時刻に着手可能になる高優先度タスクが先に時間枠を確保し、低優先度タスクはその前後の空き時間に入ります。

例えば、12:00の昼の料理、13:00の昼の食事、18:00の夜の料理が高優先度で存在し、13:00以降に着手可能な低優先度タスクが10時間分あっても、低優先度タスクが18:00の夜の料理を押し出すことはありません。18:00の枠を避けて、配置可能な空き時間に割り当てられます。

親タスクは、子タスクが実際に割り当てられた作業予定時刻で完了した後にのみ配置されます。つまり、子タスクが他のタスクとの衝突回避によって後ろにずれた場合、親タスクもその子タスクの実際の完了予定時刻以降にずれます。

長いタスクが次の作業予定時刻までの空き時間に入りきらない場合は、表示上の複数セグメントに分割されます。分割された各セグメントの合計時間は元タスクの残作業時間と一致し、後続の高優先度タスクや締切が早いタスクは押し出されません。ただし、分割後の前半または後半のどちらかが5分以下になる分割は行わず、その場合は次の十分な空き時間へ送られます。

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
