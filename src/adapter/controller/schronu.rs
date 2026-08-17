#[path = "schronu/runtime.rs"]
mod runtime;

fn main() {
    runtime::application();
}

#[cfg(test)]
mod entrypoint_contract_tests {
    use std::path::Path;

    #[test]
    fn entrypoint_delegates_to_runtime_application() {
        const TEST_MODULE_MARKER: &str = "#[cfg(test)]\nmod entrypoint_contract_tests";
        const EXPECTED_ENTRYPOINT: &str = "#[path = \"schronu/runtime.rs\"]\nmod runtime;\n\nfn main() {\n    runtime::application();\n}";

        let entrypoint_source = include_str!("schronu.rs")
            .split_once(TEST_MODULE_MARKER)
            .expect("entrypoint contract test module must remain in schronu.rs")
            .0
            .trim();
        let runtime_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/adapter/controller/schronu/runtime.rs");

        assert_eq!(entrypoint_source, EXPECTED_ENTRYPOINT);
        assert!(runtime_path.is_file(), "runtime.rs must exist");
    }
}
