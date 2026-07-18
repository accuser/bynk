use std::path::Path;

fn main() {
    let src = Path::new("src");

    let mut build = cc::Build::new();
    build.include(src);
    build.flag_if_supported("-std=c11");
    build.warnings(false);

    let parser = src.join("parser.c");
    build.file(&parser);
    println!("cargo:rerun-if-changed={}", parser.to_str().unwrap());

    // The grammar declares an external scanner (`doc_block`); it must be
    // compiled and linked alongside the generated parser.
    let scanner = src.join("scanner.c");
    build.file(&scanner);
    println!("cargo:rerun-if-changed={}", scanner.to_str().unwrap());

    build.compile("tree-sitter-bynk");
}
