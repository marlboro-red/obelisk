use std::path::{Path, PathBuf};

/// Default template directory relative to the project root.
const DEFAULT_TEMPLATE_DIR: &str = ".obelisk/templates";

/// Built-in default templates per issue type.
/// Used as fallback when no file exists on disk.
fn builtin_template(issue_type: &str) -> &'static str {
    match issue_type {
        "bug" => include_str!("templates/bug.md"),
        "feature" => include_str!("templates/feature.md"),
        "task" => include_str!("templates/task.md"),
        "chore" => include_str!("templates/chore.md"),
        "epic" => include_str!("templates/epic.md"),
        _ => include_str!("templates/task.md"),
    }
}

/// Result of template resolution — includes the content and which template was used.
#[derive(Debug, Clone)]
pub struct ResolvedTemplate {
    /// The template content with variables already interpolated.
    pub content: String,
    /// Human-readable label for diagnostics (e.g. "bug.md (built-in)" or "bug.md").
    pub name: String,
}

/// Resolve the template for the given issue type.
///
/// Resolution order:
/// 1. Look for `<type>.md` in `template_dir` (hot-reloadable — reads from disk each call)
/// 2. Fall back to the built-in default for that type
/// 3. Fall back to the built-in "task" template if type is unknown
pub fn resolve(template_dir: &Path, issue_type: &str) -> ResolvedTemplate {
    let normalized_type = normalize_type(issue_type);
    let filename = format!("{}.md", normalized_type);
    let file_path = template_dir.join(&filename);

    if file_path.is_file() {
        match std::fs::read_to_string(&file_path) {
            Ok(content) => {
                return ResolvedTemplate {
                    content,
                    name: filename,
                };
            }
            Err(_) => {
                // Fall through to built-in on read error
            }
        }
    }

    ResolvedTemplate {
        content: builtin_template(&normalized_type).to_string(),
        name: format!("{} (built-in)", filename),
    }
}

/// Interpolate variables in a template string.
///
/// Supported variables: `{id}`, `{title}`, `{priority}`, `{description}`
pub fn interpolate(
    template: &str,
    id: &str,
    title: &str,
    priority: Option<i32>,
    description: Option<&str>,
) -> String {
    template
        .replace("{id}", id)
        .replace("{title}", title)
        .replace(
            "{priority}",
            &priority.map(|p| p.to_string()).unwrap_or_else(|| "?".into()),
        )
        .replace("{description}", description.unwrap_or(""))
}

/// Normalize issue type string to match template filenames.
fn normalize_type(issue_type: &str) -> String {
    issue_type.to_lowercase().trim().to_string()
}

/// Return the default template directory path.
pub fn default_template_dir() -> PathBuf {
    PathBuf::from(DEFAULT_TEMPLATE_DIR)
}
