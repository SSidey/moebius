use std::path::Path;
use anyhow::Result;
use serde_json::json;

use crate::adapters::ToolDef;
use crate::run_state::{RubricStatus, RubricVerification, SharedRunState};
use super::ToolHandler;

pub struct VerifyRubricsTool {
    pub state: SharedRunState,
}

impl ToolHandler for VerifyRubricsTool {
    fn name(&self) -> &'static str {
        "verify_rubrics"
    }

    fn definition(&self) -> ToolDef {
        ToolDef {
            name: "verify_rubrics",
            description: "Record the pass/fail/na verdict for each structured rubric criterion in the specification's ## Rubric section. Call this as your final tool invocation before your completion summary.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "criteria": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Criterion name from the rubric table." },
                                "status": { "type": "string", "enum": ["pass", "fail", "na"] },
                                "note": { "type": "string", "description": "Optional explanation, required when status is fail." }
                            },
                            "required": ["name", "status"]
                        }
                    }
                },
                "required": ["criteria"]
            }),
        }
    }

    fn execute(&self, args: &serde_json::Value, _working_dir: &Path) -> Result<String> {
        let criteria_json = args["criteria"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("verify_rubrics: 'criteria' must be an array"))?;

        let mut passes = 0usize;
        let mut fails = 0usize;
        let mut nas = 0usize;

        let verifications: Vec<RubricVerification> = criteria_json
            .iter()
            .map(|c| {
                let name = c["name"].as_str().unwrap_or("").to_string();
                let status_str = c["status"].as_str().unwrap_or("na");
                let note = c["note"].as_str().map(|s| s.to_string());
                let status = match status_str {
                    "pass" => { passes += 1; RubricStatus::Pass }
                    "fail" => { fails += 1; RubricStatus::Fail }
                    _ => { nas += 1; RubricStatus::Na }
                };
                RubricVerification { name, status, note }
            })
            .collect();

        self.state.lock().unwrap().rubric_verifications = verifications;

        let mut result = format!("Rubric verified: {} pass, {} fail, {} na.", passes, fails, nas);
        if fails > 0 {
            result.push_str(&format!(" WARNING: {} criterion/criteria failed.", fails));
        }
        Ok(result)
    }
}
