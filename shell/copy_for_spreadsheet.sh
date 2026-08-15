#!/bin/zsh
# 2024/03/16
set -ue

cell_row_num=3
cat - | awk '
/^0/ && !/^----/ {
    line = $0
    for (i = 1; i <= 9; i++) {
        sub(/^[[:space:]]+/, "", line)
        if (!match(line, /^[^[:space:]]+/)) {
            next
        }
        column[i] = substr(line, RSTART, RLENGTH)
        line = substr(line, RSTART + RLENGTH)
    }

    sub(/^[[:space:]]+/, "", line)
    if (line == "") {
        next
    }
    gsub(/[[:space:]]+/, " ", line)

    for (i = 1; i <= 9; i++) {
        printf "%s\t", column[i]
    }
    print line
}
' | tac | while IFS=$'\t' read -r rank task_id icon remaining_time scheduled_time priority estimated_minutes project_number category task_name; do
    prev_cell_row_num=$[$cell_row_num - 1]

    hour="LEFT(MID(E${cell_row_num}, 10, 5), 2)"
    minute="RIGHT(MID(E${cell_row_num}, 10, 5), 2)"
    now_time='TIME(HOUR($V$2),MINUTE($V$2),SECOND($V$2))'

    manu_cell="=NOT(ISFORMULA(L${cell_row_num}))"
    k_cell="=MAX(TIME(${hour}, ${minute}, 0), V${prev_cell_row_num}, ${now_time})"

    l_cell="=L${cell_row_num}"
    m_cell=''
    n_cell="=P${cell_row_num}"
    o_cell='=IF(OR(R'"${cell_row_num}"'="W", R'"${cell_row_num}"'="d"), L'"${cell_row_num}"', L'"${cell_row_num}"'+TIME(0, G'"${cell_row_num}"', 0))'
    p_cell="=NOT(ISFORMULA(P${cell_row_num}))"
    q_cell=''

    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "${rank}" "${task_id}" "${icon}" "${remaining_time}" "${scheduled_time}" \
        "${priority}" "${estimated_minutes}" "${project_number}" "${category}" "${task_name}" \
        "${manu_cell}" "${k_cell}" "${l_cell}" "${m_cell}" "${n_cell}" "${o_cell}" \
        "${p_cell}" "${q_cell}"

    cell_row_num=$[$cell_row_num + 1]
done

tabs_line=$(seq 1 10 | awk '{print ""}' | tr '\n' '\t')
yes "${tabs_line}" | head -n 50
