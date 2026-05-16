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
    #[serde(default)]
    pub cache_hit: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheUsageEvent {
    pub attempt: u32,
    pub turn: u32,
    /// Tokens served from cache (saves full input-token cost).
    pub cache_read_tokens: u64,
    /// Tokens written to cache (Anthropic only; 0 for OpenAI automatic caching).
    pub cache_created_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionEvent {
    pub attempt: u32,
    pub turn: u32,
    pub chars_before: usize,
    pub chars_after: usize,
    pub messages_compacted: usize,
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
    CacheUsage(CacheUsageEvent),
    Compaction(CompactionEvent),
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
#[path = "trace_tests.rs"]
mod tests;
