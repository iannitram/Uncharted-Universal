//! `include/uncharted_runtime.h` is what `cbindgen` makes of this crate now (the drift check CI
//! runs with `cargo test`). After changing an exported symbol, rewrite it with
//! `UNCHARTED_WRITE_HEADER=1 cargo test -p uncharted_runtime --test header` and commit it.

use std::path::Path;

#[test]
fn header_matches_the_exported_symbols() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = cbindgen::Config::from_file(crate_dir.join("cbindgen.toml")).unwrap();
    let mut generated = Vec::new();
    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_config(config)
        .generate()
        .expect("cbindgen")
        .write(&mut generated);
    let generated = String::from_utf8(generated).unwrap();
    let path = crate_dir.join("include/uncharted_runtime.h");
    if std::env::var_os("UNCHARTED_WRITE_HEADER").is_some_and(|value| value == "1") {
        std::fs::write(&path, &generated).unwrap();
    }
    let checked_in = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        checked_in == generated,
        "{} is out of date: run UNCHARTED_WRITE_HEADER=1 cargo test -p uncharted_runtime --test header",
        path.display()
    );
}
