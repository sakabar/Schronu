use super::paths::{expand_local_globs, method_names, references};
use super::source::{controller_modules, module_family};
use std::collections::{BTreeMap, BTreeSet};

fn violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    let mut errors = Vec::new();
    let mut driver_paths = BTreeSet::new();
    let mut driver_methods = BTreeSet::new();
    for root in ["controller::runtime", "controller::interactive"] {
        for (module, file) in module_family(modules, root) {
            let file = match expand_local_globs(module, file, modules) {
                Ok(file) => file,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };
            let paths = match references(module, &file) {
                Ok(paths) => paths,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };
            let methods = match method_names(module, &file) {
                Ok(methods) => methods,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };
            if root == "controller::interactive" {
                driver_paths.extend(paths);
                driver_methods.extend(methods);
            } else {
                for path in paths {
                    if [
                        "termion::event",
                        "termion::input",
                        "termion::raw",
                        "termion::cursor",
                        "termion::clear",
                        "std::io::stdin",
                    ]
                    .iter()
                    .any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}::")))
                    {
                        errors.push(format!("runtime owns terminal dependency: {path}"));
                    }
                }
                for method in methods.intersection(
                    &["recv_timeout", "keys", "into_raw_mode"]
                        .map(String::from)
                        .into(),
                ) {
                    errors.push(format!("runtime owns terminal operation: {method}"));
                }
            }
        }
    }
    for required in [
        "termion::event::Key",
        "termion::input::TermRead",
        "termion::raw::IntoRawMode",
        "termion::raw::RawTerminal",
        "termion::cursor",
        "termion::clear",
        "std::io::stdin",
    ] {
        if !driver_paths
            .iter()
            .any(|path| path == required || path.starts_with(&format!("{required}::")))
        {
            errors.push(format!("interactive driver must own {required}"));
        }
    }
    for required in ["recv_timeout", "keys", "into_raw_mode"] {
        if !driver_methods.contains(required) {
            errors.push(format!("interactive driver must perform {required}"));
        }
    }
    errors
}

#[test]
fn runtime_cannot_own_terminal_cursor_operations() {
    let mut modules = controller_modules();
    modules
        .get_mut("controller::runtime")
        .unwrap()
        .items
        .push(syn::parse_quote! {
            fn arbitrary_name() { let _ = termion::cursor::Goto(1, 2); }
        });
    assert!(!violations(&modules).is_empty());
}

#[test]
fn product_terminal_operations_belong_to_interactive_driver() {
    assert_eq!(violations(&controller_modules()), Vec::<String>::new());
}

#[test]
fn aliases_macros_and_nested_helpers_cannot_hide_terminal_operations() {
    for source in [
        "use termion::cursor as position; fn renamed() { position::Goto(1, 2); }",
        "fn renamed() { format!(\"{}\", termion::clear::All); }",
        "impl Helper { fn renamed(&self) { input.recv_timeout(timeout); } }",
    ] {
        let mut modules = controller_modules();
        modules.insert(
            "controller::runtime::helper".into(),
            super::source::product_file(source).unwrap(),
        );
        assert!(!violations(&modules).is_empty(), "{source}");
    }
}

#[test]
fn terminal_operation_comments_and_literals_are_not_dependencies() {
    let mut modules = controller_modules();
    modules
        .get_mut("controller::runtime")
        .unwrap()
        .items
        .push(syn::parse_quote! {
            fn arbitrary_name() { let note = "termion::cursor and receiver.recv_timeout()"; }
        });
    assert!(violations(&modules).is_empty());
}
