//! `runCommand` — the one tool that is not a filesystem operation.

use crate::domain::command_exec::{CommandError, CommandRequest};
use crate::domain::llm::LlmToolDefinition;
use crate::domain::tools::{ToolDeps, ToolError, ToolResult, ToolScope};
use crate::infra::process_runner;

use super::super::resolve::resolve_existing;

pub fn run_command(
    scope: &ToolScope,
    request: &CommandRequest,
    deps: &ToolDeps,
) -> Result<ToolResult, ToolError> {
    if request.command.trim().is_empty() {
        return Err(ToolError::InvalidArguments {
            tool: "runCommand".to_string(),
            reason: "command must not be empty".to_string(),
        });
    }

    // The one path this tool has, resolved like every other. It keeps `cd`
    // honest; it does not contain the command, and nothing here should be read
    // as claiming otherwise — see `domain::command_exec`.
    let cwd = match &request.cwd {
        Some(path) if !path.is_empty() && path != "." => resolve_existing(scope, path)?,
        _ => scope.root().to_path_buf(),
    };

    process_runner::run(&deps.shell, request, &cwd, deps.output.as_ref())
        .map(ToolResult::CommandRan)
        .map_err(|e| match e {
            CommandError::Cwd(message) => ToolError::InvalidArguments {
                tool: "runCommand".to_string(),
                reason: message,
            },
            other => ToolError::Command(other.to_string()),
        })
}

/// What the model is told `runCommand` is for.
pub(super) fn definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "runCommand".to_string(),
        description: "Run a shell command in the workspace — build, test, lint, inspect. This is how you check your own work: after changing code, run the tests rather than claiming they pass. The exit code, stdout and stderr all come back; a non-zero exit is an ordinary answer, not a failure of the call. Output is streamed as it is produced and cut in the middle if it is very long, keeping both the first lines and the last. The command runs to completion or is killed at its timeout, together with everything it started — nothing survives the call, so do not use this to start a server you expect to keep running."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command line, as you would type it. Pipes, redirection and `&&` work. Prefer the project's own tooling (its test runner, its formatter) over ad-hoc shell."
                },
                "cwd": {
                    "type": ["string", "null"],
                    "description": "Directory to run in, relative to the workspace root. Omit for the root itself."
                },
                "timeoutSeconds": {
                    "type": ["integer", "null"],
                    "minimum": 0,
                    "description": "How long to allow, up to 600. Omit for 120. A command still running then is killed, and the result says so — raise this for a slow build rather than re-running it in pieces."
                }
            },
            "required": ["command"]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tools::{ReadFiles, ToolCall};
    use crate::services::ai_tools::tools::execute_tool;
    use crate::testing::temp_dir;
    use std::path::Path;

    fn ask(command: &str, cwd: Option<&str>) -> ToolCall {
        ToolCall::RunCommand(CommandRequest {
            command: command.to_string(),
            cwd: cwd.map(|c| c.to_string()),
            timeout_seconds: Some(10),
        })
    }

    fn run(root: &Path, call: ToolCall) -> Result<ToolResult, ToolError> {
        let scope = ToolScope::new(root).expect("scope");
        execute_tool(
            &scope,
            &call,
            &mut ReadFiles::default(),
            &mut Vec::new(),
            &ToolDeps::default(),
        )
    }

    fn output(result: ToolResult) -> crate::domain::command_exec::CommandOutput {
        match result {
            ToolResult::CommandRan(output) => output,
            other => panic!("expected a command result, got {other:?}"),
        }
    }

    #[test]
    fn a_command_runs_in_the_workspace_root_by_default() {
        let root = temp_dir("cmd-root");
        std::fs::write(root.join("here.txt"), "x").unwrap();

        let out = output(run(&root, ask("ls", None)).expect("runs"));

        assert!(out.stdout.contains("here.txt"), "{}", out.stdout);
        assert_eq!(out.exit_code, Some(0));
    }

    #[test]
    fn a_cwd_is_resolved_inside_the_workspace() {
        let root = temp_dir("cmd-cwd");
        std::fs::create_dir(root.join("crate")).unwrap();
        std::fs::write(root.join("crate/inner.txt"), "x").unwrap();

        let out = output(run(&root, ask("ls", Some("crate"))).expect("runs"));

        assert!(out.stdout.contains("inner.txt"), "{}", out.stdout);
    }

    /// The `cwd` goes through the same resolution as every other path, so it
    /// cannot point outside — which is a convenience, not containment: the
    /// command itself is free to name any path it likes.
    #[test]
    fn a_cwd_outside_the_workspace_is_refused() {
        let root = temp_dir("cmd-escape");
        let err = run(&root, ask("ls", Some("../.."))).expect_err("refused");
        assert!(matches!(err, ToolError::PathEscape(_)), "{err}");
    }

    #[test]
    fn a_cwd_that_does_not_exist_says_so() {
        let root = temp_dir("cmd-missing-cwd");
        let err = run(&root, ask("ls", Some("nope"))).expect_err("refused");
        assert!(matches!(err, ToolError::NotFound(_)), "{err}");
    }

    /// A model that sends an empty command has made a mistake it can fix;
    /// handing that to a shell produces a silent success instead.
    #[test]
    fn an_empty_command_is_refused() {
        let root = temp_dir("cmd-empty");
        let err = run(&root, ask("   ", None)).expect_err("refused");
        assert!(matches!(err, ToolError::InvalidArguments { .. }), "{err}");
    }

    /// The point of the tool: the model finds out that the thing it changed
    /// does not work.
    #[test]
    fn a_failing_command_comes_back_as_a_result_with_its_output() {
        let root = temp_dir("cmd-failing");

        let out = output(
            run(&root, ask("echo 'assertion failed' >&2; exit 1", None)).expect("runs"),
        );

        assert_eq!(out.exit_code, Some(1));
        assert!(out.stderr.contains("assertion failed"), "{}", out.stderr);
        assert!(!out.succeeded());
    }

    #[test]
    fn the_wire_shape_is_stable_and_counts_as_risky() {
        let call = ask("cargo test", None);
        let json = serde_json::to_value(&call).unwrap();
        assert_eq!(json["tool"], "runCommand");
        assert_eq!(json["args"]["command"], "cargo test");
        assert!(call.is_risky(), "a command line is never safe by its name alone");
    }
}
