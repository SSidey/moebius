use std::path::Path;
use anyhow::Result;
use serde_json::json;

use crate::adapters::ToolDef;
use crate::run_state::{SharedRunState, Task, TaskStatus};
use super::ToolHandler;

pub struct CreateTaskListTool {
    pub state: SharedRunState,
}

impl ToolHandler for CreateTaskListTool {
    fn name(&self) -> &'static str {
        "create_task_list"
    }

    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "create_task_list",
            description: "Record your implementation plan before beginning file modifications. Call this as your very first tool invocation. Each task should identify which file(s) are involved and what change is required.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "array",
                        "description": "Ordered list of implementation tasks.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string", "description": "Short identifier, e.g. \"1\" or \"step-3\"." },
                                "description": { "type": "string", "description": "What this task does and which file(s) it touches." }
                            },
                            "required": ["id", "description"]
                        }
                    }
                },
                "required": ["tasks"]
            }),
        }
    }

    fn execute(&self, args: &serde_json::Value, _working_dir: &Path) -> Result<String> {
        let tasks_json = args["tasks"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("create_task_list: 'tasks' must be an array"))?;

        let tasks: Vec<Task> = tasks_json
            .iter()
            .map(|t| {
                let id = t["id"].as_str().unwrap_or("").to_string();
                let description = t["description"].as_str().unwrap_or("").to_string();
                Task { id, description, status: TaskStatus::Pending }
            })
            .collect();

        let count = tasks.len();
        self.state.lock().unwrap().tasks = tasks;

        Ok(format!("Task list recorded: {} tasks.", count))
    }
}
