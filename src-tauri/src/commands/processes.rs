//! The Terminal tab: the background processes, and the user's Stop.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::domain::background::{BackgroundProcesses, ProcessInfo};
use crate::infra::background::Processes;

/// How much of each process's output the tab shows: its last screenfuls.
const TAIL_BYTES: usize = 8 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessView {
    #[serde(flatten)]
    pub info: ProcessInfo,
    pub tail: String,
}

fn views(processes: &Processes) -> Vec<ProcessView> {
    // Newest first: the one just started is the one being watched.
    processes
        .list()
        .into_iter()
        .rev()
        .map(|info| ProcessView { tail: processes.tail(info.id, TAIL_BYTES).unwrap_or_default(), info })
        .collect()
}

/// Asked for once a second while the tab is open — cheap, and simpler than
/// an event per line of a chatty server.
#[tauri::command]
pub fn processes_list(processes: State<'_, Arc<Processes>>) -> Vec<ProcessView> {
    views(&processes)
}

/// The model is told at its next round that the user stopped it.
#[tauri::command]
pub fn process_stop(id: u32, processes: State<'_, Arc<Processes>>) -> Result<Vec<ProcessView>, String> {
    stop(&processes, id)
}

fn stop(processes: &Processes, id: u32) -> Result<Vec<ProcessView>, String> {
    processes.stop_by_user(id).map_err(|e| e.to_string())?;
    Ok(views(processes))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::domain::command_exec::Shell;

    #[test]
    fn the_tab_lists_newest_first_with_what_each_wrote() {
        let processes = Processes::default();
        let dir = crate::testing::temp_dir("cmd-processes");
        processes.start(&Shell::default(), "echo first; sleep 30", &dir, ".").unwrap();
        processes.start(&Shell::default(), "sleep 30", &dir, "sub").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !views(&processes)[1].tail.contains("first") {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let shown = views(&processes);
        assert_eq!(shown.iter().map(|v| v.info.id).collect::<Vec<_>>(), [2, 1]);
        assert_eq!(shown[0].info.cwd, "sub");
        let json = serde_json::to_value(&shown[1]).unwrap();
        assert_eq!((json["id"].clone(), json["tail"].clone()), (serde_json::json!(1), serde_json::json!("first\n")));
    }

    /// The tab's Stop is the user's: the model hears of it at its next round.
    #[test]
    fn a_stop_from_the_tab_is_told_to_the_model() {
        let processes = Processes::default();
        let dir = crate::testing::temp_dir("cmd-processes-stop");
        processes.start(&Shell::default(), "sleep 30", &dir, ".").unwrap();
        let shown = stop(&processes, 1).unwrap();
        assert_eq!(shown[0].info.state, crate::domain::background::ProcessState::Stopped);
        assert_eq!(processes.take_ended().len(), 1);
        assert!(stop(&processes, 7).unwrap_err().contains("no background process #7"));
    }
}
