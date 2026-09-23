//! The terminals themselves — see `domain::terminal`.
//!
//! Each is a PTY with the user's login shell on its far side, and four
//! threads: one reads the PTY, one gathers what it read into frames for the
//! screen, one writes what the user typed, one waits for the shell to end.
//! Typing goes through a queue rather than straight into the PTY: a write
//! blocks while the program on the other side is not reading, and the command
//! that carried the keystroke would block the window with it.

use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use portable_pty::{native_pty_system, Child, ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::domain::terminal::{
    Scrollback, TerminalChanged, TerminalError, TerminalEventSink, TerminalInfo, TerminalOutputSink, TerminalSize,
    TerminalState,
};

/// Output is handed to the screen at most once a frame. The first chunk after
/// a quiet spell goes at once — an echoed key is not held back — and `yes` is
/// sixty messages a second over IPC, not thousands.
const FRAME: Duration = Duration::from_millis(16);

pub struct Terminals {
    registry: Mutex<Registry>,
    /// Told of every open, exit and close.
    changed: TerminalEventSink,
}

/// Nobody listening — tests, and anything built before the window is.
impl Default for Terminals {
    fn default() -> Self {
        Self::new(Arc::new(|_| {}))
    }
}

#[derive(Default)]
struct Registry {
    next_id: u32,
    entries: Vec<Entry>,
}

struct Entry {
    id: u32,
    shell: String,
    master: Box<dyn MasterPty + Send>,
    /// To the writer thread; dropping it ends that thread.
    input: Sender<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    shared: Arc<Mutex<Shared>>,
}

/// What the entry's threads touch. Lock order: the registry, then this —
/// the threads take only this one.
struct Shared {
    state: TerminalState,
    scrollback: Scrollback,
    /// The screen drawing it, by the key it attached with.
    screen: Option<(u32, TerminalOutputSink)>,
}

impl Registry {
    fn entry(&self, id: u32) -> Result<&Entry, TerminalError> {
        self.entries.iter().find(|e| e.id == id).ok_or(TerminalError::NotFound(id))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Terminals {
    pub fn new(changed: TerminalEventSink) -> Self {
        Self { registry: Mutex::default(), changed }
    }

    /// The user's login shell, in `cwd`.
    pub fn open(&self, cwd: &Path, size: TerminalSize) -> Result<TerminalInfo, TerminalError> {
        let mut command = CommandBuilder::new_default_prog();
        let shell = Path::new(&command.get_shell())
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "shell".to_string());
        prepare(&mut command, cwd);
        self.spawn(command, shell, size)
    }

    fn spawn(&self, command: CommandBuilder, shell: String, size: TerminalSize) -> Result<TerminalInfo, TerminalError> {
        let pair = native_pty_system().openpty(pty_size(size)).map_err(not_started)?;
        let reader = pair.master.try_clone_reader().map_err(not_started)?;
        let writer = pair.master.take_writer().map_err(not_started)?;
        let child = pair.slave.spawn_command(command).map_err(not_started)?;
        // The shell holds the far side now. Ours kept open would keep the
        // PTY from ever telling the reader it ended.
        drop(pair.slave);

        let shared = Arc::new(Mutex::new(Shared {
            state: TerminalState::Running,
            scrollback: Scrollback::default(),
            screen: None,
        }));
        let (input, typed) = mpsc::channel();
        let killer = child.clone_killer();

        let mut registry = lock(&self.registry);
        registry.next_id += 1;
        let id = registry.next_id;
        read(reader, Arc::clone(&shared));
        write(writer, typed);
        wait(child, Arc::clone(&shared), Arc::clone(&self.changed), id);
        let info = TerminalInfo { id, shell: shell.clone(), state: TerminalState::Running };
        registry.entries.push(Entry { id, shell, master: pair.master, input, killer, shared });
        drop(registry);
        (self.changed)(TerminalChanged { id });
        Ok(info)
    }

    /// Oldest first, the order they were opened in.
    pub fn list(&self) -> Vec<TerminalInfo> {
        lock(&self.registry)
            .entries
            .iter()
            .map(|e| TerminalInfo { id: e.id, shell: e.shell.clone(), state: lock(&e.shared).state })
            .collect()
    }

    /// Sends `screen` what the terminal wrote so far, then everything it
    /// writes next, in order and without a gap: both happen under the lock
    /// the frame thread takes to hand a frame on. A second attach takes over
    /// from the first — the tab was drawn again.
    pub fn attach(&self, id: u32, key: u32, screen: TerminalOutputSink) -> Result<(), TerminalError> {
        let shared = self.shared(id)?;
        let mut shared = lock(&shared);
        let replay = shared.scrollback.to_vec();
        if !replay.is_empty() {
            screen(&replay);
        }
        shared.screen = Some((key, screen));
        Ok(())
    }

    /// Only the screen that attached with `key`: a tab drawn again attaches
    /// before the old one's cleanup detaches, and that detach must not take
    /// the new screen with it.
    pub fn detach(&self, id: u32, key: u32) -> Result<(), TerminalError> {
        let shared = self.shared(id)?;
        let mut shared = lock(&shared);
        if shared.screen.as_ref().is_some_and(|(attached, _)| *attached == key) {
            shared.screen = None;
        }
        Ok(())
    }

    pub fn write(&self, id: u32, bytes: Vec<u8>) -> Result<(), TerminalError> {
        let registry = lock(&self.registry);
        let entry = registry.entry(id)?;
        if lock(&entry.shared).state != TerminalState::Running {
            return Err(TerminalError::Ended(id));
        }
        entry.input.send(bytes).map_err(|_| TerminalError::Ended(id))
    }

    pub fn resize(&self, id: u32, size: TerminalSize) -> Result<(), TerminalError> {
        let registry = lock(&self.registry);
        registry
            .entry(id)?
            .master
            .resize(pty_size(size))
            .map_err(|e| TerminalError::Resize { id, reason: e.to_string() })
    }

    /// Hangs up on the shell, as closing a terminal window does, and forgets it.
    pub fn close(&self, id: u32) -> Result<(), TerminalError> {
        let entry = {
            let mut registry = lock(&self.registry);
            let at = registry.entries.iter().position(|e| e.id == id).ok_or(TerminalError::NotFound(id))?;
            registry.entries.remove(at)
        };
        hang_up(entry);
        (self.changed)(TerminalChanged { id });
        Ok(())
    }

    /// The folder changed or the app is quitting.
    pub fn close_all(&self) {
        let entries = std::mem::take(&mut lock(&self.registry).entries);
        for entry in entries {
            let id = entry.id;
            hang_up(entry);
            (self.changed)(TerminalChanged { id });
        }
    }

    fn shared(&self, id: u32) -> Result<Arc<Mutex<Shared>>, TerminalError> {
        Ok(Arc::clone(&lock(&self.registry).entry(id)?.shared))
    }
}

impl Drop for Terminals {
    fn drop(&mut self) {
        self.close_all();
    }
}

/// Where it starts and what it is told about the terminal it is in.
fn prepare(command: &mut CommandBuilder, cwd: &Path) {
    command.cwd(cwd);
    // Inherited, it would name the directory the app started in, and `pwd`
    // in a shell that trusts it would print that.
    command.env("PWD", cwd);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    // Started from iTerm (`tauri dev`), the app carries iTerm's name; programs
    // that read it would assume features xterm.js does not have.
    command.env_remove("TERM_PROGRAM");
    command.env_remove("TERM_PROGRAM_VERSION");
    // An app started from the Dock has no `LANG`, and zsh then draws every
    // non-ASCII character as an escape.
    #[cfg(unix)]
    if command.get_env("LANG").is_none() {
        command.env("LANG", "en_US.UTF-8");
    }
}

fn not_started(e: impl std::fmt::Display) -> TerminalError {
    TerminalError::NotStarted(e.to_string())
}

fn pty_size(size: TerminalSize) -> PtySize {
    PtySize { rows: size.rows, cols: size.cols, pixel_width: 0, pixel_height: 0 }
}

/// Hangs up on the shell. What runs in its foreground — a `vim`, a `sleep`, in
/// a process group of its own — gets its SIGHUP from the kernel when the shell,
/// the terminal's controlling process, ends. Not signalled once the shell has
/// been reaped: its pid may belong to someone else by then.
fn hang_up(mut entry: Entry) {
    let mut shared = lock(&entry.shared);
    shared.screen = None;
    if shared.state != TerminalState::Running {
        return;
    }
    // SIGHUP on Unix, TerminateProcess on Windows.
    // ponytail: no SIGKILL for a shell that traps SIGHUP; add one on a timer if that shows up.
    let _ = entry.killer.kill();
}

/// Reads until the PTY ends, which it does when the last process holding its
/// far side is gone.
fn read(mut reader: Box<dyn Read + Send>, shared: Arc<Mutex<Shared>>) {
    let (chunks, received) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if chunks.send(chunk[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                // EIO on Linux once the shell is gone.
                Err(_) => break,
            }
        }
    });
    thread::spawn(move || frames(received, shared));
}

/// What arrived while the last frame was shown goes out as one.
fn frames(received: Receiver<Vec<u8>>, shared: Arc<Mutex<Shared>>) {
    while let Ok(mut frame) = received.recv() {
        frame.extend(received.try_iter().flatten());
        {
            let mut shared = lock(&shared);
            shared.scrollback.push(&frame);
            if let Some((_, screen)) = &shared.screen {
                screen(&frame);
            }
        }
        thread::sleep(FRAME);
    }
}

fn write(mut writer: Box<dyn Write + Send>, typed: Receiver<Vec<u8>>) {
    thread::spawn(move || {
        for bytes in typed {
            if writer.write_all(&bytes).and_then(|()| writer.flush()).is_err() {
                break;
            }
        }
    });
}

fn wait(mut child: Box<dyn Child + Send + Sync>, shared: Arc<Mutex<Shared>>, changed: TerminalEventSink, id: u32) {
    thread::spawn(move || {
        let code = match child.wait() {
            Ok(status) if status.signal().is_none() => Some(status.exit_code()),
            _ => None,
        };
        lock(&shared).state = TerminalState::Exited { code };
        changed(TerminalChanged { id });
    });
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::testing::temp_dir;
    use std::path::PathBuf;
    use std::time::Instant;

    const SIZE: TerminalSize = TerminalSize { cols: 80, rows: 24 };

    /// `/bin/sh` rather than the login shell: the user's `.zshrc` is not
    /// something a test should run, or wait for.
    fn open_sh(terminals: &Terminals, cwd: &Path) -> u32 {
        let mut command = CommandBuilder::new("/bin/sh");
        prepare(&mut command, cwd);
        terminals.spawn(command, "sh".into(), SIZE).expect("opens").id
    }

    fn folder(label: &str) -> PathBuf {
        temp_dir(label).canonicalize().unwrap()
    }

    /// A screen whose output lands in the returned buffer, and how many
    /// times it was called.
    fn screen() -> (TerminalOutputSink, Arc<Mutex<(Vec<u8>, usize)>>) {
        let seen = Arc::new(Mutex::new((Vec::new(), 0)));
        let sink = Arc::clone(&seen);
        (
            Arc::new(move |bytes: &[u8]| {
                let mut sink = sink.lock().unwrap();
                sink.0.extend_from_slice(bytes);
                sink.1 += 1;
            }),
            seen,
        )
    }

    fn text(seen: &Mutex<(Vec<u8>, usize)>) -> String {
        String::from_utf8_lossy(&seen.lock().unwrap().0).into_owned()
    }

    fn until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Each test types a sum, so the echo of the line typed is never what
    /// it waits for — only the shell's answer is.
    fn type_line(terminals: &Terminals, id: u32, line: &str) {
        terminals.write(id, format!("{line}\n").into_bytes()).unwrap();
    }

    #[test]
    fn what_is_typed_reaches_the_shell_in_the_folder() {
        let terminals = Terminals::default();
        let dir = folder("term-echo");
        let id = open_sh(&terminals, &dir);
        let (sink, seen) = screen();
        terminals.attach(id, 1, sink).unwrap();
        // One byte at a time, the way keys arrive: the queue keeps the order.
        for byte in "echo laika-$((6*7)); pwd; echo $TERM\n".bytes() {
            terminals.write(id, vec![byte]).unwrap();
        }
        until("the answer", || {
            let out = text(&seen);
            out.contains("laika-42") && out.contains(&dir.display().to_string()) && out.contains("xterm-256color")
        });
    }

    #[test]
    fn a_resize_reaches_the_shell() {
        let terminals = Terminals::default();
        let id = open_sh(&terminals, &folder("term-size"));
        let (sink, seen) = screen();
        terminals.attach(id, 1, sink).unwrap();
        terminals.resize(id, TerminalSize { cols: 101, rows: 37 }).unwrap();
        type_line(&terminals, id, "stty size");
        until("the new size", || text(&seen).contains("37 101"));
    }

    /// The tab closed and opened again: the new screen is drawn from what
    /// was kept, and an old screen's late detach leaves the new one alone.
    #[test]
    fn an_attach_replays_and_only_its_own_detach_ends_it() {
        let terminals = Terminals::default();
        let id = open_sh(&terminals, &folder("term-attach"));
        let (first, first_seen) = screen();
        terminals.attach(id, 1, first).unwrap();
        type_line(&terminals, id, "echo one-$((0+1))");
        until("the first answer", || text(&first_seen).contains("one-1"));

        let (second, second_seen) = screen();
        terminals.attach(id, 2, second).unwrap();
        assert!(text(&second_seen).contains("one-1"), "replayed at once: {}", text(&second_seen));
        terminals.detach(id, 1).unwrap();
        type_line(&terminals, id, "echo two-$((1+1))");
        until("the second answer", || text(&second_seen).contains("two-2"));
        assert!(!text(&first_seen).contains("two-2"), "the first screen was replaced");

        terminals.detach(id, 2).unwrap();
        type_line(&terminals, id, "echo three-$((1+2))");
        let (third, third_seen) = screen();
        until("the third answer, kept", || {
            terminals.attach(id, 3, Arc::clone(&third)).unwrap();
            text(&third_seen).contains("three-3")
        });
        assert!(!text(&second_seen).contains("three-3"), "detached");
    }

    #[test]
    fn a_shell_that_exits_stays_listed_with_its_code() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&heard);
        let terminals = Terminals::new(Arc::new(move |event: TerminalChanged| sink.lock().unwrap().push(event.id)));
        let id = open_sh(&terminals, &folder("term-exit"));
        assert_eq!(*heard.lock().unwrap(), [id], "the open");
        type_line(&terminals, id, "exit 3");
        until("the exit", || terminals.list()[0].state != TerminalState::Running);
        assert_eq!(terminals.list(), [TerminalInfo { id, shell: "sh".into(), state: TerminalState::Exited { code: Some(3) } }]);
        assert_eq!(*heard.lock().unwrap(), [id, id], "and the exit");
        assert_eq!(terminals.write(id, b"x".to_vec()), Err(TerminalError::Ended(id)));

        // Its tab closed: gone from the list, and said so.
        terminals.close(id).unwrap();
        assert!(terminals.list().is_empty());
        assert_eq!(*heard.lock().unwrap(), [id, id, id]);
    }

    /// What runs in the foreground has a process group of its own; closing
    /// the terminal ends it too, and says so.
    #[test]
    fn closing_ends_what_runs_in_it() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&heard);
        let terminals = Terminals::new(Arc::new(move |event: TerminalChanged| sink.lock().unwrap().push(event.id)));
        let dir = folder("term-close");
        let id = open_sh(&terminals, &dir);
        let (shell_file, sleep_file) = (dir.join("shell.pid"), dir.join("sleep.pid"));
        type_line(
            &terminals,
            id,
            &format!("echo $$ > {}; sh -c 'echo $$ > {}; exec sleep 300'", shell_file.display(), sleep_file.display()),
        );
        let pid = |file: &Path| -> i32 {
            until("a pid", || std::fs::read_to_string(file).is_ok_and(|s| s.ends_with('\n')));
            std::fs::read_to_string(file).unwrap().trim().parse().unwrap()
        };
        let (shell, sleep) = (pid(&shell_file), pid(&sleep_file));

        terminals.close(id).unwrap();
        until("the sleep to end", || unsafe { libc::kill(sleep, 0) } != 0);
        until("the shell to end", || unsafe { libc::kill(shell, 0) } != 0);
        assert!(terminals.list().is_empty());
        assert_eq!(terminals.write(id, b"x".to_vec()), Err(TerminalError::NotFound(id)));
        // The open, the close, and the shell's exit, which may come after it.
        let heard = heard.lock().unwrap();
        assert!(heard.len() >= 2 && heard.iter().all(|&h| h == id), "{heard:?}");
    }

    #[test]
    fn a_shell_ended_by_a_signal_has_no_code() {
        let terminals = Terminals::default();
        let id = open_sh(&terminals, &folder("term-signal"));
        type_line(&terminals, id, "kill -9 $$");
        until("the exit", || terminals.list()[0].state != TerminalState::Running);
        assert_eq!(terminals.list()[0].state, TerminalState::Exited { code: None });
    }

    #[test]
    fn closing_all_forgets_each_and_says_so() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&heard);
        let terminals = Terminals::new(Arc::new(move |event: TerminalChanged| sink.lock().unwrap().push(event.id)));
        let dir = folder("term-close-all");
        let ids = [open_sh(&terminals, &dir), open_sh(&terminals, &dir)];
        assert_eq!(terminals.list().iter().map(|t| t.id).collect::<Vec<_>>(), ids);
        // Ended already, so no exit can be heard in place of the close's.
        ids.iter().for_each(|&id| type_line(&terminals, id, "exit"));
        until("both exits", || terminals.list().iter().all(|t| t.state != TerminalState::Running));
        heard.lock().unwrap().clear();
        terminals.close_all();
        assert!(terminals.list().is_empty());
        assert_eq!(*heard.lock().unwrap(), ids);
    }

    /// A flood reaches the screen a frame at a time, not a read at a time.
    #[test]
    fn a_flood_is_handed_on_in_frames() {
        let terminals = Terminals::default();
        let id = open_sh(&terminals, &folder("term-flood"));
        let (sink, seen) = screen();
        terminals.attach(id, 1, sink).unwrap();
        let started = Instant::now();
        type_line(&terminals, id, "yes laika | head -n 200000; echo done-$((2*2))");
        until("the end of it", || text(&seen).contains("done-4"));
        let frames = seen.lock().unwrap().1 as u128;
        let at_most = started.elapsed().as_millis() / FRAME.as_millis() + 2;
        assert!(frames <= at_most, "{frames} frames in {:?}", started.elapsed());
        assert!(seen.lock().unwrap().0.len() > 1_000_000);
    }

    #[test]
    fn an_unknown_terminal_is_named() {
        let terminals = Terminals::default();
        let (sink, _) = screen();
        assert_eq!(terminals.attach(9, 1, sink), Err(TerminalError::NotFound(9)));
        assert_eq!(terminals.detach(9, 1), Err(TerminalError::NotFound(9)));
        assert_eq!(terminals.write(9, vec![]), Err(TerminalError::NotFound(9)));
        assert_eq!(terminals.resize(9, SIZE), Err(TerminalError::NotFound(9)));
        assert_eq!(terminals.close(9), Err(TerminalError::NotFound(9)));
    }

    #[test]
    fn the_shell_is_told_it_is_in_an_xterm_in_the_folder() {
        let mut command = CommandBuilder::new_default_prog();
        command.env("TERM_PROGRAM", "iTerm.app");
        prepare(&mut command, Path::new("/work/laika"));
        assert_eq!(command.get_cwd().map(PathBuf::from), Some(PathBuf::from("/work/laika")));
        assert_eq!(command.get_env("PWD"), Some("/work/laika".as_ref()));
        assert_eq!(command.get_env("TERM"), Some("xterm-256color".as_ref()));
        assert_eq!(command.get_env("COLORTERM"), Some("truecolor".as_ref()));
        assert_eq!(command.get_env("TERM_PROGRAM"), None);
        assert!(command.get_env("LANG").is_some());
    }
}
