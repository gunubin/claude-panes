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

/// Check if the pane content indicates active Claude Code processing.
/// Captures the entire visible area so we can locate the separator bar
/// and inspect only the lines immediately above it.
pub fn has_spinner_in_content(pane_id: &str) -> bool {
    if !is_valid_pane_id(pane_id) {
        return false;
    }

    let output = Command::new("tmux")
        .args(["capture-pane", "-t", pane_id, "-p"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            is_active_content(&text)
        }
        Ok(o) => {
            eprintln!(
                "claude-panes: capture-pane failed for {}: {}",
                pane_id,
                String::from_utf8_lossy(&o.stderr).trim()
            );
            false
        }
        Err(e) => {
            eprintln!("claude-panes: failed to run tmux capture-pane: {}", e);
            false
        }
    }
}

/// Check if captured pane content shows signs of active Claude Code processing.
/// Finds the separator bar (────) in the bottom ~15 lines, then inspects
/// only the 3 lines immediately above it. This prevents false positives from
/// output content like `… +2 lines (ctrl+o to expand)`.
pub fn is_active_content(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let len = lines.len();
    let search_start = len.saturating_sub(15);

    // Find the *last* separator bar in the bottom ~15 lines (there may be multiple)
    let separator = (search_start..len)
        .rev()
        .find(|&i| is_horizontal_bar(lines[i]));

    // Inspect up to 3 lines above the separator (or bottom 3 lines if no separator)
    let check_end = separator.unwrap_or(len);
    let check_start = check_end.saturating_sub(3);

    for line in lines.iter().take(check_end).skip(check_start) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if contains_braille_spinner(trimmed) || trimmed.contains("· ↓") {
            return true;
        }
        let mut chars = trimmed.chars();
        if let (Some(first), Some(' ')) = (chars.next(), chars.next()) {
            if !first.is_ascii() && trimmed.contains('…') {
                return true;
            }
        }
    }
    false
}

/// Check if text contains braille spinner characters used by Claude Code.
pub fn contains_braille_spinner(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c, '⠋' | '⠙' | '⠹' | '⠸' | '⠼' | '⠴' | '⠦' | '⠧' | '⠇' | '⠏'))
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
            &[("CLAUDE_PANES_CALLER_PANE", None), ("TMUX_PANE", None)],
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

    // --- contains_braille_spinner ---

    #[test]
    fn braille_spinner_all_chars() {
        for ch in ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'] {
            let text = format!("{} Working", ch);
            assert!(
                contains_braille_spinner(&text),
                "should detect spinner char {}",
                ch
            );
        }
    }

    #[test]
    fn braille_spinner_in_status_line() {
        let text = "line1\nline2\nline3\n────────────────────────────\n⠹ Reading src/main.rs";
        assert!(contains_braille_spinner(text));
    }

    #[test]
    fn braille_spinner_absent() {
        let text = "line1\nline2\n> some prompt";
        assert!(!contains_braille_spinner(text));
    }

    #[test]
    fn braille_spinner_empty() {
        assert!(!contains_braille_spinner(""));
    }

    #[test]
    fn has_spinner_in_content_invalid_pane() {
        assert!(!has_spinner_in_content("invalid"));
        assert!(!has_spinner_in_content(""));
        assert!(!has_spinner_in_content("%abc"));
    }

    // --- is_active_content ---

    #[test]
    fn active_content_braille_spinner() {
        // Spinner above separator → detected
        let text = "some output\n⠹ Reading src/main.rs\n────────────────────────────\n> ";
        assert!(is_active_content(text));
    }

    #[test]
    fn active_content_status_line_with_tokens() {
        // Real Claude Code layout: status line above separator
        let text = "     └ Done\n\
                    ✱ Dilly-dallying… (3m 5s · ↓ 4.6k tokens · thought for 4s)\n\
                    ────────────────────────────\n\
                    > ";
        assert!(is_active_content(text));
    }

    #[test]
    fn active_content_status_line_with_ellipsis() {
        // Status line with spinner icon + ellipsis above separator
        let text = "some output\n✱ Thinking…\n────────────────────────────\n> ";
        assert!(is_active_content(text));
    }

    #[test]
    fn active_content_status_line_with_time() {
        let text = "some output\n✱ Thinking… (30s)\n────────────────────────────\n> ";
        assert!(is_active_content(text));
    }

    #[test]
    fn active_content_subagent_line() {
        // Sub-agent status above separator
        let text = "some output\n\
                    ✱ Working…\n\
                         └ Searching for 5 patterns, reading 4 files…\n\
                    ────────────────────────────\n\
                    > ";
        assert!(is_active_content(text));
    }

    #[test]
    fn active_content_idle_prompt() {
        // Idle state: no spinner, no status line
        let text = "some output\n────────────────────────────\n> ";
        assert!(!is_active_content(text));
    }

    #[test]
    fn active_content_plain_text() {
        let text = "line1\nline2\nline3";
        assert!(!is_active_content(text));
    }

    #[test]
    fn active_content_empty() {
        assert!(!is_active_content(""));
    }

    #[test]
    fn active_content_ascii_only_no_match() {
        // ASCII text with ellipsis-like content should NOT match
        let text = "Loading...\nPlease wait\n────────────────────────────\n> ";
        assert!(!is_active_content(text));
    }

    #[test]
    fn active_content_token_counter_deep() {
        // Status line buried under tip + separator + prompt (real Claude Code layout)
        let text = "content\n\
                    ✱ Vibing… (1m 5s · ↓ 1.0k tokens)\n\
                    └ Tip: Use /config to change mode\n\
                    ────────────────────────────\n\
                    > \n\
                    \n\
                    ";
        assert!(is_active_content(text));
    }

    #[test]
    fn active_content_bar_not_matched() {
        // Separator bar alone should not match
        let text = "some output\n────────────────────────────\n> ";
        assert!(!is_active_content(text));
    }

    #[test]
    fn active_content_cjk_not_matched() {
        // CJK text should not false-positive (no space after first char)
        let text = "修正方針：\n全てのです・ます調を変換…\n────────────────────────────\n> ";
        assert!(!is_active_content(text));
    }

    #[test]
    fn active_content_collapsed_lines_no_match() {
        // `… +2 lines (ctrl+o to expand)` in output area (above separator) should NOT match
        // because it's more than 3 lines above the separator
        let text = "some code output\n\
                    … +2 lines (ctrl+o to expand)\n\
                    more output\n\
                    even more output\n\
                    final output line\n\
                    ────────────────────────────\n\
                    > ";
        assert!(!is_active_content(text));
    }

    #[test]
    fn active_content_no_separator_fallback() {
        // No separator: check bottom 3 lines. Status line at bottom → detected
        let text = "line1\nline2\n✱ Vibing…";
        assert!(is_active_content(text));
    }

    #[test]
    fn active_content_no_separator_plain() {
        // No separator: bottom 3 lines are plain text → no match
        let text = "line1\nline2\nline3";
        assert!(!is_active_content(text));
    }

    #[test]
    fn active_content_down_arrow_token_counter() {
        // "· ↓" pattern alone (without braille spinner) should be detected
        let text = "content\n\
                    · ↓ 4.6k tokens\n\
                    ────────────────────────────\n\
                    > ";
        assert!(is_active_content(text));
    }

    #[test]
    fn active_content_multiple_separators() {
        // With multiple separators, only lines above the *last* separator are checked.
        // Spinner is above the first separator but >3 lines above the last → not detected.
        let text = "⠹ Working on something\n\
                    output line\n\
                    ────────────────────────────\n\
                    middle content\n\
                    more middle\n\
                    even more\n\
                    ────────────────────────────\n\
                    > ";
        assert!(!is_active_content(text));
    }

    #[test]
    fn active_content_multiple_separators_spinner_near_last() {
        // Spinner is within 3 lines above the last separator → detected
        let text = "output\n\
                    ────────────────────────────\n\
                    middle\n\
                    ⠹ Still working\n\
                    ────────────────────────────\n\
                    > ";
        assert!(is_active_content(text));
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
