    use super::*;
    use std::env;
    use std::fs;
    use std::sync::MutexGuard;
    use tempfile::TempDir;

    use crate::config::{tests::CWD_LOCK, MoebConfig, MOEB_DIR};

    fn in_temp_dir() -> (TempDir, MutexGuard<'static, ()>) {
        let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        env::set_current_dir(dir.path()).expect("set_current_dir");
        fs::create_dir_all(MOEB_DIR).expect("create .moeb");
        (dir, guard)
    }

    #[test]
    fn configure_run_retention_sets_value() {
        let (_dir, _guard) = in_temp_dir();
        run_configure("RUN_RETENTION", "10").unwrap();
        let loaded = MoebConfig::load().unwrap();
        assert_eq!(loaded.run_retention, Some(10));
    }

    #[test]
    fn configure_log_file_content_false() {
        let (_dir, _guard) = in_temp_dir();
        run_configure("LOG_FILE_CONTENT", "false").unwrap();
        let loaded = MoebConfig::load().unwrap();
        assert_eq!(loaded.log_file_content, Some(false));
    }

    #[test]
    fn configure_prompt_cache_false_round_trips() {
        let (_dir, _guard) = in_temp_dir();
        run_configure("PROMPT_CACHE", "false").unwrap();
        let loaded = MoebConfig::load().unwrap();
        assert_eq!(loaded.prompt_cache, Some(false));
        assert!(!loaded.effective_prompt_cache());
    }

    #[test]
    fn configure_rejects_invalid_retention() {
        let (_dir, _guard) = in_temp_dir();
        let err = run_configure("RUN_RETENTION", "abc").unwrap_err();
        assert!(
            err.to_string().contains("integer"),
            "expected 'integer' in error, got: {}",
            err
        );
    }

    #[test]
    fn configure_rejects_retention_below_minus_one() {
        let (_dir, _guard) = in_temp_dir();
        let err = run_configure("RUN_RETENTION", "-5").unwrap_err();
        assert!(
            err.to_string().contains("-1"),
            "expected '-1' in error, got: {}",
            err
        );
    }

    #[test]
    fn configure_rejects_unknown_key() {
        let (_dir, _guard) = in_temp_dir();
        let err = run_configure("TIMEOUT", "30").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Unknown configuration key"),
            "expected 'Unknown configuration key', got: {}",
            msg
        );
        assert!(
            msg.contains("RUN_RETENTION") && msg.contains("LOG_FILE_CONTENT") && msg.contains("PROMPT_CACHE"),
            "expected valid keys listed, got: {}",
            msg
        );
    }
