pub mod create_task_list;
pub mod grep_files;
pub mod list_directory;
pub mod read_file;
pub mod read_file_range;
pub mod read_files;
pub mod search_files;
pub mod update_task;
pub mod verify_rubrics;
pub mod write_file;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use anyhow::Result;
use sha2::Digest;

use crate::adapters::ToolDef;
use crate::ports::ToolExecutorPort;
use crate::run_state::SharedRunState;

pub const MAX_READ_BYTES: usize = 102_400;
pub const MAX_RANGE_LINES: usize = 300;

pub fn truncate_to_byte_limit(content: String, limit: usize) -> String {
    if content.len() <= limit {
        return content;
    }
    let mut boundary = limit;
    while boundary > 0 && !content.is_char_boundary(boundary) {
        boundary -= 1;
    }
    let total = content.len();
    let shown = boundary;
    format!(
        "{}\n[... truncated: {} of {} chars shown ...]",
        &content[..boundary],
        shown,
        total
    )
}

// ── ToolHandler ───────────────────────────────────────────────────────────────

pub trait ToolHandler: Send + Sync {
    fn name(&self) -> &'static str;
    fn definition(&self) -> ToolDef;
    fn execute(&self, args: &serde_json::Value, working_dir: &Path) -> Result<String>;
}

// ── ToolRegistry ──────────────────────────────────────────────────────────────

pub struct ToolRegistry {
    handlers: HashMap<&'static str, Box<dyn ToolHandler>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { handlers: HashMap::new() }
    }

    /// Register the ten standard tools (seven file tools + three task-list tools).
    pub fn standard(state: SharedRunState) -> Self {
        let mut r = Self::new();
        r.register(Box::new(read_file::ReadFileTool));
        r.register(Box::new(write_file::WriteFileTool));
        r.register(Box::new(list_directory::ListDirectoryTool));
        r.register(Box::new(search_files::SearchFilesTool));
        r.register(Box::new(grep_files::GrepFilesTool));
        r.register(Box::new(read_files::ReadFilesTool));
        r.register(Box::new(read_file_range::ReadFileRangeTool));
        r.register(Box::new(create_task_list::CreateTaskListTool { state: std::sync::Arc::clone(&state) }));
        r.register(Box::new(update_task::UpdateTaskTool { state: std::sync::Arc::clone(&state) }));
        r.register(Box::new(verify_rubrics::VerifyRubricsTool { state: std::sync::Arc::clone(&state) }));
        r
    }

    pub fn register(&mut self, handler: Box<dyn ToolHandler>) {
        self.handlers.insert(handler.name(), handler);
    }

    pub fn execute(
        &self,
        name: &str,
        args: &serde_json::Value,
        working_dir: &Path,
    ) -> Result<String> {
        match self.handlers.get(name) {
            Some(h) => h.execute(args, working_dir),
            None => anyhow::bail!(
                "Unknown tool '{}'. Available: {}",
                name,
                self.handlers.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
        }
    }

    /// Returns definitions in stable order.
    pub fn definitions(&self) -> Vec<ToolDef> {
        let order = [
            "read_file", "write_file", "list_directory",
            "search_files", "grep_files", "read_files", "read_file_range",
            "create_task_list", "update_task", "verify_rubrics",
        ];
        order.iter()
            .filter_map(|name| self.handlers.get(name).map(|h| h.definition()))
            .collect()
    }
}

// ── RealToolExecutor ──────────────────────────────────────────────────────────

/// Per-run in-memory deduplication cache.
/// Key: file path string as provided by the agent.
/// Value: (sha256_hex of content returned, turn number on which it was first sent).
type ContentCache = Mutex<HashMap<String, (String, u32)>>;

pub struct RealToolExecutor {
    pub state: SharedRunState,
    registry: ToolRegistry,
    cache: ContentCache,
    read_paths: Mutex<std::collections::HashSet<String>>,
}

impl RealToolExecutor {
    pub fn new(state: SharedRunState) -> Self {
        Self {
            registry: ToolRegistry::standard(std::sync::Arc::clone(&state)),
            cache: Mutex::new(HashMap::new()),
            read_paths: Mutex::new(std::collections::HashSet::new()),
            state,
        }
    }
}

impl ToolExecutorPort for RealToolExecutor {
    fn execute(
        &self,
        name: &str,
        _call_id: &str,
        args: &serde_json::Value,
        working_dir: &Path,
        current_turn: u32,
    ) -> Result<(String, bool)> {
        if name == "write_file" {
            if !self.state.lock().unwrap().task_list_created() {
                eprintln!(
                    "moeb: warning: write_file called without a prior create_task_list \
                     — consider calling create_task_list first to record your plan."
                );
            }
            if let Some(path) = args["path"].as_str() {
                let normalized = path.replace('\\', "/");
                if working_dir.join(path).exists() {
                    let read_paths = self.read_paths.lock().unwrap();
                    if !read_paths.contains(&normalized) {
                        return Ok((
                            format!(
                                "write_file rejected: '{}' exists on disk but has not been read \
                                 during this run. Call read_file on '{}' to obtain the current \
                                 content, then write a complete replacement that carries forward \
                                 all existing code not targeted by the specification.",
                                path, path
                            ),
                            false,
                        ));
                    }
                }
            }
        }

        let tool_result = self.registry.execute(name, args, working_dir);

        if name == "read_file" {
            if let Ok(ref content) = tool_result {
                {
                    let path_key = args["path"].as_str().unwrap_or("").to_string();
                    self.read_paths.lock().unwrap().insert(path_key.replace('\\', "/"));
                }
                let path_key = args["path"].as_str().unwrap_or("").to_string();
                let digest = hex::encode(sha2::Sha256::digest(content.as_bytes()));

                let mut cache = self.cache.lock().unwrap();
                if let Some((cached_hash, first_turn)) = cache.get(&path_key) {
                    if *cached_hash == digest {
                        let msg = format!(
                            "[CACHE HIT: {} — content already sent at turn {} \
                             (sha256: {}). File is unchanged. \
                             Use the content from that turn.]",
                            path_key, first_turn, cached_hash
                        );
                        return Ok((msg, true));
                    }
                }
                cache.insert(path_key, (digest, current_turn));
            }
        }

        if name == "read_files" {
            if let Some(paths) = args["paths"].as_array() {
                let mut rp = self.read_paths.lock().unwrap();
                for pv in paths {
                    if let Some(p) = pv.as_str() {
                        rp.insert(p.replace('\\', "/"));
                    }
                }
            }
        }

        Ok((tool_result?, false))
    }
}

#[cfg(test)]
#[path = "tool_executor_tests.rs"]
mod tests;
