pub fn product_file(text: &str) -> syn::Result<syn::File> {
    syn::parse_file(text)
}
