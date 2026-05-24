// Menubar tray icon: left-click toggles the panel, right-click opens a small menu.
// Adapted from openusage's tray.rs (trimmed to Open / Quit).
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;
use tauri_nspanel::ManagerExt;

use crate::panel::{get_or_init_panel, position_panel_at_tray_icon, show_panel};
use crate::settings::Settings;

// The traffic-light palette — one scheme shared with the popover (DECISIONS · D14): these
// RGBs equal the popover's --amber / --blue-500 / --green-500. Left → right the tray reads
// waiting → working → idle.
const WAIT: [u8; 3] = [255, 194, 77]; // #ffc24d — waiting (matches popover --amber)
const WORK: [u8; 3] = [72, 150, 255]; // #4896ff — working (--blue-500)
const IDLE: [u8; 3] = [76, 215, 135]; // #4cd787 — idle (--green-500)
const HOUSING: [u8; 3] = [38, 38, 42]; // dark pill housing (the fill)
const BORDER: [u8; 3] = [140, 140, 150]; // 1px ring around the housing, so the pill stays visible against any menubar background
const OFF: [u8; 3] = [108, 108, 118]; // an unlit lamp — light gray, visible against the dark housing

/// Which lamp is lit. Exactly one lamp is on at any time (idle lights green).
#[derive(Clone, Copy)]
enum Lit {
    Waiting,
    Working,
    Idle,
}

/// Coverage (0..1) of a rounded-rectangle housing at (x, y), ~1px antialiased. Signed
/// distance to the box (negative inside, positive outside), turned into edge coverage.
fn housing_cov(x: f64, y: f64, x0: f64, y0: f64, x1: f64, y1: f64, r: f64) -> f64 {
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (hx, hy) = ((x1 - x0) / 2.0 - r, (y1 - y0) / 2.0 - r);
    let qx = (x - cx).abs() - hx;
    let qy = (y - cy).abs() - hy;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    let d = outside + inside - r;
    (0.5 - d).clamp(0.0, 1.0)
}

/// Coverage (0..1) of a circle of radius `r` at distance `d`, with a 1px feathered edge.
fn disc_cov(d: f64, r: f64) -> f64 {
    if d <= r {
        1.0
    } else if d < r + 1.0 {
        r + 1.0 - d
    } else {
        0.0
    }
}

// The tray icon: a horizontal traffic light — a rounded housing with three lamps in a row
// (yellow = waiting, blue = working, green = idle). The active lamp glows; the others sit
// dark, so the whole fixture reads at a glance. The canvas stays near-square (the menubar
// scales the icon to its height), with the fixture as a band through the middle so it
// doesn't stretch into a long bar.
fn traffic_light(active: Option<Lit>) -> Image<'static> {
    const W: u32 = 52;
    const H: u32 = 22;
    let (wait_on, work_on, idle_on) = match active {
        Some(Lit::Waiting) => (true, false, false),
        Some(Lit::Working) => (false, true, false),
        Some(Lit::Idle) => (false, false, true),
        None => (false, false, false),
    };
    // (center x, color) left → right: waiting, working, idle. Lamps sit at ~1/3 of housing
    // height with even breathing space — reads as a real traffic light, not a toggle.
    let lamps = [
        (13.0, if wait_on { WAIT } else { OFF }),
        (26.0, if work_on { WORK } else { OFF }),
        (39.0, if idle_on { IDLE } else { OFF }),
    ];
    let cy = 11.0;
    let lamp_r = 5.0;

    let mut buf = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 4) as usize;
            let (fx, fy) = (x as f64, y as f64);

            // Housing = full capsule (r = housing-height/2) with a 1px-inset inner shape;
            // the ring between them is painted in BORDER so the pill stays visible against
            // any menubar background (dark or light), not just the bright promo wallpaper.
            let outer = housing_cov(fx, fy, 2.0, 1.0, 50.0, 21.0, 10.0);
            let inner = housing_cov(fx, fy, 3.0, 2.0, 49.0, 20.0, 9.0);
            let edge_w = if outer > 0.001 { ((outer - inner).max(0.0)) / outer } else { 0.0 };
            let mut r = BORDER[0] as f64 * edge_w + HOUSING[0] as f64 * (1.0 - edge_w);
            let mut g = BORDER[1] as f64 * edge_w + HOUSING[1] as f64 * (1.0 - edge_w);
            let mut b = BORDER[2] as f64 * edge_w + HOUSING[2] as f64 * (1.0 - edge_w);
            let mut a = outer;

            for (lx, col) in lamps {
                let cov = disc_cov(((fx - lx).powi(2) + (fy - cy).powi(2)).sqrt(), lamp_r);
                if cov > 0.0 {
                    r = col[0] as f64 * cov + r * (1.0 - cov);
                    g = col[1] as f64 * cov + g * (1.0 - cov);
                    b = col[2] as f64 * cov + b * (1.0 - cov);
                    a = cov + a * (1.0 - cov);
                }
            }

            buf[i] = r.round() as u8;
            buf[i + 1] = g.round() as u8;
            buf[i + 2] = b.round() as u8;
            buf[i + 3] = (a * 255.0).round() as u8;
        }
    }
    Image::new_owned(buf, W, H)
}

/// The one question the menubar answers — does anything need me? — shown as the dot's
/// color (the traffic light) and a count, both honoring the user's tray prefs:
///   - recolor off  → always a tinted idle dot, only the number changes.
///   - alert_only   → green "working" is suppressed; only needs-you lights up.
///   - show_count off → no number.
pub fn update(app_handle: &AppHandle, needs: usize, working: usize, s: &Settings) {
    let Some(tray) = app_handle.tray_by_id("tray") else {
        return;
    };

    // Resolve which lamp lights: waiting (yellow) wins, then working (blue) unless
    // alert-only, else idle (green). Exactly one lamp is lit.
    let lit = if needs > 0 {
        Lit::Waiting
    } else if working > 0 && !s.tray_alert_only {
        Lit::Working
    } else {
        Lit::Idle
    };
    // The number is the total of everything not idle — waiting + working.
    let count = needs + working;
    // Recolor off → keep the fixture dark (no lamp lit); only the number changes.
    let active = if s.tray_recolor { Some(lit) } else { None };

    // The fixture is inherently colored, so it's never a template image. (`set_icon`
    // forces the template flag off anyway in tray-icon 0.23.)
    let _ = tray.set_icon(Some(traffic_light(active)));
    let _ = tray.set_icon_as_template(false);

    let title = if s.tray_show_count && count > 0 {
        Some(format!(" {count}"))
    } else {
        None
    };
    let _ = tray.set_title(title);
}

pub fn create(app_handle: &AppHandle) -> tauri::Result<()> {
    // Start as a dark traffic light; `update` lights a lamp as sessions change. (We draw
    // our own icon rather than resolving a Resource path, absent in `tauri dev`.)
    let icon = traffic_light(None);

    let open = MenuItem::with_id(app_handle, "open", "Open", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app_handle)?;
    let quit = MenuItem::with_id(app_handle, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app_handle, &[&open, &separator, &quit])?;

    TrayIconBuilder::with_id("tray")
        .icon(icon)
        .icon_as_template(false)
        .tooltip("OpenAgentPeek — Claude Code & Codex sessions")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app_handle, event| match event.id.as_ref() {
            "open" => show_panel(app_handle),
            "quit" => app_handle.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            let app_handle = tray.app_handle();
            if let TrayIconEvent::Click {
                button_state, rect, ..
            } = event
            {
                if button_state == MouseButtonState::Up {
                    let Some(panel) = get_or_init_panel!(app_handle) else {
                        return;
                    };
                    if panel.is_visible() {
                        panel.hide();
                        return;
                    }
                    // macOS quirk: show before positioning across monitors.
                    panel.show_and_make_key();
                    position_panel_at_tray_icon(app_handle, rect.position, rect.size);
                }
            }
        })
        .build(app_handle)?;

    Ok(())
}
