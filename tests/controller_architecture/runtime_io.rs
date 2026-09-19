use super::paths::{
    definitions, expand_local_globs, references, resolve_path, return_paths, signature_paths,
    signatures,
};
use super::source::{controller_modules, module_family, product_file};
use std::collections::BTreeMap;
use syn::ext::IdentExt;

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
                        if [
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
                        {
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
                let output = return_paths(module, &file, &signature)?;
                let parameters = signature_paths(module, &file, &signature)?;
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

fn mutation_violations(_modules: &BTreeMap<String, syn::File>) -> Vec<String> { Vec::new() }

#[test]
fn renamed_runtime_helper_cannot_mutate_task_domain_state() {
    let mut modules = controller_modules();
    modules.get_mut("controller::runtime").unwrap().items.push(syn::parse_quote! {
        fn renamed(task: TaskHandle) { task.set_pending_until(now); }
    });
    assert!(!mutation_violations(&modules).is_empty());
}
