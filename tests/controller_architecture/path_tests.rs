use super::source::product_file;

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
fn block_local_modules_cannot_escape_the_module_index() {
    for source in [
        "fn f() { mod hidden { fn g() { std::fs::write(\"x\", \"y\"); } } }",
        "fn f() { format!(\"{:?}\", { mod hidden { fn g() { std::fs::write(\"x\", \"y\"); } } 1 }); }",
    ] {
        let file = product_file(source).unwrap();
        assert!(super::paths::references("controller::handler", &file).is_err());
    }
}
