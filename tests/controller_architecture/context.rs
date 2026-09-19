use super::paths::{output_dependencies, output_functions, references};
use super::source::{controller_modules, fixture_modules, module_family};
use std::collections::BTreeMap;

fn violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    let renderers = output_functions(modules);
    let mut errors = Vec::new();
    for root in ["controller::command_context", "controller::handler"] {
        for (module, file) in module_family(modules, root) {
            let mut file = file.clone();
            if root == "controller::handler" {
                // Error Display implementations format into fmt::Formatter;
                // they do not expose a terminal writer to command handlers.
                file.items.retain(|item| !matches!(item,
                    syn::Item::Impl(implementation) if implementation.trait_.as_ref().is_some_and(|(_, path, _)|
                        path.segments.iter().map(|segment| segment.ident.to_string()).eq(["std", "fmt", "Display"])
                    )
                ));
            }
            match output_dependencies(module, &file, &renderers) {
                Ok(paths) => errors.extend(
                    paths
                        .into_iter()
                        .map(|path| format!("{module} depends on output: {path}")),
                ),
                Err(error) => errors.push(error),
            }
            if root == "controller::command_context" {
                match references(module, &file) {
                    Ok(paths) => errors.extend(
                        paths
                            .into_iter()
                            .filter(|path| {
                                path == "controller::runtime"
                                    || path.starts_with("controller::runtime::")
                            })
                            .map(|path| format!("context depends on runtime: {path}")),
                    ),
                    Err(error) => errors.push(error),
                }
            }
        }
    }
    errors
}

#[test]
fn context_rejects_a_writer_parameter_in_a_command_trait() {
    let modules = fixture_modules("mod handler; mod command_context; mod renderer;", &[
        ("handler.rs", "trait TaskTreeCommandContext { fn show(&mut self, writer: &mut dyn std::io::Write); }"),
        ("command_context.rs", ""), ("renderer.rs", ""),
    ]);
    assert!(!violations(&modules).is_empty());
}

#[test]
fn product_contexts_and_handler_contracts_are_writer_free() {
    assert_eq!(violations(&controller_modules()), Vec::<String>::new());
}

#[test]
fn context_rejects_output_in_renamed_and_nested_helpers() {
    for body in [
        "fn renamed(writer: &mut dyn SchronuWriter) {}",
        "mod helper { fn renamed() { println!(\"output\"); } }",
        "fn renamed() { format!(\"{:?}\", writer.flush()); }",
        "use super::runtime as outer; fn renamed() { outer::run(); }",
        "use super::renderer::renamed_output as emit; fn renamed() { emit(writer); }",
    ] {
        let modules = fixture_modules(
            "mod handler; mod command_context; mod renderer;",
            &[
                ("handler.rs", ""),
                ("command_context.rs", body),
                (
                    "renderer.rs",
                    "pub(super) fn renamed_output<W: SchronuWriter>(writer: W) {}",
                ),
            ],
        );
        assert!(!violations(&modules).is_empty(), "{body}");
    }
}

#[test]
fn handler_helpers_cannot_hide_renderer_calls() {
    let modules = fixture_modules(
        "mod handler; mod command_context; mod renderer;",
        &[
            (
                "handler.rs",
                "fn helper() { super::renderer::emit(writer); }",
            ),
            ("command_context.rs", ""),
            (
                "renderer.rs",
                "pub(super) fn emit(writer: &mut dyn SchronuWriter) {}",
            ),
        ],
    );
    assert!(!violations(&modules).is_empty());
}

#[test]
fn context_rejects_fully_qualified_writer_operations() {
    for operation in [
        "std::io::Write::flush(&mut std::io::stdout())",
        "std::io::Write::write_all(&mut std::io::stdout(), b\"x\")",
        "super::renderer::SchronuWriter::writeln_newline(writer, \"x\")",
    ] {
        let source = format!("fn helper() {{ {operation}.unwrap(); }}");
        let modules = fixture_modules(
            "mod handler; mod command_context; mod renderer;",
            &[
                ("handler.rs", ""),
                ("command_context.rs", &source),
                ("renderer.rs", ""),
            ],
        );
        assert!(!violations(&modules).is_empty());
    }
}

#[test]
fn renderer_writer_aliases_cannot_hide_output_capabilities() {
    let modules = fixture_modules(
        "mod handler; mod command_context; mod renderer;",
        &[
            ("handler.rs", ""),
            (
                "command_context.rs",
                "fn helper() { super::renderer::renamed(writer); }",
            ),
            (
                "renderer.rs",
                "use std::io::Write as Sink; pub(super) fn renamed<W: Sink>(writer: W) {}",
            ),
        ],
    );
    assert!(!violations(&modules).is_empty());
}
