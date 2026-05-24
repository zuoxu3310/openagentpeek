// User preferences. The source of truth lives here in Rust (the backend honors them
// live); the React settings panel just reads/writes via the get_settings/set_settings
// commands. Persisted as JSON in the app config dir. The app runs fine with none of
// these touched — they're preferences, not required config.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    // Which agents to watch (both are always tailed; this filters what's shown/counted).
    pub watch_claude: bool,
    pub watch_codex: bool,
    // Attention heuristics, in human units (converted to ms thresholds in the loop).
    pub waiting_decay_secs: u64, // how long "needs you" stays loud before it decays
    pub idle_secs: u64,          // silence before an active session reads as idle
    pub stale_mins: u64,         // silence before a session drops off the list
    // Tray appearance.
    pub tray_show_count: bool, // show the needs-you / working count next to the icon
    pub tray_recolor: bool,    // recolor the dot (amber/green) vs a plain tinted dot
    pub tray_alert_only: bool, // only light up for needs-you, ignore "working"
    // Start at login.
    pub launch_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            watch_claude: true,
            watch_codex: true,
            waiting_decay_secs: 90,
            idle_secs: 6,
            stale_mins: 30,
            tray_show_count: true,
            tray_recolor: true,
            tray_alert_only: false,
            launch_at_login: false,
        }
    }
}

fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("openagentpeek").join("settings.json"))
}

/// Load persisted settings, falling back to defaults on any error or missing fields.
pub fn load() -> Settings {
    let Some(path) = settings_path() else {
        return Settings::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

/// Persist settings to the config dir (best-effort).
pub fn save(settings: &Settings) {
    let Some(path) = settings_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(&path, json);
    }
}
