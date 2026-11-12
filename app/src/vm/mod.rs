//! One view model per page (ADR 0003): plain Rust state, focus order and what each `Nav` press
//! does. No Slint types here; the `.slint` file only draws these and sends its callbacks.

pub mod fleet;

pub use fleet::FleetVm;
