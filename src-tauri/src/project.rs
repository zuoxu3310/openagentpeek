// Project identity from a session's cwd. Ported from `../mvp/src/project.js`.
//
// Deterministic and self-disambiguating, so labels never need manual patching:
//   primary  - the name you read first (git repo name, else folder name)
//   context  - a disambiguating second line (remote owner/repo, else parent path)

use std::path::Path;

/// Walk up from cwd to the nearest ancestor that is a git repo root.
fn git_root(cwd: &str) -> Option<String> {
    let mut dir = Path::new(cwd);
    for _ in 0..40 {
        if dir.join(".git").exists() {
            return Some(dir.to_string_lossy().into_owned());
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent,
            _ => return None,
        }
    }
    None
}

/// Best-effort owner/repo from a repo's local git config (no network, no subprocess).
fn remote_slug(root: &str) -> Option<String> {
    let cfg = std::fs::read_to_string(Path::new(root).join(".git").join("config")).ok()?;
    // First `url = …` line, mirroring the JS `/url\s*=\s*(.+)/`.
    let line = cfg
        .lines()
        .find_map(|l| l.trim_start().strip_prefix("url"))
        .and_then(|rest| rest.trim_start().strip_prefix('='))?;
    let url = line.trim().trim_end_matches(".git");
    // Take the last "owner/repo" pair, splitting on the final ':' or '/' boundary.
    let tail = url.rsplit(|c| c == ':' || c == '/').take(2).collect::<Vec<_>>();
    if tail.len() >= 2 && !tail[0].is_empty() && !tail[1].is_empty() {
        // rsplit yields reversed: tail[1] = owner, tail[0] = repo.
        Some(format!("{}/{}", tail[1], tail[0]))
    } else {
        None
    }
}

/// Path with $HOME collapsed to ~, for a compact unambiguous context line.
fn home_relative(p: &str) -> String {
    match dirs::home_dir() {
        Some(home) => {
            let home = home.to_string_lossy();
            if let Some(rest) = p.strip_prefix(home.as_ref()) {
                format!("~{rest}")
            } else {
                p.to_string()
            }
        }
        None => p.to_string(),
    }
}

/// Structured project identity for a session's cwd.
pub fn project_identity(cwd: &str) -> (String, String) {
    if cwd.is_empty() {
        return ("…".to_string(), String::new());
    }
    let root = git_root(cwd);
    let base = root.as_deref().unwrap_or(cwd);
    let base_path = Path::new(base);
    let primary = base_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| base.to_string());
    let context = root
        .as_deref()
        .and_then(remote_slug)
        .unwrap_or_else(|| {
            home_relative(&base_path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default())
        });
    (primary, context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    // A unique scratch dir under the system temp, cleaned by the caller.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("pp-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn primary_resolves_to_git_root_not_deep_subdir() {
        let root = scratch("proj");
        let repo = root.join("repo");
        let deep = repo.join("services").join("api");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let (primary, _) = project_identity(&deep.to_string_lossy());
        assert_eq!(primary, "repo");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn context_disambiguates_via_parent_path() {
        let root = scratch("ctx");
        let repo = root.join("WORKSPACE").join("mvp");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let (primary, context) = project_identity(&repo.to_string_lossy());
        assert_eq!(primary, "mvp");
        assert!(context.contains("WORKSPACE")); // parent path tells you which mvp
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn context_prefers_git_remote_owner_repo() {
        let root = scratch("rem");
        let repo = root.join("thing");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(
            repo.join(".git").join("config"),
            "[remote \"origin\"]\n\turl = git@github.com:acme/thing.git\n",
        )
        .unwrap();
        let (_, context) = project_identity(&repo.to_string_lossy());
        assert_eq!(context, "acme/thing");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn empty_cwd_yields_placeholder() {
        assert_eq!(project_identity(""), ("…".to_string(), String::new()));
    }
}
