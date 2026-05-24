// Claude transcript parser. One JSONL record -> updated session state, plus the
// state shape and `display_status` that both providers share.
//
// Ported from `../mvp/src/transcript.js`. Codex (`codex_transcript.rs`) reuses
// `State` and `display_status` from here; only `apply_event` differs per provider.

use serde::Serialize;
use serde_json::Value;

// Tools where the agent is reading/looking rather than writing.
const READING_TOOLS: &[&str] = &[
    "Read", "Grep", "Glob", "LS", "WebFetch", "WebSearch", "NotebookRead",
];

// Defaults for the attention heuristics; the user can override them in settings.
//   IDLE_MS         — silence before an active session reads as idle.
//   WAITING_DECAY_MS — how long a turn-ended session stays "waiting for you" before it
//                      decays to idle (otherwise every resting session screams NEEDS
//                      YOU and the signal becomes useless).
//   STALE_MS        — silence before a session is dropped entirely.
const IDLE_MS: i64 = 6_000;
const WAITING_DECAY_MS: i64 = 90 * 1000;
const STALE_MS: i64 = 30 * 60 * 1000;

/// The tunable time thresholds, in milliseconds. `Default` is the shipped behavior.
#[derive(Clone, Copy)]
pub struct Thresholds {
    pub idle_ms: i64,
    pub waiting_decay_ms: i64,
    pub stale_ms: i64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            idle_ms: IDLE_MS,
            waiting_decay_ms: WAITING_DECAY_MS,
            stale_ms: STALE_MS,
        }
    }
}

/// Internal phase of a session, before time-decay collapses it into a `Status`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Idle,
    Active,
    Waiting,
}

/// What the renderer shows. Serialized lowercase to match the JS status vocabulary
/// (`typing | reading | waiting | idle`) the frontend already speaks.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Reading,
    Typing,
    Waiting,
    Idle,
}

/// Per-session state, shared by both providers. Claude never sets `context_window`
/// (it's guessed from the model name later); Codex reports its own, so it lives here.
#[derive(Clone)]
pub struct State {
    pub phase: Phase,
    pub tool: Option<String>,
    pub last_ts: i64,
    pub cwd: Option<String>,
    pub context_tokens: u64,
    pub model: Option<String>,
    pub context_window: Option<u64>,
}

pub fn empty_state(now_ts: i64) -> State {
    State {
        phase: Phase::Idle,
        tool: None,
        last_ts: now_ts,
        cwd: None,
        context_tokens: 0,
        model: None,
        context_window: None,
    }
}

/// Content blocks array out of a record, or empty if absent.
fn content_blocks(record: &Value) -> &[Value] {
    record
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
        .map(|v| v.as_slice())
        .unwrap_or(&[])
}

/// Prefer the record's own ISO timestamp so idle/waiting decay is measured from when
/// the event actually happened, not when we happened to read the file.
pub fn event_time(record: &Value, now_ts: i64) -> i64 {
    record
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(now_ts)
}

/// Apply one parsed JSONL record to a session state, returning the new state.
/// `now_ts` is the wall-clock fallback (ms) when the record carries no timestamp.
pub fn apply_event(state: &State, record: &Value, now_ts: i64) -> State {
    let Some(rtype) = record.get("type").and_then(|t| t.as_str()) else {
        return state.clone();
    };
    let mut next = state.clone();
    // The real working directory rides along on user/assistant/system records;
    // keep the latest one we see so we can name the project from it.
    if let Some(cwd) = record.get("cwd").and_then(|c| c.as_str()) {
        if !cwd.is_empty() {
            next.cwd = Some(cwd.to_string());
        }
    }
    let ts = event_time(record, now_ts);

    match rtype {
        "assistant" => {
            // Token bookkeeping: the input side of the latest assistant turn is how
            // full the context window is right now (input + both cache tiers).
            // output_tokens is this turn's reply, not yet standing context, so it's
            // excluded. This lags one turn after a large reply or tool result — the
            // true new occupancy only lands in the *next* request's usage.
            if let Some(usage) = record.get("message").and_then(|m| m.get("usage")) {
                let u = |k: &str| usage.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
                next.context_tokens =
                    u("input_tokens") + u("cache_read_input_tokens") + u("cache_creation_input_tokens");
            }
            if let Some(model) = record.get("message").and_then(|m| m.get("model")).and_then(|m| m.as_str()) {
                next.model = Some(model.to_string());
            }

            let tool_use = content_blocks(record)
                .iter()
                .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"));
            next.last_ts = ts;
            if let Some(tu) = tool_use {
                next.phase = Phase::Active;
                next.tool = tu.get("name").and_then(|n| n.as_str()).map(String::from);
            } else {
                // No tool call: a final answer (end_turn / stop_sequence) means the
                // turn ended and it's your move; anything else is still mid-response.
                let stop = record.get("message").and_then(|m| m.get("stop_reason")).and_then(|s| s.as_str());
                next.tool = None;
                next.phase = if stop == Some("end_turn") || stop == Some("stop_sequence") {
                    Phase::Waiting
                } else {
                    Phase::Active
                };
            }
        }
        "user" => {
            let is_tool_result = content_blocks(record)
                .iter()
                .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"));
            next.last_ts = ts;
            if is_tool_result {
                // Tool finished; still mid-turn, clear the tool but stay active.
                next.tool = None;
            } else {
                // A human prompt — a new turn is starting.
                next.phase = Phase::Active;
                next.tool = None;
            }
        }
        "system" if record.get("subtype").and_then(|s| s.as_str()) == Some("turn_duration") => {
            next.phase = Phase::Waiting;
            next.tool = None;
            next.last_ts = ts;
        }
        _ => {}
    }
    next
}

/// Collapse internal state + current time into what the renderer should show.
pub fn display_status(state: &State, now_ts: i64, th: &Thresholds) -> Status {
    match state.phase {
        Phase::Waiting => {
            if now_ts - state.last_ts > th.waiting_decay_ms {
                Status::Idle
            } else {
                Status::Waiting
            }
        }
        Phase::Active => {
            if now_ts - state.last_ts > th.idle_ms {
                Status::Idle
            } else if state.tool.as_deref().is_some_and(|t| READING_TOOLS.contains(&t)) {
                Status::Reading
            } else {
                Status::Typing
            }
        }
        Phase::Idle => Status::Idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const T0: i64 = 1000;

    #[test]
    fn assistant_writing_tool_is_typing() {
        let rec = json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Edit" }] } });
        let s = apply_event(&empty_state(0), &rec, T0);
        assert_eq!(s.phase, Phase::Active);
        assert_eq!(s.tool.as_deref(), Some("Edit"));
        assert_eq!(display_status(&s, T0, &Thresholds::default()), Status::Typing);
    }

    #[test]
    fn assistant_reading_tool_is_reading() {
        let rec = json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Read" }] } });
        let s = apply_event(&empty_state(0), &rec, T0);
        assert_eq!(display_status(&s, T0, &Thresholds::default()), Status::Reading);
    }

    #[test]
    fn system_turn_duration_is_waiting() {
        let rec = json!({ "type": "system", "subtype": "turn_duration" });
        let s = apply_event(&empty_state(0), &rec, T0);
        assert_eq!(s.phase, Phase::Waiting);
        assert_eq!(display_status(&s, T0, &Thresholds::default()), Status::Waiting);
    }

    #[test]
    fn tool_result_clears_tool_but_stays_active() {
        let s = apply_event(&empty_state(0), &json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Bash" }] } }), T0);
        let s = apply_event(&s, &json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "x" }] } }), T0 + 100);
        assert_eq!(s.phase, Phase::Active);
        assert_eq!(s.tool, None);
    }

    #[test]
    fn human_prompt_starts_a_new_active_turn() {
        let s = apply_event(&empty_state(0), &json!({ "type": "system", "subtype": "turn_duration" }), T0);
        assert_eq!(s.phase, Phase::Waiting);
        let s = apply_event(&s, &json!({ "type": "user", "message": { "content": "please continue" } }), T0 + 100);
        assert_eq!(s.phase, Phase::Active);
    }

    #[test]
    fn assistant_final_answer_end_turn_is_waiting() {
        let rec = json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "done" }], "stop_reason": "end_turn" } });
        let s = apply_event(&empty_state(0), &rec, T0);
        assert_eq!(display_status(&s, T0, &Thresholds::default()), Status::Waiting);
    }

    #[test]
    fn record_timestamp_drives_last_ts_over_fallback() {
        let iso = "2026-05-22T01:00:00.000Z";
        let rec = json!({ "type": "assistant", "timestamp": iso, "message": { "content": [{ "type": "tool_use", "name": "Edit" }] } });
        let s = apply_event(&empty_state(0), &rec, 999_999);
        let expected = chrono::DateTime::parse_from_rfc3339(iso).unwrap().timestamp_millis();
        assert_eq!(s.last_ts, expected);
    }

    #[test]
    fn waiting_decays_to_idle_after_window() {
        let s = apply_event(&empty_state(0), &json!({ "type": "system", "subtype": "turn_duration" }), T0);
        let th = Thresholds::default();
        assert_eq!(display_status(&s, T0 + 1000, &th), Status::Waiting); // fresh: needs you
        assert_eq!(display_status(&s, T0 + WAITING_DECAY_MS + 1, &th), Status::Idle); // stale: dormant
    }

    #[test]
    fn active_but_silent_past_idle_ms_is_idle() {
        let s = apply_event(&empty_state(0), &json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Edit" }] } }), T0);
        assert_eq!(display_status(&s, T0 + IDLE_MS + 1, &Thresholds::default()), Status::Idle);
    }

    #[test]
    fn record_carries_cwd_into_state() {
        let rec = json!({ "type": "assistant", "cwd": "/Users/me/Downloads/AI/ai-tryon-web", "message": { "content": [{ "type": "tool_use", "name": "Edit" }] } });
        let s = apply_event(&empty_state(0), &rec, T0);
        assert_eq!(s.cwd.as_deref(), Some("/Users/me/Downloads/AI/ai-tryon-web"));
    }

    #[test]
    fn cwd_persists_when_later_record_lacks_it() {
        let s = apply_event(&empty_state(0), &json!({ "type": "assistant", "cwd": "/a/b/proj", "message": { "content": [{ "type": "tool_use", "name": "Edit" }] } }), T0);
        let s = apply_event(&s, &json!({ "type": "system", "subtype": "turn_duration" }), T0 + 100);
        assert_eq!(s.cwd.as_deref(), Some("/a/b/proj"));
    }

    #[test]
    fn malformed_record_leaves_state_unchanged() {
        let before = apply_event(&empty_state(0), &json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Edit" }] } }), T0);
        let after = apply_event(&before, &json!({ "nonsense": true }), T0 + 50);
        assert_eq!(after.phase, before.phase);
        assert_eq!(after.tool, before.tool);
        assert_eq!(after.last_ts, before.last_ts);
        assert_eq!(after.context_tokens, before.context_tokens);
    }

    #[test]
    fn empty_state_starts_clean() {
        let s = empty_state(0);
        assert_eq!(s.context_tokens, 0);
        assert_eq!(s.model, None);
    }

    #[test]
    fn assistant_usage_accumulates_input_plus_cache_tiers() {
        let rec = json!({ "type": "assistant", "message": {
            "content": [{ "type": "tool_use", "name": "Edit" }],
            "model": "claude-opus-4-7",
            "usage": { "input_tokens": 2, "cache_read_input_tokens": 161635, "cache_creation_input_tokens": 409, "output_tokens": 2104 },
        } });
        let s = apply_event(&empty_state(0), &rec, T0);
        assert_eq!(s.context_tokens, 2 + 161635 + 409);
        assert_eq!(s.model.as_deref(), Some("claude-opus-4-7"));
    }

    #[test]
    fn missing_usage_subfields_default_to_zero() {
        let rec = json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Read" }], "usage": { "input_tokens": 100 } } });
        let s = apply_event(&empty_state(0), &rec, T0);
        assert_eq!(s.context_tokens, 100);
    }

    #[test]
    fn later_record_without_usage_keeps_last_known_tokens_and_model() {
        let s = apply_event(&empty_state(0), &json!({ "type": "assistant", "message": {
            "content": [{ "type": "tool_use", "name": "Edit" }], "model": "claude-opus-4-7", "usage": { "input_tokens": 50000 },
        } }), T0);
        let s = apply_event(&s, &json!({ "type": "system", "subtype": "turn_duration" }), T0 + 100);
        assert_eq!(s.context_tokens, 50000);
        assert_eq!(s.model.as_deref(), Some("claude-opus-4-7"));
    }
}
