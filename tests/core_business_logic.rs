//! Tests for core business logic: types, templates, serialization, and public API.
//!
//! These tests exercise the pure logic in `types.rs`, `templates.rs`, and `app.rs`
//! — everything that doesn't require a real PTY or external CLI.

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
fn build_pty_command_codex_has_exec_mode() {
    let cmd = obelisk::runtime::build_pty_command(
        Runtime::Codex,
        "gpt-5.4",
        "system prompt",
        "user prompt",
    );
    let argv = cmd.get_argv();
    let args_str: Vec<String> = argv.iter().map(|a| a.to_string_lossy().to_string()).collect();
    assert!(args_str.contains(&"exec".to_string()));
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
    assert!(args_str.contains(&"-p".to_string()));
}
