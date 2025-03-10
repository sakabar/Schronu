#!/bin/zsh
# 2024/03/16
set -ue

cell_row_num=2
cat - | grep -v -- '^----' | tr ' ' '\t' | tac | awk 'NF >= 9' | while read line_many_col; do
    line=$(echo ${line_many_col} | cut -f1-9)
    prev_cell_row_num=$[$cell_row_num - 1]

    hour="LEFT(MID(E${cell_row_num}, 10, 5), 2)"
    minute="RIGHT(MID(E${cell_row_num}, 10, 5), 2)"
    now_time='TIME(HOUR($Q$2),MINUTE($Q$2),SECOND($Q$2))'

    manu_cell="=NOT(ISFORMULA(K${cell_row_num}))"
    k_cell="=MAX(TIME(${hour}, ${minute}, 0), L${prev_cell_row_num}, ${now_time})"

    l_cell='='"K${cell_row_num}"'+time(0, '"G${cell_row_num}"', 0)'

    echo ${line}"\t${manu_cell}\t${k_cell}\t${l_cell}"

    cell_row_num=$[$cell_row_num + 1]
done

tabs_line=$(seq  1 10 | awk '{print ""}'| tr '\n' '\t')
yes ${tabs_line} | head -n 50
