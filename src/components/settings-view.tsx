import { Minus, Plus } from "lucide-react";
import type { ReactNode } from "react";
import type { Settings } from "@/lib/settings";
import { Switch } from "@/components/ui/switch";

function GroupTitle({ children }: { children: ReactNode }) {
  return (
    <div className="px-4 pt-4 pb-1 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
      {children}
    </div>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3 px-4 py-2">
      <div className="min-w-0">
        <div className="text-[13px] text-foreground">{label}</div>
        {hint && <div className="text-[11px] text-muted-foreground">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function Stepper({
  value,
  onChange,
  min,
  max,
  step,
  suffix,
}: {
  value: number;
  onChange: (value: number) => void;
  min: number;
  max: number;
  step: number;
  suffix: string;
}) {
  const set = (v: number) => onChange(Math.min(max, Math.max(min, v)));
  const btn =
    "flex size-6 items-center justify-center rounded-md bg-white/8 text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-30";
  return (
    <div className="flex items-center gap-1.5">
      <button type="button" className={btn} onClick={() => set(value - step)} disabled={value <= min}>
        <Minus className="size-3" />
      </button>
      <span className="w-11 text-center font-mono text-xs tabular-nums text-foreground">
        {value}
        {suffix}
      </span>
      <button type="button" className={btn} onClick={() => set(value + step)} disabled={value >= max}>
        <Plus className="size-3" />
      </button>
    </div>
  );
}

// The settings panel. Every change writes straight through to the backend (which is
// the source of truth), so edits land live.
export function SettingsView({
  settings,
  onChange,
}: {
  settings: Settings;
  onChange: (next: Settings) => void;
}) {
  const set = <K extends keyof Settings>(key: K, value: Settings[K]) =>
    onChange({ ...settings, [key]: value });

  return (
    <div className="pb-3">
      <GroupTitle>General</GroupTitle>
      <Row label="Launch at login" hint="Start when you log in to macOS">
        <Switch checked={settings.launchAtLogin} onChange={(v) => set("launchAtLogin", v)} />
      </Row>

      <GroupTitle>Watch</GroupTitle>
      <Row label="Claude Code">
        <Switch checked={settings.watchClaude} onChange={(v) => set("watchClaude", v)} />
      </Row>
      <Row label="Codex">
        <Switch checked={settings.watchCodex} onChange={(v) => set("watchCodex", v)} />
      </Row>

      <GroupTitle>Attention</GroupTitle>
      <Row label="“Needs you” fades after" hint="How long a finished turn stays loud">
        <Stepper
          value={settings.waitingDecaySecs}
          onChange={(v) => set("waitingDecaySecs", v)}
          min={10}
          max={600}
          step={10}
          suffix="s"
        />
      </Row>
      <Row label="Idle after" hint="Silence before a session reads as idle">
        <Stepper
          value={settings.idleSecs}
          onChange={(v) => set("idleSecs", v)}
          min={2}
          max={60}
          step={1}
          suffix="s"
        />
      </Row>
      <Row label="Drop after" hint="Silence before a session leaves the list">
        <Stepper
          value={settings.staleMins}
          onChange={(v) => set("staleMins", v)}
          min={5}
          max={240}
          step={5}
          suffix="m"
        />
      </Row>

      <GroupTitle>Tray</GroupTitle>
      <Row label="Show count">
        <Switch checked={settings.trayShowCount} onChange={(v) => set("trayShowCount", v)} />
      </Row>
      <Row label="Recolor icon" hint="Amber / green dot vs a plain tinted one">
        <Switch checked={settings.trayRecolor} onChange={(v) => set("trayRecolor", v)} />
      </Row>
      <Row label="Only alert for “needs you”" hint="Ignore the working state">
        <Switch checked={settings.trayAlertOnly} onChange={(v) => set("trayAlertOnly", v)} />
      </Row>
    </div>
  );
}
