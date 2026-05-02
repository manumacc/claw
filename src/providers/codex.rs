use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    time::Instant,
};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use tokio::{io::AsyncWriteExt, process::Command};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::{
    config::CodexConfig,
    providers::LlmProvider,
    store::Store,
    types::{AgentRequest, AgentResponse, ProviderConversationRef, ProviderRun},
};

pub struct CodexProvider {
    config: CodexConfig,
    workspace_root: PathBuf,
    store: Store,
}

impl CodexProvider {
    pub fn new(config: CodexConfig, workspace_root: PathBuf, store: Store) -> Self {
        Self {
            config,
            workspace_root,
            store,
        }
    }

    async fn run(
        &self,
        conversation: Option<ProviderConversationRef>,
        request: AgentRequest,
    ) -> Result<ProviderRun> {
        let output_path = self
            .store
            .tmp_dir()
            .join(format!("codex-last-message-{}.json", Uuid::new_v4()));
        let prompt = render_prompt(&request)?;
        let mode = if conversation.is_some() {
            "resume"
        } else {
            "start"
        };
        let provider_conversation_id = conversation
            .as_ref()
            .map(|conversation| conversation.id.as_str())
            .unwrap_or("<none>");

        let mut command = Command::new(&self.config.command);
        command
            .current_dir(&self.workspace_root)
            .arg("exec")
            .arg("--json")
            .arg("--output-schema")
            .arg(self.store.agent_response_schema_path())
            .arg("--output-last-message")
            .arg(&output_path)
            .arg("--sandbox")
            .arg("read-only")
            .arg("--skip-git-repo-check");

        if let Some(model) = &self.config.model {
            command.arg("--model").arg(model);
        }

        if let Some(profile) = &self.config.profile {
            command.arg("--profile").arg(profile);
        }

        if self.config.search {
            command.arg("--search");
        }

        if let Some(conversation) = &conversation {
            command.arg("resume").arg(&conversation.id).arg("-");
        } else {
            command.arg("-");
        }

        info!(
            chat_id = %request.chat_id,
            provider_id = "codex",
            mode = %mode,
            provider_conversation_id = %provider_conversation_id,
            command = %self.config.command,
            workspace_root = %self.workspace_root.display(),
            model = self.config.model.as_deref().unwrap_or("<default>"),
            profile = self.config.profile.as_deref().unwrap_or("<default>"),
            search = self.config.search,
            prompt_len = prompt.len(),
            "launching codex exec"
        );
        let started = Instant::now();
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("spawning {}", self.config.command))?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("failed to open codex stdin"))?;
        stdin.write_all(prompt.as_bytes()).await?;
        drop(stdin);

        let output = child.wait_with_output().await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let raw_events = parse_jsonl_events(&stdout);
        debug!(
            chat_id = %request.chat_id,
            provider_id = "codex",
            mode = %mode,
            status = %output.status,
            elapsed_ms = started.elapsed().as_millis(),
            stdout_bytes = output.stdout.len(),
            stderr_bytes = output.stderr.len(),
            raw_events = raw_events.len(),
            "codex exec exited"
        );

        if !output.status.success() {
            warn!(
                chat_id = %request.chat_id,
                provider_id = "codex",
                mode = %mode,
                status = %output.status,
                elapsed_ms = started.elapsed().as_millis(),
                stdout_bytes = output.stdout.len(),
                stderr_bytes = output.stderr.len(),
                "codex exec failed"
            );
            return Err(anyhow!(
                "codex exec failed: {}\n{}",
                stderr.trim(),
                stdout.trim()
            ));
        }

        let last_message = tokio::fs::read_to_string(&output_path)
            .await
            .with_context(|| format!("reading {}", output_path.display()))?;
        let _ = tokio::fs::remove_file(&output_path).await;

        let response = parse_agent_response(&last_message);
        let conversation_ref = extract_conversation_ref(&raw_events).or(conversation);
        info!(
            chat_id = %request.chat_id,
            provider_id = "codex",
            mode = %mode,
            provider_conversation_id = %conversation_ref
                .as_ref()
                .map(|conversation| conversation.id.as_str())
                .unwrap_or("<none>"),
            elapsed_ms = started.elapsed().as_millis(),
            assistant_message_len = response.assistant_message.len(),
            tool_requests = response.tool_requests.len(),
            raw_events = raw_events.len(),
            "codex exec completed"
        );

        Ok(ProviderRun {
            conversation_ref,
            response,
            raw_events,
        })
    }
}

#[async_trait]
impl LlmProvider for CodexProvider {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn validate_config(&self) -> Result<()> {
        if command_exists(&self.config.command) {
            Ok(())
        } else {
            Err(anyhow!("codex command not found: {}", self.config.command))
        }
    }

    async fn start_turn(&self, request: AgentRequest) -> Result<ProviderRun> {
        self.run(None, request).await
    }

    async fn resume_turn(
        &self,
        conversation: ProviderConversationRef,
        request: AgentRequest,
    ) -> Result<ProviderRun> {
        self.run(Some(conversation), request).await
    }
}

fn command_exists(command: &str) -> bool {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.exists();
    }

    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(command).exists()))
        .unwrap_or(false)
}

#[derive(Serialize)]
struct ProviderPrompt<'a> {
    instructions: &'a str,
    request: &'a AgentRequest,
}

fn render_prompt(request: &AgentRequest) -> Result<String> {
    let prompt = ProviderPrompt {
        instructions: "You are the LLM provider inside Claw. Claw owns memory and tool execution. Return JSON matching the supplied output schema. If you need information, add a tool request using one of available_tools. Put tool arguments directly in args as a JSON object; use {} when there are no arguments. Do not claim a tool was executed until Claw supplies a tool_result.",
        request,
    };
    Ok(serde_json::to_string_pretty(&prompt)?)
}

fn parse_jsonl_events(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect()
}

fn parse_agent_response(raw: &str) -> AgentResponse {
    let trimmed = raw.trim();
    if let Ok(response) = serde_json::from_str::<AgentResponse>(trimmed) {
        return response;
    }

    let unfenced = trimmed
        .strip_prefix("```json")
        .and_then(|text| text.strip_suffix("```"))
        .map(str::trim);
    if let Some(unfenced) = unfenced
        && let Ok(response) = serde_json::from_str::<AgentResponse>(unfenced)
    {
        return response;
    }

    AgentResponse::message(trimmed)
}

fn extract_conversation_ref(events: &[Value]) -> Option<ProviderConversationRef> {
    for event in events {
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .or_else(|| event.get("event").and_then(Value::as_str));

        let is_thread_start = matches!(
            event_type,
            Some("thread.started" | "session.started" | "conversation.started")
        );

        if is_thread_start {
            for pointer in [
                "/thread_id",
                "/session_id",
                "/conversation_id",
                "/thread/id",
                "/session/id",
            ] {
                if let Some(id) = event.pointer(pointer).and_then(Value::as_str) {
                    return Some(ProviderConversationRef { id: id.to_string() });
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_tool_args() {
        let response = parse_agent_response(
            r#"{
              "assistant_message": "requesting memory",
              "tool_requests": [{
                "id": "call-1",
                "tool_name": "memory.read",
                "args": {}
              }]
            }"#,
        );

        assert_eq!(response.tool_requests[0].tool_name, "memory.read");
        assert!(response.tool_requests[0].args.is_object());
    }
}
