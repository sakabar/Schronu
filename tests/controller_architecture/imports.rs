use std::collections::{BTreeMap, BTreeSet};
use syn::ext::IdentExt;
use syn::visit::Visit;
use syn::UseTree;

pub fn expand_local_globs(
    module: &str,
    file: &syn::File,
    modules: &BTreeMap<String, syn::File>,
) -> Result<syn::File, String> {
    fn expand(
        tree: &mut UseTree,
        prefix: &str,
        module: &str,
        modules: &BTreeMap<String, syn::File>,
    ) -> Result<(), String> {
        match tree {
            UseTree::Path(path) => {
                let prefix = if prefix.is_empty() {
                    path.ident.unraw().to_string()
                } else {
                    format!("{prefix}::{}", path.ident.unraw())
                };
                expand(&mut path.tree, &prefix, module, modules)?;
            }
            UseTree::Group(group) => {
                for item in &mut group.items {
                    expand(item, prefix, module, modules)?;
                }
            }
            UseTree::Glob(_) => {
                let target = qualify(module, prefix);
                let file = modules
                    .get(&target)
                    .ok_or_else(|| format!("glob target is not a local module: {target}"))?;
                let mut items = syn::punctuated::Punctuated::new();
                for item in &file.items {
                    let (visibility, ident) = match item {
                        syn::Item::Fn(item) => (&item.vis, &item.sig.ident),
                        syn::Item::Struct(item) => (&item.vis, &item.ident),
                        syn::Item::Union(item) => (&item.vis, &item.ident),
                        syn::Item::Enum(item) => (&item.vis, &item.ident),
                        syn::Item::Trait(item) => (&item.vis, &item.ident),
                        syn::Item::Type(item) => (&item.vis, &item.ident),
                        syn::Item::Const(item) => (&item.vis, &item.ident),
                        syn::Item::Static(item) => (&item.vis, &item.ident),
                        syn::Item::Mod(item) => (&item.vis, &item.ident),
                        syn::Item::Use(item) if !matches!(item.vis, syn::Visibility::Inherited) => {
                            return Err(format!(
                                "glob through re-export needs explicit support: {target}"
                            ))
                        }
                        syn::Item::Macro(_) => {
                            return Err(format!(
                                "glob through macro needs explicit support: {target}"
                            ))
                        }
                        syn::Item::Use(_) | syn::Item::Impl(_) => continue,
                        _ => return Err(format!("unsupported glob export item: {target}")),
                    };
                    if !matches!(visibility, syn::Visibility::Inherited) {
                        items.push(UseTree::Name(syn::UseName {
                            ident: ident.clone(),
                        }));
                    }
                }
                *tree = UseTree::Group(syn::UseGroup {
                    brace_token: Default::default(),
                    items,
                });
            }
            _ => {}
        }
        Ok(())
    }
    let mut file = file.clone();
    for item in &mut file.items {
        if let syn::Item::Use(item) = item {
            expand(&mut item.tree, "", module, modules)?;
        }
    }
    // Nested globs remain unsupported, so they fail rather than lose edges.
    imports(module, &file)?;
    Ok(file)
}

pub fn imports(module: &str, file: &syn::File) -> Result<BTreeMap<String, String>, String> {
    let mut collector = Imports {
        module,
        bindings: BTreeMap::new(),
        errors: Vec::new(),
    };
    collector.visit_file(file);
    let original = collector.bindings.clone();
    for (alias, path) in &mut collector.bindings {
        let mut seen = BTreeSet::from([alias.clone()]);
        loop {
            let (first, suffix) = path.split_once("::").unwrap_or((path.as_str(), ""));
            let Some(target) = original.get(first) else {
                break;
            };
            if first == target {
                break;
            }
            if !seen.insert(first.to_string()) {
                collector
                    .errors
                    .push(format!("cyclic import alias: {alias}"));
                break;
            }
            *path = if suffix.is_empty() {
                target.clone()
            } else {
                format!("{target}::{suffix}")
            };
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
        prefix.extend(["crate", "adapter"]);
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
    if result == "crate::adapter::controller" {
        return "controller".into();
    }
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
            UseTree::Path(path) => self.tree(&append(&path.ident.unraw().to_string()), &path.tree),
            UseTree::Name(name) if name.ident == "self" => {
                self.bind(
                    prefix.rsplit("::").next().unwrap_or(prefix).into(),
                    prefix.into(),
                );
            }
            UseTree::Name(name) => self.bind(
                name.ident.unraw().to_string(),
                append(&name.ident.unraw().to_string()),
            ),
            UseTree::Rename(rename) => self.bind(
                rename.rename.unraw().to_string(),
                if rename.ident == "self" {
                    prefix.into()
                } else {
                    append(&rename.ident.unraw().to_string())
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
