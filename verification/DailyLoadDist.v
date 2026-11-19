(** * 日別負荷の正規分布と、失敗確率を50%%未満にする追加時間

    対象は、同じ日別予測のもとでの残り枠と失敗確率の関係です。
    すべての時間は秒、追加で空ける時間は非負の整数分で表します。 *)
From mathcomp Require Import all_ssreflect ssralg ssrnum ssrint archimedean interval.
From mathcomp Require Import boolp classical_sets set_interval reals ereal.
From mathcomp Require Import topology normedtype measure lebesgue_measure lebesgue_integral.
From mathcomp Require Import exp ftc probability.

Set Implicit Arguments.
Unset Strict Implicit.
Unset Printing Implicit Defensive.
Import Order.TTheory GRing.Theory Num.Def Num.Theory.
Import numFieldTopology.Exports.
Local Open Scope classical_set_scope.
Local Open Scope ring_scope.

Section GaussianTail.
Context {R : realType}.
Local Notation P := (@normal_prob R 0 1).
Local Notation density := (@normal_pdf R 0 1).

(** 標準正規分布がしきい値を厳密に超える確率です。 *)
Definition standard_tail (x : R) : R := fine (P `]x, +oo[%classic).

Lemma standard_tail_bounds x : 0 <= standard_tail x <= 1.
Proof.
apply/andP; split; first exact/fine_ge0/measure_ge0.
by apply: (@fine_le _ _ 1%E); rewrite ?fin_num_measure//; exact: probability_le1.
Qed.

Lemma standard_tail_antitone (x y : R) :
  x <= y -> standard_tail y <= standard_tail x.
Proof.
move=> xy; apply: fine_le; rewrite ?fin_num_measure//.
by apply: le_measure; rewrite ?inE//; apply: subset_itvr; rewrite bnd_simp.
Qed.

(** 全実数で密度は正であり、正の長さの区間には必ず正の確率があります。 *)
Local Lemma interval_mass_positive (a b : R) : a < b -> (0 < P `]a, b]%classic)%E.
Proof.
move=> ab; rewrite lt0e measure_ge0 andbT; apply/eqP => zero.
pose D := `]a, b]%classic.
have mD : measurable D by exact: measurable_itv.
have md : measurable_fun D (fun x => (density x)%:E).
  by apply/measurable_EFinP/measurable_funTS; exact: measurable_normal_pdf.
have abs_zero : (\int[lebesgue_measure]_(x in D) `|(density x)%:E| = 0)%E.
  under eq_integral do rewrite gee0_abs ?lee_fin ?normal_pdf_ge0//.
  exact: zero.
move/(ae_eq_integral_abs lebesgue_measure mD md): abs_zero => [N [mN nullN bad]].
have DN : D `<=` N.
  move=> x Dx; apply: bad; apply/not_implyP; split => //=.
  have pos : 0 < density x.
    by rewrite /normal_pdf oner_eq0 /= mulr_gt0 ?normal_peak_gt0 ?expR_gt0.
  by apply/eqP; rewrite gt_eqF// lte_fin.
have len0 : lebesgue_measure D = 0%E.
  by apply: (subset_measure0 _ mN DN nullN); exact: measurable_itv.
move: len0; rewrite /D lebesgue_measure_itv/= lte_fin ab.
by move/eqP; rewrite -EFinB eqe subr_eq0 (gt_eqF ab).
Qed.

Lemma standard_tail_strict (x y : R) :
  x < y -> standard_tail y < standard_tail x.
Proof.
move=> xy; apply: fine_lt; rewrite ?fin_num_measure//.
have sub : `]y, +oo[ `<=` `]x, +oo[%classic.
  by apply: subset_itvr; rewrite bnd_simp ltW.
have := interval_mass_positive xy.
rewrite set_itv_splitD /= measureD ?setIidr//; last first.
  by rewrite (le_lt_trans (probability_le1 _ _)) ?ltry.
by rewrite sube_gt0.
Qed.

Lemma standard_tail_open (x : R) : 0 < standard_tail x < 1.
Proof.
have right : x < x + 1 by rewrite ltrDl.
have left : x - 1 < x by rewrite gtrBl.
apply/andP; split.
- exact: le_lt_trans (andP (standard_tail_bounds _)).1 (standard_tail_strict right).
- exact: lt_le_trans (standard_tail_strict left) (andP (standard_tail_bounds _)).2.
Qed.

(** 対称な密度の全積分は片側の2倍です。1点の積分は0なので、開区間と
    閉区間の違いは確率に影響しません。 *)
Lemma standard_tail_zero : standard_tail (0 : R) = 1 / 2.
Proof.
have symmetric : density =1 density \o -%R.
  by move=> x /=; rewrite /normal_pdf oner_eq0 /= /normal_fun !subr0 sqrrN.
have twice := @ge0_symfun_integralT R density (@normal_pdf_ge0 R 0 1)
  (@continuous_normal_pdf R 0 1 (oner_neq0 R)) symmetric.
rewrite integral_normal_pdf -set_itvcy -integral_itv_obnd_cbnd in twice;
  last by apply: measurable_funTS; exact: measurable_normal_pdf.
change (1%E = (2%:E * P `]0%R, +oo[%classic)%E) in twice.
have eq1 := congr1 fine twice.
rewrite fineM ?fin_num_measure// in eq1.
change (1 = 2 * standard_tail (0 : R)) in eq1.
by rewrite [X in X / _]eq1 [2 * _]mulrC mulfK// pnatr_eq0.
Qed.
End GaussianTail.

(** ** 追加で空ける最小の整数分

    [mu] は平均残作業秒数、[capacity] は現在の残り枠(秒)です。
    分数を整数に丸めた後、[Num.truncn] で負の値を0分にします。 *)
Section MinimumMinutes.
Context {R : archiRealFieldType}.

Definition strict_minutes (mu capacity : R) : nat :=
  Num.truncn (Num.floor ((mu - capacity) / 60) + 1).

Definition inclusive_minutes (mu capacity : R) : nat :=
  Num.truncn (Num.ceil ((mu - capacity) / 60)).

Local Lemma truncn_int_le_nat (z : int) (n : nat) :
  (Num.truncn z <= n)%N = (z <= Posz n).
Proof. by case: z => k. Qed.

(** 正の分散では枠が平均を厳密に超える必要があります。以下の同値は、
    指定の分数で達成できることと、どの小さい非負整数分でも不足することを表します。 *)
Lemma strict_minutesP mu capacity (n : nat) :
  (strict_minutes mu capacity <= n)%N = (mu < capacity + 60 * n%:R).
Proof.
rewrite /strict_minutes truncn_int_le_nat lezD1 floor_lt_int.
by rewrite ltr_pdivrMr ?ltr0Sn // ltrBlDl mulrC.
Qed.

(** 分散0では枠が平均に一致した時点で超過確率は0になります。 *)
Lemma inclusive_minutesP mu capacity (n : nat) :
  (inclusive_minutes mu capacity <= n)%N = (mu <= capacity + 60 * n%:R).
Proof.
rewrite /inclusive_minutes truncn_int_le_nat ceil_le_int_tmp.
by rewrite ler_pdivrMr ?ltr0Sn // lerBlDl mulrC.
Qed.

End MinimumMinutes.

Section MinimumMinutesExamples.
Context {R : archiRealFieldType}.

(** 枠が平均と同じ日には、分散が正ならあと1分、分散0ならあと0分です。 *)
Example strict_minutes_at_mean (mu : R) : strict_minutes mu mu = 1%N.
Proof. by rewrite /strict_minutes subrr mul0r floor0 add0r. Qed.

Example inclusive_minutes_at_mean (mu : R) : inclusive_minutes mu mu = 0%N.
Proof. by rewrite /inclusive_minutes subrr mul0r ceil0. Qed.

(** 正の分散でも、枠が平均を超えていれば追加は0分です。 *)
Example strict_minutes_already_sufficient (mu capacity : R) :
    mu < capacity -> strict_minutes mu capacity = 0%N.
Proof. by move=> gt; apply/eqP; rewrite -leqn0 strict_minutesP mulr0 addr0. Qed.

End MinimumMinutesExamples.

Section DailyLoad.
Context {R : realType}.

(** ** 日別負荷の失敗確率と整数表示

    [mu] は対象日の平均残作業秒数、[sigma] は日別に合成した分散の平方根です。
    枠が負なら失敗は確実です。分散0では平均との厳密な超過を判定します。 *)
Definition failure_probability (mu sigma capacity : R) :=
  if capacity < 0 then 1
  else if sigma == 0 then (if capacity < mu then 1 else 0)
  else standard_tail ((capacity - mu) / sigma).

Definition percent (p : R) := Num.truncn (p *+ 100).

Definition failure_percent mu sigma capacity :=
  percent (failure_probability mu sigma capacity).

(** 予測欠落と、予測値が0%%である場合を区別します。 *)
Definition forecast_percent (forecast : option (R * R)) capacity :=
  omap (fun '(mu, sigma) => failure_percent mu sigma capacity) forecast.

Lemma missing_forecast capacity : forecast_percent None capacity = None.
Proof. by []. Qed.

Lemma failure_bounds mu sigma capacity :
  0 <= failure_probability mu sigma capacity <= 1.
Proof.
rewrite /failure_probability; case: ifP => _; first by rewrite ler01 lexx.
case: ifP => _; last exact: standard_tail_bounds.
by case: ifP => _; rewrite ?ler01 ?lexx.
Qed.

Lemma percent_mono : {homo percent : x y / x <= y >-> (x <= y)%N}.
Proof. by move=> x y xy; apply/le_truncn/ler_wMn2r. Qed.

Lemma percent_zero : percent 0 = 0%N.
Proof. by rewrite /percent mul0rn truncn0. Qed.

Lemma percent_one : percent 1 = 100%N.
Proof. exact: natrK. Qed.

Lemma percent_bounds mu sigma capacity : (failure_percent mu sigma capacity <= 100)%N.
Proof. by rewrite -percent_one; apply/percent_mono; case/andP: (failure_bounds mu sigma capacity). Qed.

Lemma half_scaled : (1 / 2 : R) *+ 100 = 50%:R.
Proof.
change ((1 / 2 : R) *+ (2 * 50)%N = 50%:R).
by rewrite mulrnA -[(_ / _) *+ 2]mulr_natr divfK // pnatr_eq0.
Qed.

Lemma percent_half : percent (1 / 2) = 50%N.
Proof. by rewrite /percent half_scaled natrK. Qed.

(** 切り捨てで、整数表示と丸め前の50%%境界が一致します。 *)
Lemma percent_below_half p : 0 <= p -> (percent p < 50)%N = (p < 1 / 2).
Proof.
move=> p0; rewrite /percent truncn_lt_nat ?mulrn_wge0 //.
by rewrite -half_scaled ltr_pMn2r.
Qed.

Lemma percent_fifty_interval p : 0 <= p ->
  (percent p == 50%N) = (50%:R <= p *+ 100 < 51%:R).
Proof.
move=> p0; rewrite eqn_leq /percent andbC truncn_ge_nat ?mulrn_wge0 //.
by rewrite -ltnS truncn_lt_nat ?mulrn_wge0.
Qed.

Lemma negative_capacity_percent mu sigma capacity :
  capacity < 0 -> failure_percent mu sigma capacity = 100%N.
Proof. by move=> c0; rewrite /failure_percent /failure_probability c0 percent_one. Qed.

Lemma deterministic_percent mu capacity : 0 <= capacity ->
  failure_percent mu 0 capacity = (if capacity < mu then 100%N else 0%N).
Proof.
by move=> c0; rewrite /failure_percent /failure_probability ltNge c0 /= eqxx;
  case: ifP => _; rewrite ?percent_zero ?percent_one.
Qed.

Lemma at_mean_probability mu sigma : 0 <= mu -> 0 < sigma ->
  failure_probability mu sigma mu = 1 / 2.
Proof.
move=> mu0 s0; rewrite /failure_probability (le_gtF mu0) (gt_eqF s0).
by rewrite subrr mul0r standard_tail_zero.
Qed.

Lemma at_mean_percent mu sigma : 0 <= mu -> 0 < sigma ->
  failure_percent mu sigma mu = 50%N.
Proof. by move=> mu0 s0; rewrite /failure_percent at_mean_probability // percent_half. Qed.

(** 同じ日別予測に対して残り枠を増やすと、失敗確率も表示値も上がりません。 *)
Lemma failure_antitone mu sigma a b : 0 <= sigma -> a <= b ->
  failure_probability mu sigma b <= failure_probability mu sigma a.
Proof.
move=> s0 ab; case az: (a < 0).
  rewrite [failure_probability mu sigma a]/failure_probability az.
  by case/andP: (failure_bounds mu sigma b).
have bz : (b < 0) = false by apply: le_gtF; apply: le_trans ab; rewrite leNgt az.
rewrite /failure_probability az bz.
case: ifP => [sZ|sZ].
  case: ifP => [bm|bm]; first by rewrite (le_lt_trans ab bm) lexx.
  by case: ifP; rewrite ?ler01 ?lexx.
apply: standard_tail_antitone; apply: ler_wpM2r.
- by rewrite invr_ge0.
- by rewrite lerD2r.
Qed.

Lemma capacity_percent_antitone mu sigma a b : 0 <= sigma -> a <= b ->
  (failure_percent mu sigma b <= failure_percent mu sigma a)%N.
Proof. by move=> s0 ab; apply/percent_mono/failure_antitone. Qed.

(** [unavailable]、[elapsed]、[single] はWebが非負に補正した秒数です。
    バッファONで単発残作業の30%%を追加で引きます。 *)
Definition remaining_capacity (unavailable elapsed single : R) (buffer : bool) :=
  24 * 60 * 60 - unavailable - elapsed - single -
    (if buffer then single * (3 / 10) else 0).

(** 0.1秒単位の整数で実装する式を秒に戻すと、単発作業を1倍または1.3倍で控除した残り枠になります。 *)
Lemma remaining_capacity_tenthsE (unavailable elapsed single : R) (buffer : bool) :
  (10 * (24 * 60 * 60 - unavailable - elapsed - single) -
    (if buffer then single * 3 else 0)) / 10 = remaining_capacity unavailable elapsed single buffer.
Proof.
rewrite /remaining_capacity mulrBl [10 * _]mulrC mulfK ?pnatr_eq0 //.
by case: buffer; rewrite ?mul0r ?mulrA.
Qed.

Lemma buffer_percent mu sigma unavailable elapsed single :
  0 <= sigma -> 0 <= single ->
  (failure_percent mu sigma (remaining_capacity unavailable elapsed single false) <=
   failure_percent mu sigma (remaining_capacity unavailable elapsed single true))%N.
Proof.
move=> s0 u0; apply: capacity_percent_antitone => //.
rewrite /remaining_capacity subr0 gerBl.
by rewrite mulr_ge0 // divr_ge0 ?ler0n.
Qed.

Lemma buffer_zero_single unavailable elapsed :
  remaining_capacity unavailable elapsed 0 true = remaining_capacity unavailable elapsed 0 false.
Proof. by rewrite /remaining_capacity mul0r. Qed.

Lemma tail_below_half (x : R) : (standard_tail x < 1 / 2) = (0 < x).
Proof.
rewrite -standard_tail_zero; apply/idP/idP; last exact: standard_tail_strict.
by apply: contraLR; rewrite -!leNgt; exact: standard_tail_antitone.
Qed.

(** 平均残作業は非負です。この条件により、負の枠を100%%とする分岐も含めて
    必要十分条件を記述できます。 *)
Theorem below_half_capacity mu sigma capacity : 0 <= mu -> 0 <= sigma ->
  (failure_percent mu sigma capacity < 50)%N =
    (if sigma == 0 then mu <= capacity else mu < capacity).
Proof.
move=> mu0 s0; rewrite /failure_percent percent_below_half;
  last by case/andP: (failure_bounds mu sigma capacity).
rewrite /failure_probability; case: ifP => [c0|c0].
  have cm := lt_le_trans c0 mu0.
  rewrite ltr_pdivlMr ?ltr0n // mul1r ltrn1.
  by case: ifP => _; rewrite ?(lt_geF cm) ?(le_gtF (ltW cm)).
case: ifP => [sZ|sZ].
  rewrite leNgt; case: ifP => _; rewrite ltr_pdivlMr ?ltr0n //.
  - by rewrite mul1r ltrn1.
  - by rewrite mul0r ltr01.
have sp : 0 < sigma by rewrite lt_def s0 andbT sZ.
by rewrite tail_below_half ltr_pdivlMr // mul0r subr_gt0.
Qed.

(** 平均より小さい非負の枠では、丸め前の確率は厳密に半分を超えます。 *)
Lemma above_half_when_short mu sigma capacity :
  0 <= capacity -> capacity < mu -> 0 < sigma ->
  1 / 2 < failure_probability mu sigma capacity.
Proof.
move=> c0 cm sp; rewrite /failure_probability (le_gtF c0) (gt_eqF sp).
rewrite -standard_tail_zero; apply: standard_tail_strict.
by rewrite ltr_pdivrMr // mul0r subr_lt0.
Qed.

Lemma normal_half_iff mu sigma capacity : 0 <= capacity -> 0 < sigma ->
  (failure_probability mu sigma capacity = 1 / 2 <-> capacity = mu).
Proof.
move=> c0 sp; split; last by move=> <-; apply: at_mean_probability.
move=> half; case: (ltgtP capacity mu) => // h.
- by move: (above_half_when_short c0 h sp); rewrite half ltxx.
- have pos : 0 < (capacity - mu) / sigma by apply: divr_gt0; rewrite ?subr_gt0.
  rewrite /failure_probability (le_gtF c0) (gt_eqF sp) in half.
  by move: (standard_tail_strict pos); rewrite half standard_tail_zero ltxx.
Qed.

(** 正の分散の有限な正規予測では、枠が非負なら100%%には達しません。 *)
Lemma normal_percent_lt_hundred mu sigma capacity : 0 <= capacity -> 0 < sigma ->
  (failure_percent mu sigma capacity < 100)%N.
Proof.
move=> c0 sp; rewrite /failure_percent /failure_probability (le_gtF c0) (gt_eqF sp).
case/andP: (standard_tail_open ((capacity - mu) / sigma)) => /ltW p0 p1.
by rewrite /percent truncn_lt_nat ?mulrn_wge0 // ltr_pMn2r.
Qed.

(** 最小分数がn以下であることと、枠をn分増やした表示が50%%未満になることは同値です。 *)
Definition minimum_minutes (mu sigma capacity : R) :=
  if sigma == 0 then inclusive_minutes mu capacity else strict_minutes mu capacity.

Definition forecast_minutes (forecast : option (R * R)) capacity :=
  omap (fun '(mu, sigma) => minimum_minutes mu sigma capacity) forecast.

Lemma missing_forecast_minutes capacity : forecast_minutes None capacity = None.
Proof. by []. Qed.

Theorem minimum_minutesP mu sigma capacity (n : nat) : 0 <= mu -> 0 <= sigma ->
  (minimum_minutes mu sigma capacity <= n)%N =
  (failure_percent mu sigma (capacity + 60 * n%:R) < 50)%N.
Proof.
move=> mu0 s0; rewrite below_half_capacity // /minimum_minutes.
by case: ifP => _; rewrite ?inclusive_minutesP ?strict_minutesP.
Qed.

(** 下限0で平均が切られ、分散が残る場合の境界です。 *)
Example zero_mean_boundary (sigma : R) : 0 < sigma ->
  failure_percent 0 sigma 0 = 50%N /\ failure_percent 0 sigma (-1) = 100%N.
Proof.
move=> sp; split; first by apply: at_mean_percent; rewrite ?lexx.
by apply: negative_capacity_percent; rewrite oppr_lt0 ltr01.
Qed.

End DailyLoad.

Print Assumptions standard_tail_strict.
Print Assumptions standard_tail_zero.
Print Assumptions percent_fifty_interval.
Print Assumptions capacity_percent_antitone.
Print Assumptions buffer_percent.
Print Assumptions below_half_capacity.
Print Assumptions normal_half_iff.
Print Assumptions normal_percent_lt_hundred.
Print Assumptions minimum_minutesP.
Print Assumptions zero_mean_boundary.
