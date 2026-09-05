#[test]
fn server限定endpoint_testはweb単独buildへ混入しない() {
    let source = include_str!("../src/app/web_endpoint.rs");

    assert!(source.contains("#[cfg(all(test, feature = \"server\"))]\nmod tests"));
}
