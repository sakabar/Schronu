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
    [
        "fn ",
        "async fn ",
        "unsafe fn ",
        "const fn ",
        "extern \"C\" fn ",
        "async unsafe fn ",
        "unsafe extern \"C\" fn ",
        "const unsafe fn ",
        "const unsafe extern \"C\" fn ",
        "async unsafe extern \"C\" fn ",
    ]
    .iter()
    .any(|prefix| declaration.starts_with(prefix))
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

    [
        "fn ",
        "async fn ",
        "unsafe fn ",
        "const fn ",
        "extern \"C\" fn ",
        "async unsafe fn ",
        "unsafe extern \"C\" fn ",
        "const unsafe fn ",
        "const unsafe extern \"C\" fn ",
        "async unsafe extern \"C\" fn ",
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

fn view_writer_dependency_violations(source: &str) -> Vec<String> {
    let mut violations = Vec::new();
    for offset in top_level_product_function_offsets(source) {
        let region = function_region_from_offset(source, offset);
        let code = code_only(region);
        let signature = code.split_once('{').map_or(code.as_str(), |(head, _)| head);
        let first_line = region.lines().next().unwrap_or("<unknown function>");

        if signature.contains("SchronuWriter")
            || signature.contains("std::io::Write")
            || contains_identifier(signature, "Write")
        {
            violations.push(format!("writer type in {first_line}"));
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
                violations.push(format!("{forbidden} in {first_line}"));
            }
        }
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

    let (_, legacy_region) =
        unique_function_region(&product_sources, "execute_show_all_tasks_with_config").unwrap();
    for forbidden in [
        "let busy_s",
        "let s_for_rho1",
        "let s_for_non_repetitive_rho",
        "完了見込み日時は",
    ] {
        if legacy_region.contains(forbidden) {
            panic!(
                "execute_show_all_tasks_with_config must return typed metrics without legacy preformat: {forbidden}"
            );
        }
    }

    assert!(
        violations.is_empty(),
        "view.rs must return typed models without writer/output dependencies:\n{}",
        violations.join("\n")
    );
}

#[test]
fn view_writer_scannerはrustのfunction修飾子とwriter型を網羅する() {
    let source = r#"
fn plain(writer: &mut dyn SchronuWriter) {}
async fn asynchronous<W: Write>(writer: W) {}
unsafe fn unsafe_output(writer: impl Write) {}
extern "C" fn external(writer: &mut dyn std::io::Write) {}
async unsafe fn async_unsafe() { println ! ("bad"); }
unsafe extern "C" fn unsafe_external() { eprintln!("bad"); }
const fn constant() { print!("bad"); }
const unsafe fn constant_unsafe() { write ! (sink, "bad"); }
const unsafe extern "C" fn constant_external() { writeln!(sink, "bad"); }
async unsafe extern "C" fn combined() { sink.write_all(bytes); }
pub(super) fn renderer_call() { render_display_model(writer, model); }
fn flush_call() { writer.flush(); }
fn newline_call() { writeln_newline(writer, "bad"); }
"#;

    let violations = view_writer_dependency_violations(source);

    for function_name in [
        "plain",
        "asynchronous",
        "unsafe_output",
        "external",
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
