//! Regenerates the typed TS bindings at `src/shared/bindings/commands.ts`.
//!
//! Runs as part of `cargo test`; CI exercises this so the committed
//! bindings file is guaranteed to be up-to-date with the Rust signatures.

#[test]
fn export_typescript_bindings() {
    let path = "../src/shared/bindings/commands.ts";
    pstmacro_lib::make_builder()
        .export(specta_typescript::Typescript::default(), path)
        .expect("Failed to export typescript bindings");

    // Prepend pragmas so the auto-generated file doesn't trip
    // tsc noUnusedLocals / eslint rules. Re-prepended on every export.
    let original = std::fs::read_to_string(path).expect("read bindings");
    let with_pragmas = format!("// @ts-nocheck\n/* eslint-disable */\n{original}");
    std::fs::write(path, with_pragmas).expect("write bindings");
}
