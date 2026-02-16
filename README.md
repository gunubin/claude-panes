# claude-panes

**Monitor all your Claude Code tmux sessions from a single dashboard.**

![Rust](https://img.shields.io/badge/rust-stable-orange)
![License](https://img.shields.io/badge/license-MIT-blue)

![screenshot](assets/screenshot.png)

## Why claude-panes?

Running multiple Claude Code sessions in tmux makes it hard to know which instance is active, what it's working on, or whether it's stuck. claude-panes gives you a live dashboard to monitor and switch between all instances at a glance.

## Features

- **Multi-instance monitoring** -- List all active Claude Code instances across tmux sessions
- **Live preview** -- Pane output with ANSI color support
- **Last prompt display** -- See what each instance was asked
- **Smart filtering** -- Type to filter by project name; shortest unique keywords (e.g. `m1`, `m2`) are auto-generated for quick selection
- **Auto-jump** -- When filtering narrows to a single match, jumps automatically
- **Quick navigation** -- Jump directly to any pane with Enter
- **Auto-refresh** -- Updates every second with stable ordering
- **Pane auto-select** -- Highlights the current pane on startup
- **Stale detection** -- Detects when Claude has exited or stopped responding; pane title spinner detection prevents false "idle" during long thinking
- **Configurable** -- Layout, border color, and status stripping via config file

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

claude-panes requires Claude Code hooks to write state files.

### Automatic (recommended)

```bash
claude-panes setup
```

This will:
1. Create `~/.claude/scripts/tmux-state.sh` with the hook script
2. Add 5 hooks to `~/.claude/settings.json` (existing hooks are preserved)

Verify the setup:

```bash
claude-panes setup --check
```

To remove hooks and script:

```bash
claude-panes setup --uninstall
```

For manual setup or technical details (hook script, state file format, architecture), see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Usage

Run inside a tmux session:

```bash
claude-panes
```

Recommended: bind to a tmux key for quick access:

```tmux
bind C-a display-popup -E -w 60% -h 70% "CLAUDE_PANES_CALLER_PANE=$TMUX_PANE claude-panes"
```

> `CLAUDE_PANES_CALLER_PANE` tells claude-panes which pane launched the popup, so it can auto-select the correct instance.

### Keybindings

| Key | Action |
|-----|--------|
| `Up` / `Down` | Navigate instances |
| `Enter` | Jump to selected pane |
| `Esc` | Quit |
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
| No instances shown | Hook not writing state files | Run `claude-panes setup --check` to verify setup |
| Icons show as boxes | Terminal doesn't support Unicode | Use a terminal with Unicode support (most modern terminals) |
| Status always shows idle | PreToolUse heartbeat not configured | Run `claude-panes setup` to install all hooks |
| Status flips to idle during long thinking | Claude thinks >30s without tool use and pane title has no spinner | Spinner detection mitigates this; some terminals may not expose pane title |
| "not inside a tmux session" | Running outside tmux | Run `claude-panes` inside a tmux session |
| Last prompt column is empty | No `UserPromptSubmit` hook or session not yet prompted | Add `UserPromptSubmit` hook; prompt will appear after next submission |

## Contributing

Contributions welcome! Fork, branch, and open a PR.

```bash
cargo test && cargo clippy && cargo fmt --check
```

## License

MIT
