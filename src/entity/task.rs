use chrono::{DateTime, Duration, Local};
use dendron::{HotNode, InsertAs, Node};
use serde::Serialize;
use std::cmp::{max, min};
use std::fmt;
use uuid::Uuid;

use crate::entity::datetime::{LogicalDateTimePolicy, DEFAULT_END_OF_DAY_OFFSET_MINUTES};

#[derive(Copy, Clone, Debug, PartialEq, Serialize)]
pub enum Status {
    // 初期状態
    #[serde(rename = "todo")]
    Todo,

    // 優先度が低いなどの理由でスコープアウトした状態
    // 相手ボールの場合は相手の返答をウォッチして適宜つつくという作業があるので、Pendingではない
    #[serde(rename = "pending")]
    Pending,

    // 完了
    #[serde(rename = "done")]
    Done,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Status::Todo => {
                write!(f, "todo")
            }
            Status::Pending => {
                write!(f, "pending")
            }
            Status::Done => {
                write!(f, "done")
            }
        }
    }
}

pub fn read_status(s: &str) -> Option<Status> {
    let lc = s.to_lowercase();

    if lc == "todo" {
        return Some(Status::Todo);
    } else if lc == "pending" {
        return Some(Status::Pending);
    } else if lc == "done" {
        return Some(Status::Done);
    }

    None
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize)]
pub enum RepetitionAnchor {
    #[serde(rename = "deadline")]
    Deadline,
    #[serde(rename = "completion")]
    Completion,
}

impl fmt::Display for RepetitionAnchor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RepetitionAnchor::Deadline => write!(f, "deadline"),
            RepetitionAnchor::Completion => write!(f, "completion"),
        }
    }
}

pub(crate) fn fixed_start_applies_to_schedule(
    fixed_start: bool,
    repetition_interval_days_opt: Option<i64>,
) -> bool {
    fixed_start && repetition_interval_days_opt.is_none()
}

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum ProjectCategory {
    #[serde(rename = "earning")]
    Earning,
    #[serde(rename = "sustaining")]
    Sustaining,
    #[serde(rename = "recovery")]
    Recovery,
    #[serde(rename = "investment")]
    Investment,
    #[serde(rename = "consumption")]
    Consumption,
}

impl fmt::Display for ProjectCategory {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ProjectCategory::Earning => write!(f, "earning"),
            ProjectCategory::Sustaining => write!(f, "sustaining"),
            ProjectCategory::Recovery => write!(f, "recovery"),
            ProjectCategory::Investment => write!(f, "investment"),
            ProjectCategory::Consumption => write!(f, "consumption"),
        }
    }
}

pub fn read_project_category(s: &str) -> Option<ProjectCategory> {
    match s.to_lowercase().as_str() {
        "earning" | "獲" => Some(ProjectCategory::Earning),
        "sustaining" | "維" => Some(ProjectCategory::Sustaining),
        "recovery" | "回" => Some(ProjectCategory::Recovery),
        "investment" | "資" => Some(ProjectCategory::Investment),
        "consumption" | "消" => Some(ProjectCategory::Consumption),
        _ => None,
    }
}

// Todoの葉タスクを抽出する
pub fn extract_leaf_tasks_from_project(
    task: &TaskHandle,
) -> Result<Vec<TaskHandle>, TaskTreeError> {
    let target_status: Vec<Status> = vec![Status::Todo];
    extract_leaf_tasks_from_project_rec(task, &target_status)
}

// TodoもしくはPendingの葉タスクを抽出する
pub fn extract_leaf_tasks_from_project_with_pending(
    task: &TaskHandle,
) -> Result<Vec<TaskHandle>, TaskTreeError> {
    let target_status: Vec<Status> = vec![Status::Todo, Status::Pending];
    extract_leaf_tasks_from_project_rec(task, &target_status)
}

fn extract_leaf_tasks_from_project_rec(
    task: &TaskHandle,
    target_status_arr: &Vec<Status>,
) -> Result<Vec<TaskHandle>, TaskTreeError> {
    let mut children_are_all_done = true;
    for child_node in task.node.children() {
        if child_node
            .try_borrow_data()
            .map_err(|_| TaskTreeError::Borrow)?
            .get_status()
            != &Status::Done
        {
            children_are_all_done = false;
            break;
        }
    }

    if target_status_arr.contains(&task.get_status()?)
        && (!task.node.has_children() || children_are_all_done)
    {
        let new_task = TaskHandle {
            node: task.node.clone(),
        };
        return Ok(vec![new_task]);
    }

    let mut ans: Vec<TaskHandle> = vec![];

    // 深さ優先
    for child_node in task.node.children() {
        if child_node
            .try_borrow_data()
            .map_err(|_| TaskTreeError::Borrow)?
            .get_status()
            != &Status::Done
        {
            let child_task = TaskHandle { node: child_node };

            let leaves_with_pending: Vec<TaskHandle> =
                extract_leaf_tasks_from_project_rec(&child_task, target_status_arr)?;

            let mut leaves = Vec::new();
            for leaf in leaves_with_pending {
                if target_status_arr.contains(&leaf.get_status()?) {
                    leaves.push(TaskHandle {
                        node: leaf.node.clone(),
                    });
                }
            }
            ans.append(&mut leaves);
        }
    }

    Ok(ans)
}

pub fn round_up_sec_as_minute(seconds: i64) -> i64 {
    seconds / 60 + if seconds % 60 == 0 { 0 } else { 1 }
}

#[derive(Clone)]
pub struct TaskAttr {
    id: Uuid,
    name: String,
    orig_status: Status, // 元々のステータス。orig_status=Pendingの時、時刻によらずPendingのまま。
    status: Status, // 評価後のステータス。pendingはpending_untilを加味して評価され、Todo扱いとなる
    is_on_other_side: bool, // 相手ボールか?
    atomic: bool,   // 分割できないタスクか?
    fixed_start: bool, // raw属性。予定上fixedかは反復間隔も含めて判定する。
    pending_until: DateTime<Local>,
    last_synced_time: DateTime<Local>,

    priority: i64, // 優先度。大きいほど高い

    create_time: DateTime<Local>,               // タスクが生成された日時
    start_time: DateTime<Local>,                // タスクが着手可能になった日時
    end_time_opt: Option<DateTime<Local>>,      // タスクが完了した日時
    deadline_time_opt: Option<DateTime<Local>>, // タスクの〆切

    estimated_work_seconds: i64, // 見積もられた作業時間 (秒)
    actual_work_seconds: i64,    // 実際の作業時間 (秒)

    repetition_interval_days_opt: Option<i64>,
    repetition_anchor: RepetitionAnchor,
    days_in_advance: i64, // 繰り返しタスクについて、何日前から着手開始可能とするか
    project_category_opt: Option<ProjectCategory>,
    persistent_mutation_revision: u64,
}

// 生成するタイミングで結果が変わってしまうid, create_time, start_timeは
// 等価性判定には用いない
impl PartialEq for TaskAttr {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.orig_status == other.orig_status
            && self.status == other.status
            && self.is_on_other_side == other.is_on_other_side
            && self.atomic == other.atomic
            && self.fixed_start == other.fixed_start
            && self.pending_until == other.pending_until
            && self.last_synced_time == other.last_synced_time
            && self.priority == other.priority
            // && self.create_time == other.create_time
            // && self.start_time == other.start_time
            && self.end_time_opt == other.end_time_opt
            && self.deadline_time_opt == other.deadline_time_opt
            && self.estimated_work_seconds == other.estimated_work_seconds
            && self.actual_work_seconds == other.actual_work_seconds
            && self.repetition_interval_days_opt == other.repetition_interval_days_opt
            && self.repetition_anchor == other.repetition_anchor
            && self.days_in_advance == other.days_in_advance
            && self.project_category_opt == other.project_category_opt
    }
}

// ツリーを出力した際に複数行にまたがると見映えが悪くなるため、情報を落としている
impl fmt::Debug for TaskAttr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status_checkbox: &str = match self.status {
            Status::Todo => "[ ]",
            Status::Pending => "[-]",
            Status::Done => "[+]",
        };

        f.debug_struct("")
            .field(
                "name",
                &format!(
                    "{} {:02}m/{:02}m {}{}",
                    status_checkbox,
                    round_up_sec_as_minute(self.get_actual_work_seconds()),
                    round_up_sec_as_minute(self.get_estimated_work_seconds()),
                    if self.is_on_other_side {
                        "[待ち]"
                    } else {
                        ""
                    },
                    self.name
                )
                .as_str(),
            )
            .field("id", &self.id)
            // .field("orig_status", &self.orig_status)
            // .field("status", &self.status)
            // .field("pending_until", &self.pending_until)
            // .field("last_synced_time", &self.last_synced_time)
            // .field("priority", &self.priority)
            .finish()
    }
}

impl TaskAttr {
    pub fn with_identity(name: &str, id: Uuid, now: DateTime<Local>) -> Self {
        Self {
            id,
            name: name.to_string(),
            orig_status: Status::Todo,
            status: Status::Todo,
            is_on_other_side: false,
            atomic: false,
            fixed_start: false,
            pending_until: DateTime::<Local>::MIN_UTC.into(),
            last_synced_time: DateTime::<Local>::MIN_UTC.into(),
            priority: 0,
            create_time: now,
            start_time: now,
            end_time_opt: None,
            deadline_time_opt: None,
            estimated_work_seconds: 900,
            actual_work_seconds: 0,
            repetition_interval_days_opt: None,
            repetition_anchor: RepetitionAnchor::Deadline,
            days_in_advance: 0,
            project_category_opt: None,
            persistent_mutation_revision: 0,
        }
    }

    pub fn get_id(&self) -> &Uuid {
        &self.id
    }

    pub fn set_id(&mut self, id: Uuid) {
        self.id = id;
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn set_orig_status(&mut self, orig_status: Status) {
        self.orig_status = orig_status;
        let datetime_policy = LogicalDateTimePolicy::new(DEFAULT_END_OF_DAY_OFFSET_MINUTES);

        // pending_untilが〆切よりも後ろになってしまっている場合はpending_untilを調整する
        if let Some(deadline_time) = self
            .deadline_time_opt
            .filter(|_| self.orig_status == Status::Pending)
        {
            let pending_time_before_deadline =
                datetime_policy.deadline_pending_limit(deadline_time, self.estimated_work_seconds);

            if pending_time_before_deadline < self.pending_until {
                self.pending_until = pending_time_before_deadline;
            }
        }

        // 変わりうるのは、
        // not Done -> Todo (deadlineが近い)
        // Pending -> Todo (pending_until後 かつ start_time後)
        // Todo -> Pending (start_time起因)
        let should_be_todo = (self.orig_status != Status::Done
            && self.last_synced_time > self.start_time
            && self.deadline_time_opt.is_some()
            && datetime_policy.deadline_force_todo_after_start_threshold(
                self.deadline_time_opt.unwrap(),
                max(0, self.estimated_work_seconds - self.actual_work_seconds),
            ) < self.last_synced_time)
            || (self.orig_status != Status::Done
                && self.last_synced_time < self.start_time
                && self.deadline_time_opt.is_some()
                && datetime_policy.deadline_pending_limit(
                    self.deadline_time_opt.unwrap(),
                    self.estimated_work_seconds,
                ) < self.last_synced_time)
            || (self.orig_status == Status::Pending
                && self.last_synced_time > self.pending_until
                && self.last_synced_time > self.start_time);

        self.status = if should_be_todo {
            Status::Todo
        } else if self.orig_status == Status::Todo && self.start_time > self.last_synced_time {
            Status::Pending
        } else {
            self.orig_status
        };
    }

    pub fn get_status(&self) -> &Status {
        &self.status
    }

    pub fn get_orig_status(&self) -> &Status {
        &self.orig_status
    }

    pub fn get_is_on_other_side(&self) -> &bool {
        &self.is_on_other_side
    }

    pub fn set_is_on_other_side(&mut self, is_on_other_side: bool) {
        self.is_on_other_side = is_on_other_side;
    }

    pub fn get_atomic(&self) -> bool {
        self.atomic
    }

    pub fn set_atomic(&mut self, atomic: bool) {
        self.atomic = atomic;
    }

    pub fn get_fixed_start(&self) -> bool {
        self.fixed_start
    }

    pub(crate) fn fixed_start_applies_to_schedule(&self) -> bool {
        fixed_start_applies_to_schedule(self.fixed_start, self.repetition_interval_days_opt)
    }

    pub fn set_fixed_start(&mut self, fixed_start: bool) {
        self.fixed_start = fixed_start;
    }

    // 時刻を入力し、その時刻を用いてpending判定を行う。
    pub fn sync_clock(&mut self, now: DateTime<Local>) {
        self.last_synced_time = now;
        self.set_orig_status(*self.get_orig_status());
    }

    pub fn get_last_synced_time(&self) -> &DateTime<Local> {
        &self.last_synced_time
    }

    pub fn set_pending_until(&mut self, pending_until: DateTime<Local>) {
        self.pending_until = pending_until;
        self.set_orig_status(*self.get_orig_status());
    }

    pub fn get_pending_until(&self) -> &DateTime<Local> {
        &self.pending_until
    }

    pub fn set_priority(&mut self, priority: i64) {
        self.priority = priority;
    }

    pub fn get_priority(&self) -> i64 {
        self.priority
    }

    pub fn set_create_time(&mut self, create_time: DateTime<Local>) {
        self.create_time = create_time;
    }

    pub fn get_create_time(&self) -> &DateTime<Local> {
        &self.create_time
    }

    pub fn set_start_time(&mut self, start_time: DateTime<Local>) {
        self.start_time = start_time;
        self.set_orig_status(*self.get_orig_status());
    }

    pub fn get_start_time(&self) -> &DateTime<Local> {
        &self.start_time
    }

    pub fn set_end_time_opt(&mut self, end_time_opt: Option<DateTime<Local>>) {
        self.end_time_opt = end_time_opt;
    }

    pub fn get_end_time_opt(&self) -> &Option<DateTime<Local>> {
        &self.end_time_opt
    }

    pub fn set_deadline_time_opt(&mut self, deadline_time_opt: Option<DateTime<Local>>) {
        self.deadline_time_opt = deadline_time_opt;
    }

    pub fn get_deadline_time_opt(&self) -> &Option<DateTime<Local>> {
        &self.deadline_time_opt
    }

    pub fn set_estimated_work_seconds(&mut self, estimated_work_seconds: i64) {
        self.estimated_work_seconds = estimated_work_seconds;
    }

    pub fn get_estimated_work_seconds(&self) -> i64 {
        self.estimated_work_seconds
    }

    pub fn set_actual_work_seconds(&mut self, actual_work_seconds: i64) {
        self.actual_work_seconds = actual_work_seconds;
    }

    pub fn get_actual_work_seconds(&self) -> i64 {
        self.actual_work_seconds
    }

    pub fn set_repetition_interval_days_opt(&mut self, repetition_interval_days_opt: Option<i64>) {
        self.repetition_interval_days_opt = repetition_interval_days_opt;
    }

    pub fn get_repetition_interval_days_opt(&self) -> Option<i64> {
        self.repetition_interval_days_opt
    }

    pub fn set_repetition_anchor(&mut self, repetition_anchor: RepetitionAnchor) {
        self.repetition_anchor = repetition_anchor;
    }

    pub fn get_repetition_anchor(&self) -> RepetitionAnchor {
        self.repetition_anchor
    }

    pub fn set_days_in_advance(&mut self, days_in_advance: i64) {
        self.days_in_advance = days_in_advance;
    }

    pub fn get_days_in_advance(&self) -> i64 {
        self.days_in_advance
    }

    pub fn set_project_category_opt(&mut self, project_category_opt: Option<ProjectCategory>) {
        self.project_category_opt = project_category_opt;
    }

    pub fn get_project_category_opt(&self) -> Option<ProjectCategory> {
        self.project_category_opt
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskHandle {
    node: Node<TaskAttr>,
}

/// An owned, recursively independent view of a task subtree.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskSnapshot {
    attr: TaskAttr,
    children: Vec<Self>,
}

impl TaskSnapshot {
    pub fn attr(&self) -> &TaskAttr {
        &self.attr
    }

    pub fn name(&self) -> &str {
        self.attr.get_name()
    }

    pub fn estimated_work_seconds(&self) -> i64 {
        self.attr.get_estimated_work_seconds()
    }

    pub fn children(&self) -> &[Self] {
        &self.children
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskTreeError {
    RootOperation,
    Cycle,
    InvalidSequence,
    Borrow,
    HierarchyGrant,
    Insert,
    /// The hidden dummy root no longer has exactly one task child, or this handle is outside it.
    MissingDummyRootChild,
}

impl fmt::Display for TaskTreeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self {
            Self::RootOperation => "cannot modify the project root hierarchy",
            Self::Cycle => "cannot insert a task into its own descendant",
            Self::InvalidSequence => "sequential child range must not be empty",
            Self::Borrow => "cannot borrow task tree data",
            Self::HierarchyGrant => "cannot acquire hierarchy edit grant",
            Self::Insert => "cannot insert task subtree",
            Self::MissingDummyRootChild => {
                "task tree dummy root must have exactly one task child containing this handle"
            }
        };
        formatter.write_str(reason)
    }
}

impl std::error::Error for TaskTreeError {}

impl TaskHandle {
    // dendron::Node::try_detach_insert_subtree()は木そのものを消滅させることができない仕様のようなので、
    // ダミーのルートノードを用意することで、使いたいノードが全て子ノードになるようにする
    pub fn with_identity(
        name: &str,
        id: Uuid,
        now: DateTime<Local>,
    ) -> Result<Self, TaskTreeError> {
        let dummy_attr =
            TaskAttr::with_identity(format!("dummy-for-{}", name).as_str(), Uuid::nil(), now);
        let dummy_root = Node::new_tree(dummy_attr);

        let grant = dummy_root
            .tree()
            .grant_hierarchy_edit()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;
        let task_attr = TaskAttr::with_identity(name, id, now);
        dummy_root.create_as_last_child(&grant, task_attr);

        let node = dummy_root
            .first_child()
            .ok_or(TaskTreeError::MissingDummyRootChild)?;

        Ok(Self { node })
    }

    pub fn get_attr(&self) -> Result<TaskAttr, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.clone())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn parent(&self) -> Result<Option<Self>, TaskTreeError> {
        if self.node.parent() == Some(self.node.root()) {
            return Ok(None);
        }

        Ok(self.node.parent().map(|node| Self { node }))
    }

    pub fn root(&self) -> Result<Self, TaskTreeError> {
        self.get_attr()?;
        let dummy_root = self.node.root();
        let mut children = dummy_root.children();
        let task_root = children
            .next()
            .ok_or(TaskTreeError::MissingDummyRootChild)?;
        if children.next().is_some() {
            return Err(TaskTreeError::MissingDummyRootChild);
        }

        let mut ancestor = Some(self.node.clone());
        while let Some(node) = ancestor {
            if node.ptr_eq(&task_root) {
                return Ok(Self { node: task_root });
            }
            ancestor = node.parent();
        }

        Err(TaskTreeError::MissingDummyRootChild)
    }

    pub fn get_children(&self) -> Result<Vec<Self>, TaskTreeError> {
        self.get_attr()?;
        Ok(self.node.children().map(|node| Self { node }).collect())
    }

    pub fn snapshot(&self) -> Result<TaskSnapshot, TaskTreeError> {
        Ok(TaskSnapshot {
            attr: self.get_attr()?,
            children: self
                .node
                .children()
                .map(|node| Self { node }.snapshot())
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    pub fn create_child(&self, task_attr: TaskAttr) -> Result<Self, TaskTreeError> {
        let root = self.root()?;
        root.ensure_persistent_mutation_writable()?;
        let grant = self
            .node
            .tree()
            .grant_hierarchy_edit()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;
        let child_node = self.node.create_as_last_child(&grant, task_attr);
        root.mark_persistent_mutation()?;
        Ok(Self { node: child_node })
    }

    #[allow(dead_code)]
    pub(crate) fn complete_with_next_repetition(
        &self,
        actual_work_seconds: i64,
        finished_at: DateTime<Local>,
        adjusted_parent_estimated_work_seconds: i64,
        next_task_attr: TaskAttr,
    ) -> Result<Self, TaskTreeError> {
        let parent = self.parent()?.ok_or(TaskTreeError::RootOperation)?;
        let root = self.root()?;

        root.ensure_persistent_mutation_writable()?;
        self.node
            .try_borrow_data_mut()
            .map_err(|_| TaskTreeError::Borrow)?;
        parent
            .node
            .try_borrow_data_mut()
            .map_err(|_| TaskTreeError::Borrow)?;
        let grant = parent
            .node
            .tree()
            .grant_hierarchy_edit()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;

        let child_node = parent.node.create_as_last_child(&grant, next_task_attr);
        {
            let mut attr = self
                .node
                .try_borrow_data_mut()
                .map_err(|_| TaskTreeError::Borrow)?;
            attr.set_actual_work_seconds(actual_work_seconds);
            attr.set_orig_status(Status::Done);
            attr.set_end_time_opt(Some(finished_at));
        }
        parent
            .node
            .try_borrow_data_mut()
            .map_err(|_| TaskTreeError::Borrow)?
            .set_estimated_work_seconds(adjusted_parent_estimated_work_seconds);
        root.mark_persistent_mutation()?;

        Ok(Self { node: child_node })
    }

    pub fn reparent_to(&mut self, parent_task: &Self) -> Result<(), TaskTreeError> {
        if self.node.ptr_eq(&parent_task.node)
            || parent_task
                .node
                .ancestors()
                .any(|ancestor| ancestor.ptr_eq(&self.node))
        {
            return Err(TaskTreeError::Cycle);
        }

        let source_root = self.root()?;
        let destination_root = parent_task.root()?;
        source_root.ensure_persistent_mutation_writable()?;
        if !source_root.node.ptr_eq(&destination_root.node) {
            destination_root.ensure_persistent_mutation_writable()?;
        }
        let self_grant = self
            .node
            .tree()
            .grant_hierarchy_edit()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;
        let parent_task_hot: HotNode<TaskAttr> = parent_task
            .node
            .clone()
            .bundle_new_hierarchy_edit_grant()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;

        self.node
            .try_detach_insert_subtree(&self_grant, InsertAs::LastChildOf(&parent_task_hot))
            .map_err(|_| TaskTreeError::Insert)?;

        source_root.mark_persistent_mutation()?;
        if !source_root.node.ptr_eq(&destination_root.node) {
            destination_root.mark_persistent_mutation()?;
        }
        Ok(())
    }

    /// Inserts a newly-created parent between this task and its current parent.
    pub fn create_parent(&mut self, task_attr: TaskAttr) -> Result<(), TaskTreeError> {
        let original_parent = self.parent()?.ok_or(TaskTreeError::RootOperation)?;
        let root = self.root()?;
        root.ensure_persistent_mutation_writable()?;
        let grant = self
            .node
            .tree()
            .grant_hierarchy_edit()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;
        let new_parent_node = original_parent.node.create_as_last_child(&grant, task_attr);
        let new_parent = Self {
            node: new_parent_node,
        };
        let parent_hot = new_parent
            .node
            .clone()
            .bundle_new_hierarchy_edit_grant()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;

        self.node
            .try_detach_insert_subtree(&grant, InsertAs::LastChildOf(&parent_hot))
            .map_err(|_| TaskTreeError::Insert)?;
        root.mark_persistent_mutation()?;
        Ok(())
    }

    /// Creates a chain of child tasks and returns the deepest child.
    pub fn create_sequential_children(
        &self,
        task_name: &str,
        estimated_work_seconds: i64,
        begin_index: u64,
        end_index: u64,
        task_name_suffix: &str,
        mut create_task_attr: impl FnMut(&str) -> TaskAttr,
    ) -> Result<Self, TaskTreeError> {
        if begin_index > end_index {
            return Err(TaskTreeError::InvalidSequence);
        }
        let root = self.root()?;
        root.ensure_persistent_mutation_writable()?;
        let grant = self
            .node
            .tree()
            .grant_hierarchy_edit()
            .map_err(|_| TaskTreeError::HierarchyGrant)?;
        let mut current_node = self.node.clone();

        for index in (begin_index..=end_index).rev() {
            let mut task_attr = create_task_attr(&format!("{task_name} {index}{task_name_suffix}"));
            task_attr.set_estimated_work_seconds(estimated_work_seconds);
            current_node = current_node.create_as_last_child(&grant, task_attr);
        }

        root.mark_persistent_mutation()?;
        Ok(Self { node: current_node })
    }

    pub fn get_id(&self) -> Result<Uuid, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_id())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_id(&mut self, id: Uuid) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_id() == &id {
                false
            } else {
                attr.set_id(id);
                true
            }
        })
    }

    pub(crate) fn get_persistent_mutation_revision(&self) -> Result<u64, TaskTreeError> {
        self.root()?
            .node
            .try_borrow_data()
            .map(|attr| attr.persistent_mutation_revision)
            .map_err(|_| TaskTreeError::Borrow)
    }

    fn mark_persistent_mutation(&self) -> Result<(), TaskTreeError> {
        let root = self.root()?;
        let mut attr = root
            .node
            .try_borrow_data_mut()
            .map_err(|_| TaskTreeError::Borrow)?;
        attr.persistent_mutation_revision = attr.persistent_mutation_revision.wrapping_add(1);
        Ok(())
    }

    fn ensure_persistent_mutation_writable(&self) -> Result<(), TaskTreeError> {
        self.node
            .try_borrow_data_mut()
            .map(|_| ())
            .map_err(|_| TaskTreeError::Borrow)
    }

    fn update(&self, update: impl FnOnce(&mut TaskAttr) -> bool) -> Result<(), TaskTreeError> {
        let root = self.root()?;
        self.node
            .try_borrow_data()
            .map_err(|_| TaskTreeError::Borrow)?;
        root.ensure_persistent_mutation_writable()?;
        let changed = update(
            &mut *self
                .node
                .try_borrow_data_mut()
                .map_err(|_| TaskTreeError::Borrow)?,
        );
        if changed {
            root.mark_persistent_mutation()?;
        }
        Ok(())
    }

    pub fn get_name(&self) -> Result<String, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_name().to_string())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn get_status(&self) -> Result<Status, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_status())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn get_orig_status(&self) -> Result<Status, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_orig_status())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_orig_status(&self, orig_status: Status) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            let before = (*attr.get_orig_status(), *attr.get_pending_until());
            attr.set_orig_status(orig_status);
            before != (*attr.get_orig_status(), *attr.get_pending_until())
        })
    }

    pub fn get_is_on_other_side(&self) -> Result<bool, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_is_on_other_side())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_is_on_other_side(&self, is_on_other_side: bool) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_is_on_other_side() == &is_on_other_side {
                false
            } else {
                attr.set_is_on_other_side(is_on_other_side);
                true
            }
        })
    }

    pub fn get_atomic(&self) -> Result<bool, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_atomic())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_atomic(&self, atomic: bool) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_atomic() == atomic {
                false
            } else {
                attr.set_atomic(atomic);
                true
            }
        })
    }

    pub fn get_fixed_start(&self) -> Result<bool, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_fixed_start())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub(crate) fn fixed_start_applies_to_schedule(&self) -> Result<bool, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.fixed_start_applies_to_schedule())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_fixed_start(&self, fixed_start: bool) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_fixed_start() == fixed_start {
                false
            } else {
                attr.set_fixed_start(fixed_start);
                true
            }
        })
    }

    pub fn set_pending_until(&self, pending_until: DateTime<Local>) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            let before = *attr.get_pending_until();
            attr.set_pending_until(pending_until);
            before != *attr.get_pending_until()
        })
    }

    pub fn get_pending_until(&self) -> Result<DateTime<Local>, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_pending_until())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn sync_clock(&self, now: DateTime<Local>) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            let before = *attr.get_pending_until();
            attr.sync_clock(now);
            before != *attr.get_pending_until()
        })
    }

    pub fn get_last_synced_time(&self) -> Result<DateTime<Local>, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_last_synced_time())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_priority(&self, priority: i64) -> Result<(), TaskTreeError> {
        let root = self.root()?;
        root.update(|attr| {
            if attr.get_priority() == priority {
                false
            } else {
                attr.set_priority(priority);
                true
            }
        })
    }

    pub fn get_priority(&self) -> Result<i64, TaskTreeError> {
        self.root()?
            .node
            .try_borrow_data()
            .map(|attr| attr.get_priority())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_create_time(&self, create_time: DateTime<Local>) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_create_time() == &create_time {
                false
            } else {
                attr.set_create_time(create_time);
                true
            }
        })
    }

    pub fn get_create_time(&self) -> Result<DateTime<Local>, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_create_time())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_start_time(&self, start_time: DateTime<Local>) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            let before = (*attr.get_start_time(), *attr.get_pending_until());
            attr.set_start_time(start_time);
            before != (*attr.get_start_time(), *attr.get_pending_until())
        })
    }

    /// Sets a movable start time in one persistent mutation.
    ///
    /// A start set through the `始` command is an availability constraint, not an
    /// appointment. Updating both fields together prevents a failed second write from
    /// leaving a task with a new start time that is still fixed.
    pub fn set_flexible_start_time(
        &self,
        start_time: DateTime<Local>,
    ) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            let before = (
                *attr.get_start_time(),
                *attr.get_pending_until(),
                attr.get_fixed_start(),
            );
            attr.set_start_time(start_time);
            attr.set_fixed_start(false);
            before
                != (
                    *attr.get_start_time(),
                    *attr.get_pending_until(),
                    attr.get_fixed_start(),
                )
        })
    }

    pub fn get_start_time(&self) -> Result<DateTime<Local>, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_start_time())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_end_time_opt(
        &self,
        end_time_opt: Option<DateTime<Local>>,
    ) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_end_time_opt() == &end_time_opt {
                false
            } else {
                attr.set_end_time_opt(end_time_opt);
                true
            }
        })
    }

    pub fn get_end_time_opt(&self) -> Result<Option<DateTime<Local>>, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_end_time_opt())
            .map_err(|_| TaskTreeError::Borrow)
    }

    // Someの〆切は未完了の子孫へ伝搬し、子の〆切が親より遅くならない不変条件を維持する。
    // Noneは伝搬更新を行わない。現在のtaskの〆切を解除する場合はunset_deadline_time_optを使う。
    pub fn set_deadline_time_opt(
        &self,
        deadline_time_opt: Option<DateTime<Local>>,
    ) -> Result<(), TaskTreeError> {
        let mut updates = Vec::new();
        self.collect_deadline_updates(deadline_time_opt, None, &mut updates)?;
        self.apply_deadline_updates(updates)
    }

    fn apply_deadline_updates(
        &self,
        updates: Vec<(Node<TaskAttr>, DateTime<Local>)>,
    ) -> Result<(), TaskTreeError> {
        let root = self.root()?;
        root.node
            .try_borrow_data_mut()
            .map_err(|_| TaskTreeError::Borrow)?;
        for (node, _) in &updates {
            node.try_borrow_data_mut()
                .map_err(|_| TaskTreeError::Borrow)?;
        }
        for (node, deadline) in &updates {
            node.try_borrow_data_mut()
                .map_err(|_| TaskTreeError::Borrow)?
                .set_deadline_time_opt(Some(*deadline));
        }
        if !updates.is_empty() {
            root.mark_persistent_mutation()?;
        }
        Ok(())
    }

    fn collect_deadline_updates(
        &self,
        inherited: Option<DateTime<Local>>,
        current_deadline_override: Option<Option<DateTime<Local>>>,
        updates: &mut Vec<(Node<TaskAttr>, DateTime<Local>)>,
    ) -> Result<(), TaskTreeError> {
        let attr = self
            .node
            .try_borrow_data()
            .map_err(|_| TaskTreeError::Borrow)?;
        if *attr.get_status() == Status::Done {
            return Ok(());
        }
        let current = current_deadline_override.unwrap_or(*attr.get_deadline_time_opt());
        drop(attr);
        let Some(inherited) = inherited else {
            return Ok(());
        };
        let effective = current
            .map(|current| current.min(inherited))
            .unwrap_or(inherited);
        if current != Some(effective) {
            updates.push((self.node.clone(), effective));
        }
        for child in self.node.children() {
            Self { node: child }.collect_deadline_updates(Some(effective), None, updates)?;
        }
        Ok(())
    }

    pub fn unset_deadline_time_opt(&self) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_deadline_time_opt().is_none() {
                false
            } else {
                attr.set_deadline_time_opt(None);
                true
            }
        })
    }

    pub(crate) fn replace_deadline_time(
        &self,
        deadline_time: DateTime<Local>,
    ) -> Result<(), TaskTreeError> {
        let mut updates = Vec::new();
        let current_deadline = self
            .node
            .try_borrow_data()
            .map_err(|_| TaskTreeError::Borrow)?
            .get_deadline_time_opt()
            .to_owned();
        if current_deadline != Some(deadline_time) {
            updates.push((self.node.clone(), deadline_time));
        }
        for child in self.node.children() {
            Self { node: child }.collect_deadline_updates(
                Some(deadline_time),
                None,
                &mut updates,
            )?;
        }
        self.apply_deadline_updates(updates)
    }

    pub fn get_deadline_time_opt(&self) -> Result<Option<DateTime<Local>>, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| *attr.get_deadline_time_opt())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_estimated_work_seconds(
        &self,
        estimated_work_seconds: i64,
    ) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_estimated_work_seconds() == estimated_work_seconds {
                false
            } else {
                attr.set_estimated_work_seconds(estimated_work_seconds);
                true
            }
        })
    }

    pub fn get_estimated_work_seconds(&self) -> Result<i64, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_estimated_work_seconds())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_actual_work_seconds(&self, actual_work_seconds: i64) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_actual_work_seconds() == actual_work_seconds {
                false
            } else {
                attr.set_actual_work_seconds(actual_work_seconds);
                true
            }
        })
    }

    pub fn get_repetition_interval_days_opt(&self) -> Result<Option<i64>, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_repetition_interval_days_opt())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn get_inherited_repetition_interval_days_opt(&self) -> Result<Option<i64>, TaskTreeError> {
        let mut current_parent_opt = self.parent()?;

        while let Some(parent) = current_parent_opt {
            if let Some(repetition_interval_days) = parent.get_repetition_interval_days_opt()? {
                return Ok(Some(repetition_interval_days));
            }

            current_parent_opt = parent.parent()?;
        }

        Ok(None)
    }

    pub fn set_repetition_interval_days_opt(
        &self,
        repetition_interval_days_opt: Option<i64>,
    ) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_repetition_interval_days_opt() == repetition_interval_days_opt {
                false
            } else {
                attr.set_repetition_interval_days_opt(repetition_interval_days_opt);
                true
            }
        })
    }

    pub fn get_repetition_anchor(&self) -> Result<RepetitionAnchor, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_repetition_anchor())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_repetition_anchor(
        &self,
        repetition_anchor: RepetitionAnchor,
    ) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_repetition_anchor() == repetition_anchor {
                false
            } else {
                attr.set_repetition_anchor(repetition_anchor);
                true
            }
        })
    }

    pub fn get_days_in_advance(&self) -> Result<i64, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_days_in_advance())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_days_in_advance(&self, days_in_advance: i64) -> Result<(), TaskTreeError> {
        self.update(|attr| {
            if attr.get_days_in_advance() == days_in_advance {
                false
            } else {
                attr.set_days_in_advance(days_in_advance);
                true
            }
        })
    }

    pub fn get_project_category_opt(&self) -> Result<Option<ProjectCategory>, TaskTreeError> {
        self.root()?
            .node
            .try_borrow_data()
            .map(|attr| attr.get_project_category_opt())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn set_project_category_opt(
        &self,
        project_category_opt: Option<ProjectCategory>,
    ) -> Result<(), TaskTreeError> {
        self.root()?.update(|attr| {
            if attr.get_project_category_opt() == project_category_opt {
                false
            } else {
                attr.set_project_category_opt(project_category_opt);
                true
            }
        })
    }

    pub fn get_actual_work_seconds(&self) -> Result<i64, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map(|attr| attr.get_actual_work_seconds())
            .map_err(|_| TaskTreeError::Borrow)
    }

    pub fn make_appointment(
        &self,
        appointment_start_time: DateTime<Local>,
    ) -> Result<(), TaskTreeError> {
        let deadline_time =
            appointment_start_time + Duration::seconds(self.get_estimated_work_seconds()?);

        let root = self.root()?;
        let is_done = self.get_status()? == Status::Done;
        let mut deadline_updates = Vec::new();
        // 完了済みtaskを境界としてdeadline伝搬を止める既存の不変条件を守る。
        if !is_done {
            for child in self.node.children() {
                Self { node: child }.collect_deadline_updates(
                    Some(deadline_time),
                    None,
                    &mut deadline_updates,
                )?;
            }
        }

        // Every borrow is checked before the first write. This makes the appointment
        // fields and descendant deadline propagation a single all-or-nothing mutation.
        root.node
            .try_borrow_data_mut()
            .map_err(|_| TaskTreeError::Borrow)?;
        self.node
            .try_borrow_data_mut()
            .map_err(|_| TaskTreeError::Borrow)?;
        for (node, _) in &deadline_updates {
            node.try_borrow_data_mut()
                .map_err(|_| TaskTreeError::Borrow)?;
        }

        for (node, deadline) in &deadline_updates {
            node.try_borrow_data_mut()
                .map_err(|_| TaskTreeError::Borrow)?
                .set_deadline_time_opt(Some(*deadline));
        }
        let mut attr = self
            .node
            .try_borrow_data_mut()
            .map_err(|_| TaskTreeError::Borrow)?;
        let before = (
            *attr.get_start_time(),
            *attr.get_pending_until(),
            *attr.get_deadline_time_opt(),
            attr.get_fixed_start(),
        );
        // 完了済みtaskは旧約実装と同じく自己deadlineを解除したままにする。
        attr.set_deadline_time_opt((!is_done).then_some(deadline_time));
        attr.set_start_time(appointment_start_time);
        attr.set_fixed_start(true);
        let changed = !deadline_updates.is_empty()
            || before
                != (
                    *attr.get_start_time(),
                    *attr.get_pending_until(),
                    *attr.get_deadline_time_opt(),
                    attr.get_fixed_start(),
                );
        drop(attr);

        if changed {
            root.mark_persistent_mutation()?;
        }
        Ok(())
    }

    pub fn num_children(&self) -> Result<usize, TaskTreeError> {
        self.node
            .try_borrow_data()
            .map_err(|_| TaskTreeError::Borrow)?;
        Ok(self.node.num_children())
    }

    pub fn has_undone_children(&self) -> Result<bool, TaskTreeError> {
        for child in self.get_children()? {
            if child.get_status()? != Status::Done {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn tree_debug_pretty_print(&self) -> Result<String, TaskTreeError> {
        self.get_attr()?;
        Ok(format!("{:?}", self.node.tree().debug_pretty_print()))
    }

    pub fn get_by_id(&self, id: Uuid) -> Result<Option<TaskHandle>, TaskTreeError> {
        let node_opt = Self::get_by_id_private(&self.node, id)?;

        Ok(node_opt.map(|node| Self { node }))
    }

    fn get_by_id_private(
        node: &Node<TaskAttr>,
        id: Uuid,
    ) -> Result<Option<Node<TaskAttr>>, TaskTreeError> {
        // ベースケース
        if node
            .try_borrow_data()
            .map_err(|_| TaskTreeError::Borrow)?
            .get_id()
            == &id
        {
            return Ok(Some(node.clone()));
        }

        // 子あり
        for child_node in node.children() {
            let child_task_found_opt = Self::get_by_id_private(&child_node, id)?;
            if child_task_found_opt.is_some() {
                return Ok(child_task_found_opt);
            }
        }

        Ok(None)
    }

    pub fn all_sibling_tasks_are_all_done(&self) -> Result<bool, TaskTreeError> {
        let mut ans = true;

        for sibling_node in self.node.siblings() {
            if sibling_node.ptr_eq(&self.node) {
                continue;
            }
            if sibling_node
                .try_borrow_data()
                .map_err(|_| TaskTreeError::Borrow)?
                .get_status()
                != &Status::Done
            {
                ans = false;
                break;
            }
        }

        Ok(ans)
    }

    // 親のタスクを考慮せずに、そのタスク単体で見た時に最速で着手できる時刻
    pub fn first_available_time(&self) -> Result<DateTime<Local>, TaskTreeError> {
        let dt_cand = if self.get_orig_status()? == Status::Pending {
            vec![self.get_start_time()?, self.get_pending_until()?]
        } else {
            vec![self.get_start_time()?]
        };

        // 1要素以上ありNoneになり得ないのでunwrap()してよい
        Ok(*dt_cand.iter().max().unwrap())
    }

    // 親を辿って、Todoのタスクを全て返す
    // タプルの1つ目は最速でTodo化するタイミング
    // ただし、〆切を守れるように、pending_untilよりも〆切を優先する
    pub fn list_all_parent_tasks_with_first_available_time(
        &self,
    ) -> Result<Vec<(DateTime<Local>, TaskHandle)>, TaskTreeError> {
        let mut ans: Vec<(DateTime<Local>, TaskHandle)> = vec![];
        let mut child_task_first_available_time: DateTime<Local> =
            DateTime::<Local>::MIN_UTC.into();

        // Phase1 子→親に辿って仮のfirst_available_timeを決定する
        //   max(子の最速着手時間 + 子の見積もり時間, 親のstart_time) = 親の最速着手時間

        // Phase2 〆切に対してオーバーしている時間を計算し、逆に親→子の順に時間を修正する

        // ここからPhase1: 子→親に辿って仮のfirst_available_timeを決定する
        let mut task_opt = Some(self.clone());
        while let Some(task) = task_opt {
            let first_available_time = task.first_available_time()?;
            let task_first_available_time =
                max(child_task_first_available_time, first_available_time);

            child_task_first_available_time = task_first_available_time
                + Duration::seconds(max(
                    0,
                    task.get_estimated_work_seconds()? - task.get_actual_work_seconds()?,
                ));

            let tpl = (task_first_available_time, task.clone());
            ans.push(tpl);

            // 再代入
            task_opt = task.parent()?;
        }

        // ここからPhase2: 〆切に対してオーバーしている時間を計算し、逆に親→子の順に時間を修正する

        // 親のfirst_available_timeは〆切を考慮済みなので、それを子でも考慮するために一時保存する
        // 別のメソッドでset_deadline_time()する際に各タスクの見積もりまで考慮して設定するのは、見積もりを変えるたびにdeadline_timeを設定し直さなければいけないため複雑になる
        // そのため、このメソッド内で行う
        let mut parent_required_start_time_for_deadline = DateTime::<Local>::MAX_UTC.into();

        for (rough_first_available_time, task) in ans.iter_mut().rev() {
            // 親から引き継いだ〆切か、自分の〆切のうち早いほう
            let mut required_start_time_for_deadline = parent_required_start_time_for_deadline;
            if let Some(deadline_time) = task.get_deadline_time_opt()? {
                if deadline_time < required_start_time_for_deadline {
                    required_start_time_for_deadline = deadline_time;
                }
            }

            let lateness_duration = *rough_first_available_time
                + Duration::seconds(max(
                    0,
                    task.get_estimated_work_seconds()? - task.get_actual_work_seconds()?,
                ))
                - required_start_time_for_deadline;

            if lateness_duration >= Duration::seconds(0) {
                *rough_first_available_time -= lateness_duration;
                parent_required_start_time_for_deadline = *rough_first_available_time;
            }
        }

        let mut child_task_finish_time: DateTime<Local> = DateTime::<Local>::MIN_UTC.into();
        for (first_available_time, task) in ans.iter_mut() {
            let earliest_time_for_task = min(*first_available_time, task.first_available_time()?);
            *first_available_time = max(earliest_time_for_task, child_task_finish_time);
            child_task_finish_time = *first_available_time
                + Duration::seconds(max(
                    0,
                    task.get_estimated_work_seconds()? - task.get_actual_work_seconds()?,
                ));
        }

        Ok(ans)
    }
}

#[cfg(test)]
include!("task_tests.rs");
