//! `kontor-desktop` — Tauri 2 desktop shell for the Kontor operator console.
//!
//! # What this shell is
//!
//! A window that loads the console's built assets. That is the whole program.
//!
//! # What this shell deliberately is not
//!
//! It embeds no store, no scheduler, no runtime adapter and no second copy of
//! the daemon. It opens no database, takes no filesystem lock, binds no socket
//! and reaches no runtime. The console inside it talks to a realm over the same
//! authenticated `/v1` contract any other client would use — so the desktop
//! build has no capability the browser build lacks, and there is no privileged
//! path for a view to reach for when the contract does not serve something.
//!
//! It keeps no credential. The Stronghold vault was removed (ASMA-8645): its
//! dependency tree carries RUSTSEC-2026-0387 and no upstream release fixes it.
//! Until KON-MVP-17 decides a replacement store, the console's Stronghold
//! option fails visibly on desktop.
//!
//! Tests never enter the Tauri event loop: `run` is invoked only from the
//! `kontor-desktop` binary and no test in this crate calls it.

/// Runs the Tauri desktop application. Never called from tests.
///
/// # Panics
/// Panics when the Tauri application cannot start.
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running the Kontor desktop shell");
}
