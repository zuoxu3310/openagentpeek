mod codex_transcript;
mod context_window;
mod jump;
mod panel;
mod project;
mod providers;
mod sessions;
mod settings;
mod transcript;
mod tray;
mod watcher;

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartManagerExt};
use tauri_plugin_log::{Target, TargetKind};

use sessions::SessionManager;
use settings::Settings;
use transcript::{Status, Thresholds};

// How often we recompute the snapshot and push it to the frontend / tray. Mirrors the
// Electron prototype's 1s IPC loop.
const TICK: Duration = Duration::from_secs(1);

type SharedSettings = Arc<Mutex<Settings>>;

/// Whether a group belongs to a provider the user still wants watched.
fn enabled(group_id: &str, s: &Settings) -> bool {
    (s.watch_claude || !group_id.starts_with("claude:"))
        && (s.watch_codex || !group_id.starts_with("codex:"))
}

/// The backend's heart: drain parsed records into the manager, and once per TICK prune
/// stale sessions, snapshot, filter to the enabled providers, emit, and refresh the
/// tray — all honoring the current settings (read fresh each tick, so edits land live).
fn run_loop(app: AppHandle, rx: mpsc::Receiver<watcher::Ingest>, settings: SharedSettings) {
    let mut manager = SessionManager::new();
    let mut last_tick = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok((prov, fp, record, ts)) => {
                manager.ingest(&fp, &record, ts, prov);
                while let Ok((prov, fp, record, ts)) = rx.try_recv() {
                    manager.ingest(&fp, &record, ts, prov);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if last_tick.elapsed() >= TICK {
            let s = settings.lock().unwrap().clone();
            let th = Thresholds {
                idle_ms: s.idle_secs as i64 * 1000,
                waiting_decay_ms: s.waiting_decay_secs as i64 * 1000,
                stale_ms: s.stale_mins as i64 * 60 * 1000,
            };

            let now = watcher::now_ms();
            manager.prune(now, th.stale_ms);
            let mut snap = manager.snapshot(now, &th);
            snap.retain(|g| enabled(&g.id, &s));

            let needs = snap.iter().filter(|g| g.status == Status::Waiting).count();
            let working = snap
                .iter()
                .filter(|g| matches!(g.status, Status::Typing | Status::Reading))
                .count();

            let _ = app.emit("sessions", &snap);
            tray::update(&app, needs, working, &s);
            last_tick = Instant::now();
        }
    }
}

#[tauri::command]
fn get_settings(state: State<'_, SharedSettings>) -> Settings {
    state.lock().unwrap().clone()
}

/// Bring the clicked session's host window to the front. `provider` is the group-id
/// prefix; `cwd` is the session's working directory (used to find the live process).
#[tauri::command]
fn jump_to_session(provider: String, cwd: Option<String>) -> Result<(), String> {
    jump::jump(&provider, cwd.as_deref())
}

#[tauri::command]
fn set_settings(app: AppHandle, state: State<'_, SharedSettings>, settings: Settings) {
    apply_autostart(&app, settings.launch_at_login);
    settings::save(&settings);
    *state.lock().unwrap() = settings;
}

/// Reflect the launch-at-login preference into the OS (best-effort).
fn apply_autostart(app: &AppHandle, enable: bool) {
    let mgr = app.autolaunch();
    let _ = if enable { mgr.enable() } else { mgr.disable() };
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // Log to stdout (visible in `tauri dev`) AND to a rotating file in the app log
        // dir, so the packaged .app — which has no terminal — is still debuggable.
        // Info level keeps the per-second snapshot debug line out of the file.
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(Target::new(TargetKind::Stdout))
                .target(Target::new(TargetKind::LogDir { file_name: None }))
                .max_file_size(10_000_000) // 10 MB, then rotate
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_nspanel::init())
        .invoke_handler(tauri::generate_handler![get_settings, set_settings, jump_to_session])
        .setup(|app| {
            // Menubar app: no dock icon, no app-switcher entry.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            panel::init(&handle)?;
            tray::create(&handle)?;

            // Load settings, share them with the loop, and sync autostart to match.
            let settings: SharedSettings = Arc::new(Mutex::new(settings::load()));
            apply_autostart(&handle, settings.lock().unwrap().launch_at_login);
            app.manage(settings.clone());

            // Start the file watchers and the snapshot loop. The watcher feeds parsed
            // records over a channel; the loop owns the SessionManager (no shared state).
            let (tx, rx) = mpsc::channel::<watcher::Ingest>();
            watcher::spawn(tx);
            let loop_handle = app.handle().clone();
            std::thread::spawn(move || run_loop(loop_handle, rx, settings));

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
