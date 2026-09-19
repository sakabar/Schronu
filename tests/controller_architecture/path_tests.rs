use super::paths::imports;
use super::source::product_file;
use std::collections::BTreeMap;

#[test]
fn grouped_and_renamed_imports_preserve_dependency_identity() {
    let file = product_file(
        r#"
        use super::{runtime as driver, renderer::{self, DisplayModel as Model}};
        use std::io::{self, Write as Sink};
        fn harmless() { let text = "use super::runtime"; }
    "#,
    )
    .unwrap();
    assert_eq!(
        imports("controller::handler", &file).unwrap(),
        BTreeMap::from([
            ("driver".into(), "controller::runtime".into()),
            ("renderer".into(), "controller::renderer".into()),
            ("Model".into(), "controller::renderer::DisplayModel".into()),
            ("io".into(), "std::io".into()),
            ("Sink".into(), "std::io::Write".into()),
        ])
    );
}

#[test]
fn unhandled_globs_and_conflicting_aliases_fail_explicitly() {
    for text in [
        "use super::runtime::*;",
        "use super::runtime as x; fn f() { use super::renderer as x; }",
    ] {
        assert!(imports("controller::handler", &product_file(text).unwrap()).is_err());
    }
}

#[test]
fn qualified_imports_and_nested_modules_do_not_change_identity() {
    let file = product_file("use crate::adapter::controller::runtime as outer; mod nested { use super::renderer as outer; }").unwrap();
    assert_eq!(
        imports("controller::handler", &file).unwrap()["outer"],
        "controller::runtime"
    );
}

#[test]
fn alias_mediated_imports_are_not_returned_as_unresolved_paths() {
    for text in [
        "use super::runtime as driver; use driver::Transaction as T;",
        "use driver::Transaction as T; use super::runtime as driver;",
    ] {
        assert!(imports("controller::handler", &product_file(text).unwrap()).is_err());
    }
}

#[test]
fn expression_references_inside_macros_keep_their_module_identity() {
    for body in [
        "super::runtime::invoke();",
        "format!(\"{}\", super::runtime::invoke());",
        "vec![super::runtime::invoke(); 2];",
        "matches!(super::runtime::invoke(), Some(_));",
    ] {
        let file = product_file(&format!("fn renamed() {{ {body} }}")).unwrap();
        let paths = super::paths::references("controller::handler", &file).unwrap();
        assert!(
            paths.contains("controller::runtime::invoke"),
            "missing dependency in {body}"
        );
    }
}

#[test]
fn references_resolve_aliases_but_ignore_literal_and_comment_decoys() {
    let file = product_file(
        r###"
        use super::runtime as rt;
        fn renamed() {
            // super::renderer::fake();
            let raw = r#"super::renderer::fake();"#;
            rt::invoke();
        }
    "###,
    )
    .unwrap();
    let paths = super::paths::references("controller::handler", &file).unwrap();
    assert!(paths.contains("controller::runtime::invoke"));
    assert!(!paths.iter().any(|path| path.contains("renderer")));
}

#[test]
fn unknown_macros_are_errors_instead_of_invisible_dependencies() {
    let file = product_file("fn f() { custom!(super::runtime::invoke()); }").unwrap();
    assert!(super::paths::references("controller::handler", &file)
        .unwrap_err()
        .contains("unhandled macro"));
}

#[test]
fn macro_local_imports_cannot_hide_dependencies() {
    let file =
        product_file("fn f() { format!(\"{}\", { use super::runtime as rt; rt::invoke() }); }")
            .unwrap();
    assert!(super::paths::references("controller::handler", &file).is_err());
}

#[test]
fn raw_import_targets_and_aliases_have_the_same_identity() {
    for text in [
        "use super::r#runtime as rt; fn f() { rt::invoke(); }",
        "use super::runtime as r#type; fn f() { r#type::invoke(); }",
    ] {
        let file = product_file(text).unwrap();
        assert!(super::paths::references("controller::handler", &file)
            .unwrap()
            .contains("controller::runtime::invoke"));
    }
}
