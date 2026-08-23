use std::fs;
use std::path::Path;

fn controller_product_source() -> String {
    let controller_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/adapter/controller/schronu");
    let mut product_source = String::new();

    for entry in fs::read_dir(controller_dir).expect("controller source directory must be readable")
    {
        let path = entry
            .expect("controller source entry must be readable")
            .path();
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

    product_source
}

fn function_region<'a>(source: &'a str, function_name: &str) -> &'a str {
    let marker = format!("fn {function_name}(");
    let start = source
        .find(&marker)
        .unwrap_or_else(|| panic!("product function {function_name} must exist"));
    let tail = &source[start..];
    let end = ["\nfn ", "\nstruct ", "\nenum "]
        .iter()
        .filter_map(|next_item| tail[marker.len()..].find(next_item))
        .min()
        .map_or(tail.len(), |end| marker.len() + end);
    &tail[..end]
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

    for entry_function in [
        "execute_non_interactive_command_at",
        "execute_interactive_command",
    ] {
        let entry_source = function_region(&product_source, entry_function);
        assert!(
            entry_source.contains("execute_parsed("),
            "{entry_function} must route parsed commands through the shared dispatcher"
        );
        for forbidden_direct_dispatch in ["Command::Estimate", "CommandKind::Estimate"] {
            assert!(
                !entry_source.contains(forbidden_direct_dispatch),
                "{entry_function} must not bypass the shared handler for Estimate"
            );
        }
    }
}
