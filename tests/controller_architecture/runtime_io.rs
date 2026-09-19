use super::paths::{expand_local_globs, references, resolve_path};
use super::source::{controller_modules, module_family, product_file};
use std::collections::BTreeMap;
use syn::ext::IdentExt;
use syn::visit::{self, Visit};

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
    struct Contexts<'a> {
        module: &'a str,
        file: &'a syn::File,
        errors: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Contexts<'_> {
        fn visit_item_mod(&mut self, _item: &'ast syn::ItemMod) {}
        fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
            if item.ident.unraw().to_string().ends_with("Context") {
                self.errors
                    .push(format!("runtime owns context: {}", item.ident));
            }
            visit::visit_item_struct(self, item);
        }
        fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
            if let Some((_, path, _)) = &item.trait_ {
                match resolve_path(self.module, self.file, path) {
                    Ok(path) if path.ends_with("CommandContext") => {
                        self.errors
                            .push(format!("runtime implements command context: {path}"));
                    }
                    Err(error) => self.errors.push(error),
                    _ => {}
                }
            }
            visit::visit_item_impl(self, item);
        }
    }
    let mut errors = Vec::new();
    for (module, file) in module_family(modules, "controller::runtime") {
        match expand_local_globs(module, file, modules) {
            Ok(file) => {
                let mut visitor = Contexts {
                    module,
                    file: &file,
                    errors: Vec::new(),
                };
                visitor.visit_file(&file);
                errors.extend(visitor.errors);
            }
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
