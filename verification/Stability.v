(** * 単日の安定性と操作による余裕の変化

    Schronuの1日を、睡眠・経過・繰り返し・単発・バッファ・余裕に分解します。
    これは概念モデルの証明であり、Rust実装全体との対応を証明するものでは
    ありません。現行表示の「余差累」は累積規則と符号が異なるため、ここでの
    単日のFreeとは区別します。

    対象は同じ日内の通常進捗です。作業した秒数だけ残作業が減ると仮定します。
    睡眠以外の行動不能時間、固定予定、依存関係、日付更新、次回の繰り返し生成は
    扱いません。削除や先送り自体の所要時間は0とします。 *)

From Stdlib Require Import ZArith Lia List.
Import ListNotations.
Open Scope Z_scope.

(** ** 変数と単位

    - D: 1日の長さ。86400秒に固定します。
    - S (Sleep): その日に確保する睡眠時間。1日分の予約量です。
    - E (Elapsed): 睡眠を除く経過時間。作業と放置で増えます。
    - R (Repetition): 今日の繰り返しタスクの残作業時間の合計です。
    - U (Unique): 今日の単発タスクの残作業時間の合計です。
    - M (Margin): バッファを含む空き時間です。[M = D - S - E - R - U]。
    - B (Buffer): 単発の残作業に対する30%%のバッファです。[B = 0.3U]。
    - F (Free): バッファ確保後の余裕です。[F = M - B]。余裕があれば正です。

    SとEは重複しません。睡眠済みの時間をEに再び加えることはしません。
    RとUは元の見積もりではなく、今日に残っている作業量です。

    状態の各成分とMは整数秒です。BとFは0.1秒単位の整数で表し、30%%を丸めずに
    計算します。[buffer_tenths]は10B、[free_tenths]は10Fを表します。
    Fの不足も表すため、自然数ではなく符号付き整数[Z]を使います。 *)
Definition day_seconds : Z := 86400.

(** 状態として保存するのはS・E・R・Uだけです。M・B・Fは毎回計算するため、
    入力値と派生量が食い違う状態を持ちません。 *)
Record DayState := {
  sleep_seconds : Z;
  elapsed_seconds : Z;
  repetition_seconds : Z;
  unique_seconds : Z
}.

(** M: 睡眠・経過・残作業を除いた空き秒数です。負なら作業量が収まりません。 *)
Definition margin_seconds (s : DayState) : Z :=
  day_seconds - sleep_seconds s - elapsed_seconds s
    - repetition_seconds s - unique_seconds s.

(** 10B: 単発の残作業Uの30%%を0.1秒単位で表します。 *)
Definition buffer_tenths (s : DayState) : Z := 3 * unique_seconds s.
(** 10F: バッファを確保した後の余裕です。[10F = 10M - 3U]です。 *)
Definition free_tenths (s : DayState) : Z :=
  10 * margin_seconds s - buffer_tenths s.

(** 状態の睡眠・経過・残作業はすべて非負とします。派生量F・Mには負値も許します。 *)
Definition nonnegative_state (s : DayState) : Prop :=
  0 <= sleep_seconds s /\ 0 <= elapsed_seconds s /\
  0 <= repetition_seconds s /\ 0 <= unique_seconds s.

(** 安定状態は、状態の各成分が非負で、30%%のバッファを確保できる状態です。
    [F = 0]もちょうど確保できているため安定とします。 *)
Definition stable (s : DayState) : Prop :=
  nonnegative_state s /\ 0 <= free_tenths s.

(** 収支の恒等式です。[S + E + R + U + B + F = D]を10倍して示します。 *)
Theorem day_accounting (s : DayState) :
  10 * (sleep_seconds s + elapsed_seconds s
    + repetition_seconds s + unique_seconds s)
    + buffer_tenths s + free_tenths s = 10 * day_seconds.
Proof.
  unfold free_tenths, buffer_tenths, margin_seconds. ring.
Qed.

(** 単発には作業量の130%%を確保するため、10FではUの係数が13になります。 *)
Theorem free_tenths_formula (s : DayState) :
  free_tenths s =
    10 * (day_seconds - sleep_seconds s - elapsed_seconds s
      - repetition_seconds s) - 13 * unique_seconds s.
Proof.
  unfold free_tenths, buffer_tenths, margin_seconds. ring.
Qed.

(** [F >= 0]と[M >= B]は、同じ安定条件を表します。 *)
Theorem stability_iff_reserved_buffer (s : DayState) :
  stable s <->
    nonnegative_state s /\ buffer_tenths s <= 10 * margin_seconds s.
Proof.
  unfold stable, free_tenths. intuition lia.
Qed.

(** 安定ならバッファを含む空き時間も非負です。 *)
Theorem stable_has_nonnegative_margin (s : DayState) :
  stable s -> 0 <= margin_seconds s.
Proof.
  unfold stable, nonnegative_state, free_tenths, buffer_tenths. lia.
Qed.

(** 安定なら、睡眠・経過・残作業の合計は1日に収まります。
    配置上の制約を扱わないため、これは総量についての保証です。 *)
Theorem stable_fits_day (s : DayState) :
  stable s ->
    sleep_seconds s + elapsed_seconds s
      + repetition_seconds s + unique_seconds s <= day_seconds.
Proof.
  intro H. apply stable_has_nonnegative_margin in H.
  unfold margin_seconds in H. lia.
Qed.

(** ** 操作と収支

    作業と放置の秒数は自然数[nat]で持ち、負の経過時間を型で排除します。
    [Z.of_nat]で整数に変換してEへ加え、作業では対応するRまたはUも同量減らします。
    追加の秒数は符号付き整数[Z]です。正なら残作業を追加し、負なら除去します。
    追加と除去ではEは変わりません。すべての操作で0は状態を変えません。 *)
Inductive Action :=
| WorkRepetition (seconds : nat)
| WorkUnique (seconds : nat)
| Idle (seconds : nat)
| AddUnique (seconds : Z)
| AddRepetition (seconds : Z).

(** 負の追加は、削除と翌日以降への先送りを今日だけについて表します。
    移動先の安定性は保証しません。繰り返しは当日分の残作業だけを増減し、
    繰り返し設定や将来の発生は変更しません。
    [step]は整数上の関数として定義し、実行可能な入力は[safe_action]で制限します。 *)
Definition step (s : DayState) (a : Action) : DayState :=
  let S := sleep_seconds s in
  let E := elapsed_seconds s in
  let R := repetition_seconds s in
  let U := unique_seconds s in
  match a with
  | WorkRepetition t => Build_DayState S (E + Z.of_nat t) (R - Z.of_nat t) U
  | WorkUnique t => Build_DayState S (E + Z.of_nat t) R (U - Z.of_nat t)
  | Idle t => Build_DayState S (E + Z.of_nat t) R U
  | AddUnique t => Build_DayState S E R (U + t)
  | AddRepetition t => Build_DayState S E (R + t) U
  end.

(** Mの期待変化です。通常進捗では、経過と作業減少が相殺してMは不変です。 *)
Definition margin_delta (a : Action) : Z :=
  match a with
  | WorkRepetition _ | WorkUnique _ => 0
  | Idle t => -Z.of_nat t
  | AddUnique t | AddRepetition t => -t
  end.

(** 10Bの期待変化です。Bは単発の残作業Uだけに依存します。 *)
Definition buffer_delta_tenths (a : Action) : Z :=
  match a with
  | WorkUnique t => -3 * Z.of_nat t
  | AddUnique t => 3 * t
  | WorkRepetition _ | Idle _ | AddRepetition _ => 0
  end.

(** 10Fの期待変化です。

    - 繰り返し作業: Fは不変です。
    - 単発作業: Fは作業秒数の30%%増えます。
    - 放置: Fは放置秒数だけ減ります。
    - 単発追加: Fは符号付き増減量の130%%減ります。
    - 繰り返し追加: Fは符号付き増減量だけ減ります。

    増減量が負の場合は除去となり、その絶対値に対応する分だけFが増えます。 *)
Definition free_delta_tenths (a : Action) : Z :=
  match a with
  | WorkRepetition _ => 0
  | WorkUnique t => 3 * Z.of_nat t
  | Idle t => -10 * Z.of_nat t
  | AddUnique t => -13 * t
  | AddRepetition t => -10 * t
  end.

(** 操作後のM・B・Fが、それぞれの期待変化に一致することを示します。
    増減の恒等式自体は、初期状態の安定性を必要としません。 *)
Theorem step_accounting (s : DayState) (a : Action) :
  margin_seconds (step s a) = margin_seconds s + margin_delta a /\
  buffer_tenths (step s a) = buffer_tenths s + buffer_delta_tenths a /\
  free_tenths (step s a) = free_tenths s + free_delta_tenths a.
Proof.
  destruct s; destruct a;
    unfold free_tenths, margin_seconds, buffer_tenths,
      step, margin_delta, buffer_delta_tenths, free_delta_tenths;
    cbn -[Z.add Z.sub Z.mul Z.opp]; repeat split; ring.
Qed.

(** ** 安全条件と安定性保存

    安全条件は操作前の状態と要求秒数だけから判定します。
    作業・放置の非負性は[nat]が保証します。作業量は残作業以内に制限します。
    [n = Z.of_nat t]とすると、放置は[10n <= 10F]を要求します。
    符号付き増減量をdとすると、単発追加は[-U <= d]かつ[13d <= 10F]、
    繰り返し追加は[-R <= d]かつ[10d <= 10F]を要求します。
    下限は残作業を負にする除去を防ぎ、上限はバッファ確保後の余裕を守ります。
    操作後の安定性を条件として仮定しているわけではありません。 *)
Definition safe_action (s : DayState) (a : Action) : Prop :=
  match a with
  | WorkRepetition t => Z.of_nat t <= repetition_seconds s
  | WorkUnique t => Z.of_nat t <= unique_seconds s
  | Idle t => 10 * Z.of_nat t <= free_tenths s
  | AddUnique t => -unique_seconds s <= t /\ 13 * t <= free_tenths s
  | AddRepetition t => -repetition_seconds s <= t /\ 10 * t <= free_tenths s
  end.

(** 安定な状態に対する安全な1操作は、状態の各成分とFの非負性を保ちます。 *)
Theorem safe_step_preserves_stability (s : DayState) (a : Action) :
  stable s -> safe_action s a -> stable (step s a).
Proof.
  destruct s; destruct a;
    unfold stable, nonnegative_state, safe_action, free_tenths,
      margin_seconds, buffer_tenths, step;
    cbn -[Z.add Z.sub Z.mul Z.opp]; intros; repeat split; lia.
Qed.

(** 放置の条件は必要十分です。安定状態から自然数t秒を放置する場合、
    F以内なら安定を保ち、Fを超えれば安定を失います。 *)
Theorem idle_stable_iff (s : DayState) (t : nat) :
  stable s ->
    (stable (step s (Idle t)) <-> 10 * Z.of_nat t <= free_tenths s).
Proof.
  destruct s;
    unfold stable, nonnegative_state, free_tenths,
      margin_seconds, buffer_tenths, step;
    cbn -[Z.add Z.sub Z.mul Z.opp]; intros; split; intros; repeat split; lia.
Qed.

(** 単発の符号付き追加について、安全条件は必要十分です。
    正なら作業量とその30%%を合わせた余裕が必要で、負なら残作業以内の除去を
    許します。増減後のUが負になる場合は、Fが増えても安定ではありません。 *)
Theorem add_unique_stable_iff (s : DayState) (t : Z) :
  stable s ->
    (stable (step s (AddUnique t)) <->
      -unique_seconds s <= t /\ 13 * t <= free_tenths s).
Proof.
  destruct s;
    unfold stable, nonnegative_state, free_tenths,
      margin_seconds, buffer_tenths, step;
    cbn -[Z.add Z.sub Z.mul Z.opp]; intros; split; intros; repeat split; lia.
Qed.

(** 繰り返しの符号付き追加についても、安全条件は必要十分です。
    繰り返しはBの算出対象ではないため、正の追加に必要なFは追加量と同量です。 *)
Theorem add_repetition_stable_iff (s : DayState) (t : Z) :
  stable s ->
    (stable (step s (AddRepetition t)) <->
      -repetition_seconds s <= t /\ 10 * t <= free_tenths s).
Proof.
  destruct s;
    unfold stable, nonnegative_state, free_tenths,
      margin_seconds, buffer_tenths, step;
    cbn -[Z.add Z.sub Z.mul Z.opp]; intros; split; intros; repeat split; lia.
Qed.

(** ** 操作列と作業の分割 *)

(** 有限の操作列を左から順に実行します。 *)
Fixpoint run (s : DayState) (actions : list Action) : DayState :=
  match actions with
  | [] => s
  | a :: rest => run (step s a) rest
  end.

(** 各操作の安全条件は、直前までの操作を反映した状態で検査します。
    最初の状態の余裕をすべての操作で使い回すことはしません。 *)
Fixpoint safe_actions (s : DayState) (actions : list Action) : Prop :=
  match actions with
  | [] => True
  | a :: rest => safe_action s a /\ safe_actions (step s a) rest
  end.

(** 各段階で安全条件を守れば、任意の有限回の操作後も安定です。
    操作列の長さについて帰納法で証明します。 *)
Theorem safe_run_preserves_stability (actions : list Action) :
  forall s, stable s -> safe_actions s actions -> stable (run s actions).
Proof.
  induction actions as [| a rest IH]; intros s Hstable Hsafe.
  - exact Hstable.
  - destruct Hsafe as [Haction Hrest]. cbn.
    apply IH.
    + apply safe_step_preserves_stability; assumption.
    + exact Hrest.
Qed.

(** 繰り返しの通常進捗は、2回に分けても合計時間だけ進めても同じ状態です。 *)
Theorem work_repetition_split (s : DayState) (t1 t2 : nat) :
  step (step s (WorkRepetition t1)) (WorkRepetition t2) =
    step s (WorkRepetition (t1 + t2)%nat).
Proof.
  destruct s. unfold step. cbn -[Z.add Z.sub Z.mul Z.opp]. f_equal; lia.
Qed.

(** 単発の通常進捗も同じ分割不変性を持ちます。 *)
Theorem work_unique_split (s : DayState) (t1 t2 : nat) :
  step (step s (WorkUnique t1)) (WorkUnique t2) =
    step s (WorkUnique (t1 + t2)%nat).
Proof.
  destruct s. unfold step. cbn -[Z.add Z.sub Z.mul Z.opp]. f_equal; lia.
Qed.

(** 合計時間の進捗が安全であることと、分割後の各進捗がその直前の状態で
    安全であることは同値です。時間の非負性は[nat]が保証するため、前提は不要です。 *)
Theorem work_repetition_split_safe (s : DayState) (t1 t2 : nat) :
  (safe_action s (WorkRepetition (t1 + t2)%nat) <->
    safe_actions s [WorkRepetition t1; WorkRepetition t2]).
Proof.
  unfold safe_actions, safe_action, step. cbn -[Z.add Z.sub Z.mul Z.opp]. lia.
Qed.

(** 単発についても、分割前後の安全条件は同値です。 *)
Theorem work_unique_split_safe (s : DayState) (t1 t2 : nat) :
  (safe_action s (WorkUnique (t1 + t2)%nat) <->
    safe_actions s [WorkUnique t1; WorkUnique t2]).
Proof.
  unfold safe_actions, safe_action, step. cbn -[Z.add Z.sub Z.mul Z.opp]. lia.
Qed.

(** ** 境界値の具体例

    余裕0、操作量0、許容上限ちょうどと1秒超過、残作業全量の進捗・除去、
    除去し過ぎ、追加後の同量除去を検証します。
    すべて24時間の状態で、残り枠だけを小さくして計算します。
    具体例は一般定理の代わりではなく、意図した境界の読み取りを助けます。 *)
Section BoundaryExamples.

Let exact_buffer := Build_DayState 25200 61187 0 10.
Let idle_boundary := Build_DayState 25200 61170 0 10.
Let addition_boundary := Build_DayState 25200 61187 0 0.
Let full_progress := Build_DayState 25200 61177 10 10.

Example zero_free_is_stable :
  free_tenths exact_buffer = 0 /\ stable exact_buffer.
Proof. vm_compute. repeat split; congruence. Qed.

Example zero_actions_preserve_state :
  forall s,
  run s [WorkRepetition 0; WorkUnique 0; Idle 0; AddUnique 0;
         AddRepetition 0] = s.
Proof.
  intros [S E R U]. cbn -[Z.add Z.sub Z.mul Z.opp]. f_equal; lia.
Qed.

Example zero_actions_are_safe :
  forall s, stable s -> safe_actions s
    [WorkRepetition 0; WorkUnique 0; Idle 0; AddUnique 0;
     AddRepetition 0].
Proof.
  intros [S E R U].
  unfold stable, nonnegative_state, safe_actions, safe_action,
    free_tenths, margin_seconds, buffer_tenths, step.
  cbn -[Z.add Z.sub Z.mul Z.opp]. intros. repeat split; lia.
Qed.

Example idle_at_limit_is_stable :
  safe_action idle_boundary (Idle 17) /\
  free_tenths (step idle_boundary (Idle 17)) = 0 /\
  stable (step idle_boundary (Idle 17)).
Proof. vm_compute. repeat split; congruence. Qed.

Example idle_one_second_over_limit_is_unstable :
  ~ safe_action idle_boundary (Idle 18) /\
  ~ stable (step idle_boundary (Idle 18)).
Proof. vm_compute. intuition discriminate. Qed.

Example addition_at_limit_is_stable :
  safe_action addition_boundary (AddUnique 10) /\
  free_tenths (step addition_boundary (AddUnique 10)) = 0 /\
  stable (step addition_boundary (AddUnique 10)).
Proof. vm_compute. repeat split; congruence. Qed.

Example addition_one_second_over_limit_is_unstable :
  ~ safe_action addition_boundary (AddUnique 11) /\
  ~ stable (step addition_boundary (AddUnique 11)).
Proof. vm_compute. intuition discriminate. Qed.

(** 繰り返し追加は、単発のバッファを増やさずFを追加量だけ消費します。 *)
Example repetition_addition_at_limit_is_stable :
  stable addition_boundary /\
  safe_action addition_boundary (AddRepetition 13) /\
  step addition_boundary (AddRepetition 13) = Build_DayState 25200 61187 13 0 /\
  buffer_tenths (step addition_boundary (AddRepetition 13)) = 0 /\
  free_tenths (step addition_boundary (AddRepetition 13)) = 0 /\
  stable (step addition_boundary (AddRepetition 13)).
Proof. vm_compute. repeat split; congruence. Qed.

Example repetition_addition_one_second_over_limit_is_unstable :
  ~ safe_action addition_boundary (AddRepetition 14) /\
  ~ stable (step addition_boundary (AddRepetition 14)).
Proof. vm_compute. intuition discriminate. Qed.

Example full_remaining_work_can_be_completed :
  stable full_progress /\
  safe_actions full_progress [WorkRepetition 10; WorkUnique 10] /\
  stable (run full_progress [WorkRepetition 10; WorkUnique 10]) /\
  repetition_seconds (run full_progress [WorkRepetition 10; WorkUnique 10]) = 0 /\
  unique_seconds (run full_progress [WorkRepetition 10; WorkUnique 10]) = 0.
Proof. vm_compute. repeat split; congruence. Qed.

(** 負の単発追加は経過時間を変えず、残作業を除去します。 *)
Example negative_unique_addition_removes_work :
  stable full_progress /\
  safe_action full_progress (AddUnique (-10)) /\
  step full_progress (AddUnique (-10)) = Build_DayState 25200 61177 10 0 /\
  stable (step full_progress (AddUnique (-10))) /\
  free_tenths (step full_progress (AddUnique (-10))) = 130.
Proof. vm_compute. repeat split; congruence. Qed.

(** 負の繰り返し追加も、経過時間を戻さず当日分の残作業だけを除去します。 *)
Example negative_repetition_addition_removes_work :
  stable full_progress /\
  safe_action full_progress (AddRepetition (-10)) /\
  step full_progress (AddRepetition (-10)) = Build_DayState 25200 61177 0 10 /\
  stable (step full_progress (AddRepetition (-10))) /\
  free_tenths (step full_progress (AddRepetition (-10))) = 100.
Proof. vm_compute. repeat split; congruence. Qed.

Example all_remaining_work_can_be_removed :
  stable full_progress /\
  safe_actions full_progress [AddUnique (-10); AddRepetition (-10)] /\
  stable (run full_progress [AddUnique (-10); AddRepetition (-10)]) /\
  free_tenths (run full_progress [AddUnique (-10); AddRepetition (-10)]) = 230.
Proof. vm_compute. repeat split; congruence. Qed.

(** 除去でFが増えても、残作業が負になる操作は許可しません。 *)
Example removal_beyond_remaining_work_is_rejected :
  ~ safe_action full_progress (AddUnique (-11)) /\
  ~ stable (step full_progress (AddUnique (-11))) /\
  ~ safe_action full_progress (AddRepetition (-11)) /\
  ~ stable (step full_progress (AddRepetition (-11))).
Proof. vm_compute. intuition discriminate. Qed.

(** 正の追加と同量の負の追加は、単発・繰り返しともに元の状態へ戻ります。 *)
Example additions_and_removals_restore_state :
  stable addition_boundary /\
  safe_actions addition_boundary
    [AddUnique 10; AddUnique (-10); AddRepetition 13; AddRepetition (-13)] /\
  run addition_boundary
    [AddUnique 10; AddUnique (-10); AddRepetition 13; AddRepetition (-13)]
    = addition_boundary.
Proof. vm_compute. repeat split; congruence. Qed.

Example progress_beyond_remaining_work_is_rejected :
  ~ safe_action full_progress (WorkRepetition 11) /\
  ~ safe_action full_progress (WorkUnique 11).
Proof. vm_compute. intuition discriminate. Qed.

End BoundaryExamples.

(** ** 証明の仮定検査

    主要定理について、コンパイル時に依存する仮定を表示します。
    すべて[Closed under the global context]となることを確認します。
    各定理の明示的な前提は引数として量化されており、未証明の公理として
    外部から導入された仮定はありません。 *)
Print Assumptions day_accounting.
Print Assumptions free_tenths_formula.
Print Assumptions stability_iff_reserved_buffer.
Print Assumptions stable_has_nonnegative_margin.
Print Assumptions stable_fits_day.
Print Assumptions step_accounting.
Print Assumptions safe_step_preserves_stability.
Print Assumptions idle_stable_iff.
Print Assumptions add_unique_stable_iff.
Print Assumptions add_repetition_stable_iff.
Print Assumptions safe_run_preserves_stability.
Print Assumptions work_repetition_split.
Print Assumptions work_unique_split.
Print Assumptions work_repetition_split_safe.
Print Assumptions work_unique_split_safe.
