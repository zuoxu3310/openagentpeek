# OpenAgentPeek

English | [简体中文](README.zh-CN.md)

**See every live Claude Code and Codex session at a glance — and jump straight to the
one that needs you.**

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-macOS%20(Apple%20silicon)-black)
![Built with](https://img.shields.io/badge/built%20with-Tauri%202%20%2B%20React-24C8DB)

The open, free take on AgentPeek — a macOS menu-bar app to monitor every Claude Code and
Codex session you have running, so you can see at a glance which agent is working, idle, or
waiting on you.

![OpenAgentPeek](docs/screenshot.png)

## What it does

- **One glance, every agent.** A menu-bar traffic light — 🟡 a session is *waiting* on you,
  🔵 something's *working*, 🟢 all *quiet* — with a count of how many aren't idle.
- **Click to jump.** Click a session and its window comes to the front — the Terminal tab,
  the VSCode window, or the Codex app it's running in. No more hunting across terminals for
  the one that's blocked on you.
- **Context at a glance.** Every session shows how full its context window is.
- **Both agents, one panel.** Claude Code and Codex, side by side.
- **Read-only and zero-config.** It only ever *reads* the logs both tools already write —
  it never touches your sessions, installs no hooks, and needs no setup.

## Why

Run several agents in parallel and *you* become the bottleneck — the human tabbing through
terminals to find the one that stopped to ask a question. OpenAgentPeek turns that hunt
into a glance and a click.

## Install

> macOS, Apple silicon.

**[⬇ Download the latest .dmg](https://github.com/zuoxu3310/openagentpeek/releases/latest)** —
open it and drag OpenAgentPeek to Applications.

The build is **unsigned** for now, so macOS Gatekeeper warns on first open — right-click the
app → **Open**, or run `xattr -cr /Applications/OpenAgentPeek.app`. The first time you use
click-to-jump, macOS asks once for permission to control Terminal.

### Build from source

```bash
bun install
bun run tauri build    # needs the Rust toolchain (. "$HOME/.cargo/env") + bun
bun run tauri dev      # …or run it in development
```

## How it works

Tauri 2 (Rust) + React. The backend tails the JSONL transcripts that Claude Code
(`~/.claude/projects`) and Codex (`~/.codex/sessions`) already write, parses each into a
status + context usage, and pushes a snapshot to the popover once a second. Click-to-jump
finds the live process by its working directory and brings its window forward — all by
observation, no hooks. Architecture and source map: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Built together

OpenAgentPeek is open source and meant to be built *with* you. It's young — there's plenty
to shape, and contributions are genuinely welcome:

- **Found a bug or a rough edge?** Open an issue.
- **Want a feature?** Open an issue first so we can agree on the shape before code.
- **Sending a PR?** Keep it focused and simple, and make sure `cargo test` passes — see
  [CONTRIBUTING.md](CONTRIBUTING.md).

Good places to jump in: more terminals for click-to-jump (iTerm2, Ghostty, WezTerm…), a
third agent, or design polish in the popover.

## Built with AI

OpenAgentPeek was built almost entirely with Claude Code — the design, the Rust, the React,
and these docs — as a study in how far agent-driven development goes on a real, shippable app.

## Credits

- Built on the [openusage](https://github.com/robinebers/openusage) stack and visual system.
- Inspired by AgentPeek's "see every agent at a glance" idea.

## License

[MIT](LICENSE)
