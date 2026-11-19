(** * Rocq・Prosa環境の動作確認

    Prosaの離散時間モデルから[duration]を読み込みます。
    時間の長さに0を足しても変わらないことを、長さについての帰納法で
    証明します。この小さな証明は依存ライブラリを読み込んで検査できることの
    確認であり、Schronuの安定性は別の[Stability]モジュールで扱います。 *)
From prosa.behavior Require Import time.

(** 長さ[d]に0を加えた値は、元の長さ[d]と等しいことを示します。 *)
Example duration_add_zero (d : duration) : d + 0 = d.
Proof.
  induction d as [| d IH].
  - reflexivity.
  - simpl. rewrite IH. reflexivity.
Qed.
