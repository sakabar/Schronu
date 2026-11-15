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
opam switch create . ocaml-base-compiler.4.14.2 \
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
