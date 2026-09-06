// Thin passthrough — all application logic lives in lib.rs (required so the
// same crate can later target mobile via tauri::mobile_entry_point).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    g5kbd_gui_lib::run()
}
