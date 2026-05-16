    use super::*;
    use std::env;
    use std::sync::MutexGuard;
    use tempfile::TempDir;
    use crate::config::tests::CWD_LOCK;
    use crate::run_state::new_shared_run_state;

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
    fn standard_registry_has_ten_tools() {
        let state = new_shared_run_state();
        assert_eq!(ToolRegistry::standard(state).definitions().len(), 11);
    }

    #[test]
    fn cache_hit_returns_backreference_for_unchanged_file() {
        let (_dir, _guard) = in_temp_dir();
        std::fs::write("cached.txt", "hello cache").unwrap();

        let executor = RealToolExecutor::new(new_shared_run_state());
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

        let executor = RealToolExecutor::new(new_shared_run_state());
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

        let executor = RealToolExecutor::new(new_shared_run_state());
        let working_dir = std::path::Path::new(".");

        let args = serde_json::json!({"path": "src", "extension": "txt"});
        let (_, hit) = executor.execute("search_files", "c1", &args, working_dir, 1).unwrap();
        assert!(!hit, "search_files must never return cache_hit true");
    }

    #[test]
    fn write_file_rejected_for_existing_file_not_yet_read() {
        let (_dir, _guard) = in_temp_dir();
        std::fs::write("existing.rs", "fn old() {}").unwrap();

        let executor = RealToolExecutor::new(new_shared_run_state());
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

        let executor = RealToolExecutor::new(new_shared_run_state());
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

        let executor = RealToolExecutor::new(new_shared_run_state());
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

        let executor = RealToolExecutor::new(new_shared_run_state());
        let read_args = serde_json::json!({"paths": ["a.rs", "b.rs"]});
        executor.execute("read_files", "c1", &read_args, Path::new("."), 1).unwrap();

        let write_args = serde_json::json!({"path": "b.rs", "content": "fn b_updated() {}"});
        let (msg, _) = executor.execute("write_file", "c2", &write_args, Path::new("."), 2).unwrap();
        assert!(!msg.contains("rejected"), "write after read_files must succeed; got: {}", msg);
    }
