use super::paths::{definitions, expand_local_globs, references};
use super::source::{controller_modules, fixture_modules};
use std::collections::BTreeMap;
use syn::ext::IdentExt;
use syn::visit::{self, Visit};

fn legacy_violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    struct Legacy(Vec<String>);
    impl<'ast> Visit<'ast> for Legacy {
        fn visit_ident(&mut self, ident: &'ast syn::Ident) {
            if matches!(
                ident.unraw().to_string().as_str(),
                "DisplayRecorder" | "DisplayFragment"
            ) {
                self.0.push(format!("legacy display identifier: {ident}"));
            }
        }
        fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
            if item.ident.unraw() == "DisplayModel"
                && item.variants.iter().any(|v| v.ident.unraw() == "Legacy")
            {
                self.0.push("legacy DisplayModel variant".into());
            }
            visit::visit_item_enum(self, item);
        }
        fn visit_item_mod(&mut self, _item: &'ast syn::ItemMod) {}
    }
    let mut legacy = Legacy(Vec::new());
    for (module, file) in modules {
        legacy.visit_file(file);
        let paths =
            expand_local_globs(module, file, modules).and_then(|file| references(module, &file));
        match paths {
            Ok(paths) => {
                for path in paths {
                    if path.ends_with("DisplayModel::Legacy")
                        || path
                            .split("::")
                            .any(|part| matches!(part, "DisplayRecorder" | "DisplayFragment"))
                    {
                        legacy.0.push(format!("legacy display dependency: {path}"));
                    }
                }
            }
            Err(error) => legacy.0.push(error),
        }
    }
    legacy.0
}

#[test]
fn semantic_display_cannot_restore_a_legacy_variant() {
    let modules = fixture_modules(
        "mod renderer;",
        &[(
            "renderer.rs",
            "enum DisplayModel { Legacy { fragments: Vec<u8> } }",
        )],
    );
    assert!(!legacy_violations(&modules).is_empty());
}

#[test]
fn all_product_controller_modules_use_semantic_display() {
    assert_eq!(
        legacy_violations(&controller_modules()),
        Vec::<String>::new()
    );
}

#[test]
fn nested_and_aliased_legacy_display_dependencies_are_rejected() {
    for source in [
        "mod helper { struct DisplayRecorder; }",
        "use super::renderer::DisplayModel as Model; fn renamed() { let _ = Model::Legacy; }",
        "fn renamed() { format!(\"{}\", super::renderer::DisplayRecorder::new()); }",
    ] {
        let modules = fixture_modules(
            "mod runtime; mod renderer;",
            &[
                ("runtime.rs", source),
                ("renderer.rs", "enum DisplayModel { Message }"),
            ],
        );
        assert!(!legacy_violations(&modules).is_empty(), "{source}");
    }
}

#[test]
fn semantic_display_ignores_comments_literals_and_test_only_items() {
    let modules = fixture_modules(
        "mod renderer;",
        &[(
            "renderer.rs",
            r#"
        enum DisplayModel { Message }
        // DisplayRecorder::new();
        const NOTE: &str = "DisplayFragment DisplayModel::Legacy";
        #[cfg(test)] struct DisplayRecorder;
    "#,
        )],
    );
    assert!(legacy_violations(&modules).is_empty());
}

#[test]
fn raw_legacy_variant_declarations_have_the_same_identity() {
    let modules = fixture_modules(
        "mod renderer;",
        &[("renderer.rs", "enum r#DisplayModel { r#Legacy }")],
    );
    assert!(!legacy_violations(&modules).is_empty());
}

fn mode_violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    let mut owners = Vec::new();
    for (module, file) in modules {
        let items = match expand_local_globs(module, file, modules)
            .and_then(|file| definitions(module, &file))
        {
            Ok(items) => items,
            Err(error) => return vec![error],
        };
        for item in items {
            if matches!(item, syn::Item::Enum(item) if item.ident.unraw() == "RenderMode") {
                owners.push(module.as_str());
            }
        }
    }
    match owners.as_slice() {
        [owner]
            if *owner == "controller::renderer" || owner.starts_with("controller::renderer::") =>
        {
            Vec::new()
        }
        _ => vec!["RenderMode must have one declaration in renderer".into()],
    }
}

#[test]
fn product_render_mode_belongs_to_renderer() {
    assert_eq!(mode_violations(&controller_modules()), Vec::<String>::new());
}

#[test]
fn render_mode_owner_cannot_move_or_duplicate() {
    for (renderer, runtime) in [
        ("", "enum RenderMode { Flushed, Unflushed }"),
        (
            "enum RenderMode { Flushed }",
            "enum RenderMode { Unflushed }",
        ),
        (
            "enum RenderMode { Flushed }",
            "fn helper() { enum RenderMode { Flushed } }",
        ),
        (
            "enum RenderMode { Flushed }",
            "fn helper() { format!(\"{}\", { enum RenderMode { Flushed } 0 }); }",
        ),
        ("", ""),
    ] {
        let modules = fixture_modules(
            "mod renderer; mod runtime;",
            &[("renderer.rs", renderer), ("runtime.rs", runtime)],
        );
        assert!(!mode_violations(&modules).is_empty());
    }
}

#[test]
fn render_mode_can_belong_to_a_renderer_child() {
    let modules = fixture_modules(
        "mod renderer;",
        &[(
            "renderer.rs",
            "mod mode { enum RenderMode { Flushed, Unflushed } }",
        )],
    );
    assert!(mode_violations(&modules).is_empty());
}
