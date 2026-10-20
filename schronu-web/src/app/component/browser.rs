use super::super::carry_lock_view::CarryLockBar;
use super::super::component_dispatch::{
    dispatch_action, dispatch_action_effect, dispatch_session_action,
};
use super::super::component_models::{
    browser_monotonic_now_ms, browser_now_epoch_ms, BrowserPageModel,
};
use super::super::component_runtime::{ComponentAction, ComponentOrchestrator};
use super::super::create_task_view::CreateTaskView;
use super::super::list_view::{AllTasksViewStatus, CompletedListView, ListModeControl, ListView};
use super::super::load_view::LoadView;
use super::super::long_press_browser::BrowserLongPressScheduler;
use super::super::long_press_controller::LongPressSchedulerHandle;
use super::super::session_links::SessionLinks;
use super::super::session_view::SessionView;
use super::{
    BackgroundRefreshStatus, BufferPanel, InteractiveShell, LoadingOverlay, NavigationTabs,
    RestoringShell, SessionChrome, TIME_DISPLAY_MODE,
};
use crate::client::state::{ActiveTab, AllTasksStatus, ClientState, ListMode, ListSelection};
use crate::client::work_sessions::BrowserLocalStorage;
use dioxus::prelude::*;
use wasm_bindgen::JsValue;

const TICK_MILLIS: u32 = 1_000;

#[component]
pub(super) fn BrowserApp() -> Element {
    let mut client = use_signal(ComponentOrchestrator::new);
    let open_link_error = use_signal(|| None::<String>);
    let mut session_links = use_signal(SessionLinks::default);
    let mut session_obsidian = use_signal(SessionLinks::obsidian);
    let obsidian_supported = use_signal(super::super::obsidian::browser_supports_obsidian);
    let timer_shortcut_supported =
        use_signal(super::super::timer_shortcut::browser_supports_timer_shortcut);
    let long_press_scheduler =
        use_hook(|| LongPressSchedulerHandle::new(BrowserLongPressScheduler));

    use_effect(move || {
        let effect = client
            .write()
            .mount(&BrowserLocalStorage, browser_now_epoch_ms());
        super::super::component_dispatch::dispatch_action_effect(client, effect);
    });

    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(TICK_MILLIS).await;
            dispatch_action(
                client,
                ComponentAction::Tick {
                    wall_now_epoch_ms: browser_now_epoch_ms(),
                },
            );
        }
    });

    use_effect(move || {
        let (active, task_ids) = {
            let client = client.read();
            let active = client
                .state()
                .is_some_and(|state| state.active_tab() == ActiveTab::Session);
            let task_ids = client
                .state()
                .map(|state| {
                    state
                        .sessions()
                        .iter()
                        .map(|session| session.task_id.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (active, task_ids)
        };
        let requests = session_links.write().reconcile(active, &task_ids);
        for (task_id, request_id) in requests {
            spawn(async move {
                let result = match super::super::web_endpoint::resolve_task_link(task_id.clone())
                    .await
                {
                    Ok(Ok(url)) => Ok(url),
                    Ok(Err(error)) => Err(error.message),
                    Err(_) => Err("リンクを取得できませんでした。再試行してください。".to_owned()),
                };
                session_links
                    .write()
                    .apply_result(&task_id, request_id, result);
            });
        }
    });

    use_effect(move || {
        let requests = super::super::obsidian::refresh_requests(
            &mut session_obsidian.write(),
            client.read().state(),
            obsidian_supported(),
        );
        for (task_id, request_id) in requests {
            spawn(async move {
                let result =
                    match super::super::web_endpoint::resolve_obsidian_search_uri(task_id.clone())
                        .await
                    {
                        Ok(Ok(uri)) => Ok(Some(uri)),
                        Ok(Err(error)) => Err(error.message),
                        Err(_) => Err("サーバーへ接続できませんでした。".to_owned()),
                    };
                session_obsidian
                    .write()
                    .apply_result(&task_id, request_id, result);
            });
        }
    });

    let model = {
        let client = client.read();
        client.state().map(|state| {
            BrowserPageModel::from_state_at(
                state,
                browser_monotonic_now_ms(),
                client.task_name_filter(),
                client.all_tasks_visible_limit(),
            )
        })
    };
    let Some(model) = model else {
        return rsx! { RestoringShell {} };
    };
    let BrowserPageModel {
        active_tab,
        buffer,
        sessions,
        rows,
        completed_report,
        list_mode,
        selected_logical_date,
        band_rows,
        routine_load_report,
        band_observed_at_epoch_ms,
        band_loading,
        band_error,
        has_more_rows,
        matching_task_count,
        list_selection,
        all_tasks_status,
        all_tasks_failure,
        active_task_ids,
        dates,
        warnings,
        safety_warning,
        display_error,
        global_blocked,
        can_confirm,
        auto_session_in_flight,
        auto_session_empty,
        carry_lock,
    } = model;
    let mutations_locked = carry_lock.mutations_locked();
    let linkable_task_ids = session_links.read().linkable_task_ids();
    let link_lookup_errors = session_links.read().errors();
    let (
        server_effect_in_flight,
        background_refreshing,
        refresh_failed,
        server_actions_blocked,
        has_cached_list,
        date_input_text,
        date_input_error,
        filter_text,
        all_tasks_visible_limit,
        create_name,
        create_minutes,
        create_error,
        create_success,
    ) = {
        let client = client.read();
        (
            client.server_effect_in_flight(),
            client.background_refreshing(),
            client.refresh_failed(),
            client.server_actions_blocked(),
            client.state().is_some_and(ClientState::has_active_list),
            client.date_input().text().to_owned(),
            client.date_input().error().map(|error| error.to_string()),
            client.task_name_filter().to_owned(),
            client.all_tasks_visible_limit(),
            client.create_name().to_owned(),
            client.create_minutes().to_owned(),
            client.create_error().map(str::to_owned),
            client.create_success().map(str::to_owned),
        )
    };
    let all_tasks_view_status = (list_mode == ListMode::Scheduled
        && list_selection == ListSelection::All)
        .then(|| match all_tasks_status {
            AllTasksStatus::NotLoaded | AllTasksStatus::Loading => AllTasksViewStatus::Loading,
            AllTasksStatus::Loaded => AllTasksViewStatus::Loaded,
            AllTasksStatus::Failed => AllTasksViewStatus::Failed(
                all_tasks_failure
                    .unwrap_or_else(|| "全てのタスクを取得できませんでした。".to_owned()),
            ),
            AllTasksStatus::Invalidated => AllTasksViewStatus::Invalidated,
        });

    rsx! {
        InteractiveShell {
            blocked: server_effect_in_flight,
            SessionChrome { active_tab,
                CarryLockBar {
                    model: carry_lock,
                    scheduler: long_press_scheduler,
                    on_enable: move |_| dispatch_action(client, ComponentAction::EnableCarryLock),
                    on_arm: move |_| dispatch_action(client, ComponentAction::ArmCarryLock),
                    on_relock: move |_| dispatch_action(client, ComponentAction::RelockCarryLock),
                    on_disable: move |_| dispatch_action(client, ComponentAction::DisableCarryLock),
                }
                BufferPanel { value: buffer, mode: TIME_DISPLAY_MODE }
            }
            NavigationTabs {
                active_tab,
                on_switch: move |tab| dispatch_action(client, ComponentAction::SwitchTab(tab)),
            }
            if background_refreshing || refresh_failed {
                BackgroundRefreshStatus {
                    has_cached_list,
                    failed: refresh_failed,
                    on_retry: move |_| dispatch_action(client, ComponentAction::RetryRefresh),
                }
            }
            for warning in warnings {
                section { class: "error", role: "alert", p { "{warning}" } }
            }
            if let Some(warning) = safety_warning {
                section { class: "error", role: "alert",
                    p { "{warning}" }
                    button {
                        r#type: "button",
                        disabled: !can_confirm || mutations_locked,
                        onclick: move |_| dispatch_action(client, ComponentAction::ConfirmRepositoryChecked),
                        "repository確認済み"
                    }
                }
            }
            if let Some(error) = display_error {
                section { class: "error", role: "alert", p { "{error}" } }
            }
            if let Some(error) = open_link_error.read().as_deref() {
                section { class: "error", role: "alert", p { "{error}" } }
            }
            if active_tab == ActiveTab::Session {
                for error in link_lookup_errors {
                    section { class: "error", role: "alert", p { "リンクを確認できませんでした: {error}" } }
                }
            }
            if active_tab == ActiveTab::Session {
                if auto_session_empty && sessions.is_empty() {
                    p { role: "status", "自動選定できるタスクがありません。" }
                }
                SessionView {
                    sessions,
                    linkable_task_ids,
                    global_blocked,
                    mutations_locked,
                    auto_session_in_flight,
                    server_actions_blocked,
                    obsidian_supported: obsidian_supported(),
                    obsidian_ready_task_ids: session_obsidian.read().linkable_task_ids(),
                    obsidian_lookup_errors: session_obsidian.read().error_messages(),
                    obsidian_server_busy: client.read().server_effect_in_flight(),
                    on_open_obsidian: move |task_id: String| {
                        let client = client.read();
                        super::super::obsidian::open_obsidian(
                            client.state()?,
                            &session_obsidian.read(),
                            super::super::obsidian::browser_supports_obsidian(),
                            client.server_effect_in_flight(),
                            &task_id,
                            |uri| {
                                let browser = web_sys::window().ok_or_else(|| "browserを利用できません。".to_owned())?;
                                browser.location().assign(uri).map_err(|_| "Obsidianを開けませんでした。ブラウザの設定を確認して再試行してください。".to_owned())
                            },
                        )
                    },
                    timer_shortcut_supported: timer_shortcut_supported(),
                    timer_server_busy: client.read().server_effect_in_flight(),
                    on_auto_session: move |max_minutes: Option<i64>| {
                        let action = max_minutes.map_or(
                            ComponentAction::AutoSession,
                            ComponentAction::TimedAutoSession,
                        );
                        dispatch_action(client, action);
                    },
                    on_action: move |action| dispatch_session_action(client, action),
                    on_open_link: move |task_id| {
                        open_session_link(client, open_link_error, session_links, task_id);
                    },
                    on_open_timer: move |task_id: String| {
                        client.write().open_timer_shortcut(
                            &task_id,
                            super::super::timer_shortcut::browser_supports_timer_shortcut(),
                            browser_now_epoch_ms(),
                            browser_monotonic_now_ms(),
                            |url| {
                                let browser = web_sys::window().ok_or_else(|| "browserを利用できません。".to_owned())?;
                                browser.location().assign(url).map_err(|_| "タイマーを開けませんでした。ブラウザの設定を確認して再試行してください。".to_owned())
                            },
                        )
                    },
                }
            } else if active_tab == ActiveTab::List {
                section { class: "list-screen",
                    ListModeControl {
                        selected_mode: list_mode,
                        on_switch_list_mode: move |mode| {
                            dispatch_action(client, ComponentAction::SwitchListMode(mode));
                        },
                    }
                    if list_mode == ListMode::Completed {
                        CompletedListView {
                            dates,
                            report: completed_report,
                            selected_logical_date,
                            date_input_text,
                            date_input_error,
                            filter_text,
                            server_actions_blocked,
                            on_select_date: move |date| {
                                client.write().clear_date_input(&BrowserLocalStorage);
                                dispatch_action(client, ComponentAction::SelectDate(date));
                            },
                            on_date_input_change: move |text| {
                                client.write().edit_date_input(&BrowserLocalStorage, text);
                            },
                            on_submit_date_input: move |_| {
                                let action = client.write().submit_date_input(&BrowserLocalStorage);
                                if let Some(action) = action {
                                    dispatch_action(client, action);
                                }
                            },
                            on_filter_change: move |filter| {
                                client.write().edit_task_name_filter(&BrowserLocalStorage, filter);
                            },
                        }
                    } else {
                    ListView {
                        dates,
                        rows,
                        active_task_ids,
                        date_input_text,
                        date_input_error,
                        filter_text,
                        all_tasks_status: all_tasks_view_status,
                        visible_row_limit: (list_selection == ListSelection::All)
                            .then_some(all_tasks_visible_limit),
                        has_more_rows: (list_selection == ListSelection::All).then_some(has_more_rows),
                        matching_task_count,
                        mutations_locked,
                        mutation_globally_blocked: global_blocked,
                        server_actions_blocked,
                        on_select_date: move |date| {
                            client.write().clear_date_input(&BrowserLocalStorage);
                            dispatch_action(client, ComponentAction::SelectDate(date));
                        },
                        on_select_all_tasks: move |_| {
                        dispatch_action(client, ComponentAction::SelectAllTasks);
                        },
                        on_retry_all_tasks: move |_| {
                        dispatch_action(client, ComponentAction::RetryAllTasks);
                        },
                        on_show_more: move |_| {
                        client.write().show_more_all_tasks();
                        },
                        on_date_input_change: move |text| {
                        client.write().edit_date_input(&BrowserLocalStorage, text);
                        },
                        on_submit_date_input: move |_| {
                        let action = client.write().submit_date_input(&BrowserLocalStorage);
                        if let Some(action) = action {
                            dispatch_action(client, action);
                        }
                        },
                        on_start_session: move |(task, is_leaf)| {
                        let effect = client.write().start_session_from_list(
                            &BrowserLocalStorage,
                            browser_monotonic_now_ms(),
                            task,
                            is_leaf,
                        );
                        dispatch_action_effect(client, effect);
                        },
                        on_defer_task: move |(task_id, expected_plan)| {
                        dispatch_action(client, ComponentAction::DeferTask {
                            task_id,
                            expected_plan,
                        });
                        },
                        on_filter_change: move |filter| {
                        client.write().edit_task_name_filter(&BrowserLocalStorage, filter);
                        },
                    }
                    }
                }
            } else if active_tab == ActiveTab::Create {
                CreateTaskView {
                    name: create_name,
                    minutes: create_minutes,
                    error: create_error,
                    success: create_success,
                    blocked: mutations_locked || global_blocked || server_actions_blocked,
                    on_name_change: move |value| client.write().edit_create_name(value),
                    on_minutes_change: move |value| client.write().edit_create_minutes(value),
                    on_submit: move |_| {
                        let effect = client.write().submit_create_task(
                            &BrowserLocalStorage,
                            browser_monotonic_now_ms(),
                        );
                        dispatch_action_effect(client, effect);
                    },
                }
            } else if active_tab == ActiveTab::Load {
                LoadView {
                    rows: band_rows,
                    routine_load_report,
                    observed_at_epoch_ms: band_observed_at_epoch_ms,
                    loading: band_loading,
                    error: band_error,
                    server_actions_blocked,
                    on_refresh: move |_| dispatch_action(client, ComponentAction::RefreshLoad),
                    on_select_date: move |date| {
                        client.write().clear_date_input(&BrowserLocalStorage);
                        dispatch_action(client, ComponentAction::SelectLoadDate(date));
                    },
                }
            }
        }
        if server_effect_in_flight {
            LoadingOverlay {}
        }
    }
}

fn open_session_link(
    client: Signal<ComponentOrchestrator>,
    mut open_link_error: Signal<Option<String>>,
    session_links: Signal<SessionLinks>,
    task_id: String,
) {
    if client.read().server_actions_blocked() || client.read().server_effect_in_flight() {
        return;
    }
    let Some(url) = session_links.read().url_for(&task_id).map(str::to_owned) else {
        return;
    };
    open_link_error.set(None);
    let Some(browser) = web_sys::window() else {
        open_link_error.set(Some("browserを利用できません。".to_owned()));
        return;
    };
    let new_tab = match browser.open_with_url_and_target(&url, "_blank") {
        Ok(Some(tab)) => tab,
        _ => {
            open_link_error.set(Some(
                "新しいタブを開けませんでした。ポップアップの設定を確認してください。".to_owned(),
            ));
            return;
        }
    };
    if js_sys::Reflect::set(
        new_tab.as_ref(),
        &JsValue::from_str("opener"),
        &JsValue::NULL,
    )
    .is_err()
    {
        let _ = new_tab.close();
        open_link_error.set(Some("新しいタブを安全に開けませんでした。".to_owned()));
        return;
    }
}
