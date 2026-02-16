use glob::glob;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
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
    let mut pane_map = build_pane_position_map()?;

    let mut instances = Vec::new();
    let pattern = "/tmp/claude-tmux/pane-%*";
    let paths = match glob(pattern) {
        Ok(paths) => paths,
        Err(_) => return Some(Vec::new()),
    };

    for entry in paths.flatten() {
        // Reject symlinks to prevent /tmp symlink attacks
        if is_symlink(&entry) {
            continue;
        }

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
        let Some((command, position, title)) = pane_map.remove(&pane_id) else {
            // Re-check symlink before destructive operation (defense-in-depth)
            if !is_symlink(&entry) {
                let _ = fs::remove_file(&entry);
            }
            continue;
        };

        // Correct stale status:
        // Working/Waiting -> Idle when:
        // 1. Shell is foreground -> Claude Code has exited
        // 2. State file mtime is too old AND no active spinner in pane title
        //    (spinner = Claude Code is still thinking, just not using tools)
        let status = if (status == Status::Working || status == Status::Waiting)
            && (is_shell(&command)
                || (is_stale_mtime(&entry) && !is_active_spinner(&title)))
        {
            // Rewrite state file so next read reflects corrected status
            if !is_symlink(&entry) {
                let _ = fs::write(&entry, format!("○ {}", project));
            }
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
    if let Some(project) = content.strip_prefix("▶ ") {
        (Status::Working, project.to_string())
    } else if let Some(project) = content.strip_prefix("● ") {
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

/// Run `tmux list-panes -a` to map pane IDs to (current_command, position, title).
/// Returns None if tmux command fails (server unavailable).
fn build_pane_position_map() -> Option<HashMap<String, (String, String, String)>> {
    let output = Command::new("tmux")
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{pane_id} #{pane_current_command} #{session_name}:#{window_index}.#{pane_index} #{pane_title}",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(parse_pane_list(&String::from_utf8_lossy(&output.stdout)))
}

/// Parse the output of `tmux list-panes -a -F "#{pane_id} #{pane_current_command} #{position} #{pane_title}"`.
fn parse_pane_list(stdout: &str) -> HashMap<String, (String, String, String)> {
    let mut map = HashMap::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(4, ' ').collect();
        if parts.len() >= 3 {
            let title = if parts.len() == 4 { parts[3] } else { "" };
            map.insert(
                parts[0].to_string(),
                (parts[1].to_string(), parts[2].to_string(), title.to_string()),
            );
        }
    }
    map
}

/// Read the last user prompt from /tmp/claude-tmux/prompt-<pane_id>.
/// Rejects symlinks to prevent symlink attacks on /tmp.
fn read_last_prompt(pane_id: &str) -> String {
    let path = format!("/tmp/claude-tmux/prompt-{}", pane_id);
    if is_symlink(path.as_ref()) {
        return String::new();
    }
    fs::read_to_string(&path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Check if the pane title contains a braille spinner character,
/// indicating Claude Code is actively processing (thinking).
fn is_active_spinner(title: &str) -> bool {
    title
        .chars()
        .any(|c| matches!(c, '⠋' | '⠙' | '⠹' | '⠸' | '⠼' | '⠴' | '⠦' | '⠧' | '⠇' | '⠏'))
}

fn is_shell(command: &str) -> bool {
    matches!(
        command,
        "fish" | "bash" | "zsh" | "sh" | "dash" | "-bash" | "-zsh" | "-fish" | "-sh" | "-dash"
    )
}

/// Check if a path is a symlink (or unreadable).
fn is_symlink(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(meta) => meta.file_type().is_symlink(),
        Err(_) => true, // Treat unreadable as unsafe
    }
}

/// Check if the state file's mtime is older than STALE_THRESHOLD.
/// PreToolUse hook refreshes mtime during active work, so an old mtime
/// means Claude Code has stopped working (even if Stop hook didn't fire).
/// Returns false on errors (better to show "working" than to incorrectly
/// flip to "idle" due to a transient filesystem error).
fn is_stale_mtime(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|mtime| SystemTime::now().duration_since(mtime).ok())
        .map(|age| age > STALE_THRESHOLD)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_pane_content ---

    #[test]
    fn parse_working() {
        let (status, project) = parse_pane_content("▶ my-project");
        assert_eq!(status, Status::Working);
        assert_eq!(project, "my-project");
    }

    #[test]
    fn parse_waiting() {
        let (status, project) = parse_pane_content("● my-project");
        assert_eq!(status, Status::Waiting);
        assert_eq!(project, "my-project");
    }

    #[test]
    fn parse_idle() {
        let (status, project) = parse_pane_content("○ my-project");
        assert_eq!(status, Status::Idle);
        assert_eq!(project, "my-project");
    }

    #[test]
    fn parse_error() {
        let (status, project) = parse_pane_content("✕ my-project");
        assert_eq!(status, Status::Error);
        assert_eq!(project, "my-project");
    }

    #[test]
    fn parse_nerd_font_working() {
        let (status, project) = parse_pane_content("󰑮 my-project");
        assert_eq!(status, Status::Working);
        assert_eq!(project, "my-project");
    }

    #[test]
    fn parse_nerd_font_idle() {
        let (status, project) = parse_pane_content("󰭻 my-project");
        assert_eq!(status, Status::Idle);
        assert_eq!(project, "my-project");
    }

    #[test]
    fn parse_fallback_no_prefix() {
        let (status, project) = parse_pane_content("bare-project");
        assert_eq!(status, Status::Idle);
        assert_eq!(project, "bare-project");
    }

    #[test]
    fn parse_empty_project_name() {
        let (status, project) = parse_pane_content("▶ ");
        assert_eq!(status, Status::Working);
        assert_eq!(project, "");
    }

    // --- is_shell ---

    #[test]
    fn shell_known_shells() {
        for shell in ["fish", "bash", "zsh", "sh", "dash"] {
            assert!(is_shell(shell), "{} should be recognized as shell", shell);
        }
    }

    #[test]
    fn shell_login_shells() {
        for shell in ["-bash", "-zsh", "-fish", "-sh", "-dash"] {
            assert!(
                is_shell(shell),
                "{} should be recognized as login shell",
                shell
            );
        }
    }

    #[test]
    fn shell_non_shells() {
        for cmd in ["node", "python", "claude", "2.1.42", "Fish", "BASH", ""] {
            assert!(
                !is_shell(cmd),
                "{:?} should not be recognized as shell",
                cmd
            );
        }
    }

    // --- parse_pane_list ---

    #[test]
    fn parse_pane_list_basic() {
        let input = "%0 node 0:0.0 My Title\n%1 fish 1:0.0 Other\n";
        let map = parse_pane_list(input);
        assert_eq!(map.len(), 2);
        assert_eq!(
            map["%0"],
            ("node".into(), "0:0.0".into(), "My Title".into())
        );
        assert_eq!(
            map["%1"],
            ("fish".into(), "1:0.0".into(), "Other".into())
        );
    }

    #[test]
    fn parse_pane_list_title_with_spaces() {
        let input = "%5 claude 2:1.0 ⠋ Thinking about stuff\n";
        let map = parse_pane_list(input);
        assert_eq!(
            map["%5"],
            ("claude".into(), "2:1.0".into(), "⠋ Thinking about stuff".into())
        );
    }

    #[test]
    fn parse_pane_list_empty_title() {
        // When pane_title is empty, tmux may output trailing space or not
        let input = "%0 node 0:0.0 \n%1 fish 1:0.0\n";
        let map = parse_pane_list(input);
        assert_eq!(map.len(), 2);
        assert_eq!(map["%0"].2, "");
        assert_eq!(map["%1"].2, "");
    }

    #[test]
    fn parse_pane_list_empty_input() {
        let map = parse_pane_list("");
        assert!(map.is_empty());
    }

    #[test]
    fn parse_pane_list_malformed_lines() {
        let input = "bad line\n%0 node\n%1 fish 0:0.0 ok\n";
        let map = parse_pane_list(input);
        assert_eq!(map.len(), 1);
        assert_eq!(map["%1"].0, "fish");
    }

    // --- is_active_spinner ---

    #[test]
    fn spinner_detection_braille() {
        for ch in ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'] {
            let title = format!("⠋ claude-panes {}", ch);
            assert!(is_active_spinner(&title), "should detect spinner char {}", ch);
        }
    }

    #[test]
    fn spinner_detection_empty() {
        assert!(!is_active_spinner(""));
    }

    #[test]
    fn spinner_detection_normal_title() {
        assert!(!is_active_spinner("node"));
        assert!(!is_active_spinner("fish /Users/koki"));
        assert!(!is_active_spinner("claude-panes"));
    }

    // --- is_stale_mtime ---

    #[test]
    fn stale_fresh_file() {
        let dir = std::env::temp_dir().join("claude-panes-test-fresh");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("fresh");
        fs::write(&path, "test").unwrap();
        assert!(!is_stale_mtime(&path));
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn stale_old_file() {
        let dir = std::env::temp_dir().join("claude-panes-test-old");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("old");
        // Create file, then set mtime to 60 seconds ago
        let file = fs::File::create(&path).unwrap();
        drop(file);
        let old_time =
            filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(60));
        filetime::set_file_mtime(&path, old_time).unwrap();
        assert!(is_stale_mtime(&path));
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn stale_nonexistent_file() {
        assert!(!is_stale_mtime(Path::new("/tmp/claude-panes-nonexistent")));
    }

    // --- is_symlink ---

    #[test]
    fn symlink_regular_file() {
        let dir = std::env::temp_dir().join("claude-panes-test-symlink");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("regular");
        fs::write(&path, "test").unwrap();
        assert!(!is_symlink(&path));
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn symlink_nonexistent() {
        assert!(is_symlink(Path::new("/tmp/claude-panes-nonexistent")));
    }
}
