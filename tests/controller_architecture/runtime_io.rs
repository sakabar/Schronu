use super::paths::{
    definitions, expand_local_globs, expand_type_aliases, method_names, references, resolve_path,
    return_paths, signature_paths, signatures, type_alias_dependencies,
};
use super::source::{controller_modules, module_family, product_file};
use std::collections::BTreeMap;
use syn::ext::IdentExt;

pub fn external_io_dependency(path: &str) -> bool {
    [
        "controller::runtime",
        "crate::application::repository_transaction",
        "crate::adapter::gateway",
        "std::process",
        "std::fs",
        "std::env",
        "webbrowser",
    ]
    .iter()
    .any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}::")))
}

fn io_violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    let mut errors = Vec::new();
    for root in [
        "controller::handler",
        "controller::interactive",
        "controller::renderer",
    ] {
        for (module, file) in module_family(modules, root) {
            match references(module, file) {
                Ok(paths) => {
                    for path in paths {
                        if external_io_dependency(&path) {
                            errors.push(format!("{module} owns external I/O dependency: {path}"));
                        }
                    }
                }
                Err(error) => errors.push(error),
            }
        }
    }
    errors
}

#[test]
fn renderer_cannot_start_an_external_process() {
    let mut modules = controller_modules();
    modules
        .get_mut("controller::renderer")
        .unwrap()
        .items
        .push(syn::parse_quote! {
            fn renamed() { std::process::Command::new("external"); }
        });
    assert!(!io_violations(&modules).is_empty());
}

#[test]
fn product_external_io_stays_outside_handler_driver_and_renderer() {
    assert_eq!(io_violations(&controller_modules()), Vec::<String>::new());
}

#[test]
fn aliases_nested_methods_and_macros_cannot_hide_external_io() {
    for source in [
        "use crate::application::repository_transaction as tx; fn renamed() { tx::run_repository_transaction(); }",
        "impl Helper { fn renamed() { webbrowser::open(\"url\"); } }",
        "fn renamed() { format!(\"{:?}\", super::super::runtime::coordinate()); }",
    ] {
        let mut modules = controller_modules();
        modules.insert("controller::interactive::helper".into(), product_file(source).unwrap());
        assert!(!io_violations(&modules).is_empty(), "{source}");
    }
}

#[test]
fn runtime_may_coordinate_external_io_under_arbitrary_function_names() {
    let mut modules = controller_modules();
    modules
        .get_mut("controller::runtime")
        .unwrap()
        .items
        .push(syn::parse_quote! {
            fn arbitrary_coordinator() { std::process::Command::new("external"); }
        });
    assert!(io_violations(&modules).is_empty());
}

#[test]
fn relative_and_qualified_gateway_paths_have_the_same_boundary() {
    let errors: Vec<_> = ["crate::adapter::gateway", "super::super::gateway"].into_iter().map(|path| {
        let mut modules = controller_modules();
        modules.get_mut("controller::renderer").unwrap().items.extend(product_file(&format!("use {path} as storage; fn renamed() {{ storage::task_repository::TaskRepository::new(); }}")).unwrap().items);
        io_violations(&modules)
    }).collect();
    assert!(!errors[0].is_empty());
    assert_eq!(errors[0], errors[1]);
}

fn ownership_violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    let mut errors = Vec::new();
    for (module, file) in module_family(modules, "controller::runtime") {
        match expand_local_globs(module, file, modules) {
            Ok(file) => match definitions(module, &file) {
                Ok(items) => {
                    for item in items {
                        match item {
                            syn::Item::Struct(item)
                                if item.ident.unraw().to_string().ends_with("Context") =>
                            {
                                errors.push(format!("runtime owns context: {}", item.ident));
                            }
                            syn::Item::Impl(item) => {
                                if let Some((_, path, _)) = &item.trait_ {
                                    match resolve_path(module, &file, path) {
                                        Ok(path) if path.ends_with("CommandContext") => errors
                                            .push(format!(
                                                "runtime implements command context: {path}"
                                            )),
                                        Err(error) => errors.push(error),
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Err(error) => errors.push(error),
            },
            Err(error) => errors.push(error),
        }
    }
    errors
}

#[test]
fn runtime_cannot_define_command_contexts_under_new_names() {
    let mut modules = controller_modules();
    modules
        .get_mut("controller::runtime")
        .unwrap()
        .items
        .push(syn::parse_quote! {
            struct RenamedContext;
        });
    assert!(!ownership_violations(&modules).is_empty());
}

#[test]
fn product_runtime_does_not_implement_command_contexts() {
    assert_eq!(
        ownership_violations(&controller_modules()),
        Vec::<String>::new()
    );
}

#[test]
fn nested_and_aliased_context_implementations_remain_outside_runtime() {
    for source in [
        "struct r#RenamedContext;",
        "use super::super::handler::ProjectCommandContext as Capability; impl Capability for Adapter {}",
        "fn outer() { struct LocalContext; }",
        "fn helper() { format!(\"{}\", { struct LocalContext; 0 }); }",
        "fn helper() { format!(\"{}\", { impl CommandContext for Adapter {} 0 }); }",
    ] {
        let mut modules = controller_modules();
        modules.insert("controller::runtime::nested".into(), product_file(source).unwrap());
        assert!(!ownership_violations(&modules).is_empty(), "{source}");
    }
}

#[test]
fn runtime_context_text_is_not_a_definition() {
    let mut modules = controller_modules();
    modules.get_mut("controller::runtime").unwrap().items.push(syn::parse_quote! {
        fn renamed() { let _example = "struct ExampleContext; impl CommandContext for Adapter {}"; }
    });
    assert!(ownership_violations(&modules).is_empty());
}

fn datetime_violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    let aliases = match type_alias_dependencies(modules) {
        Ok(aliases) => aliases,
        Err(error) => return vec![error],
    };
    let mut errors = Vec::new();
    for (module, file) in module_family(modules, "controller::runtime") {
        let inspect = || -> Result<Vec<String>, String> {
            let file = expand_local_globs(module, file, modules)?;
            let mut errors = Vec::new();
            for path in references(module, &file)? {
                if [
                    "chrono::NaiveDate",
                    "chrono::NaiveTime",
                    "chrono::NaiveDateTime",
                ]
                .iter()
                .any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}::")))
                {
                    errors.push(format!("runtime interprets calendar values: {path}"));
                }
            }
            for signature in signatures(module, &file)? {
                let output =
                    expand_type_aliases(module, return_paths(module, &file, &signature)?, &aliases);
                let parameters = expand_type_aliases(
                    module,
                    signature_paths(module, &file, &signature)?,
                    &aliases,
                );
                if output.contains("chrono::DateTime")
                    || (parameters.contains("chrono::DateTime") && output.contains("i64"))
                {
                    errors.push(format!(
                        "runtime computes datetime result: {}",
                        signature.ident
                    ));
                }
            }
            Ok(errors)
        };
        match inspect() {
            Ok(found) => errors.extend(found),
            Err(error) => errors.push(error),
        }
    }
    errors
}

#[test]
fn renamed_runtime_helper_cannot_interpret_calendar_dates() {
    let mut modules = controller_modules();
    modules
        .get_mut("controller::runtime")
        .unwrap()
        .items
        .push(syn::parse_quote! {
            fn renamed() { chrono::NaiveDate::from_ymd_opt(2026, 9, 19); }
        });
    assert!(!datetime_violations(&modules).is_empty());
}

#[test]
fn product_runtime_coordinates_existing_dates_without_interpreting_them() {
    assert_eq!(
        datetime_violations(&controller_modules()),
        Vec::<String>::new()
    );
}

#[test]
fn calendar_roles_survive_aliases_nested_methods_and_macro_blocks() {
    for source in [
        "use chrono::NaiveDate as Day; fn renamed() { Day::from_ymd_opt(2026, 9, 19); }",
        "use chrono::DateTime as Instant; impl Helper { fn renamed() -> Instant<Local> { todo!() } }",
        "fn renamed() { format!(\"{}\", { fn inner(now: chrono::DateTime<Local>) -> i64 { 0 } 0 }); }",
    ] {
        let mut modules = controller_modules();
        modules.insert("controller::runtime::helper".into(), product_file(source).unwrap());
        assert!(!datetime_violations(&modules).is_empty(), "{source}");
    }
}

#[test]
fn runtime_may_pass_operation_dates_to_io_and_return_unrelated_counts() {
    let mut modules = controller_modules();
    modules.get_mut("controller::runtime").unwrap().items.extend(product_file("fn renamed(now: chrono::DateTime<Local>) { repository.reload(now); } fn io_count() -> i64 { 0 }").unwrap().items);
    assert!(datetime_violations(&modules).is_empty());
}

fn mutation_violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    // These are domain capabilities, not names of controller implementations.
    // sync_clock also belongs to TaskRepositoryTrait: runtime coordinates that
    // repository clock hook before dispatch, so the shared method name is allowed.
    let capabilities = [
        "make_appointment",
        "set_orig_status",
        "set_pending_until",
        "set_priority",
        "set_deadline_time_opt",
        "set_estimated_work_seconds",
        "set_actual_work_seconds",
        "create_child",
        "create_parent",
        "create_sequential_children",
        "create_task_attr",
        "set_repetition_interval_days_opt",
        "set_repetition_anchor",
        "set_days_in_advance",
        "set_project_category_opt",
        "set_start_time",
        "set_end_time_opt",
        "set_atomic",
        "set_fixed_start",
        "set_is_on_other_side",
        "set_flexible_start_time",
        "set_id",
        "set_create_time",
        "unset_deadline_time_opt",
        "reparent_to",
    ];
    let mut errors = Vec::new();
    for (module, file) in module_family(modules, "controller::runtime") {
        let inspect = || -> Result<Vec<String>, String> {
            let file = expand_local_globs(module, file, modules)?;
            let mut errors = Vec::new();
            let mut operations = method_names(module, &file)?;
            operations.extend(
                references(module, &file)?
                    .into_iter()
                    .map(|path| path.rsplit("::").next().unwrap_or(&path).to_string()),
            );
            for operation in operations {
                if capabilities.contains(&operation.as_str()) {
                    errors.push(format!(
                        "runtime invokes task mutation capability: {operation}"
                    ));
                }
            }
            for signature in signatures(module, &file)? {
                for path in signature_paths(module, &file, &signature)? {
                    if path.ends_with("CommandContext") {
                        errors.push(format!(
                            "runtime owns command-context operation: {}",
                            signature.ident
                        ));
                    }
                }
            }
            Ok(errors)
        };
        match inspect() {
            Ok(found) => errors.extend(found),
            Err(error) => errors.push(error),
        }
    }
    errors
}

#[test]
fn renamed_runtime_helper_cannot_mutate_task_domain_state() {
    let mut modules = controller_modules();
    modules
        .get_mut("controller::runtime")
        .unwrap()
        .items
        .push(syn::parse_quote! {
            fn renamed(task: TaskHandle) { task.set_pending_until(now); }
        });
    assert!(!mutation_violations(&modules).is_empty());
}

#[test]
fn product_runtime_delegates_task_mutations() {
    assert_eq!(
        mutation_violations(&controller_modules()),
        Vec::<String>::new()
    );
}

#[test]
fn mutation_boundary_rejects_ufcs_aliases_macro_calls_and_context_helpers() {
    for source in [
        "fn renamed(task: TaskHandle) { task.set_is_on_other_side(true); }",
        "fn renamed(task: TaskHandle) { task.set_flexible_start_time(now); }",
        "fn renamed(task: TaskHandle) { task.reparent_to(parent); }",
        "fn renamed(task: TaskHandle) { task.unset_deadline_time_opt(); }",
        "fn renamed(task: TaskHandle) { task.set_id(id); task.set_create_time(now); }",
        "use crate::entity::task::TaskHandle as Task; fn renamed() { Task::make_appointment(task, now); }",
        "impl Helper { fn renamed() { format!(\"{:?}\", task.set_actual_work_seconds(1)); } }",
        "use super::handler::ProjectCommandContext as Context; fn renamed<C: Context>(context: &mut C) {}",
    ] {
        let mut modules = controller_modules();
        modules.insert("controller::runtime::helper".into(), product_file(source).unwrap());
        assert!(!mutation_violations(&modules).is_empty(), "{source}");
    }
}

#[test]
fn runtime_may_read_tasks_and_manage_its_own_io_state() {
    let mut modules = controller_modules();
    modules.get_mut("controller::runtime").unwrap().items.extend(product_file("fn renamed(task: TaskHandle) { task.get_id(); task.get_status(); selection.set_explicit(true); repository.reload(); }").unwrap().items);
    assert!(mutation_violations(&modules).is_empty());
}

#[test]
fn type_aliases_cannot_hide_calendar_results_or_inputs() {
    for source in [
        "type Instant = chrono::DateTime<chrono::Local>; fn renamed() -> Instant { chrono::Local::now() }",
        "type Instant = chrono::DateTime<chrono::Local>; type Later = Instant; fn renamed(now: Later) -> i64 { 0 }",
        "type Pair = (chrono::DateTime<chrono::Local>, bool); fn renamed() -> Pair { todo!() }",
    ] {
        let mut modules = controller_modules();
        modules.insert("controller::runtime::helper".into(), product_file(source).unwrap());
        assert!(!datetime_violations(&modules).is_empty(), "{source}");
    }
    let mut modules = controller_modules();
    modules.insert(
        "controller::view::types".into(),
        product_file("pub type Instant = chrono::DateTime<chrono::Local>;").unwrap(),
    );
    modules.insert("controller::runtime::helper".into(), product_file("use crate::adapter::controller::view::types::Instant as Value; fn renamed() -> Value { todo!() }").unwrap());
    assert!(!datetime_violations(&modules).is_empty());
}
