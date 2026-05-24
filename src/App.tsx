import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { MoonStar } from "lucide-react";
import {
  PROVIDER_LABEL,
  PROVIDER_ORDER,
  STATUS,
  providerOf,
  type Group,
} from "@/lib/sessions";
import {
  DEFAULT_SETTINGS,
  getSettings,
  setSettings,
  type Settings,
} from "@/lib/settings";
import { SideNav, type Filter, type View } from "@/components/side-nav";
import { SessionCard } from "@/components/session-card";
import { SettingsView } from "@/components/settings-view";

function Section({ provider, groups }: { provider: string; groups: Group[] }) {
  const sorted = [...groups].sort((a, b) => STATUS[a.status].rank - STATUS[b.status].rank);
  return (
    <section className="border-t border-border first:border-t-0">
      <div className="flex items-center gap-2 px-4 pb-1.5 pt-3 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
        {PROVIDER_LABEL[provider] || provider}
        <span className="rounded-full bg-white/6 px-1.5 py-px font-mono text-[10px] font-medium text-muted-foreground/80">
          {groups.length}
        </span>
      </div>
      <div className="pb-1">
        {sorted.map((g) => (
          <SessionCard key={g.id} g={g} />
        ))}
      </div>
    </section>
  );
}

function EmptyState() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 text-muted-foreground">
      <MoonStar className="size-6 opacity-40" />
      <p className="font-mono text-[11px] opacity-70">All sessions quiet</p>
    </div>
  );
}

function Sessions({ snap, filter }: { snap: Group[]; filter: Filter }) {
  const byProvider = useMemo(() => {
    const m: Record<string, Group[]> = {};
    for (const g of snap) (m[providerOf(g)] ||= []).push(g);
    return m;
  }, [snap]);

  if (snap.length === 0) return <EmptyState />;

  const order = [
    ...PROVIDER_ORDER.filter((p) => byProvider[p]),
    ...Object.keys(byProvider).filter((p) => !PROVIDER_ORDER.includes(p)),
  ].filter((p) => filter === "all" || p === filter);

  return (
    <>
      {order.map((p) => (
        <Section key={p} provider={p} groups={byProvider[p]} />
      ))}
      <div className="border-t border-border px-4 py-2.5 font-mono text-[10px] text-muted-foreground/70">
        openagentpeek · {snap.length} live session{snap.length > 1 ? "s" : ""}
      </div>
    </>
  );
}

export default function App() {
  const [snap, setSnap] = useState<Group[]>([]);
  const [filter, setFilter] = useState<Filter>("all");
  const [view, setView] = useState<View>("sessions");
  const [settings, setSettingsState] = useState<Settings>(DEFAULT_SETTINGS);

  useEffect(() => {
    // Backend pushes a fresh snapshot every ~1s (see lib.rs `run_loop`).
    const unlisten = listen<Group[]>("sessions", (e) => setSnap(e.payload));
    getSettings()
      .then(setSettingsState)
      .catch(() => setSettingsState(DEFAULT_SETTINGS));
    return () => {
      unlisten.then((off) => off());
    };
  }, []);

  // Write straight through to the backend (the source of truth) on every edit.
  const updateSettings = (next: Settings) => {
    setSettingsState(next);
    setSettings(next).catch(console.error);
  };

  const needsByProvider = useMemo(() => {
    const n: Record<string, number> = { all: 0 };
    for (const g of snap) {
      if (g.status !== "waiting") continue;
      n.all += 1;
      n[providerOf(g)] = (n[providerOf(g)] || 0) + 1;
    }
    return n;
  }, [snap]);

  return (
    // Outer column: a bubble arrow pointing up at the tray icon, then the popover card.
    <div className="flex h-full w-full flex-col items-center">
      <div className="tray-arrow" />
      <div className="flex min-h-0 w-full flex-1 overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground backdrop-blur-xl">
        <SideNav
          view={view}
          filter={filter}
          needsByProvider={needsByProvider}
          onSelectFilter={(f) => {
            setFilter(f);
            setView("sessions");
          }}
          onOpenSettings={() => setView("settings")}
        />

        <div className="scrollbar-none flex-1 overflow-y-auto">
          {view === "settings" ? (
            <SettingsView settings={settings} onChange={updateSettings} />
          ) : (
            <Sessions snap={snap} filter={filter} />
          )}
        </div>
      </div>
    </div>
  );
}
