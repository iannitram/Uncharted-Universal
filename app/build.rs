use slint_build::{CompilerConfiguration, DefaultTranslationContext};

fn main() {
    // The texts are `@tr(...)` in the .slint file; their translations are the gettext catalogues
    // in lang/<code>/LC_MESSAGES/uncharted-app.po, bundled into the binary. No context: one
    // English text has one translation wherever it appears.
    println!("cargo:rerun-if-changed=lang");
    // Debug builds (and so `cargo test`) carry element ids, so the window tests in src/main.rs
    // can find texts on screen; release builds stay without them.
    let debug = std::env::var("PROFILE").is_ok_and(|profile| profile == "debug");
    let config = CompilerConfiguration::new()
        .with_bundled_translations("lang")
        .with_default_translation_context(DefaultTranslationContext::None)
        .with_debug_info(debug);
    slint_build::compile_with_config("ui/app.slint", config).expect("compile ui/app.slint");
}
