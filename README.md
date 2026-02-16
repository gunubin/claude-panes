# claude-panes

TUI dashboard to monitor and switch between Claude Code tmux sessions.

![Rust](https://img.shields.io/badge/rust-stable-orange)
![License](https://img.shields.io/badge/license-MIT-blue)

## Features

- List all active Claude Code instances across tmux sessions
- Live preview of pane output with ANSI color support
- Show the last user prompt for each instance
- Filter by project name
- Jump directly to a pane with Enter
- Auto-refresh every second
- Auto-selects the current pane on startup
- Detects stale "working" state via mtime heartbeat and foreground process check
- Stable ordering by tmux pane position
- Configurable layout, border color, and status stripping
- Keeps stale data on transient tmux failures (no flickering)

## How It Works

```
Claude Code hooks ──> /tmp/claude-tmux/pane-*   (status + project name)
                  ──> /tmp/claude-tmux/prompt-*  (last user prompt)
                          |
claude-panes TUI <────────┼────────> tmux list-panes  (cross-reference)
                          |
                     tmux capture-pane  (live preview)
```

1. Claude Code hooks write state files on session events (start, prompt, tool use, stop, end)
2. claude-panes reads these files, cross-references with `tmux list-panes`, and displays a live dashboard
3. Stale detection: if the state file's mtime is older than 30 seconds (no `PreToolUse` heartbeat) or the pane's foreground process is a shell, the instance is marked as idle

## Prerequisites

- [tmux](https://github.com/tmux/tmux)
- [Claude Code](https://docs.anthropic.com/en/docs/claude-code) with hooks configured (see below)
- [Rust toolchain](https://rustup.rs/) (for building)

## Installation

```bash
git clone https://github.com/gunubin/claude-panes.git
cd claude-panes
cargo install --path .
```

## Hook Setup

claude-panes requires Claude Code hooks to write state files. Add the following to your `~/.claude/settings.json`:

### 1. Create the hook script

Save the following as `~/.claude/scripts/tmux-state.sh` and make it executable (`chmod +x`):

```bash
#!/bin/bash
# tmux-state.sh - Claude Code hook: write pane state for claude-panes

ICON_WORKING="●"
ICON_WAITING="◐"
ICON_IDLE="○"
ICON_ERROR="✕"

[ -z "$TMUX" ] && exit 0

input=$(cat)
event=$(echo "$input" | jq -r '.hook_event_name // "unknown"' 2>/dev/null)

PANE_ID=$(tmux display-message -p '#{pane_id}')
STATE_DIR="/tmp/claude-tmux"
PANE_FILE="$STATE_DIR/pane-${PANE_ID}"

# Project name from current directory
DIR_NAME=$(basename "$(tmux display-message -p '#{pane_current_path}')")

case "$event" in
    SessionStart)
        mkdir -p "$STATE_DIR"
        chmod 700 "$STATE_DIR" 2>/dev/null
        echo "${ICON_IDLE} ${DIR_NAME}" > "$PANE_FILE"
        ;;
    UserPromptSubmit)
        echo "${ICON_WORKING} ${DIR_NAME}" > "$PANE_FILE"
        prompt=$(echo "$input" | jq -r '.prompt // ""' 2>/dev/null)
        echo "$prompt" > "$STATE_DIR/prompt-${PANE_ID}"
        ;;
    PreToolUse)
        # Heartbeat: refresh mtime to prove Claude is still working
        [ -f "$PANE_FILE" ] && touch "$PANE_FILE"
        ;;
    Stop)
        echo "${ICON_WAITING} ${DIR_NAME}" > "$PANE_FILE"
        ;;
    SessionEnd)
        rm -f "$PANE_FILE" "$STATE_DIR/prompt-${PANE_ID}"
        ;;
esac

exit 0
```

### 2. Register hooks in settings.json

Add the hooks section to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [
      { "matcher": "", "hooks": [{ "type": "command", "command": "~/.claude/scripts/tmux-state.sh" }] }
    ],
    "UserPromptSubmit": [
      { "matcher": "", "hooks": [{ "type": "command", "command": "~/.claude/scripts/tmux-state.sh" }] }
    ],
    "PreToolUse": [
      { "matcher": "", "hooks": [{ "type": "command", "command": "~/.claude/scripts/tmux-state.sh" }] }
    ],
    "Stop": [
      { "matcher": "", "hooks": [{ "type": "command", "command": "~/.claude/scripts/tmux-state.sh" }] }
    ],
    "SessionEnd": [
      { "matcher": "", "hooks": [{ "type": "command", "command": "~/.claude/scripts/tmux-state.sh" }] }
    ]
  }
}
```

### State file format

| File | Format | Example |
|------|--------|---------|
| `/tmp/claude-tmux/pane-%<id>` | `<symbol> <project>` | `● my-project` (working), `◐ my-project` (waiting), `○ my-project` (idle), `✕ my-project` (error) |
| `/tmp/claude-tmux/prompt-%<id>` | Plain text | `Fix the login bug` |

## Usage

Run inside a tmux session:

```bash
claude-panes
```

Recommended: bind to a tmux key for quick access:

```tmux
bind C-a display-popup -E -w 60% -h 70% "claude-panes"
```

### Keybindings

| Key | Action |
|-----|--------|
| `Up` / `Down` | Navigate instances |
| `Enter` | Jump to selected pane |
| `Esc` | Clear filter (if active) or Quit |
| `Backspace` | Delete filter character |
| Any character (except space) | Filter by project name |

## Configuration

Create a config file at the platform config directory:

- **macOS**: `~/Library/Application Support/claude-panes/config.toml`
- **Linux**: `~/.config/claude-panes/config.toml`

```toml
border_color = "cyan"   # red, green, blue, cyan, gray, white, yellow, magenta, or #RRGGBB
strip_status = true     # Remove Claude Code's status bar from preview

[layout]
list_percentage = 30    # Height of instance list (%)
preview_percentage = 70 # Height of preview pane (%)
```

All fields are optional. Values shown above are the defaults.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| No instances shown | Hook not writing state files | Verify hooks are registered in `settings.json` and `tmux-state.sh` is executable |
| Icons show as boxes | Terminal doesn't support Unicode | Use a terminal with Unicode support (most modern terminals) |
| Status always shows idle | PreToolUse heartbeat not configured | Add `PreToolUse` hook to `settings.json` |
| "not inside a tmux session" | Running outside tmux | Run `claude-panes` inside a tmux session |
| Last prompt column is empty | No `UserPromptSubmit` hook or session not yet prompted | Add `UserPromptSubmit` hook; prompt will appear after next submission |

## License

MIT
