use super::task_use_case::{
    breakdown_task, create_task, validate_task_name, ApplicationError, BreakdownTaskInput,
    CreateTaskInput, TaskFactory,
};
use crate::test_support::{new_task_handle_at, TestTaskRepository};
use chrono::{Local, TimeZone};
use uuid::Uuid;

const BLANK_REASON: &str = "must not be blank";
const INTEGER_ONLY_REASON: &str = "must not be an integer-only name";
const CONTROL_CHARACTER_REASON: &str = "must not contain control characters";

fn fixed_now() -> chrono::DateTime<Local> {
    Local.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap()
}

fn with_factory_id<T>(id: Uuid, operation: impl FnOnce(&mut TaskFactory<'_>) -> T) -> T {
    let mut next_id = move || id;
    let mut factory = TaskFactory::new(fixed_now(), &mut next_id);
    operation(&mut factory)
}

#[test]
fn canonical_validationは許可名を原文のまま受理する() {
    for name in [
        " 前後空白 ",
        "連続  空白",
        "日本語",
        "single'quote",
        "double\"quote",
        r"back\slash",
    ] {
        assert_eq!(validate_task_name(name, "name"), Ok(()), "name={name:?}");
    }
}

#[test]
fn create_taskは許可名を原文のまま保存する() {
    for (index, name) in [
        " 前後空白 ",
        "連続  空白",
        "日本語",
        "single'quote",
        "double\"quote",
        r"back\slash",
    ]
    .into_iter()
    .enumerate()
    {
        let mut repository = TestTaskRepository::new(vec![], fixed_now());
        let expected_id = Uuid::from_u128(index as u128 + 1);

        let actual = with_factory_id(expected_id, |factory| {
            create_task(
                &mut repository,
                CreateTaskInput {
                    name: name.to_string(),
                    estimated_work_minutes: None,
                    pending_until: None,
                },
                factory,
            )
        });

        assert_eq!(actual, Ok(expected_id));
        assert_eq!(repository.projects()[0].get_name().unwrap(), name);
    }
}

#[test]
fn breakdown_taskは許可名を原文のまま保存する() {
    let names = vec![
        " 前後空白 ".to_string(),
        "連続  空白".to_string(),
        "日本語".to_string(),
        "single'quote".to_string(),
        "double\"quote".to_string(),
        r"back\slash".to_string(),
    ];
    let parent = new_task_handle_at("親", fixed_now()).unwrap();
    let parent_id = parent.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());
    let mut sequence = 100_u128..;
    let mut next_id = move || Uuid::from_u128(sequence.next().unwrap());
    let mut factory = TaskFactory::new(fixed_now(), &mut next_id);

    breakdown_task(
        &mut repository,
        BreakdownTaskInput {
            parent_id,
            names: names.clone(),
            pending_until: None,
        },
        &mut factory,
    )
    .unwrap();

    assert_eq!(
        parent
            .get_children()
            .unwrap()
            .into_iter()
            .map(|child| child.get_name().unwrap())
            .collect::<Vec<_>>(),
        names
    );
}

#[test]
fn canonical_validationはblankと符号付き整数を理由付きで拒否する() {
    for (name, expected_reason) in [
        ("", BLANK_REASON),
        ("   ", BLANK_REASON),
        ("123", INTEGER_ONLY_REASON),
        (" +123 ", INTEGER_ONLY_REASON),
        (" -123 ", INTEGER_ONLY_REASON),
    ] {
        assert_eq!(
            validate_task_name(name, "names"),
            Err(ApplicationError::InvalidInput {
                field: "names",
                reason: expected_reason,
            }),
            "name={name:?}"
        );
    }
}

#[test]
fn canonical_validationは全unicode_controlを理由付きで拒否する() {
    let controls = (0..=char::MAX as u32)
        .filter_map(char::from_u32)
        .filter(|character| character.is_control());

    for control in controls {
        for name in [format!("task{control}name"), control.to_string()] {
            assert_eq!(
                validate_task_name(&name, "name"),
                Err(ApplicationError::InvalidInput {
                    field: "name",
                    reason: CONTROL_CHARACTER_REASON,
                }),
                "control={control:?}, name={name:?}"
            );
        }
    }

    assert_eq!(
        validate_task_name("\t", "name"),
        Err(ApplicationError::InvalidInput {
            field: "name",
            reason: CONTROL_CHARACTER_REASON,
        })
    );
}

#[test]
fn create_taskはcontrol名を拒否してrepositoryを変更しない() {
    let mut repository = TestTaskRepository::new(vec![], fixed_now());

    let actual = with_factory_id(Uuid::from_u128(200), |factory| {
        create_task(
            &mut repository,
            CreateTaskInput {
                name: "task\nname".to_string(),
                estimated_work_minutes: None,
                pending_until: None,
            },
            factory,
        )
    });

    assert_eq!(
        actual,
        Err(ApplicationError::InvalidInput {
            field: "name",
            reason: CONTROL_CHARACTER_REASON,
        })
    );
    assert!(repository.projects().is_empty());
}

#[test]
fn breakdown_taskはcontrol名を拒否して親を変更しない() {
    let parent = new_task_handle_at("親", fixed_now()).unwrap();
    let parent_id = parent.get_id().unwrap();
    let mut repository = TestTaskRepository::new(vec![parent.clone()], fixed_now());
    let mut sequence = 300_u128..;
    let mut next_id = move || Uuid::from_u128(sequence.next().unwrap());
    let mut factory = TaskFactory::new(fixed_now(), &mut next_id);

    let actual = breakdown_task(
        &mut repository,
        BreakdownTaskInput {
            parent_id,
            names: vec!["子".to_string(), "task\0name".to_string()],
            pending_until: None,
        },
        &mut factory,
    );

    assert_eq!(
        actual,
        Err(ApplicationError::InvalidInput {
            field: "names",
            reason: CONTROL_CHARACTER_REASON,
        })
    );
    assert!(parent.get_children().unwrap().is_empty());
}
