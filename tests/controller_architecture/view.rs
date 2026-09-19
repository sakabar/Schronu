use super::paths::{
    definitions, expand_local_globs, output_dependencies, output_functions, references,
    resolve_path, type_has, used_paths,
};
use super::source::{controller_modules, fixture_modules, module_family};
use std::collections::BTreeMap;
use syn::ext::IdentExt;

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
    for (module, file) in module_family(modules, "controller::runtime") {
        match expand_local_globs(module, file, modules).and_then(|file| used_paths(module, &file)) {
            Ok(paths) => errors.extend(
                paths
                    .into_iter()
                    .filter(|path| {
                        path.split("::").any(|part| {
                            [
                                "TreeDisplay",
                                "TaskListDisplay",
                                "TaskListMetricsDisplay",
                                "CalendarDisplay",
                                "BandDisplay",
                                "PackDisplay",
                                "FlattenDisplay",
                                "FocusDisplay",
                                "SnapshotDisplay",
                                "TaskListTaskRow",
                                "TaskListDisplayRow",
                                "TaskCategoryWorkSeconds",
                                "BandDurations",
                                "CalendarDayRow",
                                "BandDayRow",
                                "RhoMetrics",
                                "ProjectCategory",
                            ]
                            .contains(&part)
                        })
                    })
                    .map(|path| format!("runtime depends on concrete view data: {path}")),
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

fn focus_ownership_violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    let mut owners: [Vec<String>; 3] = Default::default();
    let mut errors = Vec::new();
    for (module, file) in modules {
        let expanded = match expand_local_globs(module, file, modules) {
            Ok(file) => file,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let items = match definitions(module, &expanded) {
            Ok(items) => items,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        for item in items {
            let role = match item {
                syn::Item::Trait(item) if item.ident.unraw() == "FocusDisplaySource" => Some(0),
                syn::Item::Struct(item) if item.ident.unraw() == "TaskFocusDisplaySource" => {
                    Some(1)
                }
                syn::Item::Impl(item) if type_has(&item.self_ty, "TaskFocusDisplaySource") => {
                    match item
                        .trait_
                        .map(|(_, path, _)| resolve_path(module, &expanded, &path))
                        .transpose()
                    {
                        Ok(Some(path))
                            if path.ends_with("::FocusDisplaySource")
                                || path == "FocusDisplaySource" =>
                        {
                            Some(2)
                        }
                        Err(error) => {
                            errors.push(error);
                            None
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(role) = role {
                owners[role].push(module.clone());
            }
        }
    }
    for (role, owners) in ["trait", "source", "implementation"]
        .into_iter()
        .zip(owners)
    {
        if owners.len() != 1
            || !owners
                .iter()
                .all(|owner| owner == "controller::view" || owner.starts_with("controller::view::"))
        {
            errors.push(format!(
                "focus {role} must belong exactly once to view: {owners:?}"
            ));
        }
    }
    errors
}

#[test]
fn focus_display_source_cannot_move_into_runtime() {
    let mut modules = controller_modules();
    let view = modules.get_mut("controller::view").unwrap();
    let index = view
        .items
        .iter()
        .position(
            |item| matches!(item, syn::Item::Trait(item) if item.ident == "FocusDisplaySource"),
        )
        .unwrap();
    let declaration = view.items.remove(index);
    modules
        .get_mut("controller::runtime")
        .unwrap()
        .items
        .push(declaration);
    assert!(!focus_ownership_violations(&modules).is_empty());
}

#[test]
fn product_focus_source_is_owned_by_view() {
    assert_eq!(
        focus_ownership_violations(&controller_modules()),
        Vec::<String>::new()
    );
}

#[test]
fn focus_source_ownership_rejects_nested_and_macro_duplicates() {
    for source in [
        "struct TaskFocusDisplaySource;",
        "fn renamed() { format!(\"{}\", { struct TaskFocusDisplaySource; 0 }); }",
        "use super::view::FocusDisplaySource as Source; impl Source for TaskFocusDisplaySource {}",
    ] {
        let mut modules = controller_modules();
        modules.insert(
            "controller::runtime::helper".into(),
            super::source::product_file(source).unwrap(),
        );
        assert!(!focus_ownership_violations(&modules).is_empty(), "{source}");
    }
}

#[test]
fn runtime_cannot_depend_on_concrete_view_data() {
    for body in [
        "use super::super::renderer::TreeDisplay as Data; fn f(value: Data) {}",
        "fn f() { format!(\"{:?}\", { let model: Option<super::super::renderer::TaskListDisplay> = None; model }); }",
        "type Data = super::super::renderer::TaskListMetricsDisplay;",
    ] {
        let mut modules = controller_modules();
        modules.insert("controller::runtime::helper".into(), super::source::product_file(body).unwrap());
        assert!(!violations(&modules).is_empty(), "{body}");
    }
}
