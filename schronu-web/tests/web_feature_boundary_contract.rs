#[test]
fn server限定endpoint_testはweb単独buildへ混入しない() {
    let source = include_str!("../src/app/web_endpoint.rs");

    assert!(source.contains("#[cfg(all(test, feature = \"server\"))]\nmod tests"));
}

const CI_WORKFLOW: &str = include_str!("../../.github/workflows/ci.yml");

#[test]
fn root既存gateとapps_scriptとbenchmarkingを維持する() {
    for command in [
        "node --test apps_script/main.test.mjs",
        "cargo test --locked",
        "cargo test --locked --features benchmarking --test scheduling_benchmark_contract",
        "cargo fmt --check",
        "cargo clippy --locked --all-targets -- -D warnings",
    ] {
        assert!(
            CI_WORKFLOW.contains(command),
            "missing CI command: {command}"
        );
    }
}

#[test]
fn native_web_jobはdefault_server_webを個別にtestとclippyする() {
    let job = workflow_job("web-native");

    assert!(job.contains("toolchain: 1.97.1"));
    assert!(job.contains("uses: Swatinem/rust-cache@v2"));
    for features in ["", " --features server", " --features web"] {
        let test = format!("cargo test --locked -p schronu-web --no-default-features{features}");
        let clippy = format!(
            "cargo clippy --locked -p schronu-web --no-default-features{features} --all-targets -- -D warnings"
        );
        assert!(job.contains(&test), "missing native Web test: {test}");
        assert!(job.contains(&clippy), "missing native Web Clippy: {clippy}");
    }
}

#[test]
fn wasm_jobはbrowser専用moduleを独立targetでcheckする() {
    let job = workflow_job("web-wasm");

    assert!(job.contains("toolchain: 1.97.1"));
    assert!(job.contains("targets: wasm32-unknown-unknown"));
    assert!(job.contains("uses: Swatinem/rust-cache@v2"));
    assert!(job.contains(
        "cargo check --locked -p schronu-web --no-default-features --features web --target wasm32-unknown-unknown"
    ));
    assert!(!job.contains("--all-features"));
}

fn workflow_job(name: &str) -> &str {
    let marker = format!("  {name}:\n");
    let start = CI_WORKFLOW
        .find(&marker)
        .unwrap_or_else(|| panic!("missing workflow job: {name}"));
    let body = &CI_WORKFLOW[start + marker.len()..];
    let end = body.find("\n  ").unwrap_or(body.len());
    &body[..end]
}
