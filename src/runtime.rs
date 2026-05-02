use std::time::Instant;

use anyhow::Result;
use serde::Serialize;
use serde_json::json;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use tracing::{debug, info, warn};

use crate::{
    config::AppConfig,
    providers::ProviderRegistry,
    store::Store,
    tools::{ToolContext, ToolRegistry},
    types::{
        AgentRequest, ChatManifest, ContextBundle, ProviderConversationRef, ToolResult, now_rfc3339,
    },
};

const MAX_PROVIDER_STEPS: usize = 4;

#[derive(Clone)]
pub struct Runtime {
    config: AppConfig,
    store: Store,
    providers: ProviderRegistry,
    tools: ToolRegistry,
}

impl Runtime {
    pub fn new(config: AppConfig) -> Result<Self> {
        let store = Store::new(&config);
        store.ensure(&config)?;
        let providers = ProviderRegistry::from_config(&config, &store);
        Ok(Self {
            config,
            store,
            providers,
            tools: ToolRegistry::default_local(),
        })
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub async fn chat_new(
        &self,
        prompt: String,
        provider_id: Option<String>,
    ) -> Result<ChatOutput> {
        let provider_id = provider_id.unwrap_or_else(|| self.config.default_provider.clone());
        self.providers.get(&provider_id)?.validate_config()?;

        let mut manifest = self.store.create_chat(provider_id)?;
        info!(
            chat_id = %manifest.id,
            provider_id = %manifest.provider_id,
            prompt_len = prompt.len(),
            "created chat"
        );
        self.store
            .append_turn(&manifest.id, "user", &prompt, json!({}))?;
        self.run_provider_loop(&mut manifest, prompt).await
    }

    pub async fn chat_resume(&self, chat_id: String, prompt: String) -> Result<ChatOutput> {
        let mut manifest = self.store.load_chat(&chat_id)?;
        let provider_conversation_id = manifest
            .provider_conversation_ref
            .as_ref()
            .map(|conversation| conversation.id.as_str())
            .unwrap_or("<none>");
        info!(
            chat_id = %manifest.id,
            provider_id = %manifest.provider_id,
            provider_conversation_id = %provider_conversation_id,
            prompt_len = prompt.len(),
            "loaded chat for resume"
        );
        if manifest.provider_conversation_ref.is_none() {
            warn!(
                chat_id = %manifest.id,
                provider_id = %manifest.provider_id,
                "resumed chat has no provider conversation ref; provider will start a new conversation"
            );
        }
        self.store
            .append_turn(&manifest.id, "user", &prompt, json!({}))?;
        self.run_provider_loop(&mut manifest, prompt).await
    }

    pub fn list_chats(&self) -> Result<Vec<ChatManifest>> {
        self.store.list_chats()
    }

    pub fn cleanup_chats(&self) -> Result<CleanupOutput> {
        let cutoff = OffsetDateTime::now_utc() - Duration::days(90);
        let mut removed = 0usize;

        for chat in self.store.list_chats()? {
            let updated_at = OffsetDateTime::parse(&chat.updated_at, &Rfc3339)?;
            if updated_at < cutoff {
                self.store.delete_chat(&chat.id)?;
                removed += 1;
            }
        }

        Ok(CleanupOutput { removed })
    }

    pub fn providers(&self) -> Vec<ProviderOutput> {
        self.providers
            .list()
            .into_iter()
            .map(|provider| ProviderOutput {
                id: provider.id,
                valid: provider.valid,
            })
            .collect()
    }

    async fn run_provider_loop(
        &self,
        manifest: &mut ChatManifest,
        initial_prompt: String,
    ) -> Result<ChatOutput> {
        let mut prompt = initial_prompt;
        let mut tool_results = Vec::new();
        let mut all_tool_results = Vec::new();
        let mut assistant_messages = Vec::new();

        for step in 1..=MAX_PROVIDER_STEPS {
            let run = {
                let provider = self.providers.get(&manifest.provider_id)?;
                let available_tools = self.tools.specs();
                let request = AgentRequest {
                    chat_id: manifest.id.clone(),
                    prompt: prompt.clone(),
                    context: self.context_bundle()?,
                    available_tools,
                    tool_results: tool_results.clone(),
                };
                let provider_conversation_id = manifest
                    .provider_conversation_ref
                    .as_ref()
                    .map(|conversation| conversation.id.as_str())
                    .unwrap_or("<none>");
                let mode = if manifest.provider_conversation_ref.is_some() {
                    "resume"
                } else {
                    "start"
                };
                let started = Instant::now();
                info!(
                    chat_id = %manifest.id,
                    provider_id = %manifest.provider_id,
                    provider_conversation_id = %provider_conversation_id,
                    step,
                    mode = %mode,
                    incoming_tool_results = tool_results.len(),
                    available_tools = request.available_tools.len(),
                    "provider turn starting"
                );

                match manifest.provider_conversation_ref.clone() {
                    Some(conversation) => {
                        let run = provider.resume_turn(conversation, request).await?;
                        info!(
                            chat_id = %manifest.id,
                            provider_id = %manifest.provider_id,
                            step,
                            mode = %mode,
                            elapsed_ms = started.elapsed().as_millis(),
                            assistant_message_len = run.response.assistant_message.len(),
                            tool_requests = run.response.tool_requests.len(),
                            raw_events = run.raw_events.len(),
                            "provider turn completed"
                        );
                        run
                    }
                    None => {
                        let run = provider.start_turn(request).await?;
                        info!(
                            chat_id = %manifest.id,
                            provider_id = %manifest.provider_id,
                            step,
                            mode = %mode,
                            elapsed_ms = started.elapsed().as_millis(),
                            assistant_message_len = run.response.assistant_message.len(),
                            tool_requests = run.response.tool_requests.len(),
                            raw_events = run.raw_events.len(),
                            "provider turn completed"
                        );
                        run
                    }
                }
            };

            let previous_conversation_id = manifest
                .provider_conversation_ref
                .as_ref()
                .map(|conversation| conversation.id.as_str())
                .unwrap_or("<none>")
                .to_string();
            let next_conversation_id = run
                .conversation_ref
                .as_ref()
                .map(|conversation| conversation.id.as_str())
                .unwrap_or("<none>")
                .to_string();
            if let Some(conversation_ref) = merge_conversation_ref(
                manifest.provider_conversation_ref.clone(),
                run.conversation_ref,
            ) {
                manifest.provider_conversation_ref = Some(conversation_ref);
            }
            let merged_conversation_id = manifest
                .provider_conversation_ref
                .as_ref()
                .map(|conversation| conversation.id.as_str())
                .unwrap_or("<none>");
            debug!(
                chat_id = %manifest.id,
                provider_id = %manifest.provider_id,
                step,
                previous_conversation_id = %previous_conversation_id,
                next_conversation_id = %next_conversation_id,
                merged_conversation_id = %merged_conversation_id,
                "updated provider conversation ref"
            );
            manifest.updated_at = now_rfc3339();
            self.store.save_chat(manifest)?;

            if !run.response.assistant_message.trim().is_empty() {
                self.store.append_turn(
                    &manifest.id,
                    "assistant",
                    &run.response.assistant_message,
                    json!({ "provider": manifest.provider_id }),
                )?;
                assistant_messages.push(run.response.assistant_message.clone());
            }

            tool_results.clear();
            for call in run.response.tool_requests {
                let started = Instant::now();
                info!(
                    chat_id = %manifest.id,
                    tool = %call.tool_name,
                    call_id = %call.id,
                    "tool call starting"
                );
                let result = self.tools.execute(&call, self.tool_context()).await;
                if result.ok {
                    info!(
                        chat_id = %manifest.id,
                        tool = %result.tool_name,
                        call_id = %result.call_id,
                        elapsed_ms = started.elapsed().as_millis(),
                        "tool call completed"
                    );
                } else {
                    warn!(
                        chat_id = %manifest.id,
                        tool = %result.tool_name,
                        call_id = %result.call_id,
                        elapsed_ms = started.elapsed().as_millis(),
                        error = %result.error.as_deref().unwrap_or("unknown tool error"),
                        "tool call failed"
                    );
                }
                self.store.append_turn(
                    &manifest.id,
                    "tool",
                    serde_json::to_string(&result)?,
                    json!({ "tool": call.tool_name }),
                )?;
                all_tool_results.push(result.clone());
                tool_results.push(result);
            }

            if tool_results.is_empty() {
                break;
            }

            prompt = "Continue after these Claw tool results.".to_string();
        }

        Ok(ChatOutput {
            chat_id: manifest.id.clone(),
            provider_id: manifest.provider_id.clone(),
            provider_conversation_ref: manifest.provider_conversation_ref.clone(),
            assistant_messages,
            tool_results: all_tool_results,
        })
    }

    fn context_bundle(&self) -> Result<ContextBundle> {
        Ok(ContextBundle {
            memory: self.store.read_memory()?,
            todo: self.store.read_todo()?,
            now: now_rfc3339(),
        })
    }

    fn tool_context(&self) -> ToolContext {
        ToolContext {
            store: self.store.clone(),
        }
    }
}

fn merge_conversation_ref(
    previous: Option<ProviderConversationRef>,
    next: Option<ProviderConversationRef>,
) -> Option<ProviderConversationRef> {
    next.or(previous)
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatOutput {
    pub chat_id: String,
    pub provider_id: String,
    pub provider_conversation_ref: Option<ProviderConversationRef>,
    pub assistant_messages: Vec<String>,
    pub tool_results: Vec<ToolResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupOutput {
    pub removed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderOutput {
    pub id: String,
    pub valid: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> AppConfig {
        let root = std::env::temp_dir().join(format!("claw-test-{}", crate::types::new_id()));
        AppConfig::defaults()
            .expect("defaults")
            .with_storage_root(root)
    }

    #[tokio::test]
    async fn fake_provider_executes_memory_tool() {
        let mut config = test_config();
        config.default_provider = "fake".to_string();
        let runtime = Runtime::new(config).expect("runtime");

        let output = runtime
            .chat_new("please read memory".to_string(), None)
            .await
            .expect("chat");

        assert_eq!(output.provider_id, "fake");
        assert_eq!(output.tool_results.len(), 1);
        assert_eq!(output.tool_results[0].tool_name, "memory.read");
        assert!(output.tool_results[0].ok);
    }

    #[tokio::test]
    async fn fake_provider_executes_clock_tool() {
        let mut config = test_config();
        config.default_provider = "fake".to_string();
        let runtime = Runtime::new(config).expect("runtime");

        let output = runtime
            .chat_new("what time is it?".to_string(), None)
            .await
            .expect("chat");

        assert_eq!(output.tool_results.len(), 1);
        assert_eq!(output.tool_results[0].tool_name, "clock.now");
        assert!(output.tool_results[0].ok);
    }
}
