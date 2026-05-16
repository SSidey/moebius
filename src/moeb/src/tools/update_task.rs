use std::path::Path;
use anyhow::Result;
use serde_json::json;

use crate::adapters::ToolDef;
use crate::run_state::{SharedRunState, TaskStatus};
use super::ToolHandler;

pub struct UpdateTaskTool {
    pub state: SharedRunState,
}

impl ToolHandler for UpdateTaskTool {
    fn name(&self) -> &'static str {
        "update_task"
    }

    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "update_task",
            description: "Mark a task from your create_task_list plan as done or skipped.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "The id from create_task_list." },
                    "status": { "type": "string", "enum": ["done", "skipped"] }
                },
                "required": ["id", "status"]
            }),
        }
    }

    fn execute(&self, args: &serde_json::Value, _working_dir: &Path) -> Result<String> {
        let id = args["id"].as_str().unwrap_or("").to_string();
        let status_str = args["status"].as_str().unwrap_or("done");
        let new_status = match status_str {
            "skipped" => TaskStatus::Skipped,
            _ => TaskStatus::Done,
        };

        let mut locked = self.state.lock().unwrap();
        if let Some(task) = locked.tasks.iter_mut().find(|t| t.id == id) {
            task.status = new_status;
            Ok(format!("Task {} marked {}.", id, status_str))
        } else {
            Ok(format!("Warning: no task with id '{}' in the current task list.", id))
        }
    }
}
