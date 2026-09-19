use super::paths::{input_has, output_has, references};
use super::source::{controller_modules, product_file};

fn violations(file: &syn::File, command: &syn::File) -> Vec<String> {
    let paths = match references("controller::handler", file) {
        Ok(paths) => paths,
        Err(error) => return vec![error],
    };
    let parser_paths: Vec<_> = command
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(function)
                if output_has(&function.sig, "Command")
                    && (input_has(&function.sig, "str") || input_has(&function.sig, "String")) =>
            {
                Some(format!("controller::command::{}", function.sig.ident))
            }
            _ => None,
        })
        .collect();
    paths
        .into_iter()
        .filter(|path| {
            [
                "controller::runtime",
                "controller::interactive",
                "crate::adapter::gateway",
                "crate::application::repository_transaction",
                "termion",
                "webbrowser",
                "std::env",
                "std::process",
                "std::fs",
                "std::io",
            ]
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

#[test]
fn handler_rejects_an_outer_runtime_dependency() {
    let file = product_file("use super::runtime as outer; fn renamed() { outer::run(); }").unwrap();
    assert!(!violations(&file, &product_file("").unwrap()).is_empty());
}

#[test]
fn product_handler_has_no_outer_io_dependency() {
    let modules = controller_modules();
    assert!(violations(
        &modules["controller::handler"],
        &modules["controller::command"]
    )
    .is_empty());
}

#[test]
fn handler_rejects_qualified_aliased_and_macro_io_dependencies() {
    for body in [
        "fn renamed() { crate::adapter::gateway::task_repository::TaskRepository::new(); }",
        "use std::env as settings; fn renamed() { settings::var(\"HOME\"); }",
        "fn renamed() { format!(\"{}\", super::runtime::run()); }",
        "fn renamed() { println!(\"side effect\"); }",
    ] {
        assert!(
            !violations(&product_file(body).unwrap(), &product_file("").unwrap()).is_empty(),
            "{body}"
        );
    }
}

#[test]
fn handler_cannot_reconstruct_and_reparse_a_typed_command() {
    let command = product_file(
        "pub(super) fn renamed_parser(input: &str) -> Result<Command, Error> { todo!() }",
    )
    .unwrap();
    let handler = product_file(
        "use super::command::renamed_parser as decode; fn renamed_handler() { decode(\"input\"); }",
    )
    .unwrap();
    assert!(!violations(&handler, &command).is_empty());
}

#[test]
fn handler_dependency_rules_ignore_names_and_non_code_decoys() {
    let file = product_file(
        r###"
        use super::renderer::DisplayModel;
        fn entirely_different_name() -> DisplayModel {
            // super::runtime::run();
            let text = r#"std::env TaskRepository println!"#;
            DisplayModel::Message { text: text.into() }
        }
    "###,
    )
    .unwrap();
    assert!(violations(&file, &product_file("").unwrap()).is_empty());
}
