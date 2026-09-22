//! `todo` — the model's own checklist for a multi-step turn.
//!
//! Stateless by construction: the list is handed in and handed back, never
//! stored here. It belongs to the turn, the same arrangement the read registry
//! uses, and for the same reason — it has to survive an approval pause.
//!
//! There is no read operation on purpose. The current checklist is part of the
//! model's context already; a tool call to look at it would be a wasted round.

use crate::domain::llm::LlmToolDefinition;
use crate::domain::tools::{Task, TodoArgs, TodoStatus, ToolError, ToolResult};

/// A checklist longer than this is not a plan, it is a different conversation.
/// A write that would exceed it fails outright rather than silently truncating,
/// so the model decides what to drop or split.
const MAX_TASKS: usize = 20;

pub fn todo(todos: &mut Vec<Task>, args: &TodoArgs) -> Result<ToolResult, ToolError> {
    let updated = match args {
        TodoArgs::Write { tasks } => write(todos, tasks)?,
        TodoArgs::Update { id, status, note } => {
            update(todos, id.as_deref(), (*status).into(), note.as_deref())?
        }
    };
    *todos = updated;
    Ok(ToolResult::Todo {
        tasks: todos.clone(),
    })
}

/// Appends — unless nothing on the list is open any more. A finished list is
/// done with, and the next write starts a new one: kept, the closed tasks of
/// earlier requests filled the list until no new checklist fit, with no way
/// to clear them.
fn write(todos: &[Task], titles: &[String]) -> Result<Vec<Task>, ToolError> {
    let finished = todos.iter().all(|t| matches!(t.status, TodoStatus::Completed | TodoStatus::Cancelled));
    let kept: &[Task] = if finished { &[] } else { todos };
    if kept.len() + titles.len() > MAX_TASKS {
        return Err(ToolError::TooManyTasks {
            current: kept.len(),
            adding: titles.len(),
            max: MAX_TASKS,
        });
    }
    // Numbered on from the highest id there was, the replaced list's too, so
    // an id is never reused: a stale one would silently retarget an update.
    let last = todos.iter().filter_map(|t| t.id.strip_prefix('t')?.parse::<usize>().ok()).max().unwrap_or(0);
    let mut updated = kept.to_vec();
    for (offset, title) in titles.iter().enumerate() {
        updated.push(Task {
            id: format!("t{}", last + offset + 1),
            title: title.clone(),
            status: TodoStatus::Pending,
            note: None,
        });
    }
    Ok(advance(updated))
}

fn update(
    todos: &[Task],
    id: Option<&str>,
    status: TodoStatus,
    note: Option<&str>,
) -> Result<Vec<Task>, ToolError> {
    let ids = || Some(todos.iter().map(|t| t.id.clone()).collect::<Vec<_>>());

    let target = match id {
        Some(id) => id.to_string(),
        None => todos
            .iter()
            .find(|t| t.status == TodoStatus::InProgress)
            .map(|t| t.id.clone())
            .ok_or_else(|| ToolError::TaskNotFound {
                id: String::new(),
                available: ids(),
            })?,
    };

    let mut updated = todos.to_vec();
    let task = updated
        .iter_mut()
        .find(|t| t.id == target)
        .ok_or_else(|| ToolError::TaskNotFound {
            id: target.clone(),
            available: ids(),
        })?;
    task.status = status;
    if let Some(note) = note {
        task.note = Some(note.to_string());
    }
    Ok(advance(updated))
}

/// At most one task is ever in progress, and the next pending one is promoted
/// the moment none is.
///
/// Run after both operations: a first write leaves a list with nothing active,
/// and completing the current task always does. Doing it here rather than in
/// the model's hands is the other half of `TodoUpdateStatus` — between them,
/// the order of work is the runtime's to decide.
fn advance(mut tasks: Vec<Task>) -> Vec<Task> {
    if tasks.iter().any(|t| t.status == TodoStatus::InProgress) {
        return tasks;
    }
    if let Some(next) = tasks.iter_mut().find(|t| t.status == TodoStatus::Pending) {
        next.status = TodoStatus::InProgress;
    }
    tasks
}

/// What the model is told `todo` is for.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "todo".to_string(),
        description: "Keep the checklist for a request that takes several steps (three or more). One tool, two operations chosen with `op`. `write` appends new task titles to the end of the list; once every task on it is completed or cancelled, the next `write` starts a new list instead. The runtime assigns ids and activates the first task when nothing is active. `update` changes one task to `completed` or `cancelled`; those are the only statuses you may set, and the runtime activates the next task by itself. Omit `id` to mean the task you are on, which is what almost every update means and cannot name the wrong one. There is no read operation — the current list comes back from every call. Do not use it for a one- or two-step request."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "op": {
                    "type": "string",
                    "enum": [
                        "write",
                        "update"
                    ],
                    "description": "\\\"write\\\" to append tasks, \\\"update\\\" to change one."
                },
                "tasks": {
                    "type": [
                        "array",
                        "null"
                    ],
                    "items": {
                        "type": "string"
                    },
                    "description": "Only for op \\\"write\\\": task titles to append, each a short imperative phrase."
                },
                "id": {
                    "type": [
                        "string",
                        "null"
                    ],
                    "description": "Only for op \\\"update\\\": which task to change, exactly as the list spells it. Omit it to change the active task."
                },
                "status": {
                    "type": [
                        "string",
                        "null"
                    ],
                    "enum": [
                        "completed",
                        "cancelled",
                        null
                    ],
                    "description": "Only for op \\\"update\\\". Use \\\"cancelled\\\" when a task turned out unnecessary, with a note saying why."
                },
                "note": {
                    "type": [
                        "string",
                        "null"
                    ],
                    "description": "Only for op \\\"update\\\": a short result for a completed task, or the reason for a cancelled one."
                }
            },
            "required": [
                "op"
            ]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::{ToolCall, TodoUpdateStatus};

    fn tasks(result: Result<ToolResult, ToolError>) -> Vec<Task> {
        match result.expect("the call succeeds") {
            ToolResult::Todo { tasks } => tasks,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn titles(names: &[&str]) -> TodoArgs {
        TodoArgs::Write {
            tasks: names.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn done(id: Option<&str>) -> TodoArgs {
        TodoArgs::Update {
            id: id.map(str::to_string),
            status: TodoUpdateStatus::Completed,
            note: None,
        }
    }

    fn state(tasks: &[Task]) -> Vec<(&str, TodoStatus)> {
        tasks.iter().map(|t| (t.id.as_str(), t.status)).collect()
    }

    #[test]
    fn a_first_write_activates_the_first_task_and_no_other() {
        let mut todos = Vec::new();

        let out = tasks(todo(&mut todos, &titles(&["design it", "build it", "ship it"])));

        assert_eq!(
            state(&out),
            [
                ("t1", TodoStatus::InProgress),
                ("t2", TodoStatus::Pending),
                ("t3", TodoStatus::Pending),
            ]
        );
    }

    /// The list is append-only. A second write must not reset progress on what
    /// is already there.
    #[test]
    fn a_later_write_appends_without_disturbing_the_active_task() {
        let mut todos = Vec::new();
        todo(&mut todos, &titles(&["first"])).unwrap();

        let out = tasks(todo(&mut todos, &titles(&["second"])));

        assert_eq!(
            state(&out),
            [("t1", TodoStatus::InProgress), ("t2", TodoStatus::Pending)]
        );
    }

    /// Ids are sequential over the whole list, so one is never reused after a
    /// task is closed — a reused id would silently retarget a later update.
    /// Closed tasks of an earlier request must not crowd out the next
    /// checklist; open ones are still work and stay.
    #[test]
    fn a_write_after_a_finished_list_starts_a_new_one() {
        let mut todos = Vec::new();
        todo(&mut todos, &titles(&["a", "b"])).unwrap();
        todo(&mut todos, &done(None)).unwrap();

        let open = tasks(todo(&mut todos, &titles(&["c"])));
        assert_eq!(open.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["t1", "t2", "t3"], "b is open, so it appends");

        todo(&mut todos, &done(None)).unwrap();
        // Cancelled is as closed as completed.
        todo(&mut todos, &TodoArgs::Update { id: None, status: TodoUpdateStatus::Cancelled, note: Some("not needed".into()) })
            .unwrap();
        let many: Vec<String> = (0..MAX_TASKS).map(|i| format!("task {i}")).collect();
        let fresh = tasks(todo(&mut todos, &TodoArgs::Write { tasks: many.clone() }));
        assert_eq!(fresh.len(), MAX_TASKS, "the closed tasks gave up their places");
        assert_eq!((fresh[0].id.as_str(), fresh[0].status), ("t4", TodoStatus::InProgress));
        assert_eq!(fresh.last().unwrap().id, format!("t{}", 3 + MAX_TASKS), "numbered on, never reused");

        // A third list goes on from the highest id, not from the list's length.
        for _ in 0..MAX_TASKS {
            todo(&mut todos, &done(None)).unwrap();
        }
        let third = tasks(todo(&mut todos, &titles(&["again"])));
        assert_eq!(third.iter().map(|t| t.id.clone()).collect::<Vec<_>>(), [format!("t{}", 4 + MAX_TASKS)]);
    }

    #[test]
    fn ids_are_never_reused() {
        let mut todos = Vec::new();
        todo(&mut todos, &titles(&["a", "b"])).unwrap();
        todo(&mut todos, &done(Some("t1"))).unwrap();
        todo(&mut todos, &done(Some("t2"))).unwrap();

        let out = tasks(todo(&mut todos, &titles(&["c"])));

        assert_eq!(out.last().unwrap().id, "t3");
    }

    /// Completing the current task promotes the next one immediately — the
    /// runtime's job, never the model's.
    #[test]
    fn completing_the_active_task_promotes_the_next() {
        let mut todos = Vec::new();
        todo(&mut todos, &titles(&["a", "b"])).unwrap();

        let out = tasks(todo(&mut todos, &done(None)));

        assert_eq!(
            state(&out),
            [("t1", TodoStatus::Completed), ("t2", TodoStatus::InProgress)]
        );
    }

    /// The case the default was added for: naming an id is a step that goes
    /// wrong silently, so "the one I am on" needs no id.
    #[test]
    fn an_update_without_an_id_means_the_active_task() {
        let mut todos = Vec::new();
        todo(&mut todos, &titles(&["a", "b"])).unwrap();
        todo(&mut todos, &done(None)).unwrap();

        let out = tasks(todo(&mut todos, &done(None)));

        assert_eq!(
            state(&out),
            [("t1", TodoStatus::Completed), ("t2", TodoStatus::Completed)]
        );
    }

    #[test]
    fn a_cancelled_task_also_advances_the_list_and_keeps_its_reason() {
        let mut todos = Vec::new();
        todo(&mut todos, &titles(&["a", "b"])).unwrap();

        let out = tasks(todo(
            &mut todos,
            &TodoArgs::Update {
                id: None,
                status: TodoUpdateStatus::Cancelled,
                note: Some("not needed after all".into()),
            },
        ));

        assert_eq!(out[0].status, TodoStatus::Cancelled);
        assert_eq!(out[0].note.as_deref(), Some("not needed after all"));
        assert_eq!(out[1].status, TodoStatus::InProgress);
    }

    /// Nothing is left active once the work is done — a promoted task that
    /// does not exist would be an invented one.
    #[test]
    fn a_finished_list_has_nothing_in_progress() {
        let mut todos = Vec::new();
        todo(&mut todos, &titles(&["only"])).unwrap();

        let out = tasks(todo(&mut todos, &done(None)));

        assert!(out.iter().all(|t| t.status != TodoStatus::InProgress));
    }

    #[test]
    fn an_unknown_id_lists_the_real_ones() {
        let mut todos = Vec::new();
        todo(&mut todos, &titles(&["a"])).unwrap();

        let err = todo(&mut todos, &done(Some("t9"))).expect_err("no such task");

        assert!(matches!(err, ToolError::TaskNotFound { .. }));
        assert!(err.to_string().contains("t1"), "{err}");
    }

    /// The failure that produced the message: completing a task on a checklist
    /// that was never created.
    #[test]
    fn updating_an_empty_checklist_says_how_to_start_one() {
        let mut todos = Vec::new();

        let err = todo(&mut todos, &done(None)).expect_err("nothing to update");

        assert!(err.to_string().contains("checklist is empty"), "{err}");
    }

    /// Refused outright rather than truncated, so the model chooses what to
    /// drop instead of finding out later that it was cut.
    #[test]
    fn exceeding_the_cap_is_refused_and_changes_nothing() {
        let mut todos = Vec::new();
        let many: Vec<String> = (0..MAX_TASKS).map(|i| format!("task {i}")).collect();
        todo(&mut todos, &TodoArgs::Write { tasks: many }).unwrap();

        let err = todo(&mut todos, &titles(&["one too many"])).expect_err("over the cap");

        assert!(matches!(err, ToolError::TooManyTasks { max: MAX_TASKS, .. }));
        assert_eq!(todos.len(), MAX_TASKS, "the list is unchanged");
    }

    /// A failed call must not half-apply: the caller's list is only replaced
    /// once the whole operation succeeded.
    #[test]
    fn a_failed_update_leaves_the_callers_list_alone() {
        let mut todos = Vec::new();
        todo(&mut todos, &titles(&["a"])).unwrap();
        let before = todos.clone();

        todo(&mut todos, &done(Some("t9"))).expect_err("no such task");

        assert_eq!(todos, before);
    }

    /// One wire tool with an `op` discriminator — the shape the model is given,
    /// so nothing has to fan it out into two calls.
    #[test]
    fn the_call_wire_shape_is_stable() {
        let write: ToolCall = serde_json::from_str(
            r#"{"tool": "todo", "args": {"op": "write", "tasks": ["do a thing"]}}"#,
        )
        .expect("parses");
        assert_eq!(write, ToolCall::Todo(titles(&["do a thing"])));
        assert!(!write.is_risky(), "a checklist touches no files");

        let update: ToolCall = serde_json::from_str(
            r#"{"tool": "todo", "args": {"op": "update", "status": "completed"}}"#,
        )
        .expect("parses without an id");
        assert_eq!(update, ToolCall::Todo(done(None)));
    }

    /// `pending` and `in_progress` are not spellings the model may send — the
    /// schema, not the prompt, is what keeps the order of work out of its hands.
    #[test]
    fn the_model_cannot_set_a_task_in_progress() {
        for status in ["pending", "inProgress", "in_progress"] {
            let json = format!(r#"{{"op": "update", "status": "{status}"}}"#);
            assert!(
                serde_json::from_str::<TodoArgs>(&json).is_err(),
                "{status} was accepted"
            );
        }
    }
}
