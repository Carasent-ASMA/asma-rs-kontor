//! `kontor-desktop` binary entry point.
//!
//! Prevents an additional console window on Windows in release builds and
//! delegates to the library entry point, which is never called from tests.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    kontor_desktop_lib::run()
}
