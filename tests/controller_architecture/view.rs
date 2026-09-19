use super::paths::{output_dependencies, output_functions, references};
use super::source::{controller_modules, fixture_modules, module_family};
use std::collections::BTreeMap;

fn violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    let renderers = output_functions(modules);
    let mut errors = Vec::new();
    for (module, file) in module_family(modules, "controller::view") {
        match output_dependencies(module, file, &renderers) {
            Ok(paths) => errors.extend(
                paths
                    .into_iter()
                    .map(|path| format!("view depends on output: {path}")),
            ),
            Err(error) => errors.push(error),
        }
        match references(module, file) {
            Ok(paths) => errors.extend(
                paths
                    .into_iter()
                    .filter(|path| {
                        [
                            "controller::runtime",
                            "controller::interactive",
                            "controller::handler",
                            "controller::command_context",
                            "std::fs",
                            "std::process",
                            "std::env",
                            "webbrowser",
                            "crate::application::repository_transaction",
                        ]
                        .iter()
                        .any(|prefix| path == prefix || path.starts_with(&format!("{prefix}::")))
                    })
                    .map(|path| format!("view depends on coordination: {path}")),
            ),
            Err(error) => errors.push(error),
        }
    }
    errors
}

#[test]
fn view_rejects_output_from_a_nested_builder() {
    let modules = fixture_modules(
        "mod view; mod renderer;",
        &[
            (
                "view.rs",
                "mod nested { fn renamed() { println!(\"output\"); } }",
            ),
            ("renderer.rs", ""),
        ],
    );
    assert!(!violations(&modules).is_empty());
}

#[test]
fn product_view_family_is_writer_free() {
    assert_eq!(violations(&controller_modules()), Vec::<String>::new());
}

#[test]
fn view_rejects_writer_aliases_output_functions_and_coordination() {
    for source in [
        "use std::io::Write as Sink; fn renamed(writer: &mut dyn Sink) {}",
        "fn renamed() { super::renderer::emit(writer); }",
        "use super::runtime as outer; fn renamed() { outer::run(); }",
        "fn renamed() { std::io::Write::flush(writer); }",
    ] {
        let modules = fixture_modules(
            "mod view; mod renderer;",
            &[
                ("view.rs", source),
                (
                    "renderer.rs",
                    "use std::io::Write as Sink; fn emit(writer: &mut dyn Sink) {}",
                ),
            ],
        );
        assert!(!violations(&modules).is_empty(), "{source}");
    }
}

#[test]
fn view_accepts_renamed_pure_builders_and_non_code_decoys() {
    let modules = fixture_modules(
        "mod view; mod renderer;",
        &[
            (
                "view.rs",
                r###"fn arbitrary_name() -> DisplayModel {
            // println!("not code");
            let note = r#"std::io::Write writer.flush()"#;
            DisplayModel::Message { text: note.into() }
        }"###,
            ),
            ("renderer.rs", ""),
        ],
    );
    assert!(violations(&modules).is_empty());
}

#[test]
fn external_view_helpers_keep_the_same_writer_boundary() {
    let modules = fixture_modules(
        "mod view; mod renderer;",
        &[
            ("view.rs", "mod helper;"),
            ("view/helper.rs", "fn renamed() { writer.flush(); }"),
            ("renderer.rs", ""),
        ],
    );
    assert!(!violations(&modules).is_empty());
}
