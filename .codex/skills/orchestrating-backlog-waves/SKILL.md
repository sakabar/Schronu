---
name: orchestrating-backlog-waves
description: backlog.mdのWaveを、現在の保存済みproject内でサイドバーから確認できる複数のCodex task、branch、worktreeに分けて統括する必要がある場合に使用する。
---

# バックログWaveの統括

## 概要

`backlog.md`のWaveを1つだけ、明示的なgateを通して統括する。呼び出し形式は`$orchestrating-backlog-waves N`とし、Wave `N+1`は実行対象にしない。

## 必須手順

taskを作成する前に、[references/orchestration-protocol.md](references/orchestration-protocol.md)を最後まで読み、必ず従う。repositoryの指示がより厳しい場合は、そちらを優先する。

## 厳守する境界

- 自動選択だけではtask作成権限を得られない。lane taskを作成するユーザーの明示的な依頼を必要とする。
- 各laneは、現在の保存済みproject内に`create_thread`で作成し、それぞれ別worktreeを使用する。内部subagentはreviewだけに使う。
- laneの初回turnは、調査と契約単位のcommit計画だけに限定する。file編集、実装、commit、pushは禁止する。「安全な」test、scaffold、部分実装も禁止する。
- hard dependencyは、必要な契約commitが`main`に入った時点でのみreadyになる。積み重ねbranch、draft PR、APIの推測で迂回しない。
- laneは予約済みfileだけを変更できる。範囲拡張、共有契約、Red理由の変化があれば該当laneを止め、独立laneは続行する。
- laneの自己申告だけでは公開を許可しない。親reviewと該当する統合検証の通過後にのみpush・PRを許可する。
- 非draft PRを作成するが、mergeは行わない。Wave `N+1`は残存作業としてのみ報告する。

## 早見表

| 段階 | 必要な証跡 | 次の操作 |
| --- | --- | --- |
| 調査 | 曖昧さのないWaveとwrite予約 | 計画用taskを作成 |
| 計画 | 完全な計画と変更のないworktree | readyなlaneを承認 |
| lane完了 | Red/Green履歴、内部review、品質gate | 親によるbranch review |
| 親確認 | 許可範囲内で保守可能な差分、未解消P1/P2なし | 統合検証 |
| 統合完了 | 依存順の合成とWave gateの成功 | pushと非draft PR作成 |

## 停止条件

次の提案が出た場合は、該当laneを停止する。

- 小さい、または元に戻せるという理由で初回turnから実装する。
- 未mergeの依存branchから実装を始める。
- 変更行が異なるという理由で同じfileを同時所有する。
- 内部reviewまたはGreenの自己申告を親承認の代わりにする。
- 統合検証前に公開する。
- 待ち時間に次Waveを実装する。

laneが初回turnの禁止事項に違反した場合は、その作業を隔離して回復方法をユーザーへ尋ねる。黙ってsalvage、reset、revert、archive、再作成してはならない。

## よくある合理化

| 合理化 | 必須対応 |
| --- | --- |
| 「依存branchはすでにGreen」 | 必要なcommitが`main`へ入るまで待つ。 |
| 「共有部分は10行だけ」 | file全体の所有権を直列化する。 |
| 「laneは数時間かけ、testも成功済み」 | 状態を保持して承認せず、ユーザーへ判断を求める。 |
| 「draft PRなら進捗を維持できる」 | 計画taskや依存待ちは公開gateではない。 |
| 「次Waveは独立している」 | 今回の実行範囲外として扱う。 |
