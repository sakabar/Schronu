use super::error::WebReadError;
use super::model::{AllTaskPageDto, AllTaskRowDto, ServerSnapshot, WebSuccess};
use std::collections::VecDeque;
use uuid::Uuid;

const PAGE_SIZE: usize = 500;
const MAX_SNAPSHOTS: usize = 8;

struct AllTaskSnapshot {
    id: Uuid,
    snapshot: ServerSnapshot,
    rows: Vec<AllTaskRowDto>,
}

#[derive(Default)]
pub(super) struct AllTaskSnapshots {
    snapshots: VecDeque<AllTaskSnapshot>,
}

impl AllTaskSnapshots {
    pub(super) fn first_page(
        &mut self,
        snapshot: ServerSnapshot,
        rows: Vec<AllTaskRowDto>,
    ) -> WebSuccess<AllTaskPageDto> {
        if rows.len() <= PAGE_SIZE {
            return WebSuccess {
                snapshot,
                data: AllTaskPageDto {
                    rows,
                    next_cursor: None,
                },
            };
        }

        let id = Uuid::new_v4();
        let page_rows = rows[..PAGE_SIZE].to_vec();
        if self.snapshots.len() == MAX_SNAPSHOTS {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(AllTaskSnapshot {
            id,
            snapshot: snapshot.clone(),
            rows,
        });
        WebSuccess {
            snapshot,
            data: AllTaskPageDto {
                rows: page_rows,
                next_cursor: Some(cursor(id, PAGE_SIZE)),
            },
        }
    }

    pub(super) fn next_page(
        &mut self,
        cursor_value: &str,
    ) -> Result<WebSuccess<AllTaskPageDto>, WebReadError> {
        let (id, offset) = parse_cursor(cursor_value)?;
        let index = self
            .snapshots
            .iter()
            .position(|snapshot| snapshot.id == id)
            .ok_or(WebReadError::InvalidCursor)?;
        let stored = &self.snapshots[index];
        if offset >= stored.rows.len() {
            return Err(WebReadError::InvalidCursor);
        }
        let end = offset.saturating_add(PAGE_SIZE).min(stored.rows.len());
        let snapshot = stored.snapshot.clone();
        let rows = stored.rows[offset..end].to_vec();
        let next_cursor = (end < stored.rows.len()).then(|| cursor(id, end));
        if next_cursor.is_none() {
            self.snapshots.remove(index);
        }
        Ok(WebSuccess {
            snapshot,
            data: AllTaskPageDto { rows, next_cursor },
        })
    }
}

fn cursor(id: Uuid, offset: usize) -> String {
    format!("{}:{offset}", id.hyphenated())
}

fn parse_cursor(value: &str) -> Result<(Uuid, usize), WebReadError> {
    let (id, offset) = value.split_once(':').ok_or(WebReadError::InvalidCursor)?;
    if offset.contains(':') {
        return Err(WebReadError::InvalidCursor);
    }
    let id = Uuid::parse_str(id).map_err(|_| WebReadError::InvalidCursor)?;
    let offset = offset
        .parse::<usize>()
        .map_err(|_| WebReadError::InvalidCursor)?;
    if offset == 0 || offset % PAGE_SIZE != 0 {
        return Err(WebReadError::InvalidCursor);
    }
    Ok((id, offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(segment_index: usize) -> AllTaskRowDto {
        AllTaskRowDto {
            task: super::super::model::SessionTaskDto {
                task_id: "task".to_owned(),
                task_name: "name".to_owned(),
                estimated_work_seconds: 1,
                actual_work_seconds: 0,
            },
            segment_index,
            schedule_date: "2026-09-05".to_owned(),
            deadline_epoch_ms: None,
            deadline_label: "____/__/__".to_owned(),
            misses_deadline: false,
            is_leaf: true,
        }
    }

    fn snapshot(value: i64) -> ServerSnapshot {
        ServerSnapshot {
            observed_at_epoch_ms: value,
            logical_date: "2026-09-05".to_owned(),
            buffer_seconds: value,
        }
    }

    #[test]
    fn page境界とsnapshot解放を固定する() {
        let mut store = AllTaskSnapshots::default();
        let one_page = store.first_page(snapshot(1), (0..500).map(row).collect());
        assert_eq!(one_page.data.rows.len(), 500);
        assert_eq!(one_page.data.next_cursor, None);

        let first = store.first_page(snapshot(2), (0..501).map(row).collect());
        assert_eq!(first.data.rows.len(), 500);
        let cursor = first.data.next_cursor.unwrap();
        let final_page = store.next_page(&cursor).unwrap();
        assert_eq!(final_page.snapshot, snapshot(2));
        assert_eq!(final_page.data.rows[0].segment_index, 500);
        assert_eq!(final_page.data.next_cursor, None);
        assert!(matches!(
            store.next_page(&cursor),
            Err(WebReadError::InvalidCursor)
        ));
    }

    #[test]
    fn cursor形式と範囲を全て検証する() {
        let mut store = AllTaskSnapshots::default();
        let first = store.first_page(snapshot(1), (0..1_001).map(row).collect());
        let valid = first.data.next_cursor.unwrap();
        let id = valid.split(':').next().unwrap();
        for invalid in [
            "",
            "not-a-uuid:500",
            &format!("{id}:nope"),
            &format!("{id}:0"),
            &format!("{id}:1"),
            &format!("{id}:1500"),
            &format!("{id}:500:extra"),
            "00000000-0000-4000-8000-000000000001:500",
        ] {
            assert!(
                matches!(store.next_page(invalid), Err(WebReadError::InvalidCursor)),
                "{invalid}"
            );
        }
    }

    #[test]
    fn 九個目のsnapshotは最古だけをfifoで破棄する() {
        let mut store = AllTaskSnapshots::default();
        let cursors = (0..9)
            .map(|value| {
                store
                    .first_page(snapshot(value), (0..501).map(row).collect())
                    .data
                    .next_cursor
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            store.next_page(&cursors[0]),
            Err(WebReadError::InvalidCursor)
        ));
        for cursor in &cursors[1..] {
            assert!(store.next_page(cursor).is_ok());
        }
    }
}
