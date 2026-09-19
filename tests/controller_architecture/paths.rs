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

pub fn type_alias_dependencies(
    modules: &BTreeMap<String, syn::File>,
) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let mut aliases = BTreeMap::new();
    for (module, file) in modules {
        let file = expand_local_globs(module, file, modules)?;
        for item in definitions(module, &file)? {
            if let syn::Item::Type(alias) = item {
                let mut scope = file.clone();
                scope.items.retain(|item| matches!(item, syn::Item::Use(_)));
                let key = format!("{module}::{}", alias.ident.unraw());
                scope.items.push(syn::Item::Type(alias));
                if aliases
                    .insert(key.clone(), used_paths(module, &scope)?)
                    .is_some()
                {
                    return Err(format!("ambiguous type alias: {key}"));
                }
            }
        }
    }
    Ok(aliases)
}

pub fn expand_type_aliases(
    module: &str,
    paths: BTreeSet<String>,
    aliases: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    let mut result = paths.clone();
    let mut pending: Vec<_> = paths
        .into_iter()
        .map(|path| (module.to_string(), path))
        .collect();
    let mut visited = BTreeSet::new();
    while let Some((owner, path)) = pending.pop() {
        let key = if aliases.contains_key(&path) {
            path
        } else {
            format!("{owner}::{path}")
        };
        if !visited.insert(key.clone()) {
            continue;
        }
        if let Some(dependencies) = aliases.get(&key) {
            let owner = key.rsplit_once("::").unwrap().0;
            for dependency in dependencies {
                result.insert(dependency.clone());
                pending.push((owner.to_string(), dependency.clone()));
            }
        }
    }
    result
}

pub fn signatures(module: &str, file: &syn::File) -> Result<Vec<syn::Signature>, String> {
    let mut collector = References::new(module, file)?;
    collector.visit_file(file);
    if collector.errors.is_empty() {
        Ok(collector.signatures)
    } else {
        Err(collector.errors.join("\n"))
    }
}

pub fn return_paths(
    module: &str,
    file: &syn::File,
    signature: &syn::Signature,
) -> Result<BTreeSet<String>, String> {
    let mut collector = References::new(module, file)?;
    collector.paths.clear();
    collector.visit_return_type(&signature.output);
    if collector.errors.is_empty() {
        Ok(collector.paths)
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

pub fn data_accesses(module: &str, file: &syn::File) -> Result<(BTreeSet<String>, bool), String> {
    let mut collector = References::new(module, file)?;
    collector.visit_file(file);
    if collector.errors.is_empty() {
        Ok((collector.members, collector.indexed))
    } else {
        Err(collector.errors.join("\n"))
    }
}

pub fn has_float_literal(module: &str, file: &syn::File) -> Result<bool, String> {
    let mut collector = References::new(module, file)?;
    collector.visit_file(file);
    if collector.errors.is_empty() {
        Ok(collector.float_literal)
    } else {
        Err(collector.errors.join("\n"))
    }
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
    calls: Vec<(String, syn::ExprCall)>,
    scoped_calls: bool,
    methods: BTreeSet<String>,
    fields: BTreeSet<String>,
    members: BTreeSet<String>,
    indexed: bool,
    block_depth: usize,
    callbacks: BTreeSet<String>,
    float_literal: bool,
    definitions: Vec<syn::Item>,
    signatures: Vec<syn::Signature>,
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
            calls: Vec::new(),
            scoped_calls: false,
            methods: BTreeSet::new(),
            fields: BTreeSet::new(),
            members: BTreeSet::new(),
            indexed: false,
            block_depth: 0,
            callbacks: BTreeSet::new(),
            float_literal: false,
            definitions: Vec::new(),
            signatures: Vec::new(),
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
    fn visit_member(&mut self, member: &'ast syn::Member) {
        if let syn::Member::Named(ident) = member {
            self.members.insert(ident.unraw().to_string());
        }
        visit::visit_member(self, member);
    }
    fn visit_expr_index(&mut self, expression: &'ast syn::ExprIndex) {
        self.indexed = true;
        visit::visit_expr_index(self, expression);
    }

    fn visit_lit_float(&mut self, _literal: &'ast syn::LitFloat) {
        self.float_literal = true;
    }

    fn visit_signature(&mut self, signature: &'ast syn::Signature) {
        self.signatures.push(signature.clone());
        visit::visit_signature(self, signature);
    }

    fn visit_item(&mut self, item: &'ast syn::Item) {
        if matches!(
            item,
            syn::Item::Struct(_)
                | syn::Item::Enum(_)
                | syn::Item::Trait(_)
                | syn::Item::Impl(_)
                | syn::Item::Type(_)
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

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !self.scoped_calls {
            visit::visit_item_fn(self, item);
        }
    }

    fn visit_expr_closure(&mut self, expression: &'ast syn::ExprClosure) {
        if !self.scoped_calls {
            visit::visit_expr_closure(self, expression);
        }
    }

    fn visit_expr_method_call(&mut self, expression: &'ast syn::ExprMethodCall) {
        self.methods.insert(expression.method.unraw().to_string());
        if self.scoped_calls && expression.method == "and_then" {
            // The maintenance parser's Result::and_then executes its inline
            // closure. Merely declaring another closure is not a direct call.
            self.visit_expr(&expression.receiver);
            for argument in &expression.args {
                if let syn::Expr::Closure(closure) = argument {
                    visit::visit_expr_closure(self, closure);
                } else {
                    self.visit_expr(argument);
                }
            }
        } else {
            visit::visit_expr_method_call(self, expression);
        }
    }
    fn visit_expr_call(&mut self, expression: &'ast syn::ExprCall) {
        let mut function = expression.func.as_ref();
        loop {
            function = match function {
                syn::Expr::Paren(group) => &group.expr,
                syn::Expr::Group(group) => &group.expr,
                _ => break,
            };
        }
        if let syn::Expr::Path(path) = function {
            self.calls.push((self.path(&path.path), expression.clone()));
        }
        if self.scoped_calls {
            self.visit_expr(&expression.func);
            let executes_callback = matches!(function, syn::Expr::Path(path) if self.callbacks.contains(&self.path(&path.path)));
            for argument in &expression.args {
                match argument {
                    syn::Expr::Closure(closure) if executes_callback => {
                        visit::visit_expr_closure(self, closure)
                    }
                    argument => self.visit_expr(argument),
                }
            }
        } else {
            visit::visit_expr_call(self, expression);
        }
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

pub fn input_has(signature: &syn::Signature, name: &str) -> bool {
    signature.inputs.iter().any(
        |argument| matches!(argument, syn::FnArg::Typed(argument) if type_has(&argument.ty, name)),
    )
}

pub fn output_has(signature: &syn::Signature, name: &str) -> bool {
    matches!(&signature.output, syn::ReturnType::Type(_, ty) if type_has(ty, name))
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
