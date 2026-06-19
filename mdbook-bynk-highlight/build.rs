//! Compile the generated `tree-sitter-bynk` C parser (and its external scanner)
//! and link them into this preprocessor.

use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let src = Path::new(&manifest).join("../tree-sitter-bynk/src");

    cc::Build::new()
        .include(&src)
        .file(src.join("parser.c"))
        .file(src.join("scanner.c"))
        .flag_if_supported("-std=c11")
        .warnings(false)
        .compile("tree-sitter-bynk");

    println!("cargo:rerun-if-changed={}", src.join("parser.c").display());
    println!("cargo:rerun-if-changed={}", src.join("scanner.c").display());
}
