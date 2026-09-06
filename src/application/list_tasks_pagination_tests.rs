#[cfg(feature = "benchmarking")]
use super::list_tasks_page_with_metrics;
use super::{decode_cursor, encode_cursor, list_tasks_page, ListTasksFilter, ListTasksPageRequest};
use crate::application::task_use_case::ApplicationError;
use crate::entity::task::{ProjectCategory, Status};
use crate::test_support::{new_task_attr, new_task_handle, TestTaskRepository};
use chrono::{DateTime, Local, TimeZone};
use uuid::Uuid;

fn fixed_now() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 9, 6, 12, 0, 0).unwrap()
}

fn no_filter() -> ListTasksFilter {
    ListTasksFilter {
        period: None,
        statuses: vec![],
        categories: vec![],
    }
}

fn request(limit: Option<usize>) -> ListTasksPageRequest {
    ListTasksPageRequest {
        filter: no_filter(),
        query: None,
        root_task_id: None,
        limit,
        cursor: None,
    }
}

fn task_ids(page: &super::ListTasksPage) -> Vec<Uuid> {
    page.tasks.iter().map(|task| task.id).collect()
}

fn sample_repository() -> (TestTaskRepository, Vec<Uuid>, Uuid) {
    let first_root = new_task_handle("First Root").unwrap();
    let first_child = first_root.create_as_last_child(new_task_attr("ÄPFEL child"));
    let grandchild = first_child.create_as_last_child(new_task_attr("grandchild"));
    let second_child = first_root.create_as_last_child(new_task_attr("e\u{301} decomposed"));
    second_child.set_orig_status(Status::Done).unwrap();
    let second_root = new_task_handle("Second Root").unwrap();
    let expected = vec![
        first_root.get_id().unwrap(),
        first_child.get_id().unwrap(),
        grandchild.get_id().unwrap(),
        second_child.get_id().unwrap(),
        second_root.get_id().unwrap(),
    ];
    let first_child_id = first_child.get_id().unwrap();
    (
        TestTaskRepository::new(vec![first_root, second_root], fixed_now()),
        expected,
        first_child_id,
    )
}

#[test]
fn pageをlimit変更しながら連結すると既存pre_orderと一致する() {
    let (repository, expected, _) = sample_repository();
    let first = list_tasks_page(&repository, request(Some(2))).unwrap();
    assert_eq!(task_ids(&first), expected[..2]);

    let second = list_tasks_page(
        &repository,
        ListTasksPageRequest {
            cursor: first.next_cursor,
            limit: Some(1),
            ..request(Some(1))
        },
    )
    .unwrap();
    assert_eq!(task_ids(&second), expected[2..3]);

    let third = list_tasks_page(
        &repository,
        ListTasksPageRequest {
            cursor: second.next_cursor,
            limit: Some(3),
            ..request(Some(3))
        },
    )
    .unwrap();
    assert_eq!(task_ids(&third), expected[3..]);
    assert_eq!(third.next_cursor, None);
}

#[test]
fn queryはunicode_lowercase部分一致で空は無効化し正規化しない() {
    let (repository, expected, first_child_id) = sample_repository();
    let matching = list_tasks_page(
        &repository,
        ListTasksPageRequest {
            query: Some("äpf".to_string()),
            ..request(None)
        },
    )
    .unwrap();
    assert_eq!(task_ids(&matching), vec![first_child_id]);

    let empty = list_tasks_page(
        &repository,
        ListTasksPageRequest {
            query: Some(String::new()),
            ..request(None)
        },
    )
    .unwrap();
    assert_eq!(task_ids(&empty), expected);

    let composed = list_tasks_page(
        &repository,
        ListTasksPageRequest {
            query: Some("é".to_string()),
            ..request(None)
        },
    )
    .unwrap();
    assert!(composed.tasks.is_empty());
}

#[test]
fn root_filterはroot自身を含むsubtreeを返し不存在を区別する() {
    let (repository, expected, first_child_id) = sample_repository();
    let subtree = list_tasks_page(
        &repository,
        ListTasksPageRequest {
            root_task_id: Some(first_child_id),
            ..request(None)
        },
    )
    .unwrap();
    assert_eq!(task_ids(&subtree), expected[1..3]);

    let missing_id = Uuid::new_v4();
    assert_eq!(
        list_tasks_page(
            &repository,
            ListTasksPageRequest {
                root_task_id: Some(missing_id),
                ..request(None)
            },
        ),
        Err(ApplicationError::TaskNotFound(missing_id))
    );
}

#[test]
fn cursorはcanonical_filterを照合しlimitだけの変更を許可する() {
    let (repository, _, _) = sample_repository();
    let mut first_request = request(Some(1));
    first_request.query = Some("ROOT".to_string());
    first_request.filter.statuses = vec![Status::Done, Status::Todo];
    first_request.filter.categories = vec![None, Some(ProjectCategory::Recovery)];
    let first = list_tasks_page(&repository, first_request).unwrap();

    let mut equivalent = request(Some(2));
    equivalent.query = Some("root".to_string());
    equivalent.filter.statuses = vec![Status::Todo, Status::Done, Status::Todo];
    equivalent.filter.categories = vec![Some(ProjectCategory::Recovery), None, None];
    equivalent.cursor = first.next_cursor.clone();
    assert!(list_tasks_page(&repository, equivalent).is_ok());

    let mismatch = list_tasks_page(
        &repository,
        ListTasksPageRequest {
            query: Some("different".to_string()),
            cursor: first.next_cursor,
            ..request(Some(1))
        },
    );
    assert_eq!(
        mismatch,
        Err(ApplicationError::InvalidInput {
            field: "cursor",
            reason: "filter mismatch",
        })
    );
}

#[test]
fn cursorはmalformed_version_revision_resumeの不一致を区別する() {
    let (repository, _, _) = sample_repository();
    let page = list_tasks_page(&repository, request(Some(1))).unwrap();
    let cursor = page.next_cursor.unwrap();

    assert_eq!(
        list_tasks_page(
            &repository,
            ListTasksPageRequest {
                cursor: Some("not-a-cursor".to_string()),
                ..request(Some(1))
            },
        ),
        Err(ApplicationError::InvalidInput {
            field: "cursor",
            reason: "malformed cursor",
        })
    );

    let mut decoded = decode_cursor(&cursor).unwrap();
    decoded.version += 1;
    assert_cursor_error(&repository, encode_cursor(&decoded), "version mismatch");

    let mut decoded = decode_cursor(&cursor).unwrap();
    decoded.repository_revision = Some(Uuid::new_v4());
    assert_cursor_error(&repository, encode_cursor(&decoded), "revision mismatch");

    let mut decoded = decode_cursor(&cursor).unwrap();
    decoded.resume_after_position = vec![usize::MAX];
    assert_cursor_error(
        &repository,
        encode_cursor(&decoded),
        "resume position mismatch",
    );

    let mut decoded = decode_cursor(&cursor).unwrap();
    decoded.previous_task_id = Uuid::new_v4();
    assert_cursor_error(
        &repository,
        encode_cursor(&decoded),
        "resume position mismatch",
    );
}

fn assert_cursor_error(
    repository: &TestTaskRepository,
    cursor: String,
    expected_reason: &'static str,
) {
    assert_eq!(
        list_tasks_page(
            repository,
            ListTasksPageRequest {
                cursor: Some(cursor),
                ..request(Some(1))
            },
        ),
        Err(ApplicationError::InvalidInput {
            field: "cursor",
            reason: expected_reason,
        })
    );
}

#[test]
fn limitは1から500でnoneだけがunboundedになる() {
    let (repository, expected, _) = sample_repository();
    assert_eq!(
        task_ids(&list_tasks_page(&repository, request(None)).unwrap()),
        expected
    );
    for invalid_limit in [0, 501] {
        assert_eq!(
            list_tasks_page(&repository, request(Some(invalid_limit))),
            Err(ApplicationError::InvalidInput {
                field: "limit",
                reason: "must be between 1 and 500",
            })
        );
    }
}

#[cfg(feature = "benchmarking")]
#[test]
fn typicalとstressはpage境界で走査を止め全taskを保持しない() {
    for (project_count, task_count) in [(2_213, 26_378), (8_852, 105_512)] {
        let repository = synthetic_repository(project_count, task_count);
        let (page, metrics) =
            list_tasks_page_with_metrics(&repository, request(Some(100))).unwrap();
        let max_child_count = (task_count - project_count).div_ceil(project_count);
        assert_eq!(page.tasks.len(), 100);
        assert!(page.next_cursor.is_some());
        assert_eq!(metrics.visited_task_count, 101);
        assert!(metrics.peak_retained_task_count <= project_count + max_child_count);
        assert!(serde_json::to_vec(&page).unwrap().len() <= 128 * 1024);
    }
}

#[cfg(feature = "benchmarking")]
fn synthetic_repository(project_count: usize, task_count: usize) -> TestTaskRepository {
    let mut projects = (0..project_count)
        .map(|index| new_task_handle(&format!("fixture-project-{index:05}")).unwrap())
        .collect::<Vec<_>>();
    for index in 0..task_count - project_count {
        let project_index = index % project_count;
        projects[project_index]
            .create_as_last_child(new_task_attr(&format!("fixture-task-{index:06}")));
    }
    TestTaskRepository::new(projects, fixed_now())
}
