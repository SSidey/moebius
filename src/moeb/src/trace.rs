use serde::{Deserialize, Serialize};
use sha2::Digest;

// ── Enums ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceCommand {
    Run,
    Spec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceOutcome {
    Success,
    Failure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnResponseType {
    ToolCalls,
    Text,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentFinishReason {
    Completion,
    MaxTurns,
    ConsecutiveTextTurns,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileContentMode {
    Embed,
    Hash,
}

// ── Event structs ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnStartEvent {
    pub attempt: u32,
    pub turn: u32,
    pub messages_sent: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequestEvent {
    pub attempt: u32,
    pub turn: u32,
    pub http_attempt: u32,
    pub url: String,
    pub request_body: serde_json::Value,
    pub response_status: u16,
    pub response_headers: serde_json::Value,
    pub response_body: serde_json::Value,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRetryEvent {
    pub attempt: u32,
    pub turn: u32,
    pub http_attempt: u32,
    pub delay_ms: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaWarningEvent {
    pub attempt: u32,
    pub remaining: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEvent {
    pub attempt: u32,
    pub turn: u32,
    pub call_id: String,
    pub tool: String,
    pub args: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    pub chars: u64,
    pub success: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnEndEvent {
    pub attempt: u32,
    pub turn: u32,
    pub response_type: TurnResponseType,
    pub response_content: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentFinishedEvent {
    pub attempt: u32,
    pub turns: u32,
    pub reason: AgentFinishReason,
}

// ── Tagged union ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceEvent {
    TurnStart(TurnStartEvent),
    HttpRequest(HttpRequestEvent),
    HttpRetry(HttpRetryEvent),
    QuotaWarning(QuotaWarningEvent),
    ToolCall(ToolCallEvent),
    TurnEnd(TurnEndEvent),
    AgentFinished(AgentFinishedEvent),
}

// ── Envelope ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEnvelope {
    pub version: u32,
    pub command: TraceCommand,
    pub spec: String,
    pub adapter: String,
    pub model: String,
    pub started_at: String,
    pub ended_at: String,
    pub outcome: TraceOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub total_attempts: u32,
    pub events: Vec<TraceEvent>,
}

// ── Config ────────────────────────────────────────────────────────────────────

pub struct TraceConfig {
    pub command: TraceCommand,
    pub spec: String,
    pub adapter: String,
    pub model: String,
    pub retention: i32,
    pub file_content_mode: FileContentMode,
}

// ── Context ───────────────────────────────────────────────────────────────────

pub struct TraceContext {
    config: TraceConfig,
    started_at: chrono::DateTime<chrono::Utc>,
    events: std::sync::Mutex<Vec<TraceEvent>>,
    total_attempts: std::sync::Mutex<u32>,
    /// Updated by the agent loop before each adapter.send() call.
    pub current_turn: std::sync::atomic::AtomicU32,
    /// Updated by the agent loop / service when the attempt number changes.
    pub current_attempt: std::sync::atomic::AtomicU32,
}

impl TraceContext {
    pub fn new(config: TraceConfig) -> Self {
        Self {
            config,
            started_at: chrono::Utc::now(),
            events: std::sync::Mutex::new(Vec::new()),
            total_attempts: std::sync::Mutex::new(1),
            current_turn: std::sync::atomic::AtomicU32::new(1),
            current_attempt: std::sync::atomic::AtomicU32::new(1),
        }
    }

    pub fn push(&self, event: TraceEvent) {
        self.events.lock().unwrap().push(event);
    }

    pub fn set_total_attempts(&self, n: u32) {
        *self.total_attempts.lock().unwrap() = n;
    }

    pub fn file_content_mode(&self) -> FileContentMode {
        self.config.file_content_mode
    }

    pub fn finalize(&self, outcome: TraceOutcome, error: Option<String>) -> anyhow::Result<()> {
        if self.config.retention == 0 {
            return Ok(());
        }
        let ended_at = chrono::Utc::now();
        let events = self.events.lock().unwrap().clone();
        let total_attempts = *self.total_attempts.lock().unwrap();
        let envelope = TraceEnvelope {
            version: 1,
            command: self.config.command.clone(),
            spec: self.config.spec.clone(),
            adapter: self.config.adapter.clone(),
            model: self.config.model.clone(),
            started_at: self.started_at.to_rfc3339(),
            ended_at: ended_at.to_rfc3339(),
            outcome,
            error,
            total_attempts,
            events,
        };
        let json = serde_json::to_string_pretty(&envelope)?;
        let ts = self.started_at.format("%Y%m%dT%H%M%S").to_string() + "z";
        let filename = format!("{}-{}.json", self.config.spec, ts);
        let dir = std::path::Path::new(".moeb/traces");
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(&filename), json)?;
        if self.config.retention > 0 {
            prune_traces(&self.config.spec, self.config.retention as usize);
        }
        Ok(())
    }
}

pub fn prune_traces(spec: &str, keep: usize) {
    let dir = std::path::Path::new(".moeb/traces");
    let prefix = format!("{}-", spec);
    let mut files: Vec<_> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.starts_with(&prefix) && name.ends_with(".json")
            })
            .map(|e| e.path())
            .collect(),
        Err(_) => return,
    };
    files.sort();
    if files.len() > keep {
        let to_delete = files.len() - keep;
        for path in files.iter().take(to_delete) {
            let _ = std::fs::remove_file(path);
        }
    }
}

// ── Content policy ────────────────────────────────────────────────────────────

pub fn apply_content_policy(
    tool: &str,
    result: &anyhow::Result<String>,
    mode: FileContentMode,
) -> (Option<String>, Option<String>, u64) {
    let is_read_tool = tool == "read_file" || tool == "read_files";

    match result {
        Err(e) => (Some(format!("Error: {}", e)), None, 0),
        Ok(text) => {
            if !is_read_tool || mode == FileContentMode::Embed {
                (Some(text.clone()), None, text.len() as u64)
            } else {
                let digest = sha2::Sha256::digest(text.as_bytes());
                (None, Some(hex::encode(digest)), text.len() as u64)
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::MutexGuard;
    use tempfile::TempDir;

    use crate::config::tests::CWD_LOCK;

    fn in_temp_dir() -> (TempDir, MutexGuard<'static, ()>) {
        let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        env::set_current_dir(dir.path()).expect("set_current_dir");
        (dir, guard)
    }

    fn make_config(retention: i32) -> TraceConfig {
        TraceConfig {
            command: TraceCommand::Run,
            spec: "moeb.test".to_string(),
            adapter: "anthropic".to_string(),
            model: "claude-opus-4-7".to_string(),
            retention,
            file_content_mode: FileContentMode::Embed,
        }
    }

    #[test]
    fn trace_context_noop_when_retention_zero() {
        let (_dir, _guard) = in_temp_dir();
        std::fs::create_dir_all(".moeb").unwrap();

        let ctx = TraceContext::new(make_config(0));
        ctx.push(TraceEvent::TurnStart(TurnStartEvent {
            attempt: 1,
            turn: 1,
            messages_sent: vec![],
        }));
        ctx.finalize(TraceOutcome::Success, None).unwrap();

        let trace_dir = std::path::Path::new(".moeb/traces");
        assert!(!trace_dir.exists() || std::fs::read_dir(trace_dir).unwrap().count() == 0);
    }

    #[test]
    fn trace_context_writes_file_when_retention_minus_one() {
        let (_dir, _guard) = in_temp_dir();
        std::fs::create_dir_all(".moeb").unwrap();

        let ctx = TraceContext::new(make_config(-1));
        ctx.push(TraceEvent::TurnStart(TurnStartEvent {
            attempt: 1,
            turn: 1,
            messages_sent: vec![],
        }));
        ctx.finalize(TraceOutcome::Success, None).unwrap();

        let trace_dir = std::path::Path::new(".moeb/traces");
        let files: Vec<_> = std::fs::read_dir(trace_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .collect();
        assert_eq!(files.len(), 1, "exactly one trace file should be written");

        let content = std::fs::read_to_string(files[0].path()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["version"], 1);
    }

    #[test]
    fn prune_keeps_n_most_recent() {
        let (_dir, _guard) = in_temp_dir();
        let dir = std::path::Path::new(".moeb/traces");
        std::fs::create_dir_all(dir).unwrap();

        let timestamps = [
            "20240101T000000z",
            "20240102T000000z",
            "20240103T000000z",
            "20240104T000000z",
            "20240105T000000z",
        ];
        for ts in &timestamps {
            std::fs::write(dir.join(format!("moeb.test-{}.json", ts)), "{}").unwrap();
        }

        prune_traces("moeb.test", 3);

        let remaining: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        assert_eq!(remaining.len(), 3);
        assert!(remaining.iter().any(|f| f.contains("20240103")));
        assert!(remaining.iter().any(|f| f.contains("20240104")));
        assert!(remaining.iter().any(|f| f.contains("20240105")));
    }

    #[test]
    fn apply_content_policy_embeds_by_default() {
        let result = Ok("hello".to_string());
        let (stored, hash, chars) = apply_content_policy("read_file", &result, FileContentMode::Embed);
        assert_eq!(stored, Some("hello".to_string()));
        assert_eq!(hash, None);
        assert_eq!(chars, 5);
    }

    #[test]
    fn apply_content_policy_hashes_when_mode_hash() {
        let result = Ok("hello".to_string());
        let (stored, hash, chars) = apply_content_policy("read_file", &result, FileContentMode::Hash);
        assert_eq!(stored, None);
        assert!(hash.is_some());
        assert_eq!(hash.unwrap().len(), 64);
        assert_eq!(chars, 5);
    }

    #[test]
    fn apply_content_policy_always_embeds_non_read_tools() {
        let result = Ok("wrote 10 bytes".to_string());
        let (stored, hash, _) = apply_content_policy("write_file", &result, FileContentMode::Hash);
        assert!(stored.is_some());
        assert_eq!(hash, None);
    }
}
