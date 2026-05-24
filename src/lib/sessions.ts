// The snapshot the Rust backend emits — one entry per terminal session, subagents
// nested. Mirrors `Group` in `src-tauri/src/sessions.rs`; status vocabulary is the
// back/front contract — keep both sides in sync.

export type Status = "waiting" | "typing" | "reading" | "idle";

export interface Agent {
  status: Status;
  tool: string | null;
}

export interface Usage {
  text: string;
  ratio: number | null;
}

export interface Group {
  id: string;
  project: string | null;
  context: string;
  palette: number;
  status: Status;
  tool: string | null;
  agents: Agent[];
  contextTokens: number;
  model: string | null;
  contextWindow: number | null;
  usage: Usage;
  cwd: string | null;
}

// Status → color (CSS var) + sort rank. One scheme shared with the tray (DECISIONS · D14):
// yellow = waiting (the loud "needs you"), blue = working, green = idle.
export const STATUS: Record<Status, { color: string; rank: number }> = {
  waiting: { color: "var(--amber)", rank: 0 },
  typing: { color: "var(--blue-500)", rank: 1 },
  reading: { color: "var(--blue-500)", rank: 1 },
  idle: { color: "var(--green-500)", rank: 2 },
};

export function statusWord(g: Group): string {
  if (g.status === "waiting") return "NEEDS YOU";
  if (g.status === "idle") return "idle";
  return g.tool || "working";
}

export function providerOf(g: Group): string {
  return (g.id || "").split(":")[0] || "claude";
}

export const PROVIDER_LABEL: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
};
export const PROVIDER_ORDER = ["claude", "codex"];
