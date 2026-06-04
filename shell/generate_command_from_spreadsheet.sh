#!/bin/zsh
set -ue
set -o pipefail

# AからQ列まであるスプレッドシートをクリップボードにコピーしてあるとする

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
    task_name = trim($9)
    finish_flag = trim($12)
    should_skip = trim($14)
    should_extract = trim($15)
    actual_work_minutes = trim($16)

    if (should_extract != "TRUE" && should_skip != "T") {
        next
    }

    if (task_id == "" && task_name != "") {
        if (should_skip == "T") {
            next
        }

        printf("新 %s\n", task_name)
        printf("下 スプレッドシートで仮登録したタスクを見積もる\n")
        printf("予 3\n\n")
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
        if (!(task_id in invalid_actual_line_by_id)) {
            invalid_actual_line_by_id[task_id] = NR
            invalid_actual_message_by_id[task_id] = "P列が空です"
        }
        next
    }

    work_minutes = to_minutes(actual_work_minutes)

    if (work_minutes < 0) {
        if (!(task_id in invalid_actual_line_by_id)) {
            invalid_actual_line_by_id[task_id] = NR
            invalid_actual_message_by_id[task_id] = "P列の形式が不正です: " actual_work_minutes
        }
        next
    }

    total_work_minutes_by_id[task_id] += work_minutes

    if (finish_flag != "F") {
        should_finish_by_id[task_id] = 1
    }
}

END {
    for (i = 1; i <= task_id_count; i++) {
        task_id = task_ids[i]

        if (!should_skip_by_id[task_id] && task_id in invalid_actual_line_by_id) {
            printf("line %d: %s\n", invalid_actual_line_by_id[task_id], invalid_actual_message_by_id[task_id]) > "/dev/stderr"
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
                printf("終\n")
            }
        }

        printf("\n")
    }
}
'
