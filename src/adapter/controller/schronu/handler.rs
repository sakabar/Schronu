use super::command::{self, Command, CommandKind};
use super::renderer::{self, DisplayModel, SchronuWriter};
use super::{
    decide_finish_time, decide_time, execute_breakdown, execute_breakdown_sequentially,
    execute_create_repetition_task, execute_defer, execute_defer_all_frequent_routines,
    execute_defer_routine, execute_extrude_with_config, execute_focus, execute_make_appointment,
    execute_next_up, execute_pack_with_config, execute_pick, execute_set_actual_work_minutes,
    execute_set_arrange_children_work_minutes, execute_set_deadline_with_config,
    execute_set_estimated_work_minutes, execute_set_priority, execute_set_project_category,
    execute_show_ancestor, execute_show_leaf_tasks, execute_show_tree, execute_split,
    execute_start_new_project, execute_unfocus, execute_wait_for_others, get_weekday_jp,
    parse_clear_or_gather_defer_to_datetime, parse_dated_clear_or_gather_time_range,
    report_application_result, report_command_result, resolve_show_all_pattern,
    resolve_upcoming_mmdd, scheduled_leaf_starts_on_schronu_day, split_amount_and_unit,
    write_command_error, write_flatten_result, CommandError, TaskListDisplayOrder,
    DEFAULT_LOWEST_PRIORITY_RECENT_DAYS,
};
use chrono::{DateTime, Datelike, Duration, Local, LocalResult, TimeZone, Timelike};
use regex::Regex;
use schronu::adapter::gateway::schronu_config::SchronuConfig;
use schronu::application::flatten_use_case::flatten_tasks_with_end_of_day_offset_minutes;
use schronu::application::interface::{FreeTimeManagerTrait, TaskRepositoryTrait};
use schronu::application::task_use_case::{complete_task, ApplicationError, CompleteTaskInput};
use schronu::entity::datetime::{get_next_morning_datetime, parse_local_datetime};
use schronu::entity::task::{extract_leaf_tasks_from_project_with_pending, Status, TaskHandle};
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CommandExecution {
    pub(super) kind: CommandKind,
    pub(super) display: DisplayModel,
    pub(super) external_request: Option<ExternalRequest>,
    pub(super) focus_request: Option<FocusRequest>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExternalRequest {
    OpenFocusedLink,
    OpenObsidianRootSearch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FocusRequest {
    HighestPriority,
    LowestPriority { recent_days: i64 },
}

pub(super) fn handle_command<E>(
    command: Command,
    execute: impl FnOnce(&Command) -> Result<DisplayModel, E>,
) -> Result<CommandExecution, E> {
    let kind = command.kind();
    let focus_request = match &command {
        Command::Action(super::command::CommandAction::FocusMode {
            kind: CommandKind::FocusHighest,
            ..
        }) => Some(FocusRequest::HighestPriority),
        Command::Action(super::command::CommandAction::FocusMode {
            kind: CommandKind::FocusLowest,
            recent_days,
            ..
        }) => Some(FocusRequest::LowestPriority {
            recent_days: recent_days.unwrap_or(DEFAULT_LOWEST_PRIORITY_RECENT_DAYS),
        }),
        _ => None,
    };
    let (display, external_request) = match kind {
        CommandKind::Open => Some(ExternalRequest::OpenFocusedLink),
        CommandKind::Obsidian => Some(ExternalRequest::OpenObsidianRootSearch),
        _ if focus_request.is_none() => {
            let display = execute(&command)?;
            return Ok(CommandExecution {
                kind,
                display,
                external_request: None,
                focus_request,
            });
        }
        _ => None,
    }
    .map_or_else(
        || (DisplayModel::default(), None),
        |request| (DisplayModel::default(), Some(request)),
    );
    Ok(CommandExecution {
        kind,
        display,
        external_request,
        focus_request,
    })
}

#[allow(unused_must_use)]
pub(super) fn execute_parsed_with_config(
    stdout: &mut dyn SchronuWriter,
    task_repository: &mut dyn TaskRepositoryTrait,
    free_time_manager: &mut dyn FreeTimeManagerTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    focus_started_datetime: &DateTime<Local>,
    command: &command::Command,
    config: &SchronuConfig,
) -> Result<(), CommandError> {
    if let command::Command::InteractiveShortcut(shortcut) = command {
        return execute_interactive_shortcut(task_repository, focused_task_id_opt, *shortcut);
    }
    let focused_task_opt: Option<TaskHandle> = match focused_task_id_opt {
        Some(id) => task_repository
            .get_by_id(*id)
            .map_err(ApplicationError::TaskTree)?,
        None => None,
    };

    match command.kind() {
        command::CommandKind::NewProject | command::CommandKind::HobbyProject => {
            if let command::Command::Action(command::CommandAction::NewProject {
                kind,
                name,
                estimated_minutes,
                ..
            }) = command
            {
                let defer_days_opt = if *kind == command::CommandKind::NewProject {
                    Some(1)
                } else {
                    Some(1400)
                };
                if let Err(error) = execute_start_new_project(
                    stdout,
                    focused_task_id_opt,
                    task_repository,
                    name,
                    defer_days_opt,
                    *estimated_minutes,
                ) {
                    return Err(error.into());
                }
            }
        }
        command::CommandKind::UnplannedProject => {
            if let command::Command::Action(command::CommandAction::NewProject {
                name,
                estimated_minutes,
                ..
            }) = command
            {
                if let Err(error) = execute_start_new_project(
                    stdout,
                    focused_task_id_opt,
                    task_repository,
                    name,
                    None,
                    *estimated_minutes,
                ) {
                    return Err(error.into());
                }
            }
        }
        command::CommandKind::Sequential => {
            if let command::Command::Action(command::CommandAction::Sequential {
                name,
                estimated_minutes,
                begin_index,
                end_index,
                suffix,
            }) = command
            {
                if let (Ok(begin_index), Ok(end_index)) =
                    (u64::try_from(*begin_index), u64::try_from(*end_index))
                {
                    if begin_index > end_index {
                        return Ok(());
                    }
                    let suffix = suffix
                        .as_ref()
                        .map_or_else(String::new, |suffix| format!("-{suffix}"));
                    let result = execute_breakdown_sequentially(
                        stdout,
                        focused_task_id_opt,
                        &focused_task_opt,
                        name,
                        *estimated_minutes,
                        begin_index,
                        end_index,
                        &suffix,
                    );
                    report_application_result(stdout, result);
                }
            }
        }
        command::CommandKind::Repeat => {
            if let command::Command::Action(command::CommandAction::Repeat {
                name,
                estimated_minutes,
                day,
                start_time,
                deadline_time,
            }) = command
            {
                let result = execute_create_repetition_task(
                    stdout,
                    task_repository,
                    focused_task_id_opt,
                    name,
                    day,
                    *estimated_minutes,
                    start_time,
                    deadline_time,
                );
                report_application_result(stdout, result);
            }
        }
        command::CommandKind::Appointment => {
            let command::Command::Action(command::CommandAction::TimeExpression {
                canonical_name,
                values,
                ..
            }) = command
            else {
                unreachable!()
            };
            let time_parts = std::iter::once(*canonical_name)
                .chain(values.iter().map(String::as_str))
                .collect::<Vec<_>>();
            let now = task_repository.get_last_synced_time();
            let start_time_opt = decide_time(&time_parts, &now);

            if let Some(start_time) = start_time_opt {
                execute_make_appointment(&focused_task_opt, start_time)?;
            }
        }
        command::CommandKind::Start => {
            let command::Command::Action(command::CommandAction::TimeExpression {
                canonical_name,
                values,
                ..
            }) = command
            else {
                unreachable!()
            };
            let time_parts = std::iter::once(*canonical_name)
                .chain(values.iter().map(String::as_str))
                .collect::<Vec<_>>();
            let now: DateTime<Local> = task_repository.get_last_synced_time();
            let start_dst_time_opt = decide_time(&time_parts, &now);

            if let Some(start_dst_time) = start_dst_time_opt {
                if let Some(focused_task) = match focused_task_id_opt {
                    Some(id) => task_repository
                        .get_by_id(*id)
                        .map_err(ApplicationError::TaskTree)?,
                    None => None,
                } {
                    focused_task
                        .set_start_time(start_dst_time)
                        .map_err(ApplicationError::TaskTree)?;
                }
            }
        }
        // 最初は「木」コマンドだったが、曜日だけを指定して直近のその曜日について「全」コマンドを動かすコマンドとコンフリクトしてしまったためリネームした。
        command::CommandKind::Tree => {
            execute_show_tree(stdout, &focused_task_opt)?;
        }
        command::CommandKind::Ancestor => {
            execute_show_ancestor(stdout, &focused_task_opt)?;
        }
        command::CommandKind::Root => {
            if let Some(focused_task) = focused_task_opt {
                let root_task = focused_task.root().map_err(ApplicationError::TaskTree)?;
                let root_task_id = root_task.get_id().map_err(ApplicationError::TaskTree)?;
                execute_focus(focused_task_id_opt, &root_task_id.hyphenated().to_string());
            }
        }
        command::CommandKind::Leaves => {
            execute_show_leaf_tasks(stdout, task_repository, free_time_manager)?;
        }
        command::CommandKind::ShowAll => {
            let command::Command::ShowAll { pattern } = command else {
                unreachable!()
            };
            let pattern_opt = pattern.as_deref().map(|pattern| {
                resolve_show_all_pattern(pattern, task_repository.get_last_synced_time())
            });

            renderer::execute_show_all_tasks_with_config(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
                TaskListDisplayOrder::ScheduledStartDesc,
                config,
            )?;
        }
        command::CommandKind::Tail => {
            let command::Command::Action(command::CommandAction::OptionalPattern {
                pattern, ..
            }) = command
            else {
                unreachable!()
            };
            let pattern_opt = Some(pattern.clone().unwrap_or_else(|| "今".to_string()));

            renderer::execute_show_all_tasks_with_config(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
                TaskListDisplayOrder::LowPriorityTail,
                config,
            )?;
        }
        command::CommandKind::Today => {
            let pattern_opt = Some("今".to_string());
            renderer::execute_show_all_tasks_with_config(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
                TaskListDisplayOrder::ScheduledStartDesc,
                config,
            )?;
        }
        command::CommandKind::NonRepetitive => {
            let pattern_opt = Some("単".to_string());
            renderer::execute_show_all_tasks_with_config(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
                TaskListDisplayOrder::ScheduledStartDesc,
                config,
            )?;
        }
        command::CommandKind::Calendar => {
            let pattern_opt = Some("暦".to_string());
            renderer::execute_show_all_tasks_with_config(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
                TaskListDisplayOrder::ScheduledStartDesc,
                config,
            )?;
        }
        command::CommandKind::Band => {
            let pattern_opt = Some("帯".to_string());
            renderer::execute_show_all_tasks_with_config(
                stdout,
                focused_task_id_opt,
                task_repository,
                free_time_manager,
                &pattern_opt,
                TaskListDisplayOrder::ScheduledStartDesc,
                config,
            )?;
        }
        command::CommandKind::Focus => {
            let command::Command::Focus { task_id } = command else {
                unreachable!()
            };
            execute_focus(focused_task_id_opt, &task_id.to_string());
        }
        command::CommandKind::Pick => {
            let command::Command::Action(command::CommandAction::Pick { task_id }) = command else {
                unreachable!()
            };
            execute_pick(task_repository, focused_task_id_opt, &task_id.to_string())?;
        }
        // 外部起動はhandle_commandが構造化要求としてruntimeへ返す。
        command::CommandKind::Open | command::CommandKind::Obsidian => {}
        command::CommandKind::Unfocus => {
            execute_unfocus(focused_task_id_opt);
        }
        command::CommandKind::Parent => {
            if let Some(focused_task) = focused_task_opt {
                if let Some(parent_task) =
                    focused_task.parent().map_err(ApplicationError::TaskTree)?
                {
                    let parent_task_id =
                        parent_task.get_id().map_err(ApplicationError::TaskTree)?;
                    execute_focus(
                        focused_task_id_opt,
                        &parent_task_id.hyphenated().to_string(),
                    );
                }
            }
        }
        command::CommandKind::Children => {
            // 今見ているノードの子タスクが1つだけの時、その子に移動する
            // 2つ以上ある時には、「木」コマンドを実行してツリーの様子を表示する

            if let Some(ref focused_task) = focused_task_opt {
                let tmp_children = focused_task
                    .get_children()
                    .map_err(ApplicationError::TaskTree)?;
                let children = tmp_children
                    .iter()
                    .filter_map(|child| match child.get_status() {
                        Ok(Status::Done) => None,
                        Ok(_) => Some(Ok(child)),
                        Err(error) => Some(Err(error)),
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(ApplicationError::TaskTree)?;

                match children.len() {
                    0 => {
                        // Do nothing
                    }
                    1 => {
                        *focused_task_id_opt =
                            Some(children[0].get_id().map_err(ApplicationError::TaskTree)?);
                    }
                    _ => {
                        execute_show_tree(stdout, &focused_task_opt)?;
                    }
                }
            }
        }
        command::CommandKind::Deepest => {
            // 今見ているノードの子タスクが1つだけである限り、その子に移動して同じことを繰り返す
            // 2つ以上ある時には、「木」コマンドを実行してツリーの様子を表示する

            if let Some(ref focused_task) = focused_task_opt {
                let mut tmp_focused_task_opt: Option<TaskHandle> = Some(focused_task.clone());

                while let Some(ref tmp_focused_task) = tmp_focused_task_opt {
                    let tmp_children = tmp_focused_task
                        .get_children()
                        .map_err(ApplicationError::TaskTree)?;
                    let children = tmp_children
                        .iter()
                        .filter_map(|child| match child.get_status() {
                            Ok(Status::Done) => None,
                            Ok(_) => Some(Ok(child)),
                            Err(error) => Some(Err(error)),
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(ApplicationError::TaskTree)?;

                    if children.len() != 1 {
                        break;
                    }

                    tmp_focused_task_opt = Some(children[0].clone());
                }

                if let Some(ref tmp_focused_task) = tmp_focused_task_opt {
                    *focused_task_id_opt = Some(
                        tmp_focused_task
                            .get_id()
                            .map_err(ApplicationError::TaskTree)?,
                    );

                    if tmp_focused_task
                        .get_children()
                        .map_err(ApplicationError::TaskTree)?
                        .len()
                        > 1
                    {
                        execute_show_tree(stdout, &tmp_focused_task_opt)?;
                    }
                }
            }
        }
        command::CommandKind::NextUp => {
            if let command::Command::Action(command::CommandAction::TaskWithEstimate {
                name,
                estimated_minutes,
                ..
            }) = command
            {
                let result = execute_next_up(
                    stdout,
                    focused_task_id_opt,
                    &focused_task_opt,
                    name,
                    estimated_minutes,
                );
                report_application_result(stdout, result);
            }
        }
        command::CommandKind::Breakdown => {
            if let command::Command::Action(command::CommandAction::TaskNames { names }) = command {
                // 「割」コマンドと間違えて数値を引数に取った場合は何もしない
                if !names.iter().any(|name| name.parse::<i64>().is_ok()) {
                    let new_task_names = names.iter().map(String::as_str).collect::<Vec<_>>();
                    let result = execute_breakdown(
                        stdout,
                        task_repository,
                        focused_task_id_opt,
                        &new_task_names,
                        &None,
                    );
                    report_application_result(stdout, result);
                }
            }
        }
        command::CommandKind::Split => {
            if let command::Command::Action(command::CommandAction::Split { minutes, name }) =
                command
            {
                let result = execute_split(
                    stdout,
                    focused_task_id_opt,
                    &focused_task_opt,
                    name,
                    &minutes.to_string(),
                );
                report_application_result(stdout, result);
            }
        }
        // "詳" | "description" | "desc" => {}
        command::CommandKind::Wait => {
            // フラグを立てるだけか、deferコマンドを自動実行するかは迷う。
            execute_wait_for_others(&focused_task_opt);
        }
        command::CommandKind::Deadline => {
            if let command::Command::Action(command::CommandAction::StringValue { value, .. }) =
                command
            {
                // "2023/05/23"とか。簡単のため、時刻は指定不要とし、自動的に23:59を〆切と設定する
                // 5/23のようにhh/mmで指定した場合は、年の情報を補完してその日の23:59を〆切と設定する
                // 月~日と指定した場合、明日以降で直近のその曜日の23:59を〆切と設定する

                let deadline_date_str = value;

                let now: DateTime<Local> = task_repository.get_last_synced_time();

                let mmdd_reg = Regex::new(r"^(\d{1,2})/(\d{1,2})$").unwrap();

                if value.starts_with('今') {
                    let s = (get_next_morning_datetime(now) - Duration::days(1))
                        .format("%Y/%m/%d")
                        .to_string();
                    execute_set_deadline_with_config(
                        task_repository,
                        *focused_task_id_opt,
                        &s,
                        config,
                    )?;
                } else if value.starts_with('明') {
                    let s = get_next_morning_datetime(now)
                        .format("%Y/%m/%d")
                        .to_string();
                    execute_set_deadline_with_config(
                        task_repository,
                        *focused_task_id_opt,
                        &s,
                        config,
                    )?;
                } else if ["月", "火", "水", "木", "金", "土", "日"].contains(&value.as_str())
                {
                    // 月 火 水 木 金 土 日 が指定された時は、明日以降で、直近のその曜日の23:59を〆切とする
                    // (show_all_tasksとロジック重複...)

                    let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];

                    let todays_morning_datetime =
                        get_next_morning_datetime(now) - Duration::days(1);

                    let dn = todays_morning_datetime.date_naive();
                    let now_weekday_jp = get_weekday_jp(&dn);

                    let now_days_of_week_ind = days_of_week
                        .iter()
                        .position(|&x| x == now_weekday_jp)
                        .unwrap();
                    let target_days_of_week_ind =
                        days_of_week.iter().position(|&x| x == value).unwrap();

                    let ind_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                    // 今日の〆切については「〆 今」で設定できるので、その代わりに、1週間後の同じ曜日の情報を設定するようにする
                    let days: i64 = if ind_diff == 0 { 7 } else { ind_diff as i64 };

                    let s = (get_next_morning_datetime(now) + Duration::days(days - 1))
                        .format("%Y/%m/%d")
                        .to_string();

                    if let Err(error) = execute_set_deadline_with_config(
                        task_repository,
                        *focused_task_id_opt,
                        &s,
                        config,
                    ) {
                        write_command_error(stdout, &error);
                    }
                } else if mmdd_reg.is_match(value) {
                    // FIXME 「後」コマンドとロジック重複

                    let caps = mmdd_reg.captures(value).unwrap();
                    let mm: u32 = caps[1].parse().unwrap();
                    let dd: u32 = caps[2].parse().unwrap();

                    // この時点では12:00にしているが、後で時刻を無視するので問題ない
                    let mut deadline_dst_time = Local
                        .with_ymd_and_hms(now.year(), mm, dd, 12, 0, 0)
                        .unwrap();

                    if deadline_dst_time < now {
                        deadline_dst_time = get_next_morning_datetime(
                            Local
                                .with_ymd_and_hms(now.year() + 1, mm, dd, 12, 0, 0)
                                .unwrap(),
                        ) - Duration::days(1);
                    }

                    let s = deadline_dst_time.format("%Y/%m/%d").to_string();

                    if let Err(error) = execute_set_deadline_with_config(
                        task_repository,
                        *focused_task_id_opt,
                        &s,
                        config,
                    ) {
                        write_command_error(stdout, &error);
                    }
                } else {
                    if let Err(error) = execute_set_deadline_with_config(
                        task_repository,
                        *focused_task_id_opt,
                        deadline_date_str,
                        config,
                    ) {
                        write_command_error(stdout, &error);
                    }
                }
            }
        }
        command::CommandKind::Estimate => {
            let command::Command::Estimate { minutes } = command else {
                unreachable!()
            };
            execute_set_estimated_work_minutes(
                task_repository,
                *focused_task_id_opt,
                &minutes.to_string(),
            )?;
        }
        command::CommandKind::Arrange => {
            let command::Command::Arrange {
                minutes,
                includes_zero_estimate,
            } = command
            else {
                unreachable!()
            };
            execute_set_arrange_children_work_minutes(
                &focused_task_opt,
                &minutes.to_string(),
                *includes_zero_estimate,
            );
        }
        command::CommandKind::Actual => {
            let command::Command::Action(command::CommandAction::IntegerValue { value, .. }) =
                command
            else {
                unreachable!()
            };
            execute_set_actual_work_minutes(&focused_task_opt, &value.to_string());
        }
        command::CommandKind::Priority => {
            let command::Command::Action(command::CommandAction::IntegerValue { value, .. }) =
                command
            else {
                unreachable!()
            };
            execute_set_priority(&focused_task_opt, &value.to_string())?;
        }
        command::CommandKind::Category => {
            let command::Command::Action(command::CommandAction::StringValue { value, .. }) =
                command
            else {
                unreachable!()
            };
            execute_set_project_category(task_repository, *focused_task_id_opt, value)?;
        }
        command::CommandKind::Work => {
            let command::Command::Action(command::CommandAction::OptionalInteger { value, .. }) =
                command
            else {
                unreachable!()
            };
            let additional_actual_work_minutes =
                value.unwrap_or_else(|| (Local::now() - *focus_started_datetime).num_minutes() + 1);

            if let Some(ref focused_task) = focused_task_opt {
                let original_actual_work_minutes = focused_task
                    .get_actual_work_seconds()
                    .map_err(ApplicationError::TaskTree)?
                    / 60;
                let actual_work_minutes_str = format!(
                    "{}",
                    original_actual_work_minutes + additional_actual_work_minutes
                );
                execute_set_actual_work_minutes(&focused_task_opt, &actual_work_minutes_str)?;
                *focused_task_id_opt = None;
            }
        }
        command::CommandKind::Defer => {
            if let command::Command::Defer { amount, unit } = command {
                report_command_result(
                    stdout,
                    execute_defer(
                        task_repository,
                        focused_task_id_opt,
                        &amount.to_string(),
                        &unit.to_lowercase(),
                    ),
                );
            } else if let command::Command::Action(command::CommandAction::TimeExpression {
                values,
                ..
            }) = command
            {
                if values.len() >= 2 {
                    let amount_str = &values[0];
                    let unit_str = &values[1].to_lowercase();

                    report_command_result(
                        stdout,
                        execute_defer(task_repository, focused_task_id_opt, amount_str, unit_str),
                    );
                } else if values.len() == 1 {
                    let yyyymmdd_reg = Regex::new(r"^\d{4}/\d{2}/\d{2}$").unwrap();
                    let hhmm_reg = Regex::new(r"^(\d{1,2}):(\d{1,2})$").unwrap();

                    if yyyymmdd_reg.is_match(&values[0]) {
                        let defer_dst_str = format!("{} 12:00:00", values[0]);
                        let defer_dst_date_result =
                            parse_local_datetime(&defer_dst_str, "%Y/%m/%d %H:%M:%S");

                        match defer_dst_date_result {
                            Ok(LocalResult::Single(defer_dst_date)) => {
                                let defer_dst_time =
                                    get_next_morning_datetime(defer_dst_date) - Duration::days(1);

                                let now: DateTime<Local> = task_repository.get_last_synced_time();
                                let seconds = (defer_dst_time - now).num_seconds() + 1;

                                report_command_result(
                                    stdout,
                                    execute_defer(
                                        task_repository,
                                        focused_task_id_opt,
                                        &format!("{}", seconds),
                                        "秒",
                                    ),
                                );
                            }
                            _ => {
                                // pass
                            }
                        }
                    } else if let Some(LocalResult::Single(defer_dst_time)) =
                        resolve_upcoming_mmdd(&values[0], task_repository.get_last_synced_time())
                    {
                        let now: DateTime<Local> = task_repository.get_last_synced_time();
                        let seconds = (defer_dst_time - now).num_seconds() + 1;

                        if seconds > 0 {
                            report_command_result(
                                stdout,
                                execute_defer(
                                    task_repository,
                                    focused_task_id_opt,
                                    &format!("{}", seconds),
                                    "秒",
                                ),
                            );
                        }
                    } else if hhmm_reg.is_match(&values[0]) {
                        // 時刻が指定された時は今日のその時刻まで送る。25:00のような指定も可能
                        let now: DateTime<Local> = task_repository.get_last_synced_time();

                        let caps = hhmm_reg.captures(&values[0]).unwrap();
                        let hh_i64: i64 = caps[1].parse().unwrap();
                        let mm: u32 = caps[2].parse().unwrap();

                        let hh = (hh_i64 % 24) as u32;

                        let defer_dst_time = now
                            .with_hour(hh % 24)
                            .expect("invalid hour")
                            .with_minute(mm)
                            .expect("invalid minute")
                            + Duration::days(hh_i64 / 24);

                        let seconds = (defer_dst_time - now).num_seconds() + 1;

                        if seconds > 0 {
                            report_command_result(
                                stdout,
                                execute_defer(
                                    task_repository,
                                    focused_task_id_opt,
                                    &format!("{}", seconds),
                                    "秒",
                                ),
                            );
                        }
                    } else if ["月", "火", "水", "木", "金", "土", "日"]
                        .contains(&values[0].as_str())
                    {
                        // 月 火 水 木 金 土 日 が指定された時は、明日以降で、直近のその曜日の06:00にpendingする
                        // (show_all_tasksとロジック重複...)

                        let now: DateTime<Local> = task_repository.get_last_synced_time();
                        let days_of_week = ["月", "火", "水", "木", "金", "土", "日"];

                        let todays_morning_datetime =
                            get_next_morning_datetime(now) - Duration::days(1);

                        let dn = todays_morning_datetime.date_naive();
                        let now_weekday_jp = get_weekday_jp(&dn);

                        let now_days_of_week_ind = days_of_week
                            .iter()
                            .position(|&x| x == now_weekday_jp)
                            .unwrap();
                        let target_days_of_week_ind =
                            days_of_week.iter().position(|&x| x == values[0]).unwrap();

                        let ind_diff = (7 + target_days_of_week_ind - now_days_of_week_ind) % 7;

                        // 今日の6:00にdeferする味意はないので、その代わりに、1週間後の同じ曜日にdeferできるようにする
                        let days: i64 = if ind_diff == 0 { 7 } else { ind_diff as i64 };

                        let seconds = (get_next_morning_datetime(now) + Duration::days(days - 1)
                            - now)
                            .num_seconds()
                            + 1;

                        if seconds > 0 {
                            report_command_result(
                                stdout,
                                execute_defer(
                                    task_repository,
                                    focused_task_id_opt,
                                    &format!("{}", seconds),
                                    "秒",
                                ),
                            );
                        }
                    } else {
                        // "defer 5days" のように引数が1つしか与えられなかった場合は、数字部分とそれ以降に分割する
                        let splitted = split_amount_and_unit(&values[0]);
                        if splitted.len() == 2 && !splitted[0].is_empty() {
                            let amount_str = &splitted[0];
                            let unit_str = &splitted[1].to_lowercase();

                            report_command_result(
                                stdout,
                                execute_defer(
                                    task_repository,
                                    focused_task_id_opt,
                                    amount_str,
                                    unit_str,
                                ),
                            );
                        }
                    }
                }
            }
        }
        command::CommandKind::DeferRoutines => {
            execute_defer_all_frequent_routines(
                task_repository,
                focused_task_id_opt,
                &focused_task_opt,
            )?;
        }
        command::CommandKind::Escape => {
            let command::Command::Action(command::CommandAction::Escape { defer_expression }) =
                command
            else {
                unreachable!()
            };
            // 先延ばしにしてしまう時。要求している見積もりが小さすぎる可能性があるので、2倍にする
            if let Some(focused_task) = focused_task_opt {
                let estimated_work_seconds = focused_task
                    .get_estimated_work_seconds()
                    .map_err(ApplicationError::TaskTree)?;
                focused_task
                    .set_estimated_work_seconds(estimated_work_seconds * 2)
                    .map_err(ApplicationError::TaskTree)?;

                // 引数が与えられた時はそのままdeferする
                if let Some(values) = defer_expression {
                    let defer_command =
                        command::Command::Action(command::CommandAction::TimeExpression {
                            kind: command::CommandKind::Defer,
                            canonical_name: "後",
                            values: values.clone(),
                        });
                    execute_parsed_with_config(
                        stdout,
                        task_repository,
                        free_time_manager,
                        focused_task_id_opt,
                        focus_started_datetime,
                        &defer_command,
                        config,
                    )?;
                }
            }
        }
        command::CommandKind::Flatten => {
            let result = flatten_tasks_with_end_of_day_offset_minutes(
                task_repository,
                free_time_manager,
                config.end_of_day_offset_minutes,
            )?;
            write_flatten_result(stdout, &result);
        }
        command::CommandKind::Pack => {
            execute_pack_with_config(stdout, task_repository, free_time_manager, config)?;
        }
        command::CommandKind::Extrude => {
            let command::Command::Action(command::CommandAction::Extrude { step_days }) = command
            else {
                unreachable!()
            };
            if let Some(step_days) = step_days {
                if let Some(ref focused_task) = focused_task_opt {
                    let ancestors = focused_task
                        .list_all_parent_tasks_with_first_available_time()
                        .map_err(ApplicationError::TaskTree)?;
                    let Some((first_datetime, _)) = ancestors.first() else {
                        return Ok(());
                    };
                    execute_extrude_with_config(
                        focused_task_id_opt,
                        &focused_task_opt,
                        first_datetime,
                        *step_days,
                        config,
                    )?;
                }
            }
        }
        command::CommandKind::Clear | command::CommandKind::Gather => {
            let command::Command::Action(command::CommandAction::ClearOrGather {
                kind,
                canonical_name,
                values,
            }) = command
            else {
                unreachable!()
            };
            match values.as_slice() {
                [defer_to] => {
                    let defer_to_datetime_opt = parse_clear_or_gather_defer_to_datetime(
                        canonical_name,
                        defer_to,
                        task_repository.get_last_synced_time(),
                    );

                    if let Some(defer_to_datetime) = defer_to_datetime_opt {
                        for project_root_task in task_repository.get_all_projects().iter() {
                            let leaf_tasks =
                                extract_leaf_tasks_from_project_with_pending(project_root_task)
                                    .map_err(ApplicationError::TaskTree)?;
                            for leaf_task in leaf_tasks.iter() {
                                let start_time = leaf_task
                                    .get_start_time()
                                    .map_err(ApplicationError::TaskTree)?;
                                let orig_status = leaf_task
                                    .get_orig_status()
                                    .map_err(ApplicationError::TaskTree)?;
                                let pending_until = leaf_task
                                    .get_pending_until()
                                    .map_err(ApplicationError::TaskTree)?;
                                match kind {
                                    command::CommandKind::Clear => {
                                        if start_time < defer_to_datetime
                                            && (orig_status == Status::Todo
                                                || (orig_status == Status::Pending
                                                    && pending_until < defer_to_datetime))
                                        {
                                            leaf_task
                                                .set_orig_status(Status::Pending)
                                                .map_err(ApplicationError::TaskTree)?;
                                            leaf_task
                                                .set_pending_until(defer_to_datetime)
                                                .map_err(ApplicationError::TaskTree)?;
                                        }
                                    }
                                    command::CommandKind::Gather
                                        if leaf_task
                                            .get_status()
                                            .map_err(ApplicationError::TaskTree)?
                                            == Status::Pending
                                            && start_time < defer_to_datetime
                                            && pending_until < defer_to_datetime =>
                                    {
                                        leaf_task
                                            .set_orig_status(Status::Todo)
                                            .map_err(ApplicationError::TaskTree)?;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                [time, mmdd] => {
                    let Some((schronu_day_start, end)) = parse_dated_clear_or_gather_time_range(
                        time,
                        mmdd,
                        task_repository.get_last_synced_time(),
                    ) else {
                        return Ok(());
                    };
                    let scheduled_starts =
                        scheduled_leaf_starts_on_schronu_day(task_repository, schronu_day_start)?;

                    for project_root_task in task_repository.get_all_projects().iter() {
                        let leaf_tasks =
                            extract_leaf_tasks_from_project_with_pending(project_root_task)
                                .map_err(ApplicationError::TaskTree)?;
                        for leaf_task in leaf_tasks.iter() {
                            let scheduled_starts_opt = scheduled_starts
                                .get(&leaf_task.get_id().map_err(ApplicationError::TaskTree)?);
                            let orig_status = leaf_task
                                .get_orig_status()
                                .map_err(ApplicationError::TaskTree)?;
                            let pending_until = leaf_task
                                .get_pending_until()
                                .map_err(ApplicationError::TaskTree)?;
                            match kind {
                                command::CommandKind::Clear => {
                                    let todo_is_scheduled_in_range = orig_status == Status::Todo
                                        && scheduled_starts_opt.is_some_and(|starts| {
                                            starts.iter().any(|scheduled_start| {
                                                schronu_day_start <= *scheduled_start
                                                    && *scheduled_start < end
                                            })
                                        });
                                    let pending_is_in_range = orig_status == Status::Pending
                                        && scheduled_starts_opt.is_some()
                                        && schronu_day_start <= pending_until
                                        && pending_until < end;
                                    if todo_is_scheduled_in_range || pending_is_in_range {
                                        leaf_task
                                            .set_orig_status(Status::Pending)
                                            .map_err(ApplicationError::TaskTree)?;
                                        leaf_task
                                            .set_pending_until(end)
                                            .map_err(ApplicationError::TaskTree)?;
                                    }
                                }
                                command::CommandKind::Gather
                                    if orig_status == Status::Pending
                                        && scheduled_starts_opt.is_some()
                                        && pending_until <= end =>
                                {
                                    leaf_task
                                        .set_pending_until(schronu_day_start)
                                        .map_err(ApplicationError::TaskTree)?;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        command::CommandKind::Finish => {
            let command::Command::Action(command::CommandAction::Finish { values }) = command
            else {
                unreachable!()
            };
            let finish_parts = std::iter::once("終")
                .chain(values.iter().map(String::as_str))
                .collect::<Vec<_>>();
            if let Some(ref focused_task) = focused_task_opt {
                if focused_task
                    .has_undone_children()
                    .map_err(ApplicationError::TaskTree)?
                {
                    execute_show_tree(stdout, &focused_task_opt)?;
                } else {
                    let now = task_repository.get_last_synced_time();
                    if let Some(finished_at) = decide_finish_time(&finish_parts, &now) {
                        let additional_actual_work_seconds = if values.is_empty() {
                            let focus_duration_seconds =
                                (now - *focus_started_datetime).num_seconds();
                            if focus_duration_seconds >= 60 {
                                focus_duration_seconds
                            } else {
                                0
                            }
                        } else {
                            0
                        };

                        match complete_task(
                            task_repository,
                            CompleteTaskInput {
                                task_id: focused_task
                                    .get_id()
                                    .map_err(ApplicationError::TaskTree)?,
                                finished_at,
                                additional_actual_work_seconds,
                            },
                        ) {
                            Ok(output) => {
                                *focused_task_id_opt = output.next_focus_task_id;
                            }
                            Err(ApplicationError::HasUndoneChildren(_)) => {
                                execute_show_tree(stdout, &focused_task_opt)?;
                            }
                            Err(_) => {}
                        }
                    }
                }
            }
        }
        command::CommandKind::Noop => {}
        command::CommandKind::FocusHighest | command::CommandKind::FocusLowest => {}
    }

    stdout.flush().map_err(CommandError::Output)?;
    Ok(())
}

fn execute_interactive_shortcut(
    task_repository: &mut dyn TaskRepositoryTrait,
    focused_task_id_opt: &mut Option<Uuid>,
    shortcut: command::InteractiveShortcut,
) -> Result<(), CommandError> {
    let now = task_repository.get_last_synced_time();
    match shortcut {
        command::InteractiveShortcut::NextMorning => {
            let seconds = (get_next_morning_datetime(now) - now).num_seconds() + 1;
            execute_defer(
                task_repository,
                focused_task_id_opt,
                &seconds.to_string(),
                "秒",
            )?;
        }
        command::InteractiveShortcut::NextWeek => {
            let seconds = (get_next_morning_datetime(now) - now).num_seconds() + 86_400 * 6 + 1;
            execute_defer(
                task_repository,
                focused_task_id_opt,
                &seconds.to_string(),
                "秒",
            )?;
        }
        command::InteractiveShortcut::DeferRoutine => {
            execute_defer_routine(task_repository, focused_task_id_opt);
        }
        command::InteractiveShortcut::FiveYears => {
            let seconds = (get_next_morning_datetime(now) - now).num_seconds()
                + 86_400 * (7 * 52 * 5 - 1)
                + 1;
            execute_defer(
                task_repository,
                focused_task_id_opt,
                &seconds.to_string(),
                "秒",
            )?;
        }
    }
    Ok(())
}
