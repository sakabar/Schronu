(** * 締切までの累積作業量とSchronuの予定配置

    計画開始を0秒とし、固定予定のない1人分の時間軸を扱います。有限個の独立した
    タスクはすべて開始時から着手可能です。残作業は実行した秒数だけ減ります。
    正の作業を持つ区間をモデル化し、時間を消費しない完了行は省略します。

    [src/application/scheduling_policy.rs]の通常/保護選択、slack境界、分割可否を
    この入力範囲に特殊化します。日時は整数秒で、Rustの型で表現可能な範囲を
    想定します。木・キャッシュ・日時演算を含むRustコードとの同値性は別の課題です。 *)

From Stdlib Require Import List Arith PeanoNat ZArith Lia Bool.
Import ListNotations.

Record Policy := {
  jobs : list nat;                     (** 重複しないタスクID。大小関係はUUID順。 *)
  deadline : nat -> option nat;        (** 開始から実効締切までの秒数 *)
  priority : nat -> Z;
  rank : nat -> nat;
  atomic : nat -> bool
}.

Definition due_by (p : Policy) j d :=
  match deadline p j with Some e => e <=? d | None => false end.

Definition contribution p (r : nat -> nat) d j :=
  if due_by p j d then r j else 0.
Definition demand_on p r d ids := list_sum (map (contribution p r d) ids).
Definition demand p r d := demand_on p r d (jobs p).
Definition active p (r : nat -> nat) := filter (fun j => 0 <? r j) (jobs p).

Lemma active_spec p r j : In j (active p r) <-> In j (jobs p) /\ 0 < r j.
Proof. unfold active. rewrite filter_In, Nat.ltb_lt. reflexivity. Qed.

(** [SlackDemandIndex]と同じく、残作業がある締切群だけを調べます。選択候補が
    締切[d]の需要に含まれないときだけ、この群の余裕が実行長の上限になります。
    自然数の差は0で止まります。余裕が負の入力にも締切遵守を主張しません。 *)
Definition outside_slack p t r j k :=
  match deadline p k with
  | Some d => if due_by p j d then [] else [d - (t + demand p r d)]
  | None => []
  end.
Definition outside_slacks p t r j := flat_map (outside_slack p t r j) (active p r).

Definition segment_length p t r j :=
  fold_right Nat.min (r j) (outside_slacks p t r j).

Definition critical p t r :=
  existsb (fun k => match deadline p k with
    | Some d => d <=? t + demand p r d
    | None => false
    end) (active p r).

Definition split_floor := 900.
Definition usable_split before after :=
  (split_floor <? before) && (split_floor <? after).
Definition fits p t r j :=
  let n := segment_length p t r j in
  (n =? r j) || (negb (atomic p j) && usable_split n (r j - n)).

(** 候補の比較はRustの[normal_selection_key]と[protected_selection_key]に対応します。
    実行長が0となる保護対象外の候補は[fits]で除かれるため、残った保護候補を
    締切順に調べればよいことを利用します。 *)
Definition compare_deadline (x y : option nat) :=
  match x, y with
  | Some a, Some b => Nat.compare a b
  | Some _, None => Lt | None, Some _ => Gt | None, None => Eq
  end.
Definition then_compare (x y : comparison) := match x with Eq => y | _ => x end.
Definition preferred p (protected : bool) j k :=
  let d := compare_deadline (deadline p j) (deadline p k) in
  let q := Z.compare (priority p k) (priority p j) in
  let tail := then_compare (Nat.compare (rank p j) (rank p k)) (Nat.compare j k) in
  match then_compare (if protected then then_compare d q else then_compare q d) tail
  with Gt => false | _ => true end.

Fixpoint best (before : nat -> nat -> bool) (xs : list nat) : option nat :=
  match xs with
  | [] => None
  | x :: rest => match best before rest with
    | None => Some x
    | Some y => Some (if before x y then x else y)
    end
  end.

Definition choose p t r :=
  best (preferred p (critical p t r)) (filter (fits p t r) (active p r)).

Lemma best_member before xs j : best before xs = Some j -> In j xs.
Proof.
  induction xs as [|x xs IH]; simpl; [discriminate|].
  destruct (best before xs) as [y|]; [destruct (before x y)|];
    intros H; inversion H; subst; auto.
Qed.

Lemma best_empty before xs : best before xs = None -> xs = [].
Proof.
  destruct xs as [|x xs]; simpl; auto.
  destruct (best before xs); discriminate.
Qed.

(** 任意の非空な候補集合から、他のどの締切も先に来ない候補を選べます。
    締切なしはすべての有限な締切より後として扱います。 *)
Definition no_later p j k := forall d, due_by p k d = true -> due_by p j d = true.

Lemma deadline_total p j k : no_later p j k \/ no_later p k j.
Proof.
  unfold no_later, due_by. destruct (deadline p j) as [a|], (deadline p k) as [b|].
  - destruct (Nat.le_ge_cases a b); [left|right]; intros d;
      rewrite !Nat.leb_le; lia.
  - left; discriminate.
  - right; discriminate.
  - left; auto.
Qed.

Lemma earliest_member p xs : xs <> [] ->
  exists j, In j xs /\ forall k, In k xs -> no_later p j k.
Proof.
  induction xs as [|x xs IH]; [contradiction|]. intros _.
  destruct xs as [|y ys].
  - exists x. simpl. split; auto. intros k [<-|[]] d; auto.
  - destruct IH as [j [Hj First]]; [discriminate|].
    destruct (deadline_total p x j) as [Hx|Hx].
    + exists x. split; [left; reflexivity|].
      intros k [<-|Hk] d Hd; auto. apply Hx, (First k Hk), Hd.
    + exists j. split; [right; exact Hj|].
      intros k [<-|Hk]; auto.
Qed.

Lemma earliest_full p t r j :
  (forall k, In k (active p r) -> no_later p j k) ->
  segment_length p t r j = r j.
Proof.
  intros First. unfold segment_length, outside_slacks.
  assert (E : forall xs, (forall k, In k xs -> no_later p j k) ->
    flat_map (outside_slack p t r j) xs = []).
  { intros xs. induction xs as [|k xs IH]; simpl; auto. intros H.
    unfold outside_slack at 1. destruct (deadline p k) as [d|] eqn:D.
    - assert (B : due_by p k d = true) by (unfold due_by; rewrite D, Nat.leb_refl; reflexivity).
      rewrite (H k (or_introl eq_refl) d B). simpl. apply IH. auto.
    - apply IH. auto. }
  now rewrite E.
Qed.

Theorem candidate_progress p t r :
  (exists k, In k (jobs p) /\ 0 < r k) -> exists j, choose p t r = Some j.
Proof.
  intros [k Hk]. apply active_spec in Hk.
  assert (NE : active p r <> []) by (intro E; rewrite E in Hk; contradiction).
  destruct (earliest_member p _ NE) as [j [Hj First]].
  assert (F : fits p t r j = true).
  { unfold fits. rewrite (earliest_full _ _ _ _ First), Nat.eqb_refl. reflexivity. }
  destruct (choose p t r) as [selected|] eqn:C; [eauto|].
  unfold choose in C. apply best_empty in C.
  assert (M : In j (filter (fits p t r) (active p r))) by (apply filter_In; auto).
  rewrite C in M. contradiction.
Qed.

(** [permitted]は選択された区間の長さについて証明する上限です。 *)
Definition permitted p t r j n :=
  n <= r j /\ forall k d, In k (jobs p) -> deadline p k = Some d -> 0 < r k ->
    due_by p j d = false -> n <= d - (t + demand p r d).

Lemma fold_min_bound xs cap : fold_right Nat.min cap xs <= cap /\
  forall x, In x xs -> fold_right Nat.min cap xs <= x.
Proof.
  induction xs as [|a xs [Hcap Hxs]]; simpl.
  - split; [lia|contradiction].
  - split; [lia|]. intros x [<-|Hx]; [lia|specialize (Hxs x Hx); lia].
Qed.

Lemma chosen_segment p t r j : choose p t r = Some j ->
  In j (jobs p) /\ 0 < segment_length p t r j /\
  permitted p t r j (segment_length p t r j).
Proof.
  unfold choose. intros H. apply best_member, filter_In in H.
  destruct H as [A F]. apply active_spec in A. destruct A as [M R].
  split; [exact M|]. split.
  - unfold fits, usable_split in F. apply orb_true_iff in F as [E|S].
    + apply Nat.eqb_eq in E. lia.
    + apply andb_true_iff in S as [_ S]. apply andb_true_iff in S as [S _].
      apply Nat.ltb_lt in S. lia.
  - unfold permitted, segment_length. destruct (fold_min_bound (outside_slacks p t r j) (r j))
      as [Cap Bound]. split; auto. intros k d Hk D Rk Out. apply Bound.
    apply in_flat_map. exists k. split; [now apply active_spec|].
    unfold outside_slack. rewrite D, Out. left; reflexivity.
Qed.

(** 余裕を使い切った群があれば、選ばれた作業は必ずその群の締切までに
    必要な作業に含まれます。特に最も早いcritical deadlineが保護されます。 *)
Theorem critical_group_protected p t r j k d : choose p t r = Some j ->
  In k (jobs p) -> deadline p k = Some d -> 0 < r k -> d <= t + demand p r d ->
  due_by p j d = true.
Proof.
  intros Ch Mk D R Tight. destruct (due_by p j d) eqn:B; auto.
  destruct (chosen_segment _ _ _ _ Ch) as [_ [Pos [_ Limit]]].
  specialize (Limit k d Mk D R B). lia.
Qed.

(** ** 区間の実行と締切の不変条件

    新しい区間を選ぶのは前の区間の終了時だけです。区間内では同じタスクを
    1秒ずつ実行します。この表現をそのままProsaの予定に読み替えます。 *)
Record Frame := {
  remaining : nat -> nat;
  running : nat;
  left_in_segment : nat
}.

Definition dispatch p t s :=
  match left_in_segment s with
  | 0 => option_map (fun j => (j, segment_length p t (remaining s) j))
                    (choose p t (remaining s))
  | S n => Some (running s, S n)
  end.

Definition spend (r : nat -> nat) j k := if k =? j then pred (r k) else r k.
Definition advance p t s :=
  match dispatch p t s with
  | None => s
  | Some (j, n) => {| remaining := spend (remaining s) j;
                       running := j; left_in_segment := pred n |}
  end.

Fixpoint frame_at p cost t :=
  match t with
  | 0 => {| remaining := cost; running := 0; left_in_segment := 0 |}
  | S u => advance p u (frame_at p cost u)
  end.
Definition scheduled_job p cost t := option_map fst (dispatch p t (frame_at p cost t)).

Lemma spend_le r j k : spend r j k <= r k.
Proof. unfold spend. destruct (k =? j); lia. Qed.

Lemma demand_absent p r j d xs : ~ In j xs ->
  demand_on p (spend r j) d xs = demand_on p r d xs.
Proof.
  induction xs as [|k xs IH]; simpl; auto. intros Absent.
  change (contribution p (spend r j) d k + demand_on p (spend r j) d xs =
          contribution p r d k + demand_on p r d xs).
  rewrite IH by tauto. unfold contribution, spend.
  destruct (Nat.eqb_spec k j); [tauto|reflexivity].
Qed.

Lemma demand_spent p r j d xs : NoDup xs -> In j xs -> 0 < r j ->
  demand_on p r d xs = demand_on p (spend r j) d xs + (if due_by p j d then 1 else 0).
Proof.
  induction xs as [|k xs IH]; [contradiction|]. intros ND M R.
  inversion ND as [|? ? Absent Unique]; subst.
  change (contribution p r d k + demand_on p r d xs =
          contribution p (spend r j) d k + demand_on p (spend r j) d xs +
          (if due_by p j d then 1 else 0)).
  destruct M as [<-|M].
  - rewrite demand_absent by auto. unfold contribution, spend.
    rewrite Nat.eqb_refl. destruct (due_by p k d); lia.
  - rewrite (IH Unique M R). unfold contribution.
    assert (k <> j) by (intros ->; contradiction).
    unfold spend. destruct (Nat.eqb_spec k j); [contradiction|]. lia.
Qed.

Lemma member_demand p r j d xs : In j xs -> due_by p j d = true ->
  r j <= demand_on p r d xs.
Proof.
  induction xs as [|k xs IH]; [contradiction|]. intros M B.
  change (r j <= contribution p r d k + demand_on p r d xs). destruct M as [<-|M].
  - unfold contribution. rewrite B. lia.
  - specialize (IH M B). lia.
Qed.

(** 初期条件は各入力締切までに必要な作業量から計算できます。途中では、残作業を
    持つ各締切群に対して同じ容量条件を保ちます。空になった群は棄却できます。 *)
Definition initially_fits p cost :=
  forall k d, In k (jobs p) -> deadline p k = Some d -> demand p cost d <= d.
Definition safe p t r :=
  forall k d, In k (jobs p) -> deadline p k = Some d -> 0 < r k ->
    t + demand p r d <= d.
Definition sound_frame p t s :=
  safe p t (remaining s) /\ (0 < left_in_segment s ->
    In (running s) (jobs p) /\
    permitted p t (remaining s) (running s) (left_in_segment s)).

Lemma dispatch_valid p t s j n : sound_frame p t s -> dispatch p t s = Some (j,n) ->
  In j (jobs p) /\ 0 < n /\ permitted p t (remaining s) j n.
Proof.
  intros [_ Carry]. unfold dispatch. destruct (left_in_segment s) as [|m] eqn:L.
  - destruct (choose p t (remaining s)) as [k|] eqn:C; simpl; [|discriminate].
    intros E. inversion E; subst. now apply chosen_segment.
  - intros E. inversion E; subst. specialize (Carry (Nat.lt_0_succ m)). intuition lia.
Qed.

Lemma dispatch_idle p t s : dispatch p t s = None ->
  forall k, In k (jobs p) -> remaining s k = 0.
Proof.
  unfold dispatch. destruct (left_in_segment s); [|discriminate].
  destruct (choose p t (remaining s)) eqn:C; [discriminate|].
  intros _ k M. destruct (Nat.eq_dec (remaining s k) 0); auto.
  assert (P : exists i, In i (jobs p) /\ 0 < remaining s i) by (exists k; split; auto; lia).
  destruct (candidate_progress p t _ P) as [i E]. congruence.
Qed.

Lemma work_preserves p t r j n : NoDup (jobs p) -> safe p t r ->
  In j (jobs p) -> 0 < n -> permitted p t r j n ->
  safe p (S t) (spend r j) /\ permitted p (S t) (spend r j) j (pred n).
Proof.
  intros ND Safe M Pos [Cap Guard].
  assert (Delta : forall d, demand p r d = demand p (spend r j) d +
                               (if due_by p j d then 1 else 0)).
  { intros d. apply demand_spent; auto; lia. }
  split.
  - intros k d Mk D R. assert (R0 : 0 < r k) by (pose proof (spend_le r j k); lia).
    specialize (Safe k d Mk D R0). specialize (Delta d).
    destruct (due_by p j d) eqn:B; [lia|]. specialize (Guard k d Mk D R0 B). lia.
  - split.
    + unfold spend. rewrite Nat.eqb_refl. lia.
    + intros k d Mk D R B. assert (R0 : 0 < r k) by (pose proof (spend_le r j k); lia).
      specialize (Guard k d Mk D R0 B). specialize (Delta d). rewrite B in Delta. lia.
Qed.

Lemma advance_sound p t s : NoDup (jobs p) -> sound_frame p t s ->
  sound_frame p (S t) (advance p t s).
Proof.
  intros ND H. unfold advance. destruct (dispatch p t s) as [[j n]|] eqn:E.
  - destruct (dispatch_valid _ _ _ _ _ H E) as [M [Pos P]].
    destruct (work_preserves _ _ _ _ _ ND (proj1 H) M Pos P) as [Safe Carry].
    split; simpl; auto.
  - assert (Empty := dispatch_idle _ _ _ E).
    assert (L : left_in_segment s = 0).
    { unfold dispatch in E. destruct (left_in_segment s); congruence. }
    split; [intros k d M D R; specialize (Empty k M); lia|rewrite L; lia].
Qed.

Theorem frames_safe p cost : NoDup (jobs p) -> initially_fits p cost ->
  forall t, sound_frame p t (frame_at p cost t).
Proof.
  intros ND Init t. induction t as [|t IH]; [|apply advance_sound; assumption].
  split; simpl; [intros k d M D _; now apply (Init k d)|lia].
Qed.

Theorem no_work_left_at_deadline p cost j d : NoDup (jobs p) -> initially_fits p cost ->
  In j (jobs p) -> deadline p j = Some d -> remaining (frame_at p cost d) j = 0.
Proof.
  intros ND Init M D. destruct (frames_safe p cost ND Init d) as [Safe _].
  destruct (Nat.eq_dec (remaining (frame_at p cost d) j) 0); auto.
  assert (Pos : 0 < remaining (frame_at p cost d) j) by lia.
  specialize (Safe j d M D Pos).
  assert (B : due_by p j d = true) by (unfold due_by; rewrite D; apply Nat.leb_refl).
  pose proof (member_demand p (remaining (frame_at p cost d)) j d _ M B).
  unfold demand in Safe. lia.
Qed.

(** ** 計算例

    ID 0は締切なしの高優先度タスク、ID 1と2には締切があります。
    2件目の締切までの余裕を計算するときは1件目の作業も含みます。 *)
Module Examples.
Definition workload (d1 d2 : nat) (a : bool) :=
  {| jobs := [0;1;2]; deadline := fun j =>
       match j with 1 => Some d1 | 2 => Some d2 | _ => None end;
     priority := fun j => if j =? 0 then 99%Z else 1%Z;
     rank := fun _ => 0; atomic := fun j => (j =? 0) && a |}.
Definition costs j := match j with 0 => 2400 | 1 => 600 | 2 => 900 | _ => 0 end.
Definition scenario := workload 1800 2460 false.
Example cumulative_guard :
  demand scenario costs 2460 = 1500 /\
  dispatch scenario 0 (frame_at scenario costs 0) = Some (0,960).
Proof. vm_compute. auto. Qed.

Example switch_at_guard : scheduled_job scenario costs 959 = Some 0 /\
  scheduled_job scenario costs 960 = Some 1.
Proof. vm_compute. auto. Qed.

(** 前半と後半の各々について、900秒ちょうどと901秒を区別します。 *)
Example split_threshold :
  usable_split 900 901 = false /\ usable_split 901 900 = false /\
  usable_split 901 901 = true /\
  choose (workload 1800 2400 false) 0 costs = Some 1 /\
  choose (workload 1800 2401 false) 0 costs = Some 0.
Proof. vm_compute. auto 6. Qed.

Example atomic_needs_full_segment :
  choose (workload 1800 2460 true) 0 costs = Some 1.
Proof. vm_compute. reflexivity. Qed.

(** 固定予定を含む最小例です。09:00を0とし、固定予定(ID 1)を600秒から
    1200秒まで予約します。作業(ID 0)は1200秒、締切は1800秒です。
    [schedule_selected_segment]は固定境界で短い断片を見送ります。 *)
Definition fixed_example (first_chunk t : nat) : option nat :=
  if t <? first_chunk then Some 0 else
  if t <? 600 then None else
  if t <? 1200 then Some 1 else
  if t <? 1200 + (1200 - first_chunk) then Some 0 else None.
Definition before_fixed := if usable_split 600 (1200 - 600) then 600 else 0.

(** 0秒タスクはProsaで最初から必要量に達します。Rustでは高優先度の1200秒の
    作業の後に0秒の行を置く場合があります。 *)
Definition zero_scenario :=
  {| jobs := [0;1]; deadline := fun j => if j =? 1 then Some 300 else None;
     priority := fun j => if j =? 0 then 2%Z else 1%Z;
     rank := fun _ => 0; atomic := fun _ => false |}.
Definition zero_cost j := if j =? 0 then 1200 else 0.
End Examples.

(** ** Prosa上の締切遵守

    [ideal.processor_state]では1秒に実行できるのは1件です。区間を秒単位で
    展開した[scheduled_job]をそのまま[schedule]に使い、Prosaの[service]と
    モデルの残作業の和が各タスクの初期作業量になることを証明します。 *)
Local Set Warnings "-notation-overridden,-hiding-delimiting-key".
From mathcomp Require Import ssreflect ssrbool eqtype ssrnat.
Require Import prosa.model.processor.ideal prosa.analysis.facts.behavior.service
               prosa.analysis.facts.model.ideal.schedule.

Module ProsaSchedule.
Local Notation Jobs := (Equality.clone nat _).

Definition timeline := schedule (ideal.processor_state Jobs).
Definition generated p cost : timeline := scheduled_job p cost.
Definition tick_service (slot : option nat) j :=
  match slot with Some k => if Nat.eqb j k then 1 else 0 | None => 0 end.

Lemma unit_service (sched : timeline) j t :
  service_at sched j t = tick_service (sched t) j.
Proof.
  rewrite service_at_def. case E: (sched t) => [k|] //=.
  case: (Nat.eqb_spec j k) => [->|N].
  - by rewrite eqxx.
  - have /negPf -> : (Some k != Some j) by apply/eqP; congruence.
    reflexivity.
Qed.

Lemma service_plus_remaining p cost : NoDup (jobs p) -> initially_fits p cost ->
  forall t j, Nat.add (service (generated p cost) j t) (remaining (frame_at p cost t) j) = cost j.
Proof.
  intros ND Init t. induction t as [|t IH]; intros j.
  - by rewrite service0.
  - rewrite -service_last_plus_before unit_service addnE. specialize (IH j).
    change (Nat.add (Nat.add (service (generated p cost) j t)
                 (tick_service (scheduled_job p cost t) j))
                 (remaining (advance p t (frame_at p cost t)) j) = cost j).
    unfold scheduled_job, advance. destruct (dispatch p t (frame_at p cost t))
      as [[k n]|] eqn:E; last by simpl; lia.
    destruct (dispatch_valid _ _ _ _ _ (frames_safe p cost ND Init t) E)
      as [_ [Pos [Cap _]]].
    simpl. unfold spend. destruct (Nat.eqb_spec j k); subst; lia.
Qed.

(** 主定理。数学的な仮定は有限リストのIDの一意性と、初期の累積需要条件です。
    [Some d]を持つすべての入力タスクにProsaの[completed_by]を直接適用します。 *)
Theorem all_deadlines_met p (cost : JobCost Jobs) :
  NoDup (jobs p) -> initially_fits p cost ->
  forall j d, In j (jobs p) -> deadline p j = Some d ->
    @completed_by Jobs _ (generated p cost) cost j d.
Proof.
  intros ND Init j d M D.
  pose proof (service_plus_remaining p cost ND Init d j) as Total.
  rewrite (no_work_left_at_deadline p cost j d ND Init M D) in Total.
  apply/leP. change (Nat.le (cost j) (service (generated p cost) j d)). lia.
Qed.

(** 有限の具体例を計算するときは、各秒に得た作業量を加算します。 *)
Fixpoint count_service (sched : timeline) j t :=
  match t with
  | 0 => 0
  | S u => Nat.add (count_service sched j u) (tick_service (sched u) j)
  end.
Lemma service_count sched j t : service sched j t = count_service sched j t.
Proof.
  induction t as [|t IH]; first exact: service0.
  by rewrite -service_last_plus_before unit_service IH.
Qed.

Import Examples.
Example finishes_exactly_at_deadline :
  @completed_by Jobs _ (generated scenario costs) costs 2 2460 /\
  ~~ @completed_by Jobs _ (generated scenario costs) costs 2 2459.
Proof.
  have ND : NoDup (jobs scenario) by repeat constructor; simpl; intuition discriminate.
  have Init : initially_fits scenario costs.
  { intros k d M D. destruct M as [<-|[<-|[<-|[]]]]; inversion D; vm_compute; lia. }
  split.
  - apply: (all_deadlines_met scenario costs ND Init); [simpl; auto | reflexivity].
  - pose proof (service_plus_remaining scenario costs ND Init 2459 2) as Total.
    have One : remaining (frame_at scenario costs 2459) 2 = 1 by vm_compute.
    rewrite One in Total. apply/negP => /leP Late.
    change (Nat.le 900 (service (generated scenario costs) 2 2459)) in Late.
    change (Nat.add (service (generated scenario costs) 2 2459) 1 = 900) in Total. lia.
Qed.

Definition fixed_timeline first : timeline := fixed_example first.
Definition fixed_cost : JobCost Jobs := fun j => if Nat.eqb j 0 then 1200 else 600.
Example fixed_split_counterexample :
  before_fixed = 0 /\
  @completed_by Jobs _ (fixed_timeline 600) fixed_cost 0 1800 /\
  ~~ @completed_by Jobs _ (fixed_timeline before_fixed) fixed_cost 0 1800 /\
  @completed_by Jobs _ (fixed_timeline before_fixed) fixed_cost 0 2400.
Proof.
  unfold completed_by. rewrite !service_count. by vm_compute.
Qed.

Example zero_work_is_complete_at_start :
  @completed_by Jobs _ (generated zero_scenario zero_cost) zero_cost 1 0 /\
  dispatch zero_scenario 0 (frame_at zero_scenario zero_cost 0) = Some (0,1200).
Proof. unfold completed_by. rewrite service0. by vm_compute. Qed.

End ProsaSchedule.

Print Assumptions candidate_progress.
Print Assumptions critical_group_protected.
Print Assumptions frames_safe.
Print Assumptions ProsaSchedule.service_plus_remaining.
Print Assumptions ProsaSchedule.all_deadlines_met.
Print Assumptions ProsaSchedule.finishes_exactly_at_deadline.
Print Assumptions ProsaSchedule.fixed_split_counterexample.
Print Assumptions ProsaSchedule.zero_work_is_complete_at_start.
