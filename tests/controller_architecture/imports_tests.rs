use super::imports::imports;
use super::source::product_file;
use std::collections::BTreeMap;

#[test]
fn local_globs_resolve_only_declared_visible_items() {
    let modules = super::source::fixture_modules(
        "mod runtime; mod view;",
        &[
            ("runtime.rs", "use super::view::*; fn run() { renamed(); }"),
            ("view.rs", "pub(super) fn renamed() {} fn private() {}"),
        ],
    );
    let file = super::imports::expand_local_globs(
        "controller::runtime",
        &modules["controller::runtime"],
        &modules,
    )
    .unwrap();
    let paths = super::paths::references("controller::runtime", &file).unwrap();
    assert!(paths.contains("controller::view::renamed"));
    assert!(!paths.contains("controller::view::private"));
}

#[test]
fn local_globs_preserve_unions_and_reject_unhandled_exports() {
    for (source, supported) in [
        ("pub union Payload { pub value: u32 }", true),
        ("unsafe extern \"C\" { pub fn imported(); }", false),
    ] {
        let modules = super::source::fixture_modules(
            "mod runtime; mod view;",
            &[("runtime.rs", "use super::view::*;"), ("view.rs", source)],
        );
        let result = super::imports::expand_local_globs(
            "controller::runtime",
            &modules["controller::runtime"],
            &modules,
        );
        if supported {
            assert!(
                super::paths::references("controller::runtime", &result.unwrap())
                    .unwrap()
                    .contains("controller::view::Payload")
            );
        } else {
            assert!(result.is_err());
        }
    }
}

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
        assert_eq!(
            imports("controller::handler", &product_file(text).unwrap()).unwrap()["T"],
            "controller::runtime::Transaction"
        );
    }
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

#[test]
fn cyclic_import_aliases_fail_explicitly() {
    let file = product_file("use b::item as a; use a::item as b;").unwrap();
    assert!(imports("controller::runtime", &file)
        .unwrap_err()
        .contains("cyclic"));
}

#[test]
fn unresolved_glob_targets_remain_errors() {
    let modules =
        super::source::fixture_modules("mod runtime;", &[("runtime.rs", "use super::missing::*;")]);
    assert!(super::imports::expand_local_globs(
        "controller::runtime",
        &modules["controller::runtime"],
        &modules
    )
    .is_err());
}

#[test]
fn relative_paths_keep_the_known_parents_of_controller() {
    for (path, expected) in [
        ("super::super::gateway", "crate::adapter::gateway"),
        ("super::super::super::application", "crate::application"),
        ("crate::adapter::controller", "controller"),
    ] {
        assert_eq!(
            super::imports::qualify("controller::renderer", path),
            expected
        );
    }
}
