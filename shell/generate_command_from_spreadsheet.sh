#!/bin/zsh
set -ue
set -o pipefail

# AからT列まであるスプレッドシートをクリップボードにコピーしてあるとする

pbpaste | tr -d '\r' | awk -F '\t' '
function trim(str) {
    sub(/^[[:space:]]+/, "", str)
    sub(/[[:space:]]+$/, "", str)
    return str
}

function to_minutes(time_str, parts) {
    if (time_str !~ /^[0-9]+:[0-9][0-9]:[0-9][0-9]$/) {
        return -1
    }

    split(time_str, parts, ":")
    return (parts[1] * 60) + parts[2]
}

function set_invalid(task_id, line_no, message) {
    if (!(task_id in invalid_line_by_id)) {
        invalid_line_by_id[task_id] = line_no
        invalid_message_by_id[task_id] = message
    }
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

    if (month < 1 || month > 12 || day < 1 || day > 31 || hour > 23 || minute > 59 || second > 59) {
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
        should_skip_by_id[task_id] = 0
    }
}

{
    task_id = trim($2)
    task_name = trim($10)
    finish_flag = trim($14)
    finish_datetime = trim($16)
    should_extract = trim($17)
    should_skip = trim($18)
    actual_work_minutes = trim($19)

    if (task_id == "" && task_name == "") {
        next
    }

    if (task_id == "" && task_name != "") {
        printf("新 %s\n", task_name)
        printf("下 スプレッドシートで仮登録したタスクを見積もる\n")
        printf("予 3\n\n")
        next
    }

    if (should_extract != "TRUE" && should_skip != "T") {
        next
    }

    if (task_id == "") {
        printf("line %d: B列が空です\n", NR) > "/dev/stderr"
        exit 1
    }

    initialize_task(task_id, task_name)

    if (should_skip == "T") {
        should_skip_by_id[task_id] = 1
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
    for (i = 1; i <= task_id_count; i++) {
        task_id = task_ids[i]

        if (!should_skip_by_id[task_id] && task_id in invalid_line_by_id) {
            printf("line %d: %s\n", invalid_line_by_id[task_id], invalid_message_by_id[task_id]) > "/dev/stderr"
            exit 1
        }

        printf("# %s\n", task_name_by_id[task_id])
        printf("見 %s\n", task_id)

        if (should_skip_by_id[task_id]) {
            printf("W\n")
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
