//! `kontor-desktop` — Tauri 2 desktop shell for the Kontor operator console.
//!
//! Scaffold placeholder created by KON-MVP-02. KON-MVP-17 implements the real
//! desktop shell against the Kontor API; until then this crate only fixes the
//! workspace member list and the Tauri dependency pins.
//!
//! Tests never enter the Tauri event loop: `run` is invoked only from the
//! `kontor-desktop` binary and no test in this crate calls it.

/// Runs the Tauri desktop application. Never called from tests.
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running the Kontor desktop shell");
}
