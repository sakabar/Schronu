use super::source::controller_modules;
use std::collections::BTreeMap;

fn io_violations(_modules: &BTreeMap<String, syn::File>) -> Vec<String> { Vec::new() }

#[test]
fn renderer_cannot_start_an_external_process() {
    let mut modules = controller_modules();
    modules.get_mut("controller::renderer").unwrap().items.push(syn::parse_quote! {
        fn renamed() { std::process::Command::new("external"); }
    });
    assert!(!io_violations(&modules).is_empty());
}
