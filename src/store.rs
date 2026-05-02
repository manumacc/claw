use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{
    config::AppConfig,
    types::{ChatManifest, TurnLogEntry, new_id, now_rfc3339},
};

#[derive(Debug, Clone)]
pub struct Store {
    root: Arc<PathBuf>,
}

impl Store {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            root: Arc::new(config.storage_root.clone()),
        }
    }

    pub fn root(&self) -> &Path {
        self.root.as_ref()
    }

    pub fn ensure(&self, config: &AppConfig) -> Result<()> {
        for path in [
            self.root(),
            &self.memory_dir(),
            &self.chats_dir(),
            &self.schemas_dir(),
            &self.tmp_dir(),
        ] {
            fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))?;
        }

        write_if_missing(
            &self.memory_dir().join("profile.md"),
            "# Claw Memory\n\nPersistent notes for the assistant live here.\n",
        )?;
        write_if_missing(&self.todo_path(), "# Claw Todo\n\n")?;
        write_json_pretty_if_missing(&self.root().join("config.json"), config)?;
        fs::write(self.agent_response_schema_path(), AGENT_RESPONSE_SCHEMA)
            .with_context(|| "writing agent response schema")?;
        Ok(())
    }

    pub fn memory_dir(&self) -> PathBuf {
        self.root().join("memory")
    }

    pub fn todo_path(&self) -> PathBuf {
        self.root().join("todo.md")
    }

    pub fn chats_dir(&self) -> PathBuf {
        self.root().join("chats")
    }

    pub fn schemas_dir(&self) -> PathBuf {
        self.root().join("schemas")
    }

    pub fn tmp_dir(&self) -> PathBuf {
        self.root().join("tmp")
    }

    pub fn agent_response_schema_path(&self) -> PathBuf {
        self.schemas_dir().join("agent_response.schema.json")
    }

    pub fn read_memory(&self) -> Result<String> {
        let mut paths = fs::read_dir(self.memory_dir())
            .with_context(|| "reading memory directory")?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
            .collect::<Vec<_>>();
        paths.sort();

        let mut memory = String::new();
        for path in paths {
            let content =
                fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            memory.push_str(&format!("<!-- {} -->\n{}\n", path.display(), content));
        }
        Ok(memory)
    }

    pub fn read_todo(&self) -> Result<String> {
        fs::read_to_string(self.todo_path()).with_context(|| "reading todo.md")
    }

    pub fn create_chat(&self, provider_id: impl Into<String>) -> Result<ChatManifest> {
        let now = now_rfc3339();
        let id = new_id();
        let manifest = ChatManifest {
            id: id.clone(),
            created_at: now.clone(),
            updated_at: now,
            provider_id: provider_id.into(),
            provider_conversation_ref: None,
            title: None,
        };

        fs::create_dir_all(self.chat_dir(&id))?;
        self.save_chat(&manifest)?;
        Ok(manifest)
    }

    pub fn load_chat(&self, id: &str) -> Result<ChatManifest> {
        read_json(&self.chat_manifest_path(id))
    }

    pub fn save_chat(&self, manifest: &ChatManifest) -> Result<()> {
        fs::create_dir_all(self.chat_dir(&manifest.id))?;
        write_json_pretty(&self.chat_manifest_path(&manifest.id), manifest)
    }

    pub fn list_chats(&self) -> Result<Vec<ChatManifest>> {
        if !self.chats_dir().exists() {
            return Ok(Vec::new());
        }

        let mut chats = Vec::new();
        for entry in fs::read_dir(self.chats_dir())? {
            let path = entry?.path().join("manifest.json");
            if path.exists() {
                chats.push(read_json(&path)?);
            }
        }
        chats.sort_by(|a: &ChatManifest, b| b.updated_at.cmp(&a.updated_at));
        Ok(chats)
    }

    pub fn delete_chat(&self, id: &str) -> Result<()> {
        let path = self.chat_dir(id);
        if path.exists() {
            fs::remove_dir_all(&path).with_context(|| format!("removing {}", path.display()))?;
        }
        Ok(())
    }

    pub fn append_turn(
        &self,
        chat_id: &str,
        role: &str,
        content: impl Into<String>,
        metadata: Value,
    ) -> Result<()> {
        let entry = TurnLogEntry {
            at: now_rfc3339(),
            role: role.to_string(),
            content: content.into(),
            metadata,
        };
        append_jsonl(&self.chat_dir(chat_id).join("turns.jsonl"), &entry)
    }

    fn chat_dir(&self, id: &str) -> PathBuf {
        self.chats_dir().join(id)
    }

    fn chat_manifest_path(&self, id: &str) -> PathBuf {
        self.chat_dir(id).join("manifest.json")
    }
}

fn write_if_missing(path: &Path, content: &str) -> Result<()> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

fn write_json_pretty_if_missing<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if !path.exists() {
        write_json_pretty(path, value)?;
    }
    Ok(())
}

fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(value)?;
    fs::write(path, format!("{json}\n")).with_context(|| format!("writing {}", path.display()))
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}

const AGENT_RESPONSE_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "assistant_message": { "type": "string" },
    "tool_requests": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "id": { "type": "string" },
          "tool_name": { "type": "string" },
          "args": { "type": "object" }
        },
        "required": ["tool_name", "args"]
      }
    }
  },
  "required": ["assistant_message", "tool_requests"]
}
"#;
