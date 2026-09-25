//! An MCP server reached at a URL: the Streamable HTTP transport — one
//! endpoint, a POST per message, the answer in the response as JSON or as an
//! event stream.
//!
//! The protocol itself is `infra::mcp_stdio::Connection`'s; this module only
//! moves messages, over `ureq` like the model provider — `rmcp`'s HTTP client
//! would bring `reqwest` and `hyper`, which the data policy refuses
//! (`docs/17-mcp-http.md`).
//!
//! Left out on purpose, each named in that document: OAuth (a 401 says so),
//! the old HTTP+SSE transport, the GET stream for the server's own
//! notifications, and resuming a broken stream.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use crate::domain::mcp::{McpCallResult, McpClient, McpError, McpServerConfig, McpTool};
use crate::infra::mcp_stdio::{Connection, Inbox, Outbox};

/// Enough of an error body to explain it; a server that answers an error
/// with a whole HTML page is cut here.
const ERROR_BODY_CHARS: usize = 1000;
/// Ending a session is a courtesy, not worth holding anything up for.
const DELETE_TIMEOUT: Duration = Duration::from_secs(5);

/// A session with a server at a URL. Dropping it ends the session on the
/// server too, if the server keeps sessions.
pub struct HttpServer {
    connection: Connection,
    http: Arc<Http>,
}

struct Http {
    agent: ureq::Agent,
    url: String,
    headers: BTreeMap<String, String>,
    timeout: Duration,
    /// `Mcp-Session-Id`, when the server gave one — sent with every message
    /// after the handshake.
    session: Mutex<Option<String>>,
    /// The version the server agreed to, sent with every message after the
    /// handshake, as the transport requires.
    version: Mutex<Option<String>>,
    inbox: Inbox,
    /// Why the conversation ended, for every call after it.
    ended: Mutex<Option<McpError>>,
}

impl HttpServer {
    /// Completes the handshake within the server's own timeout.
    pub fn start(config: &McpServerConfig, cancelled: &dyn Fn() -> bool) -> Result<Self, McpError> {
        let url = config.url.clone().ok_or_else(|| McpError::NotStarted("the entry has no url".into()))?;
        let agent = crate::infra::http_agent::build_agent(None).map_err(|e| McpError::NotStarted(e.to_string()))?;
        let timeout = Duration::from_secs(config.timeout_secs());
        let http = Arc::new(Http {
            agent,
            url,
            headers: config.headers.clone(),
            timeout,
            session: Mutex::default(),
            version: Mutex::default(),
            inbox: Inbox::default(),
            ended: Mutex::default(),
        });
        let sender = Arc::clone(&http);
        let send: Outbox = Arc::new(move |message| {
            sender.send(message);
            Ok(())
        });
        let ended = Arc::clone(&http);
        let connection = Connection::with_transport(send, http.inbox.clone(), timeout, move || {
            lock(&ended.ended).clone().unwrap_or_else(|| McpError::Unreachable("the connection ended".into()))
        });
        connection.initialize(cancelled)?;
        Ok(Self { connection, http })
    }
}

impl McpClient for HttpServer {
    fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
        self.connection.list_tools()
    }

    fn call_tool(&self, name: &str, arguments: Value, cancelled: &dyn Fn() -> bool) -> Result<McpCallResult, McpError> {
        self.connection.call_tool(name, arguments, cancelled)
    }

    fn is_alive(&self) -> bool {
        self.connection.is_alive()
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        let Some(session) = lock(&self.http.session).clone() else { return };
        let http = Arc::clone(&self.http);
        std::thread::spawn(move || {
            let mut delete = http
                .agent
                .delete(&http.url)
                .config()
                .timeout_global(Some(DELETE_TIMEOUT))
                .build()
                .header("Mcp-Session-Id", &session);
            for (name, value) in &http.headers {
                delete = delete.header(name, value);
            }
            let _ = delete.call();
        });
    }
}

impl Http {
    /// Every message goes on its own thread, so a call waiting for its
    /// answer still sees a Stop — except the handshake's acknowledgement,
    /// which has to reach the server before the requests sent right after it.
    fn send(self: &Arc<Self>, message: &Value) {
        if message["method"] == "notifications/initialized" {
            self.exchange(message);
            return;
        }
        let http = Arc::clone(self);
        let message = message.clone();
        std::thread::spawn(move || http.exchange(&message));
    }

    /// A request's failure goes to whoever waits for it; a notification's or
    /// a reply's has nobody to go to.
    fn exchange(&self, message: &Value) {
        let request = message["method"].as_str().zip(message["id"].as_u64());
        if let Err(error) = self.post(message, request) {
            if let Some((_, id)) = request {
                self.inbox.fail(id, error);
            }
        }
    }

    /// Sends one message and hands what comes back to the inbox. An error
    /// that ends the whole conversation ends it here and returns `Ok`: every
    /// waiter hears it through the inbox.
    fn post(&self, message: &Value, request: Option<(&str, u64)>) -> Result<(), McpError> {
        let mut post = self
            .agent
            .post(&self.url)
            .config()
            .timeout_global(Some(self.timeout))
            .build()
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");
        for (name, value) in &self.headers {
            post = post.header(name, value);
        }
        if let Some(session) = lock(&self.session).clone() {
            post = post.header("Mcp-Session-Id", session);
        }
        if let Some(version) = lock(&self.version).clone() {
            post = post.header("MCP-Protocol-Version", version);
        }
        let mut response = match post.send(message.to_string()) {
            Ok(response) => response,
            // This request took too long; the server may still be fine.
            Err(ureq::Error::Timeout(_)) => return Err(McpError::Timeout(self.timeout.as_secs())),
            Err(e) => {
                self.end(McpError::Unreachable(e.to_string()));
                return Ok(());
            }
        };
        if let Some(session) = response.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()) {
            *lock(&self.session) = Some(session.to_string());
        }

        let status = response.status().as_u16();
        if !response.status().is_success() {
            let body = response.body_mut().read_to_string().unwrap_or_default();
            let error = McpError::Http { status, body: body.chars().take(ERROR_BODY_CHARS).collect() };
            // The server forgot the session: nothing sent in it will be
            // understood again, and the next call starts a new one.
            if status == 404 && lock(&self.session).is_some() {
                self.end(error);
                return Ok(());
            }
            return Err(error);
        }
        let Some((method, id)) = request else { return Ok(()) };
        if status == 202 {
            return Err(McpError::Protocol(format!("{method} was accepted, but no answer came with it")));
        }

        let is_stream = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"));
        if is_stream {
            let reader = BufReader::new(response.into_body().into_reader());
            return self.read_stream(reader, method, id);
        }
        let body = response.body_mut().read_to_string().map_err(|e| McpError::Protocol(e.to_string()))?;
        let answer = serde_json::from_str(&body)
            .map_err(|e| McpError::Protocol(format!("{method} was answered with something that is not JSON: {e}")))?;
        if self.take(&answer, id) {
            Ok(())
        } else {
            Err(McpError::Protocol(format!("{method} was answered, but not with its answer: {answer}")))
        }
    }

    /// Server-sent events until the one that answers `id`, and not a line
    /// further: a server may hold the stream open after it. `data:` lines
    /// join into one message and a blank line ends it; event names, ids and
    /// comments are not needed. The lines are joined with nothing rather than
    /// the line break the event format puts between them — JSON needs no
    /// separator between its tokens and allows no raw break inside a string,
    /// so for a JSON payload the two are the same.
    fn read_stream(&self, reader: impl BufRead, method: &str, id: u64) -> Result<(), McpError> {
        let mut data = String::new();
        for line in reader.lines() {
            // `lines` takes a CRLF off as well as an LF.
            let Ok(line) = line else { break };
            if let Some(value) = line.strip_prefix("data:") {
                data.push_str(value);
            } else if line.is_empty() {
                if let Ok(message) = serde_json::from_str::<Value>(&std::mem::take(&mut data)) {
                    if self.take(&message, id) {
                        return Ok(());
                    }
                }
            }
        }
        Err(McpError::Protocol(format!("the event stream for {method} ended without its answer")))
    }

    /// Hands one message to the inbox and says whether it was the answer to
    /// `id` — not a request of the server's own, whose ids are its own and
    /// may be the same number. That request is answered in a message of its
    /// own.
    fn take(&self, message: &Value, id: u64) -> bool {
        let answers = message.get("method").is_none() && message["id"].as_u64() == Some(id);
        // Only `initialize` answers with a version. Kept before the inbox
        // hands the answer on: the waiter sends the next message at once, and
        // that one already carries it.
        if answers {
            if let Some(version) = message["result"]["protocolVersion"].as_str() {
                *lock(&self.version) = Some(version.to_string());
            }
        }
        if let Some(reply) = self.inbox.deliver(message) {
            let _ = self.post(&reply, None);
        }
        answers
    }

    fn end(&self, error: McpError) {
        *lock(&self.ended) = Some(error);
        self.inbox.close();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Instant;

    /// One request as the fake server read it. `CLOSED` is not a request:
    /// the client hung up on a connection held open (status 1).
    #[derive(Debug, Clone)]
    struct Seen {
        at: Instant,
        method: String,
        headers: HashMap<String, String>,
        body: Value,
    }

    /// Status 0 hangs up without a word; 1 holds the connection open, saying
    /// nothing, until the client closes it.
    struct Answer {
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: String,
    }

    type Log = Arc<Mutex<Vec<Seen>>>;

    /// A server on a free local port, answering each request with `answer`
    /// on a thread of its own — a slow answer holds up nothing else.
    fn serve(answer: impl Fn(&Seen) -> Answer + Send + Sync + 'static) -> (String, Log) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/mcp", listener.local_addr().unwrap().port());
        let log: Log = Arc::default();
        let seen = Arc::clone(&log);
        let answer = Arc::new(answer);
        std::thread::spawn(move || {
            for socket in listener.incoming() {
                let Ok(mut socket) = socket else { return };
                let (answer, seen) = (Arc::clone(&answer), Arc::clone(&seen));
                std::thread::spawn(move || {
                    let Some(request) = read_request(&mut socket) else { return };
                    seen.lock().unwrap().push(request.clone());
                    let answer = answer(&request);
                    if answer.status == 0 {
                        return;
                    }
                    if answer.status == 1 {
                        let _ = socket.set_read_timeout(Some(Duration::from_secs(10)));
                        if matches!(socket.read(&mut [0; 1]), Ok(0)) {
                            seen.lock().unwrap().push(Seen { method: "CLOSED".into(), ..request });
                        }
                        return;
                    }
                    let mut head = format!("HTTP/1.1 {} X\r\n", answer.status);
                    for (name, value) in &answer.headers {
                        head.push_str(&format!("{name}: {value}\r\n"));
                    }
                    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", answer.body.len()));
                    let _ = socket.write_all(head.as_bytes());
                    let _ = socket.write_all(answer.body.as_bytes());
                });
            }
        });
        (url, log)
    }

    fn read_request(socket: &mut TcpStream) -> Option<Seen> {
        let mut reader = BufReader::new(socket.try_clone().ok()?);
        let mut first = String::new();
        reader.read_line(&mut first).ok()?;
        let method = first.split_whitespace().next()?.to_string();
        let mut headers = HashMap::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).ok()?;
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        }
        let length = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).ok()?;
        Some(Seen { at: Instant::now(), method, headers, body: serde_json::from_slice(&body).unwrap_or(Value::Null) })
    }

    fn json_answer(request: &Seen, result: Value) -> Answer {
        Answer {
            status: 200,
            headers: vec![("Content-Type", "application/json".into())],
            body: json!({ "jsonrpc": "2.0", "id": request.body["id"], "result": result }).to_string(),
        }
    }

    fn stream(body: String) -> Answer {
        Answer { status: 200, headers: vec![("Content-Type", "text/event-stream".into())], body }
    }

    fn events(messages: &[Value]) -> Answer {
        stream(messages.iter().map(|m| format!("event: message\ndata: {m}\n\n")).collect())
    }

    fn status(status: u16, body: &str) -> Answer {
        Answer { status, headers: vec![], body: body.into() }
    }

    fn handshake(request: &Seen, session: Option<&str>) -> Answer {
        let mut answer = json_answer(request, json!({ "protocolVersion": "2025-06-18", "capabilities": { "tools": {} } }));
        if let Some(session) = session {
            answer.headers.push(("Mcp-Session-Id", session.into()));
        }
        answer
    }

    /// A server that keeps a session: the handshake as JSON, the tool list
    /// as an event split over several `data:` lines, a call as a stream that
    /// pings the client before answering.
    fn well_behaved(request: &Seen) -> Answer {
        if request.method == "DELETE" {
            return status(200, "");
        }
        let id = &request.body["id"];
        match request.body["method"].as_str() {
            Some("initialize") => handshake(request, Some("s-1")),
            Some("tools/list") => stream(format!(
                ": keep-alive\r\nevent: message\r\ndata: {{\"jsonrpc\": \"2.0\", \"id\": {id},\r\ndata: \"result\": {{\"tools\": [{{\"name\": \"search\"}}]}}}}\r\n\r\n"
            )),
            // The server's ids are its own: its ping may carry the very
            // number of the call it answers.
            Some("tools/call") => events(&[
                json!({ "jsonrpc": "2.0", "id": id, "method": "ping" }),
                json!({ "jsonrpc": "2.0", "method": "notifications/progress", "params": {} }),
                json!({ "jsonrpc": "2.0", "id": id, "result": { "content": [{ "type": "text", "text": "found it" }] } }),
                json!({ "jsonrpc": "2.0", "id": "srv-late", "method": "ping" }),
            ]),
            _ => status(202, ""),
        }
    }

    fn config(url: &str, timeout_secs: u64) -> McpServerConfig {
        McpServerConfig {
            url: Some(url.into()),
            headers: [("Authorization".to_string(), "Bearer t0k3n".to_string())].into(),
            timeout_secs: Some(timeout_secs),
            ..Default::default()
        }
    }

    /// Waits for what a background thread will do; a test does not sleep
    /// its way to it.
    fn eventually(log: &Log, found: impl Fn(&Seen) -> bool) -> Seen {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(seen) = log.lock().unwrap().iter().find(|s| found(s)) {
                return seen.clone();
            }
            assert!(Instant::now() < deadline, "never arrived: {:#?}", log.lock().unwrap());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn position(log: &Log, method: &str) -> usize {
        log.lock().unwrap().iter().position(|s| s.body["method"] == method).unwrap()
    }

    #[test]
    fn a_session_is_started_used_and_ended() {
        let (url, log) = serve(well_behaved);
        let server = HttpServer::start(&config(&url, 5), &|| false).unwrap();
        assert_eq!(server.list_tools().unwrap()[0].name, "search", "an event over several data: lines");
        assert_eq!(server.call_tool("search", json!({ "q": "x" }), &|| false).unwrap().text, "found it");

        let seen = log.lock().unwrap().clone();
        let init = &seen[0];
        assert_eq!(init.body["method"], "initialize");
        assert_eq!(init.headers["accept"], "application/json, text/event-stream");
        assert_eq!(init.headers["content-type"], "application/json");
        assert_eq!(init.headers["authorization"], "Bearer t0k3n");
        assert!(!init.headers.contains_key("mcp-session-id") && !init.headers.contains_key("mcp-protocol-version"));
        for later in &seen[1..] {
            assert_eq!(later.headers["mcp-session-id"], "s-1", "{later:?}");
            assert_eq!(later.headers["mcp-protocol-version"], "2025-06-18", "{later:?}");
            assert_eq!(later.headers["authorization"], "Bearer t0k3n");
        }
        assert!(position(&log, "notifications/initialized") < position(&log, "tools/list"), "acknowledged first");

        let call = eventually(&log, |s| s.body["method"] == "tools/call");
        let pong = eventually(&log, |s| s.body["id"] == call.body["id"] && s.body.get("method").is_none());
        assert_eq!(pong.body["result"], json!({}), "the server's ping is answered");
        assert_eq!(pong.headers["mcp-session-id"], "s-1");

        drop(server);
        let delete = eventually(&log, |s| s.method == "DELETE");
        assert!(
            log.lock().unwrap().iter().all(|s| s.body["id"] != "srv-late"),
            "the stream is left once its answer is in"
        );
        assert_eq!((delete.headers["mcp-session-id"].as_str(), delete.headers["authorization"].as_str()), ("s-1", "Bearer t0k3n"));
    }

    /// The server may refuse what comes before the acknowledgement, so the
    /// next request waits for it to be taken — even when taking it is slow.
    #[test]
    fn nothing_is_sent_before_the_handshake_is_acknowledged() {
        let (url, log) = serve(|request| match request.body["method"].as_str() {
            Some("notifications/initialized") => {
                std::thread::sleep(Duration::from_millis(300));
                status(202, "")
            }
            _ => well_behaved(request),
        });
        let server = HttpServer::start(&config(&url, 5), &|| false).unwrap();
        server.list_tools().unwrap();
        let at = |method: &str| {
            let index = position(&log, method);
            log.lock().unwrap()[index].at
        };
        assert!(at("tools/list") >= at("notifications/initialized") + Duration::from_millis(300));
    }

    /// No session, nothing to end.
    #[test]
    fn a_server_without_sessions_is_sent_none_and_not_told_goodbye() {
        let (url, log) = serve(|request| match request.body["method"].as_str() {
            Some("initialize") => handshake(request, None),
            _ => well_behaved(request),
        });
        let server = HttpServer::start(&config(&url, 5), &|| false).unwrap();
        server.list_tools().unwrap();
        drop(server);
        std::thread::sleep(Duration::from_millis(200));
        let seen = log.lock().unwrap();
        assert!(seen.iter().all(|s| !s.headers.contains_key("mcp-session-id") && s.method == "POST"), "{seen:#?}");
    }

    #[test]
    fn a_server_that_wants_a_sign_in_does_not_start_and_says_why() {
        let (url, _log) = serve(|_| status(401, "missing bearer token"));
        let Err(err) = HttpServer::start(&config(&url, 5), &|| false) else { panic!("started") };
        assert_eq!(err, McpError::Http { status: 401, body: "missing bearer token".into() });
        assert!(err.to_string().contains("OAuth"));
    }

    /// One refused call is that call's failure, not the server's.
    #[test]
    fn an_error_status_fails_the_call_and_keeps_the_session() {
        let (url, _log) = serve(|request| match request.body["method"].as_str() {
            Some("tools/call") => status(500, &"x".repeat(5000)),
            _ => well_behaved(request),
        });
        let server = HttpServer::start(&config(&url, 5), &|| false).unwrap();
        let err = server.call_tool("search", json!({}), &|| false).unwrap_err();
        assert_eq!(err, McpError::Http { status: 500, body: "x".repeat(ERROR_BODY_CHARS) }, "the body is cut");
        assert!(server.is_alive());
        assert_eq!(server.list_tools().unwrap().len(), 1);
    }

    /// The server forgot the session: this call fails, the server counts as
    /// gone, and the restart in `services::mcp_servers` starts a new one.
    #[test]
    fn a_forgotten_session_ends_the_connection() {
        let (url, _log) = serve(|request| match request.body["method"].as_str() {
            Some("tools/call") => status(404, "unknown session"),
            _ => well_behaved(request),
        });
        let server = HttpServer::start(&config(&url, 30), &|| false).unwrap();
        let gone = McpError::Http { status: 404, body: "unknown session".into() };
        assert_eq!(server.call_tool("search", json!({}), &|| false).unwrap_err(), gone);
        assert!(!server.is_alive());
        let started = Instant::now();
        assert_eq!(server.list_tools().unwrap_err(), gone, "and every call after it");
        assert!(started.elapsed() < Duration::from_secs(5), "without waiting");
    }

    /// Without a session, a 404 is the call's own — the tool, say, is
    /// behind a path that is not there.
    #[test]
    fn a_404_without_a_session_is_the_calls_failure() {
        let (url, _log) = serve(|request| match request.body["method"].as_str() {
            Some("initialize") => handshake(request, None),
            Some("tools/call") => status(404, "not here"),
            _ => well_behaved(request),
        });
        let server = HttpServer::start(&config(&url, 5), &|| false).unwrap();
        assert!(matches!(server.call_tool("search", json!({}), &|| false), Err(McpError::Http { status: 404, .. })));
        assert!(server.is_alive());
    }

    /// Gone mid-session: this call fails, and so does every one after it
    /// until the restart.
    #[test]
    fn a_server_that_hangs_up_ends_the_connection() {
        let (url, _log) = serve(|request| match request.body["method"].as_str() {
            Some("tools/call") => status(0, ""),
            _ => well_behaved(request),
        });
        let server = HttpServer::start(&config(&url, 30), &|| false).unwrap();
        let err = server.call_tool("search", json!({}), &|| false).unwrap_err();
        assert!(matches!(err, McpError::Unreachable(_)), "{err}");
        assert!(!server.is_alive());
        assert_eq!(server.list_tools().unwrap_err(), err);
    }

    #[test]
    fn a_server_nobody_listens_for_is_unreachable() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let result = HttpServer::start(&config(&format!("http://127.0.0.1:{port}/mcp"), 5), &|| false);
        assert!(matches!(result, Err(McpError::Unreachable(_))), "{:?}", result.err());
    }

    #[test]
    fn a_stop_returns_at_once_and_tells_the_server() {
        let (url, log) = serve(|request| match request.body["method"].as_str() {
            Some("tools/call") => {
                std::thread::sleep(Duration::from_secs(3));
                well_behaved(request)
            }
            _ => well_behaved(request),
        });
        let server = HttpServer::start(&config(&url, 30), &|| false).unwrap();
        let started = Instant::now();
        let stop_after = started + Duration::from_millis(100);
        assert_eq!(server.call_tool("slow", json!({}), &|| Instant::now() > stop_after), Err(McpError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(2), "waited for the server instead");

        let call = eventually(&log, |s| s.body["method"] == "tools/call");
        let cancel = eventually(&log, |s| s.body["method"] == "notifications/cancelled");
        assert_eq!(cancel.body["params"]["requestId"], call.body["id"]);
        assert_eq!(cancel.headers["mcp-session-id"], "s-1");
    }

    /// A server that never answers does not hold a thread and a connection
    /// here forever: the request gives up with the call.
    #[test]
    fn a_request_nobody_answers_is_given_up() {
        let (url, log) = serve(|request| match request.body["method"].as_str() {
            Some("tools/call") => status(1, ""),
            _ => well_behaved(request),
        });
        let server = HttpServer::start(&config(&url, 1), &|| false).unwrap();
        assert_eq!(server.call_tool("hang", json!({}), &|| false), Err(McpError::Timeout(1)));
        let closed = eventually(&log, |s| s.method == "CLOSED");
        assert_eq!(closed.body["method"], "tools/call");
    }

    /// A slow answer is a failed call, not a dead server.
    #[test]
    fn a_slow_server_times_out_and_stays() {
        let (url, _log) = serve(|request| match request.body["method"].as_str() {
            Some("tools/call") => {
                std::thread::sleep(Duration::from_secs(3));
                well_behaved(request)
            }
            _ => well_behaved(request),
        });
        let server = HttpServer::start(&config(&url, 1), &|| false).unwrap();
        let started = Instant::now();
        assert_eq!(server.call_tool("slow", json!({}), &|| false), Err(McpError::Timeout(1)));
        assert!(started.elapsed() < Duration::from_millis(2500));
        std::thread::sleep(Duration::from_millis(500));
        assert!(server.is_alive(), "the request's own timeout does not end the session");
    }

    /// Not an answer the call then waits out its timeout for — whether the
    /// stream ends without it, the server only accepted the request, or its
    /// JSON is something else.
    #[test]
    fn a_response_without_the_answer_fails_the_call_at_once() {
        let (url, _log) = serve(|request| match request.body["method"].as_str() {
            Some("tools/call") if request.body["params"]["name"] == "json" => Answer {
                status: 200,
                headers: vec![("Content-Type", "application/json".into())],
                body: json!({ "jsonrpc": "2.0", "method": "notifications/message", "params": {} }).to_string(),
            },
            Some("tools/call") => events(&[json!({ "jsonrpc": "2.0", "method": "notifications/progress", "params": {} })]),
            Some("tools/list") => status(202, ""),
            _ => well_behaved(request),
        });
        let server = HttpServer::start(&config(&url, 30), &|| false).unwrap();
        let started = Instant::now();
        let err = server.call_tool("search", json!({}), &|| false).unwrap_err();
        assert!(matches!(&err, McpError::Protocol(m) if m.contains("ended without its answer")), "{err}");
        let err = server.list_tools().unwrap_err();
        assert!(matches!(&err, McpError::Protocol(m) if m.contains("accepted")), "{err}");
        let err = server.call_tool("json", json!({}), &|| false).unwrap_err();
        assert!(matches!(&err, McpError::Protocol(m) if m.contains("not with its answer")), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// An error answer in JSON is the server's own, with its code.
    #[test]
    fn an_error_answer_in_json_carries_the_servers_code() {
        let (url, _log) = serve(|request| match request.body["method"].as_str() {
            Some("tools/call") => Answer {
                status: 200,
                headers: vec![("Content-Type", "application/json".into())],
                body: json!({ "jsonrpc": "2.0", "id": request.body["id"], "error": { "code": -32602, "message": "unknown tool" } })
                    .to_string(),
            },
            _ => well_behaved(request),
        });
        let server = HttpServer::start(&config(&url, 5), &|| false).unwrap();
        assert_eq!(
            server.call_tool("nope", json!({}), &|| false),
            Err(McpError::Server { code: -32602, message: "unknown tool".into() })
        );
    }
}
