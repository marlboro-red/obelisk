use crate::types::AgentUsage;
use std::path::{Path, PathBuf};

/// Encode a filesystem path into Claude Code's project directory naming convention.
/// e.g. `/Users/cjpark/Desktop/Projects/obelisk` → `-Users-cjpark-Desktop-Projects-obelisk`
fn encode_project_path(cwd: &Path) -> String {
    let path_str = cwd.to_string_lossy();
    path_str.replace('/', "-")
}

/// Resolve the user's home directory across platforms.
/// Checks HOME (Unix / Git Bash on Windows), USERPROFILE (Windows default),
/// then HOMEDRIVE+HOMEPATH (Windows fallback).
fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .or_else(|| {
            let drive = std::env::var("HOMEDRIVE").ok()?;
            let path = std::env::var("HOMEPATH").ok()?;
            Some(format!("{}{}", drive, path))
        })
        .map(PathBuf::from)
}

/// Return the Claude Code project directory for the current working directory.
fn claude_project_dir(cwd: &Path) -> Option<PathBuf> {
    let home = home_dir()?;
    Some(
        home.join(".claude")
            .join("projects")
            .join(encode_project_path(cwd)),
    )
}

/// Per-model pricing in USD per token. Returns (input_per_token, output_per_token,
/// cache_write_per_token, cache_read_per_token).
fn model_pricing(model: &str) -> (f64, f64, f64, f64) {
    // Prices per million tokens — divide by 1_000_000 for per-token
    let (inp, out, cache_w, cache_r) = if model.contains("opus") {
        (15.0, 75.0, 18.75, 1.50)
    } else if model.contains("haiku") {
        (0.80, 4.0, 1.0, 0.08)
    } else {
        // Default to Sonnet pricing
        (3.0, 15.0, 3.75, 0.30)
    };
    (
        inp / 1_000_000.0,
        out / 1_000_000.0,
        cache_w / 1_000_000.0,
        cache_r / 1_000_000.0,
    )
}

/// Calculate cost in USD from token counts and model name.
pub fn calculate_cost(
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
) -> f64 {
    let (inp_price, out_price, cache_w_price, cache_r_price) = model_pricing(model);
    input_tokens as f64 * inp_price
        + output_tokens as f64 * out_price
        + cache_creation_tokens as f64 * cache_w_price
        + cache_read_tokens as f64 * cache_r_price
}

/// Read Claude Code usage data for a session JSONL file.
/// Sums all assistant message usage fields and returns an AgentUsage.
fn read_session_file(path: &Path) -> Option<(AgentUsage, String)> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;
    let mut cache_creation_tokens: u64 = 0;
    let mut cache_read_tokens: u64 = 0;
    let mut model = String::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if parsed.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }
        let msg = match parsed.get("message") {
            Some(m) if m.is_object() => m,
            _ => continue,
        };
        if let Some(m) = msg.get("model").and_then(|v| v.as_str()) {
            if model.is_empty() {
                model = m.to_string();
            }
        }
        if let Some(usage) = msg.get("usage") {
            input_tokens += usage
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            output_tokens += usage
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            cache_creation_tokens += usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            cache_read_tokens += usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
        }
    }

    if input_tokens == 0 && output_tokens == 0 && cache_creation_tokens == 0 && cache_read_tokens == 0 {
        return None;
    }

    let cost = calculate_cost(&model, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens);

    Some((
        AgentUsage {
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            cost_usd: cost,
        },
        model,
    ))
}

/// Find the first message timestamp in a Claude Code session JSONL file.
/// Returns the parsed DateTime if found.
fn first_message_timestamp(path: &Path) -> Option<chrono::DateTime<chrono::Utc>> {
    use std::io::{BufRead, BufReader};
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file);

    for line in reader.lines().take(10) {
        let line = line.ok()?;
        if line.trim().is_empty() {
            continue;
        }
        let parsed: serde_json::Value = serde_json::from_str(&line).ok()?;
        if let Some(ts) = parsed.get("timestamp").and_then(|t| t.as_str()) {
            return chrono::DateTime::parse_from_rfc3339(ts)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc));
        }
    }
    None
}

/// Read Claude Code usage for an agent that ran between `started_after` and `ended_before`.
/// Scans the project's Claude Code session directory for JSONL files whose first message
/// timestamp falls within the agent's active window. Returns the best-matching session's usage.
pub fn read_agent_usage(
    started_after: chrono::DateTime<chrono::Utc>,
    ended_before: chrono::DateTime<chrono::Utc>,
) -> Option<AgentUsage> {
    let cwd = std::env::current_dir().ok()?;
    let project_dir = claude_project_dir(&cwd)?;

    if !project_dir.is_dir() {
        return None;
    }

    // Allow some slack for timing differences
    let window_start = started_after - chrono::Duration::seconds(30);
    let window_end = ended_before + chrono::Duration::seconds(30);

    let mut best: Option<(AgentUsage, chrono::Duration)> = None;

    let entries = std::fs::read_dir(&project_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }

        // Check if the first message timestamp is within our window
        let first_ts = match first_message_timestamp(&path) {
            Some(ts) => ts,
            None => continue,
        };

        if first_ts < window_start || first_ts > window_end {
            continue;
        }

        // This session started within our agent's active window — read its usage
        if let Some((usage, _model)) = read_session_file(&path) {
            let distance = if first_ts > started_after {
                first_ts - started_after
            } else {
                started_after - first_ts
            };
            // Pick the session whose start time is closest to the agent's start time
            if best.as_ref().is_none_or(|(_, d)| distance < *d) {
                best = Some((usage, distance));
            }
        }
    }

    best.map(|(usage, _)| usage)
}

/// Format a USD cost value for display.
pub fn format_cost(cost: f64) -> String {
    if cost < 0.01 {
        format!("${:.4}", cost)
    } else if cost < 1.0 {
        format!("${:.3}", cost)
    } else {
        format!("${:.2}", cost)
    }
}

/// Format a token count for display (e.g. 1234 → "1.2K", 1234567 → "1.2M").
pub fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        format!("{}", count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_project_path_basic() {
        let path = Path::new("/Users/cjpark/Desktop/Projects/obelisk");
        assert_eq!(
            encode_project_path(path),
            "-Users-cjpark-Desktop-Projects-obelisk"
        );
    }

    #[test]
    fn calculate_cost_opus() {
        // 1000 input, 500 output, 2000 cache write, 10000 cache read
        let cost = calculate_cost("claude-opus-4-6", 1000, 500, 2000, 10000);
        // input: 1000 * 15/1M = 0.015
        // output: 500 * 75/1M = 0.0375
        // cache_w: 2000 * 18.75/1M = 0.0375
        // cache_r: 10000 * 1.5/1M = 0.015
        let expected = 0.015 + 0.0375 + 0.0375 + 0.015;
        assert!((cost - expected).abs() < 0.0001, "cost={} expected={}", cost, expected);
    }

    #[test]
    fn calculate_cost_sonnet() {
        let cost = calculate_cost("claude-sonnet-4-6", 1000, 500, 0, 0);
        // input: 1000 * 3/1M = 0.003
        // output: 500 * 15/1M = 0.0075
        let expected = 0.003 + 0.0075;
        assert!((cost - expected).abs() < 0.0001);
    }

    #[test]
    fn format_cost_display() {
        assert_eq!(format_cost(0.0), "$0.0000");
        assert_eq!(format_cost(0.005), "$0.0050");
        assert_eq!(format_cost(0.1234), "$0.123");
        assert_eq!(format_cost(1.50), "$1.50");
        assert_eq!(format_cost(12.345), "$12.35");
    }

    #[test]
    fn format_tokens_display() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1000), "1.0K");
        assert_eq!(format_tokens(15_432), "15.4K");
        assert_eq!(format_tokens(1_500_000), "1.5M");
    }

    #[test]
    fn home_dir_never_returns_tilde() {
        // home_dir() should return None rather than a literal "~" path
        // when no home env vars are set.
        let result = home_dir();
        if let Some(ref p) = result {
            assert_ne!(
                p.as_os_str(),
                "~",
                "home_dir() must not fall back to literal '~'"
            );
        }
    }

    #[test]
    fn claude_project_dir_returns_none_without_home() {
        // Temporarily clear all home-related env vars to verify we get None
        // instead of a bogus "~/.claude/..." path.
        let saved_home = std::env::var("HOME").ok();
        let saved_userprofile = std::env::var("USERPROFILE").ok();
        let saved_homedrive = std::env::var("HOMEDRIVE").ok();
        let saved_homepath = std::env::var("HOMEPATH").ok();

        std::env::remove_var("HOME");
        std::env::remove_var("USERPROFILE");
        std::env::remove_var("HOMEDRIVE");
        std::env::remove_var("HOMEPATH");

        let result = claude_project_dir(Path::new("/tmp/test"));
        assert!(result.is_none(), "Expected None when no home env vars are set");

        // Restore env vars
        if let Some(v) = saved_home { std::env::set_var("HOME", v); }
        if let Some(v) = saved_userprofile { std::env::set_var("USERPROFILE", v); }
        if let Some(v) = saved_homedrive { std::env::set_var("HOMEDRIVE", v); }
        if let Some(v) = saved_homepath { std::env::set_var("HOMEPATH", v); }
    }

    #[test]
    fn home_dir_uses_userprofile_fallback() {
        let saved_home = std::env::var("HOME").ok();
        let saved_userprofile = std::env::var("USERPROFILE").ok();

        std::env::remove_var("HOME");
        std::env::set_var("USERPROFILE", r"C:\Users\testuser");

        let result = home_dir();
        assert_eq!(result, Some(PathBuf::from(r"C:\Users\testuser")));

        // Restore
        if let Some(v) = saved_home { std::env::set_var("HOME", v); } else { std::env::remove_var("HOME"); }
        if let Some(v) = saved_userprofile { std::env::set_var("USERPROFILE", v); } else { std::env::remove_var("USERPROFILE"); }
    }
}
