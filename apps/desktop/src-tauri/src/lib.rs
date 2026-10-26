//! `kontor-desktop` — Tauri 2 desktop shell for the Kontor operator console.
//!
//! # What this shell is
//!
//! A window that loads the console's built assets, and a place to keep the
//! selected loopback endpoint and its realm bearer in. That is the whole program.
//!
//! # What this shell deliberately is not
//!
//! It embeds no store, no scheduler, no runtime adapter and no second copy of
//! the daemon. It opens no database, takes no filesystem lock, binds no socket
//! and reaches no runtime. The console inside it talks to a realm over the same
//! authenticated `/v1` contract any other client would use — so there is no
//! privileged path for a view to reach for when the contract does not serve
//! something.
//!
//! The one capability it adds over the browser build is a place to keep the
//! console's endpoint and realm bearer, which a browser has nowhere safe to put:
//! the operating system's own credential store, behind three commands in
//! [`credentials`]. It replaced the
//! Stronghold vault (ASMA-8645), whose dependency tree carries
//! RUSTSEC-2026-0387. A vault saved by that version is left on disk untouched
//! and is not read.
//!
//! Tests never enter the Tauri event loop: `run` is invoked only from the
//! `kontor-desktop` binary and no test in this crate calls it.

mod credentials;

use credentials::{CredentialError, CredentialStore, Endpoint, OsKeychain, SERVICE, machine_host};

fn os_store() -> CredentialStore<OsKeychain> {
    CredentialStore::new(OsKeychain::new(SERVICE), &machine_host())
}

// The commands are async so a keychain prompt waits off the main thread.

#[tauri::command]
async fn credential_save(
    profile: Option<String>,
    base_url: String,
    token: String,
) -> Result<(), CredentialError> {
    os_store().save(profile.as_deref(), &Endpoint { base_url, token })
}

#[tauri::command]
async fn credential_load(profile: Option<String>) -> Result<Endpoint, CredentialError> {
    os_store().load(profile.as_deref())
}

#[tauri::command]
async fn credential_clear(profile: Option<String>) -> Result<(), CredentialError> {
    os_store().clear(profile.as_deref())
}

/// Runs the Tauri desktop application. Never called from tests.
///
/// # Panics
/// Panics when the Tauri application cannot start.
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            credential_save,
            credential_load,
            credential_clear
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Kontor desktop shell");
}
