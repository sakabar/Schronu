#!/bin/zsh
# 2024/03/16
set -ue

function is_next_weekday() {
    local previous_weekday=$1
    local current_weekday=$2
    local expected_weekday

    case ${previous_weekday} in
        月) expected_weekday=火 ;;
        火) expected_weekday=水 ;;
        水) expected_weekday=木 ;;
        木) expected_weekday=金 ;;
        金) expected_weekday=土 ;;
        土) expected_weekday=日 ;;
        日) expected_weekday=月 ;;
        *) return 1 ;;
    esac

    [[ ${current_weekday} == ${expected_weekday} ]]
}

function is_next_calendar_date() {
    local previous_date=$1
    local current_date=$2
    local previous_weekday=$3
    local current_weekday=$4
    local previous_month=${previous_date%%/*}
    local previous_day=${previous_date#*/}
    local current_month=${current_date%%/*}
    local current_day=${current_date#*/}

    is_next_weekday "${previous_weekday}" "${current_weekday}" || return 1

    if (( 10#${previous_month} == 10#${current_month} && 10#${current_day} == 10#${previous_day} + 1 )); then
        return 0
    fi

    case ${previous_month} in
        01|03|05|07|08|10)
            (( 10#${previous_day} == 31 && 10#${current_month} == 10#${previous_month} + 1 && 10#${current_day} == 1 ))
            ;;
        04|06|09|11)
            (( 10#${previous_day} == 30 && 10#${current_month} == 10#${previous_month} + 1 && 10#${current_day} == 1 ))
            ;;
        02)
            (( (10#${previous_day} == 28 || 10#${previous_day} == 29) && 10#${current_month} == 3 && 10#${current_day} == 1 ))
            ;;
        12)
            (( 10#${previous_day} == 31 && 10#${current_month} == 1 && 10#${current_day} == 1 ))
            ;;
        *)
            return 1
            ;;
    esac
}

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

    scheduled_date=${scheduled_time%%\(*}
    scheduled_weekday=${scheduled_time#*\(}
    scheduled_weekday=${scheduled_weekday%%\)*}
    scheduled_start=${scheduled_time#*\)-}
    scheduled_start=${scheduled_start%%\~*}
    scheduled_hour=${scheduled_start%%:*}
    scheduled_minute=${scheduled_start#*:}
    scheduled_start_minutes=$((10#${scheduled_hour} * 60 + 10#${scheduled_minute}))
    sleep_minutes=0

    if (( cell_row_num > 3 )); then
        if [[ ${scheduled_date} == ${previous_scheduled_date} ]]; then
            if (( previous_scheduled_start_minutes < 360 && scheduled_start_minutes >= 360 )); then
                sleep_minutes=420
            fi
        elif (( scheduled_start_minutes >= 360 )); then
            sleep_minutes=420
        elif ! is_next_calendar_date \
            "${previous_scheduled_date}" \
            "${scheduled_date}" \
            "${previous_scheduled_weekday}" \
            "${scheduled_weekday}"; then
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
    if (( sleep_minutes > 0 )); then
        o_cell='=IF(OR(R'"${cell_row_num}"'="W", R'"${cell_row_num}"'="d"), L'"${cell_row_num}"', L'"${cell_row_num}"'+TIME(0, G'"${cell_row_num}"'+'"${sleep_minutes}"', 0))'
    else
        o_cell='=IF(OR(R'"${cell_row_num}"'="W", R'"${cell_row_num}"'="d"), L'"${cell_row_num}"', L'"${cell_row_num}"'+TIME(0, G'"${cell_row_num}"', 0))'
    fi
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
    cell_row_num=$[$cell_row_num + 1]
done

tabs_line=$(seq 1 10 | awk '{print ""}' | tr '\n' '\t')
yes "${tabs_line}" | head -n 50
