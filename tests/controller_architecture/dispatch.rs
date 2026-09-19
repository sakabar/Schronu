use super::paths::{
    expand_local_globs, function_calls, function_calls_with_callbacks, input_has, output_has,
    references, signature_paths,
};
use super::source::{controller_modules, fixture_modules, module_family};
use std::collections::{BTreeMap, BTreeSet};
use syn::visit::{self, Visit};

fn violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    let mut errors = Vec::new();
    let handler = &modules["controller::handler"];
    let handlers: Vec<_> = functions(handler)
        .filter(|f| input_has(&f.sig, "Command") && output_has(&f.sig, "CommandOutcome"))
        .collect();
    let unified: Vec<_> = handlers
        .iter()
        .filter(|f| {
            signature_paths("controller::handler", handler, &f.sig)
                .unwrap()
                .iter()
                .any(|p| p.rsplit("::").next() == Some("CommandContext"))
        })
        .collect();
    let [unified] = unified.as_slice() else {
        return vec!["one unified CommandContext handler required".into()];
    };
    let unified = format!("controller::handler::{}", unified.sig.ident);
    let component_paths: Vec<_> = handlers
        .iter()
        .map(|f| format!("controller::handler::{}", f.sig.ident))
        .filter(|p| p != &unified)
        .collect();
    let parser = &modules["controller::command"];
    let parser_paths: Vec<_> = functions(parser)
        .filter(|f| {
            !matches!(f.vis, syn::Visibility::Inherited)
                && output_has(&f.sig, "Command")
                && !input_has(&f.sig, "ParseMode")
                && !output_has(&f.sig, "CommandKind")
        })
        .map(|f| {
            (
                format!("controller::command::{}", f.sig.ident),
                input_has(&f.sig, "str"),
            )
        })
        .collect();
    let runtime = match expand_local_globs(
        "controller::runtime",
        &modules["controller::runtime"],
        modules,
    ) {
        Ok(file) => file,
        Err(error) => return vec![error],
    };
    let dispatchers: Vec<_> = functions(&runtime)
        .filter(|f| input_has(&f.sig, "Command") && output_has(&f.sig, "CommandError"))
        .collect();
    let [dispatcher] = dispatchers.as_slice() else {
        return vec!["one typed Command coordinator required".into()];
    };
    let dispatcher_path = format!("controller::runtime::{}", dispatcher.sig.ident);
    let callbacks = transaction_callbacks(&runtime);
    let entries: Vec<_> = functions(&runtime)
        .filter(|f| {
            input_has(&f.sig, "TaskRepositoryTrait")
                && ((input_has(&f.sig, "str") && output_has(&f.sig, "InteractiveCommandExecution"))
                    || (input_has(&f.sig, "String")
                        && input_has(&f.sig, "DateTime")
                        && output_has(&f.sig, "RunError")))
        })
        .collect();
    if entries.len() != 2 {
        errors.push("two typed execution entries required".into());
    }
    for entry in entries {
        let calls =
            match function_calls_with_callbacks("controller::runtime", &runtime, entry, &callbacks)
            {
                Ok(calls) => calls,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };
        if calls
            .iter()
            .filter(|(path, _)| {
                path == &dispatcher_path || path == &dispatcher.sig.ident.to_string()
            })
            .count()
            != 1
        {
            errors.push(format!(
                "entry must call shared coordinator once: {}",
                entry.sig.ident
            ));
        }
        let parsers: Vec<_> = parser_paths
            .iter()
            .filter(|(_, text)| *text == input_has(&entry.sig, "str"))
            .collect();
        if parsers.len() != 1
            || calls
                .iter()
                .filter(|(path, _)| path == &parsers[0].0)
                .count()
                != 1
        {
            errors.push(format!(
                "entry must use its typed parser: {}",
                entry.sig.ident
            ));
        }
        let mut body = runtime.clone();
        body.items = vec![syn::Item::Fn(entry.clone())];
        // Keep imports for alias identity, without counting their paths as uses.
        body.items.extend(
            runtime
                .items
                .iter()
                .filter(|item| matches!(item, syn::Item::Use(_)))
                .cloned(),
        );
        match references("controller::runtime", &body) {
            Ok(paths) => {
                for path in paths {
                    if (path.starts_with("controller::command::Command::")
                        && !matches!(path.rsplit("::").next(), Some("Backup" | "Restore")))
                        || (path.starts_with("controller::command::CommandKind::")
                            && !path.ends_with("::Verify"))
                    {
                        errors.push(format!(
                            "entry branches on a non-maintenance command: {path}"
                        ));
                    }
                }
            }
            Err(error) => errors.push(error),
        }
    }
    for (module, file) in module_family(modules, "controller::runtime") {
        let file = match expand_local_globs(module, file, modules) {
            Ok(file) => file,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        for function in functions(&file) {
            match function_calls(module, &file, function) {
                Ok(calls) => {
                    for (path, _) in calls {
                        if component_paths.contains(&path)
                            || (path == unified
                                && format!("{module}::{}", function.sig.ident) != dispatcher_path)
                        {
                            errors.push(format!("runtime bypasses unified dispatch: {path}"));
                        }
                    }
                }
                Err(error) => errors.push(error),
            }
        }
    }
    let calls = function_calls("controller::runtime", &runtime, dispatcher).unwrap();
    let direct: Vec<_> = calls.iter().filter(|(path, _)| path == &unified).collect();
    if direct.len() != 1 {
        errors.push("coordinator must call unified handler once".into());
    }
    let parameters: Vec<_> = dispatcher
        .sig
        .inputs
        .iter()
        .filter_map(|arg| match arg {
            syn::FnArg::Typed(arg) if super::paths::type_has(&arg.ty, "Command") => {
                match (&*arg.ty, &*arg.pat) {
                    (syn::Type::Reference(_), syn::Pat::Ident(name)) => {
                        Some(name.ident.to_string())
                    }
                    _ => None,
                }
            }
            _ => None,
        })
        .collect();
    if let [parameter] = parameters.as_slice() {
        struct CommandUses<'a> {
            name: &'a str,
            count: usize,
        }
        impl<'ast> Visit<'ast> for CommandUses<'_> {
            fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
                if path.path.is_ident(self.name) {
                    self.count += 1;
                }
                visit::visit_expr_path(self, path);
            }
        }
        let mut uses = CommandUses {
            name: parameter,
            count: 0,
        };
        uses.visit_block(&dispatcher.block);
        if uses.count != 1 || !direct.first().is_some_and(|(_, call)| matches!(call.args.first(), Some(syn::Expr::Path(p)) if p.path.is_ident(parameter))) {
            errors.push("coordinator must pass its Command reference directly without inspecting it".into());
        }
    } else {
        errors.push("coordinator must accept one Command reference".into());
    }
    errors
}

fn transaction_callbacks(file: &syn::File) -> BTreeSet<String> {
    let transaction = "crate::application::repository_transaction::run_repository_transaction";
    let mut callbacks = BTreeSet::new();
    for function in functions(file)
        .filter(|f| input_has(&f.sig, "FnOnce") && input_has(&f.sig, "TaskRepositoryTrait"))
    {
        let parameters: Vec<_> = function
            .sig
            .inputs
            .iter()
            .filter_map(|argument| match argument {
                syn::FnArg::Typed(argument) if super::paths::type_has(&argument.ty, "FnOnce") => {
                    match &*argument.pat {
                        syn::Pat::Ident(name) => Some(name.ident.to_string()),
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect();
        let Ok(calls) = function_calls_with_callbacks(
            "controller::runtime",
            file,
            function,
            &BTreeSet::from([transaction.into()]),
        ) else {
            continue;
        };
        if let [parameter] = parameters.as_slice() {
            if calls.iter().filter(|(path, _)| path == transaction).count() == 1
                && calls.iter().filter(|(path, _)| path == parameter).count() == 1
            {
                callbacks.insert(function.sig.ident.to_string());
                callbacks.insert(format!("controller::runtime::{}", function.sig.ident));
            }
        }
    }
    callbacks
}

fn functions(file: &syn::File) -> impl Iterator<Item = &syn::ItemFn> {
    file.items.iter().filter_map(|item| match item {
        syn::Item::Fn(f) => Some(f),
        _ => None,
    })
}

fn fixture(entry: &str) -> BTreeMap<String, syn::File> {
    fixture_modules("mod runtime; mod command; mod handler;", &[
        ("command.rs", "pub(super) fn decode_line(line: &str) -> Result<Command, Error> { todo!() } pub(super) fn decode_args(args: &[String]) -> Result<Command, Error> { todo!() }"),
        ("handler.rs", "pub(super) fn unified<C: CommandContext>(command: &Command, context: &mut C) -> CommandOutcome { todo!() } pub(super) fn component(command: &Command) -> CommandOutcome { todo!() }"),
        ("runtime.rs", &format!("use super::command::{{Command, CommandKind, decode_line, decode_args}}; use super::handler::{{unified, component}};
            fn coordinate(command: &Command) -> Result<(), CommandError> {{ unified(command, context); Ok(()) }}
            fn terminal(repository: &mut dyn TaskRepositoryTrait, input: &str) -> InteractiveCommandExecution {{ let command = decode_line(input); {entry} }}
            fn argv(repository: &mut dyn TaskRepositoryTrait, input: &[String], now: DateTime<Local>) -> Result<(), RunError> {{ let command = decode_args(input); coordinate(&command); Ok(()) }}")),
    ])
}

#[test]
fn entry_cannot_bypass_the_shared_typed_dispatcher() {
    assert!(!violations(&fixture("component(&command);")).is_empty());
}

#[test]
fn renamed_typed_entries_and_coordinator_are_accepted() {
    assert_eq!(
        violations(&fixture("coordinate(&command);")),
        Vec::<String>::new()
    );
}

#[test]
fn product_entries_share_parser_and_unified_handler() {
    assert_eq!(violations(&controller_modules()), Vec::<String>::new());
}

#[test]
fn typed_entry_rejects_missing_calls_and_nonmaintenance_branches() {
    for entry in [
        "std::mem::drop(|| coordinate(&command));",
        "let unused = || coordinate(&command);",
        "coordinate(&command); if command.kind() == CommandKind::Focus {}",
        "unified(&command, context); coordinate(&command);",
    ] {
        assert!(!violations(&fixture(entry)).is_empty(), "{entry}");
    }
}

#[test]
fn coordinator_cannot_inspect_or_replace_its_command() {
    let mut modules = fixture("coordinate(&command);");
    for body in [
        "{ command.kind(); unified(command, context); Ok(()) }",
        "{ unified(other, context); Ok(()) }",
    ] {
        let runtime = modules.get_mut("controller::runtime").unwrap();
        let function = runtime
            .items
            .iter_mut()
            .find_map(|item| match item {
                syn::Item::Fn(f) if f.sig.ident == "coordinate" => Some(f),
                _ => None,
            })
            .unwrap();
        *function.block = syn::parse_str(body).unwrap();
        assert!(!violations(&modules).is_empty());
    }
}
