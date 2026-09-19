use super::paths::{
    expand_local_globs, function_calls, input_has, output_dependencies, references, used_paths,
};
use super::source::{controller_modules, fixture_modules};
use std::collections::BTreeMap;
use syn::ext::IdentExt;
use syn::visit::{self, Visit};

fn legacy_violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    struct Legacy(Vec<String>);
    impl<'ast> Visit<'ast> for Legacy {
        fn visit_ident(&mut self, ident: &'ast syn::Ident) {
            if matches!(
                ident.unraw().to_string().as_str(),
                "DisplayRecorder" | "DisplayFragment"
            ) {
                self.0.push(format!("legacy display identifier: {ident}"));
            }
        }
        fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
            if item.ident.unraw() == "DisplayModel"
                && item.variants.iter().any(|v| v.ident.unraw() == "Legacy")
            {
                self.0.push("legacy DisplayModel variant".into());
            }
            visit::visit_item_enum(self, item);
        }
        fn visit_item_mod(&mut self, _item: &'ast syn::ItemMod) {}
    }
    let mut legacy = Legacy(Vec::new());
    for (module, file) in modules {
        legacy.visit_file(file);
        let paths =
            expand_local_globs(module, file, modules).and_then(|file| references(module, &file));
        match paths {
            Ok(paths) => {
                for path in paths {
                    if path.ends_with("DisplayModel::Legacy")
                        || path
                            .split("::")
                            .any(|part| matches!(part, "DisplayRecorder" | "DisplayFragment"))
                    {
                        legacy.0.push(format!("legacy display dependency: {path}"));
                    }
                }
            }
            Err(error) => legacy.0.push(error),
        }
    }
    legacy.0
}

#[test]
fn semantic_display_cannot_restore_a_legacy_variant() {
    let modules = fixture_modules(
        "mod renderer;",
        &[(
            "renderer.rs",
            "enum DisplayModel { Legacy { fragments: Vec<u8> } }",
        )],
    );
    assert!(!legacy_violations(&modules).is_empty());
}

#[test]
fn all_product_controller_modules_use_semantic_display() {
    assert_eq!(
        legacy_violations(&controller_modules()),
        Vec::<String>::new()
    );
}

#[test]
fn nested_and_aliased_legacy_display_dependencies_are_rejected() {
    for source in [
        "mod helper { struct DisplayRecorder; }",
        "use super::renderer::DisplayModel as Model; fn renamed() { let _ = Model::Legacy; }",
        "fn renamed() { format!(\"{}\", super::renderer::DisplayRecorder::new()); }",
    ] {
        let modules = fixture_modules(
            "mod runtime; mod renderer;",
            &[
                ("runtime.rs", source),
                ("renderer.rs", "enum DisplayModel { Message }"),
            ],
        );
        assert!(!legacy_violations(&modules).is_empty(), "{source}");
    }
}

#[test]
fn semantic_display_ignores_comments_literals_and_test_only_items() {
    let modules = fixture_modules(
        "mod renderer;",
        &[(
            "renderer.rs",
            r#"
        enum DisplayModel { Message }
        // DisplayRecorder::new();
        const NOTE: &str = "DisplayFragment DisplayModel::Legacy";
        #[cfg(test)] struct DisplayRecorder;
    "#,
        )],
    );
    assert!(legacy_violations(&modules).is_empty());
}

#[test]
fn raw_legacy_variant_declarations_have_the_same_identity() {
    let modules = fixture_modules(
        "mod renderer;",
        &[("renderer.rs", "enum r#DisplayModel { r#Legacy }")],
    );
    assert!(!legacy_violations(&modules).is_empty());
}

fn mode_violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    let mut modes = Vec::new();
    let mut renderers = Vec::new();
    let mut plain_renderers = Vec::new();
    let mut outcomes = Vec::new();
    for (module, file) in modules {
        for item in &file.items {
            if matches!(item, syn::Item::Enum(item) if item.ident.unraw() == "RenderMode") {
                modes.push(module);
            }
            let syn::Item::Fn(function) = item else {
                continue;
            };
            if input_has(&function.sig, "SchronuWriter") && input_has(&function.sig, "DisplayModel")
            {
                if input_has(&function.sig, "RenderMode") {
                    renderers.push((module, file, function));
                } else {
                    plain_renderers.push((module, file, function));
                }
            }
            if input_has(&function.sig, "CommandOutcome") {
                outcomes.push((module, file, function));
            }
        }
    }
    let (
        [(renderer_module, renderer_file, renderer)],
        [(plain_module, _, plain)],
        [(outcome_module, outcome_file, outcome)],
    ) = (
        renderers.as_slice(),
        plain_renderers.as_slice(),
        outcomes.as_slice(),
    )
    else {
        return vec!["one mode renderer, plain renderer, and outcome coordinator required".into()];
    };
    let in_family =
        |module: &str, root: &str| module == root || module.starts_with(&format!("{root}::"));
    let mut errors = Vec::new();
    if modes.len() != 1
        || !in_family(modes[0], "controller::renderer")
        || !in_family(renderer_module, "controller::renderer")
        || !in_family(plain_module, "controller::renderer")
        || !in_family(outcome_module, "controller::runtime")
    {
        errors.push("render mode belongs to renderer and outcome coordination to runtime".into());
    }
    for (module, file, function, is_renderer) in [
        (*renderer_module, *renderer_file, *renderer, true),
        (*outcome_module, *outcome_file, *outcome, false),
    ] {
        let expanded = match expand_local_globs(module, file, modules) {
            Ok(file) => file,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let calls = match function_calls(module, &expanded, function) {
            Ok(calls) => calls,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let mut scope = expanded.clone();
        scope.items.retain(|item| matches!(item, syn::Item::Use(_)));
        scope.items.push(syn::Item::Fn(function.clone()));
        let paths = match used_paths(module, &scope) {
            Ok(paths) => paths,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let output = match output_dependencies(module, &scope, &Default::default()) {
            Ok(output) => output,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let flushes = output.iter().any(|path| path.ends_with("::flush"));
        let calls_role = |owner: &str, name: &syn::Ident| {
            calls.iter().any(|(path, _)| {
                path == &format!("{owner}::{}", name.unraw())
                    || (module == owner && path == &name.unraw().to_string())
            })
        };
        if is_renderer {
            if !calls_role(plain_module, &plain.sig.ident)
                || !flushes
                || !paths
                    .iter()
                    .any(|path| path.ends_with("RenderMode::Flushed"))
            {
                errors.push("mode renderer must render semantic data and own flushing".into());
            }
        } else {
            if !calls_role(renderer_module, &renderer.sig.ident)
                || ["Flushed", "Unflushed"].iter().any(|variant| {
                    !paths
                        .iter()
                        .any(|path| path.ends_with(&format!("RenderMode::{variant}")))
                })
            {
                errors.push("outcome coordinator must select both render modes".into());
            }
            if flushes
                || calls_role(plain_module, &plain.sig.ident)
                || paths
                    .iter()
                    .any(|path| path.ends_with("DisplayModel::flush"))
            {
                errors.push("outcome coordinator must delegate renderer operations".into());
            }
        }
    }
    errors
}

fn mode_fixture(outcome: &str) -> BTreeMap<String, syn::File> {
    fixture_modules("mod renderer; mod runtime;", &[
        ("renderer.rs", "enum RenderMode { Flushed, Unflushed } fn plain(w: &mut dyn SchronuWriter, model: &DisplayModel) {} fn emit(w: &mut dyn SchronuWriter, model: &DisplayModel, mode: RenderMode) { plain(w, model); if mode == RenderMode::Flushed { w.flush(); } }"),
        ("runtime.rs", &format!("use super::renderer::{{emit, plain, RenderMode}}; fn coordinate(w: &mut dyn SchronuWriter, outcome: CommandOutcome) {{ {outcome} }}")),
    ])
}

#[test]
fn outcome_coordinator_cannot_flush_the_writer_directly() {
    let modules = mode_fixture("emit(w, &outcome.display, RenderMode::Unflushed); emit(w, &DisplayModel::empty(), RenderMode::Flushed); w.flush();");
    assert!(!mode_violations(&modules).is_empty());
}

#[test]
fn product_outcome_uses_renderer_owned_flush_modes() {
    assert_eq!(mode_violations(&controller_modules()), Vec::<String>::new());
}

#[test]
fn renamed_mode_roles_preserve_the_contract() {
    let modules = mode_fixture("emit(w, &outcome.display, RenderMode::Unflushed); emit(w, &DisplayModel::empty(), RenderMode::Flushed);");
    assert!(mode_violations(&modules).is_empty());
}

#[test]
fn mode_selection_cannot_be_replaced_by_plain_rendering_or_decoys() {
    for body in [
        "plain(w, &outcome.display);",
        "let note = \"emit(w, model, RenderMode::Flushed) RenderMode::Unflushed\";",
        "emit(w, &outcome.display, RenderMode::Unflushed);",
    ] {
        assert!(!mode_violations(&mode_fixture(body)).is_empty());
    }
}

fn diagnostic_roles(
    modules: &BTreeMap<String, syn::File>,
) -> Result<BTreeMap<&'static str, (String, syn::ItemFn)>, String> {
    let mut roles: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for module in ["controller::runtime", "controller::renderer"] {
        let file = modules
            .get(module)
            .ok_or_else(|| format!("missing diagnostic module: {module}"))?;
        for item in &file.items {
            let syn::Item::Fn(function) = item else {
                continue;
            };
            let input = |name| input_has(&function.sig, name);
            let output = |name| super::paths::output_has(&function.sig, name);
            for (role, matches) in [
                ("error_model", output("DisplayModel") && input("Display")),
                (
                    "verify_model",
                    output("DisplayModel") && function.sig.inputs.is_empty(),
                ),
                (
                    "mode_renderer",
                    input("DisplayModel") && input("SchronuWriter") && input("RenderMode"),
                ),
                (
                    "renderer",
                    input("DisplayModel") && input("SchronuWriter") && !input("RenderMode"),
                ),
                ("plain_renderer", input("DisplayModel") && input("Write")),
                (
                    "report",
                    input("Write") && input("RunError") && output("bool"),
                ),
                (
                    "exit_save",
                    input("SchronuWriter")
                        && input("TaskRepositoryTrait")
                        && output("bool")
                        && !input("FreeTimeManagerTrait"),
                ),
                (
                    "verify",
                    input("SchronuWriter")
                        && input("TaskRepositoryTrait")
                        && input("DateTime")
                        && output("RunError")
                        && function.sig.inputs.len() == 3,
                ),
                (
                    "argv",
                    input("TaskRepositoryTrait")
                        && input("String")
                        && input("DateTime")
                        && output("RunError"),
                ),
                (
                    "interactive",
                    input("str")
                        && input("TaskRepositoryTrait")
                        && output("InteractiveCommandExecution"),
                ),
            ] {
                if matches {
                    roles
                        .entry(role)
                        .or_default()
                        .push((module.to_string(), function.clone()));
                }
            }
        }
    }
    let mut result = BTreeMap::new();
    for role in [
        "error_model",
        "verify_model",
        "mode_renderer",
        "renderer",
        "plain_renderer",
        "report",
        "exit_save",
        "verify",
        "argv",
        "interactive",
    ] {
        let mut candidates = roles.remove(role).unwrap_or_default();
        if candidates.len() != 1 {
            return Err(format!(
                "one diagnostic role required: {role}, found {}",
                candidates.len()
            ));
        }
        result.insert(role, candidates.remove(0));
    }
    Ok(result)
}

fn borrowed_value(mut expression: &syn::Expr) -> &syn::Expr {
    loop {
        expression = match expression {
            syn::Expr::Reference(value) => &value.expr,
            syn::Expr::Paren(value) => &value.expr,
            syn::Expr::Group(value) => &value.expr,
            _ => return expression,
        };
    }
}

fn diagnostic_violations(modules: &BTreeMap<String, syn::File>) -> Vec<String> {
    let roles = match diagnostic_roles(modules) {
        Ok(roles) => roles,
        Err(error) => return vec![error],
    };
    let canonical = |module: &str, path: &str| {
        if path.contains("::") {
            path.to_string()
        } else {
            format!("{module}::{path}")
        }
    };
    let role_path = |role| {
        let (module, function) = &roles[role];
        format!("{module}::{}", function.sig.ident.unraw())
    };
    let mut errors = Vec::new();
    for (role, renderer, model, flushed) in [
        ("verify", "mode_renderer", "verify_model", true),
        ("exit_save", "mode_renderer", "error_model", true),
        ("report", "plain_renderer", "error_model", false),
        ("interactive", "renderer", "error_model", false),
    ] {
        let (module, function) = &roles[role];
        let check = || -> Result<(), String> {
            let file = expand_local_globs(module, &modules[module], modules)?;
            let calls = function_calls(module, &file, function)?;
            let renderer_calls: Vec<_> = calls
                .iter()
                .filter(|(path, _)| canonical(module, path) == role_path(renderer))
                .collect();
            if renderer_calls.len() != 1 {
                return Err(format!("{role} must call semantic renderer once"));
            }
            let call = &renderer_calls[0].1;
            let argument = call
                .args
                .iter()
                .nth(1)
                .ok_or_else(|| format!("{role} missing model argument"))?;
            let syn::Expr::Call(model_call) = borrowed_value(argument) else {
                return Err(format!(
                    "{role} must pass its semantic model result directly"
                ));
            };
            let syn::Expr::Path(model_function) = borrowed_value(&model_call.func) else {
                return Err(format!("{role} must call its typed model builder"));
            };
            let target = super::paths::resolve_path(module, &file, &model_function.path)?;
            if canonical(module, &target) != role_path(model) {
                return Err(format!("{role} must pass its semantic model to renderer"));
            }
            if flushed {
                let Some(syn::Expr::Path(mode)) = call.args.iter().nth(2) else {
                    return Err(format!("{role} missing flush mode"));
                };
                if super::paths::resolve_path(module, &file, &mode.path)?
                    != "controller::renderer::RenderMode::Flushed"
                {
                    return Err(format!("{role} must request flushed rendering"));
                }
            }
            Ok(())
        };
        if let Err(error) = check() {
            errors.push(error);
        }
    }
    for role in ["verify", "exit_save", "report", "argv", "interactive"] {
        let (module, function) = &roles[role];
        let check = || -> Result<(), String> {
            let file = expand_local_globs(module, &modules[module], modules)?;
            let calls = function_calls(module, &file, function)?;
            let mut scope = file.clone();
            scope.items.retain(|item| matches!(item, syn::Item::Use(_)));
            scope.items.push(syn::Item::Fn(function.clone()));
            for path in output_dependencies(module, &scope, &Default::default())? {
                if [
                    "flush",
                    "write",
                    "write_all",
                    "writeln",
                    "writeln_newline",
                    "print",
                    "println",
                    "eprint",
                    "eprintln",
                ]
                .contains(&path.rsplit("::").next().unwrap_or(&path))
                {
                    return Err(format!("{role} owns raw diagnostic output: {path}"));
                }
            }
            if role == "argv" {
                if calls
                    .iter()
                    .filter(|(path, _)| canonical(module, path) == role_path("verify"))
                    .count()
                    != 1
                {
                    return Err("argv must delegate Verify to its semantic boundary".into());
                }
                if used_paths(module, &scope)?.iter().any(|path| {
                    ["verify_model", "renderer", "mode_renderer"]
                        .iter()
                        .any(|target| canonical(module, path) == role_path(target))
                }) {
                    return Err("argv must not own Verify presentation".into());
                }
            }
            if role == "interactive" {
                if used_paths(module, &scope)?
                    .iter()
                    .any(|path| canonical(module, path) == role_path("verify_model"))
                {
                    return Err("interactive Verify must not add a success body".into());
                }
                let mode_calls: Vec<_> = calls
                    .iter()
                    .filter(|(path, _)| canonical(module, path) == role_path("mode_renderer"))
                    .collect();
                if mode_calls.len() != 1 {
                    return Err("interactive Verify must use mode renderer once".into());
                }
                let Some(syn::Expr::Path(mode)) = mode_calls[0].1.args.iter().nth(2) else {
                    return Err("interactive Verify missing flush mode".into());
                };
                if super::paths::resolve_path(module, &file, &mode.path)?
                    != "controller::renderer::RenderMode::Flushed"
                {
                    return Err("interactive Verify must request flushed rendering".into());
                }
            }
            Ok(())
        };
        if let Err(error) = check() {
            errors.push(error);
        }
    }
    errors
}

#[test]
fn exit_save_diagnostic_cannot_bypass_semantic_rendering() {
    let mut modules = controller_modules();
    let file = modules.get_mut("controller::runtime").unwrap();
    let function = file
        .items
        .iter_mut()
        .find_map(|item| match item {
            syn::Item::Fn(function)
                if input_has(&function.sig, "SchronuWriter")
                    && input_has(&function.sig, "TaskRepositoryTrait")
                    && super::paths::output_has(&function.sig, "bool")
                    && !input_has(&function.sig, "FreeTimeManagerTrait") =>
            {
                Some(function)
            }
            _ => None,
        })
        .unwrap();
    function.block = syn::parse_quote!({
        writer.flush()?;
        Ok(false)
    });
    assert!(!diagnostic_violations(&modules).is_empty());
}

#[test]
fn product_diagnostics_use_semantic_renderer_boundaries() {
    assert_eq!(
        diagnostic_violations(&controller_modules()),
        Vec::<String>::new()
    );
}

#[test]
fn diagnostic_function_renames_preserve_typed_connections() {
    use syn::visit_mut::{self, VisitMut};
    struct Rename(BTreeMap<String, syn::Ident>);
    impl VisitMut for Rename {
        fn visit_ident_mut(&mut self, ident: &mut syn::Ident) {
            if let Some(replacement) = self.0.get(&ident.unraw().to_string()) {
                *ident = replacement.clone();
            }
            visit_mut::visit_ident_mut(self, ident);
        }
    }
    let mut modules = controller_modules();
    let roles = diagnostic_roles(&modules).unwrap();
    let mut rename = Rename(
        roles
            .into_iter()
            .enumerate()
            .map(|(index, (_, (_, function)))| {
                (
                    function.sig.ident.unraw().to_string(),
                    syn::Ident::new(&format!("renamed_{index}"), function.sig.ident.span()),
                )
            })
            .collect(),
    );
    for file in modules.values_mut() {
        rename.visit_file_mut(file);
    }
    assert!(diagnostic_violations(&modules).is_empty());
}

#[test]
fn diagnostic_model_must_be_the_renderers_argument() {
    let mut modules = controller_modules();
    let roles = diagnostic_roles(&modules).unwrap();
    let (module, role) = &roles["report"];
    let file = modules.get_mut(module).unwrap();
    let function = file
        .items
        .iter_mut()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == role.sig.ident => Some(function),
            _ => None,
        })
        .unwrap();
    function.block = syn::parse_quote!({
        let ignored = error_display_model(&error);
        render_plain_display_model(writer, &DisplayModel::empty());
        false
    });
    assert!(!diagnostic_violations(&modules).is_empty());
}

#[test]
fn diagnostic_model_cannot_be_discarded_inside_the_argument() {
    let mut modules = controller_modules();
    let roles = diagnostic_roles(&modules).unwrap();
    let (module, role) = &roles["report"];
    let error_model = &roles["error_model"].1.sig.ident;
    let renderer = &roles["plain_renderer"].1.sig.ident;
    let file = modules.get_mut(module).unwrap();
    let function = file
        .items
        .iter_mut()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == role.sig.ident => Some(function),
            _ => None,
        })
        .unwrap();
    function.block = syn::parse_quote!({ #renderer(writer, &{ let _ = #error_model(error); DisplayModel::empty() }); false });
    assert!(!diagnostic_violations(&modules).is_empty());
    let file = modules.get_mut(module).unwrap();
    let function = file
        .items
        .iter_mut()
        .find_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == role.sig.ident => Some(function),
            _ => None,
        })
        .unwrap();
    function.block = syn::parse_quote!({ #renderer(writer, &((#error_model)(error))); false });
    assert!(diagnostic_violations(&modules).is_empty());
}
