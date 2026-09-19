use std::collections::BTreeMap;

pub fn imports(_module: &str, _file: &syn::File) -> Result<BTreeMap<String, String>, String> {
    Ok(BTreeMap::new())
}
