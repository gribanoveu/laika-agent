//! The tool boundary: one dispatcher, one place a call becomes an effect.
//!
//! Every call routes through [`execute_tool`], which is why permission
//! checking, logging and redaction each have exactly one site rather than one
//! per tool. Adding a tool is a variant in `ToolCall`, a variant in
//! `ToolResult`, a module here, and an arm below.
//!
//! Note what this signature does *not* take. Alfa Atlas threads dependencies
//! and the turn's todo list through here; neither has a caller yet, and both
//! arrive with the tool that needs them.

use crate::domain::tools::{ToolCall, ToolError, ToolResult, ToolScope};

pub mod grep;
pub mod list_files;
pub mod read_file;

pub fn execute_tool(scope: &ToolScope, call: &ToolCall) -> Result<ToolResult, ToolError> {
    match call {
        ToolCall::ReadFile(args) => read_file::read_file(scope, args),
        ToolCall::Grep(args) => grep::grep(scope, args),
        ToolCall::ListFiles(args) => list_files::list_files(scope, args),
    }
}
