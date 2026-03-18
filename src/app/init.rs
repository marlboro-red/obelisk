use super::*;

impl App {
    pub fn new() -> Self {
        let config_exists = std::path::Path::new(CONFIG_FILE).exists();
        let (config, config_warnings) = if config_exists {
            match std::fs::read_to_string(CONFIG_FILE) {
                Ok(raw) => match toml::from_str::<ObeliskConfig>(&raw) {
                    Ok(cfg) => {
                        let warnings = validate_config(&raw, &cfg);
                        (cfg, warnings)
                    }
                    Err(e) => (
                        ObeliskConfig::default(),
                        vec![format!("Failed to parse {}: {}", CONFIG_FILE, e)],
                    ),
                },
                Err(e) => (
                    ObeliskConfig::default(),
                    vec![format!("Failed to read {}: {}", CONFIG_FILE, e)],
                ),
            }
        } else {
            (ObeliskConfig::default(), vec![])
        };

        let mut selected_runtime = Runtime::ClaudeCode;
        let mut max_concurrent = 10usize;
        let mut auto_spawn = false;
        let mut poll_interval_secs = 30u64;
        let mut velocity_window_size = 24usize;
        let mut model_indices: HashMap<Runtime, usize> = HashMap::from([
            (Runtime::ClaudeCode, 0),
            (Runtime::Codex, 0),
            (Runtime::Copilot, 0),
        ]);

        if let Some(orch) = &config.orchestrator {
            if let Some(r) = &orch.runtime {
                selected_runtime = match r.as_str() {
                    "claude" => Runtime::ClaudeCode,
                    "codex" => Runtime::Codex,
                    "copilot" => Runtime::Copilot,
                    _ => Runtime::ClaudeCode,
                };
            }
            if let Some(mc) = orch.max_concurrent {
                max_concurrent = mc.clamp(1, 20);
            }
            if let Some(asp) = orch.auto_spawn {
                auto_spawn = asp;
            }
            if let Some(pi) = orch.poll_interval_secs {
                poll_interval_secs = pi.max(1);
            }
            if let Some(vw) = orch.velocity_window {
                velocity_window_size = vw.max(2); // minimum 2 data points
            }
        }

        if let Some(models) = &config.models {
            let pairs: &[(Runtime, &Option<String>)] = &[
                (Runtime::ClaudeCode, &models.claude),
                (Runtime::Codex, &models.codex),
                (Runtime::Copilot, &models.copilot),
            ];
            for (runtime, model_opt) in pairs {
                if let Some(model_str) = model_opt {
                    let idx = runtime
                        .models()
                        .iter()
                        .position(|m| *m == model_str.as_str())
                        .unwrap_or(0);
                    model_indices.insert(*runtime, idx);
                }
            }
        }

        let session_id = generate_session_id();
        let history_sessions = load_history_sessions();
        let last_session_summary = history_sessions.last().map(|last| format!(
            "Last session {}: {} completed, {} failed ({} agents)",
            &last.session_id[..8.min(last.session_id.len())],
            last.total_completed,
            last.total_failed,
            last.agents.len(),
        ));

        let theme_config = config.theme.clone().unwrap_or_default();
        let theme = Theme::from_config(&theme_config);
        let webhook_config = config
            .notifications
            .as_ref()
            .and_then(|n| n.webhook.clone())
            .unwrap_or_default();

        let poll_countdown = poll_interval_secs as f64;
        let mut app = Self {
            ready_tasks: Vec::new(),
            agents: Vec::new(),
            event_log: VecDeque::with_capacity(500),
            log_category_filter: None,
            active_view: View::Dashboard,
            focus: Focus::ReadyQueue,
            task_list_state: ListState::default(),
            agent_list_state: ListState::default(),
            log_scroll: 0,
            selected_runtime,
            auto_spawn,
            max_concurrent,
            poll_interval_secs,
            poll_countdown,
            should_quit: false,
            next_unit: 0,
            claimed_task_ids: HashSet::new(),
            total_completed: 0,
            total_failed: 0,
            max_retries: 3,
            retry_context_lines: 80,
            selected_agent_id: None,
            agent_output_scroll: None,
            template_dir: templates::default_template_dir(),
            frame_count: 0,
            throughput_history: VecDeque::from(vec![0; 60]),
            lines_this_tick: 0,
            alert_message: None,
            model_indices,
            pty_states: HashMap::new(),
            interactive_mode: false,
            last_pty_size: (24, 120),
            show_help: false,
            last_poll_ok: true,
            last_poll_error: None,
            consecutive_poll_failures: 0,
            sort_mode: SortMode::Priority,
            type_filter: HashSet::new(),
            priority_filter: None,
            type_filter_cursor: 0,
            session_id,
            session_started_at: chrono::Local::now(),
            history_sessions,
            history_scroll: 0,
            agent_status_filter: AgentStatusFilter::All,
            search_active: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_current_idx: 0,
            show_diff_panel: false,
            diff_data: None,
            diff_scroll: 0,
            diff_last_poll_frame: 0,
            notifications_enabled: true,
            webhook_config,
            split_pane_agents: [None; 4],
            split_pane_focus: 0,
            split_pane_scroll: [None; 4],
            split_pane_rotation_offset: 0,
            jump_active: false,
            jump_query: String::new(),
            velocity_window_size,
            confirm_kill_agent_id: None,
            confirm_complete_agent_id: None,
            confirm_quit: false,
            event_log_seen_count: 0,
            worktree_entries: Vec::new(),
            worktree_list_state: ListState::default(),
            worktree_sort_mode: WorktreeSortMode::Age,
            worktree_last_scan_frame: 0,
            recent_completions: VecDeque::with_capacity(10),
            theme,
            theme_config,
            blocked_tasks: Vec::new(),
            blocked_list_state: ListState::default(),
            layout_areas: LayoutAreas::default(),
            dep_graph_nodes: Vec::new(),
            dep_graph_rows: Vec::new(),
            dep_graph_list_state: ListState::default(),
            dep_graph_collapsed: HashSet::new(),
            dep_graph_last_poll_frame: 0,
            config_mtime: std::fs::metadata(CONFIG_FILE)
                .and_then(|m| m.modified())
                .ok(),
            config_check_frame: 0,
            repo_name: detect_repo_name(),
            issue_creation_active: false,
            issue_creation_form: IssueCreationForm::new(),
            merge_queue: VecDeque::new(),
        };

        // Seed recent completions from history sessions (most recent agents last)
        for session in app.history_sessions.iter().rev().take(3) {
            for agent in session.agents.iter().rev() {
                if app.recent_completions.len() >= 10 {
                    break;
                }
                app.recent_completions.push_front(CompletionRecord {
                    task_id: agent.task_id.clone(),
                    title: agent.task_id.clone(), // history doesn't store title
                    runtime: agent.runtime.clone(),
                    model: agent.model.clone(),
                    elapsed_secs: agent.elapsed_secs,
                    success: agent.status == "Completed",
                                    });
            }
        }
        app.log(LogCategory::System, "Orchestrator initialized".into());
        if config_exists {
            app.log(LogCategory::System, format!("Config loaded from {}", CONFIG_FILE));
        } else {
            app.log(LogCategory::System, "No config file found, using defaults".into());
        }
        for warning in &config_warnings {
            app.log(LogCategory::Alert, format!("Config: {}", warning));
        }
        if !config_warnings.is_empty() {
            app.alert_message = Some((
                format!(
                    "{} CONFIG WARNING(S) — check event log (Tab 3)",
                    config_warnings.len()
                ),
                100, // ~10 seconds at 10fps
            ));
        }
        app.log(LogCategory::System, "System online".into());
        if let Some(summary) = last_session_summary {
            app.log(LogCategory::System, summary);
        }
        app
    }

    pub fn save_config(&mut self) {
        let runtime_str = match self.selected_runtime {
            Runtime::ClaudeCode => "claude",
            Runtime::Codex => "codex",
            Runtime::Copilot => "copilot",
        };
        let config = ObeliskConfig {
            orchestrator: Some(OrchestratorConfig {
                runtime: Some(runtime_str.to_string()),
                max_concurrent: Some(self.max_concurrent),
                auto_spawn: Some(self.auto_spawn),
                poll_interval_secs: Some(self.poll_interval_secs),
                velocity_window: Some(self.velocity_window_size),
            }),
            models: Some(ModelsConfig {
                claude: Some(self.selected_model_for(Runtime::ClaudeCode).to_string()),
                codex: Some(self.selected_model_for(Runtime::Codex).to_string()),
                copilot: Some(self.selected_model_for(Runtime::Copilot).to_string()),
            }),
            theme: Some(self.theme_config.clone()),
            notifications: if self.webhook_config != WebhookConfig::default() {
                Some(NotificationsConfig {
                    webhook: Some(self.webhook_config.clone()),
                })
            } else {
                None
            },
        };
        if let Ok(toml_str) = toml::to_string_pretty(&config) {
            if std::fs::write(CONFIG_FILE, toml_str).is_ok() {
                self.log(LogCategory::System, format!("Config saved to {}", CONFIG_FILE));
                // Update stored mtime so the watcher doesn't treat our own save as an
                // external change.
                self.config_mtime = std::fs::metadata(CONFIG_FILE)
                    .and_then(|m| m.modified())
                    .ok();
            }
        }
    }

    /// Check if `obelisk.toml` has been modified since we last loaded it, and
    /// if so, re-read and apply the new values.  Called periodically from the
    /// tick handler (~every 2 seconds).  Returns `true` if config was reloaded
    /// (callers may need to propagate poll_interval changes).
    pub fn check_config_reload(&mut self) -> bool {
        // Only check every ~2s (20 ticks at 100ms)
        if self.frame_count.saturating_sub(self.config_check_frame) < 20 {
            return false;
        }
        self.config_check_frame = self.frame_count;

        let current_mtime = match std::fs::metadata(CONFIG_FILE).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => return false, // file doesn't exist or unreadable
        };

        if self.config_mtime == Some(current_mtime) {
            return false; // unchanged
        }

        // File was modified — reload
        let raw = match std::fs::read_to_string(CONFIG_FILE) {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, event = "config_reload_failed", "config reload failed: could not read file");
                self.log(
                    LogCategory::System,
                    format!("[WARN] Config reload failed (read): {}", e),
                );
                return false;
            }
        };
        let cfg: ObeliskConfig = match toml::from_str(&raw) {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, event = "config_parse_failed", "config reload failed: TOML parse error");
                self.log(
                    LogCategory::System,
                    format!("[WARN] Config reload failed (parse): {}", e),
                );
                return false;
            }
        };

        // Validate — log warnings but still apply
        let warnings = validate_config(&raw, &cfg);
        for w in &warnings {
            self.log(LogCategory::System, format!("[WARN] {}", w));
        }

        let mut changes: Vec<String> = Vec::new();

        // ── Orchestrator settings ──
        if let Some(orch) = &cfg.orchestrator {
            if let Some(r) = &orch.runtime {
                let new_rt = match r.as_str() {
                    "claude" => Runtime::ClaudeCode,
                    "codex" => Runtime::Codex,
                    "copilot" => Runtime::Copilot,
                    _ => self.selected_runtime,
                };
                if new_rt != self.selected_runtime {
                    self.selected_runtime = new_rt;
                    changes.push(format!("runtime → {}", new_rt.name()));
                }
            }
            if let Some(mc) = orch.max_concurrent {
                let mc = mc.clamp(1, 20);
                if mc != self.max_concurrent {
                    self.max_concurrent = mc;
                    changes.push(format!("max_concurrent → {}", mc));
                }
            }
            if let Some(asp) = orch.auto_spawn {
                if asp != self.auto_spawn {
                    self.auto_spawn = asp;
                    changes.push(format!(
                        "auto_spawn → {}",
                        if asp { "on" } else { "off" }
                    ));
                }
            }
            if let Some(pi) = orch.poll_interval_secs {
                let pi = pi.max(1);
                if pi != self.poll_interval_secs {
                    self.poll_interval_secs = pi;
                    changes.push(format!("poll_interval → {}s", pi));
                }
            }
            if let Some(vw) = orch.velocity_window {
                let vw = vw.max(2);
                if vw != self.velocity_window_size {
                    self.velocity_window_size = vw;
                    changes.push(format!("velocity_window → {}", vw));
                }
            }
        }

        // ── Model settings ──
        if let Some(models) = &cfg.models {
            let pairs: &[(Runtime, &Option<String>)] = &[
                (Runtime::ClaudeCode, &models.claude),
                (Runtime::Codex, &models.codex),
                (Runtime::Copilot, &models.copilot),
            ];
            for (runtime, model_opt) in pairs {
                if let Some(model_str) = model_opt {
                    let idx = runtime
                        .models()
                        .iter()
                        .position(|m| *m == model_str.as_str())
                        .unwrap_or(0);
                    let prev = self.model_indices.get(runtime).copied().unwrap_or(0);
                    if idx != prev {
                        self.model_indices.insert(*runtime, idx);
                        changes.push(format!(
                            "{} model → {}",
                            runtime.name(),
                            model_str
                        ));
                    }
                }
            }
        }

        // ── Theme settings ──
        let new_theme_config = cfg.theme.unwrap_or_default();
        if new_theme_config != self.theme_config {
            self.theme = Theme::from_config(&new_theme_config);
            self.theme_config = new_theme_config;
            changes.push("theme updated".into());
        }

        // ── Webhook settings ──
        let new_webhook = cfg
            .notifications
            .as_ref()
            .and_then(|n| n.webhook.clone())
            .unwrap_or_default();
        if new_webhook != self.webhook_config {
            self.webhook_config = new_webhook;
            changes.push("webhook config updated".into());
        }

        self.config_mtime = Some(current_mtime);

        if changes.is_empty() {
            debug!(event = "config_reload_noop", "config file changed but no effective differences");
            self.log(
                LogCategory::System,
                "Config file changed but no effective differences".into(),
            );
        } else {
            info!(
                changes = %changes.join(", "),
                change_count = changes.len(),
                event = "config_reloaded",
                "configuration hot-reloaded"
            );
            self.log(
                LogCategory::System,
                format!("Config reloaded: {}", changes.join(", ")),
            );
        }
        true
    }
}
