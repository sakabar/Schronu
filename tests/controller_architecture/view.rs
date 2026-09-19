use super::source::{controller_modules, fixture_modules};
use std::collections::BTreeMap;

fn violations(_modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    Vec::new()
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
