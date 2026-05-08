#!/bin/zsh
set -ue
set -o pipefail

# AからO列まであるスプレッドシートをクリップボードにコピーしてあるとする

pbpaste | tr -d '\r' | awk -F '\t' '
function trim(str) {
    sub(/^[[:space:]]+/, "", str)
    sub(/[[:space:]]+$/, "", str)
    return str
}

{
    task_id = trim($2)
    task_name = trim($9)
    finish_flag = trim($13)
    should_extract = trim($14)
    actual_work_minutes = trim($15)

    if (should_extract != "TRUE") {
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

    printf("# %s\n", task_name)
    printf("見 %s\n", task_id)
    printf("働 %s\n", actual_work_minutes)

    if (finish_flag != "F") {
        printf("見 %s\n", task_id)
        printf("終\n")
    }
}
'
