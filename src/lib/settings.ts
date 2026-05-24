import { invoke } from "@tauri-apps/api/core";

// Mirrors `Settings` in `src-tauri/src/settings.rs` (serde camelCase). The backend is
// the source of truth; this is just the editing surface.
export interface Settings {
  watchClaude: boolean;
  watchCodex: boolean;
  waitingDecaySecs: number;
  idleSecs: number;
  staleMins: number;
  trayShowCount: boolean;
  trayRecolor: boolean;
  trayAlertOnly: boolean;
  launchAtLogin: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  watchClaude: true,
  watchCodex: true,
  waitingDecaySecs: 90,
  idleSecs: 6,
  staleMins: 30,
  trayShowCount: true,
  trayRecolor: true,
  trayAlertOnly: false,
  launchAtLogin: false,
};

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function setSettings(settings: Settings): Promise<void> {
  return invoke("set_settings", { settings });
}
