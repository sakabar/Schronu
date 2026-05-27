#!/bin/zsh
set -ue
set -o pipefail

# AからP列まであるスプレッドシートをクリップボードにコピーしてあるとする

pbpaste | tr -d '\r' | awk -F '\t' '
function trim(str) {
    sub(/^[[:space:]]+/, "", str)
    sub(/[[:space:]]+$/, "", str)
    return str
}

function to_minutes(time_str, parts) {
    if (time_str !~ /^[0-9]+:[0-9][0-9]:[0-9][0-9]$/) {
        printf("line %d: O列の形式が不正です: %s\n", NR, time_str) > "/dev/stderr"
        exit 1
    }

    split(time_str, parts, ":")
    return (parts[1] * 60) + parts[2]
}

{
    task_id = trim($2)
    task_name = trim($9)
    finish_flag = trim($12)
    should_extract = trim($14)
    actual_work_minutes = trim($15)

    if (should_extract != "TRUE") {
        next
    }

    if (task_id == "" && task_name != "") {
        printf("新 %s\n", task_name)
        printf("下 スプレッドシートで仮登録したタスクを見積もる\n")
        printf("予 3\n\n")
        next
    }

    if (task_id == "") {
        printf("line %d: B列が空です\n", NR) > "/dev/stderr"
        exit 1
    }

    if (actual_work_minutes == "") {
        printf("line %d: O列が空です\n", NR) > "/dev/stderr"
        exit 1
    }

    work_minutes = to_minutes(actual_work_minutes)

    if (!(task_id in total_work_minutes_by_id)) {
        task_ids[++task_id_count] = task_id
        task_name_by_id[task_id] = task_name
        total_work_minutes_by_id[task_id] = 0
        should_finish_by_id[task_id] = 0
    }

    total_work_minutes_by_id[task_id] += work_minutes

    if (finish_flag != "F") {
        should_finish_by_id[task_id] = 1
    }
}

END {
    for (i = 1; i <= task_id_count; i++) {
        task_id = task_ids[i]

        printf("# %s\n", task_name_by_id[task_id])
        printf("見 %s\n", task_id)
        printf("働 %s\n", total_work_minutes_by_id[task_id])

        if (should_finish_by_id[task_id]) {
            printf("見 %s\n", task_id)
            printf("終\n")
        }

        printf("\n")
    }
}
'
