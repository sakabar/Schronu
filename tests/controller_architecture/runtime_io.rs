use super::paths::references;
use super::source::{controller_modules, module_family, product_file};
use std::collections::BTreeMap;

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
