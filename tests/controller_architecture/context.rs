use super::paths::{
    block_calls, input_has, output_dependencies, output_functions, output_has, references,
};
use super::source::{controller_modules, fixture_modules, module_family};
use std::collections::BTreeMap;

fn delegation_violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    let builders: Vec<_> = module_family(modules, "controller::view")
        .flat_map(|(module, file)| {
            file.items.iter().filter_map(move |item| match item {
                syn::Item::Fn(function)
                    if input_has(&function.sig, "TaskListDisplayOrder")
                        && output_has(&function.sig, "DisplayModel") =>
                {
                    Some(format!("{module}::{}", function.sig.ident))
                }
                _ => None,
            })
        })
        .collect();
    let [builder] = builders.as_slice() else {
        return vec!["one typed task-list view builder required".into()];
    };
    let mut errors = Vec::new();
    let mut calls = 0;
    for (module, file) in module_family(modules, "controller::command_context") {
        for item in &file.items {
            let syn::Item::Impl(implementation) = item else {
                continue;
            };
            let Some((_, trait_path, _)) = &implementation.trait_ else {
                continue;
            };
            let trait_name = match super::paths::resolve_path(module, file, trait_path) {
                Ok(path) => path,
                Err(error) => {
                    errors.push(error);
                    continue;
                }
            };
            if trait_name.rsplit("::").next() != Some("TaskTreeCommandContext")
                || !super::paths::type_has(&implementation.self_ty, "RuntimeTaskTreeCommandContext")
            {
                continue;
            }
            for item in &implementation.items {
                let syn::ImplItem::Fn(method) = item else {
                    continue;
                };
                if input_has(&method.sig, "TaskListOrder")
                    && output_has(&method.sig, "DisplayModel")
                {
                    match block_calls(module, file, &method.block) {
                        Ok(paths) => {
                            calls += paths.iter().filter(|(path, _)| path == builder).count()
                        }
                        Err(error) => errors.push(error),
                    }
                }
            }
        }
    }
    if calls != 1 {
        errors.push("task-list context must delegate directly to its view builder once".into());
    }
    errors
}

#[test]
fn task_list_context_cannot_bypass_its_view_builder() {
    let modules = fixture_modules("mod view; mod command_context;", &[
        ("view.rs", "fn renamed(order: TaskListDisplayOrder) -> DisplayModel { todo!() }"),
        ("command_context.rs", "impl TaskTreeCommandContext for RuntimeTaskTreeCommandContext { fn list(&mut self, order: TaskListOrder) -> DisplayModel { DisplayModel::empty() } }"),
    ]);
    assert!(!delegation_violations(&modules).is_empty());
}

#[test]
fn product_task_list_context_delegates_to_view() {
    assert_eq!(
        delegation_violations(&controller_modules()),
        Vec::<String>::new()
    );
}

#[test]
fn renamed_task_list_builder_preserves_delegation_without_decoys() {
    for (body, expected) in [
        ("build(order)", true),
        ("let unused = || build(order); DisplayModel::empty()", false),
        ("let note = \"build(order)\"; DisplayModel::empty()", false),
    ] {
        let modules = fixture_modules("mod view; mod command_context;", &[
            ("view.rs", "fn arbitrary_name(order: TaskListDisplayOrder) -> DisplayModel { todo!() }"),
            ("command_context.rs", &format!("use super::view::arbitrary_name as build; impl TaskTreeCommandContext for RuntimeTaskTreeCommandContext {{ fn arbitrary_method(&mut self, order: TaskListOrder) -> DisplayModel {{ {body} }} }}")),
        ]);
        assert_eq!(delegation_violations(&modules).is_empty(), expected);
    }
}

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

#[test]
fn unrelated_impl_cannot_supply_the_product_contexts_builder_call() {
    let modules = fixture_modules("mod view; mod command_context;", &[
        ("view.rs", "fn renamed(order: TaskListDisplayOrder) -> DisplayModel { todo!() }"),
        ("command_context.rs", "use super::view::renamed; impl TaskTreeCommandContext for RuntimeTaskTreeCommandContext { fn list(&mut self, order: TaskListOrder) -> DisplayModel { DisplayModel::empty() } } impl Helper { fn decoy(&mut self, order: TaskListOrder) -> DisplayModel { renamed(order) } }"),
    ]);
    assert!(!delegation_violations(&modules).is_empty());
}
