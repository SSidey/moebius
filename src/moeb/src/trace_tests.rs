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

    #[test]
    fn cache_usage_event_serde_round_trip() {
        let event = TraceEvent::CacheUsage(CacheUsageEvent {
            attempt: 1,
            turn: 2,
            cache_read_tokens: 1500,
            cache_created_tokens: 300,
        });
        let json = serde_json::to_string(&event).unwrap();
        let parsed: TraceEvent = serde_json::from_str(&json).unwrap();
        if let TraceEvent::CacheUsage(e) = parsed {
            assert_eq!(e.attempt, 1);
            assert_eq!(e.turn, 2);
            assert_eq!(e.cache_read_tokens, 1500);
            assert_eq!(e.cache_created_tokens, 300);
        } else {
            panic!("expected CacheUsage variant");
        }
        assert!(json.contains("\"type\":\"cache_usage\""), "type tag must be 'cache_usage', got: {}", json);
    }
