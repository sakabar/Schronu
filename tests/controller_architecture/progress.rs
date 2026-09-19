use super::paths::{function_calls, function_has_scaling, output_has, type_has};
use super::source::product_file;

fn violations(file: &syn::File) -> Vec<String> {
    let formatters: Vec<_> = file.items.iter().filter_map(|item| match item {
        syn::Item::Fn(function) if function.sig.inputs.len() == 3 && output_has(&function.sig, "String") && function.sig.inputs.iter().all(|input| matches!(input, syn::FnArg::Typed(input) if type_has(&input.ty, "i64"))) => Some(function),
        _ => None,
    }).collect();
    let [formatter] = formatters.as_slice() else {
        return vec!["one three-duration progress formatter required".into()];
    };
    let calls = match function_calls("controller::renderer", file, formatter) {
        Ok(calls) => calls,
        Err(error) => return vec![error],
    };
    let mut errors = Vec::new();
    if calls
        .iter()
        .filter(|(path, _)| path.starts_with("crate::application::session_progress::"))
        .count()
        != 1
    {
        errors.push("progress formatter must delegate once to application session progress".into());
    }
    match function_has_scaling("controller::renderer", file, formatter) {
        Ok(true) => errors.push("progress formatter owns scaling arithmetic".into()),
        Err(error) => errors.push(error),
        Ok(false) => {}
    }
    errors
}

#[test]
fn progress_formatter_cannot_own_percentage_arithmetic() {
    let file = product_file("fn renamed(estimate: i64, actual: i64, focused: i64) -> String { ((actual + focused) * 100 / estimate).to_string() }").unwrap();
    assert!(!violations(&file).is_empty());
}

#[test]
fn product_progress_formatter_delegates_calculation() {
    let modules = super::source::controller_modules();
    assert_eq!(
        violations(&modules["controller::renderer"]),
        Vec::<String>::new()
    );
}

#[test]
fn renamed_formatter_and_aliased_calculator_keep_the_contract() {
    let file = product_file(
        r#"use crate::application::session_progress::calculate_session_progress as compute;
        fn arbitrary_name(estimate: i64, actual: i64, focused: i64) -> String {
            let note = "calculate_session_progress and * 100";
            let value = compute(estimate, actual, focused); format!("{value:?} {note}")
        }"#,
    )
    .unwrap();
    assert!(violations(&file).is_empty());
}

#[test]
fn progress_calculator_decoys_do_not_satisfy_delegation() {
    for body in [
        "let unused = || crate::application::session_progress::calculate_session_progress(a,b,c); String::new()",
        "let text = \"calculate_session_progress(a,b,c)\"; text.into()",
        "object.calculate_session_progress(a,b,c).to_string()",
    ] {
        let file = product_file(&format!("fn renamed(a: i64, b: i64, c: i64) -> String {{ {body} }}")).unwrap();
        assert!(!violations(&file).is_empty(), "{body}");
    }
}

#[test]
fn progress_arithmetic_inside_format_arguments_is_rejected() {
    let file = product_file(
        r#"fn renamed(a: i64, b: i64, c: i64) -> String {
        let _ = crate::application::session_progress::calculate_session_progress(a, b, c);
        format!("{}", (b + c) * 100 / a)
    }"#,
    )
    .unwrap();
    assert_eq!(
        violations(&file),
        ["progress formatter owns scaling arithmetic"]
    );
}
