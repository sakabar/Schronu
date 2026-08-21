use super::error::InvalidParams;
use crate::application::task_use_case::{
    BreakdownTaskInput as ApplicationBreakdownTaskInput,
    CompleteTaskInput as ApplicationCompleteTaskInput,
    CreateTaskInput as ApplicationCreateTaskInput, ListTasksFilter, TaskPeriodField,
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
        let value = Value::deserialize(deserializer)?;
        let Value::String(value) = value else {
            return Err(serde::de::Error::custom(format!(
                "{SCHEMA_ERROR_PREFIX}must be a date string"
            )));
        };
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

#[allow(dead_code, reason = "used by the staged typed-tool migration")]
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
#[allow(dead_code, reason = "used by the staged typed-handler migration")]
pub(super) struct GetTaskInput {
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
pub(super) struct CompleteTaskInput {
    pub(super) task_id: UuidValue,
    #[serde(default)]
    pub(super) finished_at: OptionalValue<Rfc3339DateTime>,
    #[serde(default = "zero_non_negative")]
    #[schemars(schema_with = "additional_work_seconds_schema")]
    pub(super) additional_actual_work_seconds: NonNegativeI64,
}

impl CompleteTaskInput {
    pub(super) fn into_application(self) -> ApplicationCompleteTaskInput {
        ApplicationCompleteTaskInput {
            task_id: self.task_id.0,
            finished_at: match self.finished_at {
                OptionalValue::Missing => Local::now(),
                OptionalValue::Value(finished_at) => finished_at.0,
            },
            additional_actual_work_seconds: self.additional_actual_work_seconds.0,
        }
    }
}

fn zero_non_negative() -> NonNegativeI64 {
    NonNegativeI64(0)
}

fn additional_work_seconds_schema(_generator: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "integer", "minimum": 0, "default": 0})
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
    #[serde(default)]
    pub(super) period: OptionalValue<TaskPeriodInput>,
    #[serde(default)]
    pub(super) statuses: OptionalValue<Vec<StatusValue>>,
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
    pub(super) field: TaskPeriodFieldValue,
    pub(super) from: Rfc3339DateTime,
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
    #[serde(default)]
    pub(super) from: OptionalValue<IsoDate>,
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
}

fn schedule_day_start(
    date: IsoDate,
    field: &'static str,
) -> Result<DateTime<Local>, ToolInputError> {
    let local_noon = date
        .0
        .and_hms_opt(12, 0, 0)
        .ok_or(ToolInputError::Semantic {
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
        "must contain at least one item" => "must contain at least one item",
        "must be a string or null" => "must be a string or null",
        "must be a date string" => "must be a date string",
        "must be a supported period field" => "must be a supported period field",
        "must be todo, pending, or done" => "must be todo, pending, or done",
        "must be a supported category or null" => "must be a supported category or null",
        UPDATE_TASK_FIELD_REQUIRED_REASON => UPDATE_TASK_FIELD_REQUIRED_REASON,
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
mod tests {
    use super::{
        common_input_contract, decode_input, generated_input_schema, BreakdownTaskInput,
        CompleteTaskInput, CreateTaskInput, DeferTaskInput, GetFocusInput, GetScheduleInput,
        GetTaskInput, ListTasksInput, NonNegativeI64, NullablePatch, OptionalValue,
        ProjectCategoryValue, Rfc3339DateTime, ToolInputError, UpdateTaskInput,
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

    #[test]
    fn creation_tool_inputs_match_public_schema_and_decode_contract() {
        assert_reference_input_contract::<CreateTaskInput>(
            "create_task",
            public_tool_schema("create_task"),
            create_task_input_cases(),
        );
        assert_reference_input_contract::<BreakdownTaskInput>(
            "breakdown_task",
            public_tool_schema("breakdown_task"),
            breakdown_task_input_cases(),
        );
    }

    #[test]
    fn state_change_tool_inputs_match_public_schema_and_decode_contract() {
        assert_reference_input_contract::<DeferTaskInput>(
            "defer_task",
            public_tool_schema("defer_task"),
            defer_task_input_cases(),
        );
        assert_reference_input_contract::<CompleteTaskInput>(
            "complete_task",
            public_tool_schema("complete_task"),
            complete_task_input_cases(),
        );
        assert_reference_input_contract::<UpdateTaskInput>(
            "update_task",
            public_tool_schema("update_task"),
            update_task_input_cases(),
        );
    }

    #[test]
    fn state_change_input_payload_and_patch_values_are_preserved() {
        let task_id = "80d7db87-324e-4e8d-a5b7-ff78cd5bf39a";
        let expected_pending_until = DateTime::parse_from_rfc3339("2026-08-20T10:00:00+09:00")
            .unwrap()
            .with_timezone(&Local);
        let expected_finished_at = DateTime::parse_from_rfc3339("2026-08-19T10:00:00+09:00")
            .unwrap()
            .with_timezone(&Local);
        let expected_deadline = DateTime::parse_from_rfc3339("2026-08-19T10:00:00+09:00")
            .unwrap()
            .with_timezone(&Local);
        let defer = decode_input::<DeferTaskInput>(&json!({
            "task_id": task_id,
            "pending_until": "2026-08-20T10:00:00+09:00"
        }))
        .unwrap_or_else(|_| panic!("defer payload must decode"));
        assert_eq!(defer.task_id.0.to_string(), task_id);
        assert_eq!(defer.pending_until.0, expected_pending_until);

        let complete = decode_input::<CompleteTaskInput>(&json!({"task_id": task_id}))
            .unwrap_or_else(|_| panic!("required complete input must decode"));
        assert_eq!(complete.task_id.0.to_string(), task_id);
        assert!(matches!(complete.finished_at, OptionalValue::Missing));
        assert_eq!(complete.additional_actual_work_seconds.0, 0);

        let complete = decode_input::<CompleteTaskInput>(&json!({
            "task_id": task_id,
            "finished_at": "2026-08-19T10:00:00+09:00",
            "additional_actual_work_seconds": 15
        }))
        .unwrap_or_else(|_| panic!("complete optional values must decode"));
        assert!(matches!(
            complete.finished_at,
            OptionalValue::Value(value) if value.0 == expected_finished_at
        ));
        assert_eq!(complete.additional_actual_work_seconds.0, 15);

        let update = decode_input::<UpdateTaskInput>(&json!({
            "task_id": task_id,
            "category": null
        }))
        .unwrap_or_else(|_| panic!("nullable update patch must decode"));
        assert_eq!(update.task_id.0.to_string(), task_id);
        assert!(matches!(
            update.estimated_work_minutes,
            OptionalValue::Missing
        ));
        assert!(matches!(update.deadline_time, NullablePatch::Missing));
        assert!(matches!(update.category, NullablePatch::Null));

        let update = decode_input::<UpdateTaskInput>(&json!({
            "task_id": task_id,
            "deadline_time": null
        }))
        .unwrap_or_else(|_| panic!("nullable deadline patch must decode"));
        assert!(matches!(update.deadline_time, NullablePatch::Null));
        assert!(matches!(update.category, NullablePatch::Missing));

        let update = decode_input::<UpdateTaskInput>(&json!({
            "task_id": task_id,
            "estimated_work_minutes": 30,
            "deadline_time": "2026-08-19T10:00:00+09:00",
            "category": "earning"
        }))
        .unwrap_or_else(|_| panic!("update patch values must decode"));
        assert!(matches!(
            update.estimated_work_minutes,
            OptionalValue::Value(value) if value.0 == 30
        ));
        assert!(matches!(
            update.deadline_time,
            NullablePatch::Value(value) if value.0 == expected_deadline
        ));
        assert!(matches!(
            update.category,
            NullablePatch::Value(ProjectCategoryValue::Earning)
        ));
    }

    fn defer_task_input_cases() -> Vec<ContractCase> {
        let task_id = "80d7db87-324e-4e8d-a5b7-ff78cd5bf39a";
        let pending_until = "2026-08-19T10:00:00+09:00";
        vec![
            valid_case(
                "required defer fields",
                json!({"task_id": task_id, "pending_until": pending_until}),
            ),
            schema_case(
                "missing defer task id",
                json!({"pending_until": pending_until}),
                "task_id",
                "field is required",
            ),
            schema_case(
                "missing pending until",
                json!({"task_id": task_id}),
                "pending_until",
                "field is required",
            ),
            schema_case(
                "defer task id has wrong type",
                json!({"task_id": 42, "pending_until": pending_until}),
                "task_id",
                "must be a string",
            ),
            semantic_case(
                "defer task id is invalid",
                json!({"task_id": "not-a-uuid", "pending_until": pending_until}),
                "task_id",
                "must be a valid UUID",
            ),
            schema_case(
                "pending until has wrong type",
                json!({"task_id": task_id, "pending_until": 42}),
                "pending_until",
                "must be a string",
            ),
            semantic_case(
                "pending until is invalid",
                json!({"task_id": task_id, "pending_until": "not-a-date"}),
                "pending_until",
                "must be a valid RFC 3339 date-time",
            ),
            schema_case(
                "defer task has unknown field",
                json!({"task_id": task_id, "pending_until": pending_until, "extra": true}),
                "arguments.extra",
                "additional property is not allowed",
            ),
            schema_case(
                "defer arguments has wrong type",
                json!(42),
                "arguments",
                "must be an object",
            ),
        ]
    }

    fn complete_task_input_cases() -> Vec<ContractCase> {
        let task_id = "80d7db87-324e-4e8d-a5b7-ff78cd5bf39a";
        vec![
            valid_case("required complete field", json!({"task_id": task_id})),
            valid_case(
                "all complete fields",
                json!({
                    "task_id": task_id,
                    "finished_at": "2026-08-19T10:00:00+09:00",
                    "additional_actual_work_seconds": 0
                }),
            ),
            schema_case(
                "missing complete task id",
                json!({}),
                "task_id",
                "field is required",
            ),
            semantic_case(
                "complete task id is invalid",
                json!({"task_id": "not-a-uuid"}),
                "task_id",
                "must be a valid UUID",
            ),
            schema_case(
                "complete task id has wrong type",
                json!({"task_id": 42}),
                "task_id",
                "must be a string",
            ),
            schema_case(
                "finished at cannot be null",
                json!({"task_id": task_id, "finished_at": null}),
                "finished_at",
                "must be a string",
            ),
            schema_case(
                "finished at has wrong type",
                json!({"task_id": task_id, "finished_at": 42}),
                "finished_at",
                "must be a string",
            ),
            semantic_case(
                "finished at is invalid",
                json!({"task_id": task_id, "finished_at": "not-a-date"}),
                "finished_at",
                "must be a valid RFC 3339 date-time",
            ),
            schema_case(
                "additional work cannot be null",
                json!({"task_id": task_id, "additional_actual_work_seconds": null}),
                "additional_actual_work_seconds",
                "must be a non-negative integer",
            ),
            schema_case(
                "additional work cannot be negative",
                json!({"task_id": task_id, "additional_actual_work_seconds": -1}),
                "additional_actual_work_seconds",
                "must be a non-negative integer",
            ),
            schema_case(
                "additional work cannot be fractional",
                json!({"task_id": task_id, "additional_actual_work_seconds": 1.5}),
                "additional_actual_work_seconds",
                "must be a non-negative integer",
            ),
            schema_case(
                "additional work has wrong type",
                json!({"task_id": task_id, "additional_actual_work_seconds": "1"}),
                "additional_actual_work_seconds",
                "must be a non-negative integer",
            ),
            valid_case(
                "additional work accepts i64 maximum",
                json!({"task_id": task_id, "additional_actual_work_seconds": i64::MAX}),
            ),
            semantic_case_with_schema_acceptance(
                "additional work outside i64 range",
                json!({"task_id": task_id, "additional_actual_work_seconds": u64::MAX}),
                true,
                "additional_actual_work_seconds",
                "is outside the supported integer range",
            ),
            schema_case(
                "complete task has unknown field",
                json!({"task_id": task_id, "extra": true}),
                "arguments.extra",
                "additional property is not allowed",
            ),
            schema_case(
                "complete arguments has wrong type",
                json!(42),
                "arguments",
                "must be an object",
            ),
        ]
    }

    fn update_task_input_cases() -> Vec<ContractCase> {
        let task_id = "80d7db87-324e-4e8d-a5b7-ff78cd5bf39a";
        vec![
            valid_case(
                "update estimate",
                json!({"task_id": task_id, "estimated_work_minutes": 0}),
            ),
            valid_case(
                "set update deadline",
                json!({
                    "task_id": task_id,
                    "deadline_time": "2026-08-19T10:00:00+09:00"
                }),
            ),
            valid_case(
                "clear update deadline",
                json!({"task_id": task_id, "deadline_time": null}),
            ),
            valid_case(
                "set every update category",
                json!({
                    "task_id": task_id,
                    "category": "earning",
                    "deadline_time": null,
                    "estimated_work_minutes": 30
                }),
            ),
            schema_case(
                "no update field",
                json!({"task_id": task_id}),
                "arguments",
                "must include at least one field to update",
            ),
            schema_case(
                "missing update task id",
                json!({"category": null}),
                "task_id",
                "field is required",
            ),
            semantic_case(
                "update task id is invalid",
                json!({"task_id": "not-a-uuid", "category": null}),
                "task_id",
                "must be a valid UUID",
            ),
            schema_case(
                "update task id has wrong type",
                json!({"task_id": 42, "category": null}),
                "task_id",
                "must be a string",
            ),
            schema_case(
                "estimate cannot be null",
                json!({"task_id": task_id, "estimated_work_minutes": null}),
                "estimated_work_minutes",
                "must be a non-negative integer",
            ),
            schema_case(
                "estimate cannot be negative",
                json!({"task_id": task_id, "estimated_work_minutes": -1}),
                "estimated_work_minutes",
                "must be a non-negative integer",
            ),
            schema_case(
                "estimate cannot be fractional",
                json!({"task_id": task_id, "estimated_work_minutes": 1.5}),
                "estimated_work_minutes",
                "must be a non-negative integer",
            ),
            schema_case(
                "estimate has wrong type",
                json!({"task_id": task_id, "estimated_work_minutes": "1"}),
                "estimated_work_minutes",
                "must be a non-negative integer",
            ),
            valid_case(
                "estimate accepts i64 maximum",
                json!({"task_id": task_id, "estimated_work_minutes": i64::MAX}),
            ),
            semantic_case_with_schema_acceptance(
                "estimate outside i64 range",
                json!({"task_id": task_id, "estimated_work_minutes": u64::MAX}),
                true,
                "estimated_work_minutes",
                "is outside the supported integer range",
            ),
            schema_case(
                "deadline has wrong type",
                json!({"task_id": task_id, "deadline_time": 42}),
                "deadline_time",
                "must be a string or null",
            ),
            semantic_case(
                "deadline is invalid",
                json!({"task_id": task_id, "deadline_time": "not-a-date"}),
                "deadline_time",
                "must be a valid RFC 3339 date-time",
            ),
            valid_case(
                "clear update category",
                json!({"task_id": task_id, "category": null}),
            ),
            valid_case(
                "all category values",
                json!({"task_id": task_id, "category": "consumption"}),
            ),
            valid_case(
                "sustaining category",
                json!({"task_id": task_id, "category": "sustaining"}),
            ),
            valid_case(
                "recovery category",
                json!({"task_id": task_id, "category": "recovery"}),
            ),
            valid_case(
                "investment category",
                json!({"task_id": task_id, "category": "investment"}),
            ),
            schema_case(
                "category has wrong type",
                json!({"task_id": task_id, "category": 42}),
                "category",
                "must be a supported category or null",
            ),
            schema_case(
                "category is unsupported",
                json!({"task_id": task_id, "category": "unknown"}),
                "category",
                "must be a supported category or null",
            ),
            schema_case(
                "update task has unknown field",
                json!({"task_id": task_id, "category": null, "extra": true}),
                "arguments.extra",
                "additional property is not allowed",
            ),
            schema_case(
                "update arguments has wrong type",
                json!(42),
                "arguments",
                "must be an object",
            ),
        ]
    }

    fn valid_case(name: &'static str, input: Value) -> ContractCase {
        ContractCase {
            name,
            input,
            schema_accepts: true,
            decode: ExpectedDecode::Valid,
        }
    }

    fn schema_case(
        name: &'static str,
        input: Value,
        field: &'static str,
        reason: &'static str,
    ) -> ContractCase {
        ContractCase {
            name,
            input,
            schema_accepts: false,
            decode: ExpectedDecode::Schema { field, reason },
        }
    }

    fn semantic_case(
        name: &'static str,
        input: Value,
        field: &'static str,
        reason: &'static str,
    ) -> ContractCase {
        semantic_case_with_schema_acceptance(name, input, false, field, reason)
    }

    fn semantic_case_with_schema_acceptance(
        name: &'static str,
        input: Value,
        schema_accepts: bool,
        field: &'static str,
        reason: &'static str,
    ) -> ContractCase {
        ContractCase {
            name,
            input,
            schema_accepts,
            decode: ExpectedDecode::Semantic { field, reason },
        }
    }

    fn create_task_input_cases() -> Vec<ContractCase> {
        vec![
            ContractCase {
                name: "required name only",
                input: json!({"name": "write contract test"}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "all create fields",
                input: json!({
                    "name": "write contract test",
                    "estimated_work_minutes": 0,
                    "pending_until": "2026-08-19T10:00:00+09:00"
                }),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "missing name",
                input: json!({}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "name",
                    reason: "field is required",
                },
            },
            ContractCase {
                name: "name has wrong type",
                input: json!({"name": 42}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "name",
                    reason: "must be a string",
                },
            },
            ContractCase {
                name: "name is empty",
                input: json!({"name": ""}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "name",
                    reason: "must not be empty",
                },
            },
            ContractCase {
                name: "whitespace-only name is decoded for application validation",
                input: json!({"name": "   "}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "estimated work can be omitted",
                input: json!({"name": "write contract test"}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "estimated work cannot be null",
                input: json!({"name": "write contract test", "estimated_work_minutes": null}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "estimated_work_minutes",
                    reason: "must be a non-negative integer",
                },
            },
            ContractCase {
                name: "estimated work can be zero",
                input: json!({"name": "write contract test", "estimated_work_minutes": 0}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "estimated work cannot be negative",
                input: json!({"name": "write contract test", "estimated_work_minutes": -1}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "estimated_work_minutes",
                    reason: "must be a non-negative integer",
                },
            },
            ContractCase {
                name: "estimated work cannot be fractional",
                input: json!({"name": "write contract test", "estimated_work_minutes": 1.5}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "estimated_work_minutes",
                    reason: "must be a non-negative integer",
                },
            },
            ContractCase {
                name: "estimated work has wrong type",
                input: json!({"name": "write contract test", "estimated_work_minutes": "1"}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "estimated_work_minutes",
                    reason: "must be a non-negative integer",
                },
            },
            ContractCase {
                name: "estimated work accepts i64 maximum",
                input: json!({
                    "name": "write contract test",
                    "estimated_work_minutes": i64::MAX
                }),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "estimated work outside i64 range",
                input: json!({
                    "name": "write contract test",
                    "estimated_work_minutes": u64::MAX
                }),
                schema_accepts: true,
                decode: ExpectedDecode::Semantic {
                    field: "estimated_work_minutes",
                    reason: "is outside the supported integer range",
                },
            },
            ContractCase {
                name: "pending until can be omitted",
                input: json!({"name": "write contract test"}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "pending until cannot be null",
                input: json!({"name": "write contract test", "pending_until": null}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "pending_until",
                    reason: "must be a string",
                },
            },
            ContractCase {
                name: "pending until is invalid",
                input: json!({"name": "write contract test", "pending_until": "not-a-date"}),
                schema_accepts: false,
                decode: ExpectedDecode::Semantic {
                    field: "pending_until",
                    reason: "must be a valid RFC 3339 date-time",
                },
            },
            ContractCase {
                name: "create task has unknown field",
                input: json!({"name": "write contract test", "extra": true}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "arguments.extra",
                    reason: "additional property is not allowed",
                },
            },
            ContractCase {
                name: "create task arguments has wrong type",
                input: json!(42),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "arguments",
                    reason: "must be an object",
                },
            },
        ]
    }

    fn breakdown_task_input_cases() -> Vec<ContractCase> {
        let parent_id = "80d7db87-324e-4e8d-a5b7-ff78cd5bf39a";
        vec![
            ContractCase {
                name: "required breakdown fields",
                input: json!({"parent_id": parent_id, "names": ["first child"]}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "all breakdown fields and names",
                input: json!({
                    "parent_id": parent_id,
                    "names": ["first child", "second child"],
                    "pending_until": "2026-08-19T10:00:00+09:00"
                }),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "missing parent id",
                input: json!({"names": ["first child"]}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "parent_id",
                    reason: "field is required",
                },
            },
            ContractCase {
                name: "parent id has wrong type",
                input: json!({"parent_id": 42, "names": ["first child"]}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "parent_id",
                    reason: "must be a string",
                },
            },
            ContractCase {
                name: "parent id is invalid",
                input: json!({"parent_id": "not-a-uuid", "names": ["first child"]}),
                schema_accepts: false,
                decode: ExpectedDecode::Semantic {
                    field: "parent_id",
                    reason: "must be a valid UUID",
                },
            },
            ContractCase {
                name: "missing names",
                input: json!({"parent_id": parent_id}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "names",
                    reason: "field is required",
                },
            },
            ContractCase {
                name: "names has wrong type",
                input: json!({"parent_id": parent_id, "names": "first child"}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "names",
                    reason: "must be an array",
                },
            },
            ContractCase {
                name: "names is empty",
                input: json!({"parent_id": parent_id, "names": []}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "names",
                    reason: "must contain at least one item",
                },
            },
            ContractCase {
                name: "name element has wrong type",
                input: json!({"parent_id": parent_id, "names": [42]}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "names[0]",
                    reason: "must be a string",
                },
            },
            ContractCase {
                name: "name element is empty",
                input: json!({"parent_id": parent_id, "names": [""]}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "names[0]",
                    reason: "must not be empty",
                },
            },
            ContractCase {
                name: "whitespace-only element is decoded for application validation",
                input: json!({"parent_id": parent_id, "names": ["   "]}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "breakdown pending until can be omitted",
                input: json!({"parent_id": parent_id, "names": ["first child"]}),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
            },
            ContractCase {
                name: "breakdown pending until cannot be null",
                input: json!({
                    "parent_id": parent_id,
                    "names": ["first child"],
                    "pending_until": null
                }),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "pending_until",
                    reason: "must be a string",
                },
            },
            ContractCase {
                name: "breakdown pending until is invalid",
                input: json!({
                    "parent_id": parent_id,
                    "names": ["first child"],
                    "pending_until": "not-a-date"
                }),
                schema_accepts: false,
                decode: ExpectedDecode::Semantic {
                    field: "pending_until",
                    reason: "must be a valid RFC 3339 date-time",
                },
            },
            ContractCase {
                name: "breakdown task has unknown field",
                input: json!({"parent_id": parent_id, "names": ["first child"], "extra": true}),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "arguments.extra",
                    reason: "additional property is not allowed",
                },
            },
            ContractCase {
                name: "breakdown task arguments has wrong type",
                input: json!(42),
                schema_accepts: false,
                decode: ExpectedDecode::Schema {
                    field: "arguments",
                    reason: "must be an object",
                },
            },
        ]
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
