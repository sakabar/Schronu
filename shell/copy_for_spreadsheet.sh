#!/bin/zsh
# 2024/03/16
set -ue
set -o pipefail

cell_row_num=3
weekday_order='月火水木金土日月'
month_offset=(0 31 59 90 120 151 181 212 243 273 304 334)
task_rows_output=$(
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

    task_row = ""
    for (i = 1; i <= 9; i++) {
        task_row = task_row column[i] "\t"
    }
    task_rows[++task_row_count] = task_row line
}

END {
    for (i = task_row_count; i >= 1; i--) {
        print task_rows[i]
    }
}
' | while IFS=$'\t' read -r rank task_id icon remaining_time scheduled_time priority estimated_minutes project_number category task_name; do
    prev_cell_row_num=$[$cell_row_num - 1]

    scheduled_date=${scheduled_time%%\(*}
    scheduled_weekday=${scheduled_time#*\(}
    scheduled_weekday=${scheduled_weekday%%\)*}
    scheduled_start=${scheduled_time#*\)-}
    scheduled_start=${scheduled_start%%\~*}
    scheduled_hour=${scheduled_start%%:*}
    scheduled_minute=${scheduled_start#*:}
    scheduled_start_minutes=$((10#${scheduled_hour} * 60 + 10#${scheduled_minute}))
    scheduled_month=${scheduled_date%%/*}
    scheduled_day=${scheduled_date#*/}
    scheduled_ordinal=$((month_offset[10#${scheduled_month}] + 10#${scheduled_day}))
    sleep_minutes=0

    if (( cell_row_num > 3 )); then
        calendar_day_gap=$(((scheduled_ordinal - previous_scheduled_ordinal + 365) % 365))
        [[ ${previous_scheduled_date} == '02/29' && ${scheduled_date} == '03/01' ]] && calendar_day_gap=1
        if [[ ${scheduled_date} == ${previous_scheduled_date} ]]; then
            if (( previous_scheduled_start_minutes < 360 && scheduled_start_minutes >= 360 )); then
                sleep_minutes=420
            fi
        elif (( scheduled_start_minutes >= 360 || previous_scheduled_start_minutes < 360 )) ||
            (( calendar_day_gap != 1 )) ||
            [[ ${weekday_order} != *${previous_scheduled_weekday}${scheduled_weekday}* ]]; then
            sleep_minutes=420
        fi
    fi

    hour="LEFT(MID(E${cell_row_num}, 10, 5), 2)"
    minute="RIGHT(MID(E${cell_row_num}, 10, 5), 2)"
    now_time='TIME(HOUR($V$2),MINUTE($V$2),SECOND($V$2))'

    manu_cell="=NOT(ISFORMULA(L${cell_row_num}))"
    k_cell="=MAX(TIME(${hour}, ${minute}, 0), V${prev_cell_row_num}, ${now_time})"

    l_cell="=L${cell_row_num}"
    m_cell=''
    n_cell="=P${cell_row_num}"
    estimated_minutes_formula="G${cell_row_num}"
    if (( sleep_minutes > 0 )); then
        m_cell='F'
        estimated_minutes_formula+="+${sleep_minutes}"
    fi
    o_cell='=IF(OR(R'"${cell_row_num}"'="W", R'"${cell_row_num}"'="d"), L'"${cell_row_num}"', L'"${cell_row_num}"'+TIME(0, '"${estimated_minutes_formula}"', 0))'
    p_cell="=NOT(ISFORMULA(P${cell_row_num}))"
    q_cell=''

    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "${rank}" "${task_id}" "${icon}" "${remaining_time}" "${scheduled_time}" \
        "${priority}" "${estimated_minutes}" "${project_number}" "${category}" "${task_name}" \
        "${manu_cell}" "${k_cell}" "${l_cell}" "${m_cell}" "${n_cell}" "${o_cell}" \
        "${p_cell}" "${q_cell}"

    previous_scheduled_date=${scheduled_date}
    previous_scheduled_weekday=${scheduled_weekday}
    previous_scheduled_start_minutes=${scheduled_start_minutes}
    previous_scheduled_ordinal=${scheduled_ordinal}
    cell_row_num=$[$cell_row_num + 1]
done
) || exit $?

if [[ -n ${task_rows_output} ]]; then
    print -r -- "${task_rows_output}"
fi
tabs_line=$'\t\t\t\t\t\t\t\t\t\t'
repeat 50 print -r -- "${tabs_line}"
