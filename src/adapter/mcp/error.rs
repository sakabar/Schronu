#[derive(Debug)]
pub(super) struct InvalidParams {
    pub(super) field: String,
    pub(super) reason: &'static str,
}
