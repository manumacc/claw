use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub id: String,
    pub request: IpcRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub id: String,
    pub ok: bool,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum IpcRequest {
    ChatNew {
        prompt: String,
        provider: Option<String>,
    },
    ChatResume {
        chat_id: String,
        prompt: String,
    },
    ChatsList,
    ChatsCleanup,
    ProvidersList,
}

impl IpcRequest {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ChatNew { .. } => "chat_new",
            Self::ChatResume { .. } => "chat_resume",
            Self::ChatsList => "chats_list",
            Self::ChatsCleanup => "chats_cleanup",
            Self::ProvidersList => "providers_list",
        }
    }
}

impl ResponseEnvelope {
    pub fn ok(id: String, result: impl Serialize) -> Self {
        Self {
            id,
            ok: true,
            result: Some(serde_json::to_value(result).expect("serializable response")),
            error: None,
        }
    }

    pub fn error(id: String, error: impl ToString) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(error.to_string()),
        }
    }
}
