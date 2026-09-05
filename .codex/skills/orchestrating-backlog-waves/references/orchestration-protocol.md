# Wave統括手順

## 1. 調査と変更範囲の予約

1. `N`を0以上の整数としてparseする。正確な`Wave N` section、すべての`WN-*` lane、backlog項目、契約、依存関係、write範囲、Wave内の注意事項、統合gate、存在する場合は`Wave N+1` sectionを特定する。
2. laneの所属、依存種別、完了契約、write所有権のいずれかが曖昧なら、task作成前に停止する。隣接Waveから不足方針を推測してはならない。
3. サイドバーから確認できる新しいlane taskの作成を、ユーザーが明示的に依頼していることを確認する。Skillの自動選択やWaveの説明・計画依頼は`create_thread`の許可にならない。権限がなければ先に尋ねる。
4. repositoryの指示を読み、`main`、revision、現在のworktree、remote、関連backlog項目を調査する。status表記だけで依存充足とは扱わず、必要な契約commitが`main`に存在することを確認する。
5. 製品file、test、fixture、文書の予約表を作る。所有単位はfile全体とする。全laneの計画用taskを作成するが、実装を承認するのは依存がreadyなlaneだけとする。
6. `list_projects`で保存済みprojectを解決する。`git worktree list`で得たprimary worktree pathとGit repository projectを一致させる。一意に一致しなければ停止してユーザーへ尋ね、projectless taskで代用しない。

## 2. 計画用taskの作成

各laneについて、解決した`projectId`と`environment.type: worktree`を指定して`create_thread`を呼ぶ。modelとthinkingは上書きしない。titleにはlane IDとbacklog項目を含める。

worktree準備中で`clientThreadId`だけが返った場合、それをreadyな`threadId`が必要なtoolへ渡してはならない。task一覧からreadyなtaskを解決してから、wait、read、message送信を行う。親のlane表には、readyなtask ID、host ID、worktree path、予定branch、依存関係、予約状態を記録する。

初回promptには次を含める。

- Wave/lane ID、backlog項目、固定する契約、依存関係、予約済みwrite範囲、基点となる`main` revision、予定branch `feature/<lane>-<short-name>`。
- 初回turnでのfile編集、実装、branch作成・切り替え、commit、push、PR作成の絶対禁止。
- `backlog.md`、`AGENTS.md`、関連コード・test、依存のready状態、write競合の調査依頼。
- 各契約単位のcommit計画。各commitについてmessage、固定する契約、責務・module、先行commitへの依存、対象test、想定する単一のRed理由、Green確認commandを必須とする。
- 計画提示後にturnを終了し、親の明示承認を待つ指示。

`wait_threads`で待機する。laneが8件を超える場合は最大8 targetのgroupへ分け、返されたcursorを後続waitで使用する。`read_thread`は詳細不足、対応要求、最終証跡の確認にだけ使う。初回turnでcommit追加もfile変更もないことをlane worktreeで確認する。違反があればそのlaneを隔離し、ほかのlaneを続行しながら回復方針をユーザーへ尋ねる。

## 3. 実装の承認と監視

各計画をbacklog契約、repositoryのcommit規則、依存関係、予約表と照合する。計画と依存関係の両方を承認した場合にだけ、実装開始を明示するmessageを送る。編集前に専用feature branchを作成または確認させる。

各laneで次を実施する。

1. 契約単位のRed testを1つ追加して実行し、想定した1つの理由で失敗したことを確認してからcommitする。
2. 最小のGreen実装を追加し、対象testとrepositoryの品質gateを実行する。review対象を固定するため、基礎Green実装をcommitする。
3. Green commit後に内部subagentによる仕様・code quality reviewを行う。指摘は1件ずつ、対象testで検証し、1指摘1commitで修正する。P1/P2を却下したり、以前のtestを弱めたりしない。
4. `main...branch`累積差分について、責務境界、重複、file size、fixture、failure path、依存方向、repository固有の保守性閾値をreviewする。
5. 製品Green後、親からそのlane専用の`backlog.md`文書leaseを得てから、自laneのstatus、詳細、検証、残存作業だけを独立した文書commitで更新する。commit後はleaseを解放する。leaseは依存順と予定PR merge順に付与し、共有の全体検証summaryや他laneの項目は編集しない。
6. cleanなworktree、全品質gate、`git diff --check`、履歴reviewを確認して終了する。この時点ではpushもPR作成も行わない。

予約外file、公開API・schema・error変更、異なるRed理由が必要になった場合は、その範囲拡張だけを停止し、現在の状態を保持させる。影響をreviewし、親またはユーザーの明示承認を得てから予約表とcommit計画を更新する。古くなった予約内へ収めるために、人工的な呼び出し、製品コードのtest専用分岐、error情報の削減、helperの複製を行ってはならない。

## 4. 親reviewと統合gate

完了報告を受けた各laneについて、親が次を個別に確認する。

- lane範囲のsubjectとbodyを含む`git log --reverse`。
- `git diff --stat`、`--numstat`、`--name-only`、`--check`、`main...branch`の全差分。
- Red/Greenの対応、commitの単一目的性、review修正の分離、許可file、既存testの維持、製品経路のcoverage、文書変更の分離、clean status。
- fileまたは累積追加が800行を超える、fixtureが巨大、helperやerror変換が重複、責務を局所的に追えないなど、repositoryの保守性閾値。

具体的な指摘は同じlaneへ返す。P1/P2は修正と再reviewを必須とし、判断が難しい指摘は安全な修正を止めずに記録する。test suiteがGreenでも、保守性または履歴の失敗を上書きできない。

Wave内hard dependencyがない場合は、全laneが親reviewを通過するまで待つ。その後、現在の`main`から使い捨てintegration worktreeを作り、feature branchを変更せず、依存順と文書化された直列化順にlane commitを適用する。semantic conflictは黙って解消せず、所有laneへ戻してbranchのrebaseまたは修正、親の再reviewを行い、使い捨て統合状態を再構築する。

Wave内hard dependencyがある場合は、deadlockを避けるためdependency frontier単位で進める。readyなfrontierをすべて親reviewし、現在の`main`へそのfrontierとmerge済みWave作業を合成して該当gateを実行し、そのfrontierのPRだけを公開する。ユーザーが前提PRを`main`へmergeするまで依存laneの実装を開始しない。merge commitを確認し、依存laneをrebaseして続行する。最後のfrontierでは、PR公開前にWave全体の統合gateを実行する。前提PRをユーザーに代わってmergeしてはならない。

`git diff --check`、repository全体のformat・lint・test gate、Wave固有gate、backlogが要求する境界横断testを実行する。使い捨てworktreeは安全なrepository運用に従って削除または残置し、その合成履歴は公開しない。

選択Waveが共有backlog summaryの更新を要求する場合は、Wave全体の統合gate後にだけ更新する。backlogで担当laneが指定されていればそれを使い、未指定なら最終dependency frontierでlane IDが辞書順の最後となるlaneを割り当てる。別taskは作らない。その既存laneへ文書leaseを与え、共有summaryだけの最終文書commitを作成させる。親がcommitをreviewし、使い捨て統合状態を再構築して、`git diff --check`、文書固有の検査、変更文書を入力とするrepository gateを再実行してから公開する。

## 5. 公開と報告

必要な親reviewと統合gateの通過後にだけ、対象laneへfeature branchの通常pushと`main`向け非draft PR作成を指示する。Wave内依存がある場合の対象laneは現在のdependency frontier、それ以外は全laneとする。PR本文には目的、契約変更、Red/Green履歴、review修正、検証、互換性、依存関係、merge順、残存作業を記載する。merge、force-push、合成integration branchの公開は禁止する。

各laneが別々のbacklog文書commit用leaseを順番に取得するため、PRのmerge順を明記する。先行PRのmerge後、競合する後続branchは最新`main`へrebaseし、merge済みのbacklog証跡を維持したうえで、関連gateと全gate、親reviewを再実行しなければmerge可能と報告しない。必須の共有summaryは、割り当て済みの既存laneに置く。

最終報告には各laneのtask title/ID、branch、commit、review状態、gate、PR URL/state、必要なmerge・rebase順、blockedまたは未承認の範囲を含める。「開発・PR準備完了」と「mainへmerge済み」を区別する。Wave `N+1`の依存関係と残存作業は説明するが、そのtask、branch、commit、PRは作成しない。

## 初回prompt例

```text
あなたはW4-A / TD-039の計画だけを担当します。固定する契約、依存関係、予約済みの製品・test・fixture・文書file、予定branch、基点となるmain revisionを以下に記載します。

初回turnでは、file編集、実装、branch作成・切り替え、commit、push、PR作成を行わないでください。backlog.md、AGENTS.md、依存関係、関連コード・testを調査してください。各commitについて、message、固定する契約、責務・module、依存関係、対象test、想定する単一のRed理由、Green確認commandを含む契約単位のRed/Green commit計画を提示してください。計画提示後にturnを終了し、親の明示承認を待ってください。
```
