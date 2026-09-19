use super::source::fixture_modules;
use std::collections::BTreeMap;

fn legacy_violations(_modules: &BTreeMap<String, syn::File>) -> Vec<String> { Vec::new() }

#[test]
fn semantic_display_cannot_restore_a_legacy_variant() {
    let modules = fixture_modules("mod renderer;", &[("renderer.rs", "enum DisplayModel { Legacy { fragments: Vec<u8> } }")]);
    assert!(!legacy_violations(&modules).is_empty());
}
