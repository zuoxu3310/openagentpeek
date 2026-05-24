import { clamp01 } from "@/lib/utils";

// Context-window meter, openusage's rounded-bar look: a muted track with a colored
// fill. Turns rust when the window is nearly full (>85%) — pressure, not status.
export function UsageBar({ ratio, color }: { ratio: number; color: string }) {
  const pct = Math.round(clamp01(ratio) * 100);
  return (
    <div className="h-2 w-full overflow-hidden rounded-full bg-white/8">
      <div
        className="h-full rounded-full transition-[width] duration-300 ease-out"
        style={{ width: `${pct}%`, background: ratio > 0.85 ? "var(--rust)" : color }}
      />
    </div>
  );
}
