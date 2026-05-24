import { Activity, Settings as SettingsIcon } from "lucide-react";
import type { ComponentType } from "react";
import { cn } from "@/lib/utils";
import { ClaudeLogo, CodexLogo } from "@/components/brand-icons";

export type Filter = "all" | "claude" | "codex";
export type View = "sessions" | "settings";

type IconCmp = ComponentType<{ className?: string }>;

// `color` pins a brand tint on the icon (openusage colors provider logos by brand);
// "All" has none, so it follows the active/inactive text color instead.
const FILTERS: { id: Filter; label: string; Icon: IconCmp; color?: string }[] = [
  { id: "all", label: "All sessions", Icon: Activity },
  { id: "claude", label: "Claude Code", Icon: ClaudeLogo, color: "#d97757" },
  { id: "codex", label: "Codex", Icon: CodexLogo, color: "#10a37f" },
];

function NavButton({
  active,
  label,
  Icon,
  color,
  badge,
  onClick,
}: {
  active: boolean;
  label: string;
  Icon: IconCmp;
  color?: string;
  badge?: number;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        "relative flex w-full items-center justify-center p-2.5 transition-colors hover:bg-accent",
        active
          ? "text-foreground before:absolute before:left-0 before:top-1.5 before:bottom-1.5 before:w-[2px] before:rounded-full before:bg-page-accent"
          : "text-muted-foreground"
      )}
    >
      <span
        className={cn("transition-opacity", color && (active ? "opacity-100" : "opacity-55"))}
        style={color ? { color } : undefined}
      >
        <Icon className="size-5" />
      </span>
      {badge ? (
        <span className="absolute right-1.5 top-1.5 size-1.5 rounded-full bg-amber" />
      ) : null}
    </button>
  );
}

// Left icon rail, modeled on openusage's side-nav: provider filters up top, settings
// gear at the bottom. The dot on a tab flags a needs-you session in that bucket.
export function SideNav({
  view,
  filter,
  needsByProvider,
  onSelectFilter,
  onOpenSettings,
}: {
  view: View;
  filter: Filter;
  needsByProvider: Record<string, number>;
  onSelectFilter: (f: Filter) => void;
  onOpenSettings: () => void;
}) {
  return (
    <nav className="flex w-12 shrink-0 flex-col border-r border-border py-3">
      {FILTERS.map(({ id, label, Icon, color }) => (
        <NavButton
          key={id}
          active={view === "sessions" && filter === id}
          label={label}
          Icon={Icon}
          color={color}
          badge={id === "all" ? needsByProvider.all : needsByProvider[id]}
          onClick={() => onSelectFilter(id)}
        />
      ))}

      <div className="flex-1" />

      <NavButton
        active={view === "settings"}
        label="Settings"
        Icon={SettingsIcon}
        onClick={onOpenSettings}
      />
    </nav>
  );
}
