# Rocq による形式検証

Prosa 0.6 のリリースで動作確認された Rocq (旧 Coq) 9.0.0、MathComp
2.4.0、mczify 1.5.0 を使います。
[Prosa 0.6 のリリースノート](https://prosa.mpi-sws.org/releases/v0.6/index.html)

## 初回セットアップ

opam 2.0 以上を初期化してから、リポジトリのルートで次を実行します。
OCaml 4.14.2 は POET の環境構築手順に合わせています。
[POET の環境構築手順](https://github.com/PROSA-Project/POET#rocq-toolchain)

```sh
cd verification
opam switch create . ocaml-base-compiler.4.14.2 --no-install \
  --repositories=default,rocq-released=https://rocq-prover.org/opam/released
opam install --switch=. --deps-only ./schronu-verification.opam
```

## 最初の証明

`Smoke.v` では Prosa の離散時間モデルから `duration` を読み込み、長さに
0 を足しても変わらないことを帰納法で証明します。`verification/` で
コンパイルし、生成された証明を検査します。

```sh
opam exec --switch=. -- rocq compile Smoke.v
opam exec --switch=. -- rocqchk -silent Smoke
```

## 単日の安定性

`Stability.v`は、単日の時間収支と操作による安定性の保存を証明します。
`.v`内の日本語Coqdocコメントにも、変数・前提・各定理の意味を記載しています。

### 変数と安定状態

| 記号 | 意味 | Rocqでの表現 |
| --- | --- | --- |
| D | 1日の長さ、86400秒 | `day_seconds` |
| S (Sleep) | その日に確保する1日分の睡眠秒数 | `sleep_seconds` |
| E (Elapsed) | 睡眠を除く経過秒数 | `elapsed_seconds` |
| R (Repetition) | 今日の繰り返しタスクの残作業秒数の合計 | `repetition_seconds` |
| U (Unique) | 今日の単発タスクの残作業秒数の合計 | `unique_seconds` |
| M (Margin) | バッファを含む空き時間、`D - S - E - R - U` | `margin_seconds`は秒単位 |
| B (Buffer) | 単発の残作業の30%、`0.3U` | `buffer_tenths`は10B |
| F (Free) | バッファ確保後の余裕、`M - B` | `free_tenths`は10F |

状態はS・E・R・Uのみを保持し、M・B・Fは計算で求めます。SとEは重複せず、
睡眠済みの時間をEへ再び加えません。RとUは元の見積もりではなく残作業です。

時間収支は`S + E + R + U + B + F = D`です。状態の各成分は整数秒とし、BとFは
0.1秒単位の整数`Z`で扱います。`10B = 3U`、
`10F = 10(D - S - E - R) - 13U`なので、30%の計算に丸めはありません。

`stable`は、S・E・R・Uが非負かつ`F >= 0`であることです。`F = 0`も安定に
含めます。これは`M >= B`と同値であり、睡眠・経過・残作業の総量が1日に
収まることも保証します。Fは単日の量であり、余裕があると負になる現行表示の
「余差累」とは、符号も集計規則も異なります。

### 操作と安全条件

`Action`は次の5種類です。作業・放置の時間は`nat`とし、負の経過時間を型で
排除します。追加の増減量は`Z`とし、正なら追加、負なら削除・先送りを表します。

```coq
Inductive Action :=
| WorkRepetition (seconds : nat)
| WorkUnique (seconds : nat)
| Idle (seconds : nat)
| AddUnique (seconds : Z)
| AddRepetition (seconds : Z).
```

状態と収支計算は`Z`を維持します。以下では`t : nat`、`n = Z.of_nat t`、
`d : Z`とします。作業と放置ではEがn増え、作業では対応する残作業がn減ります。
追加ではUまたはRへdを加えます。追加・除去操作自体の所要時間は0で、Eは変わりません。

| 操作 | Mの変化 | Bの変化 | Fの変化 | 安定状態からの安全条件 |
| --- | --- | --- | --- | --- |
| `WorkRepetition t` | 0 | 0 | 0 | `n <= R` |
| `WorkUnique t` | 0 | `-0.3n` | `+0.3n` | `n <= U` |
| `Idle t` | `-n` | 0 | `-n` | `10n <= 10F` |
| `AddUnique d` | `-d` | `+0.3d` | `-1.3d` | `-U <= d`かつ`13d <= 10F` |
| `AddRepetition d` | `-d` | 0 | `-d` | `-R <= d`かつ`10d <= 10F` |

`AddUnique (-10)`は単発の残作業を10秒除去し、`AddRepetition (-10)`は
繰り返し当日分の残作業を10秒除去します。除去は削除または翌日以降への先送りを
表します。先送り先の安定性、繰り返し設定、将来の発生は扱いません。

作業・放置の非負性は型が保証しますが、残作業量や余裕の上限は安全条件で検査します。
符号付き追加の下限は残作業を負にする除去を防ぎ、上限は30%のバッファを守ります。
安全条件は操作前の状態と要求量から求め、操作後の安定性を仮定しません。

### 主な定理

| 定理 | 保証 |
| --- | --- |
| `day_accounting`、`free_tenths_formula` | 時間収支と10Fの計算式 |
| `stability_iff_reserved_buffer` | 安定条件と30%のバッファ確保の同値性 |
| `stable_has_nonnegative_margin`、`stable_fits_day` | 安定ならMは非負で、総量が1日に収まる |
| `step_accounting` | 全操作のM・B・Fの変化が上の表に一致する |
| `safe_step_preserves_stability` | 安全条件を満たす1操作は安定性を保つ |
| `idle_stable_iff` | 放置の上限条件は必要十分である |
| `add_unique_stable_iff`、`add_repetition_stable_iff` | 正負両方の追加について、残作業の下限と余裕の上限を合わせた条件が必要十分である |
| `safe_run_preserves_stability` | 各段階で安全条件を満たす任意の有限操作列は安定性を保つ |
| `work_repetition_split`、`work_unique_split` | 同じ種類の作業を2回に分けても最終状態は同じ |
| `work_repetition_split_safe`、`work_unique_split_safe` | `nat`の時間で作業を分割すると、安全条件も同値である。非負性の前提は不要 |

`safe_actions`は各操作の直前の状態で安全条件を検査します。最初の状態の余裕を
後の操作でも使い回すことはありません。具体例では、余裕0、全操作の操作量0、
放置・単発追加・繰り返し追加の許容上限ちょうどと1秒超過、残作業全量の
進捗・除去、残作業を超える進捗と除去の拒否、追加後の同量除去による状態の復元を
検査します。負の追加が経過時間を変えないことも確認します。

### 前提と実装との境界

通常進捗では、同じ日内で作業した秒数だけ残作業が減ると仮定します。
睡眠以外の行動不能時間、固定予定、依存関係、日付更新、次回の繰り返し生成は
含みません。総量が収まることから、これらの制約下での配置可能性は導きません。

## 証明の検証

`verification/`で次を実行します。

```sh
opam exec --switch=. -- rocq compile Smoke.v
opam exec --switch=. -- rocq compile Stability.v
opam exec --switch=. -- rocqchk -silent Smoke Stability
```

`Stability.v`末尾の`Print Assumptions`が主要定理の依存仮定を表示します。
全15件が`Closed under the global context`となり、すべてのコマンドが終了コード0に
なることを確認します。証明には`Admitted`や独自の`Axiom`を使いません。
`stable s`などの明示的な定理の前提は、未証明の外部公理とは区別します。

## 日本語Coqdocの生成

上記のコンパイル後、同じ`verification/`でHTMLを生成できます。

```sh
mkdir -p _doc
opam exec --switch=. -- coqdoc --html --utf8 --toc -d _doc Smoke.v Stability.v
```

`_doc/Stability.html`に変数・前提・定理の説明と証明を、
`_doc/Smoke.html`に環境確認の説明と証明を出力します。生成物と`lia`のキャッシュは
Git管理から除外します。Coqdocコメントの正本は各`.v`ファイルです。
コメント内でパーセント記号を表示するには`%%`と記述します。単独の`%`は
CoqdocのLaTeX用エスケープとして解釈され、HTMLで説明が欠落するためです。
