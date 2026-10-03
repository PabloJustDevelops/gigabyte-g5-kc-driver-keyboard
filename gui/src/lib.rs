//! The panel as a library, so the binary and the headless snapshot tests can
//! build the exact same views. `main.rs` is only the window boilerplate.

pub mod backend;
pub mod layout;
pub mod motion;
pub mod panel;
pub mod theme;
pub mod views;
pub mod widgets;
