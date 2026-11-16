# Treills

Rust terminal dashboard for monitoring local **Claude Code** and **Codex** sessions over SSH.

This project is a **clean-room Rust implementation inspired by** [yarg119/trellis](https://github.com/yarg119/trellis). It is not a byte-for-byte translation, nor an official fork. The upstream project provides a larger Claude harness with planning rules, optional Jev routing, third-party skills, and an embedded PTY; Treills focuses first on a portable, read-only activity dashboard and opt-in metadata hooks. Upstream code and vendored assets are not copied.

## Features

- Native Rust TUI with session list, per-session agent tree, recent tools and skills, and chronological activity
- Passive discovery of local Claude Code and Codex JSONL transcripts
- Optional Claude/Codex **metadata-only** hooks for more reliable starts, tool calls, and subagent lifecycle events
- Start Claude Code or Codex alongside the dashboard via tmux, or attach to existing sessions
- No listener, cloud service, API key, prompt capture, or background daemon required
- SSH-friendly: run `treills monitor` after connecting to the machine running the CLI

The dashboard labels a session **recent** when events arrive, **idle** when they do not, and **ended** when an explicit end event is observed. Recent activity does **not** prove that a CLI process is still running. Skill tool invocations are **confirmed**, whereas an inferred SKILL.md read is labeled **candidate**.

## Build

Rust stable (edition 2021) is required.

```bash
git clone https://github.com/moveju112/treills.git
cd treills
cargo build --release
install -m 755 target/release/treills ~/.local/bin/treills
```

## Usage

```bash
treills monitor                 # live local dashboard (also works over SSH)
treills sessions                # non-interactive list
treills doctor                  # inspect install and paths
treills install --dry-run       # show which configs would change
treills install --with-codex    # opt-in metadata hooks for both CLIs
treills run --cli claude        # tmux split: Claude Code + Treills
treills run --cli codex         # tmux split: Codex + Treills
treills events --follow         # sanitized hook events
treills uninstall               # remove only hooks installed by Treills
```

Use **q** to quit, **Up/Down** to select a session, and **r** to refresh.

Hook installation backs up existing `~/.claude/settings.json` and, when opted in, `~/.codex/hooks.json`. It *adds* hook groups rather than overwriting other hooks. On first launch Codex may require a hook trust approval. Running `uninstall` never deletes other users' hooks.

## Privacy

By default, passive monitoring reads local CLI transcript files in memory. The optional hooks persist **only** timestamps, source, session identifiers, working directories, model names, tool names, explicitly invoked skill names, and subagent identifiers/types under `~/.local/state/treills/events.jsonl`. Prompt bodies, tool arguments, tool outputs, and secrets are not written by Treills. Access to this directory should remain limited to the current user.

`~/.claude/projects` and `~/.codex/sessions` are CLI-owned formats, not stable APIs. Passive detection is best-effort; install the hooks for more reliable activity. The installed hooks observe events only; they do not approve/deny tools, modify prompts, or execute tests.

## Current limits vs upstream

This first Rust port does **not** include: the upstream full-screen embedded Claude PTY, resume/worktree launcher, Jev policy routing, advisor integration, original project templates, skill vendoring, or the original guard/stop-gate policy. `treills run` provides a tmux split as the practical SSH alternative.

Support: Linux and macOS terminals (Windows may need a POSIX environment and tmux). Requires the CLI session files or installed hooks to be accessible to the same OS user.

## Credits

UI/workflow inspiration: [yarg119/trellis](https://github.com/yarg119/trellis). This is a new Rust implementation; refer to the original project for its own feature set and terms.
