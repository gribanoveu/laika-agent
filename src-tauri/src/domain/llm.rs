//! The model-facing protocol.
//!
//! Only the piece the tool boundary needs so far: what a tool call looks like
//! coming off the wire. The provider trait, the turn event stream and the
//! pause/resume types arrive with the loop that uses them.

use serde::{Deserialize, Serialize};

/// One tool call as the model produced it — a name and a JSON string it
/// generated, neither of which is trusted to be well-formed.
///
/// Turning this into a typed `ToolCall` is `services::ai_tools::parse`'s job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmToolCall {
    /// The provider's id for this call, echoed back with its result so a round
    /// with several calls can be matched up.
    pub id: String,
    pub name: String,
    /// Raw JSON. May be empty for a tool that takes no arguments, and may be
    /// malformed — a model getting its own call wrong is ordinary, and is fed
    /// back to it as a tool result rather than failing the turn.
    #[serde(default)]
    pub arguments: String,
}
