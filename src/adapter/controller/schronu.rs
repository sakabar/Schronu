#[path = "schronu/command.rs"]
mod command;

#[path = "schronu/handler.rs"]
mod handler;

#[path = "schronu/renderer.rs"]
mod renderer;

#[path = "schronu/runtime.rs"]
mod runtime;

#[cfg(test)]
#[path = "schronu/command_contract_tests.rs"]
mod command_contract_tests;

#[cfg(test)]
#[path = "schronu/handler_contract_tests.rs"]
mod handler_contract_tests;

#[cfg(test)]
#[path = "schronu/renderer_contract_tests.rs"]
mod renderer_contract_tests;

fn main() {
    runtime::application();
}

#[cfg(test)]
mod entrypoint_contract_tests {
    use std::path::Path;

    #[test]
    fn entrypoint_delegates_to_runtime_application() {
        const TEST_MODULE_MARKER: &str = "#[cfg(test)]\nmod entrypoint_contract_tests";
        const EXPECTED_MAIN: &str = "fn main() {\n    runtime::application();\n}";

        let entrypoint_source = include_str!("schronu.rs")
            .split_once(TEST_MODULE_MARKER)
            .expect("entrypoint contract test module must remain in schronu.rs")
            .0
            .trim();
        let (module_source, main_body) = entrypoint_source
            .split_once("\nfn main()")
            .expect("entrypoint must contain a top-level main function");
        let runtime_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/adapter/controller/schronu/runtime.rs");

        assert!(
            entrypoint_source.lines().count() <= 40,
            "entrypoint must remain thin"
        );
        assert_eq!(format!("fn main(){main_body}"), EXPECTED_MAIN);

        let mut module_names = Vec::new();
        let mut has_pending_attribute = false;
        for line in module_source
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            if line.starts_with("#[") && line.ends_with(']') {
                has_pending_attribute = true;
                continue;
            }

            let module_name = line
                .strip_prefix("mod ")
                .and_then(|line| line.strip_suffix(';'))
                .filter(|name| {
                    let mut characters = name.chars();
                    characters
                        .next()
                        .is_some_and(|character| character.is_ascii_lowercase())
                        && characters.all(|character| {
                            character.is_ascii_lowercase()
                                || character.is_ascii_digit()
                                || character == '_'
                        })
                })
                .expect("entrypoint may only declare private modules before main");
            module_names.push(module_name);
            has_pending_attribute = false;
        }

        assert!(!has_pending_attribute, "attribute must apply to a module");
        assert!(
            module_names.contains(&"runtime"),
            "runtime module must be declared"
        );
        assert!(runtime_path.is_file(), "runtime.rs must exist");
    }
}
