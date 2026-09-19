use super::paths::{function_calls, input_has, output_has};
use super::source::product_file;

fn violations(file: &syn::File) -> Vec<String> {
    let functions: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(function)
                if !matches!(function.vis, syn::Visibility::Inherited)
                    && output_has(&function.sig, "Command") =>
            {
                Some(function)
            }
            _ => None,
        })
        .collect();
    let cores: Vec<_> = functions
        .iter()
        .filter(|function| {
            input_has(&function.sig, "ParseMode") && input_has(&function.sig, "String")
        })
        .collect();
    let [core] = cores.as_slice() else {
        return vec!["parser needs one typed token core".into()];
    };
    let entries: Vec<_> = functions
        .iter()
        .filter(|function| !input_has(&function.sig, "ParseMode"))
        .collect();
    let mut errors = Vec::new();
    if entries.len() != 3 {
        errors.push("parser needs text, argv, and maintenance entries".into());
    }
    for entry in entries {
        let calls = match function_calls("controller::command", file, entry) {
            Ok(calls) => calls,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let core_calls: Vec<_> = calls
            .iter()
            .filter(|(path, _)| {
                path == &core.sig.ident.to_string()
                    || path == &format!("controller::command::{}", core.sig.ident)
            })
            .collect();
        if core_calls.len() != 1 {
            errors.push(format!(
                "parser entry must call the typed core once: {}",
                entry.sig.ident
            ));
            continue;
        }
        let expected_mode = if input_has(&entry.sig, "str") {
            "Interactive"
        } else {
            "NonInteractive"
        };
        let mode = core_calls[0].1.args.iter().nth(1);
        if !matches!(mode, Some(syn::Expr::Path(path)) if path.path.segments.last().is_some_and(|part| part.ident == expected_mode))
        {
            errors.push(format!("parser entry has wrong mode: {}", entry.sig.ident));
        }
    }
    errors
}

fn fixture() -> syn::File {
    product_file(r#"
        pub(super) fn decode(tokens: &[String], mode: ParseMode) -> Result<Command, Error> { todo!() }
        pub(super) fn line(input: &str) -> Result<Command, Error> { decode(&tokenize(input), ParseMode::Interactive) }
        pub(super) fn arguments(input: &[String]) -> Result<Command, Error> { decode(input, ParseMode::NonInteractive) }
        pub(super) fn maintenance(input: &str) -> (Result<Command, Error>, Option<CommandKind>) { (decode(&tokenize(input), ParseMode::Interactive), None) }
    "#).unwrap()
}

#[test]
fn parser_rejects_an_entry_that_bypasses_the_shared_typed_core() {
    let mut file = fixture();
    let syn::Item::Fn(entry) = &mut file.items[1] else {
        panic!("fixture entry")
    };
    *entry.block = syn::parse_str("{ Ok(Command::Noop) }").unwrap();
    assert_eq!(
        violations(&file),
        ["parser entry must call the typed core once: line"]
    );
}

#[test]
fn product_parser_routes_all_three_entries_through_the_typed_core() {
    let modules = super::source::controller_modules();
    assert_eq!(
        violations(&modules["controller::command"]),
        Vec::<String>::new()
    );
}

#[test]
fn parser_function_renames_preserve_the_contract() {
    use syn::visit_mut::VisitMut;
    struct Rename;
    impl VisitMut for Rename {
        fn visit_ident_mut(&mut self, ident: &mut syn::Ident) {
            if matches!(
                ident.to_string().as_str(),
                "decode" | "line" | "arguments" | "maintenance"
            ) {
                *ident = syn::Ident::new(&format!("renamed_{ident}"), ident.span());
            }
        }
    }
    let mut file = fixture();
    assert!(violations(&file).is_empty());
    Rename.visit_file_mut(&mut file);
    assert!(violations(&file).is_empty());
}

#[test]
fn parser_rejects_method_and_literal_decoys_and_wrong_mode() {
    for body in [
        "{ object.decode(input, ParseMode::Interactive) }",
        "{ let text = \"decode(input, ParseMode::Interactive)\"; Ok(Command::Noop) }",
        "{ decode(input, ParseMode::NonInteractive) }",
    ] {
        let mut file = fixture();
        let syn::Item::Fn(entry) = &mut file.items[1] else {
            panic!("fixture entry")
        };
        *entry.block = syn::parse_str(body).unwrap();
        assert_eq!(violations(&file).len(), 1);
    }
}

#[test]
fn parser_rejects_calls_hidden_in_unexecuted_scopes() {
    for body in [
        "{ fn unused(input: &str) -> Result<Command, Error> { decode(&tokenize(input), ParseMode::Interactive) } Ok(Command::Noop) }",
        "{ let unused = || decode(&tokenize(input), ParseMode::Interactive); Ok(Command::Noop) }",
    ] {
        let mut file = fixture();
        let syn::Item::Fn(entry) = &mut file.items[1] else { panic!("fixture entry") };
        *entry.block = syn::parse_str(body).unwrap();
        assert_eq!(violations(&file).len(), 1);
    }
}
