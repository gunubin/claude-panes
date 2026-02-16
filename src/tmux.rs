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

/// Get the current tmux pane ID
pub fn current_pane_id() -> Option<String> {
    Command::new("tmux")
        .args(["display-message", "-p", "#{pane_id}"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
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
