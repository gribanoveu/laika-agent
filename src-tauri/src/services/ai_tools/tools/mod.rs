//! The tool boundary: one dispatcher, one place a call becomes an effect.
//!
//! Every call routes through [`execute_tool`], which is why permission
//! checking, logging and redaction each have exactly one site rather than one
//! per tool. Adding a tool is a variant in `ToolCall`, a variant in
//! `ToolResult`, a module here, and an arm below.
//!
//! `reads` is the caller's record of what the agent has looked at. It lives
//! here rather than inside the executor for the same reason the turn's todo
//! list does: the executor stays stateless, and the state survives an approval
//! pause. Alfa Atlas also threads dependencies through here; those arrive with
//! the tool that needs them.

use crate::domain::tools::{ReadFiles, ToolCall, ToolError, ToolResult, ToolScope};

pub mod edit_file;
pub mod grep;
pub mod list_files;
pub mod write_file;
pub mod read_file;

pub fn execute_tool(
    scope: &ToolScope,
    call: &ToolCall,
    reads: &mut ReadFiles,
) -> Result<ToolResult, ToolError> {
    match call {
        ToolCall::ReadFile(args) => read_file::read_file(scope, args, reads),
        ToolCall::Grep(args) => grep::grep(scope, args),
        ToolCall::ListFiles(args) => list_files::list_files(scope, args),
        ToolCall::WriteFile(args) => write_file::write_file(scope, args, reads),
        ToolCall::EditFile(args) => edit_file::edit_file(scope, args, reads),
    }
}
