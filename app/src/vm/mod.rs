//! One view model per page (ADR 0003): plain Rust state, focus and what each input does, with no
//! Slint types, so every page is tested without a window. The Continue screen's is
//! `src/screen.rs` (2.04); these are the pages one button away from it.

pub mod about;
pub mod saves;
pub mod settings;

pub use saves::SavesVm;
pub use settings::SettingsVm;
