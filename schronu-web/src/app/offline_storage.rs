use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[wasm_bindgen(module = "/assets/offline-db.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = prepareShell)]
    async fn prepare() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = cancelPreparation)]
    async fn cancel() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = loadState)]
    async fn load_state() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = saveState)]
    async fn save_state(expected: Option<String>, value: &str) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = removeState)]
    async fn remove_state(expected: &str) -> Result<JsValue, JsValue>;
}

fn browser_error(error: JsValue) -> String {
    error
        .dyn_ref::<js_sys::Error>()
        .map(|error| String::from(error.message()))
        .or_else(|| error.as_string())
        .unwrap_or_else(|| "オフラインデータを操作できませんでした。".into())
}

pub(super) async fn prepare_shell() -> Result<(), String> {
    prepare().await.map(|_| ()).map_err(browser_error)
}

pub(super) async fn cancel_preparation() -> Result<(), String> {
    cancel().await.map(|_| ()).map_err(browser_error)
}

pub(super) async fn load() -> Result<Option<(String, String)>, String> {
    let result = load_state().await.map_err(browser_error)?;
    if result.is_null() {
        return Ok(None);
    }
    let entries = result
        .dyn_into::<js_sys::Array>()
        .map_err(|_| "保存形式を読み取れません。")?;
    let revision = entries
        .get(0)
        .as_string()
        .ok_or("保存版を読み取れません。")?;
    let json = entries
        .get(1)
        .as_string()
        .ok_or("保存内容を読み取れません。")?;
    Ok(Some((revision, json)))
}

pub(super) async fn save(expected: Option<&str>, value: &str) -> Result<String, String> {
    save_state(expected.map(str::to_owned), value)
        .await
        .map_err(browser_error)?
        .as_string()
        .ok_or_else(|| "保存版を読み取れません。".into())
}

pub(super) async fn remove(expected: &str) -> Result<(), String> {
    remove_state(expected)
        .await
        .map(|_| ())
        .map_err(browser_error)
}
