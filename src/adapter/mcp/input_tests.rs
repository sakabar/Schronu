use super::{
    common_input_contract, decode_input, generated_input_schema, BreakdownTaskInput,
    CompleteTaskInput, CreateTaskInput, DeferRoutineTaskInput, DeferTaskInput, GetFocusInput,
    GetScheduleInput, GetTaskInput, ListTasksInput, NonNegativeI64, NullablePatch, OptionalValue,
    ProjectCategoryValue, Rfc3339DateTime, ToolInputError, UpdateTaskInput,
};
use crate::application::task_use_case::ApplicationError;
use chrono::{DateTime, FixedOffset, Local, NaiveDate};
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
fn common_input_schema_describes_public_value_formats() {
    let contract = common_input_contract();
    let properties = contract.schema()["properties"]
        .as_object()
        .expect("common input schema must define properties");

    assert_eq!(
        properties["task_id"],
        json!({
            "type": "string",
            "format": "uuid",
            "description": "A valid UUID string.",
            "examples": ["80d7db87-324e-4e8d-a5b7-ff78cd5bf39a"]
        })
    );
    assert_eq!(
        properties["pending_until"],
        json!({
            "type": "string",
            "format": "date-time",
            "description": "An RFC 3339 date-time string with Z or a numeric UTC offset.",
            "examples": ["2026-08-29T10:00:00+09:00", "2026-08-29T01:00:00Z"]
        })
    );
    assert_eq!(
        properties["date"],
        json!({
            "type": "string",
            "format": "date",
            "description": "A calendar date in YYYY-MM-DD format without a time or time zone.",
            "examples": ["2026-08-29"]
        })
    );
    assert_eq!(
        properties["work_seconds"],
        json!({
            "type": "integer",
            "minimum": 0,
            "description": "A non-negative integer."
        })
    );
    assert_eq!(
        properties["name"],
        json!({
            "type": "string",
            "minLength": 1,
            "description": "A non-empty string."
        })
    );
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
                "task_id": {
                    "type": "string",
                    "format": "uuid",
                    "description": "The UUID of the existing task to return.",
                    "examples": ["80d7db87-324e-4e8d-a5b7-ff78cd5bf39a"]
                }
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
fn defer_routine_task_input_schemaとdecode契約が一致する() {
    let task_id = "80d7db87-324e-4e8d-a5b7-ff78cd5bf39a";
    assert_reference_input_contract::<DeferRoutineTaskInput>(
        "defer_routine_task",
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "format": "uuid",
                    "description": "A valid UUID string.",
                    "examples": ["80d7db87-324e-4e8d-a5b7-ff78cd5bf39a"]
                }
            },
            "required": ["task_id"],
            "additionalProperties": false
        }),
        vec![
            valid_case("required task id", json!({"task_id": task_id})),
            schema_case("missing task id", json!({}), "task_id", "field is required"),
            schema_case(
                "task id has wrong type",
                json!({"task_id": 42}),
                "task_id",
                "must be a string",
            ),
            semantic_case(
                "task id is invalid",
                json!({"task_id": "not-a-uuid"}),
                "task_id",
                "must be a valid UUID",
            ),
            schema_case(
                "unknown field",
                json!({"task_id": task_id, "extra": true}),
                "arguments.extra",
                "additional property is not allowed",
            ),
            schema_case(
                "arguments has wrong type",
                json!(42),
                "arguments",
                "must be an object",
            ),
        ],
    );

    let input = decode_input::<DeferRoutineTaskInput>(&json!({"task_id": task_id}))
        .unwrap_or_else(|_| panic!("defer routine task payload must decode"));
    assert_eq!(input.task_id.0.to_string(), task_id);
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
                reason: "must be a YYYY-MM-DD date string",
            },
        },
        ContractCase {
            name: "until has wrong type",
            input: json!({"until": 42}),
            schema_accepts: false,
            decode: ExpectedDecode::Schema {
                field: "until",
                reason: "must be a YYYY-MM-DD date string",
            },
        },
        ContractCase {
            name: "date is invalid",
            input: json!({"from": "2026-02-30"}),
            schema_accepts: false,
            decode: ExpectedDecode::Semantic {
                field: "from",
                reason: "must be a valid calendar date in YYYY-MM-DD format",
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
            name: "RFC 3339 date-time with Z",
            input: with_field(
                valid_input(),
                "pending_until",
                json!("2026-08-29T01:00:00Z"),
            ),
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
                reason: "must be a valid calendar date in YYYY-MM-DD format",
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
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NestedArguments {
    period: NestedPeriod,
}

#[allow(dead_code)]
#[derive(Deserialize, JsonSchema)]
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
        Err(ToolInputError::Application(error)) => format!("application({error})"),
    }
}

fn maximum_logical_date_start() -> DateTime<Local> {
    DateTime::<Local>::from_naive_utc_and_offset(
        NaiveDate::MAX.and_hms_opt(6, 0, 0).unwrap(),
        FixedOffset::east_opt(0).unwrap(),
    )
}

#[test]
fn schedule_periodは引数なしの次論理日境界errorを保持する() {
    let now = maximum_logical_date_start();
    let result = GetScheduleInput {
        from: OptionalValue::Missing,
        until: OptionalValue::Missing,
    }
    .into_period(now);

    match result {
        Err(ToolInputError::Application(error)) => assert_eq!(
            error,
            ApplicationError::LogicalDateOutOfRange {
                operation: "next_logical_date_start",
                datetime: now,
            }
        ),
        _ => panic!("expected an application datetime error"),
    }
}

#[test]
fn schedule_periodはfromのみの次論理日境界errorを保持する() {
    let from = match super::schedule_day_start(super::IsoDate(NaiveDate::MAX), "from") {
        Ok(from) => from,
        Err(_) => panic!("expected the maximum date to resolve to a local datetime"),
    };
    let result = GetScheduleInput {
        from: OptionalValue::Value(super::IsoDate(NaiveDate::MAX)),
        until: OptionalValue::Missing,
    }
    .into_period(maximum_logical_date_start());

    match result {
        Err(ToolInputError::Application(error)) => assert_eq!(
            error,
            ApplicationError::LogicalDateOutOfRange {
                operation: "next_logical_date_start",
                datetime: from,
            }
        ),
        _ => panic!("expected an application datetime error"),
    }
}
