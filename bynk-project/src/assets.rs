//! #1883 (static assets), slice 1b: the project's assets folder — the one
//! directory an `asset name = "path"` declaration may name a file in — and
//! the walk that reads it.
//!
//! Assets are public by construction, so they come only from
//! `<project root>/assets/` (decided on #1883 after the review of #1903): a
//! declaration can never publish `.env` or a `.bynk` source by mistake. The
//! checker states the rule (`bynk.asset.outside_assets_dir`); the driver and
//! the test harnesses pre-read the folder with [`read_asset_files`], so
//! `bynk-emit` and `bynk-check` stay disk-free, and the LSP lists it with
//! [`list_asset_files`] for the existence check.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

/// The project's assets folder, relative to the project root.
pub const PROJECT_ASSETS_DIR: &str = "assets";

/// `path` as the key [`read_asset_files`] files it under: separators made
/// `/`, and empty and `.` segments dropped, so `./assets//board.css` and
/// `assets\board.css` both become `assets/board.css`. A `..` segment is kept
/// (the checker rejects it separately, `bynk.asset.unsafe_path`).
pub fn normalise_asset_path(path: &str) -> String {
    path.split(['/', '\\'])
        .filter(|seg| !seg.is_empty() && *seg != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// Whether the normalised `path` names a file inside the assets folder —
/// `assets/<at least one more segment>`.
pub fn is_in_assets_dir(path: &str) -> bool {
    let normalised = normalise_asset_path(path);
    let mut segs = normalised.split('/');
    segs.next() == Some(PROJECT_ASSETS_DIR) && segs.next().is_some()
}

/// Every regular file under `<project_root>/assets/`, keyed by its
/// normalised project-relative path (`assets/board.css`), with its bytes.
/// Symbolic links are not followed, so a link cannot pull a file in from
/// outside the folder; a missing folder is an empty map.
pub fn read_asset_files(project_root: &Path) -> io::Result<BTreeMap<String, Vec<u8>>> {
    let mut out = BTreeMap::new();
    walk(project_root, &mut |rel, path| {
        out.insert(rel, std::fs::read(path)?);
        Ok(())
    })?;
    Ok(out)
}

/// [`read_asset_files`]' keys alone, without reading any bytes — what the
/// editor needs to check that a declared file exists.
pub fn list_asset_files(project_root: &Path) -> io::Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    walk(project_root, &mut |rel, _| {
        out.insert(rel);
        Ok(())
    })?;
    Ok(out)
}

fn walk(
    project_root: &Path,
    visit: &mut dyn FnMut(String, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let dir = project_root.join(PROJECT_ASSETS_DIR);
    if !dir.is_dir() {
        return Ok(());
    }
    walk_dir(&dir, PROJECT_ASSETS_DIR, visit)
}

fn walk_dir(
    dir: &Path,
    rel: &str,
    visit: &mut dyn FnMut(String, &Path) -> io::Result<()>,
) -> io::Result<()> {
    let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        // `DirEntry::file_type` does not follow a symlink, so a link is
        // neither a file nor a directory here and is skipped.
        let ty = entry.file_type()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let child = format!("{rel}/{name}");
        if ty.is_dir() {
            walk_dir(&entry.path(), &child, visit)?;
        } else if ty.is_file() {
            visit(child, &entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_and_folder_membership() {
        assert_eq!(
            normalise_asset_path("./assets//board.css"),
            "assets/board.css"
        );
        assert_eq!(
            normalise_asset_path("assets\\img\\a.png"),
            "assets/img/a.png"
        );
        assert!(is_in_assets_dir("assets/board.css"));
        assert!(is_in_assets_dir("./assets/.well-known/security.txt"));
        assert!(!is_in_assets_dir("assets"));
        assert!(!is_in_assets_dir("assets/"));
        assert!(!is_in_assets_dir(".env"));
        assert!(!is_in_assets_dir("src/app/page.bynk"));
        assert!(!is_in_assets_dir("public/assets/x.css"));
    }

    #[test]
    fn the_walk_reads_regular_files_and_skips_links() {
        let root = std::env::temp_dir().join(format!("bynk-project-assets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("assets/img")).unwrap();
        std::fs::write(root.join("assets/board.css"), b"body{}").unwrap();
        std::fs::write(root.join("assets/img/a.png"), [0x89, 0x00]).unwrap();
        std::fs::write(root.join("secret.txt"), b"no").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("secret.txt"), root.join("assets/leak.txt")).unwrap();

        let files = read_asset_files(&root).unwrap();
        assert_eq!(
            files.keys().cloned().collect::<Vec<_>>(),
            vec!["assets/board.css", "assets/img/a.png"]
        );
        assert_eq!(files["assets/img/a.png"], vec![0x89, 0x00]);
        assert_eq!(
            list_asset_files(&root).unwrap(),
            files.keys().cloned().collect()
        );
        assert!(read_asset_files(&root.join("nowhere")).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
