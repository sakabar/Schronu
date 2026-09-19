use super::paths::{
    data_accesses, input_has, method_names, output_has, references, signature_paths, used_paths,
};
use super::source::{controller_modules, fixture_modules, module_family};
use std::collections::BTreeMap;
use syn::ext::IdentExt;

fn violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    let parser_paths: Vec<_> = module_family(modules, "controller::command")
        .flat_map(|(module, file)| {
            file.items.iter().filter_map(move |item| match item {
                syn::Item::Fn(function)
                    if output_has(&function.sig, "Command")
                        && (input_has(&function.sig, "str")
                            || input_has(&function.sig, "String")) =>
                {
                    Some(format!("{module}::{}", function.sig.ident.unraw()))
                }
                _ => None,
            })
        })
        .collect();
    module_family(modules, "controller::handler")
        .flat_map(|(module, file)| module_violations(module, file, &parser_paths))
        .collect()
}

fn module_violations(module: &str, file: &syn::File, parser_paths: &[String]) -> Vec<String> {
    let paths = match references(module, file) {
        Ok(paths) => paths,
        Err(error) => return vec![error],
    };
    let mut errors: Vec<_> = paths
        .into_iter()
        .filter(|path| {
            super::runtime_io::external_io_dependency(path)
                || ["controller::interactive", "termion", "std::io"]
                    .iter()
                    .any(|prefix| path == prefix || path.starts_with(&format!("{prefix}::")))
                || [
                    "TaskRepository",
                    "TaskRepositoryTrait",
                    "print",
                    "println",
                    "eprint",
                    "eprintln",
                ]
                .contains(&path.rsplit("::").next().unwrap_or(path))
                || parser_paths.contains(path)
        })
        .map(|path| format!("handler owns an outer dependency: {path}"))
        .collect();
    for item in &file.items {
        let syn::Item::Fn(function) = item else {
            continue;
        };
        let check = || -> Result<(), String> {
            if !signature_paths(module, file, &function.sig)?
                .iter()
                .any(|path| path.ends_with("FinishPlacementCommandContext"))
            {
                return Ok(());
            }
            let mut scope = file.clone();
            scope.items.retain(|item| matches!(item, syn::Item::Use(_)));
            scope.items.push(syn::Item::Fn(function.clone()));
            let (members, indexed) = data_accesses(module, &scope)?;
            let methods = method_names(module, &scope)?;
            let paths = used_paths(module, &scope)?;
            if indexed
                || members.contains("canonical_name")
                || methods
                    .iter()
                    .any(|method| ["split_whitespace", "get"].contains(&method.as_str()))
                || paths.iter().any(|path| {
                    path.rsplit("::").next().is_some_and(|name| {
                        ["split_whitespace", "canonical_name", "get"].contains(&name)
                    })
                })
            {
                return Err("finish/placement handler reconstructs raw command tokens".into());
            }
            Ok(())
        };
        if let Err(error) = check() {
            errors.push(error);
        }
    }
    errors
}

fn fixture(handler: &str, command: &str) -> BTreeMap<String, syn::File> {
    fixture_modules(
        "mod handler; mod command;",
        &[("handler.rs", handler), ("command.rs", command)],
    )
}

#[test]
fn handler_rejects_an_outer_runtime_dependency() {
    assert!(!violations(&fixture(
        "use super::runtime as outer; fn renamed() { outer::run(); }",
        ""
    ))
    .is_empty());
}

#[test]
fn product_handler_has_no_outer_io_dependency() {
    assert!(violations(&controller_modules()).is_empty());
}

#[test]
fn handler_rejects_qualified_aliased_and_macro_io_dependencies() {
    for body in [
        "fn renamed() { crate::adapter::gateway::task_repository::TaskRepository::new(); }",
        "use std::env as settings; fn renamed() { settings::var(\"HOME\"); }",
        "fn renamed() { format!(\"{}\", super::runtime::run()); }",
        "fn renamed() { println!(\"side effect\"); }",
    ] {
        assert!(!violations(&fixture(body, "")).is_empty(), "{body}");
    }
}

#[test]
fn handler_cannot_reconstruct_and_reparse_a_typed_command() {
    let modules = fixture(
        "use super::command::renamed_parser as decode; fn renamed_handler() { decode(\"input\"); }",
        "pub(super) fn renamed_parser(input: &str) -> Result<Command, Error> { todo!() }",
    );
    assert!(!violations(&modules).is_empty());
}

#[test]
fn handler_dependency_rules_ignore_names_and_non_code_decoys() {
    let modules = fixture(
        r###"
        use super::renderer::DisplayModel;
        fn entirely_different_name() -> DisplayModel {
            // super::runtime::run();
            let text = r#"std::env TaskRepository println!"#;
            DisplayModel::Message { text: text.into() }
        }
    "###,
        "",
    );
    assert!(violations(&modules).is_empty());
}

#[test]
fn handler_owned_helpers_cannot_hide_outer_io() {
    let direct = violations(&fixture(
        "fn renamed() { std::fs::write(\"output\", \"data\").unwrap(); }",
        "",
    ));
    let inline = violations(&fixture("mod helper { pub(super) fn run() { std::fs::write(\"output\", \"data\").unwrap(); } } fn renamed_handler() { helper::run(); }", ""));
    let external = fixture_modules(
        "mod handler; mod command;",
        &[
            (
                "handler.rs",
                "mod helper; fn renamed_handler() { helper::run(); }",
            ),
            (
                "handler/helper.rs",
                "pub(super) fn run() { std::fs::write(\"output\", \"data\").unwrap(); }",
            ),
            ("command.rs", ""),
        ],
    );
    assert!(!direct.is_empty());
    assert_eq!(inline, direct);
    assert_eq!(violations(&external), direct);
}

#[test]
fn finish_placement_cannot_reconstruct_tokens_without_calling_a_parser() {
    let modules = fixture(
        "fn renamed<C: FinishPlacementCommandContext>(command: &Command, context: &mut C) -> CommandOutcome { let tokens: Vec<_> = canonical_name.split_whitespace().collect(); todo!() }",
        "",
    );
    assert!(!violations(&modules).is_empty());
}

#[test]
fn finish_placement_token_reconstruction_cannot_hide_in_macros_or_member_aliases() {
    for body in [
        "match command { Command::Action(CommandAction::NoArguments { canonical_name: alias, .. }) => consume(alias), _ => () };",
        "format!(\"{:?}\", values.get(0));",
        "let alias = values; consume(alias[0]);",
        "<[String]>::get(values, 0);",
        "str::split_whitespace(canonical_name);",
    ] {
        let modules = fixture(&format!("fn renamed<C: FinishPlacementCommandContext>(command: &Command, context: &mut C) -> CommandOutcome {{ {body} todo!() }}"), "");
        assert!(!violations(&modules).is_empty(), "{body}");
    }
}

#[test]
fn renamed_finish_placement_handler_may_delegate_typed_finish_values() {
    let modules = fixture("use super::handler::FinishPlacementCommandContext as Context; fn renamed<C: Context>(command: &Command, context: &mut C) -> CommandOutcome { let example = \"canonical_name values[0] split_whitespace\"; if values.is_empty() {} decide_typed_values(values); todo!() }", "");
    assert!(violations(&modules).is_empty());
}
