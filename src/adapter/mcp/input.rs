use super::error::InvalidParams;
use crate::application::task_use_case::{
    BreakdownTaskInput, CompleteTaskInput, CreateTaskInput, ListTasksFilter, TaskPeriodField,
    TaskPeriodFilter,
};
use crate::entity::datetime::get_next_morning_datetime;
use crate::entity::task::{ProjectCategory, Status};
use chrono::{DateTime, Duration, Local, LocalResult, NaiveDate};
use serde_json::{Map, Value};
use uuid::Uuid;

pub(super) enum ToolInputError {
    Schema(InvalidParams),
    Semantic {
        field: &'static str,
        message: &'static str,
    },
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
        field,
        message: "must be a valid UUID",
    })
}

fn datetime_argument(
    arguments: &Map<String, Value>,
    field: &'static str,
) -> Result<DateTime<Local>, ToolInputError> {
    let value = string_argument(arguments, field).map_err(ToolInputError::Schema)?;
    parse_local_datetime(value).map_err(|_| ToolInputError::Semantic {
        field,
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
                field,
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
                    field,
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
                    field,
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
            field: "until",
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
            field,
            message: "must be a valid ISO 8601 date",
        })?;
    let local_noon = date.and_hms_opt(12, 0, 0).ok_or(ToolInputError::Semantic {
        field,
        message: "must be a valid ISO 8601 date",
    })?;
    let local_noon = match local_noon.and_local_timezone(Local) {
        LocalResult::Single(datetime) => datetime,
        _ => {
            return Err(ToolInputError::Semantic {
                field,
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
        field,
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
mod tests {
    use super::{common_input_contract, ToolInputError};
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

    fn common_input_cases() -> Vec<ContractCase> {
        vec![
            ContractCase {
                name: "valid values",
                input: valid_input(),
                schema_accepts: true,
                decode: ExpectedDecode::Valid,
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
