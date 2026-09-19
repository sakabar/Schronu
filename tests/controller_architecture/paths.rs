use std::collections::{BTreeMap, BTreeSet};
use syn::ext::IdentExt;
use syn::parse::Parser;
use syn::visit::{self, Visit};
use syn::UseTree;

pub fn references(module: &str, file: &syn::File) -> Result<BTreeSet<String>, String> {
    let bindings = imports(module, file)?;
    let mut collector = References {
        module,
        paths: bindings.values().cloned().collect(),
        bindings,
        errors: Vec::new(),
        in_macro: false,
    };
    collector.visit_file(file);
    if collector.errors.is_empty() {
        Ok(collector.paths)
    } else {
        Err(collector.errors.join("\n"))
    }
}

struct References<'a> {
    module: &'a str,
    bindings: BTreeMap<String, String>,
    paths: BTreeSet<String>,
    errors: Vec<String>,
    in_macro: bool,
}

impl References<'_> {
    fn path(&self, path: &syn::Path) -> String {
        let parts: Vec<_> = path
            .segments
            .iter()
            .map(|segment| segment.ident.unraw().to_string())
            .collect();
        let raw = parts.join("::");
        if let Some(target) = parts.first().and_then(|first| self.bindings.get(first)) {
            if parts.len() == 1 {
                target.clone()
            } else {
                format!("{target}::{}", parts[1..].join("::"))
            }
        } else {
            qualify(self.module, &raw)
        }
    }
}

impl<'ast> Visit<'ast> for References<'_> {
    fn visit_item_use(&mut self, _item: &'ast syn::ItemUse) {
        if self.in_macro {
            self.errors
                .push("macro-local import needs explicit support".into());
        }
    }
    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.paths.insert(self.path(path));
        visit::visit_path(self, path);
    }

    fn visit_item_mod(&mut self, _item: &'ast syn::ItemMod) {}

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        let name = self.path(&invocation.path);
        self.paths.insert(name.clone());
        let builtin = name.strip_prefix("std::").unwrap_or(&name);
        if !matches!(
            builtin,
            "format"
                | "format_args"
                | "vec"
                | "matches"
                | "write"
                | "writeln"
                | "print"
                | "println"
                | "eprint"
                | "eprintln"
                | "panic"
                | "unreachable"
                | "assert"
                | "assert_eq"
                | "assert_ne"
                | "debug_assert"
                | "debug_assert_eq"
                | "debug_assert_ne"
                | "todo"
                | "unimplemented"
                | "include_str"
                | "include_bytes"
                | "env"
                | "option_env"
        ) {
            self.errors.push(format!("unhandled macro: {name}"));
            return;
        }
        let parser = |input: syn::parse::ParseStream<'_>| {
            let mut expressions = Vec::new();
            let mut pattern = None;
            if builtin == "matches" {
                expressions.push(input.parse::<syn::Expr>()?);
                input.parse::<syn::Token![,]>()?;
                pattern = Some(input.call(syn::Pat::parse_multi)?);
                if input.peek(syn::Token![if]) {
                    input.parse::<syn::Token![if]>()?;
                    expressions.push(input.parse()?);
                }
                if input.peek(syn::Token![,]) {
                    input.parse::<syn::Token![,]>()?;
                }
            } else {
                while !input.is_empty() {
                    expressions.push(input.parse::<syn::Expr>()?);
                    if input.is_empty() {
                        break;
                    }
                    if builtin == "vec" && input.peek(syn::Token![;]) {
                        input.parse::<syn::Token![;]>()?;
                    } else {
                        input.parse::<syn::Token![,]>()?;
                    }
                }
            }
            Ok((expressions, pattern))
        };
        match parser.parse2(invocation.tokens.clone()) {
            Ok((expressions, pattern)) => {
                let previous = std::mem::replace(&mut self.in_macro, true);
                for expression in &expressions {
                    self.visit_expr(expression);
                }
                if let Some(pattern) = pattern {
                    self.visit_pat(&pattern);
                }
                self.in_macro = previous;
            }
            Err(error) => self
                .errors
                .push(format!("unhandled {name}! arguments: {error}")),
        }
    }
}

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
