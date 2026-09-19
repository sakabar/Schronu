use std::collections::BTreeMap;
use syn::visit::Visit;
use syn::UseTree;

pub fn imports(module: &str, file: &syn::File) -> Result<BTreeMap<String, String>, String> {
    let mut collector = Imports {
        module,
        bindings: BTreeMap::new(),
        errors: Vec::new(),
    };
    collector.visit_file(file);
    for (alias, path) in &collector.bindings {
        let first = path.split("::").next().unwrap_or(path);
        if collector.bindings.contains_key(first) && first != path {
            collector.errors.push(format!(
                "alias-mediated import needs explicit support: {alias} = {path}"
            ));
        }
    }
    if collector.errors.is_empty() {
        Ok(collector.bindings)
    } else {
        Err(collector.errors.join("\n"))
    }
}

pub fn qualify(module: &str, path: &str) -> String {
    let mut parts = path.split("::").peekable();
    let mut prefix = Vec::new();
    if matches!(parts.peek(), Some(&"self" | &"super")) {
        prefix.extend(module.split("::"));
        while let Some(part) = parts.peek() {
            match *part {
                "self" => {
                    parts.next();
                }
                "super" => {
                    prefix.pop();
                    parts.next();
                }
                _ => break,
            }
        }
    }
    prefix.extend(parts);
    let result = prefix.join("::");
    if let Some(rest) = result.strip_prefix("crate::adapter::controller::") {
        format!("controller::{rest}")
    } else {
        result
    }
}

struct Imports<'a> {
    module: &'a str,
    bindings: BTreeMap<String, String>,
    errors: Vec<String>,
}

impl Imports<'_> {
    fn bind(&mut self, alias: String, path: String) {
        let path = qualify(self.module, &path);
        if let Some(previous) = self.bindings.insert(alias.clone(), path.clone()) {
            if previous != path {
                self.errors
                    .push(format!("ambiguous import {alias}: {previous}, {path}"));
            }
        }
    }

    fn tree(&mut self, prefix: &str, tree: &UseTree) {
        let append = |name: &str| {
            if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}::{name}")
            }
        };
        match tree {
            UseTree::Path(path) => self.tree(&append(&path.ident.to_string()), &path.tree),
            UseTree::Name(name) if name.ident == "self" => {
                self.bind(
                    prefix.rsplit("::").next().unwrap_or(prefix).into(),
                    prefix.into(),
                );
            }
            UseTree::Name(name) => {
                self.bind(name.ident.to_string(), append(&name.ident.to_string()))
            }
            UseTree::Rename(rename) => self.bind(
                rename.rename.to_string(),
                if rename.ident == "self" {
                    prefix.into()
                } else {
                    append(&rename.ident.to_string())
                },
            ),
            UseTree::Group(group) => {
                for item in &group.items {
                    self.tree(prefix, item);
                }
            }
            UseTree::Glob(_) => self
                .errors
                .push(format!("glob import needs explicit expansion: {prefix}")),
        }
    }
}

impl<'ast> Visit<'ast> for Imports<'_> {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.tree("", &item.tree);
    }
    // Inline modules have separate entries in the product module index.
    fn visit_item_mod(&mut self, _item: &'ast syn::ItemMod) {}
}
