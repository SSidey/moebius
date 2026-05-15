pub mod grep_files;
pub mod list_directory;
pub mod read_file;
pub mod read_file_range;
pub mod read_files;
pub mod search_files;
pub mod write_file;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use anyhow::Result;
use sha2::Digest;

use crate::adapters::ToolDef;
use crate::ports::ToolExecutorPort;

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

    /// Register the seven standard file tools.
    pub fn standard() -> Self {
        let mut r = Self::new();
        r.register(Box::new(read_file::ReadFileTool));
        r.register(Box::new(write_file::WriteFileTool));
        r.register(Box::new(list_directory::ListDirectoryTool));
        r.register(Box::new(search_files::SearchFilesTool));
        r.register(Box::new(grep_files::GrepFilesTool));
        r.register(Box::new(read_files::ReadFilesTool));
        r.register(Box::new(read_file_range::ReadFileRangeTool));
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

    /// Returns definitions in the same stable order as the original `file_tools()`.
    pub fn definitions(&self) -> Vec<ToolDef> {
        let order = [
            "read_file", "write_file", "list_directory",
            "search_files", "grep_files", "read_files", "read_file_range",
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
    registry: ToolRegistry,
    cache: ContentCache,
    read_paths: Mutex<std::collections::HashSet<String>>,
}

impl RealToolExecutor {
    pub fn new() -> Self {
        Self {
            registry: ToolRegistry::standard(),
            cache: Mutex::new(HashMap::new()),
            read_paths: Mutex::new(std::collections::HashSet::new()),
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

    #[test]
    fn truncate_to_byte_limit_passes_short_content() {
        let s = "hello world".to_string();
        let result = truncate_to_byte_limit(s.clone(), MAX_READ_BYTES);
        assert_eq!(result, s);
    }

    #[test]
    fn truncate_to_byte_limit_truncates_long_content() {
        let s = "x".repeat(MAX_READ_BYTES + 1000);
        let result = truncate_to_byte_limit(s.clone(), MAX_READ_BYTES);
        assert!(result.len() <= MAX_READ_BYTES + 80);
        assert!(result.contains("[... truncated:"));
        assert!(result.contains(&format!("of {}", s.len())));
    }

    #[test]
    fn cache_hit_returns_backreference_for_unchanged_file() {
        let (_dir, _guard) = in_temp_dir();
        std::fs::write("cached.txt", "hello cache").unwrap();

        let executor = RealToolExecutor::new();
        let args = serde_json::json!({"path": "cached.txt"});
        let working_dir = std::path::Path::new(".");

        let (first, hit1) = executor.execute("read_file", "c1", &args, working_dir, 1).unwrap();
        assert!(!hit1, "first read must not be a cache hit");
        assert_eq!(first, "hello cache");

        let (second, hit2) = executor.execute("read_file", "c2", &args, working_dir, 2).unwrap();
        assert!(hit2, "second read of unchanged file must be a cache hit");
        assert!(second.starts_with("[CACHE HIT:"), "backreference must start with [CACHE HIT:");
        assert!(second.contains("turn 1"), "backreference must mention turn 1");
    }

    #[test]
    fn cache_miss_on_changed_file_returns_new_content() {
        let (_dir, _guard) = in_temp_dir();
        std::fs::write("changing.txt", "version one").unwrap();

        let executor = RealToolExecutor::new();
        let args = serde_json::json!({"path": "changing.txt"});
        let working_dir = std::path::Path::new(".");

        let (_, hit1) = executor.execute("read_file", "c1", &args, working_dir, 1).unwrap();
        assert!(!hit1);

        std::fs::write("changing.txt", "version two").unwrap();

        let (second, hit2) = executor.execute("read_file", "c2", &args, working_dir, 2).unwrap();
        assert!(!hit2, "changed file must not be a cache hit");
        assert_eq!(second, "version two");
    }

    #[test]
    fn non_read_file_tools_always_return_cache_hit_false() {
        let (_dir, _guard) = in_temp_dir();
        std::fs::write("target.txt", "content").unwrap();
        std::fs::create_dir_all("src").unwrap();

        let executor = RealToolExecutor::new();
        let working_dir = std::path::Path::new(".");

        let args = serde_json::json!({"path": "src", "extension": "txt"});
        let (_, hit) = executor.execute("search_files", "c1", &args, working_dir, 1).unwrap();
        assert!(!hit, "search_files must never return cache_hit true");
    }

    #[test]
    fn write_file_rejected_for_existing_file_not_yet_read() {
        let (_dir, _guard) = in_temp_dir();
        std::fs::write("existing.rs", "fn old() {}").unwrap();

        let executor = RealToolExecutor::new();
        let args = serde_json::json!({"path": "existing.rs", "content": "fn new() {}"});
        let (msg, _) = executor.execute("write_file", "c1", &args, Path::new("."), 1).unwrap();
        assert!(msg.contains("rejected"), "must reject unread existing file; got: {}", msg);
        assert!(msg.contains("existing.rs"), "rejection must name the file; got: {}", msg);
        assert!(msg.contains("read_file"), "rejection must instruct to call read_file; got: {}", msg);
        let on_disk = std::fs::read_to_string("existing.rs").unwrap();
        assert_eq!(on_disk, "fn old() {}", "file must not be modified on rejection");
    }

    #[test]
    fn write_file_allowed_after_read_file() {
        let (_dir, _guard) = in_temp_dir();
        std::fs::write("target.rs", "fn original() {}").unwrap();

        let executor = RealToolExecutor::new();
        let read_args = serde_json::json!({"path": "target.rs"});
        executor.execute("read_file", "c1", &read_args, Path::new("."), 1).unwrap();

        let write_args = serde_json::json!({"path": "target.rs", "content": "fn updated() {}"});
        let (msg, _) = executor.execute("write_file", "c2", &write_args, Path::new("."), 2).unwrap();
        assert!(!msg.contains("rejected"), "write after read must succeed; got: {}", msg);
        assert_eq!(std::fs::read_to_string("target.rs").unwrap(), "fn updated() {}");
    }

    #[test]
    fn write_file_allowed_for_new_file_without_prior_read() {
        let (_dir, _guard) = in_temp_dir();

        let executor = RealToolExecutor::new();
        let args = serde_json::json!({"path": "brand_new.rs", "content": "fn fresh() {}"});
        let (msg, _) = executor.execute("write_file", "c1", &args, Path::new("."), 1).unwrap();
        assert!(!msg.contains("rejected"), "new file must not require prior read; got: {}", msg);
        assert!(Path::new("brand_new.rs").exists());
    }

    #[test]
    fn write_file_allowed_after_read_files_batch() {
        let (_dir, _guard) = in_temp_dir();
        std::fs::write("a.rs", "fn a() {}").unwrap();
        std::fs::write("b.rs", "fn b() {}").unwrap();

        let executor = RealToolExecutor::new();
        let read_args = serde_json::json!({"paths": ["a.rs", "b.rs"]});
        executor.execute("read_files", "c1", &read_args, Path::new("."), 1).unwrap();

        let write_args = serde_json::json!({"path": "b.rs", "content": "fn b_updated() {}"});
        let (msg, _) = executor.execute("write_file", "c2", &write_args, Path::new("."), 2).unwrap();
        assert!(!msg.contains("rejected"), "write after read_files must succeed; got: {}", msg);
    }
}
