use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use crate::{
    providers::LlmProvider,
    types::{
        AgentRequest, AgentResponse, ProviderConversationRef, ProviderRun, ToolCallRequest, new_id,
    },
};

pub struct FakeProvider;

#[async_trait]
impl LlmProvider for FakeProvider {
    fn id(&self) -> &'static str {
        "fake"
    }

    fn validate_config(&self) -> Result<()> {
        Ok(())
    }

    async fn start_turn(&self, request: AgentRequest) -> Result<ProviderRun> {
        Ok(fake_run(None, request))
    }

    async fn resume_turn(
        &self,
        conversation: ProviderConversationRef,
        request: AgentRequest,
    ) -> Result<ProviderRun> {
        Ok(fake_run(Some(conversation), request))
    }
}

fn fake_run(
    conversation_ref: Option<ProviderConversationRef>,
    request: AgentRequest,
) -> ProviderRun {
    let conversation_ref =
        conversation_ref.unwrap_or_else(|| ProviderConversationRef { id: new_id() });

    if !request.tool_results.is_empty() {
        return ProviderRun {
            conversation_ref: Some(conversation_ref),
            response: AgentResponse::message(format!(
                "Received {} tool result(s).",
                request.tool_results.len()
            )),
            raw_events: vec![json!({ "provider": "fake", "event": "tool_results" })],
        };
    }

    let prompt = request.prompt.to_lowercase();
    let tool_name = if prompt.contains("memory") {
        Some("memory.read")
    } else if prompt.contains("todo") {
        Some("todo.read")
    } else if prompt.contains("time") || prompt.contains("clock") {
        Some("clock.now")
    } else {
        None
    };

    let response = if let Some(tool_name) = tool_name {
        AgentResponse {
            assistant_message: format!("Requesting `{tool_name}`."),
            tool_requests: vec![ToolCallRequest {
                id: new_id(),
                tool_name: tool_name.to_string(),
                args: json!({}),
            }],
        }
    } else {
        AgentResponse::message(format!("Fake provider received: {}", request.prompt))
    };

    ProviderRun {
        conversation_ref: Some(conversation_ref),
        response,
        raw_events: vec![json!({ "provider": "fake", "event": "turn" })],
    }
}
