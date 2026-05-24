// Filesystem tailer + `notify` integration. Ported from `../mvp/src/watcher.js`.
//
// `notify` only reports changes after watching starts (unlike chokidar's
// `ignoreInitial:false`), so we walk the tree once at startup to seed offsets and
// replay the tail of recently-active files — that way sessions already running before
// launch appear immediately, not just new writers.

use std::collections::HashMap;
use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::Value;

use crate::providers::{Provider, PROVIDERS};

/// One parsed record on its way to the manager: which agent, which file, the record,
/// and the timestamp to attribute it to.
pub type Ingest = (Provider, PathBuf, Value, i64);

// On startup, replay the tail of files touched within this window so sessions already
// running before launch show up immediately.
const STARTUP_RECENT_MS: i64 = 30 * 60 * 1000;
const TAIL_BYTES: u64 = 64 * 1024;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn mtime_ms(meta: &Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Read a byte range with the file always released, "" on any failure. Uses a relaxed
/// read (not read_exact) so a file that shrank between stat and read still yields what
/// it has rather than erroring out.
fn read_range(path: &Path, from: u64, to: u64) -> String {
    if to <= from {
        return String::new();
    }
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    if f.seek(SeekFrom::Start(from)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    if f.take(to - from).read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

struct Tailer {
    offsets: HashMap<PathBuf, u64>,   // bytes already consumed
    residual: HashMap<PathBuf, String>, // trailing partial line not yet terminated
}

impl Tailer {
    fn new() -> Self {
        Tailer {
            offsets: HashMap::new(),
            residual: HashMap::new(),
        }
    }

    fn emit_lines(
        &mut self,
        fp: &Path,
        text: &str,
        ts: i64,
        keep_residual: bool,
        prov: Provider,
        tx: &Sender<Ingest>,
    ) {
        let mut parts: Vec<&str> = text.split('\n').collect();
        // The final element has no trailing newline yet: it's an incomplete line.
        let residual = if keep_residual {
            parts.pop().unwrap_or("").to_string()
        } else {
            String::new()
        };
        self.residual.insert(fp.to_path_buf(), residual);
        for line in parts {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(record) = serde_json::from_str::<Value>(trimmed) {
                let _ = tx.send((prov, fp.to_path_buf(), record, ts));
            }
        }
    }

    fn seed_offset(&mut self, fp: &Path, prov: Provider, tx: &Sender<Ingest>) {
        let meta = match std::fs::metadata(fp) {
            Ok(m) => m,
            Err(_) => {
                self.offsets.insert(fp.to_path_buf(), 0);
                return;
            }
        };
        let size = meta.len();
        let mtime = mtime_ms(&meta);
        // Recently-active files: replay the tail to recover current state. Old files
        // are dead — just mark read.
        if now_ms() - mtime <= STARTUP_RECENT_MS && size > 0 {
            let start = size.saturating_sub(TAIL_BYTES);
            let mut text = read_range(fp, start, size);
            if start > 0 {
                // Drop the partial first line.
                text = match text.find('\n') {
                    Some(i) => text[i + 1..].to_string(),
                    None => String::new(),
                };
            }
            self.emit_lines(fp, &text, mtime, false, prov, tx);
        }
        self.offsets.insert(fp.to_path_buf(), size);
        self.residual.insert(fp.to_path_buf(), String::new());
    }

    fn read_new(&mut self, fp: &Path, prov: Provider, tx: &Sender<Ingest>) {
        let from = self.offsets.get(fp).copied().unwrap_or(0);
        let size = match std::fs::metadata(fp) {
            Ok(m) => m.len(),
            Err(_) => return,
        };
        if size < from {
            // Truncated/rotated: start over.
            self.offsets.insert(fp.to_path_buf(), 0);
            self.residual.insert(fp.to_path_buf(), String::new());
            return;
        }
        if size == from {
            return;
        }
        let residual = self.residual.get(fp).cloned().unwrap_or_default();
        let text = residual + &read_range(fp, from, size);
        self.offsets.insert(fp.to_path_buf(), size);
        // Keep the trailing partial line buffered until its newline arrives, so a
        // record read mid-write is parsed once, intact, rather than dropped.
        self.emit_lines(fp, &text, now_ms(), true, prov, tx);
    }

    /// First sighting of a file -> seed (replay recent tail); thereafter -> incremental.
    fn on_path(&mut self, fp: &Path, prov: Provider, tx: &Sender<Ingest>) {
        if fp.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            return;
        }
        if self.offsets.contains_key(fp) {
            self.read_new(fp, prov, tx);
        } else {
            self.seed_offset(fp, prov, tx);
        }
    }

    fn forget(&mut self, fp: &Path) {
        self.offsets.remove(fp);
        self.residual.remove(fp);
    }

    fn handle_event(&mut self, event: &Event, prov: Provider, tx: &Sender<Ingest>) {
        if event.kind.is_remove() {
            for path in &event.paths {
                self.forget(path);
            }
        } else if event.kind.is_create() || event.kind.is_modify() {
            for path in &event.paths {
                self.on_path(path, prov, tx);
            }
        }
    }

    /// Recursively seed every existing `.jsonl` under `dir`.
    fn walk_seed(&mut self, dir: &Path, prov: Provider, tx: &Sender<Ingest>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(ft) if ft.is_dir() => self.walk_seed(&path, prov, tx),
                Ok(_) if path.extension().and_then(|e| e.to_str()) == Some("jsonl") => {
                    self.seed_offset(&path, prov, tx);
                }
                _ => {}
            }
        }
    }
}

/// Spawn one watcher per provider. Records flow out over `tx`. The watchers are kept
/// alive on a parked thread for the life of the (resident menubar) process.
pub fn spawn(tx: Sender<Ingest>) {
    std::thread::spawn(move || {
        let mut keep: Vec<RecommendedWatcher> = Vec::new();
        for prov in PROVIDERS {
            let dir = prov.dir();
            let mut tailer = Tailer::new();
            // Seed existing files before handing the tailer to the notify callback.
            tailer.walk_seed(&dir, prov, &tx);

            let tx2 = tx.clone();
            let mut watcher = match notify::recommended_watcher(move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    tailer.handle_event(&event, prov, &tx2);
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    log::warn!("failed to create watcher for {}: {e}", prov.name());
                    continue;
                }
            };
            // The dir may not exist (e.g. Codex never used) — that's fine, skip it.
            if let Err(e) = watcher.watch(&dir, RecursiveMode::Recursive) {
                log::warn!("not watching {dir:?}: {e}");
                continue;
            }
            log::info!("watching {dir:?} for {} sessions", prov.name());
            keep.push(watcher);
        }
        // Keep the watchers alive; the callbacks run on notify's own threads.
        loop {
            std::thread::park();
        }
    });
}
