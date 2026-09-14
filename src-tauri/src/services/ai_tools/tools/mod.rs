//! The tool boundary: one dispatcher, one place a call becomes an effect.
//!
//! Every call routes through [`execute_tool`], which is why permission
//! checking, logging and redaction each have exactly one site rather than one
//! per tool. Adding a tool is a variant in `ToolCall`, a variant in
//! `ToolResult`, a module here, and an arm below.
//!
//! `reads` and `todos` are the caller's, not this module's. It lives
//! here rather than inside the executor for the same reason the turn's todo
//! list does: the executor stays stateless, and the state survives an approval
//! pause. Alfa Atlas also threads dependencies through here; those arrive with
//! the tool that needs them.

use crate::domain::tools::{ReadFiles, Task, ToolCall, ToolError, ToolResult, ToolScope};

pub mod create_directory;
pub mod delete_directory;
pub mod delete_file;
pub mod edit_file;
pub mod grep;
pub mod list_files;
pub mod move_path;
pub mod todo;
pub mod write_file;
pub mod read_file;

pub fn execute_tool(
    scope: &ToolScope,
    call: &ToolCall,
    reads: &mut ReadFiles,
    todos: &mut Vec<Task>,
) -> Result<ToolResult, ToolError> {
    match call {
        ToolCall::ReadFile(args) => read_file::read_file(scope, args, reads),
        ToolCall::Grep(args) => grep::grep(scope, args),
        ToolCall::ListFiles(args) => list_files::list_files(scope, args),
        ToolCall::WriteFile(args) => write_file::write_file(scope, args, reads),
        ToolCall::EditFile(args) => edit_file::edit_file(scope, args, reads),
        ToolCall::CreateDirectory(args) => create_directory::create_directory(scope, args),
        ToolCall::DeleteFile(args) => delete_file::delete_file(scope, args, reads),
        ToolCall::DeleteDirectory(args) => delete_directory::delete_directory(scope, args),
        ToolCall::Move(args) => move_path::move_path(scope, args),
        ToolCall::Todo(args) => todo::todo(todos, args),
    }
}
