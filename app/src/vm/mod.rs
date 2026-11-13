//! View models: what each screen shows, built in plain Rust so it is tested without a window
//! (docs/code-map.md, `app/src/vm/`). No Slint types here; the `.slint` file only draws these.

pub mod about;
pub mod fleet;

pub use fleet::FleetVm;
