use super::command::{parse_command, representative_valid_commands, CommandKind, ParseMode};
use super::interactive::should_suppress_leaf_tasks_after_command;
use std::fs;
use std::path::{Path, PathBuf};

struct ControllerProductSource {
    path: PathBuf,
    text: String,
}

fn controller_product_sources() -> Vec<ControllerProductSource> {
    let controller_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/adapter/controller/schronu");
    let mut product_sources = Vec::new();

    append_controller_product_sources(&controller_dir, &mut product_sources);
    product_sources.sort_by(|left, right| left.path.cmp(&right.path));
    product_sources
}

fn append_controller_product_sources(
    directory: &Path,
    product_sources: &mut Vec<ControllerProductSource>,
) {
    for entry in
        fs::read_dir(directory).expect("controller source directory must be readable recursively")
    {
        let entry = entry.expect("controller source entry must be readable");
        let file_type = entry
            .file_type()
            .expect("controller source entry type must be readable");
        let path = entry.path();
        if file_type.is_dir() {
            append_controller_product_sources(&path, product_sources);
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs")
            || file_name.ends_with("_tests.rs")
            || file_name.ends_with("_test_support.rs")
        {
            continue;
        }
        let text = fs::read_to_string(&path).expect("controller product source must be readable");
        product_sources.push(ControllerProductSource { path, text });
    }
}

fn unique_function_region<'a>(
    sources: &'a [ControllerProductSource],
    function_name: &str,
) -> Result<(&'a Path, &'a str), String> {
    let mut definitions = sources
        .iter()
        .flat_map(|source| {
            top_level_function_definition_offsets(&source.text, function_name)
                .into_iter()
                .map(move |offset| (source, offset))
        })
        .collect::<Vec<_>>();
    if definitions.len() != 1 {
        let paths = definitions
            .iter()
            .map(|(source, _)| source.path.display().to_string())
            .collect::<Vec<_>>();
        return Err(format!(
            "product function {function_name} must have exactly one top-level definition; found {} in {paths:?}",
            definitions.len()
        ));
    }
    let (source, start) = definitions.pop().unwrap();
    Ok((
        &source.path,
        function_region_from_offset(&source.text, start),
    ))
}

fn source_depends_on_module(source: &str, module_name: &str) -> bool {
    let code = compact_code(&code_only(source));
    code.contains(&format!("::{module_name}")) || code.contains(&format!("{module_name}::"))
}

fn handler_entry_boundary_violations(sources: &[ControllerProductSource]) -> Vec<String> {
    let mut violations = Vec::new();
    let Some(command_context) = sources.iter().find(|source| {
        source.path.file_name().and_then(|name| name.to_str()) == Some("command_context.rs")
    }) else {
        return vec!["command_context.rs must remain a product module".into()];
    };
    if source_depends_on_module(&command_context.text, "runtime") {
        violations
            .push("command_context.rs must not depend on its outer runtime coordinator".into());
    }

    let Ok((_, execute_parsed_source)) = unique_function_region(sources, "execute_parsed") else {
        violations.push("runtime must keep exactly one parsed-command coordinator".into());
        return violations;
    };
    let execute_parsed_code = compact_code(&code_only(execute_parsed_source));
    let Some(body_start) = execute_parsed_code.find('{') else {
        violations.push("parsed-command coordinator must have a function body".into());
        return violations;
    };
    let Some(handler_offset) = execute_parsed_code.find("handle_command(") else {
        violations.push("parsed-command coordinator must call the unified handler".into());
        return violations;
    };
    let signature = &execute_parsed_code[..body_start];
    let Some(command_type_offset) = signature.find(":&Command") else {
        violations
            .push("parsed-command coordinator must accept one typed Command reference".into());
        return violations;
    };
    let command_parameter = signature[..command_type_offset]
        .chars()
        .rev()
        .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    if command_parameter.is_empty() {
        violations.push("parsed-command coordinator must name its typed Command parameter".into());
        return violations;
    }
    if contains_identifier(
        &execute_parsed_code[body_start + 1..handler_offset],
        &command_parameter,
    ) {
        violations.push(
            "runtime must not inspect or delegate the typed command before handle_command".into(),
        );
    }
    violations
}

fn function_region_from_offset(source: &str, start: usize) -> &str {
    let tail = &source[start..];
    let signature_end = tail.find('\n').map_or(tail.len(), |offset| offset + 1);
    let mut end = tail.len();
    let mut line_start = signature_end;
    let mut non_code_state = MultilineNonCodeState::default();
    for line in tail[signature_end..].split_inclusive('\n') {
        if !non_code_state.starts_in_non_code()
            && is_top_level_rust_item(line.trim_end_matches('\n'))
        {
            end = line_start;
            break;
        }
        non_code_state.scan_line(line);
        line_start += line.len();
    }
    &tail[..end]
}

fn top_level_function_definition_offsets(source: &str, function_name: &str) -> Vec<usize> {
    top_level_item_definition_offsets(source, &format!("fn {function_name}"))
}

fn top_level_product_function_offsets(source: &str) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut line_start = 0;
    let mut non_code_state = MultilineNonCodeState::default();
    let mut cfg_test_attribute_pending = false;

    for line in source.split_inclusive('\n') {
        let source_line = line.trim_end_matches(['\r', '\n']);
        if !non_code_state.starts_in_non_code()
            && !source_line.chars().next().is_some_and(char::is_whitespace)
        {
            if source_line.starts_with("#[") {
                cfg_test_attribute_pending |= source_line.contains("cfg(test)");
            } else if is_top_level_rust_item(source_line) {
                if !cfg_test_attribute_pending && is_top_level_function_declaration(source_line) {
                    offsets.push(line_start);
                }
                cfg_test_attribute_pending = false;
            }
        }
        non_code_state.scan_line(line);
        line_start += line.len();
    }
    offsets
}

fn is_top_level_function_declaration(line: &str) -> bool {
    let declaration = strip_top_level_visibility(line);
    let bytes = declaration.as_bytes();
    let mut index = 0;

    loop {
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        let token_start = index;
        while bytes
            .get(index)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            index += 1;
        }
        let token = &declaration[token_start..index];
        if token == "fn" {
            return bytes.get(index).is_some_and(u8::is_ascii_whitespace);
        }
        if !matches!(token, "async" | "const" | "unsafe" | "extern") {
            return false;
        }
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if token == "extern" && bytes.get(index) == Some(&b'"') {
            index += 1;
            let mut escaped = false;
            while let Some(byte) = bytes.get(index) {
                index += 1;
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' {
                    escaped = true;
                } else if *byte == b'"' {
                    break;
                }
            }
        }
    }
}

fn top_level_item_definition_offsets(source: &str, item_prefix: &str) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut line_start = 0;
    let mut non_code_state = MultilineNonCodeState::default();
    let mut cfg_test_attribute_pending = false;

    for line in source.split_inclusive('\n') {
        let source_line = line.trim_end_matches(['\r', '\n']);
        if !non_code_state.starts_in_non_code()
            && !source_line.chars().next().is_some_and(char::is_whitespace)
        {
            if source_line.starts_with("#[") {
                cfg_test_attribute_pending |= source_line.contains("cfg(test)");
            } else if is_top_level_rust_item(source_line) {
                let declaration = strip_top_level_visibility(source_line);
                if !cfg_test_attribute_pending
                    && declaration.strip_prefix(item_prefix).is_some_and(|rest| {
                        rest.is_empty()
                            || rest.chars().next().is_some_and(|character| {
                                character.is_whitespace()
                                    || matches!(character, '<' | '(' | '{' | ':')
                            })
                    })
                {
                    offsets.push(line_start);
                }
                cfg_test_attribute_pending = false;
            }
        }
        non_code_state.scan_line(line);
        line_start += line.len();
    }
    offsets
}

#[derive(Default)]
struct MultilineNonCodeState {
    raw_string_terminator_opt: Option<String>,
    block_comment_depth: usize,
    in_quoted_string: bool,
}

impl MultilineNonCodeState {
    fn starts_in_non_code(&self) -> bool {
        self.raw_string_terminator_opt.is_some()
            || self.block_comment_depth > 0
            || self.in_quoted_string
    }

    fn scan_line(&mut self, line: &str) {
        let _ = self.code_mask(line);
    }

    fn code_mask(&mut self, source: &str) -> Vec<bool> {
        let bytes = source.as_bytes();
        let mut mask = vec![false; bytes.len()];
        let mut index = 0;
        while index < bytes.len() {
            if let Some(terminator) = self.raw_string_terminator_opt.as_ref() {
                if bytes[index..].starts_with(terminator.as_bytes()) {
                    index += terminator.len();
                    self.raw_string_terminator_opt = None;
                } else {
                    index += 1;
                }
                continue;
            }

            if self.block_comment_depth > 0 {
                if bytes[index..].starts_with(b"/*") {
                    self.block_comment_depth += 1;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    self.block_comment_depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
                continue;
            }

            if self.in_quoted_string {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == b'"' {
                    self.in_quoted_string = false;
                    index += 1;
                } else {
                    index += 1;
                }
                continue;
            }

            if bytes[index..].starts_with(b"//") {
                break;
            }
            if bytes[index..].starts_with(b"/*") {
                self.block_comment_depth = 1;
                index += 2;
                continue;
            }
            if let Some((opener_length, terminator)) = raw_string_opener_at(source, index) {
                self.raw_string_terminator_opt = Some(terminator);
                index += opener_length;
                continue;
            }
            if bytes[index] == b'"' {
                self.in_quoted_string = true;
            } else if bytes[index] == b'\'' {
                if let Some(end) = char_literal_end_at(bytes, index) {
                    index = end;
                } else {
                    mask[index] = true;
                }
            } else {
                mask[index] = true;
            }
            index += 1;
        }
        mask
    }
}

fn char_literal_end_at(bytes: &[u8], index: usize) -> Option<usize> {
    if bytes
        .get(index + 1)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        && bytes.get(index + 2) != Some(&b'\'')
    {
        return None;
    }
    let mut cursor = index + 1;
    let mut escaped = false;
    while cursor < bytes.len() && bytes[cursor] != b'\n' {
        if escaped {
            escaped = false;
        } else if bytes[cursor] == b'\\' {
            escaped = true;
        } else if bytes[cursor] == b'\'' {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

fn raw_string_opener_at(source: &str, index: usize) -> Option<(usize, String)> {
    let bytes = source.as_bytes();
    if index > 0 && (bytes[index - 1].is_ascii_alphanumeric() || bytes[index - 1] == b'_') {
        return None;
    }
    let prefix_length = if bytes[index..].starts_with(b"br") {
        2
    } else if bytes.get(index) == Some(&b'r') {
        1
    } else {
        return None;
    };
    let mut quote_index = index + prefix_length;
    while bytes.get(quote_index) == Some(&b'#') {
        quote_index += 1;
    }
    if bytes.get(quote_index) != Some(&b'"') {
        return None;
    }
    let hash_count = quote_index - index - prefix_length;
    Some((
        prefix_length + hash_count + 1,
        format!("\"{}", "#".repeat(hash_count)),
    ))
}

fn is_top_level_rust_item(line: &str) -> bool {
    if line.is_empty() || line.chars().next().is_some_and(char::is_whitespace) {
        return false;
    }
    if line.starts_with("#[") || line.starts_with("#![") {
        return true;
    }

    let declaration = strip_top_level_visibility(line);

    if is_top_level_function_declaration(line) {
        return true;
    }

    [
        "struct ",
        "enum ",
        "union ",
        "impl ",
        "trait ",
        "const ",
        "static ",
        "type ",
        "mod ",
        "use ",
        "extern ",
        "macro_rules!",
    ]
    .iter()
    .any(|item_prefix| declaration.starts_with(item_prefix))
}

fn code_only(source: &str) -> String {
    let mut output = vec![b' '; source.len()];
    let mut state = MultilineNonCodeState::default();
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let mask = state.code_mask(line);
        for (index, (byte, is_code)) in line.as_bytes().iter().zip(mask).enumerate() {
            if is_code || *byte == b'\n' {
                output[offset + index] = *byte;
            }
        }
        offset += line.len();
    }

    String::from_utf8(output).expect("source code bytes remain valid UTF-8")
}

fn contains_identifier(source: &str, identifier: &str) -> bool {
    source
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .any(|token| token == identifier)
}

fn contains_direct_free_function_call(code: &str, function_name: &str) -> bool {
    code.match_indices(function_name).any(|(start, _)| {
        let before = &code[..start];
        let after = &code[start + function_name.len()..];
        let has_identifier_boundary = before
            .chars()
            .next_back()
            .is_none_or(|character| !(character.is_ascii_alphanumeric() || character == '_'))
            && after
                .chars()
                .next()
                .is_none_or(|character| !(character.is_ascii_alphanumeric() || character == '_'));
        let is_unqualified = !matches!(
            before
                .chars()
                .rev()
                .find(|character| !character.is_whitespace()),
            Some('.' | ':')
        );
        let previous_token = before
            .trim_end()
            .rsplit(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .next()
            .unwrap_or_default();
        has_identifier_boundary
            && is_unqualified
            && previous_token != "fn"
            && after.trim_start().starts_with('(')
    })
}

struct DirectMethodLocation {
    start: usize,
    signature_end: usize,
    body_end_opt: Option<usize>,
}

fn direct_method_locations(container: &str, method_name: &str) -> Vec<DirectMethodLocation> {
    let code = code_only(container);
    let bytes = code.as_bytes();
    let mut locations = Vec::new();
    let mut brace_depth: usize = 0;
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'{' => {
                brace_depth += 1;
                index += 1;
            }
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                index += 1;
            }
            byte if brace_depth == 1 && (byte.is_ascii_alphabetic() || byte == b'_') => {
                let token_start = index;
                while bytes
                    .get(index)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    index += 1;
                }
                if &code[token_start..index] != "fn" {
                    continue;
                }
                while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
                    index += 1;
                }
                let name_start = index;
                while bytes
                    .get(index)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    index += 1;
                }
                if &code[name_start..index] != method_name {
                    continue;
                }
                let Some(relative_signature_end) = bytes[index..]
                    .iter()
                    .position(|byte| matches!(byte, b'{' | b';'))
                else {
                    continue;
                };
                let signature_end = index + relative_signature_end;
                if bytes[signature_end] == b';' {
                    locations.push(DirectMethodLocation {
                        start: token_start,
                        signature_end,
                        body_end_opt: None,
                    });
                    index = signature_end + 1;
                    continue;
                }

                let body_start = signature_end;
                let mut body_depth = 1;
                let mut body_end = body_start + 1;
                while body_end < bytes.len() && body_depth > 0 {
                    match bytes[body_end] {
                        b'{' => body_depth += 1,
                        b'}' => body_depth -= 1,
                        _ => {}
                    }
                    body_end += 1;
                }
                if body_depth == 0 {
                    locations.push(DirectMethodLocation {
                        start: token_start,
                        signature_end,
                        body_end_opt: Some(body_end),
                    });
                    index = body_end;
                }
            }
            _ => index += 1,
        }
    }
    locations
}

fn direct_impl_method_regions<'a>(implementation: &'a str, method_name: &str) -> Vec<&'a str> {
    direct_method_locations(implementation, method_name)
        .into_iter()
        .filter_map(|location| {
            location
                .body_end_opt
                .map(|body_end| &implementation[location.start..body_end])
        })
        .collect()
}

fn direct_method_signature_regions<'a>(container: &'a str, method_name: &str) -> Vec<&'a str> {
    direct_method_locations(container, method_name)
        .into_iter()
        .map(|location| &container[location.start..location.signature_end])
        .collect()
}

fn compact_code(source: &str) -> String {
    code_only(source)
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn normalized_method_signature(source: &str) -> String {
    compact_code(source).replace(",)", ")")
}

fn unique_top_level_item_region<'a>(source: &'a str, item_prefix: &str) -> Result<&'a str, String> {
    let offsets = top_level_item_definition_offsets(source, item_prefix);
    if offsets.len() != 1 {
        return Err(format!(
            "{item_prefix} must have exactly one product definition; found {}",
            offsets.len()
        ));
    }
    Ok(function_region_from_offset(source, offsets[0]))
}

fn task_tree_writer_free_boundary_violations(
    handler_source: &str,
    context_source: &str,
) -> Vec<String> {
    let mut violations = Vec::new();

    let trait_region =
        match unique_top_level_item_region(handler_source, "trait TaskTreeCommandContext") {
            Ok(region) => region,
            Err(error) => {
                violations.push(error);
                return violations;
            }
        };
    for (method_name, expected_signature) in [
        (
            "focus_children",
            "fnfocus_children(&mutself)->Result<Option<DisplayModel>,ApplicationError>",
        ),
        (
            "focus_deepest",
            "fnfocus_deepest(&mutself)->Result<Option<DisplayModel>,ApplicationError>",
        ),
        (
            "next_up",
            "fnnext_up(&mutself,name:&str,estimated_minutes:Option<i64>)->Result<NextUpResult,ApplicationError>",
        ),
    ] {
        let signatures = direct_method_signature_regions(trait_region, method_name);
        if signatures.len() != 1
            || normalized_method_signature(signatures.first().copied().unwrap_or_default())
                != expected_signature
        {
            violations.push(format!(
                "TaskTreeCommandContext::{method_name} must have signature {expected_signature}"
            ));
        }
    }
    let trait_code = compact_code(trait_region);
    for forbidden in ["SchronuWriter", "supports_ansi_color"] {
        if trait_code.contains(forbidden) {
            violations.push(format!("TaskTreeCommandContext retains {forbidden}"));
        }
    }

    let handler_offsets =
        top_level_function_definition_offsets(handler_source, "handle_task_tree_command");
    if handler_offsets.len() != 1 {
        violations.push(format!(
            "handle_task_tree_command must have exactly one product definition; found {}",
            handler_offsets.len()
        ));
    } else {
        let handler_code = compact_code(function_region_from_offset(
            handler_source,
            handler_offsets[0],
        ));
        for required in [
            "semantic_display=context.focus_children()?",
            "semantic_display=context.focus_deepest()?",
            "matchcontext.next_up(name,*estimated_minutes)?",
            "NextUpResult::ReportedError(error)=>Some(DisplayModel::Message",
            "level:MessageLevel::Error",
        ] {
            if !handler_code.contains(required) {
                violations.push(format!("task-tree handler missing {required}"));
            }
        }
        for forbidden in ["&mutdisplay", "DisplayRecorder::", "supports_ansi_color"] {
            if handler_code.contains(forbidden) {
                violations.push(format!("task-tree handler retains {forbidden}"));
            }
        }
    }

    let runtime_struct = match unique_top_level_item_region(
        context_source,
        "struct RuntimeTaskTreeCommandContext",
    ) {
        Ok(region) => region,
        Err(error) => {
            violations.push(error);
            return violations;
        }
    };
    if compact_code(runtime_struct).contains("supports_ansi_color") {
        violations.push("RuntimeTaskTreeCommandContext retains supports_ansi_color".to_string());
    }

    for implementation_prefix in [
        "impl TaskTreeCommandContext for RuntimeTaskTreeCommandContext",
        "impl TaskTreeCommandContext for CliCommandContext",
    ] {
        let implementation =
            match unique_top_level_item_region(context_source, implementation_prefix) {
                Ok(region) => region,
                Err(error) => {
                    violations.push(error);
                    continue;
                }
            };
        let implementation_code = compact_code(implementation);
        for forbidden in ["SchronuWriter", "supports_ansi_color"] {
            if implementation_code.contains(forbidden) {
                violations.push(format!("{implementation_prefix} retains {forbidden}"));
            }
        }
        for method_name in ["focus_children", "focus_deepest", "next_up"] {
            let signatures = direct_method_signature_regions(implementation, method_name);
            if signatures.len() != 1 {
                violations.push(format!(
                    "{implementation_prefix} must define {method_name} exactly once; found {}",
                    signatures.len()
                ));
                continue;
            }
            let signature = normalized_method_signature(signatures[0]);
            let expected_return = if method_name == "next_up" {
                "->Result<NextUpResult,ApplicationError>"
            } else {
                "->Result<Option<DisplayModel>,ApplicationError>"
            };
            if !signature.ends_with(expected_return) {
                violations.push(format!(
                    "{implementation_prefix}::{method_name} must return typed optional display"
                ));
            }
        }
    }

    violations
}

fn project_recorder_dependency_violations(sources: &[ControllerProductSource]) -> Vec<String> {
    let mut violations = Vec::new();
    for function_name in [
        "handle_project_command",
        "handle_breakdown_split_command",
        "execute_breakdown",
        "execute_split",
        "execute_create_repetition_task",
    ] {
        let (_, region) = match unique_function_region(sources, function_name) {
            Ok(definition) => definition,
            Err(error) => {
                violations.push(error);
                continue;
            }
        };
        let code = code_only(region);
        violations.extend(output_dependency_violations(function_name, region));
        for forbidden in ["DisplayRecorder", "DisplayModel::Legacy", ".model()"] {
            if code.contains(forbidden) {
                violations.push(format!(
                    "{function_name} retains legacy output: {forbidden}"
                ));
            }
        }
    }
    violations
}

fn finish_recorder_dependency_violations(sources: &[ControllerProductSource]) -> Vec<String> {
    let mut violations = Vec::new();
    let source_for = |file_name: &str| {
        sources
            .iter()
            .find(|source| {
                source.path.file_name().and_then(|name| name.to_str()) == Some(file_name)
            })
            .unwrap_or_else(|| panic!("missing controller product module: {file_name}"))
    };
    let handler_source = &source_for("handler.rs").text;
    let context_source = &source_for("command_context.rs").text;

    let trait_region =
        match unique_top_level_item_region(handler_source, "trait FinishPlacementCommandContext") {
            Ok(region) => region,
            Err(error) => {
                violations.push(error);
                return violations;
            }
        };
    let trait_code = compact_code(trait_region);
    for forbidden in ["SchronuWriter", "supports_ansi_color"] {
        if trait_code.contains(forbidden) {
            violations.push(format!("FinishPlacementCommandContext retains {forbidden}"));
        }
    }
    let show_signatures = direct_method_signature_regions(trait_region, "show_focused_tree");
    if show_signatures.len() != 1
        || normalized_method_signature(show_signatures[0])
            != "fnshow_focused_tree(&mutself)->Result<TreeDisplay,ApplicationError>"
    {
        violations.push(
            "FinishPlacementCommandContext::show_focused_tree must return TreeDisplay".to_string(),
        );
    }

    let (_, handler_region) =
        match unique_function_region(sources, "handle_finish_placement_command") {
            Ok(definition) => definition,
            Err(error) => {
                violations.push(error);
                return violations;
            }
        };
    violations.extend(output_dependency_violations(
        "handle_finish_placement_command",
        handler_region,
    ));
    let handler_code = compact_code(handler_region);
    for forbidden in [
        "DisplayRecorder",
        "DisplayModel::Legacy",
        "supports_ansi_color",
        "&mutdisplay",
        ".model()",
    ] {
        if handler_code.contains(forbidden) {
            violations.push(format!("finish handler retains legacy output: {forbidden}"));
        }
    }
    for required in [
        "DisplayModel::Tree",
        "DisplayModel::Pack",
        "DisplayModel::Flatten",
    ] {
        if !handler_code.contains(required) {
            violations.push(format!("finish handler missing typed path: {required}"));
        }
    }

    let context_impl = match unique_top_level_item_region(
        context_source,
        "impl FinishPlacementCommandContext for CliCommandContext",
    ) {
        Ok(region) => region,
        Err(error) => {
            violations.push(error);
            return violations;
        }
    };
    let context_impl_code = compact_code(context_impl);
    for forbidden in [
        "SchronuWriter",
        "DisplayRecorder",
        "render_display_model",
        "supports_ansi_color",
        ".flush(",
    ] {
        if context_impl_code.contains(forbidden) {
            violations.push(format!("Finish context impl retains {forbidden}"));
        }
    }
    let context_show_signatures =
        direct_method_signature_regions(context_impl, "show_focused_tree");
    if context_show_signatures.len() != 1
        || normalized_method_signature(context_show_signatures[0])
            != "fnshow_focused_tree(&mutself)->Result<TreeDisplay,ApplicationError>"
    {
        violations.push("CliCommandContext::show_focused_tree must return TreeDisplay".to_string());
    }

    let context_struct =
        match unique_top_level_item_region(context_source, "struct CliCommandContext") {
            Ok(region) => region,
            Err(error) => {
                violations.push(error);
                return violations;
            }
        };
    if compact_code(context_struct).contains("supports_ansi_color") {
        violations.push("CliCommandContext retains finish ANSI capability".to_string());
    }

    violations
}

fn output_dependency_violations(region_name: &str, region: &str) -> Vec<String> {
    let code = code_only(region);
    let mut violations = Vec::new();
    let signature = code.split_once('{').map_or(code.as_str(), |(head, _)| head);

    if signature.contains("SchronuWriter")
        || signature.contains("std::io::Write")
        || contains_identifier(signature, "Write")
    {
        violations.push(format!("writer type in {region_name}"));
    }

    let compact_code = code
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    for forbidden in [
        "print!(",
        "println!(",
        "eprintln!(",
        "write!(",
        "writeln!(",
        ".write_all(",
        ".flush(",
        "writeln_newline(",
        "render_display_model(",
    ] {
        if compact_code.contains(forbidden) {
            violations.push(format!("{forbidden} in {region_name}"));
        }
    }

    violations
}

fn view_writer_dependency_violations(source: &str) -> Vec<String> {
    let mut violations = Vec::new();
    for offset in top_level_product_function_offsets(source) {
        let region = function_region_from_offset(source, offset);
        let first_line = region.lines().next().unwrap_or("<unknown function>");
        violations.extend(output_dependency_violations(first_line, region));
    }
    violations
}

fn strip_top_level_visibility(line: &str) -> &str {
    if let Some(declaration) = line.strip_prefix("pub ") {
        declaration
    } else if let Some(visibility) = line.strip_prefix("pub(") {
        visibility
            .split_once(") ")
            .map_or(line, |(_, declaration)| declaration)
    } else {
        line
    }
}

fn top_level_struct_name(region: &str) -> Option<&str> {
    let declaration = strip_top_level_visibility(region.lines().next()?);
    let rest = declaration.strip_prefix("struct ")?;
    let name_end = rest
        .find(|character: char| {
            character.is_whitespace() || matches!(character, '<' | '(' | '{' | ';')
        })
        .unwrap_or(rest.len());
    Some(&rest[..name_end])
}

const MOVED_DATETIME_INTERPRETATION_SYMBOLS: &[&str] = &[
    "decide_time_values",
    "decide_finish_time_values",
    "decide_naive_datetime_values",
    "resolve_date_and_time",
    "resolve_upcoming_mmdd",
    "resolve_deadline_date",
    "resolve_deadline_time",
    "resolve_show_all_pattern",
    "resolve_upcoming_clear_or_gather_day",
    "resolve_dated_clear_or_gather_end_naive",
    "parse_clear_or_gather_defer_to_datetime",
    "parse_dated_clear_or_gather_time_range",
    "defer_logical_date_target",
    "seconds_until_next_logical_date_start_with_offset",
];

const MOVED_DOMAIN_MUTATION_SYMBOLS: &[&str] = &[
    "execute_start_new_project",
    "execute_make_appointment",
    "execute_breakdown_sequentially",
    "execute_breakdown",
    "execute_split",
    "execute_create_repetition_task",
    "execute_clear_or_gather",
    "execute_next_up",
    "execute_defer",
    "execute_defer_expression",
    "execute_extrude_with_config",
    "execute_defer_routine",
    "execute_defer_all_frequent_routines",
    "execute_set_arrange_children_work_minutes",
    "set_focused_task_actual_work_minutes",
    "set_focused_task_priority",
];

const MOVED_DISPLAY_CALCULATION_SYMBOLS: &[&str] = &[
    "get_weekday_jp",
    "task_list_search_text",
    "get_adjustable_prefix_label",
    "calculate_daily_band_durations",
    "replace_task_list_icon",
    "project_category_summary_index",
    "summarize_scheduled_work_seconds_by_project_category",
    "format_scheduled_work_seconds_by_project_category",
    "task_category_work_seconds",
    "calculate_project_category_denominator_seconds",
    "advance_display_datetime_cursor",
    "sort_task_list_display_rows",
    "mark_give_up_candidate_rows",
    "mark_give_up_candidate_rows_by_date",
    "calculate_rho_metrics",
    "calculate_lq_opt",
    "build_tree_display",
    "build_ancestor_tree_display",
    "build_show_all_tasks_display_with_config",
    "build_focus_header_display",
    "build_focus_timing_display",
    "build_leaf_tree_display",
];

fn runtime_final_boundary_violations(sources: &[ControllerProductSource]) -> Vec<String> {
    let mut violations = Vec::new();
    let source_for = |file_name: &str| {
        sources
            .iter()
            .find(|source| {
                source.path.file_name().and_then(|name| name.to_str()) == Some(file_name)
            })
            .unwrap_or_else(|| panic!("missing controller product module: {file_name}"))
    };
    let runtime = source_for("runtime.rs");
    let renderer = source_for("renderer.rs");

    for offset in top_level_item_definition_offsets(&runtime.text, "struct") {
        let region = function_region_from_offset(&runtime.text, offset);
        if let Some(name) = top_level_struct_name(region) {
            if name.ends_with("Context") {
                violations.push(format!("runtime.rs owns Context-named struct {name}"));
            }
        }
    }

    for offset in top_level_item_definition_offsets(&runtime.text, "impl") {
        let region = function_region_from_offset(&runtime.text, offset);
        let code = code_only(region);
        let signature = code.split_once('{').map_or(code.as_str(), |(head, _)| head);
        if compact_code(signature).contains("CommandContextfor") {
            violations.push(format!(
                "runtime.rs owns CommandContext implementation: {}",
                signature.trim()
            ));
        }
    }

    for (responsibility, function_names) in [
        (
            "command argument datetime interpretation",
            MOVED_DATETIME_INTERPRETATION_SYMBOLS,
        ),
        ("domain mutation", MOVED_DOMAIN_MUTATION_SYMBOLS),
        ("display calculation", MOVED_DISPLAY_CALCULATION_SYMBOLS),
    ] {
        for function_name in function_names {
            if !top_level_function_definition_offsets(&runtime.text, function_name).is_empty() {
                violations.push(format!(
                    "runtime.rs regressed moved {responsibility} symbol {function_name}"
                ));
            }
        }
    }

    let renderer_has_legacy_variant =
        match unique_top_level_item_region(&renderer.text, "enum DisplayModel") {
            Ok(display_model) => {
                let code = compact_code(display_model);
                code.contains("Legacy{") || code.contains("Legacy(")
            }
            Err(error) => {
                violations.push(error);
                false
            }
        };
    for source in sources {
        let code = code_only(&source.text);
        let compact_product_code = compact_code(&code);
        let mut legacy_markers = Vec::new();
        for legacy_type in ["DisplayFragment", "DisplayRecorder"] {
            if contains_identifier(&code, legacy_type) {
                legacy_markers.push(legacy_type);
            }
        }
        if compact_product_code.contains("DisplayModel::Legacy")
            || (source.path.file_name().and_then(|name| name.to_str()) == Some("renderer.rs")
                && renderer_has_legacy_variant)
        {
            legacy_markers.push("DisplayModel::Legacy");
        }
        if !legacy_markers.is_empty() {
            violations.push(format!(
                "{} retains legacy display boundary: {}",
                source.path.display(),
                legacy_markers.join(", ")
            ));
        }
    }

    match unique_function_region(sources, "apply_command_outcome") {
        Ok((path, region)) => {
            if path.file_name().and_then(|name| name.to_str()) != Some("runtime.rs") {
                violations
                    .push("apply_command_outcome must remain runtime I/O coordination".into());
            }
            let code = compact_code(region);
            for required in [
                "render_display_model_with_mode(",
                "RenderMode::Flushed",
                "RenderMode::Unflushed",
            ] {
                if !code.contains(required) {
                    violations.push(format!(
                        "runtime outcome coordination missing mode boundary {required}"
                    ));
                }
            }
            for forbidden in ["DisplayModel::flush()", "render_display_model(", ".flush("] {
                if code.contains(forbidden) {
                    violations.push(format!(
                        "runtime outcome coordination performs renderer work via {forbidden}"
                    ));
                }
            }
        }
        Err(error) => violations.push(error),
    }

    let render_mode_owners = sources
        .iter()
        .flat_map(|source| {
            top_level_item_definition_offsets(&source.text, "enum RenderMode")
                .into_iter()
                .map(move |_| source.path.as_path())
        })
        .collect::<Vec<_>>();
    if render_mode_owners.len() != 1
        || render_mode_owners[0]
            .file_name()
            .and_then(|name| name.to_str())
            != Some("renderer.rs")
    {
        violations.push("RenderMode must be defined exactly once by renderer.rs".to_string());
    }
    match unique_function_region(sources, "render_display_model_with_mode") {
        Ok((path, region)) => {
            if path.file_name().and_then(|name| name.to_str()) != Some("renderer.rs") {
                violations.push("mode-aware rendering must be owned by renderer.rs".to_string());
            }
            let code = compact_code(region);
            for required in ["render_display_model(", "RenderMode::Flushed", ".flush("] {
                if !code.contains(required) {
                    violations.push(format!(
                        "renderer mode boundary missing rendering responsibility {required}"
                    ));
                }
            }
        }
        Err(error) => violations.push(error),
    }

    violations
}

const COMPONENT_SUB_HANDLERS: &[&str] = &[
    "handle",
    "handle_project_command",
    "handle_breakdown_split_command",
    "handle_task_attribute_command",
    "handle_defer_command",
    "handle_finish_placement_command",
    "handle_task_tree_command",
];

fn interactive_unified_handler_violations(
    interactive_region: &str,
    parsed_dispatcher_region: &str,
) -> Vec<String> {
    let mut legacy_dispatches = Vec::<String>::new();
    let interactive_code = compact_code(interactive_region);
    if !contains_direct_function_call(&interactive_code, "execute_parsed") {
        legacy_dispatches.push("interactive missing execute_parsed(".to_string());
    }
    for sub_handler in COMPONENT_SUB_HANDLERS {
        if contains_direct_function_call(&interactive_code, sub_handler) {
            legacy_dispatches.push(format!("interactive direct {sub_handler}("));
        }
    }
    if contains_identifier(&interactive_code, "RuntimeDeferCommandContext") {
        legacy_dispatches.push("interactive direct RuntimeDeferCommandContext".to_string());
    }

    let dispatcher_code = compact_code(parsed_dispatcher_region);
    if !contains_direct_function_call(&dispatcher_code, "handle_command") {
        legacy_dispatches.push("execute_parsed missing handle_command(".to_string());
    }
    for sub_handler in COMPONENT_SUB_HANDLERS {
        if contains_direct_function_call(&dispatcher_code, sub_handler) {
            legacy_dispatches.push(format!("execute_parsed direct {sub_handler}("));
        }
    }
    if contains_identifier(&dispatcher_code, "RuntimeDeferCommandContext") {
        legacy_dispatches.push("execute_parsed direct RuntimeDeferCommandContext".to_string());
    }

    if legacy_dispatches.is_empty() {
        Vec::new()
    } else {
        vec![format!(
            "interactive Verify以外の製品経路が統一handle_commandを通らない: {}",
            legacy_dispatches.join(", ")
        )]
    }
}

fn contains_direct_function_call(code: &str, function_name: &str) -> bool {
    code.match_indices(function_name).any(|(offset, _)| {
        let has_direct_call_prefix = code[..offset].chars().next_back().is_none_or(|character| {
            character != '.' && !(character.is_ascii_alphanumeric() || character == '_')
        });
        if !has_direct_call_prefix {
            return false;
        }

        let suffix = &code[offset + function_name.len()..];
        if suffix.starts_with('(') {
            return true;
        }
        let Some(generic_arguments) = suffix.strip_prefix("::<") else {
            return false;
        };
        let mut angle_depth = 1usize;
        for (index, byte) in generic_arguments.bytes().enumerate() {
            match byte {
                b'<' => angle_depth += 1,
                b'>' => {
                    if index > 0 && generic_arguments.as_bytes()[index - 1] == b'-' {
                        continue;
                    }
                    angle_depth -= 1;
                    if angle_depth == 0 {
                        return generic_arguments.as_bytes().get(index + 1) == Some(&b'(');
                    }
                }
                _ => {}
            }
        }
        false
    })
}

#[test]
fn interactive_terminal_driver_is_isolated_from_runtime() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let interactive_path = manifest_dir.join("src/adapter/controller/schronu/interactive.rs");
    assert!(
        interactive_path.is_file(),
        "interactive terminal driver module must exist"
    );

    let interactive_source =
        fs::read_to_string(interactive_path).expect("interactive source must be readable");
    let runtime_source = include_str!("runtime.rs");

    for required in [
        "termion::event::Key",
        "termion::input::TermRead",
        "termion::raw::IntoRawMode",
        "termion::raw::RawTerminal",
        "std::io::stdin().keys()",
        "recv_timeout",
        "fn render_prompt(",
        "fn get_byte_offset_for_insert(",
        "fn get_byte_offset_for_deletion(",
        "termion::cursor",
        "termion::clear",
    ] {
        assert!(
            interactive_source.contains(required),
            "interactive driver must own {required}"
        );
        assert!(
            !runtime_source.contains(required),
            "runtime must not retain terminal detail {required}"
        );
    }
}

#[test]
fn interactiveとnoninteractiveは単一のtyped_parserを共有する() {
    let product_sources = controller_product_sources();
    unique_function_region(&product_sources, "parse_command")
        .expect("controller must keep one shared typed parser definition");
    for parse_mode in ["Interactive", "NonInteractive"] {
        assert!(
            product_sources.iter().any(|source| source
                .text
                .split_whitespace()
                .collect::<String>()
                .contains(&format!("parse_command(command,ParseMode::{parse_mode})"))),
            "{parse_mode} product entry must call the shared typed parser"
        );
    }
}

#[test]
fn interactiveとnoninteractiveは単一のparsed_command_dispatcherを共有する() {
    let product_sources = controller_product_sources();
    let (_, parsed_dispatcher_region) = unique_function_region(&product_sources, "execute_parsed")
        .expect("controller must define exactly one shared parsed-command dispatcher");
    let (_, interactive_region) =
        unique_function_region(&product_sources, "execute_interactive_command")
            .unwrap_or_else(|error| panic!("{error}"));
    let mut violations =
        interactive_unified_handler_violations(interactive_region, parsed_dispatcher_region);

    for entry_function in [
        "execute_non_interactive_command_at",
        "execute_interactive_command",
    ] {
        let (_, entry_region) = unique_function_region(&product_sources, entry_function)
            .unwrap_or_else(|error| panic!("{error}"));
        let code = compact_code(entry_region);
        if !contains_direct_function_call(&code, "execute_parsed") {
            violations.push(format!("{entry_function} missing execute_parsed("));
        }
        if !code.contains("CommandKind::Verify") {
            violations.push(format!(
                "{entry_function} missing intentional Verify dispatch"
            ));
        }
        let code_without_verify = code.replace("CommandKind::Verify", "");
        if code_without_verify.contains("Command::")
            || code_without_verify.contains("CommandKind::")
        {
            violations.push(format!(
                "{entry_function} retains Verify以外のtyped command variant reference"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "shared parsed-command dispatcher violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn interactive統一handler_scannerはcommentとstringを無視する() {
    let interactive_fixture = r#"
fn execute_interactive_command() {
    let comment_shaped = "handle(&parsed); handle_defer_command(&parsed, context); RuntimeDeferCommandContext";
    /* handle(&parsed); */
    // handle_defer_command(&parsed, context);
    driver.handle(&parsed);
    execute_parsed::<C>(&parsed);
}
"#;
    let dispatcher_fixture = r#"
fn execute_parsed() {
    let comment_shaped = "handle(&parsed); handle_defer_command(&parsed, context); RuntimeDeferCommandContext";
    handle_command::<fn() -> bool>(&parsed, context);
}
"#;

    assert_eq!(
        interactive_unified_handler_violations(interactive_fixture, dispatcher_fixture),
        Vec::<String>::new()
    );
}

#[test]
fn interactive統一handler_scannerはdirect_handleとdefer_sub_handlerを独立検出する() {
    let direct_handle = r#"
fn execute_interactive_command() {
    execute_parsed(&parsed);
    handle(&parsed);
}
"#;
    let defer_sub_handler = r#"
fn execute_interactive_command() {
    execute_parsed(&parsed);
    let mut context = RuntimeDeferCommandContext { repository };
    handle_defer_command(&parsed, &mut context);
}
"#;
    let valid_dispatcher = "fn execute_parsed() { handle_command(&parsed, context); }";

    let direct_violations =
        interactive_unified_handler_violations(direct_handle, valid_dispatcher).join("\n");
    assert!(direct_violations.contains("interactive direct handle("));
    assert!(!direct_violations.contains("handle_defer_command("));
    assert!(!direct_violations.contains("RuntimeDeferCommandContext"));

    let defer_violations =
        interactive_unified_handler_violations(defer_sub_handler, valid_dispatcher).join("\n");
    assert!(!defer_violations.contains("interactive direct handle("));
    assert!(defer_violations.contains("interactive direct handle_defer_command("));
    assert!(defer_violations.contains("interactive direct RuntimeDeferCommandContext"));
}

#[test]
fn interactive統一handler_scannerは全7_component_sub_handlerのturbofish_callを個別検出する() {
    let valid_dispatcher = "fn execute_parsed() { handle_command::<C>(&parsed, context); }";

    for sub_handler in [
        "handle",
        "handle_project_command",
        "handle_breakdown_split_command",
        "handle_task_attribute_command",
        "handle_defer_command",
        "handle_finish_placement_command",
        "handle_task_tree_command",
    ] {
        let interactive_fixture = format!(
            "fn execute_interactive_command() {{ execute_parsed::<C>(&parsed); {sub_handler}::<C>(&parsed, context); }}"
        );
        let violations =
            interactive_unified_handler_violations(&interactive_fixture, valid_dispatcher)
                .join("\n");
        assert!(
            violations.contains(&format!("interactive direct {sub_handler}(")),
            "missing turbofish direct-call detection for {sub_handler}: {violations}"
        );
    }
}

#[test]
fn interactive統一handler_scannerはmethodとprefixを必須callと誤認しない() {
    let interactive_lookalikes = r#"
fn execute_interactive_command() {
    driver.execute_parsed::<C>(&parsed);
    execute_parsed_wrapper::<C>(&parsed);
}
"#;
    let dispatcher_lookalikes = r#"
fn execute_parsed() {
    driver.handle_command::<C>(&parsed, context);
    handle_command_wrapper::<C>(&parsed, context);
}
"#;

    let violations =
        interactive_unified_handler_violations(interactive_lookalikes, dispatcher_lookalikes)
            .join("\n");
    assert!(violations.contains("interactive missing execute_parsed("));
    assert!(violations.contains("execute_parsed missing handle_command("));
}

#[test]
fn interactive統一handler_scannerはparsed_dispatcherのhandler欠如と直接sub_handlerを検出する() {
    let valid_interactive = "fn execute_interactive_command() { execute_parsed(&parsed); }";
    let missing_handler = "fn execute_parsed() {}";
    let direct_sub_handlers = r#"
fn execute_parsed() {
    handle_command(&parsed, context);
    handle(&parsed);
    handle_defer_command(&parsed, context);
    let context = RuntimeDeferCommandContext { repository };
}
"#;

    let missing_violations =
        interactive_unified_handler_violations(valid_interactive, missing_handler).join("\n");
    assert!(missing_violations.contains("execute_parsed missing handle_command("));

    let direct_violations =
        interactive_unified_handler_violations(valid_interactive, direct_sub_handlers).join("\n");
    for expected in [
        "execute_parsed direct handle(",
        "execute_parsed direct handle_defer_command(",
        "execute_parsed direct RuntimeDeferCommandContext",
    ] {
        assert!(direct_violations.contains(expected));
    }
}

#[test]
fn verify製品分岐は意味的modelをrendererへ渡す() {
    let product_sources = controller_product_sources();

    let (_, non_interactive_source) =
        unique_function_region(&product_sources, "execute_non_interactive_command_at")
            .unwrap_or_else(|error| panic!("{error}"));
    assert!(non_interactive_source.contains("execute_verify_command("));
    assert!(!non_interactive_source.contains("verify_display_model("));
    assert!(!non_interactive_source.contains("render_display_model("));

    let (_, verify_source) = unique_function_region(&product_sources, "execute_verify_command")
        .expect("non-interactive Verify must have one private product I/O boundary");
    for required in [
        "verify_display_model(",
        "render_display_model_with_mode(",
        "RenderMode::Flushed",
    ] {
        assert!(verify_source.contains(required));
    }
    for forbidden in ["println!(", "eprintln!(", "render_verify_flush("] {
        assert!(!verify_source.contains(forbidden));
    }

    let (_, interactive_source) =
        unique_function_region(&product_sources, "execute_interactive_command")
            .unwrap_or_else(|error| panic!("{error}"));
    assert!(interactive_source.contains("RenderMode::Flushed"));
    assert!(interactive_source.contains("render_display_model_with_mode("));
    assert!(!interactive_source.contains("DisplayModel::flush()"));
    assert!(
        !interactive_source.contains("verify_display_model(Ok"),
        "interactive Verify must not add a success body"
    );

    for (entry_function, entry_source) in [
        ("execute_non_interactive_command_at", non_interactive_source),
        ("execute_interactive_command", interactive_source),
    ] {
        for forbidden in ["println!(", "eprintln!(", "render_verify_flush("] {
            assert!(
                !entry_source.contains(forbidden),
                "{entry_function} must not retain raw Verify output via {forbidden}"
            );
        }
    }

    assert!(
        product_sources.iter().all(|source| {
            top_level_function_definition_offsets(&source.text, "render_verify_flush").is_empty()
        }),
        "legacy Verify flush helper must be removed after semantic renderer migration"
    );

    let (_, report_source) = unique_function_region(&product_sources, "report_run_result")
        .expect("run-result reporter must remain the repository Verify error boundary");
    assert!(report_source.contains("error_display_model("));
    assert!(report_source.contains("render_plain_display_model("));
    assert!(!report_source.contains("println!("));
    assert!(!report_source.contains("eprintln!("));
}

#[test]
fn runtimeはio調停だけを所有する() {
    let product_sources = controller_product_sources();
    let source_for = |file_name: &str| {
        product_sources
            .iter()
            .find(|source| {
                source.path.file_name().and_then(|name| name.to_str()) == Some(file_name)
            })
            .unwrap_or_else(|| panic!("missing controller product module: {file_name}"))
    };
    let runtime = source_for("runtime.rs");
    let view = source_for("view.rs");
    let mut violations = Vec::new();

    for item_prefix in [
        "trait FocusDisplaySource",
        "struct TaskFocusDisplaySource",
        "impl FocusDisplaySource for TaskFocusDisplaySource",
    ] {
        let runtime_item_offsets = top_level_item_definition_offsets(&runtime.text, item_prefix);
        let view_item_offsets = top_level_item_definition_offsets(&view.text, item_prefix);
        if !runtime_item_offsets.is_empty() || view_item_offsets.len() != 1 {
            violations.push(format!(
                "{item_prefix} must be owned exactly once by view.rs and absent from runtime.rs"
            ));
        }
        if let [view_item_offset] = view_item_offsets.as_slice() {
            let view_item = function_region_from_offset(&view.text, *view_item_offset);
            let code = compact_code(&code_only(view_item));
            for forbidden in ["SchronuWriter", "render_display_model(", ".flush("] {
                if code.contains(forbidden) {
                    violations.push(format!(
                        "{item_prefix} must remain a pure view source without {forbidden}"
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "runtime Focus source ownership violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn runtime最終責務境界はsemantic_rendererとio調停を分離する() {
    let product_sources = controller_product_sources();
    let mut violations = runtime_final_boundary_violations(&product_sources);
    violations.extend(handler_entry_boundary_violations(&product_sources));
    let (_, save_before_exit_source) =
        unique_function_region(&product_sources, "try_save_before_exit")
            .expect("interactive exit save must remain a unique runtime boundary");
    let save_before_exit_code = compact_code(&code_only(save_before_exit_source));
    for required in [
        "error_display_model(",
        "render_display_model_with_mode(",
        "RenderMode::Flushed",
    ] {
        if !save_before_exit_code.contains(required) {
            violations.push(format!(
                "interactive exit save error must use semantic renderer boundary {required}"
            ));
        }
    }
    for forbidden in ["writeln_newline(", ".flush("] {
        if save_before_exit_code.contains(forbidden) {
            violations.push(format!(
                "interactive exit save must not own presentation operation {forbidden}"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "runtime final responsibility boundary violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn handler入口境界scannerはruntime依存のuse表現とhelper迂回を検出する() {
    for dependency in [
        "use super::{runtime::CommandError};",
        "use crate::adapter::controller::schronu::runtime::CommandError as OuterError;",
        "use super::runtime as outer;",
        "type OuterError = super::runtime::CommandError;",
        "fn convert(error: super::runtime::CommandError) {}",
    ] {
        let sources = vec![
            ControllerProductSource {
                path: PathBuf::from("command_context.rs"),
                text: dependency.to_string(),
            },
            ControllerProductSource {
                path: PathBuf::from("runtime.rs"),
                text: "fn execute_parsed(parsed_command: &Command) { handle_command(parsed_command); }"
                    .to_string(),
            },
        ];
        assert!(handler_entry_boundary_violations(&sources)
            .join("\n")
            .contains("must not depend"));
    }

    let sources = vec![
        ControllerProductSource {
            path: PathBuf::from("command_context.rs"),
            text: "use super::handler::HandlerError;".to_string(),
        },
        ControllerProductSource {
            path: PathBuf::from("runtime.rs"),
            text: r#"
fn execute_parsed(command: &Command) {
    check_input(command);
    handle_command(command);
}
"#
            .to_string(),
        },
    ];
    assert!(handler_entry_boundary_violations(&sources)
        .join("\n")
        .contains("must not inspect or delegate"));
}

#[test]
fn runtime最終責務scannerは任意名のio調停とnon_codeを許可する() {
    let sources = vec![
        ControllerProductSource {
            path: PathBuf::from("runtime.rs"),
            text: r#"
struct InteractiveRepositoryState;
struct FocusRenderState;
const EXAMPLE: &str = "pub(crate) struct HiddenContext; DisplayRecorder; DisplayModel::Legacy";
/*
pub(super) struct BlockContext;
impl ProjectCommandContext for BlockContext {}
fn execute_breakdown() {}
let display = DisplayModel::Legacy { fragments };
*/
fn arbitrary_external_io() {}
fn coordinate_repository_transaction() {}
fn apply_command_outcome() {
    let mode = if interactive { RenderMode::Unflushed } else { RenderMode::Flushed };
    render_display_model_with_mode(writer, model, mode);
}
"#
            .to_string(),
        },
        ControllerProductSource {
            path: PathBuf::from("renderer.rs"),
            text: r#"
enum DisplayModel { Message }
enum RenderMode { Flushed, Unflushed }
fn render_display_model() {}
fn render_display_model_with_mode() {
    render_display_model(writer, model);
    if mode == RenderMode::Flushed { writer.flush(); }
}
"#
            .to_string(),
        },
        ControllerProductSource {
            path: PathBuf::from("handler.rs"),
            text: "// enum RenderMode { Flushed, Unflushed }\n".to_string(),
        },
        ControllerProductSource {
            path: PathBuf::from("view.rs"),
            text: "const LEGACY: &str = \"DisplayModel::Legacy\";\n".to_string(),
        },
    ];

    assert_eq!(
        runtime_final_boundary_violations(&sources),
        Vec::<String>::new()
    );
}

#[test]
fn runtime最終責務scannerはcontext命名structと移動済みsymbolとlegacyとmode違反を検出する() {
    let sources = vec![
        ControllerProductSource {
            path: PathBuf::from("runtime.rs"),
            text: r#"
struct PrivateContext;
pub struct PublicContext;
pub(super) struct SuperContext;
pub(crate) struct CrateContext;
pub(in crate) struct InContext;
impl ProjectCommandContext for PublicContext {}
impl /* { non-code brace */ RepetitionCommandContext for SuperContext {}
fn decide_time_values() {}
fn execute_breakdown() {}
fn build_tree_display() {}
fn apply_command_outcome() {
    render_display_model(writer, model);
    writer.flush();
}
"#
            .to_string(),
        },
        ControllerProductSource {
            path: PathBuf::from("renderer.rs"),
            text: r#"
enum DisplayModel { Legacy { fragments: Vec<DisplayFragment> } }
enum RenderMode { Flushed, Unflushed }
struct DisplayRecorder;
enum DisplayFragment { Flush }
fn render_display_model() {}
fn render_display_model_with_mode() {
    render_display_model(writer, model);
}
"#
            .to_string(),
        },
        ControllerProductSource {
            path: PathBuf::from("handler.rs"),
            text: "enum RenderMode { Flushed, Unflushed }\n".to_string(),
        },
        ControllerProductSource {
            path: PathBuf::from("view.rs"),
            text: "pub(crate) enum RenderMode { Flushed, Unflushed }\n".to_string(),
        },
    ];

    let violations = runtime_final_boundary_violations(&sources).join("\n");
    for expected in [
        "Context-named struct PrivateContext",
        "Context-named struct PublicContext",
        "Context-named struct SuperContext",
        "Context-named struct CrateContext",
        "Context-named struct InContext",
        "CommandContext implementation",
        "regressed moved command argument datetime interpretation symbol decide_time_values",
        "regressed moved domain mutation symbol execute_breakdown",
        "regressed moved display calculation symbol build_tree_display",
        "legacy display boundary: DisplayFragment, DisplayRecorder, DisplayModel::Legacy",
        "RenderMode must be defined exactly once by renderer.rs",
        "runtime outcome coordination missing mode boundary RenderMode::Flushed",
        "runtime outcome coordination missing mode boundary RenderMode::Unflushed",
        "runtime outcome coordination performs renderer work via render_display_model(",
        "runtime outcome coordination performs renderer work via .flush(",
        "renderer mode boundary missing rendering responsibility RenderMode::Flushed",
        "renderer mode boundary missing rendering responsibility .flush(",
    ] {
        assert!(
            violations.contains(expected),
            "missing final-boundary mutation detection {expected}:\n{violations}"
        );
    }
    assert_eq!(
        violations.matches("CommandContext implementation").count(),
        2,
        "both plain and comment-brace CommandContext impls must be detected:\n{violations}"
    );
}

#[test]
fn view表示計算はwriterと直接出力に依存しない() {
    let product_sources = controller_product_sources();
    let view = product_sources
        .iter()
        .find(|source| source.path.file_name().and_then(|name| name.to_str()) == Some("view.rs"))
        .expect("view.rs must be a controller product module");
    let violations = view_writer_dependency_violations(&view.text);

    let (_, builder_region) =
        unique_function_region(&product_sources, "build_show_all_tasks_display_with_config")
            .unwrap();
    for forbidden in [
        "let busy_s",
        "let s_for_rho1",
        "let s_for_non_repetitive_rho",
        "完了見込み日時は",
    ] {
        if builder_region.contains(forbidden) {
            panic!(
                "build_show_all_tasks_display_with_config must return typed metrics without legacy preformat: {forbidden}"
            );
        }
    }
    assert!(
        product_sources.iter().all(|source| {
            top_level_function_definition_offsets(
                &source.text,
                "execute_show_all_tasks_with_config",
            )
            .is_empty()
        }),
        "the legacy writer-based show-all view function must be removed"
    );

    assert!(
        violations.is_empty(),
        "view.rs must return typed models without writer/output dependencies:\n{}",
        violations.join("\n")
    );
}

#[test]
fn task_tree製品境界はcode領域だけでwriter非依存を検証する() {
    let product_sources = controller_product_sources();
    let source_for = |file_name: &str| {
        product_sources
            .iter()
            .find(|source| {
                source.path.file_name().and_then(|name| name.to_str()) == Some(file_name)
            })
            .unwrap_or_else(|| panic!("missing controller product module: {file_name}"))
    };
    let violations = task_tree_writer_free_boundary_violations(
        &source_for("handler.rs").text,
        &source_for("command_context.rs").text,
    );

    assert!(
        violations.is_empty(),
        "TaskTree writer-free boundary violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn project_breakdown_repetition製品経路はrecorderに依存しない() {
    let violations = project_recorder_dependency_violations(&controller_product_sources());

    assert!(
        violations.is_empty(),
        "Project/breakdown/repetition recorder dependency violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn finish製品経路はtyped_treeを返しrecorderに依存しない() {
    let violations = finish_recorder_dependency_violations(&controller_product_sources());

    assert!(
        violations.is_empty(),
        "Finish recorder dependency violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn task_tree境界scannerは非codeと対象外itemを無視する() {
    let handler_source = r###"
pub(super) trait TaskTreeCommandContext {
    // fn focus_children(&mut self, display: &mut dyn SchronuWriter);
    fn focus_children(&mut self) -> Result<Option<DisplayModel>, ApplicationError>;
    fn focus_deepest(&mut self) -> Result<Option<DisplayModel>, ApplicationError>;
    fn next_up(
        &mut self,
        name: &str,
        estimated_minutes: Option<i64>,
    ) -> Result<NextUpResult, ApplicationError>;
}

pub(super) fn handle_task_tree_command() {
    let quoted = "DisplayRecorder::with_ansi_color(context.supports_ansi_color())";
    let raw = r#"context.focus_children(&mut display)?"#;
    /* context.focus_deepest(&mut display)?; */
    semantic_display = context.focus_children()?;
    semantic_display = context.focus_deepest()?;
    semantic_display = match context.next_up(name, *estimated_minutes)? {
        NextUpResult::NoDisplay => None,
        NextUpResult::ReportedError(error) => Some(DisplayModel::Message {
            level: MessageLevel::Error,
            text: error.to_string(),
        }),
    };
}

fn unrelated_handler() {
    let mut display = DisplayRecorder::default();
    context.focus_children(&mut display);
}
"###;
    let context_source = r###"
pub(super) struct RuntimeTaskTreeCommandContext {
    value: bool,
    /* supports_ansi_color: bool, */
}

impl TaskTreeCommandContext for RuntimeTaskTreeCommandContext<'_> {
    fn focus_children(&mut self) -> Result<Option<DisplayModel>, ApplicationError> {
        let raw = r#"SchronuWriter supports_ansi_color"#;
        Ok(None)
    }
    fn focus_deepest(&mut self) -> Result<Option<DisplayModel>, ApplicationError> { Ok(None) }
    fn next_up(&mut self, name: &str, estimated_minutes: Option<i64>) -> Result<NextUpResult, ApplicationError> { Ok(NextUpResult::NoDisplay) }
}

impl TaskTreeCommandContext for CliCommandContext<'_> {
    fn focus_children(&mut self) -> Result<Option<DisplayModel>, ApplicationError> { Ok(None) }
    fn focus_deepest(&mut self) -> Result<Option<DisplayModel>, ApplicationError> { Ok(None) }
    fn next_up(&mut self, name: &str, estimated_minutes: Option<i64>) -> Result<NextUpResult, ApplicationError> { Ok(NextUpResult::NoDisplay) }
}

impl UnrelatedContext {
    fn focus_children(&mut self, display: &mut dyn SchronuWriter) {
        self.supports_ansi_color = true;
    }
}
"###;

    assert_eq!(
        task_tree_writer_free_boundary_violations(handler_source, context_source),
        Vec::<String>::new()
    );

    let bad_handler = handler_source.replace(
        "semantic_display = context.focus_children()?;",
        "semantic_display = context.focus_children(&mut display)?;",
    );
    assert!(
        task_tree_writer_free_boundary_violations(&bad_handler, context_source)
            .iter()
            .any(|violation| violation.contains("&mutdisplay"))
    );
}

#[test]
fn task_tree_signature_scannerは整形と末尾commaに依存しない() {
    let without_trailing_comma = r#"
trait Context {
    fn next_up(&mut self, name: &str, estimated_minutes: Option<i64>) -> Result<Option<DisplayModel>, ApplicationError>;
}
"#;
    let with_trailing_comma = r#"
trait Context {
    fn next_up(
        &mut self,
        name: &str,
        estimated_minutes: Option<i64>,
    ) -> Result<Option<DisplayModel>, ApplicationError>;
}
"#;

    let normalized = |source| {
        let signatures = direct_method_signature_regions(source, "next_up");
        assert_eq!(signatures.len(), 1);
        normalized_method_signature(signatures[0])
    };

    assert_eq!(
        normalized(without_trailing_comma),
        normalized(with_trailing_comma)
    );
}

#[test]
fn command_contextの製品task_list経路はwriter_free_builderへ直接委譲する() {
    let product_sources = controller_product_sources();
    let command_context = product_sources
        .iter()
        .find(|source| {
            source.path.file_name().and_then(|name| name.to_str()) == Some("command_context.rs")
        })
        .expect("command_context.rs must be a controller product module");
    let impl_offsets = top_level_item_definition_offsets(
        &command_context.text,
        "impl TaskTreeCommandContext for RuntimeTaskTreeCommandContext",
    );
    assert_eq!(
        impl_offsets.len(),
        1,
        "runtime task-tree context must have one product implementation"
    );
    let implementation = function_region_from_offset(&command_context.text, impl_offsets[0]);
    let methods = direct_impl_method_regions(implementation, "show_task_list");
    assert_eq!(
        methods.len(),
        1,
        "runtime task-tree context must have one direct show_task_list method"
    );
    let method_code = code_only(methods[0]);

    assert_eq!(
        method_code
            .matches("build_show_all_tasks_display_with_config(")
            .count(),
        1,
        "product TaskTree context must call the writer-free view builder directly"
    );
    for forbidden in [
        "execute_show_all_tasks_with_config(",
        "DisplayRecorder::",
        "legacy_display",
    ] {
        assert!(
            !method_code.contains(forbidden),
            "product TaskTree context must not retain the parallel writer path: {forbidden}"
        );
    }
}

#[test]
fn method_region検出は対象methodのcodeだけを返す() {
    let implementation = r###"
impl TaskTreeCommandContext for RuntimeTaskTreeCommandContext<'_, '_, '_> {
    fn other_method(&mut self) {
        build_show_all_tasks_display_with_config(in_other_method);
        execute_show_all_tasks_with_config(in_other_method);
    }

    fn show_task_list(&mut self) -> Result<DisplayModel, ApplicationError> {
        let comment_shaped = "execute_show_all_tasks_with_config(DisplayRecorder::default())";
        let raw = r#"legacy_display build_show_all_tasks_display_with_config(in_raw)"#;
        // execute_show_all_tasks_with_config(in_comment);
        /* DisplayRecorder::with_ansi_color(true); */
        if ready() {
            nested_call();
        }
        build_show_all_tasks_display_with_config(in_product_method)
    }

    fn trailing_method(&mut self) {
        let legacy_display = DisplayRecorder::default();
    }
}
"###;

    let methods = direct_impl_method_regions(implementation, "show_task_list");

    assert_eq!(methods.len(), 1);
    let method_code = code_only(methods[0]);
    assert_eq!(
        method_code
            .matches("build_show_all_tasks_display_with_config(")
            .count(),
        1
    );
    for excluded in [
        "execute_show_all_tasks_with_config(",
        "DisplayRecorder::",
        "legacy_display",
        "in_other_method",
        "trailing_method",
    ] {
        assert!(!method_code.contains(excluded), "{excluded}: {method_code}");
    }
}

#[test]
fn view_writer_scannerはrustのfunction修飾子とwriter型を網羅する() {
    let source = r#"
fn plain(writer: &mut dyn SchronuWriter) {}
async fn asynchronous<W: Write>(writer: W) {}
unsafe fn unsafe_output(writer: impl Write) {}
extern fn bare_external(writer: &mut dyn std::io::Write) {}
extern "system" fn system_external(writer: impl Write) {}
unsafe extern "C-unwind" fn unwind_external(writer: impl Write) {}
async unsafe fn async_unsafe() { println ! ("bad"); }
unsafe extern "C" fn unsafe_external() { eprintln!("bad"); }
const fn constant() { print!("bad"); }
const unsafe fn constant_unsafe() { write ! (sink, "bad"); }
pub(crate) const unsafe extern "system" fn constant_external() { writeln!(sink, "bad"); }
pub(in crate) async unsafe extern "C-unwind" fn combined() { sink.write_all(bytes); }
pub(super) fn renderer_call() { render_display_model(writer, model); }
fn flush_call() { writer.flush(); }
fn newline_call() { writeln_newline(writer, "bad"); }
"#;

    let violations = view_writer_dependency_violations(source);

    for function_name in [
        "plain",
        "asynchronous",
        "unsafe_output",
        "bare_external",
        "system_external",
        "unwind_external",
        "async_unsafe",
        "unsafe_external",
        "constant",
        "constant_unsafe",
        "constant_external",
        "combined",
        "renderer_call",
        "flush_call",
        "newline_call",
    ] {
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains(function_name)),
            "scanner must reject {function_name}: {violations:?}"
        );
    }
}

#[test]
fn view_writer_scannerは非codeとtest専用functionを除外する() {
    let source = r#####"
fn clean<'a>(value: &'a str) {
    let quote = '"';
    let apostrophe = '\'';
    let byte_quote = b'"';
    let byte_apostrophe = b'\'';
    let text = "println!(\"not code\") and dyn Write";
    let bytes = b"writer.flush() and impl Write and \\\"quoted\\\"";
    let raw = r###"
        writeln_newline(writer, "not code");
    "###;
    let byte_raw = br###"
        sink.write_all(bytes);
        fn fake(writer: impl Write) {}
    "###;
    // render_display_model(writer, model);
    /*
    /* nested println!("not code"); */
    unsafe extern "C" fn commented(writer: impl Write) {}
    */
}

#[cfg(test)]
fn test_only(writer: &mut dyn SchronuWriter) {
    writer.flush();
}
"#####;

    assert_eq!(
        view_writer_dependency_violations(source),
        Vec::<String>::new()
    );
}

#[test]
fn source_maskはrustで有効なbyte_raw_prefixだけを認識する() {
    assert!(raw_string_opener_at("br###\"bytes\"###", 0).is_some());
    assert!(raw_string_opener_at("rb###\"bytes\"###", 0).is_none());
}

#[test]
fn function_regionはpub_superの次item前で切れる() {
    let source = "fn target() {\n    let raw = r#\"\nfn fake() {}\nstruct Fake;\n\"#;\n    shared_dispatch();\n}\npub(super) fn next() {\n    bypass();\n}\n";
    let sources = [ControllerProductSource {
        path: PathBuf::from("fixture.rs"),
        text: source.to_string(),
    }];

    let (_, region) = unique_function_region(&sources, "target").unwrap();

    assert!(region.contains("fn fake() {}"));
    assert!(region.contains("struct Fake;"));
    assert!(region.contains("shared_dispatch();"));
    assert!(!region.contains("pub(super) fn next"));
    assert!(!region.contains("bypass();"));
}

#[test]
fn unique_function_regionはblock_comment内のraw_openerを無視する() {
    let sources = [ControllerProductSource {
        path: PathBuf::from("fixture.rs"),
        text: "/*\nr#\"\nfn target() {}\n*/\nfn target() {\n    shared_dispatch();\n}\n"
            .to_string(),
    }];

    let (path, region) = unique_function_region(&sources, "target").unwrap();

    assert_eq!(path, Path::new("fixture.rs"));
    assert!(region.contains("shared_dispatch();"));
}

#[test]
fn top_level_item検出はnestedとtest専用と非codeを除外する() {
    let source = r##"
#[cfg(test)]
trait FocusDisplaySource {}
#[cfg(test)]
impl FocusDisplaySource for TaskFocusDisplaySource<'_> {}

fn nested_owner() {
    trait FocusDisplaySource {}
    struct TaskFocusDisplaySource<'a> { value: &'a str }
    impl FocusDisplaySource for TaskFocusDisplaySource<'_> {}
}

const RAW: &str = r#"
trait FocusDisplaySource {}
struct TaskFocusDisplaySource<'a> { value: &'a str }
impl FocusDisplaySource for TaskFocusDisplaySource<'_> {}
"#;

/*
trait FocusDisplaySource {}
struct TaskFocusDisplaySource<'a> { value: &'a str }
impl FocusDisplaySource for TaskFocusDisplaySource<'_> {}
*/

pub(super) trait FocusDisplaySource {}
pub(super) struct TaskFocusDisplaySource<'a> { value: &'a str }
impl FocusDisplaySource for TaskFocusDisplaySource<'_> {}
"##;

    let trait_offsets = top_level_item_definition_offsets(source, "trait FocusDisplaySource");
    let struct_offsets = top_level_item_definition_offsets(source, "struct TaskFocusDisplaySource");
    let impl_offsets = top_level_item_definition_offsets(
        source,
        "impl FocusDisplaySource for TaskFocusDisplaySource",
    );

    assert_eq!(trait_offsets.len(), 1);
    assert_eq!(struct_offsets.len(), 1);
    assert_eq!(impl_offsets.len(), 1);
    assert!(source[trait_offsets[0]..].starts_with("pub(super) trait FocusDisplaySource"));
    assert!(source[struct_offsets[0]..].starts_with("pub(super) struct TaskFocusDisplaySource"));
    assert!(source[impl_offsets[0]..]
        .starts_with("impl FocusDisplaySource for TaskFocusDisplaySource<'_>"));
}

#[test]
fn unique_function_regionはpath単位の重複定義を拒否する() {
    let sources = [
        ControllerProductSource {
            path: PathBuf::from("a.rs"),
            text: "fn duplicate() {}\n".to_string(),
        },
        ControllerProductSource {
            path: PathBuf::from("b.rs"),
            text: "/*\nfn duplicate() {}\n*/\n    fn duplicate() {}\nconst RAW: &str = r#\"\nfn duplicate() {}\n\"#;\nfn duplicate() {}\n"
                .to_string(),
        },
    ];

    let error = unique_function_region(&sources, "duplicate").unwrap_err();

    assert!(error.contains("found 2"), "{error}");
    assert!(error.contains("a.rs"), "{error}");
    assert!(error.contains("b.rs"), "{error}");
}

#[test]
fn interactive再描画判断はtyped_command_kindだけで決まる() {
    for kind in [
        CommandKind::NewProject,
        CommandKind::UnplannedProject,
        CommandKind::Tree,
        CommandKind::Leaves,
        CommandKind::ShowAll,
        CommandKind::Tail,
        CommandKind::Today,
        CommandKind::Calendar,
        CommandKind::Band,
        CommandKind::DeferRoutines,
        CommandKind::Flatten,
        CommandKind::Pack,
    ] {
        assert!(
            should_suppress_leaf_tasks_after_command(kind),
            "{kind:?} must not append the leaf tree after its own display"
        );
    }

    for kind in [
        CommandKind::Estimate,
        CommandKind::Focus,
        CommandKind::Defer,
        CommandKind::Finish,
    ] {
        assert!(
            !should_suppress_leaf_tasks_after_command(kind),
            "{kind:?} must refresh the leaf tree after mutation"
        );
    }
}

#[test]
fn interactive_aliasは同じtyped_kindと再描画方針になる() {
    for (aliases, expected_kind) in [
        (["新 project 15", "new project 15"], CommandKind::NewProject),
        (["全", "all"], CommandKind::ShowAll),
    ] {
        let kinds = aliases.map(|command| {
            parse_command(command, ParseMode::Interactive)
                .expect("alias fixture must parse")
                .kind()
        });

        assert_eq!(kinds, [expected_kind, expected_kind]);
        assert!(kinds
            .into_iter()
            .all(should_suppress_leaf_tasks_after_command));
    }
}

#[test]
fn interactive製品eventはtyped_classifierへ直接接続する() {
    let product_sources = controller_product_sources();
    let (classifier_path, classifier_source) =
        unique_function_region(&product_sources, "should_suppress_leaf_tasks_after_command")
            .expect("controller must define one interactive redraw classifier");
    assert_eq!(
        classifier_path.file_name().and_then(|name| name.to_str()),
        Some("interactive.rs"),
        "interactive driver must own the redraw classifier"
    );
    assert!(
        classifier_source.contains("kind: CommandKind"),
        "redraw classifier must accept the parsed command kind"
    );
    for forbidden in ["parse_command(", ".chars().next(", ".split_whitespace("] {
        assert!(
            !classifier_source.contains(forbidden),
            "redraw classifier must not inspect raw command text with {forbidden}"
        );
    }

    let (_, caller_source) =
        unique_function_region(&product_sources, "handle_interactive_driver_event")
            .expect("interactive driver event boundary must remain unique");
    let caller_code = code_only(caller_source);
    assert!(
        caller_code.contains("should_suppress_leaf_tasks_after_command(command_kind)"),
        "interactive command completion must pass its typed kind directly to the redraw classifier"
    );
    for forbidden in ["parse_command(", ".chars().next(", ".split_whitespace("] {
        assert!(
            !caller_code.contains(forbidden),
            "interactive event caller must not recover command meaning with {forbidden}"
        );
    }

    let (_, entrypoint_source) =
        unique_function_region(&product_sources, "interactive_application")
            .expect("interactive application entrypoint must remain unique");
    let entrypoint_code = code_only(entrypoint_source);
    assert!(
        contains_direct_free_function_call(&entrypoint_code, "handle_interactive_driver_event"),
        "interactive application must delegate product events to the shared driver boundary"
    );
}

#[test]
fn direct_free_function_call_scannerはqualified_callと非codeを除外する() {
    let function_name = "handle_interactive_driver_event";
    for source in [
        "another_handle_interactive_driver_event();",
        "driver.handle_interactive_driver_event();",
        "runtime::handle_interactive_driver_event();",
        "fn handle_interactive_driver_event() {}",
        "// handle_interactive_driver_event();",
        "let marker = \"handle_interactive_driver_event();\";",
    ] {
        assert!(
            !contains_direct_free_function_call(&code_only(source), function_name),
            "scanner must reject non-direct call: {source}"
        );
    }
    assert!(contains_direct_free_function_call(
        &code_only("handle_interactive_driver_event ();"),
        function_name
    ));
}

#[test]
fn interactive再描画分類は全command_kindを網羅する() {
    let all_command_kinds = representative_valid_commands().into_iter().fold(
        Vec::<CommandKind>::new(),
        |mut kinds, command| {
            let kind = command.kind();
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
            kinds
        },
    );
    assert_eq!(
        all_command_kinds.len(),
        50,
        "shared representative command fixture must cover every CommandKind"
    );
    for (index, kind) in all_command_kinds.iter().enumerate() {
        assert!(
            !all_command_kinds[..index].contains(kind),
            "classification fixture must contain {kind:?} only once"
        );
    }

    for kind in all_command_kinds {
        let expected = match kind {
            CommandKind::NewProject
            | CommandKind::UnplannedProject
            | CommandKind::Tree
            | CommandKind::Leaves
            | CommandKind::ShowAll
            | CommandKind::Tail
            | CommandKind::Today
            | CommandKind::Calendar
            | CommandKind::Band
            | CommandKind::DeferRoutines
            | CommandKind::Flatten
            | CommandKind::Pack => true,
            CommandKind::Noop
            | CommandKind::HobbyProject
            | CommandKind::Sequential
            | CommandKind::Repeat
            | CommandKind::Appointment
            | CommandKind::Start
            | CommandKind::Ancestor
            | CommandKind::Root
            | CommandKind::NonRepetitive
            | CommandKind::Focus
            | CommandKind::Pick
            | CommandKind::Open
            | CommandKind::Obsidian
            | CommandKind::Unfocus
            | CommandKind::Parent
            | CommandKind::Children
            | CommandKind::Deepest
            | CommandKind::NextUp
            | CommandKind::Breakdown
            | CommandKind::Split
            | CommandKind::Wait
            | CommandKind::Deadline
            | CommandKind::Estimate
            | CommandKind::Arrange
            | CommandKind::TuckAway
            | CommandKind::Actual
            | CommandKind::Priority
            | CommandKind::Category
            | CommandKind::Work
            | CommandKind::Defer
            | CommandKind::Escape
            | CommandKind::Extrude
            | CommandKind::Clear
            | CommandKind::Gather
            | CommandKind::Finish
            | CommandKind::FocusHighest
            | CommandKind::FocusLowest
            | CommandKind::Verify => false,
        };

        assert_eq!(
            should_suppress_leaf_tasks_after_command(kind),
            expected,
            "classification changed for {kind:?}"
        );
    }
}
