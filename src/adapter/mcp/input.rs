use super::error::InvalidParams;
use crate::application::daily_capacity::{try_logical_date_start, try_next_logical_date_start};
use crate::application::task_use_case::{
    ApplicationError, BreakdownTaskInput as ApplicationBreakdownTaskInput,
    CompleteTaskInput as ApplicationCompleteTaskInput,
    CreateTaskInput as ApplicationCreateTaskInput, ListTasksFilter, TaskPeriodField,
    TaskPeriodFilter,
};
use crate::entity::task::{ProjectCategory, Status};
use chrono::{DateTime, Local, NaiveDate};
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
    Application(ApplicationError),
}

const SCHEMA_ERROR_PREFIX: &str = "mcp-schema:";
const SEMANTIC_ERROR_PREFIX: &str = "mcp-semantic:";

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
        json_schema!({
            "type": "string",
            "format": "uuid",
            "description": "A valid UUID string.",
            "examples": ["80d7db87-324e-4e8d-a5b7-ff78cd5bf39a"]
        })
    }
}

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
        json_schema!({
            "type": "string",
            "format": "date-time",
            "description": "An RFC 3339 date-time string with Z or a numeric UTC offset.",
            "examples": ["2026-08-29T10:00:00+09:00", "2026-08-29T01:00:00Z"]
        })
    }
}

pub(super) struct IsoDate(pub(super) NaiveDate);

impl<'de> Deserialize<'de> for IsoDate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let Value::String(value) = value else {
            return Err(serde::de::Error::custom(format!(
                "{SCHEMA_ERROR_PREFIX}must be a YYYY-MM-DD date string"
            )));
        };
        NaiveDate::parse_from_str(&value, "%Y-%m-%d")
            .map(Self)
            .map_err(|_| {
                serde::de::Error::custom(format!(
                    "{SEMANTIC_ERROR_PREFIX}must be a valid calendar date in YYYY-MM-DD format"
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
        json_schema!({
            "type": "string",
            "format": "date",
            "description": "A calendar date in YYYY-MM-DD format without a time or time zone.",
            "examples": ["2026-08-29"]
        })
    }
}

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
        json_schema!({
            "type": "integer",
            "minimum": 0,
            "description": "A non-negative integer."
        })
    }
}

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
        json_schema!({
            "type": "string",
            "minLength": 1,
            "description": "A non-empty string."
        })
    }
}

pub(super) struct NonEmptyVec<T>(pub(super) Vec<T>);

impl<'de, T> Deserialize<'de> for NonEmptyVec<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = Vec::<T>::deserialize(deserializer)?;
        if values.is_empty() {
            Err(serde::de::Error::custom(format!(
                "{SCHEMA_ERROR_PREFIX}must contain at least one item"
            )))
        } else {
            Ok(Self(values))
        }
    }
}

impl<T> JsonSchema for NonEmptyVec<T>
where
    T: JsonSchema,
{
    fn schema_name() -> Cow<'static, str> {
        format!("NonEmptyVec_{}", T::schema_name()).into()
    }

    fn schema_id() -> Cow<'static, str> {
        format!("{}::NonEmptyVec<{}>", module_path!(), T::schema_id()).into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let item_schema = generator.subschema_for::<T>().to_value();
        json_schema!({"type": "array", "items": item_schema, "minItems": 1})
    }
}

#[derive(Default)]
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
pub(super) enum NullablePatch<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}

pub(super) trait NullableValue {
    const WRONG_TYPE_REASON: &'static str;
}

impl NullableValue for Rfc3339DateTime {
    const WRONG_TYPE_REASON: &'static str = "must be a string or null";
}

impl NullableValue for ProjectCategoryValue {
    const WRONG_TYPE_REASON: &'static str = "must be a supported category or null";
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
pub(super) struct GetTaskInput {
    /// The UUID of the existing task to return.
    pub(super) task_id: UuidValue,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateTaskInput {
    pub(super) name: NonEmptyString,
    #[serde(default)]
    pub(super) estimated_work_minutes: OptionalValue<NonNegativeI64>,
    #[serde(default)]
    pub(super) pending_until: OptionalValue<Rfc3339DateTime>,
}

impl CreateTaskInput {
    pub(super) fn into_application(self) -> ApplicationCreateTaskInput {
        let estimated_work_minutes = match self.estimated_work_minutes {
            OptionalValue::Missing => None,
            OptionalValue::Value(minutes) => Some(minutes.0),
        };

        ApplicationCreateTaskInput {
            name: self.name.0,
            estimated_work_minutes,
            pending_until: match self.pending_until {
                OptionalValue::Missing => None,
                OptionalValue::Value(pending_until) => Some(pending_until.0),
            },
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct BreakdownTaskInput {
    pub(super) parent_id: UuidValue,
    pub(super) names: NonEmptyVec<NonEmptyString>,
    #[serde(default)]
    pub(super) pending_until: OptionalValue<Rfc3339DateTime>,
}

impl BreakdownTaskInput {
    pub(super) fn into_application(self) -> ApplicationBreakdownTaskInput {
        ApplicationBreakdownTaskInput {
            parent_id: self.parent_id.0,
            names: self.names.0.into_iter().map(|name| name.0).collect(),
            pending_until: match self.pending_until {
                OptionalValue::Missing => None,
                OptionalValue::Value(pending_until) => Some(pending_until.0),
            },
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct DeferTaskInput {
    pub(super) task_id: UuidValue,
    pub(super) pending_until: Rfc3339DateTime,
}

impl DeferTaskInput {
    pub(super) fn into_parts(self) -> (Uuid, DateTime<Local>) {
        (self.task_id.0, self.pending_until.0)
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct DeferRoutineTaskInput {
    pub(super) task_id: UuidValue,
}

impl DeferRoutineTaskInput {
    pub(super) fn into_task_id(self) -> Uuid {
        self.task_id.0
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CompleteTaskInput {
    pub(super) task_id: UuidValue,
    #[serde(default)]
    pub(super) finished_at: OptionalValue<Rfc3339DateTime>,
    #[serde(default = "zero_non_negative")]
    #[schemars(schema_with = "additional_work_seconds_schema")]
    pub(super) additional_actual_work_seconds: NonNegativeI64,
}

impl CompleteTaskInput {
    pub(super) fn into_application(
        self,
        operation_now: DateTime<Local>,
    ) -> ApplicationCompleteTaskInput {
        ApplicationCompleteTaskInput {
            task_id: self.task_id.0,
            finished_at: match self.finished_at {
                OptionalValue::Missing => operation_now,
                OptionalValue::Value(finished_at) => finished_at.0,
            },
            additional_actual_work_seconds: self.additional_actual_work_seconds.0,
        }
    }
}

fn zero_non_negative() -> NonNegativeI64 {
    NonNegativeI64(0)
}

fn additional_work_seconds_schema(generator: &mut SchemaGenerator) -> Schema {
    let mut schema = generator.subschema_for::<NonNegativeI64>();
    schema
        .ensure_object()
        .insert("default".to_string(), Value::from(0));
    schema
}

#[derive(Clone, Copy)]
enum UpdateTaskField {
    EstimatedWorkMinutes,
    DeadlineTime,
    Category,
}

impl UpdateTaskField {
    fn name(self) -> &'static str {
        match self {
            Self::EstimatedWorkMinutes => "estimated_work_minutes",
            Self::DeadlineTime => "deadline_time",
            Self::Category => "category",
        }
    }

    fn is_provided(self, fields: &UpdateTaskInputFields) -> bool {
        match self {
            Self::EstimatedWorkMinutes => {
                !matches!(fields.estimated_work_minutes, OptionalValue::Missing)
            }
            Self::DeadlineTime => !matches!(fields.deadline_time, NullablePatch::Missing),
            Self::Category => !matches!(fields.category, NullablePatch::Missing),
        }
    }
}

const UPDATE_TASK_FIELDS: [UpdateTaskField; 3] = [
    UpdateTaskField::EstimatedWorkMinutes,
    UpdateTaskField::DeadlineTime,
    UpdateTaskField::Category,
];
const UPDATE_TASK_FIELD_REQUIRED_REASON: &str = "must include at least one field to update";

#[derive(Deserialize, JsonSchema)]
#[serde(try_from = "UpdateTaskInputFields")]
pub(super) struct UpdateTaskInput {
    pub(super) task_id: UuidValue,
    pub(super) estimated_work_minutes: OptionalValue<NonNegativeI64>,
    pub(super) deadline_time: NullablePatch<Rfc3339DateTime>,
    pub(super) category: NullablePatch<ProjectCategoryValue>,
}

pub(super) struct UpdateTaskChanges {
    pub(super) task_id: Uuid,
    pub(super) estimated_work_minutes: Option<i64>,
    pub(super) deadline_time: Option<Option<DateTime<Local>>>,
    pub(super) category: Option<Option<ProjectCategory>>,
}

impl UpdateTaskInput {
    pub(super) fn into_changes(self) -> UpdateTaskChanges {
        let estimated_work_minutes = match self.estimated_work_minutes {
            OptionalValue::Missing => None,
            OptionalValue::Value(minutes) => Some(minutes.0),
        };
        let deadline_time = match self.deadline_time {
            NullablePatch::Missing => None,
            NullablePatch::Null => Some(None),
            NullablePatch::Value(deadline_time) => Some(Some(deadline_time.0)),
        };
        let category = match self.category {
            NullablePatch::Missing => None,
            NullablePatch::Null => Some(None),
            NullablePatch::Value(category) => Some(Some(category.into_category())),
        };

        UpdateTaskChanges {
            task_id: self.task_id.0,
            estimated_work_minutes,
            deadline_time,
            category,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = require_update_task_field)]
struct UpdateTaskInputFields {
    task_id: UuidValue,
    #[serde(default)]
    estimated_work_minutes: OptionalValue<NonNegativeI64>,
    #[serde(default)]
    deadline_time: NullablePatch<Rfc3339DateTime>,
    #[serde(default)]
    category: NullablePatch<ProjectCategoryValue>,
}

impl TryFrom<UpdateTaskInputFields> for UpdateTaskInput {
    type Error = String;

    fn try_from(fields: UpdateTaskInputFields) -> Result<Self, Self::Error> {
        if !UPDATE_TASK_FIELDS
            .iter()
            .any(|field| field.is_provided(&fields))
        {
            return Err(format!(
                "{SCHEMA_ERROR_PREFIX}{UPDATE_TASK_FIELD_REQUIRED_REASON}"
            ));
        }

        Ok(Self {
            task_id: fields.task_id,
            estimated_work_minutes: fields.estimated_work_minutes,
            deadline_time: fields.deadline_time,
            category: fields.category,
        })
    }
}

fn require_update_task_field(schema: &mut Schema) {
    let alternatives = UPDATE_TASK_FIELDS
        .iter()
        .map(|field| serde_json::json!({"required": [field.name()]}))
        .collect();
    schema
        .ensure_object()
        .insert("anyOf".to_string(), Value::Array(alternatives));
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ListTasksInput {
    /// A half-open RFC 3339 date-time range [from, until) applied to the selected task field. This filter is combined with status and category filters using AND. Omit this field to skip period filtering.
    #[serde(default)]
    pub(super) period: OptionalValue<TaskPeriodInput>,
    /// Effective task statuses to include. Values within the array are combined using OR. Omit this field or pass an empty array to skip status filtering.
    #[serde(default)]
    pub(super) statuses: OptionalValue<Vec<StatusValue>>,
    /// Categories to include. Values within the array are combined using OR, and null selects uncategorized tasks. Omit this field or pass an empty array to skip category filtering.
    #[serde(default)]
    #[schemars(schema_with = "categories_schema")]
    pub(super) categories: OptionalValue<Vec<Option<ProjectCategoryValue>>>,
}

impl ListTasksInput {
    pub(super) fn into_filter(self) -> ListTasksFilter {
        ListTasksFilter {
            period: match self.period {
                OptionalValue::Missing => None,
                OptionalValue::Value(period) => Some(period.into_filter()),
            },
            statuses: match self.statuses {
                OptionalValue::Missing => Vec::new(),
                OptionalValue::Value(statuses) => {
                    statuses.into_iter().map(StatusValue::into_status).collect()
                }
            },
            categories: match self.categories {
                OptionalValue::Missing => Vec::new(),
                OptionalValue::Value(categories) => categories
                    .into_iter()
                    .map(|category| category.map(ProjectCategoryValue::into_category))
                    .collect(),
            },
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct TaskPeriodInput {
    /// The task date-time field to filter. scheduled_start selects tasks whose scheduled start is within the range; created_at, deadline, and completed_at select their corresponding timestamps.
    pub(super) field: TaskPeriodFieldValue,
    /// The inclusive start of the period as an RFC 3339 date-time string with Z or a numeric UTC offset. Must be earlier than until.
    pub(super) from: Rfc3339DateTime,
    /// The exclusive end of the period as an RFC 3339 date-time string with Z or a numeric UTC offset. Must be later than from.
    pub(super) until: Rfc3339DateTime,
}

impl TaskPeriodInput {
    fn into_filter(self) -> TaskPeriodFilter {
        TaskPeriodFilter {
            field: self.field.into_field(),
            from: self.from.0,
            until: self.until.0,
        }
    }
}

pub(super) enum TaskPeriodFieldValue {
    ScheduledStart,
    CreatedAt,
    Deadline,
    CompletedAt,
}

impl TaskPeriodFieldValue {
    fn into_field(self) -> TaskPeriodField {
        match self {
            Self::ScheduledStart => TaskPeriodField::ScheduledStart,
            Self::CreatedAt => TaskPeriodField::CreatedAt,
            Self::Deadline => TaskPeriodField::Deadline,
            Self::CompletedAt => TaskPeriodField::CompletedAt,
        }
    }
}

impl<'de> Deserialize<'de> for TaskPeriodFieldValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "scheduled_start" => Ok(Self::ScheduledStart),
            "created_at" => Ok(Self::CreatedAt),
            "deadline" => Ok(Self::Deadline),
            "completed_at" => Ok(Self::CompletedAt),
            _ => Err(serde::de::Error::custom(format!(
                "{SCHEMA_ERROR_PREFIX}must be a supported period field"
            ))),
        }
    }
}

impl JsonSchema for TaskPeriodFieldValue {
    fn schema_name() -> Cow<'static, str> {
        "TaskPeriodFieldValue".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "enum": ["scheduled_start", "created_at", "deadline", "completed_at"]
        })
    }
}

pub(super) enum StatusValue {
    Todo,
    Pending,
    Done,
}

impl StatusValue {
    fn into_status(self) -> Status {
        match self {
            Self::Todo => Status::Todo,
            Self::Pending => Status::Pending,
            Self::Done => Status::Done,
        }
    }
}

impl<'de> Deserialize<'de> for StatusValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Value::deserialize(deserializer)? {
            Value::String(value) => match value.as_str() {
                "todo" => Ok(Self::Todo),
                "pending" => Ok(Self::Pending),
                "done" => Ok(Self::Done),
                _ => Err(serde::de::Error::custom(format!(
                    "{SCHEMA_ERROR_PREFIX}must be todo, pending, or done"
                ))),
            },
            _ => Err(serde::de::Error::custom(format!(
                "{SCHEMA_ERROR_PREFIX}must be todo, pending, or done"
            ))),
        }
    }
}

impl JsonSchema for StatusValue {
    fn schema_name() -> Cow<'static, str> {
        "StatusValue".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "string", "enum": ["todo", "pending", "done"]})
    }
}

pub(super) enum ProjectCategoryValue {
    Earning,
    Sustaining,
    Recovery,
    Investment,
    Consumption,
}

impl ProjectCategoryValue {
    fn into_category(self) -> ProjectCategory {
        match self {
            Self::Earning => ProjectCategory::Earning,
            Self::Sustaining => ProjectCategory::Sustaining,
            Self::Recovery => ProjectCategory::Recovery,
            Self::Investment => ProjectCategory::Investment,
            Self::Consumption => ProjectCategory::Consumption,
        }
    }
}

impl JsonSchema for ProjectCategoryValue {
    fn schema_name() -> Cow<'static, str> {
        "ProjectCategoryValue".into()
    }

    fn inline_schema() -> bool {
        true
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "enum": ["earning", "sustaining", "recovery", "investment", "consumption"]
        })
    }
}

impl<'de> Deserialize<'de> for ProjectCategoryValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Value::deserialize(deserializer)? {
            Value::String(value) => match value.as_str() {
                "earning" => Ok(Self::Earning),
                "sustaining" => Ok(Self::Sustaining),
                "recovery" => Ok(Self::Recovery),
                "investment" => Ok(Self::Investment),
                "consumption" => Ok(Self::Consumption),
                _ => Err(serde::de::Error::custom(format!(
                    "{SCHEMA_ERROR_PREFIX}must be a supported category or null"
                ))),
            },
            _ => Err(serde::de::Error::custom(format!(
                "{SCHEMA_ERROR_PREFIX}must be a supported category or null"
            ))),
        }
    }
}

fn categories_schema(generator: &mut SchemaGenerator) -> Schema {
    let category_schema = generator.subschema_for::<ProjectCategoryValue>().to_value();
    json_schema!({
        "type": "array",
        "items": {"anyOf": [category_schema, {"type": "null"}]}
    })
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct GetScheduleInput {
    /// The logical date at the inclusive start of the range, in YYYY-MM-DD format without a time or time zone. Its boundary is 06:00 local time. With no until, selects this one logical day; with no from or until, the range starts now.
    #[serde(default)]
    pub(super) from: OptionalValue<IsoDate>,
    /// The logical date at the exclusive end of the range, in YYYY-MM-DD format without a time or time zone. Its boundary is 06:00 local time. With no from, the range starts now; with neither bound, it ends at the next 06:00 boundary.
    #[serde(default)]
    pub(super) until: OptionalValue<IsoDate>,
}

impl GetScheduleInput {
    pub(super) fn into_period(
        self,
        now: DateTime<Local>,
    ) -> Result<(DateTime<Local>, DateTime<Local>), ToolInputError> {
        let from = match self.from {
            OptionalValue::Missing => None,
            OptionalValue::Value(date) => Some(schedule_day_start(date, "from")?),
        };
        let until = match self.until {
            OptionalValue::Missing => None,
            OptionalValue::Value(date) => Some(schedule_day_start(date, "until")?),
        };

        let (from, until) = match (from, until) {
            (Some(from), Some(until)) => (from, until),
            (Some(from), None) => (
                from,
                try_next_logical_date_start(from).map_err(ToolInputError::Application)?,
            ),
            (None, Some(until)) => (now, until),
            (None, None) => (
                now,
                try_next_logical_date_start(now).map_err(ToolInputError::Application)?,
            ),
        };
        if from >= until {
            return Err(ToolInputError::Semantic {
                field: "until".to_string(),
                message: "must be later than from",
            });
        }

        Ok((from, until))
    }
}

fn schedule_day_start(
    date: IsoDate,
    _field: &'static str,
) -> Result<DateTime<Local>, ToolInputError> {
    try_logical_date_start(date.0).map_err(ToolInputError::Application)
}

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

pub(super) fn decode_input<T: DeserializeOwned + JsonSchema>(
    value: &Value,
) -> Result<T, ToolInputError> {
    preflight_object_contract::<T>(value)?;
    let deserializer = value.clone().into_deserializer();
    serde_path_to_error::deserialize(deserializer).map_err(classify_decode_error)
}

fn preflight_object_contract<T: JsonSchema>(value: &Value) -> Result<(), ToolInputError> {
    let arguments = value.as_object().ok_or_else(|| {
        ToolInputError::Schema(InvalidParams {
            field: "arguments".to_string(),
            reason: "must be an object",
        })
    })?;
    let schema = generated_input_schema::<T>();
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("generated MCP input schema must define object properties");

    if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
        if let Some(field) = arguments
            .keys()
            .find(|field| !properties.contains_key(field.as_str()))
        {
            return Err(ToolInputError::Schema(InvalidParams {
                field: format!("arguments.{field}"),
                reason: "additional property is not allowed",
            }));
        }
    }

    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .expect("generated MCP input schema must define required fields");
    if let Some(field) = required
        .iter()
        .filter_map(Value::as_str)
        .find(|field| !arguments.contains_key(*field))
    {
        return Err(ToolInputError::Schema(InvalidParams {
            field: field.to_string(),
            reason: "field is required",
        }));
    }

    if required_alternatives_are_unmet(&schema, arguments) {
        return Err(ToolInputError::Schema(InvalidParams {
            field: "arguments".to_string(),
            reason: UPDATE_TASK_FIELD_REQUIRED_REASON,
        }));
    }

    Ok(())
}

fn required_alternatives_are_unmet(schema: &Value, arguments: &Map<String, Value>) -> bool {
    let Some(alternatives) = schema.get("anyOf").and_then(Value::as_array) else {
        return false;
    };
    let mut has_required_alternative = false;

    for alternative in alternatives {
        let Some(required) = alternative.get("required").and_then(Value::as_array) else {
            return false;
        };
        has_required_alternative = true;
        if required.iter().all(|field| {
            field
                .as_str()
                .is_some_and(|field| arguments.contains_key(field))
        }) {
            return false;
        }
    }

    has_required_alternative
}

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
        let field =
            if reason == UPDATE_TASK_FIELD_REQUIRED_REASON && (path.is_empty() || path == ".") {
                "arguments".to_string()
            } else {
                path
            };
        return ToolInputError::Schema(InvalidParams {
            field,
            reason: schema_reason(reason),
        });
    }
    structural_decode_error(path, &message)
}

fn error_reason<'a>(message: &'a str, prefix: &str) -> Option<&'a str> {
    let message = message.strip_prefix(prefix)?;
    Some(message.split(" at line ").next().unwrap_or_default())
}

fn semantic_reason(reason: &str) -> &'static str {
    match reason {
        "must be a valid UUID" => "must be a valid UUID",
        "must be a valid RFC 3339 date-time" => "must be a valid RFC 3339 date-time",
        "must be a valid calendar date in YYYY-MM-DD format" => {
            "must be a valid calendar date in YYYY-MM-DD format"
        }
        "is outside the supported integer range" => "is outside the supported integer range",
        _ => "contains an invalid value",
    }
}

fn schema_reason(reason: &str) -> &'static str {
    match reason {
        "must be a non-negative integer" => "must be a non-negative integer",
        "must not be empty" => "must not be empty",
        "must contain at least one item" => "must contain at least one item",
        "must be a string or null" => "must be a string or null",
        "must be a YYYY-MM-DD date string" => "must be a YYYY-MM-DD date string",
        "must be a supported period field" => "must be a supported period field",
        "must be todo, pending, or done" => "must be todo, pending, or done",
        "must be a supported category or null" => "must be a supported category or null",
        UPDATE_TASK_FIELD_REQUIRED_REASON => UPDATE_TASK_FIELD_REQUIRED_REASON,
        _ => "has an invalid value",
    }
}

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
    let reason = if message.contains("expected struct") || message.contains("expected a map") {
        "must be an object"
    } else if message.contains("expected a sequence") {
        "must be an array"
    } else if message.contains("expected a string") {
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

fn quoted_serde_field(message: &str, prefix: &str) -> Option<String> {
    let start = message.find(prefix)? + prefix.len();
    Some(message[start..].split('`').next()?.to_string())
}

fn parse_local_datetime(value: &str) -> Result<DateTime<Local>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(|time| time.with_timezone(&Local))
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
#[path = "input_tests.rs"]
mod tests;
