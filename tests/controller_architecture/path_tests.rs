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
