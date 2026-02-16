use glob::glob;
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::time::{Duration, SystemTime};

use crate::tmux;

const STALE_THRESHOLD: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Working,
    Waiting,
    Idle,
    Error,
}

#[derive(Debug, Clone)]
pub struct ClaudeInstance {
    pub pane_id: String,
    pub project: String,
    pub status: Status,
    pub position: String, // e.g. "1:1.2"
    pub last_prompt: String,
}

/// Read all pane-* state files from /tmp/claude-tmux/
/// Returns None when tmux is temporarily unavailable (caller should keep stale data).
pub fn read_state_files() -> Option<Vec<ClaudeInstance>> {
    let pane_map = match build_pane_position_map() {
        Some(map) => map,
        None => return None, // tmux unavailable, signal caller to keep stale data
    };
    let mut pane_map = pane_map;

    let mut instances = Vec::new();
    let pattern = "/tmp/claude-tmux/pane-%*";
    let paths = match glob(pattern) {
        Ok(paths) => paths,
        Err(_) => return Some(Vec::new()),
    };

    for entry in paths.flatten() {
        let Some(filename) = entry.file_name() else {
            continue;
        };
        let filename = filename.to_string_lossy();
        let Some(pane_id) = filename.strip_prefix("pane-").filter(|s| !s.is_empty()) else {
            continue;
        };

        if !tmux::is_valid_pane_id(pane_id) {
            continue;
        }

        let pane_id = pane_id.to_string();

        let content = match fs::read_to_string(&entry) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let content = content.trim();
        if content.is_empty() {
            continue;
        }

        let (status, project) = parse_pane_content(content);
        // Skip panes that no longer exist in tmux
        let Some((command, position)) = pane_map.remove(&pane_id) else {
            continue;
        };

        // Correct stale status:
        // Working/Waiting -> Idle when:
        // 1. Shell is foreground -> Claude Code has exited
        // 2. State file mtime is too old -> PreToolUse heartbeat stopped
        let status = if (status == Status::Working || status == Status::Waiting)
            && (is_shell(&command) || is_stale_mtime(&entry))
        {
            Status::Idle
        } else {
            status
        };

        let last_prompt = read_last_prompt(&pane_id);

        instances.push(ClaudeInstance {
            pane_id,
            project,
            status,
            position,
            last_prompt,
        });
    }

    // Sort by position only for stable ordering
    instances.sort_by(|a, b| a.position.cmp(&b.position));

    Some(instances)
}

fn parse_pane_content(content: &str) -> (Status, String) {
    // Unicode symbols (new)
    if let Some(project) = content.strip_prefix("● ") {
        (Status::Working, project.to_string())
    } else if let Some(project) = content.strip_prefix("◐ ") {
        (Status::Waiting, project.to_string())
    } else if let Some(project) = content.strip_prefix("○ ") {
        (Status::Idle, project.to_string())
    } else if let Some(project) = content.strip_prefix("✕ ") {
        (Status::Error, project.to_string())
    // Nerd Font icons (backward compat)
    } else if let Some(project) = content.strip_prefix("󰑮 ") {
        (Status::Working, project.to_string())
    } else if let Some(project) = content.strip_prefix("󰭻 ") {
        (Status::Idle, project.to_string())
    } else {
        // Fallback: treat as idle, entire content is project name
        (Status::Idle, content.to_string())
    }
}

/// Run `tmux list-panes -a` to map pane IDs to (current_command, position).
/// Returns None if tmux command fails (server unavailable).
fn build_pane_position_map() -> Option<HashMap<String, (String, String)>> {
    let output = Command::new("tmux")
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{pane_id} #{pane_current_command} #{session_name}:#{window_index}.#{pane_index}",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let mut map = HashMap::new();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() == 3 {
            map.insert(
                parts[0].to_string(),
                (parts[1].to_string(), parts[2].to_string()),
            );
        }
    }
    Some(map)
}

/// Read the last user prompt from /tmp/claude-tmux/prompt-<pane_id>.
/// Rejects symlinks to prevent symlink attacks on /tmp.
fn read_last_prompt(pane_id: &str) -> String {
    let path = format!("/tmp/claude-tmux/prompt-{}", pane_id);
    // Reject symlinks to prevent reading arbitrary files
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => return String::new(),
        Err(_) => return String::new(),
        Ok(_) => {}
    }
    fs::read_to_string(&path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn is_shell(command: &str) -> bool {
    matches!(command, "fish" | "bash" | "zsh")
}

/// Check if the state file's mtime is older than STALE_THRESHOLD.
/// PreToolUse hook refreshes mtime during active work, so an old mtime
/// means Claude Code has stopped working (even if Stop hook didn't fire).
/// Returns false on errors (better to show "working" than to incorrectly
/// flip to "idle" due to a transient filesystem error).
fn is_stale_mtime(path: &std::path::Path) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|mtime| SystemTime::now().duration_since(mtime).ok())
        .map(|age| age > STALE_THRESHOLD)
        .unwrap_or(false)
}
