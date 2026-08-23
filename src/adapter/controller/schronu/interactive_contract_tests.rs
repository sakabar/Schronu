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
    let marker = format!("fn {function_name}(");
    let mut offsets = Vec::new();
    let mut line_start = 0;
    let mut non_code_state = MultilineNonCodeState::default();

    for line in source.split_inclusive('\n') {
        if !non_code_state.starts_in_non_code()
            && is_top_level_function_definition(line.trim_end(), function_name)
        {
            let marker_offset = line
                .find(&marker)
                .expect("matching function signature must contain its marker");
            offsets.push(line_start + marker_offset);
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
        let bytes = line.as_bytes();
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
            if let Some((opener_length, terminator)) = raw_string_opener_at(line, index) {
                self.raw_string_terminator_opt = Some(terminator);
                index += opener_length;
                continue;
            }
            if bytes[index] == b'"' {
                self.in_quoted_string = true;
            }
            index += 1;
        }
    }
}

fn raw_string_opener_at(line: &str, index: usize) -> Option<(usize, String)> {
    let bytes = line.as_bytes();
    if bytes.get(index) != Some(&b'r') {
        return None;
    }
    if index > 0 && (bytes[index - 1].is_ascii_alphanumeric() || bytes[index - 1] == b'_') {
        return None;
    }
    let mut quote_index = index + 1;
    while bytes.get(quote_index) == Some(&b'#') {
        quote_index += 1;
    }
    if bytes.get(quote_index) != Some(&b'"') {
        return None;
    }
    let hash_count = quote_index - index - 1;
    Some((hash_count + 2, format!("\"{}", "#".repeat(hash_count))))
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

fn is_top_level_function_definition(line: &str, function_name: &str) -> bool {
    if line.is_empty() || line.chars().next().is_some_and(char::is_whitespace) {
        return false;
    }
    strip_top_level_visibility(line).starts_with(&format!("fn {function_name}("))
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
