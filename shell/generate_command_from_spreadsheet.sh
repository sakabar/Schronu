#!/bin/zsh
set -ue
set -o pipefail

# AからT列まであるスプレッドシートをクリップボードにコピーしてあるとする

function reject_nul_task_names() {
    local -a lines
    local line remaining task_name
    local -i line_no=0 column has_j_column

    # The system awk truncates a record at NUL, so validate J before crossing
    # that process boundary and release no partial input on failure.
    while IFS= read -r line || [[ -n $line ]]; do
        (( ++line_no ))
        lines+=("$line")
        remaining=$line
        has_j_column=1

        for column in {1..9}; do
            if [[ $remaining != *$'\t'* ]]; then
                has_j_column=0
                break
            fi
            remaining=${remaining#*$'\t'}
        done

        if (( has_j_column )); then
            task_name=${remaining%%$'\t'*}
            if [[ $task_name == *$'\0'* ]]; then
                print -u2 -- "line $line_no: J列のtask名にcontrol characterが含まれています"
                return 1
            fi
        fi
    done

    for line in "${lines[@]}"; do
        print -r -- "$line"
    done
}

if [[ $# -eq 0 ]]; then
    pbpaste
elif [[ $# -eq 1 && $1 == "--stdin" ]]; then
    cat
else
    print -u2 -- "usage: $0 [--stdin]"
    exit 2
fi | reject_nul_task_names | LC_ALL=C awk -F '\t' '
function trim(str) {
    sub(/^[[:space:]]+/, "", str)
    sub(/[[:space:]]+$/, "", str)
    return str
}

function byte(first, second, third) {
    if (third != "") {
        return sprintf("%c%c%c", first, second, third)
    }
    if (second != "") {
        return sprintf("%c%c", first, second)
    }
    return sprintf("%c", first)
}

function trim_task_name(str, changed, i, whitespace) {
    do {
        changed = 0
        for (i = 1; i <= task_name_whitespace_count; i++) {
            whitespace = task_name_whitespace[i]
            if (substr(str, 1, length(whitespace)) == whitespace) {
                str = substr(str, length(whitespace) + 1)
                changed = 1
                break
            }
        }
    } while (changed)

    do {
        changed = 0
        for (i = 1; i <= task_name_whitespace_count; i++) {
            whitespace = task_name_whitespace[i]
            if (substr(str, length(str) - length(whitespace) + 1) == whitespace) {
                str = substr(str, 1, length(str) - length(whitespace))
                changed = 1
                break
            }
        }
    } while (changed)

    return str
}

function contains_control(value, code, c1_lead) {
    for (code = 1; code <= 31; code++) {
        if (index(value, byte(code)) != 0) {
            return 1
        }
    }
    if (index(value, byte(127)) != 0) {
        return 1
    }

    # Match only the UTF-8 encoding of U+0080-U+009F. POSIX [:cntrl:]
    # operates byte-wise here and would reject unrelated Unicode task names.
    c1_lead = byte(194)
    for (code = 128; code <= 159; code++) {
        if (index(value, c1_lead byte(code)) != 0) {
            return 1
        }
    }

    return 0
}

function row_is_empty(i) {
    for (i = 1; i <= 19; i++) {
        if ($i != "") {
            return 0
        }
    }
    return 1
}

function escape_cli_argument(value, escaped, position, character) {
    escaped = ""
    for (position = 1; position <= length(value); position++) {
        character = substr(value, position, 1)
        if (character == "\\") {
            escaped = escaped "\\\\"
        } else if (character == "\"") {
            escaped = escaped "\\\""
        } else {
            escaped = escaped character
        }
    }
    return escaped
}

function to_minutes(time_str, parts) {
    if (time_str !~ /^[0-9]+:[0-9][0-9]:[0-9][0-9]$/) {
        return -1
    }

    split(time_str, parts, ":")

    if (parts[2] + 0 > 59 || parts[3] + 0 > 59) {
        return -1
    }

    return (parts[1] * 60) + parts[2]
}

function set_invalid(task_id, line_no, message) {
    if (!(task_id in invalid_line_by_id)) {
        invalid_line_by_id[task_id] = line_no
        invalid_message_by_id[task_id] = message
    }
}

function is_leap_year(year) {
    return year % 400 == 0 || (year % 4 == 0 && year % 100 != 0)
}

function days_in_month(year, month) {
    if (month == 2) {
        return is_leap_year(year) ? 29 : 28
    }

    if (month == 4 || month == 6 || month == 9 || month == 11) {
        return 30
    }

    return 31
}

function is_valid_calendar_date(year, month, day) {
    return month >= 1 && month <= 12 && day >= 1 && day <= days_in_month(year, month)
}

function parse_finish_datetime(datetime_str, parts, date_parts, time_parts, yyyy, month, day, hour, minute, second) {
    if (datetime_str !~ /^[0-9][0-9][0-9][0-9]\/[0-9]{1,2}\/[0-9]{1,2}[[:space:]]+[0-9]{1,2}:[0-9][0-9]:[0-9][0-9]$/) {
        return 0
    }

    split(datetime_str, parts, /[[:space:]]+/)
    split(parts[1], date_parts, "/")
    split(parts[2], time_parts, ":")

    yyyy = date_parts[1] + 0
    month = date_parts[2] + 0
    day = date_parts[3] + 0
    hour = time_parts[1] + 0
    minute = time_parts[2] + 0
    second = time_parts[3] + 0

    if (!is_valid_calendar_date(yyyy, month, day) || hour > 23 || minute > 59 || second > 59) {
        return 0
    }

    parsed_finish_sort_key = sprintf("%04d%02d%02d%02d%02d%02d", yyyy, month, day, hour, minute, second)
    parsed_finish_command = sprintf("%d:%02d:%02d %04d/%02d/%02d", hour, minute, second, yyyy, month, day)
    return 1
}

function initialize_task(task_id, task_name) {
    if (!(task_id in total_work_minutes_by_id)) {
        task_ids[++task_id_count] = task_id
        task_name_by_id[task_id] = task_name
        total_work_minutes_by_id[task_id] = 0
        should_finish_by_id[task_id] = 0
        defer_command_by_id[task_id] = ""
    }
}

BEGIN {
    task_name_whitespace[++task_name_whitespace_count] = byte(32)
    task_name_whitespace[++task_name_whitespace_count] = byte(194, 160)
    task_name_whitespace[++task_name_whitespace_count] = byte(225, 154, 128)
    for (code = 128; code <= 138; code++) {
        task_name_whitespace[++task_name_whitespace_count] = byte(226, 128, code)
    }
    task_name_whitespace[++task_name_whitespace_count] = byte(226, 128, 168)
    task_name_whitespace[++task_name_whitespace_count] = byte(226, 128, 169)
    task_name_whitespace[++task_name_whitespace_count] = byte(226, 128, 175)
    task_name_whitespace[++task_name_whitespace_count] = byte(226, 129, 159)
    task_name_whitespace[++task_name_whitespace_count] = byte(227, 128, 128)
}

{
    if (substr($0, length($0), 1) == byte(13)) {
        $0 = substr($0, 1, length($0) - 1)
    }

    if ($0 == "") {
        next
    }

    if (NF != 19) {
        printf("line %d: 列数が不正です: %d列 (A-Sの19列が必要です)\n", NR, NF) > "/dev/stderr"
        has_input_error = 1
        exit 1
    }

    if (row_is_empty()) {
        next
    }

    raw_task_name = $10
    validation_task_name = trim_task_name(raw_task_name)
    if (contains_control(raw_task_name)) {
        printf("line %d: J列のtask名にcontrol characterが含まれています\n", NR) > "/dev/stderr"
        has_input_error = 1
        exit 1
    }
    if (validation_task_name == "") {
        printf("line %d: J列のtask名が空です\n", NR) > "/dev/stderr"
        has_input_error = 1
        exit 1
    }
    if (validation_task_name ~ /^[+-]?[0-9]+$/) {
        printf("line %d: J列のtask名に整数だけは指定できません\n", NR) > "/dev/stderr"
        has_input_error = 1
        exit 1
    }

    task_id = trim($2)
    task_name = trim($10)
    finish_flag = trim($14)
    finish_datetime = trim($16)
    should_extract = trim($17)
    defer_command = trim($18)
    actual_work_minutes = trim($19)
    is_defer_command = defer_command == "W" || defer_command == "d"

    if (task_id == "" && task_name == "") {
        next
    }

    if (task_id == "" && task_name != "") {
        new_task_names[++new_task_count] = raw_task_name
        next
    }

    if (should_extract != "TRUE" && !is_defer_command) {
        next
    }

    if (task_id == "") {
        printf("line %d: B列が空です\n", NR) > "/dev/stderr"
        has_input_error = 1
        exit 1
    }

    initialize_task(task_id, task_name)

    if (is_defer_command) {
        if (defer_command_by_id[task_id] != "" && defer_command_by_id[task_id] != defer_command) {
            printf("line %d: R列の延期コマンドが競合しています: %s (%s, %s)\n", NR, task_id, defer_command_by_id[task_id], defer_command) > "/dev/stderr"
            has_input_error = 1
            exit 1
        }

        defer_command_by_id[task_id] = defer_command
    }

    if (should_extract != "TRUE") {
        next
    }

    if (actual_work_minutes == "") {
        set_invalid(task_id, NR, "S列が空です")
        next
    }

    work_minutes = to_minutes(actual_work_minutes)

    if (work_minutes < 0) {
        set_invalid(task_id, NR, "S列の形式が不正です: " actual_work_minutes)
        next
    }

    total_work_minutes_by_id[task_id] += work_minutes
    last_work_line_by_id[task_id] = NR

    if (finish_flag != "F") {
        if (finish_datetime == "") {
            set_invalid(task_id, NR, "P列が空です")
            next
        }

        if (!parse_finish_datetime(finish_datetime)) {
            set_invalid(task_id, NR, "P列の形式が不正です: " finish_datetime)
            next
        }

        should_finish_by_id[task_id] = 1
        if (!(task_id in finish_sort_key_by_id) || parsed_finish_sort_key > finish_sort_key_by_id[task_id]) {
            finish_sort_key_by_id[task_id] = parsed_finish_sort_key
            finish_command_by_id[task_id] = parsed_finish_command
        }
    }
}

END {
    if (has_input_error) {
        exit 1
    }

    for (i = 1; i <= task_id_count; i++) {
        task_id = task_ids[i]

        if (total_work_minutes_by_id[task_id] > 1380) {
            set_invalid(task_id, last_work_line_by_id[task_id], sprintf("働の分数が1380(23時間)を超えています: %s (%d分)", task_id, total_work_minutes_by_id[task_id]))
        }
    }

    for (i = 1; i <= task_id_count; i++) {
        task_id = task_ids[i]

        if (defer_command_by_id[task_id] == "" && task_id in invalid_line_by_id) {
            printf("line %d: %s\n", invalid_line_by_id[task_id], invalid_message_by_id[task_id]) > "/dev/stderr"
            exit 1
        }
    }

    for (i = 1; i <= new_task_count; i++) {
        printf("新 \"%s\"\n", escape_cli_argument(new_task_names[i]))
        printf("下 スプレッドシートで仮登録したタスクを見積もる\n")
        printf("予 3\n\n")
    }

    for (i = 1; i <= task_id_count; i++) {
        task_id = task_ids[i]

        printf("# %s\n", task_name_by_id[task_id])
        printf("見 %s\n", task_id)

        if (defer_command_by_id[task_id] != "") {
            printf("%s\n", defer_command_by_id[task_id])
        } else {
            printf("働 %s\n", total_work_minutes_by_id[task_id])

            if (should_finish_by_id[task_id]) {
                printf("見 %s\n", task_id)
                printf("終 %s\n", finish_command_by_id[task_id])
            }
        }

        printf("\n")
    }
}
'
