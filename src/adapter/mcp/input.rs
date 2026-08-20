use super::error::InvalidParams;
use crate::application::task_use_case::{
    BreakdownTaskInput, CompleteTaskInput, CreateTaskInput, ListTasksFilter, TaskPeriodField,
    TaskPeriodFilter,
};
use crate::entity::datetime::get_next_morning_datetime;
use crate::entity::task::{ProjectCategory, Status};
use chrono::{DateTime, Duration, Local, LocalResult, NaiveDate};
use schemars::{generate::SchemaSettings, json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::{de::DeserializeOwned, de::IntoDeserializer, Deserialize, Deserializer};
use serde_json::{Map, Value};
use std::borrow::Cow;
use uuid::Uuid;

pub(super) enum ToolInputError {
    Schema(InvalidParams),
    Semantic {
        field: String,
        message: &'static str,
    },
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
const SCHEMA_ERROR_PREFIX: &str = "mcp-schema:";
#[allow(dead_code, reason = "used by the staged typed-tool migration")]
const SEMANTIC_ERROR_PREFIX: &str = "mcp-semantic:";

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
pub(super) struct UuidValue(pub(super) Uuid);

impl<'de> Deserialize<'de> for UuidValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Uuid::parse_str(&value).map(Self).map_err(|_| {
            serde::de::Error::custom(format!("{SEMANTIC_ERROR_PREFIX}must be a valid UUID"))
        })
    }
}

impl JsonSchema for UuidValue {
    fn schema_name() -> Cow<'static, str> {
        "UuidValue".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "format": "uuid"})
    }
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
pub(super) struct Rfc3339DateTime(pub(super) DateTime<Local>);

impl<'de> Deserialize<'de> for Rfc3339DateTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        parse_local_datetime(&value).map(Self).map_err(|_| {
            serde::de::Error::custom(format!(
                "{SEMANTIC_ERROR_PREFIX}must be a valid RFC 3339 date-time"
            ))
        })
    }
}

impl JsonSchema for Rfc3339DateTime {
    fn schema_name() -> Cow<'static, str> {
        "Rfc3339DateTime".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "format": "date-time"})
    }
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
pub(super) struct IsoDate(pub(super) NaiveDate);

impl<'de> Deserialize<'de> for IsoDate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        NaiveDate::parse_from_str(&value, "%Y-%m-%d")
            .map(Self)
            .map_err(|_| {
                serde::de::Error::custom(format!(
                    "{SEMANTIC_ERROR_PREFIX}must be a valid ISO 8601 date"
                ))
            })
    }
}

impl JsonSchema for IsoDate {
    fn schema_name() -> Cow<'static, str> {
        "IsoDate".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "format": "date"})
    }
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
pub(super) struct NonNegativeI64(pub(super) i64);

impl<'de> Deserialize<'de> for NonNegativeI64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let Value::Number(value) = value else {
            return Err(serde::de::Error::custom(format!(
                "{SCHEMA_ERROR_PREFIX}must be a non-negative integer"
            )));
        };
        if let Some(value) = value.as_i64() {
            if value >= 0 {
                return Ok(Self(value));
            }
            return Err(serde::de::Error::custom(format!(
                "{SCHEMA_ERROR_PREFIX}must be a non-negative integer"
            )));
        }
        if value.as_u64().is_some() {
            return Err(serde::de::Error::custom(format!(
                "{SEMANTIC_ERROR_PREFIX}is outside the supported integer range"
            )));
        }
        if let Some(value) = value.as_f64() {
            const I64_UPPER_BOUND_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;
            if value >= 0.0 && value.fract() == 0.0 {
                if value < I64_UPPER_BOUND_EXCLUSIVE {
                    return Ok(Self(value as i64));
                }
                return Err(serde::de::Error::custom(format!(
                    "{SEMANTIC_ERROR_PREFIX}is outside the supported integer range"
                )));
            }
        }
        Err(serde::de::Error::custom(format!(
            "{SCHEMA_ERROR_PREFIX}must be a non-negative integer"
        )))
    }
}

impl JsonSchema for NonNegativeI64 {
    fn schema_name() -> Cow<'static, str> {
        "NonNegativeI64".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "integer", "minimum": 0})
    }
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
pub(super) struct NonEmptyString(pub(super) String);

impl<'de> Deserialize<'de> for NonEmptyString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.is_empty() {
            Err(serde::de::Error::custom(format!(
                "{SCHEMA_ERROR_PREFIX}must not be empty"
            )))
        } else {
            Ok(Self(value))
        }
    }
}

impl JsonSchema for NonEmptyString {
    fn schema_name() -> Cow<'static, str> {
        "NonEmptyString".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "minLength": 1})
    }
}

#[derive(Default)]
#[allow(dead_code, reason = "used by the staged typed-tool migration")]
pub(super) enum OptionalValue<T> {
    #[default]
    Missing,
    Value(T),
}

impl<'de, T> Deserialize<'de> for OptionalValue<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        T::deserialize(deserializer).map(Self::Value)
    }
}

impl<T> JsonSchema for OptionalValue<T>
where
    T: JsonSchema,
{
    fn schema_name() -> Cow<'static, str> {
        format!("OptionalValue_{}", T::schema_name()).into()
    }

    fn schema_id() -> Cow<'static, str> {
        format!("{}::OptionalValue<{}>", module_path!(), T::schema_id()).into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        generator.subschema_for::<T>()
    }
}

#[derive(Default)]
#[allow(dead_code, reason = "used by the staged typed-tool migration")]
pub(super) enum NullablePatch<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
pub(super) trait NullableValue {
    const WRONG_TYPE_REASON: &'static str;
}

impl NullableValue for Rfc3339DateTime {
    const WRONG_TYPE_REASON: &'static str = "must be a string or null";
}

impl<'de, T> Deserialize<'de> for NullablePatch<T>
where
    T: DeserializeOwned + NullableValue,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        if value.is_null() {
            return Ok(Self::Null);
        }
        serde_json::from_value(value)
            .map(Self::Value)
            .map_err(|error| {
                let message = error.to_string();
                if let Some(reason) = error_reason(&message, SEMANTIC_ERROR_PREFIX) {
                    return serde::de::Error::custom(format!("{SEMANTIC_ERROR_PREFIX}{reason}"));
                }
                if let Some(reason) = error_reason(&message, SCHEMA_ERROR_PREFIX) {
                    return serde::de::Error::custom(format!("{SCHEMA_ERROR_PREFIX}{reason}"));
                }
                serde::de::Error::custom(format!("{SCHEMA_ERROR_PREFIX}{}", T::WRONG_TYPE_REASON))
            })
    }
}

impl<T> JsonSchema for NullablePatch<T>
where
    T: JsonSchema,
{
    fn schema_name() -> Cow<'static, str> {
        format!("NullablePatch_{}", T::schema_name()).into()
    }

    fn schema_id() -> Cow<'static, str> {
        format!("{}::NullablePatch<{}>", module_path!(), T::schema_id()).into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let value_schema = generator.subschema_for::<T>().to_value();
        json_schema!({"anyOf": [value_schema, {"type": "null"}]})
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct GetFocusInput {}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[allow(dead_code, reason = "used by the staged typed-handler migration")]
pub(super) struct GetTaskInput {
    pub(super) task_id: UuidValue,
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
pub(super) fn generated_input_schema<T: JsonSchema>() -> Value {
    let mut settings = SchemaSettings::draft07();
    settings.meta_schema = None;
    settings.inline_subschemas = true;
    let mut schema = settings.into_generator().root_schema_for::<T>().to_value();
    if let Some(object) = schema.as_object_mut() {
        object.remove("title");
        if object.get("type") == Some(&Value::String("object".to_string())) {
            object
                .entry("properties")
                .or_insert_with(|| Value::Object(Map::new()));
            object
                .entry("required")
                .or_insert_with(|| Value::Array(Vec::new()));
        }
    }
    schema
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
pub(super) fn decode_input<T: DeserializeOwned>(value: &Value) -> Result<T, ToolInputError> {
    let deserializer = value.clone().into_deserializer();
    serde_path_to_error::deserialize(deserializer).map_err(classify_decode_error)
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
fn classify_decode_error(error: serde_path_to_error::Error<serde_json::Error>) -> ToolInputError {
    let path = error.path().to_string();
    let message = error.inner().to_string();
    if let Some(reason) = error_reason(&message, SEMANTIC_ERROR_PREFIX) {
        return ToolInputError::Semantic {
            field: path,
            message: semantic_reason(reason),
        };
    }
    if let Some(reason) = error_reason(&message, SCHEMA_ERROR_PREFIX) {
        return ToolInputError::Schema(InvalidParams {
            field: path,
            reason: schema_reason(reason),
        });
    }
    structural_decode_error(path, &message)
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
fn error_reason<'a>(message: &'a str, prefix: &str) -> Option<&'a str> {
    let message = message.strip_prefix(prefix)?;
    Some(message.split(" at line ").next().unwrap_or_default())
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
fn semantic_reason(reason: &str) -> &'static str {
    match reason {
        "must be a valid UUID" => "must be a valid UUID",
        "must be a valid RFC 3339 date-time" => "must be a valid RFC 3339 date-time",
        "must be a valid ISO 8601 date" => "must be a valid ISO 8601 date",
        "is outside the supported integer range" => "is outside the supported integer range",
        _ => "contains an invalid value",
    }
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
fn schema_reason(reason: &str) -> &'static str {
    match reason {
        "must be a non-negative integer" => "must be a non-negative integer",
        "must not be empty" => "must not be empty",
        "must be a string or null" => "must be a string or null",
        _ => "has an invalid value",
    }
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
fn structural_decode_error(path: String, message: &str) -> ToolInputError {
    if let Some(field) = quoted_serde_field(message, "unknown field `") {
        return ToolInputError::Schema(InvalidParams {
            field: child_field_path(&path, &field, true),
            reason: "additional property is not allowed",
        });
    }
    if let Some(field) = quoted_serde_field(message, "missing field `") {
        return ToolInputError::Schema(InvalidParams {
            field: child_field_path(&path, &field, false),
            reason: "field is required",
        });
    }
    if (path.is_empty() || path == ".")
        && (message.contains("expected struct") || message.contains("expected a map"))
    {
        return ToolInputError::Schema(InvalidParams {
            field: "arguments".to_string(),
            reason: "must be an object",
        });
    }
    let reason = if message.contains("expected a string") {
        "must be a string"
    } else if message.contains("expected an integer") {
        "must be a non-negative integer"
    } else {
        "has an invalid type"
    };
    ToolInputError::Schema(InvalidParams {
        field: path,
        reason,
    })
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
fn child_field_path(parent: &str, field: &str, root_has_arguments_prefix: bool) -> String {
    if parent.is_empty() || parent == "." {
        return if root_has_arguments_prefix {
            format!("arguments.{field}")
        } else {
            field.to_string()
        };
    }
    if root_has_arguments_prefix && parent == field {
        return format!("arguments.{field}");
    }
    if root_has_arguments_prefix && parent.ends_with(&format!(".{field}")) {
        return parent.to_string();
    }
    format!("{parent}.{field}")
}

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
fn quoted_serde_field(message: &str, prefix: &str) -> Option<String> {
    let start = message.find(prefix)? + prefix.len();
    Some(message[start..].split('`').next()?.to_string())
}

pub(super) struct UpdateTaskInput {
    pub(super) task_id: Uuid,
    pub(super) estimated_work_minutes: Option<i64>,
    pub(super) deadline_time: Option<Option<DateTime<Local>>>,
    pub(super) category: Option<Option<ProjectCategory>>,
}

fn uuid_argument(
    arguments: &Map<String, Value>,
    field: &'static str,
) -> Result<Uuid, ToolInputError> {
    let value = string_argument(arguments, field).map_err(ToolInputError::Schema)?;
    Uuid::parse_str(value).map_err(|_| ToolInputError::Semantic {
        field: field.to_string(),
        message: "must be a valid UUID",
    })
}

fn datetime_argument(
    arguments: &Map<String, Value>,
    field: &'static str,
) -> Result<DateTime<Local>, ToolInputError> {
    let value = string_argument(arguments, field).map_err(ToolInputError::Schema)?;
    parse_local_datetime(value).map_err(|_| ToolInputError::Semantic {
        field: field.to_string(),
        message: "must be a valid RFC 3339 date-time",
    })
}

fn optional_datetime_argument(
    arguments: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<DateTime<Local>>, ToolInputError> {
    arguments
        .get(field)
        .map(|value| {
            let value = value.as_str().ok_or_else(|| {
                ToolInputError::Schema(InvalidParams {
                    field: field.to_string(),
                    reason: "must be a string",
                })
            })?;
            parse_local_datetime(value).map_err(|_| ToolInputError::Semantic {
                field: field.to_string(),
                message: "must be a valid RFC 3339 date-time",
            })
        })
        .transpose()
}

fn optional_non_negative_i64_argument(
    arguments: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<i64>, ToolInputError> {
    arguments
        .get(field)
        .map(|value| {
            if let Some(value) = value.as_i64() {
                return if value >= 0 {
                    Ok(value)
                } else {
                    Err(ToolInputError::Schema(InvalidParams {
                        field: field.to_string(),
                        reason: "must be a non-negative integer",
                    }))
                };
            }
            if value.as_u64().is_some() {
                return Err(ToolInputError::Semantic {
                    field: field.to_string(),
                    message: "is outside the supported integer range",
                });
            }
            Err(ToolInputError::Schema(InvalidParams {
                field: field.to_string(),
                reason: "must be a non-negative integer",
            }))
        })
        .transpose()
}

pub(super) fn update_task_input(arguments: &Value) -> Result<UpdateTaskInput, ToolInputError> {
    let arguments = validate_argument_object(
        arguments,
        &[
            "task_id",
            "estimated_work_minutes",
            "deadline_time",
            "category",
        ],
        &["task_id"],
    )
    .map_err(ToolInputError::Schema)?;
    if !["estimated_work_minutes", "deadline_time", "category"]
        .iter()
        .any(|field| arguments.contains_key(*field))
    {
        return Err(ToolInputError::Schema(InvalidParams {
            field: "arguments".to_string(),
            reason: "must include at least one field to update",
        }));
    }

    let task_id = uuid_argument(arguments, "task_id")?;
    let estimated_work_minutes =
        optional_non_negative_i64_argument(arguments, "estimated_work_minutes")?;
    let deadline_time = nullable_datetime_argument(arguments, "deadline_time")?;
    let category = nullable_category_argument(arguments, "category")?;

    Ok(UpdateTaskInput {
        task_id,
        estimated_work_minutes,
        deadline_time,
        category,
    })
}

fn nullable_datetime_argument(
    arguments: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<Option<DateTime<Local>>>, ToolInputError> {
    match arguments.get(field) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(value) => {
            let value = value.as_str().ok_or_else(|| {
                ToolInputError::Schema(InvalidParams {
                    field: field.to_string(),
                    reason: "must be a string or null",
                })
            })?;
            parse_local_datetime(value)
                .map(|value| Some(Some(value)))
                .map_err(|_| ToolInputError::Semantic {
                    field: field.to_string(),
                    message: "must be a valid RFC 3339 date-time",
                })
        }
    }
}

fn nullable_category_argument(
    arguments: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<Option<ProjectCategory>>, ToolInputError> {
    match arguments.get(field) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(value)) => parse_mcp_category(value)
            .map(|category| Some(Some(category)))
            .ok_or_else(|| {
                ToolInputError::Schema(InvalidParams {
                    field: field.to_string(),
                    reason: "must be a supported category or null",
                })
            }),
        Some(_) => Err(ToolInputError::Schema(InvalidParams {
            field: field.to_string(),
            reason: "must be a supported category or null",
        })),
    }
}

fn parse_mcp_category(value: &str) -> Option<ProjectCategory> {
    match value {
        "earning" => Some(ProjectCategory::Earning),
        "sustaining" => Some(ProjectCategory::Sustaining),
        "recovery" => Some(ProjectCategory::Recovery),
        "investment" => Some(ProjectCategory::Investment),
        "consumption" => Some(ProjectCategory::Consumption),
        _ => None,
    }
}

pub(super) fn complete_task_input(arguments: &Value) -> Result<CompleteTaskInput, ToolInputError> {
    let arguments = validate_argument_object(
        arguments,
        &["task_id", "finished_at", "additional_actual_work_seconds"],
        &["task_id"],
    )
    .map_err(ToolInputError::Schema)?;
    let task_id = uuid_argument(arguments, "task_id")?;
    let finished_at =
        optional_datetime_argument(arguments, "finished_at")?.unwrap_or_else(Local::now);
    let additional_actual_work_seconds =
        optional_non_negative_i64_argument(arguments, "additional_actual_work_seconds")?
            .unwrap_or(0);

    Ok(CompleteTaskInput {
        task_id,
        finished_at,
        additional_actual_work_seconds,
    })
}

pub(super) fn defer_task_input(
    arguments: &Value,
) -> Result<(Uuid, DateTime<Local>), ToolInputError> {
    let arguments = validate_argument_object(
        arguments,
        &["task_id", "pending_until"],
        &["task_id", "pending_until"],
    )
    .map_err(ToolInputError::Schema)?;
    let task_id = uuid_argument(arguments, "task_id")?;
    let pending_until = datetime_argument(arguments, "pending_until")?;
    Ok((task_id, pending_until))
}

pub(super) fn breakdown_task_input(
    arguments: &Value,
) -> Result<BreakdownTaskInput, ToolInputError> {
    let arguments = validate_argument_object(
        arguments,
        &["parent_id", "names", "pending_until"],
        &["parent_id", "names"],
    )
    .map_err(ToolInputError::Schema)?;
    let parent_id = uuid_argument(arguments, "parent_id")?;
    let names = arguments
        .get("names")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ToolInputError::Schema(InvalidParams {
                field: "names".to_string(),
                reason: "must be an array",
            })
        })?;
    if names.is_empty() {
        return Err(ToolInputError::Schema(InvalidParams {
            field: "names".to_string(),
            reason: "must contain at least one item",
        }));
    }
    let names = names
        .iter()
        .enumerate()
        .map(|(index, value)| match value.as_str() {
            Some("") => Err(ToolInputError::Schema(InvalidParams {
                field: format!("names[{index}]"),
                reason: "must not be empty",
            })),
            Some(value) => Ok(value.to_string()),
            None => Err(ToolInputError::Schema(InvalidParams {
                field: format!("names[{index}]"),
                reason: "must be a string",
            })),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let pending_until = optional_datetime_argument(arguments, "pending_until")?;

    Ok(BreakdownTaskInput {
        parent_id,
        names,
        pending_until,
    })
}

pub(super) fn create_task_input(arguments: &Value) -> Result<CreateTaskInput, ToolInputError> {
    let arguments = validate_argument_object(
        arguments,
        &["name", "estimated_work_minutes", "pending_until"],
        &["name"],
    )
    .map_err(ToolInputError::Schema)?;
    let name = string_argument(arguments, "name").map_err(ToolInputError::Schema)?;
    if name.is_empty() {
        return Err(ToolInputError::Schema(InvalidParams {
            field: "name".to_string(),
            reason: "must not be empty",
        }));
    }

    let estimated_work_minutes =
        optional_non_negative_i64_argument(arguments, "estimated_work_minutes")?;
    let pending_until = optional_datetime_argument(arguments, "pending_until")?;

    Ok(CreateTaskInput {
        name: name.to_string(),
        estimated_work_minutes,
        pending_until,
    })
}

pub(super) fn list_tasks_filter(
    arguments: Option<&Value>,
) -> Result<ListTasksFilter, ToolInputError> {
    let Some(arguments) = arguments else {
        return Ok(ListTasksFilter {
            period: None,
            statuses: vec![],
            categories: vec![],
        });
    };
    let arguments = validate_argument_object(arguments, &["period", "statuses", "categories"], &[])
        .map_err(ToolInputError::Schema)?;

    Ok(ListTasksFilter {
        period: arguments
            .get("period")
            .map(parse_period_filter)
            .transpose()?,
        statuses: parse_status_filters(arguments.get("statuses"))?,
        categories: parse_category_filters(arguments.get("categories"))?,
    })
}

pub(super) fn schedule_period(
    arguments: Option<&Value>,
    now: DateTime<Local>,
) -> Result<(DateTime<Local>, DateTime<Local>), ToolInputError> {
    let Some(arguments) = arguments else {
        return Ok((now, get_next_morning_datetime(now)));
    };
    let arguments = validate_argument_object(arguments, &["from", "until"], &[])
        .map_err(ToolInputError::Schema)?;
    let from = arguments
        .get("from")
        .map(|value| schedule_day_start(value, "from"))
        .transpose()?;
    let until = arguments
        .get("until")
        .map(|value| schedule_day_start(value, "until"))
        .transpose()?;

    let (from, until) = match (from, until) {
        (Some(from), Some(until)) => (from, until),
        (Some(from), None) => (from, get_next_morning_datetime(from)),
        (None, Some(until)) => (now, until),
        (None, None) => (now, get_next_morning_datetime(now)),
    };
    if from >= until {
        return Err(ToolInputError::Semantic {
            field: "until".to_string(),
            message: "must be later than from",
        });
    }

    Ok((from, until))
}

fn schedule_day_start(
    value: &Value,
    field: &'static str,
) -> Result<DateTime<Local>, ToolInputError> {
    let value = value.as_str().ok_or_else(|| {
        ToolInputError::Schema(InvalidParams {
            field: field.to_string(),
            reason: "must be a date string",
        })
    })?;
    let date =
        NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| ToolInputError::Semantic {
            field: field.to_string(),
            message: "must be a valid ISO 8601 date",
        })?;
    let local_noon = date.and_hms_opt(12, 0, 0).ok_or(ToolInputError::Semantic {
        field: field.to_string(),
        message: "must be a valid ISO 8601 date",
    })?;
    let local_noon = match local_noon.and_local_timezone(Local) {
        LocalResult::Single(datetime) => datetime,
        _ => {
            return Err(ToolInputError::Semantic {
                field: field.to_string(),
                message: "must resolve to a local date-time",
            })
        }
    };

    Ok(get_next_morning_datetime(local_noon) - Duration::days(1))
}

fn parse_period_filter(value: &Value) -> Result<TaskPeriodFilter, ToolInputError> {
    let period = value.as_object().ok_or_else(|| {
        ToolInputError::Schema(InvalidParams {
            field: "period".to_string(),
            reason: "must be an object",
        })
    })?;
    if let Some(field) = period
        .keys()
        .find(|field| !["field", "from", "until"].contains(&field.as_str()))
    {
        return Err(ToolInputError::Schema(InvalidParams {
            field: format!("period.{field}"),
            reason: "additional property is not allowed",
        }));
    }
    for field in ["field", "from", "until"] {
        if !period.contains_key(field) {
            return Err(ToolInputError::Schema(InvalidParams {
                field: format!("period.{field}"),
                reason: "field is required",
            }));
        }
    }

    let field = match required_nested_string(period, "period", "field")? {
        "scheduled_start" => TaskPeriodField::ScheduledStart,
        "created_at" => TaskPeriodField::CreatedAt,
        "deadline" => TaskPeriodField::Deadline,
        "completed_at" => TaskPeriodField::CompletedAt,
        _ => {
            return Err(ToolInputError::Schema(InvalidParams {
                field: "period.field".to_string(),
                reason: "must be a supported period field",
            }))
        }
    };
    let from = parse_datetime(
        required_nested_string(period, "period", "from")?,
        "period.from",
    )?;
    let until = parse_datetime(
        required_nested_string(period, "period", "until")?,
        "period.until",
    )?;

    Ok(TaskPeriodFilter { field, from, until })
}

fn required_nested_string<'a>(
    object: &'a Map<String, Value>,
    object_name: &str,
    field: &str,
) -> Result<&'a str, ToolInputError> {
    object.get(field).and_then(Value::as_str).ok_or_else(|| {
        ToolInputError::Schema(InvalidParams {
            field: format!("{object_name}.{field}"),
            reason: "must be a string",
        })
    })
}

fn parse_datetime(value: &str, field: &'static str) -> Result<DateTime<Local>, ToolInputError> {
    parse_local_datetime(value).map_err(|_| ToolInputError::Semantic {
        field: field.to_string(),
        message: "must be a valid RFC 3339 date-time",
    })
}

fn parse_local_datetime(value: &str) -> Result<DateTime<Local>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(|time| time.with_timezone(&Local))
}

fn parse_status_filters(value: Option<&Value>) -> Result<Vec<Status>, ToolInputError> {
    let Some(value) = value else {
        return Ok(vec![]);
    };
    let values = value.as_array().ok_or_else(|| {
        ToolInputError::Schema(InvalidParams {
            field: "statuses".to_string(),
            reason: "must be an array",
        })
    })?;

    values
        .iter()
        .enumerate()
        .map(|(index, value)| match value.as_str() {
            Some("todo") => Ok(Status::Todo),
            Some("pending") => Ok(Status::Pending),
            Some("done") => Ok(Status::Done),
            _ => Err(ToolInputError::Schema(InvalidParams {
                field: format!("statuses[{index}]"),
                reason: "must be todo, pending, or done",
            })),
        })
        .collect()
}

fn parse_category_filters(
    value: Option<&Value>,
) -> Result<Vec<Option<ProjectCategory>>, ToolInputError> {
    let Some(value) = value else {
        return Ok(vec![]);
    };
    let values = value.as_array().ok_or_else(|| {
        ToolInputError::Schema(InvalidParams {
            field: "categories".to_string(),
            reason: "must be an array",
        })
    })?;

    values
        .iter()
        .enumerate()
        .map(|(index, value)| match value {
            Value::Null => Ok(None),
            Value::String(value) => parse_mcp_category(value).map(Some).ok_or_else(|| {
                ToolInputError::Schema(InvalidParams {
                    field: format!("categories[{index}]"),
                    reason: "must be a supported category or null",
                })
            }),
            _ => Err(ToolInputError::Schema(InvalidParams {
                field: format!("categories[{index}]"),
                reason: "must be a supported category or null",
            })),
        })
        .collect()
}

pub(super) fn validate_argument_object<'a>(
    arguments: &'a Value,
    allowed_fields: &[&str],
    required_fields: &[&str],
) -> Result<&'a Map<String, Value>, InvalidParams> {
    let Some(arguments) = arguments.as_object() else {
        return Err(InvalidParams {
            field: "arguments".to_string(),
            reason: "must be an object",
        });
    };

    if let Some(field) = arguments
        .keys()
        .find(|field| !allowed_fields.contains(&field.as_str()))
    {
        return Err(InvalidParams {
            field: format!("arguments.{field}"),
            reason: "additional property is not allowed",
        });
    }

    if let Some(field) = required_fields
        .iter()
        .find(|field| !arguments.contains_key(**field))
    {
        return Err(InvalidParams {
            field: (*field).to_string(),
            reason: "field is required",
        });
    }

    Ok(arguments)
}

pub(super) fn string_argument<'a>(
    arguments: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, InvalidParams> {
    arguments
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| InvalidParams {
            field: field.to_string(),
            reason: "must be a string",
        })
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CommonInputContractFields {
    task_id: UuidValue,
    #[serde(default)]
    deadline_time: NullablePatch<Rfc3339DateTime>,
    pending_until: Rfc3339DateTime,
    date: IsoDate,
    work_seconds: NonNegativeI64,
    name: NonEmptyString,
}

#[cfg(test)]
struct CommonInputContract {
    schema: Value,
}

#[cfg(test)]
impl CommonInputContract {
    fn schema(&self) -> &Value {
        &self.schema
    }

    fn decode(&self, value: &Value) -> Result<(), ToolInputError> {
        decode_input::<CommonInputContractFields>(value).map(|_| ())
    }
}

#[cfg(test)]
fn common_input_contract() -> CommonInputContract {
    CommonInputContract {
        schema: generated_input_schema::<CommonInputContractFields>(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        common_input_contract, decode_input, generated_input_schema, GetFocusInput,
        GetScheduleInput, GetTaskInput, ListTasksInput, NonNegativeI64, OptionalValue,
        Rfc3339DateTime, ToolInputError,
    };
    use chrono::{DateTime, Local};
    use schemars::JsonSchema;
    use serde::Deserialize;
    use serde_json::{json, Value};

    #[derive(Clone, Copy, Debug)]
    enum ExpectedDecode {
        Valid,
        Schema {
            field: &'static str,
            reason: &'static str,
        },
        Semantic {
            field: &'static str,
            reason: &'static str,
        },
    }

    struct ContractCase {
        name: &'static str,
        input: Value,
        schema_accepts: bool,
        decode: ExpectedDecode,
    }

    #[test]
    fn common_input_constraints_are_checked_against_the_same_case_set() {
        let contract = common_input_contract();
        let validator = jsonschema::options()
            .should_validate_formats(true)
            .build(contract.schema())
            .expect("generated common input schema must be valid JSON Schema");

        for case in common_input_cases() {
            assert_eq!(
                validator.is_valid(&case.input),
                case.schema_accepts,
                "schema outcome differed for {}",
                case.name
            );
            assert_decode_outcome(case.name, contract.decode(&case.input), case.decode);
        }
    }

    #[test]
    fn reference_tool_inputs_match_public_schema_and_decode_contract() {
        assert_reference_input_contract::<GetFocusInput>(
            "get_focus",
            json!({
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }),
            vec![
                ContractCase {
                    name: "empty object",
                    input: json!({}),
                    schema_accepts: true,
                    decode: ExpectedDecode::Valid,
                },
                ContractCase {
                    name: "unknown field",
                    input: json!({"extra": true}),
                    schema_accepts: false,
                    decode: ExpectedDecode::Schema {
                        field: "arguments.extra",
                        reason: "additional property is not allowed",
                    },
                },
                ContractCase {
                    name: "arguments has wrong type",
                    input: json!(42),
                    schema_accepts: false,
                    decode: ExpectedDecode::Schema {
                        field: "arguments",
                        reason: "must be an object",
                    },
                },
            ],
        );

        assert_reference_input_contract::<GetTaskInput>(
            "get_task",
            json!({
                "type": "object",
                "properties": {
                    "task_id": {"type": "string", "format": "uuid"}
                },
                "required": ["task_id"],
                "additionalProperties": false
            }),
            vec![
                ContractCase {
                    name: "valid UUID",
                    input: json!({"task_id": "80d7db87-324e-4e8d-a5b7-ff78cd5bf39a"}),
                    schema_accepts: true,
                    decode: ExpectedDecode::Valid,
                },
                ContractCase {
                    name: "missing task_id",
                    input: json!({}),
                    schema_accepts: false,
                    decode: ExpectedDecode::Schema {
                        field: "task_id",
                        reason: "field is required",
                    },
                },
                ContractCase {
                    name: "task_id has wrong type",
                    input: json!({"task_id": 42}),
                    schema_accepts: false,
                    decode: ExpectedDecode::Schema {
                        field: "task_id",
                        reason: "must be a string",
                    },
                },
                ContractCase {
                    name: "unknown field",
                    input: json!({
                        "task_id": "80d7db87-324e-4e8d-a5b7-ff78cd5bf39a",
                        "extra": true
                    }),
                    schema_accepts: false,
                    decode: ExpectedDecode::Schema {
                        field: "arguments.extra",
                        reason: "additional property is not allowed",
                    },
                },
                ContractCase {
                    name: "invalid UUID",
                    input: json!({"task_id": "not-a-uuid"}),
                    schema_accepts: false,
                    decode: ExpectedDecode::Semantic {
                        field: "task_id",
                        reason: "must be a valid UUID",
                    },
                },
            ],
        );
    }

    #[test]
    fn search_tool_inputs_match_public_schema_and_decode_contract() {
        assert_reference_input_contract::<ListTasksInput>(
            "list_tasks",
            public_tool_schema("list_tasks"),
            list_tasks_input_cases(),
        );
        assert_reference_input_contract::<GetScheduleInput>(
            "get_schedule",
            public_tool_schema("get_schedule"),
            get_schedule_input_cases(),
        );
    }

    fn list_tasks_input_cases() -> Vec<ContractCase> {
        let valid_period = json!({
            "field": "created_at",
            "from": "2026-08-10T00:00:00+09:00",
            "until": "2026-08-11T00:00:00+09:00"
        });
        vec![
            ContractCase {
                name: "empty filter",
                input: json!({}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "all filters",
                input: json!({
                    "period": valid_period,
                    "statuses": ["todo", "pending", "done"],
                    "categories": ["earning", "sustaining", "recovery", "investment", "consumption", null]
                }),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "scheduled_start period field",
                input: json!({
                    "period": {
                        "field": "scheduled_start",
                        "from": "2026-08-10T00:00:00+09:00",
                        "until": "2026-08-11T00:00:00+09:00"
                    }
                }),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "deadline period field",
                input: json!({
                    "period": {
                        "field": "deadline",
                        "from": "2026-08-10T00:00:00+09:00",
                        "until": "2026-08-11T00:00:00+09:00"
                    }
                }),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "completed_at period field",
                input: json!({
                    "period": {
                        "field": "completed_at",
                        "from": "2026-08-10T00:00:00+09:00",
                        "until": "2026-08-11T00:00:00+09:00"
                    }
                }),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "period is missing a required field",
                input: json!({
                    "period": {
                        "field": "created_at",
                        "from": "2026-08-10T00:00:00+09:00"
                    }
                }),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "period.until",
                    reason: "field is required",
                },
            },
            ContractCase {
                name: "period has an unknown field",
                input: json!({
                    "period": {
                        "field": "created_at",
                        "from": "2026-08-10T00:00:00+09:00",
                        "until": "2026-08-11T00:00:00+09:00",
                        "extra": true
                    }
                }),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "period.extra",
                    reason: "additional property is not allowed",
                },
            },
            ContractCase {
                name: "period has wrong type",
                input: json!({"period": true}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "period",
                    reason: "must be an object",
                },
            },
            ContractCase {
                name: "period field has wrong type",
                input: json!({
                    "period": {
                        "field": 42,
                        "from": "2026-08-10T00:00:00+09:00",
                        "until": "2026-08-11T00:00:00+09:00"
                    }
                }),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "period.field",
                    reason: "must be a string",
                },
            },
            ContractCase {
                name: "period field has unsupported value",
                input: json!({
                    "period": {
                        "field": "invalid",
                        "from": "2026-08-10T00:00:00+09:00",
                        "until": "2026-08-11T00:00:00+09:00"
                    }
                }),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "period.field",
                    reason: "must be a supported period field",
                },
            },
            ContractCase {
                name: "period date-time has wrong type",
                input: json!({
                    "period": {
                        "field": "created_at",
                        "from": 42,
                        "until": "2026-08-11T00:00:00+09:00"
                    }
                }),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "period.from",
                    reason: "must be a string",
                },
            },
            ContractCase {
                name: "period date-time is invalid",
                input: json!({
                    "period": {
                        "field": "created_at",
                        "from": "not-a-date",
                        "until": "2026-08-11T00:00:00+09:00"
                    }
                }),
                schema_accepts: false,
                decode: ExpectedDecode::Semantic {
                    field: "period.from",
                    reason: "must be a valid RFC 3339 date-time",
                },
            },
            ContractCase {
                name: "reversed period is decoded for handler validation",
                input: json!({
                    "period": {
                        "field": "created_at",
                        "from": "2026-08-11T00:00:00+09:00",
                        "until": "2026-08-10T00:00:00+09:00"
                    }
                }),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "statuses has wrong type",
                input: json!({"statuses": "pending"}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "statuses",
                    reason: "must be an array",
                },
            },
            ContractCase {
                name: "status has unsupported value",
                input: json!({"statuses": ["invalid"]}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "statuses[0]",
                    reason: "must be todo, pending, or done",
                },
            },
            ContractCase {
                name: "status has wrong type",
                input: json!({"statuses": [42]}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "statuses[0]",
                    reason: "must be todo, pending, or done",
                },
            },
            ContractCase {
                name: "categories can contain null",
                input: json!({"categories": [null]}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "categories array cannot be null",
                input: json!({"categories": null}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "categories",
                    reason: "must be an array",
                },
            },
            ContractCase {
                name: "category has unsupported value",
                input: json!({"categories": ["invalid"]}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "categories[0]",
                    reason: "must be a supported category or null",
                },
            },
            ContractCase {
                name: "category has wrong type",
                input: json!({"categories": [42]}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "categories[0]",
                    reason: "must be a supported category or null",
                },
            },
            ContractCase {
                name: "unknown field",
                input: json!({"extra": true}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "arguments.extra",
                    reason: "additional property is not allowed",
                },
            },
            ContractCase {
                name: "arguments has wrong type",
                input: json!(42),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "arguments",
                    reason: "must be an object",
                },
            },
        ]
    }

    fn get_schedule_input_cases() -> Vec<ContractCase> {
        vec![
            ContractCase {
                name: "empty range",
                input: json!({}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "complete range",
                input: json!({"from": "2026-08-10", "until": "2026-08-11"}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "from only",
                input: json!({"from": "2026-08-10"}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "until only",
                input: json!({"until": "2026-08-11"}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "from cannot be null",
                input: json!({"from": null}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "from",
                    reason: "must be a date string",
                },
            },
            ContractCase {
                name: "until has wrong type",
                input: json!({"until": 42}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "until",
                    reason: "must be a date string",
                },
            },
            ContractCase {
                name: "date is invalid",
                input: json!({"from": "2026-02-30"}),
                schema_accepts: false,
                decode: ExpectedDecode::Semantic {
                    field: "from",
                    reason: "must be a valid ISO 8601 date",
                },
            },
            ContractCase {
                name: "reversed range is decoded for handler validation",
                input: json!({"from": "2026-08-11", "until": "2026-08-10"}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "unknown field",
                input: json!({"extra": true}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "arguments.extra",
                    reason: "additional property is not allowed",
                },
            },
            ContractCase {
                name: "arguments has wrong type",
                input: json!(42),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "arguments",
                    reason: "must be an object",
                },
            },
        ]
    }

    fn public_tool_schema(tool_name: &str) -> Value {
        let tools: Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/mcp/tools-list.json"))
                .expect("MCP tools/list golden fixture must be valid JSON");
        tools
            .as_array()
            .expect("MCP tools/list golden fixture must be an array")
            .iter()
            .find(|tool| tool["name"] == tool_name)
            .unwrap_or_else(|| panic!("{tool_name} must exist in MCP tools/list golden fixture"))
            ["inputSchema"]
            .clone()
    }

    fn assert_reference_input_contract<T>(
        tool_name: &str,
        expected_schema: Value,
        cases: Vec<ContractCase>,
    ) where
        T: for<'de> Deserialize<'de> + JsonSchema,
    {
        let schema = generated_input_schema::<T>();
        assert_eq!(
            schema, expected_schema,
            "generated schema drifted from the public {tool_name} schema"
        );
        let validator = jsonschema::options()
            .should_validate_formats(true)
            .build(&schema)
            .unwrap_or_else(|error| panic!("generated {tool_name} schema must be valid: {error}"));

        for case in cases {
            assert_eq!(
                validator.is_valid(&case.input),
                case.schema_accepts,
                "schema outcome differed for {tool_name}: {}",
                case.name
            );
            assert_decode_outcome(
                case.name,
                decode_input::<T>(&case.input).map(|_| ()),
                case.decode,
            );
        }
    }

    fn common_input_cases() -> Vec<ContractCase> {
        vec![
            ContractCase {
                name: "valid values",
                input: valid_input(),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "arguments has wrong type",
                input: json!(42),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "arguments",
                    reason: "must be an object",
                },
            },
            ContractCase {
                name: "unknown field",
                input: with_field(valid_input(), "extra", json!(true)),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "arguments.extra",
                    reason: "additional property is not allowed",
                },
            },
            ContractCase {
                name: "unknown field cannot inject an error marker",
                input: with_field(valid_input(), "mcp-semantic:x", json!(true)),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "arguments.mcp-semantic:x",
                    reason: "additional property is not allowed",
                },
            },
            ContractCase {
                name: "required field",
                input: without_field(valid_input(), "task_id"),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "task_id",
                    reason: "field is required",
                },
            },
            ContractCase {
                name: "nullable value",
                input: with_field(valid_input(), "deadline_time", Value::Null),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "nullable value has wrong type",
                input: with_field(valid_input(), "deadline_time", json!(42)),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "deadline_time",
                    reason: "must be a string or null",
                },
            },
            ContractCase {
                name: "invalid UUID",
                input: with_field(valid_input(), "task_id", json!("not-a-uuid")),
                schema_accepts: false,
                decode: ExpectedDecode::Semantic {
                    field: "task_id",
                    reason: "must be a valid UUID",
                },
            },
            ContractCase {
                name: "invalid RFC 3339 date-time",
                input: with_field(valid_input(), "pending_until", json!("2026-08-19 10:00")),
                schema_accepts: false,
                decode: ExpectedDecode::Semantic {
                    field: "pending_until",
                    reason: "must be a valid RFC 3339 date-time",
                },
            },
            ContractCase {
                name: "invalid ISO date",
                input: with_field(valid_input(), "date", json!("2026-02-30")),
                schema_accepts: false,
                decode: ExpectedDecode::Semantic {
                    field: "date",
                    reason: "must be a valid ISO 8601 date",
                },
            },
            ContractCase {
                name: "negative i64",
                input: with_field(valid_input(), "work_seconds", json!(-1)),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "work_seconds",
                    reason: "must be a non-negative integer",
                },
            },
            ContractCase {
                name: "fractional number",
                input: with_field(valid_input(), "work_seconds", json!(1.5)),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "work_seconds",
                    reason: "must be a non-negative integer",
                },
            },
            ContractCase {
                name: "u64 outside i64 range",
                input: with_field(valid_input(), "work_seconds", json!(u64::MAX)),
                schema_accepts: true,
                decode: ExpectedDecode::Semantic {
                    field: "work_seconds",
                    reason: "is outside the supported integer range",
                },
            },
            ContractCase {
                name: "empty string",
                input: with_field(valid_input(), "name", json!("")),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "name",
                    reason: "must not be empty",
                },
            },
        ]
    }

    fn valid_input() -> Value {
        json!({
            "task_id": "80d7db87-324e-4e8d-a5b7-ff78cd5bf39a",
            "deadline_time": "2026-08-19T10:00:00+09:00",
            "pending_until": "2026-08-20T10:00:00+09:00",
            "date": "2026-08-19",
            "work_seconds": 0,
            "name": "write contract test"
        })
    }

    #[allow(dead_code)]
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct NestedArguments {
        period: NestedPeriod,
    }

    #[allow(dead_code)]
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct NestedPeriod {
        from: String,
        until: String,
    }

    #[test]
    fn decode_input_preserves_nested_field_paths() {
        assert_decode_outcome(
            "nested missing field",
            decode_input::<NestedArguments>(&json!({
                "period": {"from": "2026-08-19T10:00:00+09:00"}
            }))
            .map(|_| ()),
            ExpectedDecode::Schema {
                field: "period.until",
                reason: "field is required",
            },
        );
        assert_decode_outcome(
            "nested unknown field",
            decode_input::<NestedArguments>(&json!({
                "period": {
                    "from": "2026-08-19T10:00:00+09:00",
                    "until": "2026-08-20T10:00:00+09:00",
                    "extra": true
                }
            }))
            .map(|_| ()),
            ExpectedDecode::Schema {
                field: "period.extra",
                reason: "additional property is not allowed",
            },
        );
    }

    #[allow(dead_code)]
    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct OptionalInput {
        #[serde(default)]
        pending_until: OptionalValue<Rfc3339DateTime>,
    }

    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct IntegerInput {
        work_seconds: NonNegativeI64,
    }

    #[derive(Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct EmptyInput {}

    #[test]
    fn generated_object_schema_preserves_empty_required_array() {
        for (name, schema) in [
            ("empty input", generated_input_schema::<EmptyInput>()),
            (
                "all optional input",
                generated_input_schema::<OptionalInput>(),
            ),
        ] {
            assert_eq!(schema.get("required"), Some(&json!([])), "{name}");
        }
    }

    #[test]
    fn json_integer_representation_matches_schema_and_decode() {
        let schema = generated_input_schema::<IntegerInput>();
        let validator = jsonschema::options()
            .build(&schema)
            .expect("generated integer input schema must be valid JSON Schema");
        let cases = [
            ("integral float", json!({"work_seconds": 1.0}), true),
            ("fractional number", json!({"work_seconds": 1.5}), false),
        ];

        for (name, input, schema_accepts) in cases {
            assert_eq!(
                validator.is_valid(&input),
                schema_accepts,
                "schema outcome differed for {name}"
            );
            if schema_accepts {
                let decoded = decode_input::<IntegerInput>(&input)
                    .unwrap_or_else(|_| panic!("decode unexpectedly rejected {name}"));
                assert_eq!(
                    decoded.work_seconds.0, 1,
                    "decoded value differed for {name}"
                );
            } else {
                assert_decode_outcome(
                    name,
                    decode_input::<IntegerInput>(&input).map(|_| ()),
                    ExpectedDecode::Schema {
                        field: "work_seconds",
                        reason: "must be a non-negative integer",
                    },
                );
            }
        }

        let exact_i64 = json!({"work_seconds": 9_223_372_036_854_774_784.0_f64});
        assert!(validator.is_valid(&exact_i64));
        let decoded = match decode_input::<IntegerInput>(&exact_i64) {
            Ok(decoded) => decoded,
            Err(error) => panic!(
                "exact f64 integer below i64::MAX must be accepted: {}",
                describe_decode(Err(error))
            ),
        };
        assert_eq!(decoded.work_seconds.0, 9_223_372_036_854_774_784_i64);

        let i64_upper_bound = json!({"work_seconds": 9_223_372_036_854_775_808.0_f64});
        assert!(validator.is_valid(&i64_upper_bound));
        assert_decode_outcome(
            "f64 at i64 upper bound",
            decode_input::<IntegerInput>(&i64_upper_bound).map(|_| ()),
            ExpectedDecode::Semantic {
                field: "work_seconds",
                reason: "is outside the supported integer range",
            },
        );

        let negative_zero = json!({"work_seconds": -0.0_f64});
        assert!(validator.is_valid(&negative_zero));
        let decoded = match decode_input::<IntegerInput>(&negative_zero) {
            Ok(decoded) => decoded,
            Err(error) => panic!(
                "negative zero must be accepted as zero: {}",
                describe_decode(Err(error))
            ),
        };
        assert_eq!(decoded.work_seconds.0, 0);
    }

    #[test]
    fn optional_non_null_input_matches_schema_for_missing_null_and_value() {
        let schema = generated_input_schema::<OptionalInput>();
        let validator = jsonschema::options()
            .should_validate_formats(true)
            .build(&schema)
            .expect("generated optional input schema must be valid JSON Schema");
        let missing = json!({});
        assert!(validator.is_valid(&missing));
        let decoded = match decode_input::<OptionalInput>(&missing) {
            Ok(decoded) => decoded,
            Err(error) => panic!(
                "missing optional input must be accepted: {}",
                describe_decode(Err(error))
            ),
        };
        assert!(matches!(decoded.pending_until, OptionalValue::Missing));

        let value = json!({"pending_until": "2026-08-20T10:00:00+09:00"});
        assert!(validator.is_valid(&value));
        let decoded = match decode_input::<OptionalInput>(&value) {
            Ok(decoded) => decoded,
            Err(error) => panic!(
                "present optional input must be accepted: {}",
                describe_decode(Err(error))
            ),
        };
        let OptionalValue::Value(actual) = decoded.pending_until else {
            panic!("present optional input must decode as OptionalValue::Value");
        };
        assert_eq!(
            actual.0,
            DateTime::parse_from_rfc3339("2026-08-20T10:00:00+09:00")
                .expect("test date-time must be valid")
                .with_timezone(&Local)
        );

        let null = json!({"pending_until": null});
        assert!(!validator.is_valid(&null));
        assert_decode_outcome(
            "null",
            decode_input::<OptionalInput>(&null).map(|_| ()),
            ExpectedDecode::Schema {
                field: "pending_until",
                reason: "must be a string",
            },
        );
    }

    fn with_field(mut input: Value, field: &str, value: Value) -> Value {
        input
            .as_object_mut()
            .expect("test input must be an object")
            .insert(field.to_string(), value);
        input
    }

    fn without_field(mut input: Value, field: &str) -> Value {
        input
            .as_object_mut()
            .expect("test input must be an object")
            .remove(field);
        input
    }

    fn assert_decode_outcome(
        case_name: &str,
        actual: Result<(), ToolInputError>,
        expected: ExpectedDecode,
    ) {
        match (actual, expected) {
            (Ok(()), ExpectedDecode::Valid) => {}
            (Err(ToolInputError::Schema(actual)), ExpectedDecode::Schema { field, reason }) => {
                assert_eq!(actual.field, field, "schema field differed for {case_name}");
                assert_eq!(
                    actual.reason, reason,
                    "schema reason differed for {case_name}"
                );
            }
            (
                Err(ToolInputError::Semantic {
                    field: actual_field,
                    message: actual_reason,
                }),
                ExpectedDecode::Semantic { field, reason },
            ) => {
                assert_eq!(
                    actual_field, field,
                    "semantic field differed for {case_name}"
                );
                assert_eq!(
                    actual_reason, reason,
                    "semantic reason differed for {case_name}"
                );
            }
            (actual, expected) => panic!(
                "decode outcome differed for {case_name}: actual={}, expected={expected:?}",
                describe_decode(actual)
            ),
        }
    }

    fn describe_decode(result: Result<(), ToolInputError>) -> String {
        match result {
            Ok(()) => "valid".to_string(),
            Err(ToolInputError::Schema(error)) => {
                format!("schema({}, {})", error.field, error.reason)
            }
            Err(ToolInputError::Semantic { field, message }) => {
                format!("semantic({field}, {message})")
            }
        }
    }
}
