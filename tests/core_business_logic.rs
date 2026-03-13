//! Tests for core business logic: types, templates, serialization, daemon protocol, and public API.
//!
//! These tests exercise the pure logic in `types.rs`, `templates.rs`, `daemon.rs`,
//! `theme.rs`, and `app.rs` — everything that doesn't require a real PTY or external CLI.

use obelisk::daemon::{DaemonCmd, DaemonResp};
use obelisk::templates;
use obelisk::types::*;

use std::path::PathBuf;

// ═══════════════════════════════════════════════════════════════════
// Types — enum cycling & filtering
// ═══════════════════════════════════════════════════════════════════

#[test]
fn sort_mode_cycles_through_all_variants() {
    let start = SortMode::Priority;
    let second = start.next();
    assert_eq!(second, SortMode::Type);
    let third = second.next();
    assert_eq!(third, SortMode::Age);
    let fourth = third.next();
    assert_eq!(fourth, SortMode::Name);
    let back = fourth.next();
    assert_eq!(back, SortMode::Priority);
}

#[test]
fn sort_mode_labels_are_non_empty() {
    for mode in [SortMode::Priority, SortMode::Type, SortMode::Age, SortMode::Name] {
        assert!(!mode.label().is_empty());
    }
}

#[test]
fn runtime_cycles_through_all_variants() {
    let start = Runtime::ClaudeCode;
    assert_eq!(start.next(), Runtime::Codex);
    assert_eq!(start.next().next(), Runtime::Copilot);
    assert_eq!(start.next().next().next(), Runtime::ClaudeCode);
}

#[test]
fn runtime_name_matches_display() {
    for rt in [Runtime::ClaudeCode, Runtime::Codex, Runtime::Copilot] {
        assert_eq!(format!("{}", rt), rt.name());
    }
}

#[test]
fn runtime_models_are_non_empty() {
    for rt in [Runtime::ClaudeCode, Runtime::Codex, Runtime::Copilot] {
        assert!(!rt.models().is_empty(), "{} should have at least one model", rt.name());
    }
}

#[test]
fn agent_status_symbol_is_non_empty() {
    for status in [
        AgentStatus::Starting,
        AgentStatus::Running,
        AgentStatus::Completed,
        AgentStatus::Failed,
    ] {
        assert!(!status.symbol().is_empty());
    }
}

#[test]
fn agent_phase_ordering_is_correct() {
    assert!(AgentPhase::Detecting < AgentPhase::Claiming);
    assert!(AgentPhase::Claiming < AgentPhase::Worktree);
    assert!(AgentPhase::Worktree < AgentPhase::Implementing);
    assert!(AgentPhase::Implementing < AgentPhase::Verifying);
    assert!(AgentPhase::Verifying < AgentPhase::Merging);
    assert!(AgentPhase::Merging < AgentPhase::Closing);
    assert!(AgentPhase::Closing < AgentPhase::Done);
}

#[test]
fn agent_phase_short_labels_are_p0_through_p7() {
    let phases = [
        AgentPhase::Detecting,
        AgentPhase::Claiming,
        AgentPhase::Worktree,
        AgentPhase::Implementing,
        AgentPhase::Verifying,
        AgentPhase::Merging,
        AgentPhase::Closing,
        AgentPhase::Done,
    ];
    for (i, phase) in phases.iter().enumerate() {
        assert_eq!(phase.short(), format!("P{}", i));
    }
}

#[test]
fn agent_phase_label_is_non_empty() {
    let phases = [
        AgentPhase::Detecting, AgentPhase::Claiming, AgentPhase::Worktree,
        AgentPhase::Implementing, AgentPhase::Verifying, AgentPhase::Merging,
        AgentPhase::Closing, AgentPhase::Done,
    ];
    for phase in phases {
        assert!(!phase.label().is_empty());
    }
}

#[test]
fn agent_status_filter_cycles_and_wraps() {
    let start = AgentStatusFilter::All;
    let mut current = start;
    let mut seen = vec![current];
    for _ in 0..5 {
        current = current.next();
        seen.push(current);
    }
    assert_eq!(seen[5], AgentStatusFilter::All);
}

#[test]
fn agent_status_filter_all_matches_everything() {
    let filter = AgentStatusFilter::All;
    assert!(filter.matches(AgentStatus::Starting));
    assert!(filter.matches(AgentStatus::Running));
    assert!(filter.matches(AgentStatus::Completed));
    assert!(filter.matches(AgentStatus::Failed));
}

#[test]
fn agent_status_filter_specific_only_matches_own() {
    assert!(AgentStatusFilter::Running.matches(AgentStatus::Running));
    assert!(!AgentStatusFilter::Running.matches(AgentStatus::Failed));

    assert!(AgentStatusFilter::Failed.matches(AgentStatus::Failed));
    assert!(!AgentStatusFilter::Failed.matches(AgentStatus::Completed));

    assert!(AgentStatusFilter::Completed.matches(AgentStatus::Completed));
    assert!(!AgentStatusFilter::Completed.matches(AgentStatus::Starting));

    assert!(AgentStatusFilter::Starting.matches(AgentStatus::Starting));
    assert!(!AgentStatusFilter::Starting.matches(AgentStatus::Running));
}

#[test]
fn agent_status_filter_labels_are_non_empty() {
    for f in [
        AgentStatusFilter::All, AgentStatusFilter::Running,
        AgentStatusFilter::Failed, AgentStatusFilter::Completed,
        AgentStatusFilter::Starting,
    ] {
        assert!(!f.label().is_empty());
    }
}

#[test]
fn worktree_sort_mode_cycles() {
    assert_eq!(WorktreeSortMode::Age.next(), WorktreeSortMode::Status);
    assert_eq!(WorktreeSortMode::Status.next(), WorktreeSortMode::Age);
}

#[test]
fn worktree_sort_mode_labels_are_non_empty() {
    assert!(!WorktreeSortMode::Age.label().is_empty());
    assert!(!WorktreeSortMode::Status.label().is_empty());
}

#[test]
fn log_category_labels_are_non_empty() {
    for cat in [
        LogCategory::System, LogCategory::Incoming, LogCategory::Deploy,
        LogCategory::Complete, LogCategory::Alert, LogCategory::Poll,
    ] {
        assert!(!cat.label().is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════
// Templates — resolution and interpolation
// ═══════════════════════════════════════════════════════════════════

#[test]
fn template_resolve_falls_back_to_builtin() {
    let dir = PathBuf::from("/tmp/obelisk-test-nonexistent-dir");
    let resolved = templates::resolve(&dir, "bug");
    assert!(
        resolved.name.contains("built-in"),
        "expected built-in marker in name, got: {}",
        resolved.name
    );
    assert!(!resolved.content.is_empty());
}

#[test]
fn template_resolve_each_builtin_type() {
    let dir = PathBuf::from("/tmp/obelisk-test-nonexistent-dir");
    for itype in ["bug", "feature", "task", "chore", "epic"] {
        let resolved = templates::resolve(&dir, itype);
        assert!(!resolved.content.is_empty(), "{} template should have content", itype);
        assert!(resolved.name.contains("built-in"));
    }
}

#[test]
fn template_resolve_unknown_type_falls_back_to_task() {
    let dir = PathBuf::from("/tmp/obelisk-test-nonexistent-dir");
    let resolved = templates::resolve(&dir, "nonexistent_type");
    assert!(resolved.name.contains("nonexistent_type"));
    assert!(!resolved.content.is_empty());
}

#[test]
fn template_resolve_normalizes_case() {
    let dir = PathBuf::from("/tmp/obelisk-test-nonexistent-dir");
    let upper = templates::resolve(&dir, "BUG");
    let lower = templates::resolve(&dir, "bug");
    assert_eq!(upper.content, lower.content);
}

#[test]
fn template_resolve_normalizes_whitespace() {
    let dir = PathBuf::from("/tmp/obelisk-test-nonexistent-dir");
    let trimmed = templates::resolve(&dir, "  bug  ");
    let normal = templates::resolve(&dir, "bug");
    assert_eq!(trimmed.content, normal.content);
}

#[test]
fn template_interpolate_replaces_all_vars() {
    let template = "Issue: {id}\nTitle: {title}\nPriority: {priority}\nDesc: {description}";
    let result = templates::interpolate(template, "abc-123", "Fix bug", Some(1), Some("details"));
    assert!(result.contains("abc-123"));
    assert!(result.contains("Fix bug"));
    assert!(result.contains("1"));
    assert!(result.contains("details"));
}

#[test]
fn template_interpolate_handles_none_priority_and_description() {
    let template = "P={priority} D={description}";
    let result = templates::interpolate(template, "id", "title", None, None);
    assert!(result.contains("P=?"));
    assert!(result.contains("D="));
}

#[test]
fn template_interpolate_multiple_occurrences() {
    let template = "{id} then {id} again";
    let result = templates::interpolate(template, "x", "t", None, None);
    assert_eq!(result, "x then x again");
}

#[test]
fn template_default_dir_is_obelisk_templates() {
    let dir = templates::default_template_dir();
    assert!(dir.to_str().unwrap().contains("templates"));
}

#[test]
fn template_resolve_reads_custom_file() {
    let dir = std::env::temp_dir().join("obelisk-test-templates");
    let _ = std::fs::create_dir_all(&dir);
    let custom_path = dir.join("bug.md");
    std::fs::write(&custom_path, "CUSTOM TEMPLATE {id}").unwrap();

    let resolved = templates::resolve(&dir, "bug");
    assert_eq!(resolved.name, "bug.md");
    assert!(resolved.content.contains("CUSTOM TEMPLATE"));
    assert!(!resolved.name.contains("built-in"));

    let _ = std::fs::remove_file(&custom_path);
    let _ = std::fs::remove_dir(&dir);
}

// ═══════════════════════════════════════════════════════════════════
// BeadTask deserialization
// ═══════════════════════════════════════════════════════════════════

#[test]
fn bead_task_deserializes_minimal_json() {
    let json = r#"{"id":"t-1","title":"Test","status":"open"}"#;
    let task: BeadTask = serde_json::from_str(json).unwrap();
    assert_eq!(task.id, "t-1");
    assert_eq!(task.title, "Test");
    assert_eq!(task.status, "open");
    assert!(task.priority.is_none());
    assert!(task.issue_type.is_none());
}

#[test]
fn bead_task_deserializes_full_json() {
    let json = r#"{
        "id": "t-2",
        "title": "Full task",
        "status": "in_progress",
        "priority": 1,
        "issue_type": "bug",
        "assignee": "agent-01",
        "labels": ["urgent", "backend"],
        "description": "Fix the thing",
        "created_at": "2026-03-12T10:00:00Z"
    }"#;
    let task: BeadTask = serde_json::from_str(json).unwrap();
    assert_eq!(task.priority, Some(1));
    assert_eq!(task.issue_type.as_deref(), Some("bug"));
    assert_eq!(task.assignee.as_deref(), Some("agent-01"));
    assert_eq!(task.labels.as_ref().unwrap().len(), 2);
}

#[test]
fn bead_task_list_deserializes() {
    let json = r#"[
        {"id":"a","title":"A","status":"open"},
        {"id":"b","title":"B","status":"closed","priority":0}
    ]"#;
    let tasks: Vec<BeadTask> = serde_json::from_str(json).unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[1].priority, Some(0));
}

// ═══════════════════════════════════════════════════════════════════
// SessionRecord serialization round-trip
// ═══════════════════════════════════════════════════════════════════

#[test]
fn session_record_round_trip() {
    let record = SessionRecord {
        session_id: "sess-test".into(),
        started_at: "2026-03-12T10:00:00Z".into(),
        ended_at: "2026-03-12T11:00:00Z".into(),
        total_completed: 3,
        total_failed: 1,
        agents: vec![SessionAgent {
            task_id: "t-1".into(),
            runtime: "CLAUDE".into(),
            model: "claude-sonnet-4-6".into(),
            elapsed_secs: 120,
            status: "Completed".into(),
        }],
    };

    let json = serde_json::to_string(&record).unwrap();
    let restored: SessionRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.session_id, "sess-test");
    assert_eq!(restored.total_completed, 3);
    assert_eq!(restored.agents.len(), 1);
    assert_eq!(restored.agents[0].elapsed_secs, 120);
}

#[test]
fn session_agent_minimal_deserialization() {
    let json = r#"{"task_id":"t","runtime":"CLAUDE","model":"m","elapsed_secs":60,"status":"Completed"}"#;
    let agent: SessionAgent = serde_json::from_str(json).unwrap();
    assert_eq!(agent.task_id, "t");
    assert_eq!(agent.elapsed_secs, 60);
}

#[test]
fn session_record_jsonl_parsing() {
    // Verify multi-line JSONL parsing (same pattern as load_history_sessions)
    let jsonl = r#"{"session_id":"s1","started_at":"a","ended_at":"b","total_completed":1,"total_failed":0,"agents":[]}
{"session_id":"s2","started_at":"c","ended_at":"d","total_completed":2,"total_failed":1,"agents":[]}"#;

    let records: Vec<SessionRecord> = jsonl
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<SessionRecord>(line).ok())
        .collect();

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].session_id, "s1");
    assert_eq!(records[1].total_completed, 2);
}

// ═══════════════════════════════════════════════════════════════════
// DepNode deserialization
// ═══════════════════════════════════════════════════════════════════

#[test]
fn dep_node_deserializes_with_defaults() {
    let json = r#"{"id":"d-1","title":"Node","status":"open"}"#;
    let node: DepNode = serde_json::from_str(json).unwrap();
    assert_eq!(node.depth, 0);
    assert!(node.parent_id.is_none());
    assert!(!node.truncated);
}

#[test]
fn dep_node_deserializes_full() {
    let json = r#"{
        "id":"d-2","title":"Child","status":"open",
        "priority":1,"issue_type":"task","depth":2,
        "parent_id":"d-1","truncated":true
    }"#;
    let node: DepNode = serde_json::from_str(json).unwrap();
    assert_eq!(node.depth, 2);
    assert_eq!(node.parent_id.as_deref(), Some("d-1"));
    assert!(node.truncated);
}

// ═══════════════════════════════════════════════════════════════════
// DiffData
// ═══════════════════════════════════════════════════════════════════

#[test]
fn diff_data_empty_state() {
    let diff = DiffData {
        lines: Vec::new(),
        files_changed: 0,
        insertions: 0,
        deletions: 0,
        changed_files: Vec::new(),
    };
    assert_eq!(diff.files_changed, 0);
    assert!(diff.lines.is_empty());
    assert!(diff.changed_files.is_empty());
}

#[test]
fn diff_data_with_changes() {
    let diff = DiffData {
        lines: vec!["+new line".into(), "-old line".into()],
        files_changed: 1,
        insertions: 1,
        deletions: 1,
        changed_files: vec!["src/main.rs".into()],
    };
    assert_eq!(diff.files_changed, 1);
    assert_eq!(diff.insertions, 1);
    assert_eq!(diff.deletions, 1);
    assert_eq!(diff.changed_files[0], "src/main.rs");
}

// ═══════════════════════════════════════════════════════════════════
// Runtime PTY command building
// ═══════════════════════════════════════════════════════════════════

#[test]
fn build_pty_command_claude_has_correct_args() {
    let cmd = obelisk::runtime::build_pty_command(
        Runtime::ClaudeCode,
        "claude-sonnet-4-6",
        "system prompt",
        "user prompt",
    );
    let argv = cmd.get_argv();
    // First arg should be the CLI binary name
    let args_str: Vec<String> = argv.iter().map(|a| a.to_string_lossy().to_string()).collect();
    assert!(args_str.iter().any(|a| a.contains("claude")));
    assert!(args_str.contains(&"user prompt".to_string()));
    assert!(args_str.contains(&"--model".to_string()));
    assert!(args_str.contains(&"claude-sonnet-4-6".to_string()));
    assert!(args_str.contains(&"--dangerously-skip-permissions".to_string()));
}

#[test]
fn build_pty_command_codex_has_interactive_mode() {
    let cmd = obelisk::runtime::build_pty_command(
        Runtime::Codex,
        "gpt-5.4",
        "system prompt",
        "user prompt",
    );
    let argv = cmd.get_argv();
    let args_str: Vec<String> = argv.iter().map(|a| a.to_string_lossy().to_string()).collect();
    // Interactive mode: no "exec" subcommand, just direct args
    assert!(!args_str.contains(&"exec".to_string()));
    assert!(args_str.contains(&"-m".to_string()));
}

#[test]
fn build_pty_command_copilot_has_yolo_flag() {
    let cmd = obelisk::runtime::build_pty_command(
        Runtime::Copilot,
        "gpt-5",
        "system prompt",
        "user prompt",
    );
    let argv = cmd.get_argv();
    let args_str: Vec<String> = argv.iter().map(|a| a.to_string_lossy().to_string()).collect();
    assert!(args_str.contains(&"--yolo".to_string()));
    assert!(args_str.contains(&"-i".to_string()));
}

// ═══════════════════════════════════════════════════════════════════
// Daemon protocol — command serialization & deserialization
// ═══════════════════════════════════════════════════════════════════

#[test]
fn daemon_cmd_status_serialization_round_trip() {
    let cmd = DaemonCmd::Status;
    let json = serde_json::to_string(&cmd).unwrap();
    assert!(json.contains(r#""cmd":"status"#));
    let deserialized: DaemonCmd = serde_json::from_str(&json).unwrap();
    assert!(matches!(deserialized, DaemonCmd::Status));
}

#[test]
fn daemon_cmd_agents_serialization_round_trip() {
    let cmd = DaemonCmd::Agents;
    let json = serde_json::to_string(&cmd).unwrap();
    assert!(json.contains(r#""cmd":"agents"#));
    let deserialized: DaemonCmd = serde_json::from_str(&json).unwrap();
    assert!(matches!(deserialized, DaemonCmd::Agents));
}

#[test]
fn daemon_cmd_spawn_serialization_round_trip() {
    let cmd = DaemonCmd::Spawn {
        issue_id: "obelisk-abc".into(),
    };
    let json = serde_json::to_string(&cmd).unwrap();
    assert!(json.contains("obelisk-abc"));
    let deserialized: DaemonCmd = serde_json::from_str(&json).unwrap();
    match deserialized {
        DaemonCmd::Spawn { issue_id } => assert_eq!(issue_id, "obelisk-abc"),
        _ => panic!("expected Spawn variant"),
    }
}

#[test]
fn daemon_cmd_kill_serialization_round_trip() {
    let cmd = DaemonCmd::Kill { agent_id: 42 };
    let json = serde_json::to_string(&cmd).unwrap();
    assert!(json.contains("42"));
    let deserialized: DaemonCmd = serde_json::from_str(&json).unwrap();
    match deserialized {
        DaemonCmd::Kill { agent_id } => assert_eq!(agent_id, 42),
        _ => panic!("expected Kill variant"),
    }
}

#[test]
fn daemon_cmd_stop_serialization_round_trip() {
    let cmd = DaemonCmd::Stop;
    let json = serde_json::to_string(&cmd).unwrap();
    let deserialized: DaemonCmd = serde_json::from_str(&json).unwrap();
    assert!(matches!(deserialized, DaemonCmd::Stop));
}

#[test]
fn daemon_resp_ok_with_message() {
    let resp = DaemonResp {
        ok: true,
        message: Some("success".into()),
        data: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("true"));
    assert!(json.contains("success"));
    // data should be skipped when None
    assert!(!json.contains("data"));
}

#[test]
fn daemon_resp_error_with_data() {
    let resp = DaemonResp {
        ok: false,
        message: Some("not found".into()),
        data: Some(serde_json::json!({"details": "missing"})),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let restored: DaemonResp = serde_json::from_str(&json).unwrap();
    assert!(!restored.ok);
    assert_eq!(restored.message.as_deref(), Some("not found"));
    assert!(restored.data.is_some());
}

#[test]
fn daemon_resp_skip_serializing_none_fields() {
    let resp = DaemonResp {
        ok: true,
        message: None,
        data: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(!json.contains("message"));
    assert!(!json.contains("data"));
}

#[test]
fn daemon_cmd_rejects_unknown_command() {
    let json = r#"{"cmd":"unknown_cmd"}"#;
    let result = serde_json::from_str::<DaemonCmd>(json);
    assert!(result.is_err());
}

#[test]
fn daemon_cmd_spawn_rejects_missing_issue_id() {
    let json = r#"{"cmd":"spawn"}"#;
    let result = serde_json::from_str::<DaemonCmd>(json);
    assert!(result.is_err());
}

#[test]
fn daemon_cmd_kill_rejects_missing_agent_id() {
    let json = r#"{"cmd":"kill"}"#;
    let result = serde_json::from_str::<DaemonCmd>(json);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════
// Daemon port file
// ═══════════════════════════════════════════════════════════════════

#[test]
fn port_file_path_is_under_beads() {
    let path = obelisk::daemon::port_file_path();
    assert!(path.to_str().unwrap().contains(".beads"));
    assert!(path.to_str().unwrap().contains("obelisk.port"));
}

#[test]
fn read_daemon_port_fails_when_no_port_file() {
    // Ensure we're working from a temp dir where the port file doesn't exist
    let result = obelisk::daemon::read_daemon_port();
    // This may or may not fail depending on whether we're in a beads project
    // But the function should not panic
    if let Err(e) = result {
        assert!(e.contains("not running") || e.contains("invalid port"));
    }
}

// ═══════════════════════════════════════════════════════════════════
// Runtime — command argument correctness
// ═══════════════════════════════════════════════════════════════════

#[test]
fn build_pty_command_claude_includes_system_prompt() {
    let cmd = obelisk::runtime::build_pty_command(
        Runtime::ClaudeCode,
        "claude-sonnet-4-6",
        "You are a coding agent",
        "Fix the bug in main.rs",
    );
    let argv = cmd.get_argv();
    let args_str: Vec<String> = argv.iter().map(|a| a.to_string_lossy().to_string()).collect();
    assert!(args_str.contains(&"--append-system-prompt".to_string()));
    assert!(args_str.contains(&"You are a coding agent".to_string()));
}

#[test]
fn build_pty_command_codex_combines_prompts() {
    let cmd = obelisk::runtime::build_pty_command(
        Runtime::Codex,
        "gpt-5.4",
        "system prompt text",
        "user prompt text",
    );
    let argv = cmd.get_argv();
    let args_str: Vec<String> = argv.iter().map(|a| a.to_string_lossy().to_string()).collect();
    // Codex should have a combined prompt containing both system and user
    let has_combined = args_str.iter().any(|a| a.contains("user prompt text") && a.contains("system prompt text"));
    assert!(has_combined, "codex should combine user and system prompts");
}

#[test]
fn build_pty_command_copilot_combines_prompts() {
    let cmd = obelisk::runtime::build_pty_command(
        Runtime::Copilot,
        "gpt-5",
        "system prompt text",
        "user prompt text",
    );
    let argv = cmd.get_argv();
    let args_str: Vec<String> = argv.iter().map(|a| a.to_string_lossy().to_string()).collect();
    let has_combined = args_str.iter().any(|a| a.contains("user prompt text") && a.contains("system prompt text"));
    assert!(has_combined, "copilot should combine user and system prompts");
}

#[test]
fn build_pty_command_codex_has_model_flag() {
    let cmd = obelisk::runtime::build_pty_command(
        Runtime::Codex,
        "my-model",
        "sys",
        "usr",
    );
    let argv = cmd.get_argv();
    let args_str: Vec<String> = argv.iter().map(|a| a.to_string_lossy().to_string()).collect();
    assert!(args_str.contains(&"-m".to_string()));
    assert!(args_str.contains(&"my-model".to_string()));
}

#[test]
fn build_pty_command_copilot_has_model_flag() {
    let cmd = obelisk::runtime::build_pty_command(
        Runtime::Copilot,
        "my-model",
        "sys",
        "usr",
    );
    let argv = cmd.get_argv();
    let args_str: Vec<String> = argv.iter().map(|a| a.to_string_lossy().to_string()).collect();
    assert!(args_str.contains(&"--model".to_string()));
    assert!(args_str.contains(&"my-model".to_string()));
}

#[test]
fn build_pty_command_codex_has_bypass_flag() {
    let cmd = obelisk::runtime::build_pty_command(
        Runtime::Codex,
        "m",
        "s",
        "u",
    );
    let argv = cmd.get_argv();
    let args_str: Vec<String> = argv.iter().map(|a| a.to_string_lossy().to_string()).collect();
    assert!(args_str.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
}

// ═══════════════════════════════════════════════════════════════════
// Types — additional edge cases
// ═══════════════════════════════════════════════════════════════════

#[test]
fn bead_task_deserializes_empty_labels() {
    let json = r#"{"id":"t-1","title":"Test","status":"open","labels":[]}"#;
    let task: BeadTask = serde_json::from_str(json).unwrap();
    assert_eq!(task.labels.as_ref().unwrap().len(), 0);
}

#[test]
fn bead_task_deserializes_null_optional_fields() {
    let json = r#"{"id":"t-1","title":"Test","status":"open","priority":null,"issue_type":null}"#;
    let task: BeadTask = serde_json::from_str(json).unwrap();
    assert!(task.priority.is_none());
    assert!(task.issue_type.is_none());
}

#[test]
fn dep_node_deserializes_with_all_optional_fields() {
    let json = r#"{
        "id":"d-1","title":"Node","status":"open",
        "priority":0,"issue_type":"epic",
        "depth":3,"parent_id":"d-0","truncated":false
    }"#;
    let node: DepNode = serde_json::from_str(json).unwrap();
    assert_eq!(node.priority, Some(0));
    assert_eq!(node.issue_type.as_deref(), Some("epic"));
    assert_eq!(node.depth, 3);
}

#[test]
fn diff_data_preserves_line_order() {
    let diff = DiffData {
        lines: vec!["first".into(), "second".into(), "third".into()],
        files_changed: 1,
        insertions: 2,
        deletions: 1,
        changed_files: vec!["file.rs".into()],
    };
    assert_eq!(diff.lines[0], "first");
    assert_eq!(diff.lines[2], "third");
}

#[test]
fn session_record_with_multiple_agents() {
    let record = SessionRecord {
        session_id: "s-multi".into(),
        started_at: "2026-03-12T10:00:00Z".into(),
        ended_at: "2026-03-12T12:00:00Z".into(),
        total_completed: 5,
        total_failed: 2,
        agents: vec![
            SessionAgent {
                task_id: "t-1".into(),
                runtime: "CLAUDE".into(),
                model: "claude-sonnet-4-6".into(),
                elapsed_secs: 120,
                status: "Completed".into(),
            },
            SessionAgent {
                task_id: "t-2".into(),
                runtime: "CODEX".into(),
                model: "gpt-5.4".into(),
                elapsed_secs: 300,
                status: "Failed".into(),
            },
            SessionAgent {
                task_id: "t-3".into(),
                runtime: "COPILOT".into(),
                model: "gpt-5".into(),
                elapsed_secs: 45,
                status: "Completed".into(),
            },
        ],
    };
    let json = serde_json::to_string(&record).unwrap();
    let restored: SessionRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.agents.len(), 3);
    assert_eq!(restored.agents[1].runtime, "CODEX");
    assert_eq!(restored.agents[2].elapsed_secs, 45);
}

#[test]
fn runtime_all_models_are_valid_strings() {
    for rt in [Runtime::ClaudeCode, Runtime::Codex, Runtime::Copilot] {
        for model in rt.models() {
            assert!(!model.is_empty(), "{} has an empty model", rt.name());
            // Models should not contain whitespace
            assert!(!model.contains(' '), "{} model '{}' contains space", rt.name(), model);
        }
    }
}

#[test]
fn agent_status_filter_round_trips_all_variants() {
    let mut filter = AgentStatusFilter::All;
    let mut labels = vec![filter.label().to_string()];
    for _ in 0..5 {
        filter = filter.next();
        labels.push(filter.label().to_string());
    }
    // After 5 steps (All→Running→Failed→Completed→Starting→All), should be back
    assert_eq!(filter, AgentStatusFilter::All);
    // First 5 labels should all be unique (the 6th duplicates the 1st)
    let unique: std::collections::HashSet<_> = labels[..5].iter().collect();
    assert_eq!(unique.len(), 5, "all filter labels should be unique");
}
