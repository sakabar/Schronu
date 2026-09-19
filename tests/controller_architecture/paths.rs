use std::collections::{BTreeMap, BTreeSet};
use syn::ext::IdentExt;
use syn::parse::Parser;
use syn::visit::{self, Visit};
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

pub fn references(module: &str, file: &syn::File) -> Result<BTreeSet<String>, String> {
    let mut collector = References::new(module, file)?;
    collector.visit_file(file);
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
    block_depth: usize,
    callbacks: BTreeSet<String>,
    scaling: bool,
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
            block_depth: 0,
            callbacks: BTreeSet::new(),
            scaling: false,
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
    fn visit_expr_binary(&mut self, expression: &'ast syn::ExprBinary) {
        self.scaling |= matches!(expression.op, syn::BinOp::Mul(_) | syn::BinOp::Div(_));
        visit::visit_expr_binary(self, expression);
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

pub fn function_calls(
    module: &str,
    file: &syn::File,
    function: &syn::ItemFn,
) -> Result<Vec<(String, syn::ExprCall)>, String> {
    function_calls_with_callbacks(module, file, function, &BTreeSet::new())
}

pub fn function_has_scaling(
    module: &str,
    file: &syn::File,
    function: &syn::ItemFn,
) -> Result<bool, String> {
    let mut collector = References::new(module, file)?;
    collector.visit_block(&function.block);
    if collector.errors.is_empty() {
        Ok(collector.scaling)
    } else {
        Err(collector.errors.join("\n"))
    }
}

pub fn function_calls_with_callbacks(
    module: &str,
    file: &syn::File,
    function: &syn::ItemFn,
    callbacks: &BTreeSet<String>,
) -> Result<Vec<(String, syn::ExprCall)>, String> {
    block_calls_with_callbacks(module, file, &function.block, callbacks)
}

pub fn block_calls(
    module: &str,
    file: &syn::File,
    block: &syn::Block,
) -> Result<Vec<(String, syn::ExprCall)>, String> {
    block_calls_with_callbacks(module, file, block, &BTreeSet::new())
}

fn block_calls_with_callbacks(
    module: &str,
    file: &syn::File,
    block: &syn::Block,
    callbacks: &BTreeSet<String>,
) -> Result<Vec<(String, syn::ExprCall)>, String> {
    let mut collector = References::new(module, file)?;
    collector.scoped_calls = true;
    collector.callbacks = callbacks.clone();
    collector.visit_block(block);
    if collector.errors.is_empty() {
        Ok(collector.calls)
    } else {
        Err(collector.errors.join("\n"))
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
        if [
            "flush",
            "write_all",
            "writeln_newline",
            "supports_ansi_color",
        ]
        .contains(&method.as_str())
        {
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
