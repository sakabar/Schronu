From prosa.behavior Require Import time.

Example duration_add_zero (d : duration) : d + 0 = d.
Proof.
  induction d as [| d IH].
  - reflexivity.
  - simpl. rewrite IH. reflexivity.
Qed.
