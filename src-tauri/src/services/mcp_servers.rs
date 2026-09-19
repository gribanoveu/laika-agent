//! The MCP servers the app keeps running.
//!
//! Started for the first Agent turn that needs them and kept between turns —
//! a server's start can be a package download. An entry that changes, or is
//! switched off, stops its server; a workspace that changes stops them all,
//! because each runs in the workspace it was started for.
//!
//! **Failure** (decided in `docs/06-port-plan.md`, stage 7): a server that
//! stops in the middle of a turn fails the call it was on — never retried,
//! the server may have done the thing — and is started again, once per turn,
//! before the next call to it. The tool list the model was given stays as it
//! was for the rest of the turn.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::Value;

use crate::domain::mcp::{
    items, ConnectedServer, McpCallResult, McpClient, McpConfig, McpError, McpServerConfig, McpServerState,
    McpTool, McpTools,
};

/// Starts one server in a folder and completes its handshake — the stdio
/// process in the app, a scripted client in tests.
pub type Start =
    Arc<dyn Fn(&McpServerConfig, &Path, &dyn Fn() -> bool) -> Result<Arc<dyn McpClient>, McpError> + Send + Sync>;

pub struct McpServers {
    start: Start,
    pool: Mutex<Pool>,
}

#[derive(Default)]
struct Pool {
    cwd: Option<PathBuf>,
    slots: BTreeMap<String, Slot>,
}

struct Slot {
    config: McpServerConfig,
    state: SlotState,
}

enum SlotState {
    Starting,
    Running { server: Arc<Supervised>, tools: Vec<McpTool> },
    Failed(String),
}

impl McpServers {
    pub fn new(start: Start) -> Self {
        Self { start, pool: Mutex::default() }
    }

    /// Brings the running servers in line with `config` and returns their
    /// tools for one turn in `cwd`.
    ///
    /// New servers start side by side, and with the pool unlocked: the tab
    /// asks for their state while they start, and a first `npx` run can take
    /// its whole timeout. One that fails to start gives no tools; one stopped
    /// by the user's Stop is simply tried again next turn.
    pub fn for_turn(&self, config: &McpConfig, cwd: &Path, cancelled: &(dyn Fn() -> bool + Sync)) -> McpTools {
        let missing: Vec<(String, McpServerConfig)> = {
            let mut pool = lock(&self.pool);
            if pool.cwd.as_deref() != Some(cwd) {
                pool.slots.clear();
                pool.cwd = Some(cwd.to_path_buf());
            }
            let wanted = runnable(config);
            pool.slots.retain(|name, slot| wanted.get(name) == Some(&slot.config));
            let missing: Vec<_> = wanted.into_iter().filter(|(name, _)| !pool.slots.contains_key(name)).collect();
            for (name, config) in &missing {
                pool.slots.insert(name.clone(), Slot { config: config.clone(), state: SlotState::Starting });
            }
            missing
        };

        let started: Vec<(String, Option<SlotState>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = missing
                .into_iter()
                .map(|(name, config)| {
                    scope.spawn(move || {
                        let state = self.start_one(&config, cwd, cancelled);
                        (name, state)
                    })
                })
                .collect();
            handles.into_iter().filter_map(|handle| handle.join().ok()).collect()
        });

        let mut pool = lock(&self.pool);
        for (name, state) in started {
            // Switched off or edited while it started: `prune` has removed
            // the slot, and the server just started is dropped — which stops
            // it.
            let Some(slot) = pool.slots.get_mut(&name) else { continue };
            match state {
                Some(state) => slot.state = state,
                None => {
                    pool.slots.remove(&name);
                }
            }
        }
        let servers = pool
            .slots
            .iter()
            .filter_map(|(name, slot)| match &slot.state {
                SlotState::Running { server, tools } => {
                    server.restarted.store(false, Ordering::SeqCst);
                    Some(ConnectedServer {
                        name: name.clone(),
                        weight: slot.config.weight(),
                        client: Arc::clone(server) as Arc<dyn McpClient>,
                        tools: tools.clone(),
                    })
                }
                _ => None,
            })
            .collect();
        McpTools::new(servers)
    }

    /// `None` when the user's Stop cut the start short: that says nothing
    /// about the server, so it is not recorded as a failure.
    fn start_one(&self, config: &McpServerConfig, cwd: &Path, cancelled: &dyn Fn() -> bool) -> Option<SlotState> {
        let started = (self.start)(config, cwd, cancelled).and_then(|client| Ok((client.list_tools()?, client)));
        match started {
            Ok((tools, client)) => Some(SlotState::Running {
                server: Arc::new(Supervised {
                    config: config.clone(),
                    cwd: cwd.to_path_buf(),
                    start: Arc::clone(&self.start),
                    current: Mutex::new(client),
                    restarted: AtomicBool::new(false),
                    last_error: Mutex::new(None),
                }),
                tools,
            }),
            Err(McpError::Cancelled) => None,
            Err(error) => Some(SlotState::Failed(error.to_string())),
        }
    }

    /// Stops the servers whose entry is gone, changed or switched off, at
    /// once rather than at the next turn — switching a server off should
    /// stop it.
    pub fn prune(&self, config: &McpConfig) {
        let wanted = runnable(config);
        lock(&self.pool).slots.retain(|name, slot| wanted.get(name) == Some(&slot.config));
    }

    /// What the tab says about a server whose entry is `entry` now. A server
    /// running from an older entry is about to be replaced, so it is
    /// reported as not started.
    pub fn state(&self, name: &str, entry: &McpServerConfig) -> McpServerState {
        let pool = lock(&self.pool);
        let Some(slot) = pool.slots.get(name).filter(|slot| &slot.config == entry) else {
            return McpServerState::NotStarted;
        };
        match &slot.state {
            SlotState::Starting => McpServerState::Starting,
            SlotState::Running { server, tools } if server.is_alive() => McpServerState::Running { tools: tools.len() },
            SlotState::Running { server, .. } => McpServerState::Exited {
                error: lock(&server.last_error).clone().unwrap_or_else(|| "the MCP server exited".into()),
            },
            SlotState::Failed(error) => McpServerState::Failed { error: error.clone() },
        }
    }

    /// When the app quits.
    pub fn stop_all(&self) {
        lock(&self.pool).slots.clear();
    }
}

/// The entries that would start: switched on, and with nothing wrong that
/// the entry alone shows.
fn runnable(config: &McpConfig) -> BTreeMap<String, McpServerConfig> {
    items(config)
        .into_iter()
        .filter(|item| item.enabled && item.error.is_none())
        .map(|item| {
            let entry = config.mcp_servers[&item.name].clone();
            (item.name, entry)
        })
        .collect()
}

/// A server that is started again when it has stopped — at most once per
/// turn, and only before a call, never to repeat one.
struct Supervised {
    config: McpServerConfig,
    cwd: PathBuf,
    start: Start,
    current: Mutex<Arc<dyn McpClient>>,
    /// Reset by `for_turn`.
    restarted: AtomicBool,
    /// How it stopped, for the tab.
    last_error: Mutex<Option<String>>,
}

impl Supervised {
    fn live(&self, cancelled: &dyn Fn() -> bool) -> Result<Arc<dyn McpClient>, McpError> {
        let mut current = lock(&self.current);
        if current.is_alive() {
            return Ok(Arc::clone(&current));
        }
        if self.restarted.swap(true, Ordering::SeqCst) {
            return Err(McpError::NotRestarted);
        }
        *current = (self.start)(&self.config, &self.cwd, cancelled).inspect_err(|error| {
            *lock(&self.last_error) = Some(error.to_string());
        })?;
        Ok(Arc::clone(&current))
    }
}

impl McpClient for Supervised {
    fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
        lock(&self.current).list_tools()
    }

    fn call_tool(&self, name: &str, arguments: Value, cancelled: &dyn Fn() -> bool) -> Result<McpCallResult, McpError> {
        let result = self.live(cancelled)?.call_tool(name, arguments, cancelled);
        if let Err(error @ McpError::Exited { .. }) = &result {
            *lock(&self.last_error) = Some(error.to_string());
        }
        result
    }

    fn is_alive(&self) -> bool {
        lock(&self.current).is_alive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::AtomicUsize;

    /// One process of a scripted server: it dies on the tool `crash`.
    struct Process {
        alive: AtomicBool,
        run: usize,
    }

    impl McpClient for Process {
        fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
            Ok(vec![McpTool { name: "echo".into(), description: String::new(), input_schema: json!({"type": "object"}) }])
        }
        fn call_tool(&self, name: &str, _: Value, _: &dyn Fn() -> bool) -> Result<McpCallResult, McpError> {
            if name == "crash" {
                self.alive.store(false, Ordering::SeqCst);
                return Err(McpError::Exited { code: Some(1), stderr: "boom".into() });
            }
            Ok(McpCallResult { text: format!("run {}", self.run), is_error: false })
        }
        fn is_alive(&self) -> bool {
            self.alive.load(Ordering::SeqCst)
        }
    }

    /// Counts starts; a command named `broken` does not start, one named
    /// `once` starts only the first time.
    fn servers() -> (McpServers, Arc<AtomicUsize>) {
        let starts = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&starts);
        let start: Start = Arc::new(move |config, _, cancelled| {
            let run = counted.fetch_add(1, Ordering::SeqCst) + 1;
            if cancelled() {
                return Err(McpError::Cancelled);
            }
            if config.command == "broken" || (config.command == "once" && run > 1) {
                return Err(McpError::NotStarted("no such command".into()));
            }
            Ok(Arc::new(Process { alive: AtomicBool::new(true), run }) as Arc<dyn McpClient>)
        });
        (McpServers::new(start), starts)
    }

    fn config(entries: &[(&str, &str)]) -> McpConfig {
        let mut config = McpConfig::default();
        for (name, command) in entries {
            config.mcp_servers.insert(name.to_string(), McpServerConfig { command: command.to_string(), ..Default::default() });
        }
        config
    }

    const NO: &(dyn Fn() -> bool + Sync) = &|| false;

    fn call(tools: &McpTools, tool: &str) -> Result<String, McpError> {
        let entry = tools.get("mcp__a__echo").expect("the tool is offered");
        entry.client.call_tool(tool, json!({}), &|| false).map(|r| r.text)
    }

    fn cwd() -> PathBuf {
        PathBuf::from("/work")
    }

    #[test]
    fn a_server_starts_once_and_is_kept_between_turns() {
        let (servers, starts) = servers();
        let config = config(&[("a", "ok")]);
        assert_eq!(servers.state("a", &config.mcp_servers["a"]), McpServerState::NotStarted);

        let tools = servers.for_turn(&config, &cwd(), NO);
        assert_eq!(call(&tools, "echo").unwrap(), "run 1");
        servers.for_turn(&config, &cwd(), NO);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert_eq!(servers.state("a", &config.mcp_servers["a"]), McpServerState::Running { tools: 1 });
    }

    /// The done-when of F-7.4d: the model gets the error, the call is not
    /// repeated, and the next call goes to a server started again.
    #[test]
    fn a_crash_fails_its_call_and_the_next_call_restarts_the_server_once() {
        let (servers, starts) = servers();
        let config = config(&[("a", "ok")]);
        let tools = servers.for_turn(&config, &cwd(), NO);

        let error = call(&tools, "crash").unwrap_err();
        assert!(matches!(error, McpError::Exited { .. }), "{error:?}");
        assert_eq!(starts.load(Ordering::SeqCst), 1, "the failed call is not repeated on a new server");
        assert!(matches!(servers.state("a", &config.mcp_servers["a"]), McpServerState::Exited { error } if error.contains("boom")));

        assert_eq!(call(&tools, "echo").unwrap(), "run 2");
        call(&tools, "crash").unwrap_err();
        assert_eq!(call(&tools, "echo").unwrap_err(), McpError::NotRestarted, "one restart per turn");
        assert_eq!(starts.load(Ordering::SeqCst), 2);

        let tools = servers.for_turn(&config, &cwd(), NO);
        assert_eq!(call(&tools, "echo").unwrap(), "run 3", "a new turn may restart it again");
    }

    #[test]
    fn a_restart_that_fails_is_not_tried_again_that_turn() {
        let (servers, starts) = servers();
        let config = config(&[("a", "once")]);
        let tools = servers.for_turn(&config, &cwd(), NO);
        call(&tools, "crash").unwrap_err();
        assert!(matches!(call(&tools, "echo").unwrap_err(), McpError::NotStarted(_)));
        assert_eq!(call(&tools, "echo").unwrap_err(), McpError::NotRestarted);
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        assert!(matches!(servers.state("a", &config.mcp_servers["a"]), McpServerState::Exited { error } if error.contains("no such command")));
    }

    /// A server that hangs on start would otherwise cost every turn its
    /// timeout.
    #[test]
    fn a_server_that_failed_to_start_waits_for_its_entry_to_change() {
        let (servers, starts) = servers();
        let broken = config(&[("a", "broken")]);
        assert!(servers.for_turn(&broken, &cwd(), NO).get("mcp__a__echo").is_none());
        servers.for_turn(&broken, &cwd(), NO);
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert!(matches!(servers.state("a", &broken.mcp_servers["a"]), McpServerState::Failed { error } if error.contains("no such command")));

        let fixed = config(&[("a", "ok")]);
        assert_eq!(servers.state("a", &fixed.mcp_servers["a"]), McpServerState::NotStarted);
        assert!(servers.for_turn(&fixed, &cwd(), NO).get("mcp__a__echo").is_some());
    }

    #[test]
    fn a_stop_during_the_start_is_not_a_failure() {
        let (servers, starts) = servers();
        let config = config(&[("a", "ok")]);
        assert!(servers.for_turn(&config, &cwd(), &|| true).get("mcp__a__echo").is_none());
        assert_eq!(servers.state("a", &config.mcp_servers["a"]), McpServerState::NotStarted);
        servers.for_turn(&config, &cwd(), NO);
        assert_eq!(starts.load(Ordering::SeqCst), 2, "tried again");
    }

    #[test]
    fn switched_off_broken_and_changed_entries_do_not_run() {
        let (servers, starts) = servers();
        let mut config = config(&[("a", "ok"), ("b", "ok"), ("c", "")]);
        config.mcp_servers.get_mut("b").unwrap().disabled = true;
        let tools = servers.for_turn(&config, &cwd(), NO);
        assert_eq!(tools.definitions().len(), 1);
        assert_eq!(starts.load(Ordering::SeqCst), 1, "only a");

        config.mcp_servers.get_mut("a").unwrap().args = vec!["--new".into()];
        servers.for_turn(&config, &cwd(), NO);
        assert_eq!(starts.load(Ordering::SeqCst), 2, "a changed entry is a new server");
    }

    #[test]
    fn switching_a_server_off_stops_it_at_once() {
        let (servers, _) = servers();
        let mut config = config(&[("a", "ok")]);
        let tools = servers.for_turn(&config, &cwd(), NO);
        config.mcp_servers.get_mut("a").unwrap().disabled = true;
        servers.prune(&config);
        let entry = &tools.get("mcp__a__echo").unwrap().client;
        assert_eq!(Arc::strong_count(entry), 1, "held by this turn's tools alone");
        assert_eq!(servers.state("a", &config.mcp_servers["a"]), McpServerState::NotStarted);
    }

    #[test]
    fn a_server_switched_off_while_it_starts_does_not_stay() {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let start: Start = Arc::new(move |_, _, _| {
            wait.lock().unwrap().recv().unwrap();
            Ok(Arc::new(Process { alive: AtomicBool::new(true), run: 1 }) as Arc<dyn McpClient>)
        });
        let servers = Arc::new(McpServers::new(start));
        let mut config = config(&[("a", "ok")]);
        let turn = {
            let (servers, config) = (Arc::clone(&servers), config.clone());
            std::thread::spawn(move || servers.for_turn(&config, &cwd(), NO).definitions().len())
        };
        while servers.state("a", &config.mcp_servers["a"]) != McpServerState::Starting {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        config.mcp_servers.get_mut("a").unwrap().disabled = true;
        servers.prune(&config);
        release.send(()).unwrap();

        assert_eq!(turn.join().unwrap(), 0, "the turn does not get it");
        config.mcp_servers.get_mut("a").unwrap().disabled = false;
        assert_eq!(servers.state("a", &config.mcp_servers["a"]), McpServerState::NotStarted, "nor does the pool keep it");
    }

    /// On quit: the servers run in process groups of their own and would
    /// outlive the app.
    #[test]
    fn quitting_stops_every_server() {
        let (servers, _) = servers();
        let config = config(&[("a", "ok")]);
        drop(servers.for_turn(&config, &cwd(), NO));
        servers.stop_all();
        assert_eq!(servers.state("a", &config.mcp_servers["a"]), McpServerState::NotStarted);
    }

    #[test]
    fn another_workspace_starts_the_servers_again() {
        let (servers, starts) = servers();
        let config = config(&[("a", "ok")]);
        servers.for_turn(&config, &cwd(), NO);
        servers.for_turn(&config, Path::new("/elsewhere"), NO);
        assert_eq!(starts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn the_weight_comes_from_the_entry() {
        let (servers, _) = servers();
        let mut config = config(&[("a", "ok")]);
        config.mcp_servers.get_mut("a").unwrap().weight = Some(7);
        assert_eq!(servers.for_turn(&config, &cwd(), NO).weight("mcp__a__echo"), 7);
    }
}
