//! #1883: `asset` declarations through the real CLI.
//!
//! - A single file checks clean (`bynkc check page.bynk`), but its build is
//!   refused with `bynk.asset.needs_project`: it has no project root and
//!   emits no Worker (review of #1903 for the check half).
//! - A project checks clean when the file is in `assets/`, and reports
//!   `bynk.asset.missing_file` when it is not.
//! - `bynkc compile --target workers` writes the hashed file and `_headers`
//!   into the Worker's assets directory (slice 1b), read from the project's
//!   `assets/` folder by the driver.
//!
//! Each case works in its own directory under `CARGO_TARGET_TMPDIR`: the CLI
//! may write a schema lock beside its input.

use std::path::{Path, PathBuf};
use std::process::Command;

const PAGE: &str = "commons compat.page\n\nasset style = \"assets/board.css\"\n\nfn sheet() -> String {\n  style.url\n}\n";
// The context declares an asset of its own too (review of #1904): the
// Worker serves both its own and its used commons' files.
const BOARD: &str = "context compat.board\n\nuses compat.page\n\nasset icon = \"assets/icon.svg\"\n\nservice web from http {\n  on GET(\"/\") () -> Effect[HttpResult[String]] by Visitor {\n    Ok(\"\\(sheet())\\(icon.url)\")\n  }\n}\n";
const SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>";
const CSS: &[u8] = b"body{}\n";

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn project(name: &str, with_file: bool) -> PathBuf {
    let dir = scratch(name);
    std::fs::create_dir_all(dir.join("src/compat")).unwrap();
    std::fs::write(dir.join("src/compat/page.bynk"), PAGE).unwrap();
    std::fs::write(dir.join("src/compat/board.bynk"), BOARD).unwrap();
    if with_file {
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/board.css"), CSS).unwrap();
        std::fs::write(dir.join("assets/icon.svg"), SVG).unwrap();
    }
    dir
}

fn bynkc(args: &[&str], path: &Path) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_bynkc"))
        .args(args)
        .arg(path)
        .output()
        .expect("run bynkc");
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), s)
}

#[test]
fn a_single_file_with_an_asset_checks_clean_and_its_build_is_refused() {
    let dir = scratch("asset-check-single");
    let file = dir.join("page.bynk");
    std::fs::write(&file, PAGE).unwrap();

    let (ok, out) = bynkc(&["check"], &file);
    assert!(ok, "`bynkc check` must pass, got:\n{out}");

    let ts = dir.join("page.ts");
    let (ok, out) = bynkc(&["compile", "--output", ts.to_str().unwrap()], &file);
    assert!(!ok, "`bynkc compile` must refuse the asset, got:\n{out}");
    assert!(out.contains("bynk.asset.needs_project"), "{out}");
}

#[test]
fn a_project_checks_clean_with_its_file_and_reports_a_missing_one() {
    let (ok, out) = bynkc(&["check"], &project("asset-check-present", true));
    assert!(ok, "`bynkc check` on a project must pass, got:\n{out}");

    let (ok, out) = bynkc(&["check"], &project("asset-check-absent", false));
    assert!(!ok, "a missing asset file must fail the check, got:\n{out}");
    assert!(out.contains("bynk.asset.missing_file"), "{out}");
}

#[test]
fn a_workers_build_writes_the_hashed_file_and_headers() {
    let dir = project("asset-build-workers", true);
    let out_dir = dir.join("out");
    let (ok, out) = bynkc(
        &[
            "compile",
            "--target",
            "workers",
            "--output",
            out_dir.to_str().unwrap(),
        ],
        &dir,
    );
    assert!(ok, "the Workers build must pass, got:\n{out}");

    let assets = out_dir.join("workers/compat-board/assets");
    let mut served: Vec<PathBuf> = std::fs::read_dir(assets.join("_assets"))
        .expect("the Worker's assets directory")
        .map(|e| e.unwrap().path())
        .collect();
    served.sort();
    assert_eq!(
        served.len(),
        2,
        "the commons' file and the context's own: {served:?}"
    );
    let name = served[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        name.starts_with("board.") && name.ends_with(".css"),
        "hashed name: {name}"
    );
    assert_eq!(std::fs::read(&served[0]).unwrap(), CSS);
    assert_eq!(
        std::fs::read(&served[1]).unwrap(),
        SVG,
        "the context's own asset"
    );
    assert!(
        std::fs::read_to_string(assets.join("_headers"))
            .unwrap()
            .contains("immutable")
    );
    let page = std::fs::read_to_string(out_dir.join("compat/page.ts")).unwrap();
    assert!(
        page.contains(&format!("\"/_assets/{name}\"")),
        "the `.url` constant names the written file:\n{page}"
    );
    let wrangler =
        std::fs::read_to_string(out_dir.join("workers/compat-board/wrangler.toml")).unwrap();
    assert!(wrangler.contains("[assets]"), "{wrangler}");
}

/// The positive fixture `1883_asset_workers`, compiled in-process: what the
/// e2e golden files cannot show (they compare documents, never `assets`).
fn compile_fixture(target: bynkc::BuildTarget) -> bynkc::ProjectOutput {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/positive/1883_asset_workers");
    let paths = bynkc::try_read_project_paths(&root).expect("well-formed fixture manifest");
    bynkc::compile_project(&bynk_testkit::compile_options_split(root, paths).target(target))
        .unwrap_or_else(|f| panic!("compile failed: {} error(s)", f.errors.len()))
}

#[test]
fn a_workers_build_ships_the_transitive_closure_reproducibly() {
    let first = compile_fixture(bynkc::BuildTarget::Workers);
    let keys: Vec<String> = first
        .artefacts
        .assets
        .keys()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    // `compat.board` uses `compat.page`, which uses `compat.theme`: both files
    // ship; `compat.api` uses neither and ships nothing.
    assert_eq!(keys.len(), 3, "{keys:?}");
    assert!(
        keys.iter()
            .all(|k| k.starts_with("workers/compat-board/assets/")),
        "{keys:?}"
    );
    assert!(keys.iter().any(|k| k.ends_with("/_headers")), "{keys:?}");
    assert!(
        keys.iter()
            .any(|k| k.contains("/_assets/board.") && k.ends_with(".css")),
        "{keys:?}"
    );
    assert!(
        keys.iter()
            .any(|k| k.contains("/_assets/palette.") && k.ends_with(".css")),
        "{keys:?}"
    );

    let second = compile_fixture(bynkc::BuildTarget::Workers);
    assert_eq!(
        first.artefacts.assets, second.artefacts.assets,
        "reproducible"
    );
}

#[test]
fn a_bundle_build_ships_no_files() {
    let out = compile_fixture(bynkc::BuildTarget::Bundle);
    assert!(
        out.artefacts.assets.is_empty(),
        "only a Worker serves files"
    );
}

/// Review of #1904: a deployable Bundle build ships no files, so it warns
/// about each asset whose `.url` it emits; `bynkc test`'s Bundle build (with
/// tests) does not.
#[test]
fn a_deployable_bundle_build_warns_that_its_assets_are_not_served() {
    let dir = project("asset-build-bundle", true);
    let (ok, out) = bynkc(
        &["compile", "--output", dir.join("out").to_str().unwrap()],
        &dir,
    );
    assert!(ok, "a Bundle build still succeeds, got:\n{out}");
    assert!(out.contains("bynk.asset.not_served"), "{out}");

    let quiet = compile_fixture(bynkc::BuildTarget::Bundle);
    assert!(
        !quiet
            .warnings
            .iter()
            .any(|w| w.error.category == "bynk.asset.not_served"),
        "a build with tests (as `bynkc test` compiles) does not warn"
    );
}
