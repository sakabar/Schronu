use std::fs;
use std::path::Path;

fn controller_product_source() -> String {
    let controller_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/adapter/controller/schronu");
    let mut product_source = String::new();

    append_controller_product_source(&controller_dir, &mut product_source);
    product_source
}

fn append_controller_product_source(directory: &Path, product_source: &mut String) {
    for entry in
        fs::read_dir(directory).expect("controller source directory must be readable recursively")
    {
        let entry = entry.expect("controller source entry must be readable");
        let file_type = entry
            .file_type()
            .expect("controller source entry type must be readable");
        let path = entry.path();
        if file_type.is_dir() {
            append_controller_product_source(&path, product_source);
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
        product_source.push_str(
            &fs::read_to_string(path).expect("controller product source must be readable"),
        );
    }
}

fn function_region<'a>(source: &'a str, function_name: &str) -> &'a str {
    let marker = format!("fn {function_name}(");
    let start = source
        .find(&marker)
        .unwrap_or_else(|| panic!("product function {function_name} must exist"));
    let tail = &source[start..];
    let signature_end = tail.find('\n').map_or(tail.len(), |offset| offset + 1);
    let mut end = tail.len();
    let mut line_start = signature_end;
    for line in tail[signature_end..].split_inclusive('\n') {
        if is_top_level_rust_item(line.trim_end_matches('\n')) {
            end = line_start;
            break;
        }
        line_start += line.len();
    }
    &tail[..end]
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
    let product_source = controller_product_source();
    let compact_source = product_source.split_whitespace().collect::<String>();
    assert_eq!(
        compact_source.matches("fnparse_command(").count(),
        1,
        "controller must keep one shared typed parser definition"
    );
    for parse_mode in ["Interactive", "NonInteractive"] {
        assert!(
            compact_source.contains(&format!("parse_command(command,ParseMode::{parse_mode})")),
            "{parse_mode} product entry must call the shared typed parser"
        );
    }
}

#[test]
fn interactiveとnoninteractiveは単一のparsed_command_dispatcherを共有する() {
    let product_source = controller_product_source();
    assert_eq!(
        product_source
            .lines()
            .filter(|line| is_top_level_function_definition(line, "execute_parsed"))
            .count(),
        1,
        "controller must define exactly one shared parsed-command dispatcher"
    );

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
            ][..],
        ),
    ] {
        let entry_source = function_region(&product_source, entry_function);
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
    let source =
        "fn target() {\n    shared_dispatch();\n}\npub(super) fn next() {\n    bypass();\n}\n";

    let region = function_region(source, "target");

    assert!(region.contains("shared_dispatch();"));
    assert!(!region.contains("pub(super) fn next"));
    assert!(!region.contains("bypass();"));
}
