use super::paths::{
    block_calls, expand_local_globs, function_calls, input_has, method_names, output_has,
    references, used_paths,
};
use super::source::{controller_modules, module_family};
use std::collections::{BTreeMap, BTreeSet};
use syn::ext::IdentExt;
use syn::visit::{self, Visit};

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
                        || matches!(
                            path.rsplit("::").next(),
                            Some("recv_timeout" | "keys" | "into_raw_mode")
                        )
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
        if !driver_methods.contains(required)
            && !driver_paths
                .iter()
                .any(|path| path.rsplit("::").next() == Some(required))
        {
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
        "fn renamed() { std::sync::mpsc::Receiver::recv_timeout(&receiver, timeout); }",
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

fn roles<'a>(
    modules: &'a BTreeMap<String, syn::File>,
    root: &'a str,
    input: &str,
    output: &str,
) -> Vec<(&'a str, &'a syn::File, &'a syn::ItemFn)> {
    module_family(modules, root)
        .flat_map(|(module, file)| {
            file.items.iter().filter_map(move |item| match item {
                syn::Item::Fn(function)
                    if input_has(&function.sig, input) && output_has(&function.sig, output) =>
                {
                    Some((module, file, function))
                }
                _ => None,
            })
        })
        .collect()
}

fn event_violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    let classifiers = roles(modules, "controller::interactive", "CommandKind", "bool");
    let events = roles(
        modules,
        "controller::runtime",
        "DriverEvent",
        "DriverOutcome",
    );
    let drivers: Vec<_> = roles(
        modules,
        "controller::interactive",
        "FnMut",
        "DriverRunError",
    )
    .into_iter()
    .filter(|(_, _, f)| f.sig.inputs.len() == 2)
    .collect();
    let (
        [(classifier_module, classifier_file, classifier)],
        [(event_module, event_file, event)],
        [(driver_module, _, driver)],
    ) = (
        classifiers.as_slice(),
        events.as_slice(),
        drivers.as_slice(),
    )
    else {
        return vec![
            "one typed redraw classifier, event boundary, and driver entry required".into(),
        ];
    };
    let classifier_path = format!("{classifier_module}::{}", classifier.sig.ident.unraw());
    let driver_path = format!("{driver_module}::{}", driver.sig.ident.unraw());
    let event_path = format!("{event_module}::{}", event.sig.ident.unraw());
    let mut errors = Vec::new();
    let expanded = match expand_local_globs(event_module, event_file, modules) {
        Ok(file) => file,
        Err(error) => return vec![error],
    };
    let calls = match function_calls(event_module, &expanded, event) {
        Ok(calls) => calls,
        Err(error) => return vec![error],
    };
    let classifiers: Vec<_> = calls
        .iter()
        .filter(|(path, _)| path == &classifier_path)
        .collect();
    struct ExecutedKind(BTreeSet<String>);
    impl<'ast> Visit<'ast> for ExecutedKind {
        fn visit_pat_tuple_struct(&mut self, pattern: &'ast syn::PatTupleStruct) {
            if pattern
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident.unraw() == "CommandExecuted")
            {
                if let Some(syn::Pat::Ident(name)) = pattern.elems.first() {
                    self.0.insert(name.ident.unraw().to_string());
                }
            }
            visit::visit_pat_tuple_struct(self, pattern);
        }
    }
    let mut kinds = ExecutedKind(BTreeSet::new());
    kinds.visit_block(&event.block);
    if classifiers.len() != 1 || !classifiers.first().is_some_and(|(_, call)| matches!(call.args.first(), Some(syn::Expr::Path(path)) if kinds.0.iter().any(|kind| path.path.is_ident(kind)))) {
        errors.push("driver event must pass its executed CommandKind directly to the classifier".into());
    }
    for (module, file, function) in [
        (*classifier_module, *classifier_file, *classifier),
        (*event_module, &expanded, *event),
    ] {
        let mut scope = file.clone();
        scope.items.retain(|item| matches!(item, syn::Item::Use(_)));
        scope.items.push(syn::Item::Fn(function.clone()));
        let paths = match used_paths(module, &scope) {
            Ok(paths) => paths,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let methods = match method_names(module, &scope) {
            Ok(methods) => methods,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        if ["chars", "split_whitespace"].iter().any(|method| {
            methods.contains(*method)
                || paths
                    .iter()
                    .any(|path| path.ends_with(&format!("::{method}")))
        }) {
            errors.push("typed redraw boundary must not recover meaning from raw text".into());
        }
        for (parser_module, _, parser) in roles(modules, "controller::command", "str", "Command")
            .into_iter()
            .chain(roles(modules, "controller::command", "String", "Command"))
        {
            if paths.contains(&format!("{parser_module}::{}", parser.sig.ident.unraw())) {
                errors.push("typed redraw boundary must not reparse commands".into());
            }
        }
    }
    let mut connected_entries = 0;
    for (module, file) in module_family(modules, "controller::runtime") {
        let expanded = match expand_local_globs(module, file, modules) {
            Ok(file) => file,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        for item in &file.items {
            let syn::Item::Fn(function) = item else {
                continue;
            };
            let calls = match function_calls(module, &expanded, function) {
                Ok(calls) => calls,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };
            for (_, driver_call) in calls.iter().filter(|(path, _)| path == &driver_path) {
                connected_entries += 1;
                let Some(syn::Expr::Closure(callback)) = driver_call.args.last() else {
                    errors.push("driver requires an explicit event callback".into());
                    continue;
                };
                let body = syn::Block {
                    brace_token: Default::default(),
                    stmts: vec![syn::Stmt::Expr(*callback.body.clone(), None)],
                };
                let calls = match block_calls(module, &expanded, &body) {
                    Ok(calls) => calls,
                    Err(error) => {
                        errors.push(error);
                        continue;
                    }
                };
                if calls
                    .iter()
                    .filter(|(path, _)| {
                        path == &event_path
                            || (module == *event_module
                                && path == &event.sig.ident.unraw().to_string())
                    })
                    .count()
                    != 1
                {
                    errors.push(
                        "interactive entry must connect driver events to the typed boundary".into(),
                    );
                }
            }
        }
    }
    if connected_entries != 1 {
        errors.push("one connected interactive driver entry required".into());
    }
    errors
}

#[test]
fn typed_driver_event_cannot_bypass_the_redraw_classifier() {
    let mut modules = controller_modules();
    let event = modules
        .get_mut("controller::runtime")
        .unwrap()
        .items
        .iter_mut()
        .find_map(|item| match item {
            syn::Item::Fn(function)
                if super::paths::input_has(&function.sig, "DriverEvent")
                    && super::paths::output_has(&function.sig, "DriverOutcome") =>
            {
                Some(function)
            }
            _ => None,
        })
        .unwrap();
    *event.block = syn::parse_quote!({ interactive::DriverOutcome::Continue });
    assert!(!event_violations(&modules).is_empty());
}

#[test]
fn product_events_reach_the_typed_redraw_classifier() {
    assert_eq!(
        event_violations(&controller_modules()),
        Vec::<String>::new()
    );
}

#[test]
fn renaming_driver_roles_preserves_the_event_contract() {
    use syn::visit_mut::VisitMut;
    let mut modules = controller_modules();
    let names: BTreeSet<_> = roles(&modules, "controller::interactive", "CommandKind", "bool")
        .into_iter()
        .chain(roles(
            &modules,
            "controller::runtime",
            "DriverEvent",
            "DriverOutcome",
        ))
        .map(|(_, _, function)| function.sig.ident.unraw().to_string())
        .collect();
    struct Rename(BTreeSet<String>);
    impl VisitMut for Rename {
        fn visit_ident_mut(&mut self, ident: &mut syn::Ident) {
            if self.0.contains(&ident.unraw().to_string()) {
                *ident = syn::Ident::new(&format!("renamed_{}", ident.unraw()), ident.span());
            }
        }
    }
    for file in modules.values_mut() {
        Rename(names.clone()).visit_file_mut(file);
    }
    assert!(event_violations(&modules).is_empty());
}

#[test]
fn driver_entry_cannot_drop_the_product_event_callback() {
    let mut modules = controller_modules();
    let driver_name = roles(
        &modules,
        "controller::interactive",
        "FnMut",
        "DriverRunError",
    )
    .into_iter()
    .find(|(_, _, f)| f.sig.inputs.len() == 2)
    .unwrap()
    .2
    .sig
    .ident
    .clone();
    let runtime = &modules["controller::runtime"];
    let expanded = expand_local_globs("controller::runtime", runtime, &modules).unwrap();
    let index = runtime.items.iter().position(|item| matches!(item, syn::Item::Fn(function) if function_calls("controller::runtime", &expanded, function).unwrap().iter().any(|(path, _)| path == &format!("controller::interactive::{driver_name}")))).unwrap();
    let syn::Item::Fn(entry) = &mut modules.get_mut("controller::runtime").unwrap().items[index]
    else {
        panic!("entry")
    };
    *entry.block = syn::parse_quote!({ interactive::#driver_name(now, |_, event| interactive::DriverOutcome::Continue); Ok(()) });
    assert!(!event_violations(&modules).is_empty());
    let event_name = roles(
        &modules,
        "controller::runtime",
        "DriverEvent",
        "DriverOutcome",
    )[0]
    .2
    .sig
    .ident
    .clone();
    let syn::Item::Fn(entry) = &mut modules.get_mut("controller::runtime").unwrap().items[index]
    else {
        panic!("entry")
    };
    *entry.block = syn::parse_quote!({
        #event_name(writer, state, event);
        interactive::#driver_name(now, |_, event| interactive::DriverOutcome::Continue);
        Ok(())
    });
    assert!(!event_violations(&modules).is_empty());
}
