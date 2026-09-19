use super::source::product_file;

fn violations(_file: &syn::File) -> Vec<String> {
    Vec::new()
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
    entry.block = Box::new(syn::parse_str("{ Ok(Command::Noop) }").unwrap());
    assert_eq!(
        violations(&file),
        ["parser entry must call the typed core once: line"]
    );
}
