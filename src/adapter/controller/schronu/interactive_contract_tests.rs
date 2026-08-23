use std::fs;
use std::path::Path;

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
