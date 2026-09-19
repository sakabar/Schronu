use super::paths::imports;
use super::source::product_file;
use std::collections::BTreeMap;

#[test]
fn grouped_and_renamed_imports_preserve_dependency_identity() {
    let file = product_file(
        r#"
        use super::{runtime as driver, renderer::{self, DisplayModel as Model}};
        use std::io::{self, Write as Sink};
        fn harmless() { let text = "use super::runtime"; }
    "#,
    )
    .unwrap();
    assert_eq!(
        imports("controller::handler", &file).unwrap(),
        BTreeMap::from([
            ("driver".into(), "controller::runtime".into()),
            ("renderer".into(), "controller::renderer".into()),
            ("Model".into(), "controller::renderer::DisplayModel".into()),
            ("io".into(), "std::io".into()),
            ("Sink".into(), "std::io::Write".into()),
        ])
    );
}
