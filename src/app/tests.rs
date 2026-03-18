use super::*;
use std::collections::VecDeque;

fn test_task() -> BeadTask {
    BeadTask {
        id: "test-001".into(),
        title: "test task".into(),
        status: "open".into(),
        priority: None,
        issue_type: None,
        assignee: None,
        labels: None,
        description: None,
        created_at: None,
    }
}

fn test_agent(id: usize, status: AgentStatus) -> AgentInstance {
    AgentInstance {
        id,
        unit_number: 1,
        task: test_task(),
        runtime: Runtime::ClaudeCode,
        model: "test-model".into(),
        status,
        phase: AgentPhase::Detecting,
        output: VecDeque::new(),
        started_at: std::time::Instant::now(),
        elapsed_secs: 0,
        exit_code: None,
        pid: None,
        retry_count: 0,
        worktree_path: None,
        worktree_cleaned: false,
        template_name: String::new(),
        pinned_to_split: None,
        total_lines: 0,
        raw_pty_log: Vec::new(),
        pty_log_flushed_bytes: 0,
        started_at_utc: chrono::Utc::now(),
        usage: None,
    }
}

#[test]
fn exit_code_zero_marks_completed() {
    let mut app = App::new();
    app.agents.push(test_agent(42, AgentStatus::Running));

    app.on_agent_exited(42, Some(0));

    let agent = app.agents.iter().find(|a| a.id == 42).unwrap();
    assert_eq!(agent.status, AgentStatus::Completed);
    assert_eq!(agent.exit_code, Some(0));
}

#[test]
fn exit_code_nonzero_marks_failed() {
    let mut app = App::new();
    app.agents.push(test_agent(43, AgentStatus::Running));

    app.on_agent_exited(43, Some(1));

    let agent = app.agents.iter().find(|a| a.id == 43).unwrap();
    assert_eq!(agent.status, AgentStatus::Failed);
}

#[test]
fn exit_does_not_overwrite_completed_status() {
    let mut app = App::new();
    app.agents.push(test_agent(44, AgentStatus::Completed));

    // Simulate: force_complete set Completed, then exit watcher fires
    // with non-zero (process was killed after being marked complete)
    app.on_agent_exited(44, Some(1));

    let agent = app.agents.iter().find(|a| a.id == 44).unwrap();
    assert_eq!(
        agent.status,
        AgentStatus::Completed,
        "exit watcher must not overwrite Completed status"
    );
}

#[test]
fn exit_does_not_overwrite_failed_status() {
    let mut app = App::new();
    app.agents.push(test_agent(45, AgentStatus::Failed));

    // Simulate: kill_selected_agent set Failed, then exit watcher fires
    app.on_agent_exited(45, Some(0));

    let agent = app.agents.iter().find(|a| a.id == 45).unwrap();
    assert_eq!(
        agent.status,
        AgentStatus::Failed,
        "exit watcher must not overwrite Failed status"
    );
}

#[test]
fn detect_phase_ordering() {
    assert_eq!(detect_phase("bd close obelisk-1"), Some(AgentPhase::Closing));
    assert_eq!(detect_phase("git merge --no-ff"), Some(AgentPhase::Merging));
    assert_eq!(detect_phase("cargo test"), Some(AgentPhase::Verifying));
    assert_eq!(detect_phase("bd update x --notes 'done'"), Some(AgentPhase::Implementing));
    assert_eq!(detect_phase("git worktree add ../wt"), Some(AgentPhase::Worktree));
    assert_eq!(detect_phase("bd update x --claim"), Some(AgentPhase::Claiming));
    assert_eq!(detect_phase("just some text"), None);
}

#[test]
fn detect_phase_highest_wins() {
    // Text containing both claim and close markers — close is checked first
    let text = "--claim and bd close";
    assert_eq!(detect_phase(text), Some(AgentPhase::Closing));
}

#[test]
fn force_complete_sets_completed() {
    let mut app = App::new();
    app.agents.push(test_agent(50, AgentStatus::Running));

    let result = app.force_complete_agent(50);
    assert!(result.is_some());

    let agent = app.agents.iter().find(|a| a.id == 50).unwrap();
    assert_eq!(agent.status, AgentStatus::Completed);
    assert_eq!(app.total_completed, 1);
    assert_eq!(app.total_failed, 0);
}

#[test]
fn force_complete_ignores_finished_agents() {
    let mut app = App::new();
    app.agents.push(test_agent(51, AgentStatus::Failed));

    let result = app.force_complete_agent(51);
    assert!(result.is_none(), "should not force-complete a finished agent");

    let agent = app.agents.iter().find(|a| a.id == 51).unwrap();
    assert_eq!(agent.status, AgentStatus::Failed);
}

#[test]
fn force_complete_then_exit_preserves_completed() {
    let mut app = App::new();
    app.agents.push(test_agent(52, AgentStatus::Running));

    // Mark complete manually
    app.force_complete_agent(52);
    // Then exit watcher fires with non-zero (SIGTERM)
    app.on_agent_exited(52, Some(143));

    let agent = app.agents.iter().find(|a| a.id == 52).unwrap();
    assert_eq!(
        agent.status,
        AgentStatus::Completed,
        "exit watcher must not overwrite force-completed status"
    );
}

// ── Phase detection ──────────────────────────────────────────

#[test]
fn detect_phase_claiming() {
    assert_eq!(detect_phase("bd update x --claim --json"), Some(AgentPhase::Claiming));
}

#[test]
fn detect_phase_worktree() {
    assert_eq!(detect_phase("git worktree add ../worktree-abc"), Some(AgentPhase::Worktree));
}

#[test]
fn detect_phase_implementing() {
    assert_eq!(detect_phase("bd update x --notes \"doing stuff\""), Some(AgentPhase::Implementing));
}

#[test]
fn detect_phase_verifying_cargo_test() {
    assert_eq!(detect_phase("cargo test --release"), Some(AgentPhase::Verifying));
}

#[test]
fn detect_phase_verifying_cargo_clippy() {
    assert_eq!(detect_phase("cargo clippy -- -D warnings"), Some(AgentPhase::Verifying));
}

#[test]
fn detect_phase_verifying_cargo_check() {
    assert_eq!(detect_phase("cargo check"), Some(AgentPhase::Verifying));
}

#[test]
fn detect_phase_merging() {
    assert_eq!(detect_phase("git merge abc --no-ff -m \"msg\""), Some(AgentPhase::Merging));
}

#[test]
fn detect_phase_closing() {
    assert_eq!(detect_phase("bd close abc --reason done"), Some(AgentPhase::Closing));
}

#[test]
fn detect_phase_none_for_unrelated_text() {
    assert_eq!(detect_phase("echo hello world"), None);
}

#[test]
fn detect_phase_highest_wins_when_multiple() {
    // "bd close" and "--claim" both present: closing > claiming
    assert_eq!(detect_phase("bd close --claim"), Some(AgentPhase::Closing));
}

// ── Issue-closed completion via polling (obelisk-ip4) ──────

#[test]
fn on_issue_closed_marks_running_agent_completed() {
    let mut app = App::new();
    let agent = test_agent(50, AgentStatus::Running);
    app.agents.push(agent);

    app.on_issue_closed(50);

    let agent = app.agents.iter().find(|a| a.id == 50).unwrap();
    assert_eq!(agent.phase, AgentPhase::Done);
    assert_eq!(agent.status, AgentStatus::Completed);
    assert_eq!(app.total_completed, 1);
}

#[test]
fn on_issue_closed_ignores_already_completed_agent() {
    let mut app = App::new();
    let mut agent = test_agent(51, AgentStatus::Completed);
    agent.phase = AgentPhase::Done;
    app.agents.push(agent);

    app.on_issue_closed(51);

    // Should not increment completion count again
    assert_eq!(app.total_completed, 0);
}

#[test]
fn on_issue_closed_ignores_unknown_agent() {
    let mut app = App::new();
    // No agents registered — should not panic
    app.on_issue_closed(999);
    assert_eq!(app.total_completed, 0);
}

#[test]
fn exit_after_issue_closed_does_not_double_count() {
    let mut app = App::new();
    let agent = test_agent(53, AgentStatus::Running);
    app.agents.push(agent);

    // First: issue closure via polling marks Completed
    app.on_issue_closed(53);
    assert_eq!(app.total_completed, 1);

    // Then: process exits naturally — should not increment again
    app.on_agent_exited(53, Some(0));
    assert_eq!(app.total_completed, 1, "must not double-count completion");

    let agent = app.agents.iter().find(|a| a.id == 53).unwrap();
    assert_eq!(agent.status, AgentStatus::Completed);
    assert_eq!(agent.exit_code, Some(0));
}

// ── Error pattern detection ──────────────────────────────────

#[test]
fn detect_errors_compilation() {
    let lines = vec!["error[E0308]: mismatched types", "  --> src/main.rs:10:5"];
    let errors = detect_error_patterns(&lines);
    assert!(errors.iter().any(|e| e.contains("Compilation")));
}

#[test]
fn detect_errors_test_failure() {
    let lines = vec!["test result: FAILED. 2 passed; 1 failed; 0 ignored"];
    let errors = detect_error_patterns(&lines);
    assert!(errors.iter().any(|e| e.contains("Test failure")));
}

#[test]
fn detect_errors_panic() {
    let lines = vec!["thread 'main' panicked at 'index out of bounds'"];
    let errors = detect_error_patterns(&lines);
    assert!(errors.iter().any(|e| e.contains("Panic")));
}

#[test]
fn detect_errors_permission_denied() {
    let lines = vec!["Error: Permission denied (os error 13)"];
    let errors = detect_error_patterns(&lines);
    assert!(errors.iter().any(|e| e.contains("Permission denied")));
}

#[test]
fn detect_errors_merge_conflict() {
    let lines = vec!["CONFLICT: merge conflict in src/main.rs"];
    let errors = detect_error_patterns(&lines);
    assert!(errors.iter().any(|e| e.contains("merge conflict")));
}

#[test]
fn detect_errors_empty_for_clean_output() {
    let lines = vec!["running 5 tests", "test result: ok. 5 passed; 0 ignored"];
    let errors = detect_error_patterns(&lines);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}

#[test]
fn format_elapsed_various() {
    assert_eq!(App::format_elapsed(0), "00:00");
    assert_eq!(App::format_elapsed(59), "00:59");
    assert_eq!(App::format_elapsed(60), "01:00");
    assert_eq!(App::format_elapsed(3661), "61:01");
}

// ── Agent lifecycle ──────────────────────────────────────────

#[test]
fn on_agent_output_transitions_starting_to_running() {
    let mut app = App::new();
    app.agents.push(test_agent(10, AgentStatus::Starting));

    app.on_agent_output(10, "first line".into());

    let agent = app.agents.iter().find(|a| a.id == 10).unwrap();
    assert_eq!(agent.status, AgentStatus::Running);
    assert_eq!(agent.output.len(), 1);
}

#[test]
fn on_agent_output_caps_at_10000_lines() {
    let mut app = App::new();
    app.agents.push(test_agent(11, AgentStatus::Running));

    for i in 0..10005 {
        app.on_agent_output(11, format!("line {}", i));
    }

    let agent = app.agents.iter().find(|a| a.id == 11).unwrap();
    assert_eq!(agent.output.len(), 10000);
    // Oldest lines should have been discarded
    assert!(agent.output.front().unwrap().contains("line 5"));
}

#[test]
fn active_agent_count_only_counts_starting_and_running() {
    let mut app = App::new();
    app.agents.push(test_agent(1, AgentStatus::Starting));
    app.agents.push(test_agent(2, AgentStatus::Running));
    app.agents.push(test_agent(3, AgentStatus::Completed));
    app.agents.push(test_agent(4, AgentStatus::Failed));

    assert_eq!(app.active_agent_count(), 2);
}

#[test]
fn on_agent_exited_increments_counters() {
    let mut app = App::new();
    app.agents.push(test_agent(50, AgentStatus::Running));
    app.agents.push(test_agent(51, AgentStatus::Running));

    app.on_agent_exited(50, Some(0));
    assert_eq!(app.total_completed, 1);
    assert_eq!(app.total_failed, 0);

    app.on_agent_exited(51, Some(1));
    assert_eq!(app.total_completed, 1);
    assert_eq!(app.total_failed, 1);
}

#[test]
fn on_agent_exited_detaches_interactive_mode() {
    let mut app = App::new();
    app.agents.push(test_agent(60, AgentStatus::Running));
    app.interactive_mode = true;
    app.selected_agent_id = Some(60);

    app.on_agent_exited(60, Some(0));

    assert!(!app.interactive_mode, "interactive mode should be detached on exit");
}

#[test]
fn dismiss_finished_agents() {
    let mut app = App::new();
    app.agents.push(test_agent(1, AgentStatus::Running));
    app.agents.push(test_agent(2, AgentStatus::Completed));
    app.agents.push(test_agent(3, AgentStatus::Failed));

    let dismissed = app.dismiss_all_finished();
    assert_eq!(dismissed, 2);
    assert_eq!(app.agents.len(), 1);
    assert_eq!(app.agents[0].id, 1);
}

#[test]
fn dismiss_all_finished_clears_selected_if_dismissed() {
    let mut app = App::new();
    app.agents.push(test_agent(1, AgentStatus::Completed));
    app.selected_agent_id = Some(1);

    app.dismiss_all_finished();

    assert!(app.selected_agent_id.is_none());
}

// ── Poll result handling ─────────────────────────────────────

#[test]
fn on_poll_result_filters_claimed_tasks() {
    let mut app = App::new();
    app.claimed_task_ids.insert("t-1".into());

    let tasks = vec![
        BeadTask { id: "t-1".into(), title: "Claimed".into(), status: "open".into(),
                   priority: None, issue_type: None, assignee: None, labels: None,
                   description: None, created_at: None },
        BeadTask { id: "t-2".into(), title: "New".into(), status: "open".into(),
                   priority: None, issue_type: None, assignee: None, labels: None,
                   description: None, created_at: None },
    ];

    app.on_poll_result(tasks);

    assert_eq!(app.ready_tasks.len(), 1);
    assert_eq!(app.ready_tasks[0].id, "t-2");
}

#[test]
fn on_poll_result_resets_failure_state() {
    let mut app = App::new();
    app.last_poll_ok = false;
    app.consecutive_poll_failures = 5;
    app.last_poll_error = Some("timeout".into());

    app.on_poll_result(vec![]);

    assert!(app.last_poll_ok);
    assert_eq!(app.consecutive_poll_failures, 0);
    assert!(app.last_poll_error.is_none());
}

#[test]
fn on_poll_result_filters_out_epics() {
    let mut app = App::new();

    let tasks = vec![
        BeadTask { id: "e-1".into(), title: "Epic".into(), status: "open".into(),
                   priority: Some(1), issue_type: Some("epic".into()), assignee: None,
                   labels: None, description: None, created_at: None },
        BeadTask { id: "t-1".into(), title: "Task".into(), status: "open".into(),
                   priority: Some(2), issue_type: Some("task".into()), assignee: None,
                   labels: None, description: None, created_at: None },
        BeadTask { id: "b-1".into(), title: "Bug".into(), status: "open".into(),
                   priority: Some(0), issue_type: Some("bug".into()), assignee: None,
                   labels: None, description: None, created_at: None },
    ];

    app.on_poll_result(tasks);

    assert_eq!(app.ready_tasks.len(), 2, "epic should be filtered out");
    assert!(app.ready_tasks.iter().all(|t| !t.is_epic()),
            "no epics should be in the ready queue");
    assert!(app.ready_tasks.iter().any(|t| t.id == "t-1"));
    assert!(app.ready_tasks.iter().any(|t| t.id == "b-1"));
}

#[test]
fn on_poll_result_filters_epics_with_none_issue_type_kept() {
    let mut app = App::new();

    let tasks = vec![
        BeadTask { id: "e-1".into(), title: "Epic".into(), status: "open".into(),
                   priority: Some(1), issue_type: Some("epic".into()), assignee: None,
                   labels: None, description: None, created_at: None },
        BeadTask { id: "t-1".into(), title: "Default type".into(), status: "open".into(),
                   priority: None, issue_type: None, assignee: None,
                   labels: None, description: None, created_at: None },
    ];

    app.on_poll_result(tasks);

    assert_eq!(app.ready_tasks.len(), 1);
    assert_eq!(app.ready_tasks[0].id, "t-1",
               "tasks with no issue_type (defaults to task) should be kept");
}

#[test]
fn get_spawn_info_blocks_epic() {
    let mut app = App::new();
    app.ready_tasks = vec![
        BeadTask { id: "e-1".into(), title: "Epic".into(), status: "open".into(),
                   priority: Some(1), issue_type: Some("epic".into()), assignee: None,
                   labels: None, description: None, created_at: None },
    ];
    app.task_list_state.select(Some(0));

    let result = app.get_spawn_info();
    assert!(result.is_none(), "should refuse to spawn an agent for an epic");
}

#[test]
fn on_poll_failed_increments_failure_count() {
    let mut app = App::new();

    app.on_poll_failed("connection refused".into());
    assert_eq!(app.consecutive_poll_failures, 1);
    assert!(!app.last_poll_ok);

    app.on_poll_failed("timeout".into());
    assert_eq!(app.consecutive_poll_failures, 2);
}

// ── Sorting ──────────────────────────────────────────────────

#[test]
fn sort_ready_tasks_by_priority() {
    let mut app = App::new();
    app.sort_mode = SortMode::Priority;
    app.ready_tasks = vec![
        BeadTask { id: "c".into(), title: "Low".into(), status: "open".into(),
                   priority: Some(3), issue_type: None, assignee: None, labels: None,
                   description: None, created_at: None },
        BeadTask { id: "a".into(), title: "High".into(), status: "open".into(),
                   priority: Some(0), issue_type: None, assignee: None, labels: None,
                   description: None, created_at: None },
        BeadTask { id: "b".into(), title: "Med".into(), status: "open".into(),
                   priority: Some(1), issue_type: None, assignee: None, labels: None,
                   description: None, created_at: None },
    ];

    app.sort_ready_tasks();

    assert_eq!(app.ready_tasks[0].id, "a"); // P0
    assert_eq!(app.ready_tasks[1].id, "b"); // P1
    assert_eq!(app.ready_tasks[2].id, "c"); // P3
}

#[test]
fn sort_ready_tasks_by_name() {
    let mut app = App::new();
    app.sort_mode = SortMode::Name;
    app.ready_tasks = vec![
        BeadTask { id: "1".into(), title: "Zebra".into(), status: "open".into(),
                   priority: None, issue_type: None, assignee: None, labels: None,
                   description: None, created_at: None },
        BeadTask { id: "2".into(), title: "Apple".into(), status: "open".into(),
                   priority: None, issue_type: None, assignee: None, labels: None,
                   description: None, created_at: None },
    ];

    app.sort_ready_tasks();

    assert_eq!(app.ready_tasks[0].title, "Apple");
    assert_eq!(app.ready_tasks[1].title, "Zebra");
}

#[test]
fn sort_ready_tasks_by_type() {
    let mut app = App::new();
    app.sort_mode = SortMode::Type;
    app.ready_tasks = vec![
        BeadTask { id: "1".into(), title: "T".into(), status: "open".into(),
                   priority: Some(1), issue_type: Some("task".into()), assignee: None,
                   labels: None, description: None, created_at: None },
        BeadTask { id: "2".into(), title: "B".into(), status: "open".into(),
                   priority: Some(1), issue_type: Some("bug".into()), assignee: None,
                   labels: None, description: None, created_at: None },
    ];

    app.sort_ready_tasks();

    assert_eq!(app.ready_tasks[0].issue_type.as_deref(), Some("bug"));
    assert_eq!(app.ready_tasks[1].issue_type.as_deref(), Some("task"));
}

// ── Filtering ────────────────────────────────────────────────

#[test]
fn filtered_tasks_with_type_filter() {
    let mut app = App::new();
    app.ready_tasks = vec![
        BeadTask { id: "1".into(), title: "T".into(), status: "open".into(),
                   priority: None, issue_type: Some("bug".into()), assignee: None,
                   labels: None, description: None, created_at: None },
        BeadTask { id: "2".into(), title: "T".into(), status: "open".into(),
                   priority: None, issue_type: Some("task".into()), assignee: None,
                   labels: None, description: None, created_at: None },
    ];
    app.type_filter.insert("bug".into());

    let filtered = app.filtered_tasks();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].issue_type.as_deref(), Some("bug"));
}

#[test]
fn filtered_tasks_with_priority_filter() {
    let mut app = App::new();
    app.ready_tasks = vec![
        BeadTask { id: "1".into(), title: "T".into(), status: "open".into(),
                   priority: Some(0), issue_type: None, assignee: None,
                   labels: None, description: None, created_at: None },
        BeadTask { id: "2".into(), title: "T".into(), status: "open".into(),
                   priority: Some(3), issue_type: None, assignee: None,
                   labels: None, description: None, created_at: None },
    ];
    app.priority_filter = Some(0..=1);

    let filtered = app.filtered_tasks();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].priority, Some(0));
}

#[test]
fn filtered_agents_respects_status_filter() {
    let mut app = App::new();
    app.agents.push(test_agent(1, AgentStatus::Running));
    app.agents.push(test_agent(2, AgentStatus::Completed));
    app.agents.push(test_agent(3, AgentStatus::Failed));

    app.agent_status_filter = AgentStatusFilter::Failed;
    let filtered = app.filtered_agents();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].1.id, 3);
}

// ── Navigation ───────────────────────────────────────────────

#[test]
fn toggle_focus_switches_between_panels() {
    let mut app = App::new();
    assert_eq!(app.focus, Focus::ReadyQueue);

    app.toggle_focus();
    assert_eq!(app.focus, Focus::BlockedQueue);

    app.toggle_focus();
    assert_eq!(app.focus, Focus::AgentList);

    app.toggle_focus();
    assert_eq!(app.focus, Focus::ReadyQueue);
}

#[test]
fn navigate_wraps_around_in_ready_queue() {
    let mut app = App::new();
    app.active_view = View::Dashboard;
    app.focus = Focus::ReadyQueue;
    app.ready_tasks = vec![
        BeadTask { id: "a".into(), title: "A".into(), status: "open".into(),
                   priority: None, issue_type: None, assignee: None, labels: None,
                   description: None, created_at: None },
        BeadTask { id: "b".into(), title: "B".into(), status: "open".into(),
                   priority: None, issue_type: None, assignee: None, labels: None,
                   description: None, created_at: None },
    ];
    app.task_list_state.select(Some(0));

    // Navigate up from 0 → wraps to last
    app.navigate_up();
    assert_eq!(app.task_list_state.selected(), Some(1));

    // Navigate down from last → wraps to 0
    app.navigate_down();
    assert_eq!(app.task_list_state.selected(), Some(0));
}

// ── On tick ──────────────────────────────────────────────────

#[test]
fn on_tick_increments_frame_count() {
    let mut app = App::new();
    let before = app.frame_count;
    app.on_tick();
    assert_eq!(app.frame_count, before + 1);
}

#[test]
fn on_tick_decrements_poll_countdown() {
    let mut app = App::new();
    app.poll_countdown = 5.0;
    app.on_tick();
    assert!(app.poll_countdown < 5.0);
}

#[test]
fn on_tick_clears_expired_alert() {
    let mut app = App::new();
    app.alert_message = Some(("Test alert".into(), 0)); // Already expired
    app.frame_count = 1;
    app.on_tick(); // frame_count becomes 2, which is > 0
    assert!(app.alert_message.is_none());
}

// ── Model selection ──────────────────────────────────────────

#[test]
fn selected_model_returns_valid_model() {
    let app = App::new();
    let model = app.selected_model();
    assert!(!model.is_empty());
}

#[test]
fn cycle_model_advances_index() {
    let mut app = App::new();
    let first = app.selected_model().to_string();
    app.cycle_model();
    let second = app.selected_model().to_string();
    assert_ne!(first, second, "model should change after cycling");
}

#[test]
fn cycle_model_wraps_around() {
    let mut app = App::new();
    let num_models = app.selected_runtime.models().len();
    let first = app.selected_model().to_string();
    for _ in 0..num_models {
        app.cycle_model();
    }
    assert_eq!(app.selected_model(), first, "should wrap back to first model");
}

#[test]
fn completion_rate_with_no_agents() {
    let app = App::new();
    assert_eq!(app.completion_rate(), 0.0);
}

#[test]
fn completion_rate_calculates_correctly() {
    let mut app = App::new();
    app.agents.push(test_agent(1, AgentStatus::Completed));
    app.agents.push(test_agent(2, AgentStatus::Failed));

    let rate = app.completion_rate();
    assert!((rate - 50.0).abs() < f64::EPSILON);
}

#[test]
fn completion_rate_unaffected_by_dismiss() {
    let mut app = App::new();
    // 4 agents: 2 completed, 1 failed, 1 running
    app.agents.push(test_agent(1, AgentStatus::Completed));
    app.agents.push(test_agent(2, AgentStatus::Completed));
    app.agents.push(test_agent(3, AgentStatus::Failed));
    app.agents.push(test_agent(4, AgentStatus::Running));
    assert!((app.completion_rate() - 50.0).abs() < f64::EPSILON);

    // Dismiss the 2 completed + 1 failed agents
    app.agents.retain(|a| matches!(a.status, AgentStatus::Starting | AgentStatus::Running));
    // Only running agent remains → 0% completed, not inflated
    assert!((app.completion_rate() - 0.0).abs() < f64::EPSILON);
}

// ── Worktree management ──────────────────────────────────────

#[test]
fn on_worktree_orphans_sets_alert() {
    let mut app = App::new();
    app.on_worktree_orphans(vec!["/tmp/worktree-abc".into(), "/tmp/worktree-def".into()]);

    assert!(app.alert_message.is_some());
    let (msg, _) = app.alert_message.as_ref().unwrap();
    assert!(msg.contains("2"), "alert should mention count");
}

#[test]
fn on_worktree_cleaned_marks_agent() {
    let mut app = App::new();
    let mut agent = test_agent(1, AgentStatus::Completed);
    agent.worktree_path = Some("/tmp/worktree-test".into());
    app.agents.push(agent);

    app.on_worktree_cleaned(vec!["/tmp/worktree-test".into()], vec![]);

    let agent = app.agents.iter().find(|a| a.id == 1).unwrap();
    assert!(agent.worktree_cleaned);
}

#[test]
fn selected_agent_worktree_returns_none_when_cleaned() {
    let mut app = App::new();
    let mut agent = test_agent(1, AgentStatus::Completed);
    agent.worktree_path = Some("/tmp/worktree-test".into());
    agent.worktree_cleaned = true;
    app.agents.push(agent);
    app.selected_agent_id = Some(1);

    assert!(app.selected_agent_worktree().is_none());
}

#[test]
fn selected_agent_worktree_returns_path_when_not_cleaned() {
    let mut app = App::new();
    let mut agent = test_agent(1, AgentStatus::Running);
    agent.worktree_path = Some("/tmp/worktree-test".into());
    app.agents.push(agent);
    app.selected_agent_id = Some(1);

    assert_eq!(app.selected_agent_worktree(), Some("/tmp/worktree-test".into()));
}

// ── Worktree scan processing ─────────────────────────────────

#[test]
fn on_worktree_scanned_classifies_active_worktrees() {
    let mut app = App::new();
    let mut agent = test_agent(1, AgentStatus::Running);
    agent.task = BeadTask {
        id: "abc".into(),
        title: "task".into(),
        status: "in_progress".into(),
        priority: None, issue_type: None, assignee: None, labels: None,
        description: None, created_at: None,
    };
    agent.worktree_path = Some("/tmp/worktree-abc".into());
    app.agents.push(agent);

    app.on_worktree_scanned(vec![("/tmp/worktree-abc".into(), "abc".into())]);

    assert_eq!(app.worktree_entries.len(), 1);
    assert_eq!(app.worktree_entries[0].status, WorktreeStatus::Active);
}

#[test]
fn on_worktree_scanned_classifies_orphaned_worktrees() {
    let mut app = App::new();
    // No agents registered

    app.on_worktree_scanned(vec![("/tmp/worktree-xyz".into(), "xyz".into())]);

    assert_eq!(app.worktree_entries.len(), 1);
    assert_eq!(app.worktree_entries[0].status, WorktreeStatus::Orphaned);
}

// ── Jump-to-issue ────────────────────────────────────────────

#[test]
fn jump_matches_searches_ready_queue_and_agents() {
    let mut app = App::new();
    app.ready_tasks = vec![
        BeadTask { id: "obelisk-abc".into(), title: "T".into(), status: "open".into(),
                   priority: None, issue_type: None, assignee: None, labels: None,
                   description: None, created_at: None },
    ];
    let mut agent = test_agent(1, AgentStatus::Running);
    agent.task.id = "obelisk-def".into();
    app.agents.push(agent);

    app.jump_query = "obelisk".into();
    let matches = app.jump_matches();
    assert_eq!(matches.len(), 2);
}

#[test]
fn jump_matches_is_case_insensitive() {
    let mut app = App::new();
    app.ready_tasks = vec![
        BeadTask { id: "ABC-123".into(), title: "T".into(), status: "open".into(),
                   priority: None, issue_type: None, assignee: None, labels: None,
                   description: None, created_at: None },
    ];

    app.jump_query = "abc".into();
    let matches = app.jump_matches();
    assert_eq!(matches.len(), 1);
}

#[test]
fn jump_execute_returns_false_for_empty_query() {
    let mut app = App::new();
    app.jump_query = "".into();
    assert!(!app.jump_execute());
}

// ── Split-pane ───────────────────────────────────────────────

#[test]
fn split_pane_count_scales_with_width() {
    let app = App::new();
    assert_eq!(app.split_pane_count(60), 1);
    assert_eq!(app.split_pane_count(100), 2);
    assert_eq!(app.split_pane_count(200), 4);
}

#[test]
fn auto_fill_fixed_panels_when_4_or_fewer_agents() {
    let mut app = App::new();
    // Add 3 running agents
    app.agents.push(test_agent(1, AgentStatus::Running));
    app.agents.push(test_agent(2, AgentStatus::Running));
    app.agents.push(test_agent(3, AgentStatus::Running));

    app.auto_fill_split_panes();
    let first_fill = app.split_pane_agents;

    // Call again — panels should NOT rotate
    app.auto_fill_split_panes();
    assert_eq!(app.split_pane_agents, first_fill);

    // Call many more times — still stable
    for _ in 0..20 {
        app.auto_fill_split_panes();
    }
    assert_eq!(app.split_pane_agents, first_fill);

    // All 3 agents should be visible, 4th slot empty
    assert!(app.split_pane_agents[..3].iter().all(|s| s.is_some()));
    assert_eq!(app.split_pane_agents[3], None);
}

#[test]
fn auto_fill_rotates_when_more_than_4_agents() {
    let mut app = App::new();
    // Add 6 running agents
    for i in 1..=6 {
        app.agents.push(test_agent(i, AgentStatus::Running));
    }

    app.auto_fill_split_panes();
    let first_fill = app.split_pane_agents;
    // All 4 slots should be occupied
    assert!(first_fill.iter().all(|s| s.is_some()));

    // Call again — panels should rotate (different agents shown)
    app.auto_fill_split_panes();
    assert_ne!(app.split_pane_agents, first_fill);
}

#[test]
fn auto_fill_respects_pinned_agents_during_rotation() {
    let mut app = App::new();
    for i in 1..=6 {
        app.agents.push(test_agent(i, AgentStatus::Running));
    }

    // Pin agent 1 to slot 0
    app.split_pane_agents[0] = Some(1);
    app.agents[0].pinned_to_split = Some(0);

    app.auto_fill_split_panes();
    assert_eq!(app.split_pane_agents[0], Some(1)); // pinned stays

    // Rotate multiple times — pinned agent stays in slot 0
    for _ in 0..10 {
        app.auto_fill_split_panes();
        assert_eq!(app.split_pane_agents[0], Some(1));
    }
}

#[test]
fn auto_fill_replaces_finished_agents_in_fixed_mode() {
    let mut app = App::new();
    app.agents.push(test_agent(1, AgentStatus::Running));
    app.agents.push(test_agent(2, AgentStatus::Running));
    app.agents.push(test_agent(3, AgentStatus::Running));

    app.auto_fill_split_panes();
    assert!(app.split_pane_agents.contains(&Some(1)));

    // Agent 1 finishes, agent 4 starts
    app.agents[0].status = AgentStatus::Completed;
    app.agents.push(test_agent(4, AgentStatus::Running));

    app.auto_fill_split_panes();
    // Agent 1 should be gone, agent 4 should appear
    assert!(!app.split_pane_agents.contains(&Some(1)));
    assert!(app.split_pane_agents.contains(&Some(4)));
}

// ── Log ──────────────────────────────────────────────────────

#[test]
fn log_entries_are_capped_at_500() {
    let mut app = App::new();
    for i in 0..510 {
        app.log(LogCategory::System, format!("msg {}", i));
    }
    assert!(app.event_log.len() <= 500);
}

#[test]
fn log_entry_has_timestamp() {
    let mut app = App::new();
    app.log(LogCategory::System, "test".into());
    assert!(!app.event_log[0].timestamp.is_empty());
}

// ── Config parsing ───────────────────────────────────────────

#[test]
fn config_parse_unknown_runtime_defaults_to_claude() {
    let toml_str = r#"
        [orchestrator]
        runtime = "unknown_runtime"
    "#;
    let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
    let runtime = match config.orchestrator.unwrap().runtime.unwrap().as_str() {
        "claude" => Runtime::ClaudeCode,
        "codex" => Runtime::Codex,
        "copilot" => Runtime::Copilot,
        _ => Runtime::ClaudeCode, // default fallback
    };
    assert_eq!(runtime, Runtime::ClaudeCode);
}

#[test]
fn config_parse_max_concurrent_is_clamped() {
    // Test the clamping logic from App::new
    let val: usize = 100;
    let clamped = val.clamp(1, 20);
    assert_eq!(clamped, 20);

    let val: usize = 0;
    let clamped = val.clamp(1, 20);
    assert_eq!(clamped, 1);
}

#[test]
fn config_round_trip() {
    let config = ObeliskConfig {
        orchestrator: Some(OrchestratorConfig {
            runtime: Some("claude".into()),
            max_concurrent: Some(5),
            auto_spawn: Some(true),
            poll_interval_secs: Some(60),
            velocity_window: Some(12),
        }),
        models: Some(ModelsConfig {
            claude: Some("claude-opus-4-6".into()),
            codex: Some("gpt-5.4".into()),
            copilot: Some("gpt-5".into()),
        }),
        theme: None,
        notifications: None,
    };

    let toml_str = toml::to_string_pretty(&config).unwrap();
    let restored: ObeliskConfig = toml::from_str(&toml_str).unwrap();

    let orch = restored.orchestrator.unwrap();
    assert_eq!(orch.runtime.as_deref(), Some("claude"));
    assert_eq!(orch.max_concurrent, Some(5));
    assert_eq!(orch.auto_spawn, Some(true));
    assert_eq!(orch.poll_interval_secs, Some(60));

    let models = restored.models.unwrap();
    assert_eq!(models.claude.as_deref(), Some("claude-opus-4-6"));
}

/// Config hot-reload tests are combined into a single test because they use
/// `set_current_dir` which is process-global and would race with parallel tests.
#[test]
fn config_hot_reload() {
    let dir = std::env::temp_dir().join(format!("obelisk-test-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let orig_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    // ── Part 1: detects orchestrator changes ──

    let initial = "[orchestrator]\nmax_concurrent = 5\nauto_spawn = false\npoll_interval_secs = 30\n";
    std::fs::write("obelisk.toml", initial).unwrap();

    let mut app = App::new();
    assert_eq!(app.max_concurrent, 5);
    assert!(!app.auto_spawn);
    assert_eq!(app.poll_interval_secs, 30);

    // No change yet
    app.frame_count = 100;
    assert!(!app.check_config_reload());

    // Write new config with different values (sleep to ensure mtime differs)
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let updated = "[orchestrator]\nmax_concurrent = 12\nauto_spawn = true\npoll_interval_secs = 10\n";
    std::fs::write("obelisk.toml", updated).unwrap();
    app.frame_count = 200;

    assert!(app.check_config_reload());
    assert_eq!(app.max_concurrent, 12);
    assert!(app.auto_spawn);
    assert_eq!(app.poll_interval_secs, 10);

    // Second check without changes should return false
    app.frame_count = 300;
    assert!(!app.check_config_reload());

    // ── Part 2: theme change ──

    let original_primary = app.theme.primary;
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write("obelisk.toml", "[theme]\npreset = \"nord\"\n").unwrap();
    app.frame_count = 400;
    assert!(app.check_config_reload());
    assert_ne!(app.theme.primary, original_primary);

    // ── Part 3: invalid TOML logs warning, preserves old values ──

    let mc_before = app.max_concurrent;
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write("obelisk.toml", "[invalid toml!!!").unwrap();
    app.frame_count = 500;

    assert!(!app.check_config_reload());
    assert_eq!(app.max_concurrent, mc_before);
    let has_warning = app.event_log.iter().any(|e| e.message.contains("Config reload failed"));
    assert!(has_warning);

    // Cleanup
    std::env::set_current_dir(&orig_dir).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

// ── Config validation ───────────────────────────────────────

#[test]
fn validate_warns_on_unknown_top_level_key() {
    let toml_str = r#"
        [orchestrator]
        runtime = "claude"
        [bogus_section]
        foo = 1
    "#;
    let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
    let warnings = validate_config(toml_str, &config);
    assert!(warnings.iter().any(|w| w.contains("bogus_section")));
}

#[test]
fn validate_warns_on_unknown_orchestrator_key() {
    let toml_str = r#"
        [orchestrator]
        runtime = "claude"
        turbo_mode = true
    "#;
    let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
    let warnings = validate_config(toml_str, &config);
    assert!(warnings.iter().any(|w| w.contains("orchestrator.turbo_mode")));
}

#[test]
fn validate_warns_on_unknown_runtime() {
    let toml_str = r#"
        [orchestrator]
        runtime = "gemini"
    "#;
    let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
    let warnings = validate_config(toml_str, &config);
    assert!(warnings.iter().any(|w| w.contains("Unknown runtime 'gemini'")));
}

#[test]
fn validate_warns_on_max_concurrent_zero() {
    let toml_str = r#"
        [orchestrator]
        max_concurrent = 0
    "#;
    let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
    let warnings = validate_config(toml_str, &config);
    assert!(warnings.iter().any(|w| w.contains("max_concurrent=0")));
}

#[test]
fn validate_warns_on_max_concurrent_too_high() {
    let toml_str = r#"
        [orchestrator]
        max_concurrent = 99
    "#;
    let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
    let warnings = validate_config(toml_str, &config);
    assert!(warnings.iter().any(|w| w.contains("max_concurrent=99")));
}

#[test]
fn validate_warns_on_poll_interval_zero() {
    let toml_str = r#"
        [orchestrator]
        poll_interval_secs = 0
    "#;
    let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
    let warnings = validate_config(toml_str, &config);
    assert!(warnings.iter().any(|w| w.contains("poll_interval_secs")));
}

#[test]
fn validate_warns_on_velocity_window_too_small() {
    let toml_str = r#"
        [orchestrator]
        velocity_window = 1
    "#;
    let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
    let warnings = validate_config(toml_str, &config);
    assert!(warnings.iter().any(|w| w.contains("velocity_window=1")));
}

#[test]
fn validate_warns_on_unknown_model() {
    let toml_str = r#"
        [models]
        claude = "nonexistent-model-v9"
    "#;
    let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
    let warnings = validate_config(toml_str, &config);
    assert!(warnings.iter().any(|w| w.contains("nonexistent-model-v9")));
}

#[test]
fn validate_warns_on_unknown_theme_preset() {
    let toml_str = r#"
        [theme]
        preset = "nonexistent"
    "#;
    let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
    let warnings = validate_config(toml_str, &config);
    assert!(warnings.iter().any(|w| w.contains("nonexistent")));
}

#[test]
fn validate_accepts_theme_preset_aliases() {
    for alias in &["frost", "ember", "ash", "deep", "dusk", "amber", "twilight", "carbon", "bloom", "moss"] {
        let toml_str = format!("[theme]\npreset = \"{}\"", alias);
        let config: ObeliskConfig = toml::from_str(&toml_str).unwrap();
        let warnings = validate_config(&toml_str, &config);
        let preset_warnings: Vec<_> = warnings.iter()
            .filter(|w| w.contains("Unknown theme preset"))
            .collect();
        assert!(preset_warnings.is_empty(),
            "Alias '{}' should be accepted but got: {:?}", alias, preset_warnings);
    }
}

#[test]
fn validate_no_warnings_for_valid_config() {
    let toml_str = r#"
        [orchestrator]
        runtime = "claude"
        max_concurrent = 10
        auto_spawn = false
        poll_interval_secs = 30
        velocity_window = 24

        [models]
        claude = "claude-opus-4-6"
        codex = "gpt-5.4"
        copilot = "gpt-5"

        [theme]
        preset = "nord"
    "#;
    let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
    let warnings = validate_config(toml_str, &config);
    // Filter out PATH-related warnings since those depend on the test environment
    let non_path: Vec<_> = warnings.iter()
        .filter(|w| !w.contains("not found on PATH"))
        .collect();
    assert!(non_path.is_empty(), "Unexpected warnings: {:?}", non_path);
}

#[test]
fn validate_warns_on_unknown_models_key() {
    let toml_str = r#"
        [models]
        gemini = "gemini-pro"
    "#;
    let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
    let warnings = validate_config(toml_str, &config);
    assert!(warnings.iter().any(|w| w.contains("models.gemini")));
}

#[test]
fn validate_warns_on_unknown_theme_key() {
    let toml_str = r#"
        [theme]
        background_image = "cats.png"
    "#;
    let config: ObeliskConfig = toml::from_str(toml_str).unwrap();
    let warnings = validate_config(toml_str, &config);
    assert!(warnings.iter().any(|w| w.contains("theme.background_image")));
}

// ── Aggregate stats ──────────────────────────────────────────

#[test]
fn aggregate_stats_with_empty_history() {
    let mut app = App::new();
    app.history_sessions.clear();

    let (sessions, completed, failed, avg_dur, total_cost) = app.aggregate_stats();
    assert_eq!(sessions, 0);
    assert_eq!(completed, 0);
    assert_eq!(failed, 0);
    assert_eq!(avg_dur, 0.0);
    assert_eq!(total_cost, 0.0);
}

#[test]
fn aggregate_stats_sums_across_sessions() {
    let mut app = App::new();
    app.history_sessions = vec![
        SessionRecord {
            session_id: "s1".into(),
            started_at: "2026-03-12T10:00:00Z".into(),
            ended_at: "2026-03-12T11:00:00Z".into(),
            total_completed: 3,
            total_failed: 1,
            total_cost_usd: 1.50,
            agents: vec![SessionAgent {
                task_id: "t".into(), runtime: "CLAUDE".into(), model: "m".into(),
                elapsed_secs: 100, status: "Completed".into(),
                input_tokens: 1000, output_tokens: 500, estimated_cost_usd: 1.50,
            }],
        },
        SessionRecord {
            session_id: "s2".into(),
            started_at: "2026-03-12T12:00:00Z".into(),
            ended_at: "2026-03-12T13:00:00Z".into(),
            total_completed: 2,
            total_failed: 0,
            total_cost_usd: 2.00,
            agents: vec![SessionAgent {
                task_id: "t".into(), runtime: "CLAUDE".into(), model: "m".into(),
                elapsed_secs: 200, status: "Completed".into(),
                input_tokens: 2000, output_tokens: 1000, estimated_cost_usd: 2.00,
            }],
        },
    ];

    let (sessions, completed, failed, avg_dur, total_cost) = app.aggregate_stats();
    assert_eq!(sessions, 2);
    assert_eq!(completed, 5);
    assert_eq!(failed, 1);
    assert!((avg_dur - 150.0).abs() < f64::EPSILON); // (100+200)/2
    assert!((total_cost - 3.50).abs() < f64::EPSILON);
}

// ── Dep graph ────────────────────────────────────────────────

#[test]
fn rebuild_dep_graph_flat_list() {
    let mut app = App::new();
    app.dep_graph_nodes = vec![
        DepNode { id: "a".into(), title: "A".into(), status: "open".into(),
                  priority: Some(1), issue_type: None, depth: 0, parent_id: None, truncated: false },
        DepNode { id: "b".into(), title: "B".into(), status: "open".into(),
                  priority: Some(2), issue_type: None, depth: 0, parent_id: None, truncated: false },
    ];

    app.rebuild_dep_graph_rows();

    assert_eq!(app.dep_graph_rows.len(), 2);
}

#[test]
fn rebuild_dep_graph_with_parent_child() {
    let mut app = App::new();
    app.dep_graph_nodes = vec![
        DepNode { id: "root".into(), title: "Root".into(), status: "open".into(),
                  priority: Some(1), issue_type: None, depth: 0, parent_id: None, truncated: false },
        DepNode { id: "child".into(), title: "Child".into(), status: "open".into(),
                  priority: Some(2), issue_type: None, depth: 1, parent_id: Some("root".into()), truncated: false },
    ];

    app.rebuild_dep_graph_rows();

    assert_eq!(app.dep_graph_rows.len(), 2);
    assert!(app.dep_graph_rows[0].has_children);
}

#[test]
fn dep_graph_collapse_hides_children() {
    let mut app = App::new();
    app.dep_graph_nodes = vec![
        DepNode { id: "root".into(), title: "Root".into(), status: "open".into(),
                  priority: Some(1), issue_type: None, depth: 0, parent_id: None, truncated: false },
        DepNode { id: "child".into(), title: "Child".into(), status: "open".into(),
                  priority: Some(2), issue_type: None, depth: 1, parent_id: Some("root".into()), truncated: false },
    ];

    // Collapse root
    app.dep_graph_collapsed.insert("root".into());
    app.rebuild_dep_graph_rows();

    assert_eq!(app.dep_graph_rows.len(), 1, "child should be hidden when root is collapsed");
    assert!(app.dep_graph_rows[0].collapsed);
}

// ── Dependency-aware auto-spawn ─────────────────────────────

#[test]
fn auto_spawn_skips_tasks_with_unclosed_dep_graph_parent() {
    let mut app = App::new();
    app.auto_spawn = true;
    app.max_concurrent = 5;

    // Task B depends on task A (parent_id points to A). A is still open.
    app.ready_tasks = vec![
        BeadTask { id: "B".into(), title: "Child".into(), status: "open".into(),
                   priority: Some(1), issue_type: None, assignee: None,
                   labels: None, description: None, created_at: None },
    ];
    app.dep_graph_nodes = vec![
        DepNode { id: "A".into(), title: "Parent".into(), status: "open".into(),
                  priority: Some(1), issue_type: None, depth: 0, parent_id: None, truncated: false },
        DepNode { id: "B".into(), title: "Child".into(), status: "open".into(),
                  priority: Some(1), issue_type: None, depth: 1, parent_id: Some("A".into()), truncated: false },
    ];

    // B has an unclosed parent A — should be skipped
    let result = app.get_auto_spawn_info();
    assert!(result.is_none(), "should not spawn B when parent A is not closed");
}

#[test]
fn auto_spawn_allows_task_when_dep_graph_parent_is_closed() {
    let mut app = App::new();
    app.auto_spawn = true;
    app.max_concurrent = 5;

    app.ready_tasks = vec![
        BeadTask { id: "B".into(), title: "Child".into(), status: "open".into(),
                   priority: Some(1), issue_type: None, assignee: None,
                   labels: None, description: None, created_at: None },
    ];
    app.dep_graph_nodes = vec![
        DepNode { id: "A".into(), title: "Parent".into(), status: "closed".into(),
                  priority: Some(1), issue_type: None, depth: 0, parent_id: None, truncated: false },
        DepNode { id: "B".into(), title: "Child".into(), status: "open".into(),
                  priority: Some(1), issue_type: None, depth: 1, parent_id: Some("A".into()), truncated: false },
    ];

    // A is closed — B should be eligible
    let result = app.get_auto_spawn_info();
    assert!(result.is_some(), "should spawn B when parent A is closed");
}

#[test]
fn auto_spawn_skips_blocked_tasks() {
    let mut app = App::new();
    app.auto_spawn = true;
    app.max_concurrent = 5;

    app.ready_tasks = vec![
        BeadTask { id: "X".into(), title: "Blocked".into(), status: "open".into(),
                   priority: Some(1), issue_type: None, assignee: None,
                   labels: None, description: None, created_at: None },
    ];
    app.blocked_tasks = vec![
        BlockedTask {
            task: BeadTask { id: "X".into(), title: "Blocked".into(), status: "blocked".into(),
                             priority: Some(1), issue_type: None, assignee: None,
                             labels: None, description: None, created_at: None },
            remaining_deps: 1,
        },
    ];

    let result = app.get_auto_spawn_info();
    assert!(result.is_none(), "should not spawn a blocked task");
}

#[test]
fn auto_spawn_picks_eligible_task_over_blocked_one() {
    let mut app = App::new();
    app.auto_spawn = true;
    app.max_concurrent = 5;

    // Two tasks: X is blocked (P1), Y is free (P2)
    app.ready_tasks = vec![
        BeadTask { id: "X".into(), title: "Blocked".into(), status: "open".into(),
                   priority: Some(1), issue_type: None, assignee: None,
                   labels: None, description: None, created_at: None },
        BeadTask { id: "Y".into(), title: "Free".into(), status: "open".into(),
                   priority: Some(2), issue_type: None, assignee: None,
                   labels: None, description: None, created_at: None },
    ];
    app.dep_graph_nodes = vec![
        DepNode { id: "dep".into(), title: "Dep".into(), status: "open".into(),
                  priority: Some(1), issue_type: None, depth: 0, parent_id: None, truncated: false },
        DepNode { id: "X".into(), title: "Blocked".into(), status: "open".into(),
                  priority: Some(1), issue_type: None, depth: 1, parent_id: Some("dep".into()), truncated: false },
    ];

    // X has unclosed dep, Y is free — should pick Y even though X has higher priority
    let result = app.get_auto_spawn_info();
    assert!(result.is_some(), "should spawn the eligible task Y");
    let spawned = result.unwrap();
    assert_eq!(spawned.task.id, "Y", "should have spawned Y, not blocked X");
}

// ── Search helpers ───────────────────────────────────────────

#[test]
fn compute_search_matches_finds_text() {
    let mut parser = vt100::Parser::new(5, 20, 10);
    parser.process(b"hello world\r\nhello again\r\n");
    let matches = compute_search_matches(parser.screen(), "hello");
    assert_eq!(matches.len(), 2);
}

#[test]
fn compute_search_matches_is_case_insensitive() {
    let mut parser = vt100::Parser::new(5, 20, 10);
    parser.process(b"Hello HELLO\r\n");
    let matches = compute_search_matches(parser.screen(), "hello");
    assert_eq!(matches.len(), 2);
}

#[test]
fn compute_search_matches_empty_query_returns_empty() {
    let parser = vt100::Parser::new(5, 20, 10);
    let matches = compute_search_matches(parser.screen(), "");
    assert!(matches.is_empty());
}

#[test]
fn search_next_wraps_around() {
    let mut app = App::new();
    app.search_matches = vec![(0, 0), (1, 0), (2, 0)];
    app.search_current_idx = 2;

    app.search_next();
    assert_eq!(app.search_current_idx, 0);
}

#[test]
fn search_prev_wraps_around() {
    let mut app = App::new();
    app.search_matches = vec![(0, 0), (1, 0), (2, 0)];
    app.search_current_idx = 0;

    app.search_prev();
    assert_eq!(app.search_current_idx, 2);
}

// ── Failure context extraction ───────────────────────────────

#[test]
fn extract_failure_context_includes_exit_code() {
    let mut agent = test_agent(1, AgentStatus::Failed);
    agent.exit_code = Some(1);
    agent.output = VecDeque::from(vec!["line 1".to_string(), "line 2".to_string()]);

    let ctx = extract_failure_context(&agent, 10);
    assert!(ctx.contains("Exit code: 1"));
}

#[test]
fn extract_failure_context_limits_output_lines() {
    let mut agent = test_agent(1, AgentStatus::Failed);
    agent.exit_code = Some(1);
    for i in 0..100 {
        agent.output.push_back(format!("line {}", i));
    }

    let ctx = extract_failure_context(&agent, 5);
    assert!(ctx.contains("Last 5 lines"));
    assert!(ctx.contains("line 99")); // should include the last line
    assert!(!ctx.contains("line 0"));  // should NOT include the first line
}

// ── Active task IDs ──────────────────────────────────────────

#[test]
fn active_task_ids_only_returns_active_agents() {
    let mut app = App::new();
    let mut a1 = test_agent(1, AgentStatus::Running);
    a1.task.id = "running-task".into();
    let mut a2 = test_agent(2, AgentStatus::Completed);
    a2.task.id = "done-task".into();
    app.agents = vec![a1, a2];

    let ids = app.active_task_ids();
    assert!(ids.contains("running-task"));
    assert!(!ids.contains("done-task"));
}

// ── Diff result handling ─────────────────────────────────────

#[test]
fn on_diff_result_accepted_when_viewing_same_agent() {
    let mut app = App::new();
    app.show_diff_panel = true;
    app.selected_agent_id = Some(1);

    let diff = DiffData {
        lines: vec!["+ new line".into()],
        files_changed: 1,
        insertions: 1,
        deletions: 0,
        changed_files: vec!["src/main.rs".into()],
    };

    app.on_diff_result(1, diff);
    assert!(app.diff_data.is_some());
}

#[test]
fn on_diff_result_rejected_when_viewing_different_agent() {
    let mut app = App::new();
    app.show_diff_panel = true;
    app.selected_agent_id = Some(1);

    let diff = DiffData {
        lines: vec![], files_changed: 0, insertions: 0, deletions: 0, changed_files: vec![],
    };

    app.on_diff_result(2, diff); // Different agent
    assert!(app.diff_data.is_none());
}

#[test]
fn toggle_diff_panel_resets_state() {
    let mut app = App::new();
    app.show_diff_panel = false;
    app.diff_scroll = 5;

    app.toggle_diff_panel();
    assert!(app.show_diff_panel);
    assert_eq!(app.diff_scroll, 0);

    app.toggle_diff_panel();
    assert!(!app.show_diff_panel);
    assert!(app.diff_data.is_none());
}

// ── Sync PTY sizes ───────────────────────────────────────────

#[test]
fn sync_pty_sizes_ignores_too_small() {
    let mut app = App::new();
    app.last_pty_size = (24, 120);

    app.sync_pty_sizes(1, 5); // Too small
    assert_eq!(app.last_pty_size, (24, 120), "should not change for tiny sizes");
}

#[test]
fn sync_pty_sizes_skips_if_unchanged() {
    let mut app = App::new();
    app.last_pty_size = (24, 120);

    app.sync_pty_sizes(24, 120); // Same
    // No-op: just verify it doesn't panic
}

// ── Recent completions ───────────────────────────────────────

#[test]
fn recent_completions_capped_at_10() {
    let mut app = App::new();
    for i in 0..15 {
        app.recent_completions.push_back(CompletionRecord {
            task_id: format!("t-{}", i),
            title: "T".into(),
            runtime: "CLAUDE".into(),
            model: "m".into(),
            elapsed_secs: 60,
            success: true,
        });
        if app.recent_completions.len() > 10 {
            app.recent_completions.pop_front();
        }
    }
    assert_eq!(app.recent_completions.len(), 10);
}

// ── Merge queue ────────────────────────────────────────────

fn test_agent_with_task(id: usize, status: AgentStatus, task_id: &str) -> AgentInstance {
    AgentInstance {
        id,
        unit_number: id,
        task: BeadTask {
            id: task_id.into(),
            title: format!("task {}", task_id),
            status: "in_progress".into(),
            priority: None,
            issue_type: None,
            assignee: None,
            labels: None,
            description: None,
            created_at: None,
        },
        runtime: Runtime::ClaudeCode,
        model: "test-model".into(),
        status,
        phase: AgentPhase::Verifying,
        output: VecDeque::new(),
        started_at: std::time::Instant::now(),
        elapsed_secs: 0,
        exit_code: None,
        pid: None,
        retry_count: 0,
        worktree_path: None,
        worktree_cleaned: false,
        template_name: String::new(),
        pinned_to_split: None,
        total_lines: 0,
        raw_pty_log: Vec::new(),
        pty_log_flushed_bytes: 0,
        started_at_utc: chrono::Utc::now(),
        usage: None,
    }
}

#[test]
fn merge_queue_enqueues_on_merging_phase() {
    let mut app = App::new();
    app.agents.push(test_agent_with_task(10, AgentStatus::Running, "obelisk-abc"));

    // Simulate PTY data with --no-ff (triggers Merging phase)
    app.on_agent_pty_data(10, b"git merge abc --no-ff -m \"msg\"");

    assert_eq!(app.merge_queue.len(), 1);
    assert_eq!(app.merge_queue[0].agent_id, 10);
    assert_eq!(app.merge_queue[0].task_id, "obelisk-abc");
    // Should log the merge entry
    assert!(app.event_log.iter().any(|e| e.message.contains("MERGE-QUEUE") && e.message.contains("obelisk-abc")));
}

#[test]
fn merge_queue_does_not_duplicate_enqueue() {
    let mut app = App::new();
    app.agents.push(test_agent_with_task(10, AgentStatus::Running, "obelisk-abc"));

    // First trigger — should enqueue
    app.on_agent_pty_data(10, b"git merge abc --no-ff -m \"msg\"");
    assert_eq!(app.merge_queue.len(), 1);

    // Second trigger with same phase — should not duplicate
    app.on_agent_pty_data(10, b"some other --no-ff text");
    assert_eq!(app.merge_queue.len(), 1);
}

#[test]
fn merge_queue_dequeues_on_closing_phase() {
    let mut app = App::new();
    let mut agent = test_agent_with_task(10, AgentStatus::Running, "obelisk-abc");
    agent.phase = AgentPhase::Verifying;
    app.agents.push(agent);

    // Enter merging phase
    app.on_agent_pty_data(10, b"git merge abc --no-ff -m \"msg\"");
    assert_eq!(app.merge_queue.len(), 1);

    // Advance to closing phase — should dequeue
    app.on_agent_pty_data(10, b"bd close obelisk-abc --reason done");
    assert_eq!(app.merge_queue.len(), 0);
    assert!(app.event_log.iter().any(|e| e.message.contains("merge complete")));
}

#[test]
fn merge_queue_dequeues_on_agent_exit() {
    let mut app = App::new();
    let mut agent = test_agent_with_task(10, AgentStatus::Running, "obelisk-abc");
    agent.phase = AgentPhase::Verifying;
    app.agents.push(agent);

    // Enter merging phase
    app.on_agent_pty_data(10, b"git merge abc --no-ff -m \"msg\"");
    assert_eq!(app.merge_queue.len(), 1);

    // Agent exits (e.g. crash) — should dequeue
    app.on_agent_exited(10, Some(1));
    assert_eq!(app.merge_queue.len(), 0);
    assert!(app.event_log.iter().any(|e| e.message.contains("exited while merging")));
}

#[test]
fn merge_queue_logs_conflict_detection() {
    let mut app = App::new();
    let mut agent = test_agent_with_task(10, AgentStatus::Running, "obelisk-abc");
    agent.phase = AgentPhase::Verifying;
    app.agents.push(agent);

    // Enter merging phase
    app.on_agent_pty_data(10, b"git merge abc --no-ff -m \"msg\"");

    // Simulate conflict output during merge
    app.on_agent_pty_data(10, b"CONFLICT (content): Merge conflict in src/main.rs");
    assert!(app.event_log.iter().any(|e| e.message.contains("CONFLICT") && e.message.contains("obelisk-abc")));
}

#[test]
fn merge_queue_multiple_agents_tracks_position() {
    let mut app = App::new();
    let mut agent1 = test_agent_with_task(10, AgentStatus::Running, "obelisk-abc");
    agent1.phase = AgentPhase::Verifying;
    let mut agent2 = test_agent_with_task(11, AgentStatus::Running, "obelisk-def");
    agent2.phase = AgentPhase::Verifying;
    app.agents.push(agent1);
    app.agents.push(agent2);

    // Agent 1 enters merge phase
    app.on_agent_pty_data(10, b"git merge abc --no-ff -m \"msg\"");
    assert_eq!(app.merge_queue.len(), 1);
    // Should log "proceeding" (queue was empty)
    assert!(app.event_log.iter().any(|e| e.message.contains("proceeding")));

    // Agent 2 enters merge phase while agent 1 is still merging
    app.on_agent_pty_data(11, b"git merge def --no-ff -m \"msg\"");
    assert_eq!(app.merge_queue.len(), 2);
    // Should log "1 agent(s) ahead"
    assert!(app.event_log.iter().any(|e| e.message.contains("1 agent(s) ahead")));

    // Agent 1 finishes merge (advances to closing)
    app.on_agent_pty_data(10, b"bd close obelisk-abc --reason done");
    assert_eq!(app.merge_queue.len(), 1);
    assert_eq!(app.merge_queue[0].agent_id, 11);
}
