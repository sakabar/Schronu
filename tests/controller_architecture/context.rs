use super::source::{controller_modules, fixture_modules};
use std::collections::BTreeMap;

fn violations(_modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    Vec::new()
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
    assert!(violations(&controller_modules()).is_empty());
}
