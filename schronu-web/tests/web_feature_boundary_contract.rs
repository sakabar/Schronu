#[test]
fn server限定endpoint_testはweb単独buildへ混入しない() {
    let source = include_str!("../src/app/web_endpoint.rs");

    assert!(source.contains("#[cfg(all(test, feature = \"server\"))]\nmod tests"));
}

const CI_WORKFLOW: &str = include_str!("../../.github/workflows/ci.yml");

#[test]
fn root既存gateとapps_scriptとbenchmarkingを維持する() {
    let job = workflow_job("quality");

    for command in [
        "node --test apps_script/main.test.mjs",
        "cargo test --locked",
        "cargo test --locked --features benchmarking --test scheduling_benchmark_contract",
        "cargo fmt --check",
        "cargo clippy --locked --all-targets -- -D warnings",
    ] {
        assert_job_command(job, command);
    }
}

#[test]
fn native_web_jobはdefault_server_webを個別にtestとclippyする() {
    let job = workflow_job("web-native");

    assert_job_value(job, "toolchain: 1.97.1");
    assert_job_value(job, "uses: Swatinem/rust-cache@v2");
    assert_job_command(
        job,
        "cargo clippy --locked -p schronu-web --no-default-features --all-targets -- -D warnings -A dead-code"
    );
    assert_job_command(
        job,
        "cargo test --locked -p schronu-web --no-default-features",
    );
    assert_job_command(
        job,
        "cargo test --locked -p schronu-web --no-default-features --features server",
    );
    assert_job_command(
        job,
        "cargo clippy --locked -p schronu-web --no-default-features --features server --all-targets -- -D warnings"
    );
    assert_job_command(
        job,
        "cargo test --locked -p schronu-web --no-default-features --features web --test '*'",
    );
    assert_job_command(
        job,
        "cargo clippy --locked -p schronu-web --no-default-features --features web --lib -- -D warnings"
    );
}

#[test]
fn wasm_jobはbrowser専用moduleを独立targetでcheckする() {
    let job = workflow_job("web-wasm");

    assert_job_value(job, "toolchain: 1.97.1");
    assert_job_value(job, "targets: wasm32-unknown-unknown");
    assert_job_value(job, "uses: Swatinem/rust-cache@v2");
    assert_job_command(
        job,
        "cargo check --locked -p schronu-web --no-default-features --features web --target wasm32-unknown-unknown"
    );
    assert!(!job_commands(job)
        .iter()
        .any(|command| command.contains("--all-features")));
}

fn assert_job_command(job: &str, expected: &str) {
    assert!(
        job_commands(job).contains(&expected),
        "missing active workflow command: {expected}"
    );
}

fn assert_job_value(job: &str, expected: &str) {
    assert!(
        job.lines().any(|line| line.trim() == expected),
        "missing active workflow value: {expected}"
    );
}

fn job_commands(job: &str) -> Vec<&str> {
    job.lines()
        .filter_map(|line| line.strip_prefix("        run: "))
        .collect()
}

fn workflow_job(name: &str) -> &str {
    let marker = format!("  {name}:\n");
    let start = CI_WORKFLOW
        .find(&marker)
        .unwrap_or_else(|| panic!("missing workflow job: {name}"));
    let body = &CI_WORKFLOW[start + marker.len()..];
    let end = body
        .match_indices("\n  ")
        .find_map(|(index, _)| {
            body.as_bytes()
                .get(index + 3)
                .filter(|byte| **byte != b' ')
                .map(|_| index)
        })
        .unwrap_or(body.len());
    &body[..end]
}
