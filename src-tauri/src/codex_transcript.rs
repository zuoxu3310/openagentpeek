// Codex transcript parser. Ported from `../mvp/src/codexTranscript.js`.
//
// Codex rollout records are {type, timestamp, payload} — a different protocol from
// Claude's per-message format. Activity is driven by `event_msg` subtypes, usage by
// `token_count`, identity (cwd/model) by `session_meta` / `turn_context`. The state
// shape and idle/waiting decay are shared with the Claude parser (`State` and
// `display_status` are reused), so the rest of the app treats both the same.

use serde_json::Value;

use crate::transcript::{event_time, Phase, State};

// event_msg subtypes that mean the agent is actively working. A whitelist, because
// Codex also emits metadata events (thread_name_updated, thread_rolled_back, …) that
// must NOT revive a turn that already ended.
const ACTIVE_EVENTS: &[&str] = &[
    "task_started", "user_message", "agent_message", "agent_reasoning",
    "exec_command_begin", "exec_command_end", "patch_apply_begin", "patch_apply_end",
    "web_search_begin", "web_search_end", "mcp_tool_call_begin", "mcp_tool_call_end",
];
// Subtypes that end the turn and hand control back to you.
const TURN_END_EVENTS: &[&str] = &["task_complete", "turn_aborted"];

pub fn apply_event(state: &State, record: &Value, now_ts: i64) -> State {
    let Some(rtype) = record.get("type").and_then(|t| t.as_str()) else {
        return state.clone();
    };
    let mut next = state.clone();
    let ts = event_time(record, now_ts);
    let p = record.get("payload");
    let pstr = |k: &str| p.and_then(|p| p.get(k)).and_then(|v| v.as_str());

    match rtype {
        "session_meta" => {
            if let Some(cwd) = pstr("cwd") {
                if !cwd.is_empty() {
                    next.cwd = Some(cwd.to_string());
                }
            }
            if let Some(model) = pstr("model") {
                next.model = Some(model.to_string());
            }
        }
        "turn_context" => {
            if let Some(cwd) = pstr("cwd") {
                if !cwd.is_empty() {
                    next.cwd = Some(cwd.to_string());
                }
            }
            if let Some(model) = pstr("model") {
                next.model = Some(model.to_string());
            }
            next.last_ts = ts;
        }
        "event_msg" => {
            let sub = p.and_then(|p| p.get("type")).and_then(|v| v.as_str());
            match sub {
                Some("token_count") => {
                    // Usage report. Update tokens + window only — don't touch the
                    // phase, since a token_count often lands right after task_complete
                    // and would wrongly un-wait it. input_tokens already includes the
                    // cached portion, so it's the occupancy.
                    let info = p.and_then(|p| p.get("info"));
                    if let Some(input) = info
                        .and_then(|i| i.get("last_token_usage"))
                        .and_then(|lu| lu.get("input_tokens"))
                        .and_then(|v| v.as_u64())
                    {
                        next.context_tokens = input;
                    }
                    if let Some(win) = info
                        .and_then(|i| i.get("model_context_window"))
                        .and_then(|v| v.as_u64())
                    {
                        next.context_window = Some(win);
                    }
                    next.last_ts = ts;
                }
                Some(s) if TURN_END_EVENTS.contains(&s) => {
                    next.phase = Phase::Waiting; // turn ended (or aborted); it's your move
                    next.tool = None;
                    next.last_ts = ts;
                }
                Some(s) if ACTIVE_EVENTS.contains(&s) => {
                    next.phase = Phase::Active;
                    next.last_ts = ts;
                }
                // Metadata / unknown subtypes (thread_name_updated, …): keep lastTs
                // fresh but leave the phase alone — they must not un-wait a turn.
                Some(_) => next.last_ts = ts,
                None => {}
            }
        }
        // response_item and other record types: keep lastTs fresh (it's activity) but
        // let event_msg drive the phase.
        _ => next.last_ts = ts,
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{display_status, empty_state, Status, Thresholds};
    use serde_json::json;

    const T0: i64 = 1000;
    const IDLE_MS: i64 = 6_000;
    const WAITING_DECAY_MS: i64 = 90 * 1000;

    // A codex rollout record: { type, timestamp, payload }; event_msg carries a subtype.
    fn evt(subtype: &str, extra: serde_json::Value) -> serde_json::Value {
        let mut payload = json!({ "type": subtype });
        if let serde_json::Value::Object(extra) = extra {
            for (k, v) in extra {
                payload[k] = v;
            }
        }
        json!({ "type": "event_msg", "timestamp": "", "payload": payload })
    }

    #[test]
    fn empty_state_zero_tokens_no_model_no_window() {
        let s = empty_state(0);
        assert_eq!(s.context_tokens, 0);
        assert_eq!(s.model, None);
        assert_eq!(s.context_window, None);
    }

    #[test]
    fn task_started_is_active() {
        let s = apply_event(&empty_state(0), &evt("task_started", json!({})), T0);
        assert_eq!(s.phase, Phase::Active);
        assert_eq!(display_status(&s, T0, &Thresholds::default()), Status::Typing);
    }

    #[test]
    fn task_complete_is_waiting() {
        let s = apply_event(&empty_state(0), &evt("task_complete", json!({})), T0);
        assert_eq!(s.phase, Phase::Waiting);
        assert_eq!(display_status(&s, T0, &Thresholds::default()), Status::Waiting);
    }

    #[test]
    fn user_message_starts_a_fresh_active_turn() {
        let s = apply_event(&empty_state(0), &evt("task_complete", json!({})), T0);
        assert_eq!(s.phase, Phase::Waiting);
        let s = apply_event(&s, &evt("user_message", json!({})), T0 + 100);
        assert_eq!(s.phase, Phase::Active);
    }

    #[test]
    fn token_count_records_usage_without_unwaiting() {
        let s = apply_event(&empty_state(0), &evt("task_complete", json!({})), T0);
        let tc = evt("token_count", json!({
            "info": {
                "last_token_usage": { "input_tokens": 116022, "cached_input_tokens": 115584, "output_tokens": 165 },
                "model_context_window": 258400,
            },
        }));
        let s = apply_event(&s, &tc, T0 + 50);
        assert_eq!(s.context_tokens, 116022);
        assert_eq!(s.context_window, Some(258400));
        assert_eq!(s.phase, Phase::Waiting);
    }

    #[test]
    fn turn_context_carries_cwd_and_model() {
        let rec = json!({ "type": "turn_context", "timestamp": "", "payload": { "cwd": "/Users/me/proj", "model": "gpt-5.5" } });
        let s = apply_event(&empty_state(0), &rec, T0);
        assert_eq!(s.cwd.as_deref(), Some("/Users/me/proj"));
        assert_eq!(s.model.as_deref(), Some("gpt-5.5"));
    }

    #[test]
    fn session_meta_carries_cwd() {
        let rec = json!({ "type": "session_meta", "timestamp": "", "payload": { "cwd": "/Users/me/proj2", "model_provider": "openai" } });
        let s = apply_event(&empty_state(0), &rec, T0);
        assert_eq!(s.cwd.as_deref(), Some("/Users/me/proj2"));
    }

    #[test]
    fn active_but_silent_past_idle_is_idle() {
        let s = apply_event(&empty_state(0), &evt("task_started", json!({})), T0);
        assert_eq!(display_status(&s, T0 + IDLE_MS + 1, &Thresholds::default()), Status::Idle);
    }

    #[test]
    fn waiting_decays_to_idle() {
        let s = apply_event(&empty_state(0), &evt("task_complete", json!({})), T0);
        let th = Thresholds::default();
        assert_eq!(display_status(&s, T0 + 1000, &th), Status::Waiting);
        assert_eq!(display_status(&s, T0 + WAITING_DECAY_MS + 1, &th), Status::Idle);
    }

    #[test]
    fn metadata_event_after_complete_does_not_unwait() {
        let s = apply_event(&empty_state(0), &evt("task_complete", json!({})), T0);
        let s = apply_event(&s, &evt("thread_name_updated", json!({})), T0 + 100);
        assert_eq!(s.phase, Phase::Waiting);
    }

    #[test]
    fn turn_aborted_ends_the_turn() {
        let s = apply_event(&empty_state(0), &evt("turn_aborted", json!({})), T0);
        assert_eq!(s.phase, Phase::Waiting);
    }

    #[test]
    fn malformed_record_leaves_state_unchanged() {
        let before = apply_event(&empty_state(0), &evt("task_started", json!({})), T0);
        let after = apply_event(&before, &json!({ "nonsense": true }), T0 + 10);
        assert_eq!(after.phase, before.phase);
        assert_eq!(after.last_ts, before.last_ts);
    }
}
