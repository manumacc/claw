use std::{collections::HashMap, sync::Arc};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::Serialize;

use crate::{
    config::AppConfig,
    store::Store,
    types::{AgentRequest, ProviderConversationRef, ProviderRun},
};

pub mod codex;
pub mod fake;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn validate_config(&self) -> Result<()>;
    async fn start_turn(&self, request: AgentRequest) -> Result<ProviderRun>;
    async fn resume_turn(
        &self,
        conversation: ProviderConversationRef,
        request: AgentRequest,
    ) -> Result<ProviderRun>;
}

#[derive(Clone)]
pub struct ProviderRegistry {
    providers: Arc<HashMap<String, Arc<dyn LlmProvider>>>,
}

impl ProviderRegistry {
    pub fn from_config(config: &AppConfig, store: &Store) -> Self {
        let mut providers: HashMap<String, Arc<dyn LlmProvider>> = HashMap::new();
        let codex = Arc::new(codex::CodexProvider::new(
            config.codex.clone(),
            config.workspace_root.clone(),
            store.clone(),
        ));
        let fake = Arc::new(fake::FakeProvider);
        providers.insert(codex.id().to_string(), codex);
        providers.insert(fake.id().to_string(), fake);
        Self {
            providers: Arc::new(providers),
        }
    }

    pub fn get(&self, id: &str) -> Result<Arc<dyn LlmProvider>> {
        self.providers
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown provider: {id}"))
    }

    pub fn list(&self) -> Vec<ProviderInfo> {
        let mut providers = self
            .providers
            .values()
            .map(|provider| ProviderInfo {
                id: provider.id().to_string(),
                valid: provider.validate_config().is_ok(),
            })
            .collect::<Vec<_>>();
        providers.sort_by(|a, b| a.id.cmp(&b.id));
        providers
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub valid: bool,
}
