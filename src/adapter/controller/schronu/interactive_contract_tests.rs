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
    unique_function_region(&product_sources, "execute_parsed")
        .expect("controller must define exactly one shared parsed-command dispatcher");

    for (entry_function, allowed_direct_dispatches) in [
        (
            "execute_non_interactive_command_at",
            &["CommandKind::Verify"][..],
        ),
        (
            "execute_interactive_command",
            &[
                "Command::Focus",
                "Command::Defer",
                "Command::InteractiveShortcut",
                "CommandKind::Verify",
            ][..],
        ),
    ] {
        let (_, entry_source) = unique_function_region(&product_sources, entry_function)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            entry_source.contains("execute_parsed("),
            "{entry_function} must route parsed commands through the shared dispatcher"
        );
        let mut source_without_allowed_dispatches = entry_source.to_string();
        for allowed_direct_dispatch in allowed_direct_dispatches {
            assert!(
                entry_source.contains(allowed_direct_dispatch),
                "{entry_function} must retain intentional direct dispatch {allowed_direct_dispatch}"
            );
            source_without_allowed_dispatches =
                source_without_allowed_dispatches.replace(allowed_direct_dispatch, "");
        }
        assert!(
            !source_without_allowed_dispatches.contains("Command::")
                && !source_without_allowed_dispatches.contains("CommandKind::"),
            "{entry_function} must not add a direct typed-command dispatch outside the allowlist"
        );
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
    for required in ["verify_display_model(", "render_display_model("] {
        assert!(verify_source.contains(required));
    }
    for forbidden in ["println!(", "eprintln!(", "render_verify_flush("] {
        assert!(!verify_source.contains(forbidden));
    }

    let (_, interactive_source) =
        unique_function_region(&product_sources, "execute_interactive_command")
            .unwrap_or_else(|error| panic!("{error}"));
    assert!(interactive_source.contains("DisplayModel::flush()"));
    assert!(interactive_source.contains("render_display_model("));
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

    for allowed_runtime_boundary in [
        "run_repository_transaction(",
        "StorageLock::",
        "webbrowser::open(",
        "process::Command::new(",
        "fn interactive_application(",
        "fn execute_verify_command(",
        "fn render_focus_from_source(",
    ] {
        assert!(
            runtime.text.contains(allowed_runtime_boundary),
            "runtime.rs must retain I/O coordination through {allowed_runtime_boundary}"
        );
    }

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
            for forbidden in ["SchronuWriter", "render_display_model(", ".flush("] {
                if view_item.contains(forbidden) {
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

    let (_, caller_source) = unique_function_region(&product_sources, "interactive_application")
        .expect("interactive application entry must remain unique");
    assert!(
        caller_source.contains("should_suppress_leaf_tasks_after_command(command_kind)"),
        "interactive command completion must pass its typed kind directly to the redraw classifier"
    );
    for forbidden in ["parse_command(", ".chars().next(", ".split_whitespace("] {
        assert!(
            !caller_source.contains(forbidden),
            "interactive event caller must not recover command meaning with {forbidden}"
        );
    }
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
        49,
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
