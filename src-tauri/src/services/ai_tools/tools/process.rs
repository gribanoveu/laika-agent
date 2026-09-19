//! `readOutput` and `stopProcess` — the other two thirds of a background
//! process; `runCommand` with `background` is the first.

use crate::domain::background::BackgroundProcesses;
use crate::domain::llm::LlmToolDefinition;
use crate::domain::tools::{ProcessArgs, ToolDeps, ToolError, ToolResult};

pub fn read_output(args: &ProcessArgs, deps: &ToolDeps) -> Result<ToolResult, ToolError> {
    let (processes, id) = target("readOutput", args, deps)?;
    Ok(ToolResult::ProcessOutput(processes.read(id)?))
}

pub fn stop_process(args: &ProcessArgs, deps: &ToolDeps) -> Result<ToolResult, ToolError> {
    let (processes, id) = target("stopProcess", args, deps)?;
    Ok(ToolResult::ProcessStopped(processes.stop(id)?))
}

fn target<'a>(
    tool: &str,
    args: &ProcessArgs,
    deps: &'a ToolDeps,
) -> Result<(&'a dyn BackgroundProcesses, u32), ToolError> {
    let id = args.id.ok_or_else(|| ToolError::InvalidArguments {
        tool: tool.to_string(),
        reason: "id is required: the number runCommand returned when it started the process".to_string(),
    })?;
    let processes = deps.processes.as_deref().ok_or_else(unavailable)?;
    Ok((processes, id))
}

pub(super) fn unavailable() -> ToolError {
    ToolError::Command("background processes are not available here".to_string())
}

pub(super) fn read_definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "readOutput".to_string(),
        description: "Read what a background process (started by runCommand with background: true) has written since you last read it — stdout and stderr together, in order — and whether it is still running or how it ended. Each call returns only new output. Check it after starting a server to see it came up, and before relying on it."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "integer", "description": "The process number runCommand returned." }
            },
            "required": ["id"]
        }),
    }
}

pub(super) fn stop_definition() -> LlmToolDefinition {
    LlmToolDefinition {
        name: "stopProcess".to_string(),
        description: "Stop a background process and everything it started. Stop what you no longer need: at most five run at once."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "integer", "description": "The process number runCommand returned." }
            },
            "required": ["id"]
        }),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::domain::background::{BackgroundError, ProcessState};
    use crate::domain::command_exec::CommandRequest;
    use crate::domain::tools::{ReadFiles, ToolCall, ToolScope};
    use crate::infra::background::Processes;
    use crate::services::ai_tools::tools::execute_tool;
    use crate::testing::temp_dir;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn run(root: &std::path::Path, deps: &ToolDeps, call: ToolCall) -> Result<ToolResult, ToolError> {
        execute_tool(&ToolScope::new(root).unwrap(), &call, &mut ReadFiles::default(), &mut Vec::new(), deps)
    }

    fn background(command: &str, cwd: Option<&str>) -> ToolCall {
        ToolCall::RunCommand(CommandRequest {
            command: command.into(),
            cwd: cwd.map(Into::into),
            timeout_seconds: Some(1),
            background: Some(true),
        })
    }

    /// Start, read, stop — the three tools over one process, which outlives
    /// its one-second timeout because a background process has none.
    #[test]
    fn a_process_is_started_read_and_stopped_through_the_tools() {
        let root = temp_dir("tool-bg");
        std::fs::create_dir(root.join("app")).unwrap();
        let deps = ToolDeps { processes: Some(Arc::new(Processes::default())), ..ToolDeps::default() };

        let ToolResult::ProcessStarted(info) = run(&root, &deps, background("pwd; echo ready; sleep 30", Some("app"))).unwrap() else {
            panic!("expected a start");
        };
        assert_eq!((info.id, info.cwd.as_str(), info.state), (1, "app", ProcessState::Running));

        let read = ToolCall::ReadOutput(ProcessArgs { id: Some(1) });
        let mut seen = String::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !seen.contains("ready") {
            assert!(Instant::now() < deadline, "no output: {seen:?}");
            let ToolResult::ProcessOutput(out) = run(&root, &deps, read.clone()).unwrap() else { panic!() };
            seen.push_str(&out.output);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(seen.contains("/app\n"), "{seen}");
        std::thread::sleep(Duration::from_millis(1200));
        let ToolResult::ProcessOutput(out) = run(&root, &deps, read).unwrap() else { panic!() };
        assert!(out.process.running(), "the timeout did not apply");

        let ToolResult::ProcessStopped(stopped) =
            run(&root, &deps, ToolCall::StopProcess(ProcessArgs { id: Some(1) })).unwrap()
        else {
            panic!()
        };
        assert_eq!(stopped.state, ProcessState::Stopped);
    }

    #[test]
    fn what_the_model_gets_wrong_is_said_to_it() {
        let root = temp_dir("tool-bg-wrong");
        let deps = ToolDeps { processes: Some(Arc::new(Processes::default())), ..ToolDeps::default() };
        let missing = run(&root, &deps, ToolCall::ReadOutput(ProcessArgs { id: None })).unwrap_err();
        assert!(missing.to_string().contains("id is required"), "{missing}");
        let unknown = run(&root, &deps, ToolCall::StopProcess(ProcessArgs { id: Some(9) })).unwrap_err();
        assert!(matches!(unknown, ToolError::Background(BackgroundError::NotFound(9))));
        let escape = run(&root, &deps, background("true", Some("../.."))).unwrap_err();
        assert!(matches!(escape, ToolError::PathEscape(_)), "{escape}");

        let none = ToolDeps::default();
        for call in [background("true", None), ToolCall::ReadOutput(ProcessArgs { id: Some(1) })] {
            let err = run(&root, &none, call).unwrap_err();
            assert!(err.to_string().contains("not available"), "{err}");
        }
    }
}
