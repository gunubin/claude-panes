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
# Validate pane ID format (%<digits>) to prevent path traversal / injection
[[ "$PANE_ID" =~ ^%[0-9]+$ ]] || exit 0
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
