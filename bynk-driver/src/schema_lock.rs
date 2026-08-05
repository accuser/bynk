//! Reading and atomically writing `bynk.schema.lock` — the driver's side of
//! `CompileOptions::schema_registry` (#1078, R2.3/T0.7 residue).
//!
//! Ported verbatim from what used to be `bynk-emit/src/project/schema_registry.rs`'s
//! `read`/`write` — same crash-safety discipline (temp file + `sync_all` +
//! rename + best-effort directory fsync), same unchanged-content no-op. Only
//! the location moved: `bynk-emit` now only ever sees pre-read content (via
//! `SchemaLock::On`) and hands back serialized content to write (via
//! `ProjectOutput::schema_lock`) — it touches no disk for this file, same as
//! it no longer does for `.bynk` sources on the real CLI path (#1077/#1081).
//!
//! `bynk-emit`'s own `schema_registry::parse`/`serialize` are the pure
//! counterparts this module's [`read`]/[`write()`] wrap disk I/O around.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const LOCK_FILE: &str = "bynk.schema.lock";

/// Where the registry lives for a given project root — exposed so callers
/// reporting a write failure can name the actual file, not just the root.
pub fn lock_path(project_root: &Path) -> PathBuf {
    project_root.join(LOCK_FILE)
}

/// Read `bynk.schema.lock`'s raw content from `project_root`, if it exists.
/// `Ok(None)` means **verified absent** — no such file — which is the only
/// thing `CompileOptions`' `SchemaLock::On { existing: None }` is allowed to
/// mean (see that type's own doc). A present-but-unreadable file is a real
/// `io::Error`, not folded into `None` — silently treating a permission
/// error as "fresh project" would risk re-baselining a real registry's
/// history exactly as wrongly as never reading it at all.
pub fn read(project_root: &Path) -> io::Result<Option<String>> {
    let path = lock_path(project_root);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(std::fs::read_to_string(&path)?))
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `bynk.schema.lock` to `project_root`, atomically (temp file +
/// `sync_all` + rename + best-effort directory fsync, so a crash mid-write
/// can only leave the intact old file or the intact new one — `ledger.rs`'s
/// own discipline). A no-op when `content` is byte-identical to what is
/// already on disk, so a clean rebuild never touches the file's mtime or
/// produces a spurious `git diff`.
pub fn write(project_root: &Path, content: &str) -> io::Result<()> {
    let path = lock_path(project_root);
    if let Ok(existing) = std::fs::read_to_string(&path)
        && existing == content
    {
        return Ok(());
    }

    let dir = project_root;
    let (tmp, mut file) = loop {
        let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = dir.join(format!(".{LOCK_FILE}.{}.{n}.tmp", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(f) => break (candidate, f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };

    let write_then_sync = file
        .write_all(content.as_bytes())
        .and_then(|()| file.sync_all());
    if let Err(e) = write_then_sync {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    drop(file);
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Ok(dir_handle) = std::fs::File::open(dir) {
        let _ = dir_handle.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway on-disk directory, removed on drop (including on panic) —
    /// mirrors this crate's other test modules' own `Scratch`.
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch_dir(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "bynk_1078_schema_lock_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    #[test]
    fn read_of_an_absent_file_is_verified_absent() {
        let dir = scratch_dir("absent");
        assert_eq!(read(&dir.0).unwrap(), None);
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = scratch_dir("roundtrip");
        let content = "version = 1\n";
        write(&dir.0, content).unwrap();
        assert_eq!(read(&dir.0).unwrap().as_deref(), Some(content));
    }

    #[test]
    fn write_is_a_no_op_when_content_is_unchanged() {
        let dir = scratch_dir("noop-write");
        let content = "version = 1\n";
        write(&dir.0, content).unwrap();
        let mtime_before = std::fs::metadata(lock_path(&dir.0))
            .unwrap()
            .modified()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        write(&dir.0, content).unwrap();
        let mtime_after = std::fs::metadata(lock_path(&dir.0))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(
            mtime_before, mtime_after,
            "unchanged content must not rewrite the file"
        );
    }
}
