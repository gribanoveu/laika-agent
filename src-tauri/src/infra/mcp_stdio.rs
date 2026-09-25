//! An MCP server started as a process and spoken to over its stdin and
//! stdout: newline-delimited JSON-RPC 2.0.
//!
//! Written here rather than taken from `rmcp`, which is async on tokio;
//! everything around this is blocking, and the stdio subset used is small.
//! `domain::mcp::McpClient` is the seam an `rmcp`-based HTTP client would
//! plug into.
//!
//! **Protocol era.** The handshake is the `initialize` one, which every
//! server up to 2025-11-25 speaks — nearly all of them today. A server that
//! speaks only 2026-07-28 or later (no handshake, `server/discover`) refuses
//! it and is reported as such. **Revisit when** such servers are common: the
//! specification's dual-era client probes `server/discover` first.
//!
//! Two layers: [`Connection`] is the protocol over any transport — a reader
//! and a writer, which is what the tests drive with no process, or a send
//! function and an [`Inbox`], which is how `infra::mcp_http` uses it — and
//! [`StdioServer`] starts the process and hands its pipes to one.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::domain::mcp::{render_content, McpCallResult, McpClient, McpError, McpServerConfig, McpTool};
use crate::infra::process_runner::{kill_tree, set_process_group};

/// What this client asks for in `initialize`. A server answering with an
/// older version is accepted: `tools/list` and `tools/call` have not changed
/// shape since the first one.
pub const PROTOCOL_VERSION: &str = "2025-11-25";

/// How often a waiting call looks at its stop flag.
const POLL: Duration = Duration::from_millis(50);
/// Lines of the server's stderr kept to explain an exit.
const STDERR_LINES: usize = 20;
const STDERR_LINE_CHARS: usize = 500;
/// A server that pages its tool list forever is broken, not large.
const MAX_TOOL_PAGES: usize = 50;

type Reply = Result<Value, McpError>;
type Pending = Arc<Mutex<HashMap<u64, Sender<Reply>>>>;

/// Puts one message on the wire. Must not wait for an answer: answers come
/// back through the [`Inbox`].
pub type Outbox = Arc<dyn Fn(&Value) -> std::io::Result<()> + Send + Sync>;

/// Where the transport hands what the server sends: answers go to whoever
/// is waiting for them, and the end of the conversation reaches every
/// waiter at once instead of leaving each to its timeout.
#[derive(Clone, Default)]
pub struct Inbox {
    pending: Pending,
    closed: Arc<AtomicBool>,
}

impl Inbox {
    /// Takes one message from the server. Returns the reply the server is
    /// owed when the message is a request of its own: `ping` is the one a
    /// client that declared no capabilities must answer, and anything else is
    /// refused so the server is not left waiting either. Notifications are
    /// not needed yet, and an answer to nobody — a call already abandoned —
    /// is dropped.
    pub fn deliver(&self, message: &Value) -> Option<Value> {
        match (message.get("id"), message["method"].as_str()) {
            (Some(id), None) => {
                let waiter = id.as_u64().and_then(|id| lock(&self.pending).remove(&id))?;
                let reply = match message.get("error") {
                    Some(error) => Err(McpError::Server {
                        code: error["code"].as_i64().unwrap_or(0),
                        message: error["message"].as_str().unwrap_or("no message").to_string(),
                    }),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = waiter.send(reply);
                None
            }
            (Some(id), Some("ping")) => Some(json!({ "jsonrpc": "2.0", "id": id, "result": {} })),
            (Some(id), Some(method)) => Some(
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("{method} is not supported") } }),
            ),
            _ => None,
        }
    }

    /// One request failed on its way, not the whole conversation.
    pub fn fail(&self, id: u64, error: McpError) {
        if let Some(waiter) = lock(&self.pending).remove(&id) {
            let _ = waiter.send(Err(error));
        }
    }

    /// The conversation is over. Every waiter, and every request made from
    /// now on, gets the connection's `describe_close`.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        for (_, waiter) in lock(&self.pending).drain() {
            let _ = waiter.send(Err(McpError::Exited { code: None, stderr: String::new() }));
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

/// JSON-RPC over one transport.
pub struct Connection {
    send: Outbox,
    inbox: Inbox,
    next_id: AtomicU64,
    timeout: Duration,
    /// Says what the end of the conversation means — for a process, its
    /// exit code and last stderr lines.
    describe_close: Box<dyn Fn() -> McpError + Send + Sync>,
}

impl Connection {
    /// Newline-delimited messages over a reader and a writer: stdio.
    pub fn new(
        reader: impl BufRead + Send + 'static,
        writer: impl Write + Send + 'static,
        timeout: Duration,
        describe_close: impl Fn() -> McpError + Send + Sync + 'static,
    ) -> Self {
        let writer = Mutex::new(writer);
        let send: Outbox = Arc::new(move |message| {
            let mut writer = lock(&writer);
            // `Value`'s compact form has no line breaks, which is what the
            // transport requires of a message.
            writeln!(writer, "{message}")?;
            writer.flush()
        });
        let inbox = Inbox::default();
        read_in_background(reader, inbox.clone(), Arc::clone(&send));
        Self::with_transport(send, inbox, timeout, describe_close)
    }

    /// Any other transport: it sends with `send` and hands what comes back
    /// to `inbox`.
    pub fn with_transport(
        send: Outbox,
        inbox: Inbox,
        timeout: Duration,
        describe_close: impl Fn() -> McpError + Send + Sync + 'static,
    ) -> Self {
        Self { send, inbox, next_id: AtomicU64::new(1), timeout, describe_close: Box::new(describe_close) }
    }

    /// The handshake. Nothing else may be sent before it.
    pub fn initialize(&self, cancelled: &dyn Fn() -> bool) -> Result<(), McpError> {
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "laika-agent", "version": env!("CARGO_PKG_VERSION") },
        });
        let result = self.request("initialize", params, cancelled).map_err(|e| match e {
            // -32601: no such method; -32022: unsupported protocol version —
            // how a server of the handshake-less era answers this one.
            McpError::Server { code: code @ (-32601 | -32022), message } => McpError::Handshake(format!(
                "{message} (code {code}). It may speak only MCP 2026-07-28 or later, which this build does not yet"
            )),
            McpError::Server { code, message } => McpError::Handshake(format!("{message} (code {code})")),
            other => other,
        })?;
        if !result["protocolVersion"].is_string() {
            return Err(McpError::Protocol(format!("initialize returned no protocolVersion: {result}")));
        }
        (self.send)(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).map_err(|_| self.close_error())
    }

    fn request(&self, method: &str, params: Value, cancelled: &dyn Fn() -> bool) -> Reply {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        lock(&self.inbox.pending).insert(id, tx);
        // `close` sets the flag before it drains `pending`, so a request
        // registered after the drain sees the flag here instead of waiting
        // out its timeout for an answer nobody will send.
        if self.inbox.is_closed() {
            lock(&self.inbox.pending).remove(&id);
            return Err(self.close_error());
        }
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if (self.send)(&message).is_err() {
            lock(&self.inbox.pending).remove(&id);
            return Err(self.close_error());
        }

        let deadline = Instant::now() + self.timeout;
        loop {
            match rx.recv_timeout(POLL) {
                Ok(Err(McpError::Exited { .. })) | Err(RecvTimeoutError::Disconnected) => {
                    return Err(self.close_error())
                }
                Ok(reply) => return reply,
                Err(RecvTimeoutError::Timeout) => {
                    if cancelled() {
                        self.abandon(id, "cancelled by the user");
                        return Err(McpError::Cancelled);
                    }
                    if Instant::now() >= deadline {
                        self.abandon(id, "timed out");
                        return Err(McpError::Timeout(self.timeout.as_secs()));
                    }
                }
            }
        }
    }

    /// Stops waiting, and tells the server so it can stop too. Whatever it
    /// answers later is dropped by the inbox: nobody is waiting for it.
    fn abandon(&self, id: u64, reason: &str) {
        lock(&self.inbox.pending).remove(&id);
        let _ = (self.send)(
            &json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": id, "reason": reason } }),
        );
    }

    fn close_error(&self) -> McpError {
        (self.describe_close)()
    }
}

impl McpClient for Connection {
    fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_TOOL_PAGES {
            let params = match &cursor {
                Some(cursor) => json!({ "cursor": cursor }),
                None => json!({}),
            };
            let page = self.request("tools/list", params, &|| false)?;
            let listed = page["tools"]
                .as_array()
                .ok_or_else(|| McpError::Protocol(format!("tools/list returned no tools array: {page}")))?;
            tools.extend(listed.iter().filter_map(tool));
            match page["nextCursor"].as_str() {
                Some(next) if !next.is_empty() => cursor = Some(next.to_string()),
                _ => return Ok(tools),
            }
        }
        Err(McpError::Protocol(format!("tools/list kept paging past {MAX_TOOL_PAGES} pages")))
    }

    fn call_tool(&self, name: &str, arguments: Value, cancelled: &dyn Fn() -> bool) -> Result<McpCallResult, McpError> {
        let result = self.request("tools/call", json!({ "name": name, "arguments": arguments }), cancelled)?;
        Ok(McpCallResult { text: render_content(&result), is_error: result["isError"].as_bool().unwrap_or(false) })
    }

    fn is_alive(&self) -> bool {
        !self.inbox.is_closed()
    }
}

/// A tool entry, or nothing for one without a name — there is no way to
/// call it.
fn tool(entry: &Value) -> Option<McpTool> {
    let name = entry["name"].as_str().filter(|n| !n.is_empty())?;
    Some(McpTool {
        name: name.to_string(),
        description: entry["description"].as_str().unwrap_or_default().to_string(),
        input_schema: match &entry["inputSchema"] {
            schema @ Value::Object(_) => schema.clone(),
            _ => json!({ "type": "object" }),
        },
    })
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // A panic elsewhere while holding it leaves the map usable; a poisoned
    // lock is not a reason to stop talking to the server.
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The stdio side of the inbox: a line per message. A line that is not JSON
/// is a server logging to stdout, which the transport forbids and several
/// do — skipped rather than fatal.
fn read_in_background(reader: impl BufRead + Send + 'static, inbox: Inbox, send: Outbox) {
    std::thread::spawn(move || {
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
            if let Some(reply) = inbox.deliver(&message) {
                let _ = send(&reply);
            }
        }
        inbox.close();
    });
}

// ------------------------------------------------------------- the process

/// A running server process and the session with it. Dropping it kills the
/// process and everything it started — `npx` runs the real server as a
/// child, and killing only `npx` would leave that one behind.
pub struct StdioServer {
    child: Arc<Mutex<Child>>,
    connection: Connection,
}

impl StdioServer {
    /// Starts the process in `cwd` and completes the handshake, within the
    /// server's own timeout — a first `npx` run downloads the package.
    pub fn start(config: &McpServerConfig, cwd: &Path, cancelled: &dyn Fn() -> bool) -> Result<Self, McpError> {
        let mut command = Command::new(&config.command);
        // Before the entry's own `env`, which may set a `PATH` of its own.
        crate::infra::login_path::apply(&mut command);
        command
            .args(&config.args)
            .envs(&config.env)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        set_process_group(&mut command);
        let mut child = command.spawn().map_err(|e| McpError::NotStarted(format!("{}: {e}", config.command)))?;

        let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            kill_tree(&mut child);
            return Err(McpError::NotStarted("its standard streams were not available".into()));
        };
        let tail = keep_tail(stderr);
        let child = Arc::new(Mutex::new(child));
        let exited = Arc::clone(&child);
        let connection = Connection::new(
            BufReader::new(stdout),
            stdin,
            Duration::from_secs(config.timeout_secs()),
            move || McpError::Exited { code: exit_code(&exited), stderr: lock(&tail).iter().cloned().collect::<Vec<_>>().join("\n") },
        );

        let server = Self { child, connection };
        server.connection.initialize(cancelled)?;
        Ok(server)
    }
}

impl McpClient for StdioServer {
    fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
        self.connection.list_tools()
    }

    fn call_tool(&self, name: &str, arguments: Value, cancelled: &dyn Fn() -> bool) -> Result<McpCallResult, McpError> {
        self.connection.call_tool(name, arguments, cancelled)
    }

    /// The stream closing is how an exit shows first; the process is asked
    /// too, for one that is gone while its stdout is still held open by a
    /// child of its own.
    fn is_alive(&self) -> bool {
        self.connection.is_alive() && matches!(lock(&self.child).try_wait(), Ok(None))
    }
}

impl Drop for StdioServer {
    fn drop(&mut self) {
        let mut child = lock(&self.child);
        kill_tree(&mut child);
        // The server itself as well, by its pid: if the group signal missed
        // it for any reason, `wait` below would block the app on a process
        // that is still reading its stdin.
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// The stream closed because the process is exiting; give it a moment to
/// finish so its code can be reported, rather than none.
fn exit_code(child: &Mutex<Child>) -> Option<i32> {
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        if let Ok(Some(status)) = lock(child).try_wait() {
            return status.code();
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The last lines of stderr, read as they come so the pipe never fills and
/// stalls the server.
fn keep_tail(stderr: impl std::io::Read + Send + 'static) -> Arc<Mutex<VecDeque<String>>> {
    let tail: Arc<Mutex<VecDeque<String>>> = Arc::default();
    let kept = Arc::clone(&tail);
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            let mut kept = lock(&kept);
            if kept.len() == STDERR_LINES {
                kept.pop_front();
            }
            kept.push_back(line.chars().take(STDERR_LINE_CHARS).collect());
        }
    });
    tail
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{PipeReader, PipeWriter};

    /// A server in a thread, answering each message it reads with whatever
    /// `answer` returns — an empty list for silence, `None` to hang up. Every
    /// message it received is kept, in order, for the test to check.
    struct Fake {
        seen: Arc<Mutex<Vec<Value>>>,
    }

    fn fake(
        timeout: Duration,
        answer: impl Fn(&Value) -> Option<Vec<Value>> + Send + 'static,
    ) -> (Connection, Fake) {
        let (client_reads, server_writes) = std::io::pipe().unwrap();
        let (server_reads, client_writes) = std::io::pipe().unwrap();
        let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
        let log = Arc::clone(&seen);
        std::thread::spawn(move || serve(server_reads, server_writes, log, answer));
        let connection = Connection::new(BufReader::new(client_reads), client_writes, timeout, || McpError::Exited {
            code: Some(3),
            stderr: "gone".into(),
        });
        (connection, Fake { seen })
    }

    fn serve(
        reads: PipeReader,
        mut writes: PipeWriter,
        seen: Arc<Mutex<Vec<Value>>>,
        answer: impl Fn(&Value) -> Option<Vec<Value>>,
    ) {
        for line in BufReader::new(reads).lines() {
            let Ok(line) = line else { return };
            let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
            seen.lock().unwrap().push(message.clone());
            let Some(replies) = answer(&message) else { return };
            for reply in replies {
                // A string starting with RAW: goes out as bare text, the way a
                // server that logs to stdout sends it.
                let line = match reply.as_str().and_then(|r| r.strip_prefix("RAW:")) {
                    Some(raw) => raw.to_string(),
                    None => reply.to_string(),
                };
                if writeln!(writes, "{line}").is_err() {
                    return;
                }
            }
        }
    }

    fn ok(request: &Value, result: Value) -> Option<Vec<Value>> {
        Some(vec![json!({ "jsonrpc": "2.0", "id": request["id"], "result": result })])
    }

    /// A well-behaved server: handshake, two pages of tools, and a call.
    fn well_behaved(request: &Value) -> Option<Vec<Value>> {
        match request["method"].as_str() {
            Some("initialize") => ok(request, json!({ "protocolVersion": "2025-06-18", "capabilities": { "tools": {} }, "serverInfo": { "name": "f" } })),
            Some("tools/list") if request["params"]["cursor"].is_null() => ok(
                request,
                json!({ "tools": [
                    { "name": "search", "description": "Search issues", "inputSchema": { "type": "object", "properties": { "q": { "type": "string" } } } },
                    { "description": "no name, cannot be called" }
                ], "nextCursor": "p2" }),
            ),
            Some("tools/list") => ok(request, json!({ "tools": [{ "name": "open" }] })),
            Some("tools/call") => ok(
                request,
                json!({ "content": [{ "type": "text", "text": format!("called {}", request["params"]["name"]) }], "isError": request["params"]["arguments"]["fail"] == true }),
            ),
            _ => Some(vec![]),
        }
    }

    /// Answers the handshake, then says nothing.
    fn silent_after_handshake(request: &Value) -> Option<Vec<Value>> {
        match request["method"].as_str() {
            Some("initialize" | "notifications/initialized") => well_behaved(request),
            _ => Some(vec![]),
        }
    }

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn the_handshake_comes_first_and_is_acknowledged() {
        let (connection, fake) = fake(SECOND, well_behaved);
        connection.initialize(&|| false).unwrap();
        connection.list_tools().unwrap();

        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen[0]["method"], "initialize");
        assert_eq!(seen[0]["params"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(seen[0]["params"]["clientInfo"]["name"], "laika-agent");
        assert_eq!(seen[1], json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        assert_eq!(seen[2]["method"], "tools/list");
    }

    #[test]
    fn tools_are_listed_across_pages_and_a_nameless_one_is_skipped() {
        let (connection, fake) = fake(SECOND, well_behaved);
        connection.initialize(&|| false).unwrap();
        let tools = connection.list_tools().unwrap();

        assert_eq!(
            tools,
            [
                McpTool {
                    name: "search".into(),
                    description: "Search issues".into(),
                    input_schema: json!({ "type": "object", "properties": { "q": { "type": "string" } } }),
                },
                McpTool { name: "open".into(), description: String::new(), input_schema: json!({ "type": "object" }) },
            ]
        );
        assert_eq!(fake.seen.lock().unwrap()[3]["params"]["cursor"], "p2");
    }

    #[test]
    fn a_call_returns_its_text_and_whether_the_tool_failed() {
        let (connection, fake) = fake(SECOND, well_behaved);
        connection.initialize(&|| false).unwrap();

        let done = connection.call_tool("search", json!({ "q": "bug" }), &|| false).unwrap();
        assert_eq!(done, McpCallResult { text: "called \"search\"".into(), is_error: false });
        let failed = connection.call_tool("search", json!({ "fail": true }), &|| false).unwrap();
        assert!(failed.is_error);
        assert_eq!(fake.seen.lock().unwrap()[2]["params"], json!({ "name": "search", "arguments": { "q": "bug" } }));
    }

    /// The model has to hear it rather than wait on a server that went
    /// quiet — and the server is told, so it can stop too.
    #[test]
    fn a_silent_server_times_out_and_is_told_to_stop() {
        let (connection, fake) = fake(Duration::from_millis(200), silent_after_handshake);
        connection.initialize(&|| false).unwrap();
        let started = Instant::now();

        assert_eq!(connection.call_tool("slow", json!({}), &|| false), Err(McpError::Timeout(0)));
        assert!(started.elapsed() < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(100));
        let seen = fake.seen.lock().unwrap();
        let cancel = seen.last().unwrap();
        assert_eq!(cancel["method"], "notifications/cancelled");
        assert_eq!(cancel["params"]["requestId"], seen[2]["id"]);
    }

    #[test]
    fn a_stop_returns_at_once_and_tells_the_server() {
        let (connection, fake) = fake(Duration::from_secs(30), silent_after_handshake);
        connection.initialize(&|| false).unwrap();
        let started = Instant::now();
        let stop_after = Instant::now() + Duration::from_millis(100);

        assert_eq!(connection.call_tool("slow", json!({}), &|| Instant::now() > stop_after), Err(McpError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(2), "waited out the 30 s instead");
        std::thread::sleep(Duration::from_millis(100));
        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen.last().unwrap()["params"]["reason"], "cancelled by the user");
    }

    /// The server dying mid-call is reported at once, with what the process
    /// layer knows about why — not after the timeout.
    #[test]
    fn a_server_that_goes_away_mid_call_is_reported_at_once() {
        let (connection, _fake) = fake(Duration::from_secs(30), |request| match request["method"].as_str() {
            Some("tools/call") => {
                std::thread::sleep(Duration::from_millis(100));
                None
            }
            _ => well_behaved(request),
        });
        connection.initialize(&|| false).unwrap();
        let started = Instant::now();

        let err = connection.call_tool("slow", json!({}), &|| false).unwrap_err();
        assert_eq!(err, McpError::Exited { code: Some(3), stderr: "gone".into() });
        assert!(started.elapsed() < Duration::from_secs(5));
        // And every call after it, without waiting either.
        assert_eq!(connection.call_tool("again", json!({}), &|| false).unwrap_err(), err);
    }

    /// A server that closed its stdout but still reads its stdin: the write
    /// succeeds, and without the closed flag the call would wait out its
    /// whole timeout for an answer that cannot come.
    #[test]
    fn a_call_after_the_stream_ended_fails_at_once_even_if_the_write_succeeds() {
        let (client_reads, server_writes) = std::io::pipe().unwrap();
        let (server_reads, client_writes) = std::io::pipe().unwrap();
        drop(server_writes);
        std::thread::spawn(move || for _ in BufReader::new(server_reads).lines() {});
        let connection = Connection::new(BufReader::new(client_reads), client_writes, Duration::from_secs(30), || {
            McpError::Exited { code: Some(3), stderr: "gone".into() }
        });
        std::thread::sleep(Duration::from_millis(100));
        let started = Instant::now();

        assert_eq!(
            connection.call_tool("x", json!({}), &|| false),
            Err(McpError::Exited { code: Some(3), stderr: "gone".into() })
        );
        assert!(started.elapsed() < Duration::from_secs(5), "waited out the timeout");
    }

    /// An error answer is the server's, and says so.
    #[test]
    fn an_error_reply_carries_the_servers_code_and_message() {
        let (connection, _fake) = fake(SECOND, |request| match request["method"].as_str() {
            Some("initialize") => well_behaved(request),
            _ => Some(vec![json!({ "jsonrpc": "2.0", "id": request["id"], "error": { "code": -32602, "message": "unknown tool" } })]),
        });
        connection.initialize(&|| false).unwrap();
        assert_eq!(
            connection.call_tool("nope", json!({}), &|| false),
            Err(McpError::Server { code: -32602, message: "unknown tool".into() })
        );
    }

    /// The server's own requests are answered, a log line on stdout is
    /// skipped, and an answer to nobody is dropped — none of it disturbs the
    /// call in flight.
    #[test]
    fn noise_from_the_server_does_not_disturb_a_call() {
        let (connection, fake) = fake(SECOND, |request| match request["method"].as_str() {
            Some("tools/call") => {
                let mut replies = vec![
                    json!({ "jsonrpc": "2.0", "id": "srv-1", "method": "ping" }),
                    json!({ "jsonrpc": "2.0", "id": "srv-2", "method": "roots/list" }),
                    json!({ "jsonrpc": "2.0", "method": "notifications/message", "params": {} }),
                    json!({ "jsonrpc": "2.0", "id": 999, "result": {} }),
                    Value::String("RAW:Server listening on stdio".into()),
                ];
                replies.extend(well_behaved(request)?);
                Some(replies)
            }
            _ => well_behaved(request),
        });
        connection.initialize(&|| false).unwrap();
        assert_eq!(connection.call_tool("search", json!({}), &|| false).unwrap().text, "called \"search\"");

        std::thread::sleep(Duration::from_millis(100));
        let seen = fake.seen.lock().unwrap();
        let answered: HashMap<String, Value> = seen
            .iter()
            .filter(|m| m["id"].is_string())
            .map(|m| (m["id"].as_str().unwrap().to_string(), m.clone()))
            .collect();
        assert_eq!(answered["srv-1"]["result"], json!({}));
        assert_eq!(answered["srv-2"]["error"]["code"], -32601);
    }

    /// A handshake-less server refuses `initialize`; the message says what
    /// that probably means.
    #[test]
    fn a_server_of_the_next_protocol_era_is_named_as_such() {
        let (connection, _fake) = fake(SECOND, |request| {
            Some(vec![json!({ "jsonrpc": "2.0", "id": request["id"], "error": { "code": -32022, "message": "Unsupported protocol version" } })])
        });
        let err = connection.initialize(&|| false).unwrap_err();
        assert!(matches!(&err, McpError::Handshake(m) if m.contains("2026-07-28")), "{err}");

        let (connection, _fake) = fake(SECOND, |request| {
            Some(vec![json!({ "jsonrpc": "2.0", "id": request["id"], "error": { "code": -32000, "message": "no token" } })])
        });
        let err = connection.initialize(&|| false).unwrap_err();
        assert!(matches!(&err, McpError::Handshake(m) if m == "no token (code -32000)"), "{err}");
    }

    /// A server that hung up is known gone without a call to find out.
    #[test]
    fn a_connection_whose_server_hung_up_is_not_alive() {
        let (connection, _fake) = fake(SECOND, |request| match request["method"].as_str() {
            Some("tools/list") => None,
            _ => well_behaved(request),
        });
        connection.initialize(&|| false).unwrap();
        assert!(connection.is_alive());
        connection.list_tools().unwrap_err();
        assert!(!connection.is_alive());
    }

    #[test]
    fn an_initialize_answer_without_a_version_is_a_protocol_error() {
        let (connection, _fake) = fake(SECOND, |request| ok(request, json!({})));
        assert!(matches!(connection.initialize(&|| false), Err(McpError::Protocol(_))));
    }

    // ------------------------------------------------------- a real process

    fn sh(script: &str, timeout_secs: u64) -> McpServerConfig {
        McpServerConfig {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            timeout_secs: Some(timeout_secs),
            ..Default::default()
        }
    }

    /// The whole path through a process: spawn, pipes, handshake, a call —
    /// a server written in `sh`, reading one line per request.
    #[cfg(unix)]
    #[test]
    fn a_process_is_started_and_spoken_to() {
        let script = r#"
            read init; echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{}}}'
            read initialized
            read call; echo '{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"hello '"$MCP_TEST_TOKEN"'"}]}}'
            read _
        "#;
        let dir = crate::testing::temp_dir("mcp-stdio-process");
        // `env` is where a server's token lives; it has to reach the process.
        let config = McpServerConfig { env: [("MCP_TEST_TOKEN".to_string(), "t0k3n".to_string())].into(), ..sh(script, 5) };
        let server = StdioServer::start(&config, &dir, &|| false).unwrap();
        assert_eq!(server.call_tool("hi", json!({}), &|| false).unwrap().text, "hello t0k3n");
    }

    /// Why an exit is worth its own variant: the code and the last lines of
    /// stderr are usually the whole explanation.
    #[cfg(unix)]
    #[test]
    fn a_process_that_dies_reports_its_code_and_stderr() {
        let dir = crate::testing::temp_dir("mcp-stdio-dies");
        let result = StdioServer::start(&sh("echo 'Error: GITHUB_TOKEN is not set' >&2; exit 7", 5), &dir, &|| false);
        let Err(err) = result else { panic!("started") };
        assert_eq!(err, McpError::Exited { code: Some(7), stderr: "Error: GITHUB_TOKEN is not set".into() });
    }

    /// Only the end of a long stderr is kept — enough to explain an exit,
    /// not a whole log held in memory.
    #[cfg(unix)]
    #[test]
    fn only_the_last_lines_of_stderr_are_kept() {
        let dir = crate::testing::temp_dir("mcp-stdio-tail");
        let result = StdioServer::start(&sh("for i in $(seq 1 30); do echo line $i >&2; done; exit 1", 5), &dir, &|| false);
        let Err(McpError::Exited { stderr, .. }) = result else { panic!("expected an exit") };
        let lines: Vec<&str> = stderr.lines().collect();
        assert_eq!(lines.len(), STDERR_LINES);
        assert_eq!((lines[0], lines[STDERR_LINES - 1]), ("line 11", "line 30"));
    }

    /// What the restart in `services::mcp_servers` goes by: a server that
    /// died on a call is known dead before the next one is sent.
    #[cfg(unix)]
    #[test]
    fn a_process_that_exits_on_a_call_is_no_longer_alive() {
        let script = r#"
            read init; echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25"}}'
            read initialized
            read call; echo 'crashed' >&2; exit 2
        "#;
        let dir = crate::testing::temp_dir("mcp-stdio-alive");
        let server = StdioServer::start(&sh(script, 5), &dir, &|| false).unwrap();
        assert!(server.is_alive());
        let err = server.call_tool("hi", json!({}), &|| false).unwrap_err();
        assert_eq!(err, McpError::Exited { code: Some(2), stderr: "crashed".into() });
        assert!(!server.is_alive());
    }

    /// Its stdout still open — held by a child it left behind — while the
    /// server itself is gone.
    #[cfg(unix)]
    #[test]
    fn a_server_whose_process_exited_is_not_alive_while_its_stream_stays_open() {
        let script = r#"
            read init; echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25"}}'
            read initialized
            sleep 300 &
            exit 0
        "#;
        let dir = crate::testing::temp_dir("mcp-stdio-orphan");
        let server = StdioServer::start(&sh(script, 5), &dir, &|| false).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while server.is_alive() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!server.is_alive());
        assert!(server.connection.is_alive(), "the stream alone would not have told");
    }

    #[test]
    fn a_command_that_does_not_exist_is_not_started() {
        let dir = crate::testing::temp_dir("mcp-stdio-missing");
        let config = McpServerConfig { command: "definitely-not-a-command-4f2a".into(), ..Default::default() };
        assert!(matches!(StdioServer::start(&config, &dir, &|| false), Err(McpError::NotStarted(m)) if m.contains("definitely-not")));
    }

    /// Dropping the server takes its children with it: `npx` is only the
    /// parent of the real server.
    #[cfg(unix)]
    #[test]
    fn dropping_the_server_kills_what_it_started() {
        let dir = crate::testing::temp_dir("mcp-stdio-drop");
        let pid_file = dir.join("child.pid");
        let script = format!(
            r#"sleep 300 & echo $! > {}
            read init; echo '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-11-25"}}}}'
            read _; wait"#,
            pid_file.display()
        );
        let server = StdioServer::start(&sh(&script, 5), &dir, &|| false).unwrap();
        let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, 0, "the grandchild runs");

        drop(server);
        let deadline = Instant::now() + Duration::from_secs(3);
        while unsafe { libc::kill(pid, 0) } == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "the grandchild outlived the server");
    }
}
