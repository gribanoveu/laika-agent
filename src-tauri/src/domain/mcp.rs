//! MCP servers as the user configures them.
//!
//! The format is the `mcpServers` object Claude Desktop and Cursor use, so a
//! configuration copied from a server's README pastes in unchanged. Fields
//! this app adds (`weight`, `timeoutSecs`) sit beside the standard ones, and
//! fields it does not know — `type`, another client's own — are kept rather
//! than dropped on the next save.
//!
//! Decisions behind this (names, approval, failure, weight, log) are in
//! `docs/06-port-plan.md`, stage 7, "Решения по MCP".

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

/// Loop weight of one call when the server does not say: as much as a
/// `grep`. Nothing is known about what a foreign tool costs, and the
/// iteration cap catches a weight that turns out wrong.
pub const DEFAULT_WEIGHT: u32 = 3;
pub const DEFAULT_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConfig {
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    /// Empty for an HTTP server's entry, which has a `url` instead — kept in
    /// `extra` and reported, not refused, so pasting a mixed config still
    /// loads the servers this build can run.
    #[serde(default)]
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// Cline's and Cursor's spelling for a server kept but not started.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl McpServerConfig {
    pub fn weight(&self) -> u32 {
        self.weight.unwrap_or(DEFAULT_WEIGHT)
    }

    pub fn timeout_secs(&self) -> u64 {
        self.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS)
    }
}

/// One row of the MCP tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerItem {
    pub name: String,
    /// The command line as it will run, for reading — not for running.
    pub command: String,
    pub enabled: bool,
    /// Why this server will not start, when that is already known from its
    /// entry alone.
    pub error: Option<String>,
}

#[derive(Debug, Error)]
pub enum McpConfigError {
    #[error("could not read the MCP configuration: {0}")]
    Read(String),
    #[error("the MCP configuration is not valid: {0}")]
    Parse(String),
    #[error("could not write the MCP configuration: {0}")]
    Write(String),
    #[error("no MCP server named {0:?}")]
    NotFound(String),
}

/// The server's part of a tool name, `mcp__<key>__<tool>`: what the
/// providers accept in a name (`[A-Za-z0-9_-]`), lowercased so two spellings
/// of one server cannot become two prefixes.
pub fn server_key(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '_' })
        .collect()
}

/// The rows of the tab, in name order, each with what is wrong with it.
pub fn items(config: &McpConfig) -> Vec<McpServerItem> {
    let mut seen: HashMap<String, &str> = HashMap::new();
    config
        .mcp_servers
        .iter()
        .map(|(name, server)| {
            let key = server_key(name);
            let clash = seen.get(key.as_str()).map(|first| {
                format!("clashes with \"{first}\": both become \"{key}\" in tool names — rename one")
            });
            seen.entry(key).or_insert(name);
            McpServerItem {
                name: name.clone(),
                command: command_line(server),
                enabled: !server.disabled,
                error: clash.or_else(|| problem(name, server)),
            }
        })
        .collect()
}

fn problem(name: &str, server: &McpServerConfig) -> Option<String> {
    if name.trim().is_empty() {
        return Some("the server needs a name".into());
    }
    if server.command.trim().is_empty() {
        return Some(if server.extra.contains_key("url") {
            "HTTP servers are not supported yet — only ones started by a command".into()
        } else {
            "no command to start it with".into()
        });
    }
    if server.weight == Some(0) {
        return Some("weight must be at least 1".into());
    }
    if server.timeout_secs == Some(0) {
        return Some("timeoutSecs must be at least 1".into());
    }
    None
}

fn command_line(server: &McpServerConfig) -> String {
    std::iter::once(server.command.as_str())
        .chain(server.args.iter().map(String::as_str))
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parses the file's text. The whole file is refused only when it is not
/// JSON of this shape; a server that cannot run is a row with an error.
pub fn parse(text: &str) -> Result<McpConfig, McpConfigError> {
    if text.trim().is_empty() {
        return Ok(McpConfig::default());
    }
    serde_json::from_str(text).map_err(|e| McpConfigError::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A README's snippet, verbatim — the reason for this format.
    #[test]
    fn a_claude_desktop_config_reads_as_is() {
        let config = parse(
            r#"{"mcpServers": {"github": {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"],
                "env": {"GITHUB_PERSONAL_ACCESS_TOKEN": "t"}}}}"#,
        )
        .unwrap();
        let github = &config.mcp_servers["github"];
        assert_eq!(github.args, ["-y", "@modelcontextprotocol/server-github"]);
        assert_eq!(github.env["GITHUB_PERSONAL_ACCESS_TOKEN"], "t");
        assert_eq!((github.weight(), github.timeout_secs()), (DEFAULT_WEIGHT, DEFAULT_TIMEOUT_SECS));
        assert_eq!(
            items(&config),
            [McpServerItem {
                name: "github".into(),
                command: "npx -y @modelcontextprotocol/server-github".into(),
                enabled: true,
                error: None,
            }]
        );
    }

    /// Another client's fields survive a save made here.
    #[test]
    fn unknown_fields_are_kept() {
        let text = r#"{"mcpServers":{"a":{"command":"x","type":"stdio","autoApprove":["t"]}}}"#;
        let config = parse(text).unwrap();
        let written: Value = serde_json::to_value(&config).unwrap();
        assert_eq!(written["mcpServers"]["a"]["type"], "stdio");
        assert_eq!(written["mcpServers"]["a"]["autoApprove"], serde_json::json!(["t"]));
        assert!(written["mcpServers"]["a"].get("disabled").is_none(), "defaults are not written out");
    }

    /// A mixed config loads; the server this build cannot run says why.
    #[test]
    fn a_server_that_cannot_run_is_a_row_with_the_reason() {
        let config = parse(
            r#"{"mcpServers":{
                "remote":{"url":"https://example.com/mcp"},
                "nothing":{"args":["--flag"]},
                "free":{"command":"x","weight":0},
                "instant":{"command":"x","timeoutSecs":0},
                "off":{"command":"x","disabled":true}
            }}"#,
        )
        .unwrap();
        let rows: BTreeMap<String, McpServerItem> =
            items(&config).into_iter().map(|i| (i.name.clone(), i)).collect();
        assert!(rows["remote"].error.as_deref().unwrap().contains("HTTP"));
        assert!(rows["nothing"].error.as_deref().unwrap().contains("no command"));
        assert_eq!(rows["nothing"].command, "--flag", "no leading space for the missing command");
        assert!(rows["free"].error.as_deref().unwrap().contains("weight"));
        assert!(rows["instant"].error.as_deref().unwrap().contains("timeoutSecs"));
        assert_eq!((rows["off"].enabled, rows["off"].error.clone()), (false, None));
    }

    /// Two entries that would share a tool-name prefix: the second is the
    /// one reported, so the first keeps working.
    #[test]
    fn names_that_collapse_to_one_prefix_clash() {
        let config = parse(
            r#"{"mcpServers":{"My Server":{"command":"x"},"my server":{"command":"y"},"my_server":{"command":"z"}}}"#,
        )
        .unwrap();
        let rows = items(&config);
        assert_eq!(rows[0].error, None);
        // Every later one names the first, the one that keeps the prefix.
        for row in &rows[1..] {
            assert!(row.error.as_deref().unwrap().contains("\"My Server\""), "{:?}", row.error);
        }
    }

    #[test]
    fn a_server_key_is_what_a_tool_name_may_hold() {
        assert_eq!(server_key("My Server.v2"), "my_server_v2");
        assert_eq!(server_key("git-hub"), "git-hub");
    }

    #[test]
    fn an_empty_file_is_no_servers_and_broken_json_is_refused() {
        assert_eq!(parse("  ").unwrap(), McpConfig::default());
        assert!(matches!(parse("{\"mcpServers\": ["), Err(McpConfigError::Parse(_))));
        assert!(matches!(parse(r#"{"mcpServers": {"a": {"args": "x"}}}"#), Err(McpConfigError::Parse(_))));
    }
}
