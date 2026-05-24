import { invoke } from "@tauri-apps/api/core";

import { cn } from "@/lib/utils";
import { STATUS, statusWord, providerOf, type Group } from "@/lib/sessions";
import { UsageBar } from "@/components/usage-bar";

// One session, in openusage's per-item language: roomier rows, a status pill on the
// right, a rounded usage meter, then the figures muted underneath. Needs-you gets an
// amber wash + the breathing left bar (our one perpetual motion).
//
// Clicking the card jumps to the session's host window (terminal tab / VSCode / Codex
// app) — the backend resolves the window from the cwd. See `jump.rs`.
export function SessionCard({ g }: { g: Group }) {
  const meta = STATUS[g.status];
  const hot = g.status === "waiting";

  const jump = () =>
    invoke("jump_to_session", { provider: providerOf(g), cwd: g.cwd }).catch((e) =>
      console.error("jump to session failed:", e)
    );

  return (
    <div
      onClick={jump}
      title={g.cwd ? `Jump to ${g.cwd}` : "Jump to session"}
      className={cn(
        "relative cursor-pointer px-4 py-3 transition-colors hover:bg-foreground/[0.04]",
        hot && "bg-[linear-gradient(90deg,rgba(255,194,77,0.10),transparent_75%)]",
        g.status === "idle" && "opacity-55"
      )}
    >
      {hot && (
        <span className="breathe absolute left-0 top-1.5 bottom-1.5 w-[3px] rounded-full bg-amber" />
      )}

      <div className="mb-2 flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-baseline gap-1.5">
          <h3 className="truncate text-sm font-semibold tracking-tight text-foreground">
            {g.project || "…"}
          </h3>
          {g.agents.length > 0 && (
            <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
              +{g.agents.length}
            </span>
          )}
        </div>
        <div
          className="flex shrink-0 items-center gap-1.5 font-mono text-[11px] font-medium"
          style={{ color: meta.color }}
        >
          <span className="size-2 rounded-full" style={{ background: meta.color }} />
          <span className="max-w-[120px] truncate">{statusWord(g)}</span>
        </div>
      </div>

      {g.usage.ratio != null && <UsageBar ratio={g.usage.ratio} color={meta.color} />}

      <div className="mt-1.5 flex items-baseline justify-between gap-3 font-mono text-[10px]">
        <span className="shrink-0 text-muted-foreground">{g.usage.text}</span>
        <span className="min-w-0 truncate text-right text-muted-foreground/70">
          {g.context}
        </span>
      </div>
    </div>
  );
}
