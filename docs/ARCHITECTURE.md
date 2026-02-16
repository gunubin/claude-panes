# Architecture

Technical details of how claude-panes works internally.

## How It Works

```
Claude Code hooks ──> ~/.claude/pane-state/pane-*   (status + project name)
                  ──> ~/.claude/pane-state/prompt-*  (last user prompt)
                              |
claude-panes TUI <------------+--------> tmux list-panes  (cross-reference + pane title)
                              |
                         tmux capture-pane  (live preview)
```

1. Claude Code hooks write state files on session events (start, prompt, tool use, stop, end)
2. claude-panes reads these files, cross-references with `tmux list-panes`, and displays a live dashboard
3. Stale "working" status is corrected to "idle" when the foreground process is a shell (Claude exited) or the heartbeat has stopped (unless a pane title spinner indicates Claude is still thinking)

## State File Format

| File | Format | Example |
|------|--------|---------|
| `~/.claude/pane-state/pane-%<id>` | `<symbol> <project>` | `▶ my-project` (working), `● my-project` (waiting), `○ my-project` (idle), `✕ my-project` (error) |
| `~/.claude/pane-state/prompt-%<id>` | Plain text | `Fix the login bug` |

## Manual Hook Setup

If you prefer to set up hooks manually instead of using `claude-panes setup`:

### 1. Create the hook script

Save the following as `~/.claude/scripts/tmux-state.sh` and make it executable (`chmod +x`):

```bash
#!/bin/bash
# tmux-state.sh - Claude Code hook: write pane state for claude-panes

ICON_WORKING="▶"
ICON_WAITING="●"
ICON_IDLE="○"
ICON_ERROR="✕"

[ -z "$TMUX" ] && exit 0

input=$(cat)
event=$(echo "$input" | jq -r '.hook_event_name // "unknown"' 2>/dev/null)

PANE_ID="$TMUX_PANE"
[ -z "$PANE_ID" ] && exit 0
STATE_DIR="$HOME/.claude/pane-state"
PANE_FILE="$STATE_DIR/pane-${PANE_ID}"

# Project name from this pane's directory (-t ensures correct pane, not active pane)
DIR_NAME=$(basename "$(tmux display-message -p -t "$PANE_ID" '#{pane_current_path}')")

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
