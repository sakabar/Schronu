use super::source::product_file;

fn violations(_file: &syn::File) -> Vec<String> { Vec::new() }

#[test]
fn progress_formatter_cannot_own_percentage_arithmetic() {
    let file = product_file("fn renamed(estimate: i64, actual: i64, focused: i64) -> String { ((actual + focused) * 100 / estimate).to_string() }").unwrap();
    assert!(!violations(&file).is_empty());
}
