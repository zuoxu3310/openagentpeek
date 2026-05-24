// Jump to a session's host window: given the clicked group's provider + cwd, find the
// live agent process and bring its window to the front — the "needs you → there in one
// click" payoff that turns the tray dot from a status light into an action.
//
// Read-only by design: we only *observe* processes (`ps`/`lsof`) and drive each host
// through its own scripting (Terminal AppleScript, the `code` CLI, app activation). We
// never write into agent config or install a hook. The trade-off is identity: a session
// is matched to a process by working directory, so two Claude sessions in the *same*
// directory can't be told apart — we focus the first match. A per-session "name tag"
// hook would disambiguate, but that's deliberately out of scope to stay hook-free.
//
// Host adapters, dispatched by how the session is hosted:
//   - Codex                 → activate the Codex desktop app (whole-app focus is enough).
//   - Claude inside VSCode   → focus the folder's window via the `code` CLI.
//   - Claude inside a terminal → select the Terminal.app tab whose tty matches.

use std::path::Path;
use std::process::Command;

const CODEX_BUNDLE_ID: &str = "com.openai.codex";

/// Bring the given session's window to the front. `provider` is the group-id prefix
/// ("claude" / "codex"); `cwd` is the session's working directory (None until a record
/// with a cwd has arrived).
pub fn jump(provider: &str, cwd: Option<&str>) -> Result<(), String> {
    match provider {
        "codex" => activate_app(CODEX_BUNDLE_ID),
        "claude" => jump_claude(cwd.ok_or("session has no working directory yet")?),
        other => Err(format!("don't know how to jump to provider '{other}'")),
    }
}

/// Locate the live Claude process for `cwd`, then route to the right host adapter.
fn jump_claude(cwd: &str) -> Result<(), String> {
    let pid = find_claude_pid(cwd)
        .ok_or_else(|| format!("no running Claude session found in {cwd}"))?;
    let chain = ancestor_commands(pid);
    let in_vscode = chain
        .iter()
        .any(|c| c.contains("Visual Studio Code") || c.contains("Code Helper"));
    if in_vscode {
        focus_vscode(cwd)
    } else if let Some(tty) = controlling_tty(pid) {
        focus_terminal_tab(&tty)
    } else {
        Err("could not find the session's terminal".into())
    }
}

// ── process observation ──────────────────────────────────────────────────────

/// First running `claude` whose working directory equals `cwd`.
fn find_claude_pid(cwd: &str) -> Option<u32> {
    let out = Command::new("pgrep").args(["-x", "claude"]).output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .filter_map(|s| s.parse::<u32>().ok())
        .find(|&pid| process_cwd(pid).as_deref() == Some(cwd))
}

/// A process's working directory, via `lsof` (the `n` line of the cwd fd).
fn process_cwd(pid: u32) -> Option<String> {
    let out = Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix('n').map(str::to_string))
}

/// Command of `pid` and each ancestor, walking up toward launchd. Used to tell a
/// VSCode-hosted session (ancestry runs through "Code Helper") from a terminal one.
fn ancestor_commands(pid: u32) -> Vec<String> {
    let mut cmds = Vec::new();
    let mut p = pid;
    for _ in 0..10 {
        match ps(p, "ppid=,comm=") {
            Some(line) => {
                let line = line.trim();
                let Some((ppid, comm)) = line.split_once(char::is_whitespace) else { break };
                cmds.push(comm.trim().to_string());
                let Ok(ppid) = ppid.trim().parse::<u32>() else { break };
                if p == 1 {
                    break;
                }
                p = ppid;
            }
            None => break,
        }
    }
    cmds
}

/// First real controlling terminal walking up from `pid`, e.g. "/dev/ttys002". A hook
/// (and `tty` in a piped child) can't see this directly, but the parent `claude` process
/// owns the tty, so a short walk recovers it.
fn controlling_tty(pid: u32) -> Option<String> {
    let mut p = pid;
    for _ in 0..10 {
        let line = ps(p, "ppid=,tty=")?;
        let mut it = line.split_whitespace();
        let ppid: u32 = it.next()?.parse().ok()?;
        let tty = it.next().unwrap_or("");
        if tty.starts_with("ttys") {
            return Some(format!("/dev/{tty}"));
        }
        if p == 1 {
            break;
        }
        p = ppid;
    }
    None
}

/// One `ps -o <fields> -p <pid>` line (header suppressed by the trailing `=`).
fn ps(pid: u32, fields: &str) -> Option<String> {
    let out = Command::new("ps")
        .args(["-o", fields, "-p", &pid.to_string()])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.trim();
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

// ── host adapters ────────────────────────────────────────────────────────────

fn activate_app(bundle_id: &str) -> Result<(), String> {
    osascript(&format!(r#"tell application id "{bundle_id}" to activate"#)).map(|_| ())
}

/// Focus the VSCode window already showing `cwd` (the live session's window) and raise
/// VSCode. Plain `code <folder>` focuses an open folder's window; we avoid `-r`, which
/// would reuse/replace some other window. AppleScript activation of VSCode is unreliable
/// (Apple-event error -609), so the `code` CLI is the path.
fn focus_vscode(cwd: &str) -> Result<(), String> {
    let bin = vscode_cli().ok_or("VSCode `code` CLI not found on PATH")?;
    let status = Command::new(bin).arg(cwd).status().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("`code` exited with an error".into())
    }
}

fn vscode_cli() -> Option<String> {
    const BUNDLED: &str = "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code";
    if Command::new("sh")
        .args(["-c", "command -v code"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        Some("code".into())
    } else if Path::new(BUNDLED).exists() {
        Some(BUNDLED.into())
    } else {
        None
    }
}

/// Select the Terminal.app tab whose tty matches and raise its window. Only Terminal.app
/// is handled in v1 (it's the macOS default); iTerm/Ghostty/etc. would each need their
/// own adapter.
fn focus_terminal_tab(tty: &str) -> Result<(), String> {
    let script = format!(
        r#"set targetTTY to "{tty}"
tell application "Terminal"
  repeat with w in windows
    repeat with t in tabs of w
      try
        if (tty of t) is targetTTY then
          set selected of t to true
          set frontmost of w to true
          activate
          return "ok"
        end if
      end try
    end repeat
  end repeat
end tell
return "notfound""#
    );
    if osascript(&script)?.trim() == "ok" {
        Ok(())
    } else {
        Err(format!("no Terminal tab is on {tty}"))
    }
}

/// Run an AppleScript snippet, returning its stdout (trimmed of nothing — caller trims).
fn osascript(script: &str) -> Result<String, String> {
    let out = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}
