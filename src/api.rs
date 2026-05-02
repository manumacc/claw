use anyhow::Result;
use async_trait::async_trait;

use crate::{config::AppConfig, ipc::IpcRequest, runtime::Runtime};

#[async_trait]
pub trait RequestHandler: Clone + Send + Sync + 'static {
    async fn handle(&self, request: IpcRequest) -> Result<serde_json::Value>;
}

#[derive(Clone)]
pub struct ClawApi {
    runtime: Runtime,
}

impl ClawApi {
    pub fn new(config: AppConfig) -> Result<Self> {
        Ok(Self {
            runtime: Runtime::new(config)?,
        })
    }

    pub fn from_runtime(runtime: Runtime) -> Self {
        Self { runtime }
    }
}

#[async_trait]
impl RequestHandler for ClawApi {
    async fn handle(&self, request: IpcRequest) -> Result<serde_json::Value> {
        let value = match request {
            IpcRequest::ChatNew { prompt, provider } => {
                serde_json::to_value(self.runtime.chat_new(prompt, provider).await?)?
            }
            IpcRequest::ChatResume { chat_id, prompt } => {
                serde_json::to_value(self.runtime.chat_resume(chat_id, prompt).await?)?
            }
            IpcRequest::ChatsList => serde_json::to_value(self.runtime.list_chats()?)?,
            IpcRequest::ChatsCleanup => serde_json::to_value(self.runtime.cleanup_chats()?)?,
            IpcRequest::ProvidersList => serde_json::to_value(self.runtime.providers())?,
        };
        Ok(value)
    }
}
