use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{
    store::Store,
    types::{ToolCallRequest, ToolResult, ToolSpec, now_rfc3339},
};

#[derive(Clone)]
pub struct ToolContext {
    pub store: Store,
}

#[async_trait]
pub trait ClawTool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> Value {
        json!({ "type": "object" })
    }
    async fn execute(&self, args: Value, context: ToolContext) -> Result<Value>;
}

#[derive(Clone)]
pub struct ToolRegistry {
    tools: Arc<HashMap<String, Arc<dyn ClawTool>>>,
}

impl ToolRegistry {
    pub fn default_local() -> Self {
        let mut tools: HashMap<String, Arc<dyn ClawTool>> = HashMap::new();
        for tool in [
            Arc::new(MemoryRead) as Arc<dyn ClawTool>,
            Arc::new(TodoRead) as Arc<dyn ClawTool>,
            Arc::new(ClockNow) as Arc<dyn ClawTool>,
        ] {
            tools.insert(tool.name().to_string(), tool);
        }
        Self {
            tools: Arc::new(tools),
        }
    }

    pub fn names(&self) -> Vec<String> {
        let mut names = self.tools.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        let mut specs = self
            .tools
            .values()
            .map(|tool| ToolSpec {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
            })
            .collect::<Vec<_>>();
        specs.sort_by(|a, b| a.name.cmp(&b.name));
        specs
    }

    pub async fn execute(&self, call: &ToolCallRequest, context: ToolContext) -> ToolResult {
        let Some(tool) = self.tools.get(&call.tool_name) else {
            return ToolResult::error(call, format!("unknown tool: {}", call.tool_name));
        };

        match tool.execute(call.args.clone(), context).await {
            Ok(output) => ToolResult::ok(call, output),
            Err(error) => ToolResult::error(call, error.to_string()),
        }
    }
}

struct MemoryRead;

#[async_trait]
impl ClawTool for MemoryRead {
    fn name(&self) -> &'static str {
        "memory.read"
    }

    fn description(&self) -> &'static str {
        "Read Claw's local markdown memory files."
    }

    async fn execute(&self, _args: Value, context: ToolContext) -> Result<Value> {
        Ok(json!({ "memory": context.store.read_memory()? }))
    }
}

struct TodoRead;

#[async_trait]
impl ClawTool for TodoRead {
    fn name(&self) -> &'static str {
        "todo.read"
    }

    fn description(&self) -> &'static str {
        "Read Claw's local markdown todo list."
    }

    async fn execute(&self, _args: Value, context: ToolContext) -> Result<Value> {
        Ok(json!({ "todo": context.store.read_todo()? }))
    }
}

struct ClockNow;

#[async_trait]
impl ClawTool for ClockNow {
    fn name(&self) -> &'static str {
        "clock.now"
    }

    fn description(&self) -> &'static str {
        "Return the current UTC timestamp."
    }

    async fn execute(&self, _args: Value, _context: ToolContext) -> Result<Value> {
        Ok(json!({ "now": now_rfc3339() }))
    }
}
