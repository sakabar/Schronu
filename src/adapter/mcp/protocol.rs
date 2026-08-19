use super::InvalidParams;
use serde_json::{json, Map, Value};

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LifecycleState {
    Uninitialized,
    InitializeResponded,
    Initialized,
}

pub(super) fn validate_request_envelope(request: &Value) -> Result<(String, Option<Value>), Value> {
    let Some(request) = request.as_object() else {
        return Err(Value::Null);
    };
    let response_id = request
        .get("id")
        .filter(|id| id.is_string() || id.is_number())
        .cloned()
        .unwrap_or(Value::Null);

    if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(response_id);
    }
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return Err(response_id);
    };
    let id = match request.get("id") {
        Some(id) if id.is_string() || id.is_number() => Some(id.clone()),
        Some(_) => return Err(Value::Null),
        None if matches!(method, "initialize" | "tools/list" | "tools/call") => {
            return Err(Value::Null)
        }
        None => None,
    };

    Ok((method.to_string(), id))
}

pub(super) fn initialized_notification_params_are_valid(request: &Value) -> bool {
    let Some(params) = request.get("params") else {
        return true;
    };
    let Some(params) = params.as_object() else {
        return false;
    };
    optional_object_field(params, "_meta", "params._meta").is_ok()
}

pub(super) fn validate_initialize_params(request: &Value) -> Result<(), InvalidParams> {
    let params = request.get("params").ok_or_else(|| InvalidParams {
        field: "params".to_string(),
        reason: "is required",
    })?;
    let params = params.as_object().ok_or_else(|| InvalidParams {
        field: "params".to_string(),
        reason: "must be an object",
    })?;

    required_string_field(params, "protocolVersion", "params.protocolVersion")?;
    let capabilities = required_object_field(params, "capabilities", "params.capabilities")?;
    validate_client_capabilities(capabilities)?;
    let client_info = required_object_field(params, "clientInfo", "params.clientInfo")?;
    required_string_field(client_info, "name", "params.clientInfo.name")?;
    required_string_field(client_info, "version", "params.clientInfo.version")?;
    if client_info
        .get("title")
        .is_some_and(|title| !title.is_string())
    {
        return Err(InvalidParams {
            field: "params.clientInfo.title".to_string(),
            reason: "must be a string",
        });
    }
    if let Some(meta) = optional_object_field(params, "_meta", "params._meta")? {
        optional_string_or_number_field(meta, "progressToken", "params._meta.progressToken")?;
    }

    Ok(())
}

fn validate_client_capabilities(capabilities: &Map<String, Value>) -> Result<(), InvalidParams> {
    if let Some(roots) = optional_object_field(capabilities, "roots", "params.capabilities.roots")?
    {
        optional_boolean_field(
            roots,
            "listChanged",
            "params.capabilities.roots.listChanged",
        )?;
    }
    optional_object_field(capabilities, "sampling", "params.capabilities.sampling")?;
    optional_object_field(
        capabilities,
        "elicitation",
        "params.capabilities.elicitation",
    )?;
    if let Some(experimental) = optional_object_field(
        capabilities,
        "experimental",
        "params.capabilities.experimental",
    )? {
        for (name, capability) in experimental {
            if !capability.is_object() {
                return Err(InvalidParams {
                    field: format!("params.capabilities.experimental.{name}"),
                    reason: "must be an object",
                });
            }
        }
    }

    Ok(())
}

fn required_string_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    field_path: &str,
) -> Result<&'a str, InvalidParams> {
    let value = object.get(field).ok_or_else(|| InvalidParams {
        field: field_path.to_string(),
        reason: "is required",
    })?;
    value.as_str().ok_or_else(|| InvalidParams {
        field: field_path.to_string(),
        reason: "must be a string",
    })
}

fn required_object_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    field_path: &str,
) -> Result<&'a Map<String, Value>, InvalidParams> {
    let value = object.get(field).ok_or_else(|| InvalidParams {
        field: field_path.to_string(),
        reason: "is required",
    })?;
    value.as_object().ok_or_else(|| InvalidParams {
        field: field_path.to_string(),
        reason: "must be an object",
    })
}

fn optional_object_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    field_path: &str,
) -> Result<Option<&'a Map<String, Value>>, InvalidParams> {
    object
        .get(field)
        .map(|value| {
            value.as_object().ok_or_else(|| InvalidParams {
                field: field_path.to_string(),
                reason: "must be an object",
            })
        })
        .transpose()
}

fn optional_boolean_field(
    object: &Map<String, Value>,
    field: &str,
    field_path: &str,
) -> Result<Option<bool>, InvalidParams> {
    object
        .get(field)
        .map(|value| {
            value.as_bool().ok_or_else(|| InvalidParams {
                field: field_path.to_string(),
                reason: "must be a boolean",
            })
        })
        .transpose()
}

fn optional_string_or_number_field<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    field_path: &str,
) -> Result<Option<&'a Value>, InvalidParams> {
    object
        .get(field)
        .map(|value| {
            if value.is_string() || value.is_number() {
                Ok(value)
            } else {
                Err(InvalidParams {
                    field: field_path.to_string(),
                    reason: "must be a string or number",
                })
            }
        })
        .transpose()
}

pub(super) fn initialize_response(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {
                "tools": {"listChanged": false}
            },
            "serverInfo": {
                "name": "schronu",
                "version": env!("CARGO_PKG_VERSION")
            }
        }
    })
}

pub(super) fn tools_list_response(id: Value, tools: Vec<Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"tools": tools}
    })
}

pub(super) fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

pub(super) fn invalid_params_response(id: Value, error: InvalidParams) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32602,
            "message": "Invalid params",
            "data": {
                "code": "invalid_input",
                "field": error.field,
                "reason": error.reason
            }
        }
    })
}

pub(super) fn tool_result_response(id: Value, structured_content: Value, is_error: bool) -> Value {
    let text = structured_content.to_string();
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{"type": "text", "text": text}],
            "structuredContent": structured_content,
            "isError": is_error
        }
    })
}
