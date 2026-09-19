use super::source::controller_modules;
use std::collections::BTreeMap;

fn violations(_modules: &BTreeMap<String, syn::File>) -> Vec<String> { Vec::new() }

#[test]
fn runtime_cannot_own_terminal_cursor_operations() {
    let mut modules = controller_modules();
    modules.get_mut("controller::runtime").unwrap().items.push(syn::parse_quote! {
        fn arbitrary_name() { let _ = termion::cursor::Goto(1, 2); }
    });
    assert!(!violations(&modules).is_empty());
}
