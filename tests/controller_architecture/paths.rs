pub use super::imports::{expand_local_globs, imports, qualify};
use std::collections::{BTreeMap, BTreeSet};
use syn::ext::IdentExt;
use syn::parse::Parser;
use syn::visit::{self, Visit};

pub fn references(module: &str, file: &syn::File) -> Result<BTreeSet<String>, String> {
    let mut collector = References::new(module, file)?;
    collector.visit_file(file);
    if collector.errors.is_empty() {
        Ok(collector.paths)
    } else {
        Err(collector.errors.join("\n"))
    }
}

pub fn definitions(module: &str, file: &syn::File) -> Result<Vec<syn::Item>, String> {
    let mut collector = References::new(module, file)?;
    collector.visit_file(file);
    if collector.errors.is_empty() {
        Ok(collector.definitions)
    } else {
        Err(collector.errors.join("\n"))
    }
}

pub fn used_paths(module: &str, file: &syn::File) -> Result<BTreeSet<String>, String> {
    let mut collector = References::new(module, file)?;
    collector.paths.clear();
    collector.visit_file(file);
    if collector.errors.is_empty() {
        Ok(collector.paths)
    } else {
        Err(collector.errors.join("\n"))
    }
}

pub fn resolve_path(module: &str, file: &syn::File, path: &syn::Path) -> Result<String, String> {
    Ok(References::new(module, file)?.path(path))
}

pub fn method_names(module: &str, file: &syn::File) -> Result<BTreeSet<String>, String> {
    let mut collector = References::new(module, file)?;
    collector.visit_file(file);
    if collector.errors.is_empty() {
        Ok(collector.methods)
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
    methods: BTreeSet<String>,
    fields: BTreeSet<String>,
    block_depth: usize,
    definitions: Vec<syn::Item>,
}

impl<'a> References<'a> {
    fn new(module: &'a str, file: &syn::File) -> Result<Self, String> {
        let bindings = imports(module, file)?;
        Ok(Self {
            module,
            paths: bindings.values().cloned().collect(),
            bindings,
            errors: Vec::new(),
            in_macro: false,
            methods: BTreeSet::new(),
            fields: BTreeSet::new(),
            block_depth: 0,
            definitions: Vec::new(),
        })
    }

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
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if matches!(
            item,
            syn::Item::Struct(_) | syn::Item::Enum(_) | syn::Item::Trait(_) | syn::Item::Impl(_)
        ) {
            self.definitions.push(item.clone());
        }
        visit::visit_item(self, item);
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.block_depth += 1;
        visit::visit_block(self, block);
        self.block_depth -= 1;
    }
    fn visit_field(&mut self, field: &'ast syn::Field) {
        if let Some(ident) = &field.ident {
            self.fields.insert(ident.unraw().to_string());
        }
        visit::visit_field(self, field);
    }

    fn visit_expr_method_call(&mut self, expression: &'ast syn::ExprMethodCall) {
        self.methods.insert(expression.method.unraw().to_string());
        visit::visit_expr_method_call(self, expression);
    }
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

    fn visit_item_mod(&mut self, _item: &'ast syn::ItemMod) {
        if self.block_depth > 0 {
            self.errors
                .push("block-local module needs explicit support".into());
        }
    }

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

struct TypeName<'a> {
    name: &'a str,
    found: bool,
}
impl<'ast> Visit<'ast> for TypeName<'_> {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.found |= path
            .segments
            .last()
            .is_some_and(|segment| segment.ident.unraw() == self.name);
        visit::visit_path(self, path);
    }
}

pub fn type_has(ty: &syn::Type, name: &str) -> bool {
    let mut find = TypeName { name, found: false };
    find.visit_type(ty);
    find.found
}

pub fn signature_paths(
    module: &str,
    file: &syn::File,
    signature: &syn::Signature,
) -> Result<BTreeSet<String>, String> {
    let mut facts = References::new(module, file)?;
    facts.paths.clear();
    facts.visit_signature(signature);
    if facts.errors.is_empty() {
        Ok(facts.paths)
    } else {
        Err(facts.errors.join("\n"))
    }
}

pub fn output_functions(modules: &BTreeMap<String, syn::File>) -> BTreeSet<String> {
    super::source::module_family(modules, "controller::renderer")
        .flat_map(|(module, file)| {
            file.items.iter().filter_map(move |item| {
                let syn::Item::Fn(function) = item else {
                    return None;
                };
                let paths = signature_paths(module, file, &function.sig)
                    .expect("renderer signature dependencies must resolve");
                paths
                    .iter()
                    .any(|path| writer_path(path))
                    .then(|| format!("{module}::{}", function.sig.ident.unraw()))
            })
        })
        .collect()
}

pub const WRITER_OUTPUT_OPERATIONS: &[&str] = &[
    "flush",
    "write",
    "write_all",
    "write_fmt",
    "write_vectored",
    "writeln_newline",
];

pub fn output_dependencies(
    module: &str,
    file: &syn::File,
    output_functions: &BTreeSet<String>,
) -> Result<BTreeSet<String>, String> {
    let mut facts = References::new(module, file)?;
    facts.visit_file(file);
    if !facts.errors.is_empty() {
        return Err(facts.errors.join("\n"));
    }
    let mut output: BTreeSet<_> = facts
        .paths
        .into_iter()
        .filter(|path| {
            writer_path(path)
                || output_functions.contains(path)
                || WRITER_OUTPUT_OPERATIONS.contains(&path.rsplit("::").next().unwrap_or(path))
                || [
                    "SchronuWriter",
                    "DisplayRecorder",
                    "DisplayFragment",
                    "print",
                    "println",
                    "eprint",
                    "eprintln",
                    "write",
                    "writeln",
                ]
                .contains(&path.rsplit("::").next().unwrap_or(path))
        })
        .collect();
    for method in facts.methods {
        if WRITER_OUTPUT_OPERATIONS.contains(&method.as_str()) || method == "supports_ansi_color" {
            output.insert(format!("method::{method}"));
        }
    }
    if facts.fields.contains("supports_ansi_color") {
        output.insert("field::supports_ansi_color".into());
    }
    Ok(output)
}

fn writer_path(path: &str) -> bool {
    path == "std::io::Write"
        || path.starts_with("std::io::Write::")
        || path.split("::").any(|part| part == "SchronuWriter")
}
