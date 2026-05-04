//! Provider-agnostic tool definition.
//!
//! Each provider adapter knows how to translate `ToolDef` into the wire
//! format its API expects. Constructing the list is the chat loop's job —
//! see `nitro::tools` for the actual `Bash` and `AskUser` schemas.

use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool's input parameters.
    pub input_schema: Value,
}
