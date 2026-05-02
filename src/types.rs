use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBundle {
    pub memory: String,
    pub todo: String,
    pub now: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    pub chat_id: String,
    pub prompt: String,
    pub context: ContextBundle,
    #[serde(default)]
    pub available_tools: Vec<ToolSpec>,
    #[serde(default)]
    pub tool_results: Vec<ToolResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    #[serde(default)]
    pub assistant_message: String,
    #[serde(default)]
    pub tool_requests: Vec<ToolCallRequest>,
}

impl AgentResponse {
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            assistant_message: message.into(),
            tool_requests: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    #[serde(default = "new_id")]
    pub id: String,
    pub tool_name: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub call_id: String,
    pub tool_name: String,
    pub ok: bool,
    #[serde(default)]
    pub output: Value,
    #[serde(default)]
    pub error: Option<String>,
}

impl ToolResult {
    pub fn ok(call: &ToolCallRequest, output: Value) -> Self {
        Self {
            call_id: call.id.clone(),
            tool_name: call.tool_name.clone(),
            ok: true,
            output,
            error: None,
        }
    }

    pub fn error(call: &ToolCallRequest, error: impl Into<String>) -> Self {
        Self {
            call_id: call.id.clone(),
            tool_name: call.tool_name.clone(),
            ok: false,
            output: Value::Null,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConversationRef {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRun {
    #[serde(default)]
    pub conversation_ref: Option<ProviderConversationRef>,
    pub response: AgentResponse,
    #[serde(default)]
    pub raw_events: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatManifest {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub provider_id: String,
    #[serde(default)]
    pub provider_conversation_ref: Option<ProviderConversationRef>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnLogEntry {
    pub at: String,
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub metadata: Value,
}
