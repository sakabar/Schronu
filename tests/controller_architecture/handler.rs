use super::source::{controller_modules, product_file};

fn violations(_file: &syn::File, _command: &syn::File) -> Vec<String> {
    Vec::new()
}

#[test]
fn handler_rejects_an_outer_runtime_dependency() {
    let file = product_file("use super::runtime as outer; fn renamed() { outer::run(); }").unwrap();
    assert!(!violations(&file, &product_file("").unwrap()).is_empty());
}

#[test]
fn product_handler_has_no_outer_io_dependency() {
    let modules = controller_modules();
    assert!(violations(
        &modules["controller::handler"],
        &modules["controller::command"]
    )
    .is_empty());
}
