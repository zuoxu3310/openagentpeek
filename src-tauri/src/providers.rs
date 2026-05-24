// Per-agent adapter. Ported from `../mvp/src/providers.js`.
//
// A provider knows three things: where its session files live, how to map a file
// path to a (terminal session, subagent?) identity, and how to parse one record into
// the shared state shape. Adding a coding agent = adding one enum variant + arms.
//
// The JS used objects carrying function pointers; an enum is the idiomatic Rust
// equivalent — no boxed trait objects, dispatch is a `match`.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::transcript::{self, State};
use crate::codex_transcript;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Claude,
    Codex,
}

/// How a file path maps to a group identity.
pub struct Classify {
    pub is_sub: bool,
    pub parent_id: String,
}

pub const PROVIDERS: [Provider; 2] = [Provider::Claude, Provider::Codex];

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
        }
    }

    /// Root directory to watch. Returns the home-relative absolute path even if it
    /// doesn't exist yet (the watcher tolerates a missing dir).
    pub fn dir(self) -> PathBuf {
        let home = dirs::home_dir().unwrap_or_default();
        match self {
            Provider::Claude => home.join(".claude").join("projects"),
            Provider::Codex => home.join(".codex").join("sessions"),
        }
    }

    /// Map a session file path to its group identity.
    ///   Claude: `.../<encoded-dir>/<sessionId>.jsonl`; subagents nest under
    ///           `.../<encoded-dir>/<sessionId>/subagents/agent-*.jsonl`.
    ///   Codex:  each `rollout-*.jsonl` is one standalone session; no nesting.
    pub fn classify(self, file_path: &Path) -> Classify {
        let stem = || {
            file_path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        match self {
            Provider::Codex => Classify {
                is_sub: false,
                parent_id: stem(),
            },
            Provider::Claude => {
                let dir = file_path.parent();
                let dir_name = dir.and_then(|d| d.file_name()).map(|n| n.to_string_lossy());
                if dir_name.as_deref() == Some("subagents") {
                    // Parent terminal id is the dir two levels up from the file.
                    let parent_id = dir
                        .and_then(|d| d.parent())
                        .and_then(|d| d.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    Classify { is_sub: true, parent_id }
                } else {
                    Classify {
                        is_sub: false,
                        parent_id: stem(),
                    }
                }
            }
        }
    }

    pub fn empty_state(self, now_ts: i64) -> State {
        transcript::empty_state(now_ts)
    }

    pub fn apply_event(self, state: &State, record: &Value, now_ts: i64) -> State {
        match self {
            Provider::Claude => transcript::apply_event(state, record, now_ts),
            Provider::Codex => codex_transcript::apply_event(state, record, now_ts),
        }
    }
}
