# 予定配置policy

## 目的

Schronuは、締切が近いtaskを無条件で先に並べません。通常は重要度を表すpriorityが高いtaskを進め、締切を守るために残された空き時間が尽きる時だけ締切taskへ切り替えます。fixed予定はこの選択とは独立して、指定時刻へ予約されます。

このpolicyは`全`、MCPの`get_schedule`、`詰`、`平`など、同じscheduleを利用する処理へ一貫して適用されます。

## 用語

| 用語 | 意味 |
| --- | --- |
| fixed | 開始時刻を動かさない予定。flexible taskに対して予約区間となる |
| flexible | fixed予約とeventの間へ選択policyに従って配置するtask |
| release | `start_time`、`pending_until`、dependency完了をすべて満たし、taskが着手可能になる最早時刻 |
| effective deadline | 明示deadlineと、そのtaskを必要とするfixed予定の開始時刻のうち最も早い時刻 |
| cumulative demand | effective deadlineが`D`以下であるunfinished flexible taskの残作業秒数の合計 |
| slack | deadlineまでのfree capacityからcumulative demandを引いた秒数 |
| atomic | 中断せず1segmentで完了できる時だけ開始するtask |
| completion event | window内で作業が完了するfixedについて、通常は元window終了とdependency完了の双方を待ってdependentを解放するscheduler内部のevent |

## Policyの4 phase

### 1. fixedとflexibleの分類

fixedは指定開始時刻と元の見積時間から予約windowを作ります。fixed同士が重なっても一方を移動したり消したりしません。二重予約は入力上の事実であり、schedule上でも見える必要があるためです。

flexible taskと、fixed windowに収まらなかった残作業はevent loopの候補にします。

### 2. 予約unionとcompletion eventの構築

flexible taskから見たfixed予約はunion化します。これにより、重なったfixedの時間を空き時間から二重に差し引きません。一方、表示では各fixedを個別に残します。

fixedの表示window長と実際に残っている作業秒数は別の値です。完了済みの作業があっても予約windowは消さず、window内で実行する残作業だけを`scheduled_work_seconds`として記録します。windowに収まらない残作業は、元window終了後にflexibleとして続行します。

fixedを必要とするtransitive dependencyには、fixed開始をsynthetic effective deadlineとして逆伝搬します。fixed自体を動かすのではなく、その準備に必要な容量をslack guardで保護するためです。

window内で残作業が完了するfixedには、task ID、最早発生時刻、dependency IDsだけを持つprivateな`CompletionEvent`を作ります。通常経路では元window終了とdependency完了の双方を待ってからdependentを解放します。missing dependencyまたはcycleでは、loopやevent消失を避ける決定論的なfallbackによって内部完了扱いにします。いずれもscheduler内の依存graphだけを進め、永続的な`Task`状態は変更しません。このeventは作業量を持たず、slack需要、priority選択、schedule出力には入りません。windowを超える残作業がある場合はeventへ置き換えず、実作業を持つflexible taskとして続行し、その完了後にdependentを解放します。

### 3. Event loop

時刻`t`とeffective deadline`D`に対し、次を計算します。

`slack(D,t)=fixed予約を除く[t,D)の空き秒 - effective deadline<=Dのunfinished flexible残秒`

全slackが正なら、ready taskをpriority降順で選びます。同priorityではeffective deadlineがあるもの、その時刻が早いもの、rank、UUIDの順です。

slackが0以下のgroupがある場合は、最も早いcritical deadline以下のready taskだけを保護対象にします。その中ではeffective deadline昇順、priority降順、rank、UUIDの順です。保護対象がまだreleaseされていない場合は、実行可能なtaskを通常順で進め、releaseまたはslack境界で再選択します。

segmentは次の最も早いeventで閉じます。

- 選択taskの完了
- fixed予約の開始
- non-atomic taskでは次のcandidate release。atomic taskでは実際にpreemptionを起こすcandidate release
- 保護対象外の作業によってslackが0になる時刻

この分割で前半または後半が5分以下になる場合は、無益な短時間segmentを避けます。ただしfixed境界は越えず、deadline保護taskがその境界でreleaseされる場合は、必要な前半segmentを保持します。

### 4. 決定論的な表示sort

配置完了後、segmentを開始時刻、deadlineの有無、priority、rank、UUIDでsortします。これは表示順を安定させる処理であり、taskの選択規則ではありません。

## Fixed予定

- fixed同士の重複はそのまま表示します。flexible taskは重複区間のunionを避けます。
- fixed開始が現在より過去なら、現在から元window終了までを見える予約として残します。
- 元windowへ収まらない残作業は元window終了後のflexible taskとなります。予約windowと後続作業を合わせた作業秒数は元の残作業量と一致します。
- fixed開始をsynthetic effective deadlineとしてdependencyへ伝えます。window内で完了する場合、completion eventは通常経路で元window終了とdependency完了の双方を待ちます。missing dependencyまたはcycleでは上記fallbackを使います。超過する場合は後続の実作業が完了するまでdependentを解放しません。

`約`または`appointment`は開始時刻を設定して`fixed_start = true`にします。`始`または`start`は開始時刻を設定して`fixed_start = false`に戻します。

旧YAMLに`fixed_start` fieldがない場合だけ、次の完全一致で従来の予定を推定します。

`deadline_time == start_time + estimated_work_seconds`

明示されたtrueまたはfalseは推定より優先します。推定済みの値や、上記の形に一致する明示的なfalseは保存時に確定値として書き出し、次回読込で再推定しません。

## Atomic taskと不能時の扱い

atomic taskは完了まで連続する枠がある場合だけ開始します。fixed開始、slackが0になる時刻、atomicより先に選ばれるtaskのreleaseを跨ぐ場合は候補を後順へ送り、収まる候補を探します。release予測は、その時点のpriorityとcritical groupでも実際にpreemptionが起きる場合だけ境界に採用します。

dependencyの欠落やcycle、deadlineまでの容量不足などで通常配置が不能でも、taskを消したりloopしたりしません。通常選択keyによる決定論的なfallbackで1segment進め、残作業をevent loopへ戻します。deadline超過はそのままscheduleへ現れ、上位層の既存警告で利用者に示されます。

日時に作業秒数を加算できない場合はfallbackで値を丸めず、task ID、開始時刻、作業秒数を保持したerrorを返します。

## 不変条件

- work conservation: 各taskの全`scheduled_work_seconds`はschedule開始時の残作業秒数と一致し、欠落も重複もない
- fixed immobility: fixedの指定開始と元windowをpriorityによって移動しない
- flexible non-overlap: flexible segmentはfixed予約のunionおよび他のflexible segmentと重ならない
- dependency: dependencyの完了時刻より前にdependent taskを開始しない
- determinism: 同じtask状態と基準時刻には同じsegment順を返す
- checked datetime: 表現不能な日時加算は情報を保持したerrorにする

## Timeline例

### 高priorityを進めてからslackで切り替える

- 09:00: 高priority task A(4時間、deadlineなし)とtask B(2時間、deadline 14:00)がready
- 09:00時点: Bのslackは3時間なのでAを選ぶ
- 12:00: Bのslackが0になり、Aのsegmentを閉じる
- 12:00-14:00: critical groupのBを実行する
- 14:00以降: Aの残り1時間へ戻る

### Meetingが重複する

- meeting M1は13:00-14:00、M2は13:30-14:30にfixed
- 表示にはM1とM2を両方残す
- flexible taskから見た予約は13:00-14:30の90分であり、重複30分を二重計上しない

### Atomic taskが連続枠へ収まらない

- 10:00に60分のatomic task Aがready、10:30にfixedが始まる
- Aは30分で中断できないため開始しない
- 10:30までに収まる別taskを選ぶか、候補がなければ次eventへ進む
- fixed終了後に60分の連続枠が得られた時点でAを再評価する

### 過去開始のfixedに超過作業がある

- 現在10:30、fixedの元windowは10:00-11:00、残作業は90分
- 10:30-11:00の30分をfixed window内の作業として配置する
- 見える予約は10:30-11:00のまま保つ
- 残る60分は11:00以降のflexible作業として配置する

## 実装note

`src/application/scheduling_policy.rs`だけが配置判断を持ち、上位use caseは候補生成と結果変換を担当します。slackの定義を変えずに性能を保つため、deadline prefixの差分をrange tree、ready/release/dependencyの差分をfrontier、atomicの将来release評価を世代付きcacheで保持します。

通常CIでは`ScheduleMetrics`のselection event、candidate probe、slack probe、occupied slot probe、atomic release cacheなどに決定論的な上限を設けます。週次・手動のrelease benchmarkはREADMEの[scheduling性能契約](../../README.md#scheduling性能契約)に記載したtypical 500ms、stress 5,000msを上限とします。
