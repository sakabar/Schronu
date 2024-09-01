#!/bin/bash

# one rhoはこのコマンドのまま変えずに取得できる
# grep 'Lq'| grep -o '([^ ]*' | tr -d '(' | tr '\n' '\t' | cut -f1-2 | sed -e 's/\n$//'

# rep rho
grep 'Lq'| grep 'rep' | grep -o '([^)]\+)' | while read exp; do
    echo $exp | bc
done | awk '{ printf("%.2f\n", $0) }' | tr '\n' '\t' | cut -f1-2 | sed -e 's/\n$//'
