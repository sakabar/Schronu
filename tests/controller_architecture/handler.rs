use super::paths::{input_has, output_has, references};
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
    paths
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
        .collect()
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
