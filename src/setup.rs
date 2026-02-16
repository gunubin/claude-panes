use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub const HOOK_SCRIPT: &str = include_str!("hook_script.sh");

const SCRIPT_REL_PATH: &str = ".claude/scripts/tmux-state.sh";
const SETTINGS_REL_PATH: &str = ".claude/settings.json";
const HOOK_COMMAND: &str = "~/.claude/scripts/tmux-state.sh";

const HOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "Stop",
    "SessionEnd",
];

fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "could not determine home directory".to_string())
}

fn script_path() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(SCRIPT_REL_PATH))
}

fn settings_path() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(SETTINGS_REL_PATH))
}

/// Replace home directory prefix with ~ for display
fn display_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rel) = path.strip_prefix(&home) {
            return format!("~/{}", rel.display());
        }
    }
    path.display().to_string()
}

/// Create a single hook entry for our script
fn hook_entry() -> Value {
    json!({
        "matcher": "",
        "hooks": [{"type": "command", "command": HOOK_COMMAND}]
    })
}

/// Check if a hook entry belongs to us (contains our command)
fn is_our_hook_entry(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .is_some_and(|hs| {
            hs.iter().any(|h| {
                h.get("command").and_then(|c| c.as_str()) == Some(HOOK_COMMAND)
            })
        })
}

/// Merge our hooks into an existing settings JSON value.
/// Returns (merged_value, list_of_added_event_names).
/// Pure function: does not perform I/O.
pub fn merge_hooks_into(mut root: Value) -> (Value, Vec<String>) {
    let mut added = Vec::new();

    if !root.is_object() {
        root = json!({});
    }

    if !root.get("hooks").is_some_and(|v| v.is_object()) {
        root["hooks"] = json!({});
    }

    let entry = hook_entry();

    for &event in HOOK_EVENTS {
        let hooks_obj = root["hooks"].as_object_mut().unwrap();

        // Check if our hook already exists for this event
        let already_exists = hooks_obj
            .get(event)
            .and_then(|v| v.as_array())
            .is_some_and(|arr| arr.iter().any(is_our_hook_entry));

        if already_exists {
            continue;
        }

        // Append our entry to the event's array
        let arr = hooks_obj.entry(event).or_insert_with(|| json!([]));
        if let Some(arr) = arr.as_array_mut() {
            arr.push(entry.clone());
        }
        added.push(event.to_string());
    }

    (root, added)
}

/// Remove our hooks from settings JSON.
/// Returns (cleaned_value, list_of_removed_event_names).
/// Pure function: does not perform I/O.
pub fn remove_hooks_from(mut root: Value) -> (Value, Vec<String>) {
    let mut removed = Vec::new();

    let Some(hooks) = root.get_mut("hooks").and_then(|v| v.as_object_mut()) else {
        return (root, removed);
    };

    for &event in HOOK_EVENTS {
        if let Some(arr) = hooks.get_mut(event).and_then(|v| v.as_array_mut()) {
            let before = arr.len();
            arr.retain(|e| !is_our_hook_entry(e));
            if arr.len() < before {
                removed.push(event.to_string());
            }
        }
    }

    // Remove empty event arrays
    let empty_keys: Vec<String> = hooks
        .iter()
        .filter(|(_, v)| v.as_array().is_some_and(|a| a.is_empty()))
        .map(|(k, _)| k.clone())
        .collect();
    for key in empty_keys {
        hooks.remove(&key);
    }

    // Remove empty hooks object
    let hooks_empty = root
        .get("hooks")
        .and_then(|v| v.as_object())
        .is_some_and(|o| o.is_empty());
    if hooks_empty {
        root.as_object_mut().unwrap().remove("hooks");
    }

    (root, removed)
}

fn read_settings(path: &Path) -> Result<Value, String> {
    if path.exists() {
        let content =
            fs::read_to_string(path).map_err(|e| format!("failed to read settings: {}", e))?;
        serde_json::from_str(&content).map_err(|e| format!("failed to parse settings.json: {}", e))
    } else {
        Ok(json!({}))
    }
}

fn write_settings(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create directory: {}", e))?;
    }
    let content = serde_json::to_string_pretty(value)
        .map_err(|e| format!("failed to serialize settings: {}", e))?;
    fs::write(path, content + "\n").map_err(|e| format!("failed to write settings: {}", e))
}

pub fn run_setup() -> Result<(), String> {
    println!();
    println!("claude-panes setup");

    // 1. Create script file
    let path = script_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create directory: {}", e))?;
    }

    let script_changed = if path.exists() {
        fs::read_to_string(&path).map_or(true, |content| content != HOOK_SCRIPT)
    } else {
        true
    };

    if script_changed {
        let existed = path.exists();
        fs::write(&path, HOOK_SCRIPT).map_err(|e| format!("failed to write script: {}", e))?;
        #[cfg(unix)]
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("failed to set permissions: {}", e))?;
        if existed {
            println!("  ✓ Updated {}", display_path(&path));
        } else {
            println!("  ✓ Created {}", display_path(&path));
        }
    } else {
        println!("  ✓ {} (already up to date)", display_path(&path));
    }

    // 2. Merge hooks into settings.json
    let settings = settings_path()?;
    let root = read_settings(&settings)?;
    let (merged, added) = merge_hooks_into(root);

    if !added.is_empty() {
        write_settings(&settings, &merged)?;
        for event in &added {
            println!("  ✓ Added {} hook", event);
        }
    } else {
        for event in HOOK_EVENTS {
            println!("  ✓ {} hook (already configured)", event);
        }
    }

    println!();
    if added.is_empty() && !script_changed {
        println!("Already configured. No changes needed.");
    } else {
        println!("Setup complete. Restart Claude Code sessions to activate.");
    }

    Ok(())
}

pub fn run_check() -> Result<bool, String> {
    let mut all_ok = true;

    println!();
    println!("claude-panes setup --check");

    // Check script
    let path = script_path()?;
    if !path.exists() {
        println!("  ✗ Script missing: {}", display_path(&path));
        all_ok = false;
    } else {
        let meta = fs::metadata(&path).map_err(|e| format!("failed to stat script: {}", e))?;
        #[cfg(unix)]
        let executable = meta.permissions().mode() & 0o111 != 0;
        #[cfg(not(unix))]
        let executable = true;

        if !executable {
            println!("  ✗ Script not executable: {}", display_path(&path));
            all_ok = false;
        } else {
            let content =
                fs::read_to_string(&path).map_err(|e| format!("failed to read script: {}", e))?;
            if content == HOOK_SCRIPT {
                println!("  ✓ Script: {}", display_path(&path));
            } else {
                println!("  ✗ Script content differs: {}", display_path(&path));
                all_ok = false;
            }
        }
    }

    // Check hooks
    let settings = settings_path()?;
    if !settings.exists() {
        println!("  ✗ Settings file missing: {}", display_path(&settings));
        return Ok(false);
    }

    let root = read_settings(&settings)?;

    for &event in HOOK_EVENTS {
        let found = root
            .get("hooks")
            .and_then(|h| h.get(event))
            .and_then(|a| a.as_array())
            .is_some_and(|arr| arr.iter().any(is_our_hook_entry));

        if found {
            println!("  ✓ {} hook", event);
        } else {
            println!("  ✗ {} hook missing", event);
            all_ok = false;
        }
    }

    Ok(all_ok)
}

pub fn run_uninstall() -> Result<(), String> {
    println!();
    println!("claude-panes setup --uninstall");

    // 1. Remove hooks from settings
    let settings = settings_path()?;
    if settings.exists() {
        let root = read_settings(&settings)?;
        let (cleaned, removed) = remove_hooks_from(root);

        if !removed.is_empty() {
            write_settings(&settings, &cleaned)?;
            for event in &removed {
                println!("  ✓ Removed {} hook", event);
            }
        } else {
            println!("  · No hooks to remove");
        }
    } else {
        println!("  · No settings file found");
    }

    // 2. Remove script
    let path = script_path()?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("failed to remove script: {}", e))?;
        println!("  ✓ Removed {}", display_path(&path));
    } else {
        println!("  · Script already removed");
    }

    println!();
    println!("Uninstall complete.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_script_starts_with_shebang() {
        assert!(HOOK_SCRIPT.starts_with("#!/bin/bash"));
    }

    #[test]
    fn hook_script_ends_with_exit_zero() {
        assert!(HOOK_SCRIPT.trim_end().ends_with("exit 0"));
    }

    #[test]
    fn merge_into_empty_settings() {
        let root = json!({});
        let (merged, added) = merge_hooks_into(root);

        assert_eq!(added.len(), 5);
        assert_eq!(
            added,
            vec![
                "SessionStart",
                "UserPromptSubmit",
                "PreToolUse",
                "Stop",
                "SessionEnd"
            ]
        );

        let hooks = merged.get("hooks").unwrap().as_object().unwrap();
        assert_eq!(hooks.len(), 5);

        for event in HOOK_EVENTS {
            let arr = hooks.get(*event).unwrap().as_array().unwrap();
            assert_eq!(arr.len(), 1);
            assert!(is_our_hook_entry(&arr[0]));
        }
    }

    #[test]
    fn merge_preserves_existing_hooks() {
        let root = json!({
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "",
                        "hooks": [{"type": "command", "command": "other-tool.sh"}]
                    }
                ]
            },
            "other_key": "preserved"
        });

        let (merged, added) = merge_hooks_into(root);

        // All 5 events should be added (existing SessionStart gets appended to)
        assert_eq!(added.len(), 5);

        // Existing hook is preserved
        let session_start = merged["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(session_start.len(), 2);
        assert_eq!(
            session_start[0]["hooks"][0]["command"].as_str().unwrap(),
            "other-tool.sh"
        );
        assert!(is_our_hook_entry(&session_start[1]));

        // Other keys preserved
        assert_eq!(merged["other_key"].as_str().unwrap(), "preserved");
    }

    #[test]
    fn merge_is_idempotent() {
        let root = json!({});
        let (merged_once, added_once) = merge_hooks_into(root);
        assert_eq!(added_once.len(), 5);

        let (merged_twice, added_twice) = merge_hooks_into(merged_once.clone());
        assert!(added_twice.is_empty());
        assert_eq!(merged_once, merged_twice);
    }

    #[test]
    fn uninstall_removes_only_our_hooks() {
        let root = json!({
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "",
                        "hooks": [{"type": "command", "command": "other-tool.sh"}]
                    },
                    {
                        "matcher": "",
                        "hooks": [{"type": "command", "command": HOOK_COMMAND}]
                    }
                ],
                "UserPromptSubmit": [
                    {
                        "matcher": "",
                        "hooks": [{"type": "command", "command": HOOK_COMMAND}]
                    }
                ],
                "PreToolUse": [
                    {
                        "matcher": "",
                        "hooks": [{"type": "command", "command": HOOK_COMMAND}]
                    }
                ],
                "Stop": [
                    {
                        "matcher": "",
                        "hooks": [{"type": "command", "command": HOOK_COMMAND}]
                    }
                ],
                "SessionEnd": [
                    {
                        "matcher": "",
                        "hooks": [{"type": "command", "command": HOOK_COMMAND}]
                    }
                ]
            }
        });

        let (cleaned, removed) = remove_hooks_from(root);
        assert_eq!(removed.len(), 5);

        // Other hook in SessionStart preserved
        let session_start = cleaned["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(session_start.len(), 1);
        assert_eq!(
            session_start[0]["hooks"][0]["command"].as_str().unwrap(),
            "other-tool.sh"
        );

        // Empty event arrays removed
        assert!(cleaned["hooks"].get("UserPromptSubmit").is_none());
        assert!(cleaned["hooks"].get("PreToolUse").is_none());
        assert!(cleaned["hooks"].get("Stop").is_none());
        assert!(cleaned["hooks"].get("SessionEnd").is_none());
    }

    #[test]
    fn uninstall_cleans_empty_hooks_object() {
        let root = json!({
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "",
                        "hooks": [{"type": "command", "command": HOOK_COMMAND}]
                    }
                ]
            },
            "other_key": true
        });

        let (cleaned, removed) = remove_hooks_from(root);
        assert_eq!(removed, vec!["SessionStart"]);

        // hooks object itself removed when empty
        assert!(cleaned.get("hooks").is_none());
        // other keys preserved
        assert_eq!(cleaned["other_key"].as_bool().unwrap(), true);
    }

    #[test]
    fn uninstall_noop_on_empty_settings() {
        let root = json!({});
        let (cleaned, removed) = remove_hooks_from(root);
        assert!(removed.is_empty());
        assert_eq!(cleaned, json!({}));
    }

    #[test]
    fn merge_then_uninstall_roundtrip() {
        let original = json!({"existing": true});
        let (merged, _) = merge_hooks_into(original.clone());
        let (cleaned, removed) = remove_hooks_from(merged);

        assert_eq!(removed.len(), 5);
        assert_eq!(cleaned, original);
    }
}
