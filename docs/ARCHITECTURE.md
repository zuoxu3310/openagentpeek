# Architecture

openagentpeek is a Tauri 2 app: a Rust backend that reads agent session logs, and a React
popover that renders them.

## How it works

The backend tails the JSONL transcripts that Claude Code (`~/.claude/projects`) and Codex
(`~/.codex/sessions`) already write. It parses each into a status (working / waiting / idle)
plus context-window usage, and once a second emits a snapshot to the frontend and updates
the tray. Everything is **read-only** — no hooks, nothing written into agent config.

## Backend (`src-tauri/src/`)

- `lib.rs` — app setup (tray, NSPanel popover, settings, autostart) + the 1 s loop that
  drains parsed records, snapshots, emits the `sessions` event, and updates the tray.
  Commands: `get_settings`, `set_settings`, `jump_to_session`.
- `watcher.rs` — a `notify`-based file tailer. Walks the trees once at startup to seed byte
  offsets and replay recent tails, then follows changes, feeding records over an `mpsc`
  channel.
- `transcript.rs` — the Claude parser plus the shared `State` / `Phase` / `Status` and
  `display_status`.
- `codex_transcript.rs` — Codex `{type, payload}` events, reusing `State`.
- `providers.rs` — the `Provider` enum (Claude / Codex) adapter.
- `sessions.rs` — `SessionManager`: ingest records, group by terminal session (subagents
  nested), prune, and produce the `Group` snapshot the frontend consumes.
- `context_window.rs` — model → window size, and token formatting.
- `project.rs` — a project name from cwd / git root / remote.
- `jump.rs` — bring a clicked session's window to the front (see below).
- `panel.rs` — the native NSPanel popover, dropped just below the menu bar.
- `tray.rs` — the menu-bar traffic light, drawn pixel-by-pixel in Rust.

## Frontend (`src/`)

- `App.tsx` — listens for the `sessions` event and renders the popover (side rail + cards)
  and the settings view.
- `components/` — `session-card` (clickable → `jump_to_session`), `side-nav`,
  `settings-view`, `brand-icons`, `ui/switch`.
- `lib/sessions.ts` — the `Group` type (mirrors `sessions.rs`) and the status palette.

## Click-to-jump (`jump.rs`)

Read-only and per-host. Given the clicked session's working directory, it finds the live
agent process (`pgrep` / `lsof` / `ps`), then a host adapter focuses it:

- **Terminal.app** — match the controlling tty, select that tab (AppleScript).
- **VSCode** — `code <cwd>` focuses the folder's window.
- **Codex app** — `activate` the desktop app.

A session is matched to a process by working directory, so two sessions in the *same*
directory can't be told apart — it focuses the first match. More terminals (iTerm2,
Ghostty, WezTerm…) are great contributions. The packaged app asks once for macOS Automation
permission the first time it controls Terminal.

## Gotchas

- The tray icon is drawn in Rust (`tray::traffic_light`), not loaded from a file — a
  bundled resource path isn't present in `tauri dev` and panics on launch.
- `notify` doesn't replay existing files, so `watcher.rs` walks the tree once at startup;
  drop that and pre-launch sessions vanish until they next write.
- Idle/waiting decay is measured from each record's own ISO timestamp (via `chrono`), not
  the wall clock — so a session that ended an hour ago doesn't show "needs you" for 90 s
  after launch.
- The packaged app launches with a minimal `PATH`, so `jump.rs` falls back to VSCode's
  bundled `code` binary (since `/usr/local/bin` isn't on it).

## Tests

The backend logic is unit-tested — run `cargo test` in `src-tauri/` (59 tests).
