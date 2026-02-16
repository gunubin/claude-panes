use std::process::Command;

/// Check that tmux is available and we are inside a session
pub fn check_available() -> Result<(), String> {
    let output = Command::new("tmux")
        .args(["display-message", "-p", ""])
        .output()
        .map_err(|e| format!("could not run tmux: {}. Is tmux installed?", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("not inside a tmux session: {}", stderr.trim()));
    }
    Ok(())
}

/// Validate that a pane ID looks like %<number>
pub fn is_valid_pane_id(pane_id: &str) -> bool {
    pane_id.starts_with('%')
        && pane_id.len() > 1
        && pane_id[1..].bytes().all(|b| b.is_ascii_digit())
}

/// Capture the last N lines of a tmux pane's output
pub fn capture_pane(pane_id: &str, lines: i32, strip_status: bool) -> String {
    if !is_valid_pane_id(pane_id) {
        return String::from("(invalid pane ID)");
    }

    let output = Command::new("tmux")
        .args([
            "capture-pane",
            "-t",
            pane_id,
            "-p",
            "-e", // include escape sequences for color
            "-S",
            &format!("-{}", lines),
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let raw = String::from_utf8_lossy(&o.stdout);
            if strip_status {
                strip_claude_status(raw.trim_end())
            } else {
                raw.trim_end().to_string()
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            format!("(capture failed: {})", stderr.trim())
        }
        Err(e) => format!("(failed to run tmux: {})", e),
    }
}

/// Remove Claude Code's status/prompt area from captured output.
/// Searches the bottom ~15 lines for horizontal bar lines (────)
/// and cuts at the topmost one found (above the prompt + status area).
fn strip_claude_status(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let len = lines.len();
    let search_start = len.saturating_sub(15);

    let topmost_bar = (search_start..len).find(|&i| is_horizontal_bar(lines[i]));

    if let Some(cut) = topmost_bar {
        lines[..cut].join("\n")
    } else {
        text.to_string()
    }
}

fn is_horizontal_bar(line: &str) -> bool {
    // 20+ consecutive ─ (U+2500) is a Claude Code UI separator
    line.contains("────────────────────")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    /// Run a closure with specific env vars set, then restore originals.
    /// Uses a mutex to prevent parallel tests from interfering.
    fn with_env_vars<F, R>(vars: &[(&str, Option<&str>)], f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _lock = ENV_MUTEX.lock().unwrap();
        let originals: Vec<(&str, Option<String>)> = vars
            .iter()
            .map(|(key, _)| (*key, std::env::var(key).ok()))
            .collect();
        for (key, value) in vars {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        let result = f();
        for (key, original) in &originals {
            match original {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        result
    }

    // --- current_pane_id ---

    #[test]
    fn current_pane_id_prefers_caller_pane() {
        with_env_vars(
            &[
                ("CLAUDE_PANES_CALLER_PANE", Some("%42")),
                ("TMUX_PANE", Some("%99")),
            ],
            || {
                assert_eq!(current_pane_id(), Some("%42".to_string()));
            },
        );
    }

    #[test]
    fn current_pane_id_falls_back_to_tmux() {
        with_env_vars(
            &[
                ("CLAUDE_PANES_CALLER_PANE", None),
                ("TMUX_PANE", Some("%99")),
            ],
            || {
                assert_eq!(current_pane_id(), Some("%99".to_string()));
            },
        );
    }

    #[test]
    fn current_pane_id_skips_empty_caller() {
        with_env_vars(
            &[
                ("CLAUDE_PANES_CALLER_PANE", Some("")),
                ("TMUX_PANE", Some("%7")),
            ],
            || {
                assert_eq!(current_pane_id(), Some("%7".to_string()));
            },
        );
    }

    #[test]
    fn current_pane_id_none_when_unset() {
        with_env_vars(
            &[
                ("CLAUDE_PANES_CALLER_PANE", None),
                ("TMUX_PANE", None),
            ],
            || {
                assert_eq!(current_pane_id(), None);
            },
        );
    }

    // --- is_valid_pane_id ---

    #[test]
    fn valid_pane_ids() {
        assert!(is_valid_pane_id("%0"));
        assert!(is_valid_pane_id("%1"));
        assert!(is_valid_pane_id("%123"));
        assert!(is_valid_pane_id("%999999"));
    }

    #[test]
    fn invalid_pane_ids() {
        assert!(!is_valid_pane_id(""));
        assert!(!is_valid_pane_id("%"));
        assert!(!is_valid_pane_id("0"));
        assert!(!is_valid_pane_id("%abc"));
        assert!(!is_valid_pane_id("%12abc"));
        assert!(!is_valid_pane_id("%%1"));
        assert!(!is_valid_pane_id("%1 "));
        assert!(!is_valid_pane_id("% 1"));
        assert!(!is_valid_pane_id("%1;malicious"));
        assert!(!is_valid_pane_id("%-1"));
        assert!(!is_valid_pane_id("%1\0"));
        assert!(!is_valid_pane_id("%1/../../etc"));
    }

    // --- strip_claude_status ---

    #[test]
    fn strip_no_bar() {
        let text = "line1\nline2\nline3";
        assert_eq!(strip_claude_status(text), text);
    }

    #[test]
    fn strip_with_bar() {
        let text = "output line 1\noutput line 2\n────────────────────────────\nstatus line";
        assert_eq!(strip_claude_status(text), "output line 1\noutput line 2");
    }

    #[test]
    fn strip_multiple_bars() {
        let text =
            "line1\n────────────────────────────\nmiddle\n────────────────────────────\nstatus";
        assert_eq!(strip_claude_status(text), "line1");
    }

    #[test]
    fn strip_empty() {
        assert_eq!(strip_claude_status(""), "");
    }

    // --- is_horizontal_bar ---

    #[test]
    fn horizontal_bar_detection() {
        assert!(is_horizontal_bar("────────────────────────────"));
        assert!(is_horizontal_bar("  ────────────────────────────  "));
        assert!(!is_horizontal_bar("───────────")); // too short
        assert!(!is_horizontal_bar("--------------------")); // ASCII dashes
        assert!(!is_horizontal_bar(""));
    }
}

/// Get the current tmux pane ID.
/// Prefers CLAUDE_PANES_CALLER_PANE (set by tmux display-popup wrapper)
/// over TMUX_PANE (which points to the popup's ephemeral pane).
pub fn current_pane_id() -> Option<String> {
    std::env::var("CLAUDE_PANES_CALLER_PANE")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("TMUX_PANE").ok().filter(|s| !s.is_empty()))
}

/// Jump to a specific tmux pane (select window then pane)
pub fn jump_to_pane(pane_id: &str) -> Result<(), String> {
    if !is_valid_pane_id(pane_id) {
        return Err(format!("invalid pane ID: {}", pane_id));
    }

    let status = Command::new("tmux")
        .args(["select-pane", "-t", pane_id])
        .status()
        .map_err(|e| format!("failed to run tmux select-pane: {}", e))?;

    if !status.success() {
        return Err(format!("tmux select-pane failed for pane {}", pane_id));
    }

    let status = Command::new("tmux")
        .args(["switch-client", "-t", pane_id])
        .status()
        .map_err(|e| format!("failed to run tmux switch-client: {}", e))?;

    if !status.success() {
        return Err(format!("tmux switch-client failed for pane {}", pane_id));
    }

    Ok(())
}
