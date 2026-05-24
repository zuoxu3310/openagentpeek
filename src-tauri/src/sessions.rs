// Session manager: tail records in, grouped snapshot out.
// Ported from `../mvp/src/sessions.js`.
//
// Data flow: watcher -> `ingest` (one record) -> `snapshot` (one entry per terminal
// session, subagents nested) -> emitted to the frontend.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::context_window::{format_usage, Usage};
use crate::project::project_identity;
use crate::providers::Provider;
use crate::transcript::{display_status, State, Status, Thresholds};

// Lower rank = louder; used to roll a group's status up from its members.
fn rank(s: Status) -> u8 {
    match s {
        Status::Waiting => 0,
        Status::Typing | Status::Reading => 1,
        Status::Idle => 2,
    }
}

struct Session {
    parent_id: String,
    is_sub: bool,
    project: String,
    context: String,
    state: State,
    provider: Provider,
}

/// A subagent row inside a group.
#[derive(Serialize)]
pub struct Agent {
    pub status: Status,
    pub tool: Option<String>,
}

/// One terminal session (plus its subagents) — the frontend's row contract.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub project: Option<String>,
    pub context: String,
    pub palette: usize,
    pub status: Status,
    pub tool: Option<String>,
    pub agents: Vec<Agent>,
    pub context_tokens: u64,
    pub model: Option<String>,
    pub context_window: Option<u64>,
    pub usage: Usage,
    /// The session's working directory — what `jump` matches against a live process to
    /// find the host window. None until a record carrying a cwd has arrived.
    pub cwd: Option<String>,
}

pub struct SessionManager {
    sessions: HashMap<PathBuf, Session>,
    palettes: HashMap<String, usize>, // parentId -> palette index (stable color per group)
    next_palette: usize,
    id_cache: HashMap<String, (String, String)>, // cwd -> (primary, context)
}

impl SessionManager {
    pub fn new() -> Self {
        SessionManager {
            sessions: HashMap::new(),
            palettes: HashMap::new(),
            next_palette: 0,
            id_cache: HashMap::new(),
        }
    }

    fn identity(&mut self, cwd: &str) -> (String, String) {
        if let Some(cached) = self.id_cache.get(cwd) {
            return cached.clone();
        }
        let id = project_identity(cwd);
        self.id_cache.insert(cwd.to_string(), id.clone());
        id
    }

    fn palette_for(&mut self, parent_id: &str) -> usize {
        if let Some(&p) = self.palettes.get(parent_id) {
            return p;
        }
        let p = self.next_palette % 6;
        self.palettes.insert(parent_id.to_string(), p);
        self.next_palette += 1;
        p
    }

    pub fn ingest(&mut self, file_path: &Path, record: &serde_json::Value, now_ts: i64, prov: Provider) {
        let now = now_ts;
        if !self.sessions.contains_key(file_path) {
            let c = prov.classify(file_path);
            // Namespace the group id by provider so a Claude and a Codex session with
            // colliding local ids never merge into one group.
            let session = Session {
                parent_id: format!("{}:{}", prov.name(), c.parent_id),
                is_sub: c.is_sub,
                project: "…".to_string(),
                context: String::new(),
                state: prov.empty_state(now),
                provider: prov,
            };
            self.sessions.insert(file_path.to_path_buf(), session);
        }

        // Apply the record, then read back cwd to (re)derive the project label.
        let (new_state, cwd) = {
            let s = self.sessions.get(file_path).unwrap();
            let new_state = s.provider.apply_event(&s.state, record, now);
            let cwd = new_state.cwd.clone();
            (new_state, cwd)
        };
        let identity = cwd.as_deref().filter(|c| !c.is_empty()).map(|c| self.identity(c));
        let s = self.sessions.get_mut(file_path).unwrap();
        s.state = new_state;
        // Name from the git repo root once a cwd appears; until then keep the neutral
        // placeholder rather than a fake leaf guessed from the encoded dir.
        if let Some((primary, context)) = identity {
            s.project = primary;
            s.context = context;
        }
    }

    pub fn prune(&mut self, now_ts: i64, stale_ms: i64) {
        self.sessions.retain(|_, s| now_ts - s.state.last_ts <= stale_ms);
        // Drop palette assignments for groups that no longer have any sessions.
        let live: std::collections::HashSet<&String> =
            self.sessions.values().map(|s| &s.parent_id).collect();
        self.palettes.retain(|parent_id, _| live.contains(parent_id));
    }

    pub fn snapshot(&mut self, now_ts: i64, th: &Thresholds) -> Vec<Group> {
        // Build mutable group accumulators keyed by parent_id, then finalize.
        struct Acc {
            project: Option<String>,
            context: String,
            palette: usize,
            parent_status: Option<Status>,
            tool: Option<String>,
            agents: Vec<Agent>,
            context_tokens: u64,
            model: Option<String>,
            context_window: Option<u64>,
            cwd: Option<String>,
        }

        // Resolve palettes first to satisfy the borrow checker (palette_for is &mut self).
        let parent_ids: Vec<String> = {
            let mut seen: Vec<String> = Vec::new();
            for s in self.sessions.values() {
                if !seen.contains(&s.parent_id) {
                    seen.push(s.parent_id.clone());
                }
            }
            seen
        };
        let mut palettes: HashMap<String, usize> = HashMap::new();
        for pid in &parent_ids {
            let p = self.palette_for(pid);
            palettes.insert(pid.clone(), p);
        }

        let mut accs: HashMap<String, Acc> = HashMap::new();
        // Preserve a stable order (insertion order of parent_ids).
        let mut order: Vec<String> = Vec::new();

        for s in self.sessions.values() {
            let acc = accs.entry(s.parent_id.clone()).or_insert_with(|| {
                order.push(s.parent_id.clone());
                Acc {
                    project: None,
                    context: String::new(),
                    palette: *palettes.get(&s.parent_id).unwrap_or(&0),
                    parent_status: None,
                    tool: None,
                    agents: Vec::new(),
                    context_tokens: 0,
                    model: None,
                    context_window: None,
                    cwd: None,
                }
            });

            let status = display_status(&s.state, now_ts, th);
            if s.is_sub {
                acc.agents.push(Agent {
                    status,
                    tool: s.state.tool.clone(),
                });
            } else {
                acc.parent_status = Some(status);
                acc.tool = s.state.tool.clone();
                acc.project = Some(s.project.clone());
                acc.context = s.context.clone();
                acc.context_tokens = s.state.context_tokens;
                acc.model = s.state.model.clone();
                acc.context_window = s.state.context_window;
                acc.cwd = s.state.cwd.clone();
            }
            // Name from any member if the terminal session hasn't been seen yet.
            if acc.project.is_none() {
                acc.project = Some(s.project.clone());
            }
        }

        // A group is as busy as its busiest member, so a parent never shows idle while
        // a subagent is actively working. Subagent "waiting" is not a needs-you signal
        // (subagents don't wait on the user), so treat it as idle.
        order
            .into_iter()
            .map(|pid| {
                let acc = accs.remove(&pid).unwrap();
                let mut best = Status::Idle;
                let mut consider = |c: Status| {
                    if rank(c) < rank(best) {
                        best = c;
                    }
                };
                consider(acc.parent_status.unwrap_or(Status::Idle));
                for a in &acc.agents {
                    consider(if a.status == Status::Waiting {
                        Status::Idle
                    } else {
                        a.status
                    });
                }
                let usage = format_usage(acc.context_tokens, acc.model.as_deref(), acc.context_window);
                Group {
                    id: pid,
                    project: acc.project,
                    context: acc.context,
                    palette: acc.palette,
                    status: best,
                    tool: acc.tool,
                    agents: acc.agents,
                    context_tokens: acc.context_tokens,
                    model: acc.model,
                    context_window: acc.context_window,
                    usage,
                    cwd: acc.cwd,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::path::Path;

    // Default stale window, used to exercise prune.
    const STALE_MS: i64 = 30 * 60 * 1000;
    const FILE_A: &str = "/Users/me/.claude/projects/-Users-me-projA/sess-aaa.jsonl";
    const FILE_B: &str = "/Users/me/.claude/projects/-Users-me-projB/sess-bbb.jsonl";
    const SUB_A: &str = "/Users/me/.claude/projects/-Users-me-projA/sess-aaa/subagents/agent-xyz.jsonl";
    const T0: i64 = 1000;

    fn edit_rec() -> Value {
        json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Edit" }] } })
    }
    fn read_rec() -> Value {
        json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Read" }] } })
    }
    fn ingest(m: &mut SessionManager, fp: &str, rec: &Value, ts: i64, prov: Provider) {
        m.ingest(Path::new(fp), rec, ts, prov);
    }

    #[test]
    fn one_file_yields_one_session() {
        let mut m = SessionManager::new();
        ingest(&mut m, FILE_A, &edit_rec(), T0, Provider::Claude);
        let snap = m.snapshot(T0, &Thresholds::default());
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].status, Status::Typing);
        assert_eq!(snap[0].palette, 0);
    }

    #[test]
    fn two_files_get_distinct_palettes() {
        let mut m = SessionManager::new();
        ingest(&mut m, FILE_A, &edit_rec(), T0, Provider::Claude);
        ingest(&mut m, FILE_B, &edit_rec(), T0, Provider::Claude);
        let snap = m.snapshot(T0, &Thresholds::default());
        assert_eq!(snap.len(), 2);
        let mut palettes: Vec<usize> = snap.iter().map(|g| g.palette).collect();
        palettes.sort();
        assert_eq!(palettes, vec![0, 1]);
    }

    #[test]
    fn prune_drops_stale_sessions() {
        let mut m = SessionManager::new();
        ingest(&mut m, FILE_A, &edit_rec(), T0, Provider::Claude);
        m.prune(T0 + STALE_MS + 1, STALE_MS);
        assert_eq!(m.snapshot(T0 + STALE_MS + 1, &Thresholds::default()).len(), 0);
    }

    #[test]
    fn snapshot_carries_a_short_project_label() {
        let mut m = SessionManager::new();
        ingest(&mut m, FILE_A, &edit_rec(), T0, Provider::Claude);
        let p = m.snapshot(T0, &Thresholds::default())[0].project.clone();
        assert!(p.is_some_and(|s| !s.is_empty()));
    }

    #[test]
    fn project_name_comes_from_record_cwd() {
        let mut m = SessionManager::new();
        let rec = json!({ "type": "assistant", "cwd": "/Users/me/Downloads/AI/ai-tryon-web", "message": { "content": [{ "type": "tool_use", "name": "Edit" }] } });
        ingest(&mut m, FILE_A, &rec, T0, Provider::Claude);
        assert_eq!(m.snapshot(T0, &Thresholds::default())[0].project.as_deref(), Some("ai-tryon-web"));
    }

    #[test]
    fn snapshot_exposes_the_current_tool() {
        let mut m = SessionManager::new();
        ingest(&mut m, FILE_A, &edit_rec(), T0, Provider::Claude);
        assert_eq!(m.snapshot(T0, &Thresholds::default())[0].tool.as_deref(), Some("Edit"));
    }

    #[test]
    fn subagent_nests_under_parent_as_one_group() {
        let mut m = SessionManager::new();
        ingest(&mut m, FILE_A, &edit_rec(), T0, Provider::Claude);
        ingest(&mut m, SUB_A, &read_rec(), T0, Provider::Claude);
        let snap = m.snapshot(T0, &Thresholds::default());
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].status, Status::Typing); // parent's own status
        assert_eq!(snap[0].agents.len(), 1);
        assert_eq!(snap[0].agents[0].status, Status::Reading);
    }

    #[test]
    fn subagent_shares_group_palette() {
        let mut m = SessionManager::new();
        ingest(&mut m, FILE_A, &edit_rec(), T0, Provider::Claude);
        ingest(&mut m, SUB_A, &read_rec(), T0, Provider::Claude);
        assert_eq!(m.snapshot(T0, &Thresholds::default())[0].palette, 0);
    }

    #[test]
    fn group_busy_when_subagent_works_even_if_parent_idle() {
        let mut m = SessionManager::new();
        ingest(&mut m, FILE_A, &json!({ "type": "system", "subtype": "turn_duration" }), T0, Provider::Claude);
        let later = T0 + 200 * 1000; // past the waiting-decay window
        ingest(&mut m, SUB_A, &edit_rec(), later, Provider::Claude);
        assert_eq!(m.snapshot(later, &Thresholds::default())[0].status, Status::Typing);
    }

    #[test]
    fn snapshot_carries_a_context_line() {
        let mut m = SessionManager::new();
        let rec = json!({ "type": "assistant", "cwd": "/Users/me/Downloads/AI/ai-tryon-web", "message": { "content": [{ "type": "tool_use", "name": "Edit" }] } });
        ingest(&mut m, FILE_A, &rec, T0, Provider::Claude);
        // Context is always a String (possibly empty); just assert the field resolves.
        let _ = &m.snapshot(T0, &Thresholds::default())[0].context;
    }

    #[test]
    fn snapshot_exposes_context_usage_from_terminal() {
        let mut m = SessionManager::new();
        let usage_rec = json!({ "type": "assistant", "message": {
            "content": [{ "type": "tool_use", "name": "Edit" }],
            "model": "claude-opus-4-7",
            "usage": { "input_tokens": 2, "cache_read_input_tokens": 161635, "cache_creation_input_tokens": 409 },
        } });
        ingest(&mut m, FILE_A, &usage_rec, T0, Provider::Claude);
        let g = &m.snapshot(T0, &Thresholds::default())[0];
        assert_eq!(g.usage.text, "162k / 1M");
        let r = g.usage.ratio.unwrap();
        assert!(r > 0.0 && r < 1.0);
    }

    #[test]
    fn snapshot_usage_clean_zero_before_token_data() {
        let mut m = SessionManager::new();
        ingest(&mut m, FILE_A, &edit_rec(), T0, Provider::Claude); // no usage/model -> unknown window
        let g = &m.snapshot(T0, &Thresholds::default())[0];
        assert_eq!(g.usage.text, "0");
        assert_eq!(g.usage.ratio, None);
    }

    #[test]
    fn codex_session_uses_its_own_reported_window() {
        let codex_file = "/Users/me/.codex/sessions/2026/05/rollout-abc.jsonl";
        let mut m = SessionManager::new();
        ingest(&mut m, codex_file, &json!({ "type": "turn_context", "payload": { "cwd": "/tmp/nope-proj", "model": "gpt-5.5" } }), T0, Provider::Codex);
        ingest(&mut m, codex_file, &json!({ "type": "event_msg", "payload": { "type": "task_started" } }), T0, Provider::Codex);
        ingest(&mut m, codex_file, &json!({ "type": "event_msg", "payload": {
            "type": "token_count", "info": { "last_token_usage": { "input_tokens": 116022 }, "model_context_window": 258400 },
        } }), T0, Provider::Codex);
        let snap = m.snapshot(T0, &Thresholds::default());
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].status, Status::Typing); // task_started -> working
        assert_eq!(snap[0].usage.text, "116k / 258k"); // codex's own window as denominator
    }

    #[test]
    fn claude_and_codex_same_local_id_stay_separate() {
        let mut m = SessionManager::new();
        ingest(&mut m, "/u/.claude/projects/d/collide.jsonl", &edit_rec(), T0, Provider::Claude);
        ingest(&mut m, "/u/.codex/sessions/collide.jsonl", &json!({ "type": "event_msg", "payload": { "type": "task_started" } }), T0, Provider::Codex);
        assert_eq!(m.snapshot(T0, &Thresholds::default()).len(), 2); // provider-namespaced -> not merged
    }
}
