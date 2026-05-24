# Contributing to openagentpeek

Thanks for helping shape openagentpeek. It's a young project — issues and pull requests are
both welcome.

## Ground rules

- **Keep it simple.** This is a glance-and-go menu-bar app. We'd rather say no to a feature
  than let it sprawl — focus beats completeness here.
- **Talk before big changes.** For anything beyond a small fix, open an issue first so we
  can agree on the shape before you write code.
- **Tested and green.** The backend logic lives in pure Rust modules with unit tests.
  `cargo test` (in `src-tauri/`) must pass, and changes to parsing/status/usage logic should
  come with tests.

## Getting set up

```bash
bun install
bun run tauri dev            # run it — needs the Rust toolchain (. "$HOME/.cargo/env") + bun
cd src-tauri && cargo test   # backend tests
```

Where everything lives and how it fits together: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Good first contributions

- **A new terminal for click-to-jump** (iTerm2, Ghostty, WezTerm, Kitty…) — the host
  adapters are in `src-tauri/src/jump.rs`; only Terminal.app and VSCode are handled today.
- **A third agent** (Gemini CLI, opencode…) — providers are an enum in
  `src-tauri/src/providers.rs`; adding one should be a single new variant.
- **Design and UX polish** in the React popover (`src/`).

## Reporting bugs

Open an issue with what you did, what you expected, what happened, and — if it's visual — a
screenshot. The app logs to `~/Library/Logs/com.zuoxu.openagentpeek/`, which helps a lot.
