//! The background processes themselves — see `domain::background`.
//!
//! Each runs in a process group of its own, like a `runCommand` command, so
//! stopping it stops what it started too: `npm run dev` is a tree. Its two
//! streams are read by threads into one buffer, and a third thread notices
//! the exit. A process that exits on its own has its group killed as well,
//! for the same reason `process_runner` does it — something it left behind
//! would otherwise hold the pipes and run on unseen.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use crate::domain::background::{
    BackgroundError, BackgroundProcesses, OutputBuffer, ProcessInfo, ProcessOutput, ProcessState, MAX_FINISHED,
    MAX_RUNNING,
};
use crate::domain::command_exec::{truncate_output, Shell, MAX_OUTPUT_CHARS};

use super::process_runner::{kill_tree, set_process_group};

/// How often an exit is looked for.
const POLL: Duration = Duration::from_millis(100);

#[derive(Default)]
pub struct Processes {
    registry: Arc<Mutex<Registry>>,
}

#[derive(Default)]
struct Registry {
    next_id: u32,
    entries: Vec<Entry>,
}

struct Entry {
    info: ProcessInfo,
    /// `Some` while it runs.
    child: Option<Child>,
    buffer: OutputBuffer,
    /// Its end has been told to the model, or needs no telling.
    reported: bool,
}

impl Registry {
    fn entry(&mut self, id: u32) -> Result<&mut Entry, BackgroundError> {
        self.entries.iter_mut().find(|e| e.info.id == id).ok_or(BackgroundError::NotFound(id))
    }
}

fn lock(registry: &Mutex<Registry>) -> MutexGuard<'_, Registry> {
    registry.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Processes {
    /// Stops one the user asked to stop, from the Terminal tab. Unlike the
    /// model's own `stopProcess`, the model is told at its next round.
    pub fn stop_by_user(&self, id: u32) -> Result<ProcessInfo, BackgroundError> {
        self.stop_inner(id, false)
    }

    /// The last `max_bytes` of a process's output, for the tab — without
    /// moving the model's place.
    pub fn tail(&self, id: u32, max_bytes: usize) -> Result<String, BackgroundError> {
        Ok(lock(&self.registry).entry(id)?.buffer.tail(max_bytes).to_string())
    }

    /// The workspace changed or the app is quitting. Nothing is reported:
    /// there is no conversation left that these belong to.
    pub fn stop_all(&self) {
        let mut registry = lock(&self.registry);
        for entry in registry.entries.iter_mut() {
            if let Some(mut child) = entry.child.take() {
                end(&mut child);
                entry.info.state = ProcessState::Stopped;
            }
            entry.reported = true;
        }
    }

    fn stop_inner(&self, id: u32, reported: bool) -> Result<ProcessInfo, BackgroundError> {
        let mut registry = lock(&self.registry);
        let entry = registry.entry(id)?;
        if let Some(mut child) = entry.child.take() {
            end(&mut child);
            entry.info.state = ProcessState::Stopped;
            entry.reported = reported;
        }
        Ok(entry.info.clone())
    }
}

impl Drop for Processes {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn end(child: &mut Child) {
    kill_tree(child);
    let _ = child.kill();
    let _ = child.wait();
}

impl BackgroundProcesses for Processes {
    fn start(&self, shell: &Shell, command: &str, cwd: &Path, shown_cwd: &str) -> Result<ProcessInfo, BackgroundError> {
        let mut registry = lock(&self.registry);
        let running: Vec<String> =
            registry.entries.iter().filter(|e| e.info.running()).map(|e| e.info.describe()).collect();
        if running.len() >= MAX_RUNNING {
            return Err(BackgroundError::TooMany(running.join("; ")));
        }

        let mut process = Command::new(&shell.program);
        process
            .args(&shell.args)
            .arg(command)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        set_process_group(&mut process);
        super::login_path::apply(&mut process);
        let mut child = process.spawn().map_err(|e| BackgroundError::NotStarted(format!("{}: {e}", shell.program)))?;

        registry.next_id += 1;
        let id = registry.next_id;
        read_into(&self.registry, id, child.stdout.take());
        read_into(&self.registry, id, child.stderr.take());
        let info = ProcessInfo { id, command: command.to_string(), cwd: shown_cwd.to_string(), state: ProcessState::Running };
        registry.entries.push(Entry { info: info.clone(), child: Some(child), buffer: OutputBuffer::default(), reported: false });
        watch(&self.registry, id);

        // Forget the oldest finished ones past the limit.
        let finished = registry.entries.iter().filter(|e| !e.info.running()).count();
        let mut excess = finished.saturating_sub(MAX_FINISHED);
        registry.entries.retain(|e| {
            let drop = excess > 0 && !e.info.running();
            if drop {
                excess -= 1;
            }
            !drop
        });
        Ok(info)
    }

    fn read(&self, id: u32) -> Result<ProcessOutput, BackgroundError> {
        let mut registry = lock(&self.registry);
        let entry = registry.entry(id)?;
        let (unread, missed) = entry.buffer.take_unread();
        let (output, truncated) = truncate_output(&unread, MAX_OUTPUT_CHARS);
        Ok(ProcessOutput { process: entry.info.clone(), output, missed, truncated })
    }

    fn stop(&self, id: u32) -> Result<ProcessInfo, BackgroundError> {
        self.stop_inner(id, true)
    }

    fn list(&self) -> Vec<ProcessInfo> {
        lock(&self.registry).entries.iter().map(|e| e.info.clone()).collect()
    }

    fn take_ended(&self) -> Vec<ProcessInfo> {
        let mut registry = lock(&self.registry);
        registry
            .entries
            .iter_mut()
            .filter(|e| !e.info.running() && !e.reported)
            .map(|e| {
                e.reported = true;
                e.info.clone()
            })
            .collect()
    }
}

/// Lossy on a chunk boundary, like `process_runner`'s reader: one broken
/// character is better than a lost stream.
fn read_into(registry: &Arc<Mutex<Registry>>, id: u32, pipe: Option<impl Read + Send + 'static>) {
    let Some(mut pipe) = pipe else { return };
    let registry = Arc::clone(registry);
    thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&chunk[..n]);
                    match lock(&registry).entry(id) {
                        Ok(entry) => entry.buffer.push(&text),
                        Err(_) => break,
                    }
                }
            }
        }
    });
}

fn watch(registry: &Arc<Mutex<Registry>>, id: u32) {
    let registry = Arc::clone(registry);
    thread::spawn(move || loop {
        thread::sleep(POLL);
        let mut registry = lock(&registry);
        let Ok(entry) = registry.entry(id) else { return };
        let Some(child) = entry.child.as_mut() else { return };
        match child.try_wait() {
            Ok(None) => {}
            Ok(Some(status)) => {
                kill_tree(child);
                entry.child = None;
                entry.info.state = ProcessState::Exited { code: status.code() };
                return;
            }
            Err(_) => {
                entry.child = None;
                entry.info.state = ProcessState::Exited { code: None };
                return;
            }
        }
    });
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::testing::temp_dir;
    use std::time::Instant;

    fn start(processes: &Processes, command: &str) -> ProcessInfo {
        processes.start(&Shell::default(), command, &temp_dir("bg"), ".").expect("starts")
    }

    fn until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_process_outlives_its_start_and_is_read_in_pieces() {
        let processes = Processes::default();
        let info = start(&processes, "echo ready; echo oops >&2; sleep 30");
        assert_eq!(info.state, ProcessState::Running);
        let mut seen = String::new();
        until("both streams", || {
            seen.push_str(&processes.read(info.id).unwrap().output);
            seen.contains("ready") && seen.contains("oops")
        });
        assert_eq!(processes.read(info.id).unwrap().output, "", "nothing new");
        assert!(processes.list()[0].running());
    }

    #[test]
    fn an_exit_is_noticed_and_told_once() {
        let processes = Processes::default();
        let info = start(&processes, "exit 3");
        until("the exit", || !processes.list()[0].running());
        assert_eq!(processes.list()[0].state, ProcessState::Exited { code: Some(3) });
        assert_eq!(processes.take_ended().len(), 1);
        assert!(processes.take_ended().is_empty(), "once");
        assert_eq!(processes.read(info.id).unwrap().process.state, ProcessState::Exited { code: Some(3) });
    }

    /// The model stopped it and knows; the user stopped it and the model
    /// has to be told.
    #[test]
    fn a_stop_ends_the_whole_tree_and_only_the_users_is_reported() {
        let processes = Processes::default();
        let dir = temp_dir("bg-tree");
        let pid_file = dir.join("child.pid");
        let own = processes
            .start(&Shell::default(), &format!("sleep 300 & echo $! > {}; wait", pid_file.display()), &dir, ".")
            .unwrap();
        until("the child", || pid_file.exists() && !std::fs::read_to_string(&pid_file).unwrap().trim().is_empty());
        let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();

        assert_eq!(processes.stop(own.id).unwrap().state, ProcessState::Stopped);
        until("the grandchild to die", || unsafe { libc::kill(pid, 0) } != 0);
        assert!(processes.take_ended().is_empty(), "the model asked for it");
        assert_eq!(processes.stop(own.id).unwrap().state, ProcessState::Stopped, "stopping again says how it ended");

        let theirs = start(&processes, "sleep 30");
        processes.stop_by_user(theirs.id).unwrap();
        assert_eq!(processes.take_ended().iter().map(|p| p.id).collect::<Vec<_>>(), [theirs.id]);
    }

    #[test]
    fn only_so_many_run_at_once() {
        let processes = Processes::default();
        for _ in 0..MAX_RUNNING {
            start(&processes, "sleep 30");
        }
        let refused = processes.start(&Shell::default(), "sleep 30", &temp_dir("bg-full"), ".").unwrap_err();
        assert!(matches!(&refused, BackgroundError::TooMany(list) if list.contains("#1 `sleep 30` is running")), "{refused}");
        processes.stop(1).unwrap();
        start(&processes, "sleep 30");
    }

    /// The oldest finished one goes — never one still running, however old.
    #[test]
    fn finished_ones_are_forgotten_past_the_limit() {
        let processes = Processes::default();
        start(&processes, "sleep 30");
        for _ in 0..=MAX_FINISHED {
            let info = start(&processes, "true");
            processes.stop(info.id).unwrap();
        }
        start(&processes, "sleep 30");
        let ids: Vec<u32> = processes.list().iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), MAX_FINISHED + 2);
        assert_eq!(&ids[..2], [1, 3], "the oldest finished one went, the running one stayed");
        assert_eq!(processes.read(2), Err(BackgroundError::NotFound(2)));
    }

    /// A read carries at most what a tool result may: a chatty server's
    /// backlog is cut in the middle, and the result says so.
    #[test]
    fn a_long_backlog_is_cut_for_the_model() {
        let processes = Processes::default();
        let info = start(&processes, "seq 1 20000; echo done; sleep 30");
        until("the backlog", || processes.tail(info.id, 10).unwrap().contains("done"));
        let read = processes.read(info.id).unwrap();
        assert!(read.truncated);
        assert!(read.output.chars().count() <= MAX_OUTPUT_CHARS + 200, "{}", read.output.len());
        assert!(read.output.starts_with("1\n") && read.output.trim_end().ends_with("done"), "{}", &read.output[read.output.len() - 40..]);
    }

    #[test]
    fn stopping_everything_is_not_reported() {
        let processes = Processes::default();
        start(&processes, "sleep 30");
        processes.stop_all();
        assert_eq!(processes.list()[0].state, ProcessState::Stopped);
        assert!(processes.take_ended().is_empty());
    }

    #[test]
    fn the_tab_reads_the_tail_without_moving_the_models_place() {
        let processes = Processes::default();
        let info = start(&processes, "echo ready; sleep 30");
        until("output", || processes.tail(info.id, 100).unwrap().contains("ready"));
        assert_eq!(processes.tail(info.id, 3).unwrap(), "dy\n");
        assert!(processes.read(info.id).unwrap().output.contains("ready"));
    }
}
