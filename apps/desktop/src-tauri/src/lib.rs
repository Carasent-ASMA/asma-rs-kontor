//! `kontor-desktop` — Tauri 2 desktop shell for the Kontor operator console.
//!
//! # What this shell is
//!
//! A window that loads the console's built assets, and a vault to keep the
//! selected loopback endpoint and its realm bearer in. That is the whole
//! program.
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
//! The one thing it adds is durable, encrypted storage for a credential, which
//! a browser has nowhere safe to put.
//!
//! Tests never enter the Tauri event loop: `run` is invoked only from the
//! `kontor-desktop` binary and no test in this crate calls it.

use tauri::Manager;

/// The file the vault's Argon2 salt is kept in.
///
/// It lives beside the vault in this application's own data directory, and is
/// created by the plugin on first use.
const SALT_FILE: &str = "kontor-console.salt";

/// Runs the Tauri desktop application. Never called from tests.
///
/// # Panics
/// Panics when the application's data directory cannot be created or the
/// Stronghold plugin cannot be registered — both of which mean the shell has
/// nowhere to keep a credential, and starting anyway would silently degrade to
/// a window that cannot remember anything.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Argon2 needs a salt that survives restarts, so it is kept in the
            // application's own data directory rather than derived from
            // anything about the machine.
            let data_dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let salt = data_dir.join(SALT_FILE);
            app.handle()
                .plugin(tauri_plugin_stronghold::Builder::with_argon2(&salt).build())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Kontor desktop shell");
}
