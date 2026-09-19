use super::source::product_file;

#[test]
fn product_items_exclude_test_configuration_without_scanning_literals() {
    let file = product_file(
        r####"
        // #[cfg(test)] fn not_an_item() {}
        const NOTE: &str = r###"#[cfg(test)] fn not_an_item() {}"###;
        #[cfg(test)] fn test_only() {}
        #[cfg(all(unix, test))] mod tests { fn hidden() {} }
        #[cfg(not(test))] fn product() {}
        #[cfg(any(test, unix))] fn possible_product() {}
        impl Example {
            #[cfg(test)] fn test_method() {}
            fn product_method() {}
        }
        "####,
    )
    .unwrap();
    assert_eq!(file.items.len(), 4, "test-only items must be excluded");
    let syn::Item::Impl(implementation) = &file.items[3] else {
        panic!("last product item must be the implementation");
    };
    assert_eq!(implementation.items.len(), 1);
}

#[test]
fn invalid_rust_is_an_error_instead_of_an_empty_product_file() {
    assert!(product_file("fn broken(").is_err());
}

#[test]
fn inner_file_configuration_excludes_the_entire_test_file() {
    assert!(product_file("#![cfg(test)] fn helper() {}")
        .unwrap()
        .items
        .is_empty());
    assert_eq!(
        product_file("#![cfg(not(test))] fn product() {}")
            .unwrap()
            .items
            .len(),
        1
    );
}
