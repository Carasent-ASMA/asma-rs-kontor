//! The `kontor-daemon` executable: one Realm, one loopback socket.
//!
//! Everything reusable lives in the library, so this file is only the four
//! things a binary owns — argument parsing, binding the socket, the signals that
//! end or rotate the process, and the operator commands that act on a state root
//! without serving it.
//!
//! # Why the recovery commands live here and not in `kontor`
//!
//! The `kontor` CLI is a client: it holds a bearer token and talks to a running
//! daemon over loopback, and its dependency graph deliberately reaches no store.
//! A restore replaces the database file a daemon has open; an import writes to
//! it directly; a rotation of a stopped Realm needs the state root's exclusive
//! lock. All three are decisions about a *state root*, which is this process's
//! own subject, and none of them can be expressed as a request to a daemon that
//! may not be running.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use kontor_api::state::BarrierState;
use kontor_core::id::{ProjectId, Timestamp};
use kontor_daemon::{
    COMPLETION_SCAN_INTERVAL, DEFAULT_PORT, Daemon, DaemonConfig, endpoint, jira_sync, logging,
    recovery, runtimes, usage,
};
use tracing::{error, info, warn};

/// Serve one Kontor realm on loopback, or act on its state root.
#[derive(Debug, Parser)]
#[command(name = "kontor-daemon", version, about)]
struct Arguments {
    /// The state root holding this realm's database, lock and credentials.
    #[arg(long, global = true)]
    state_root: Option<PathBuf>,
    /// The loopback port to bind.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,
    /// A browser origin to answer, repeatable. Defaults to the desktop shell's.
    #[arg(long = "origin")]
    origins: Vec<String>,
    /// An operator command. Serving is what happens when none is given, so the
    /// existing invocation keeps working unchanged.
    #[command(subcommand)]
    command: Option<Command>,
}

/// The operator commands that act on a state root.
#[derive(Debug, Subcommand)]
enum Command {
    /// Copy the database into a verified snapshot and prune stale ones.
    ///
    /// Safe while the daemon serves.
    Snapshot {
        /// Where to write it. Defaults to `backups/` inside the state root.
        #[arg(long)]
        into: Option<PathBuf>,
    },
    /// List this realm's verified snapshots, newest first.
    Snapshots {
        /// Where to look. Defaults to `backups/` inside the state root.
        #[arg(long)]
        into: Option<PathBuf>,
    },
    /// Restore a verified snapshot into the state root. Requires a stopped realm.
    Restore {
        /// The snapshot file. Its manifest must be beside it.
        #[arg(long)]
        snapshot: PathBuf,
    },
    /// Write the versioned, redacted export document.
    ///
    /// Safe while the daemon serves.
    Export {
        /// Where to write it. Defaults to standard output.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Import another realm's export. Requires a stopped realm and an explicit
    /// destination project.
    Import {
        /// The export document.
        #[arg(long)]
        from: PathBuf,
        /// The destination project the records are imported into.
        #[arg(long)]
        project: String,
    },
    /// Mint a new credential set for a stopped realm.
    ///
    /// A running daemon rotates its own credentials on `SIGHUP`, which swaps the
    /// in-memory set in the same operation.
    RotateCredentials,
    /// Install a strict Jira credential from bounded stdin for a stopped realm.
    InstallJiraCredential {
        /// The non-secret alias referenced by this realm's `jira.json`.
        #[arg(long)]
        alias: String,
    },
}

#[derive(Debug, thiserror::Error)]
enum OperatorError {
    #[error(transparent)]
    Recovery(#[from] recovery::RecoveryError),
    #[error("another daemon holds the state root, or its lock is unavailable")]
    CredentialLock,
    #[error(transparent)]
    Jira(#[from] kontor_jira::JiraError),
    #[error(
        "credential installation requires an absolute initialized realm root and a configured alias"
    )]
    CredentialScope,
    #[error(transparent)]
    CredentialInstall(#[from] kontor_jira::CredentialInstallError),
}

impl OperatorError {
    fn category(&self) -> &'static str {
        match self {
            Self::Recovery(error) => error.category(),
            Self::CredentialLock => "state_root_locked",
            Self::Jira(_) | Self::CredentialScope | Self::CredentialInstall(_) => "jira_credential",
        }
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    logging::install();
    let arguments = Arguments::parse();
    let Some(state_root) = arguments.state_root.clone() else {
        error!(category = "usage", "--state-root is required");
        return std::process::ExitCode::FAILURE;
    };

    if let Some(command) = arguments.command {
        return match run(&state_root, command) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                error!(category = error.category(), detail = %error, "the command did not run");
                std::process::ExitCode::FAILURE
            }
        };
    }

    serve(state_root, arguments.port, arguments.origins).await
}

/// Run one operator command against a state root.
fn run(state_root: &Path, command: Command) -> Result<(), OperatorError> {
    let now = Timestamp::now();
    match command {
        Command::Snapshot { into } => {
            let (outcome, pruned) = recovery::snapshot(state_root, into.as_deref(), now)?;
            println!("{}", outcome.snapshot.display());
            info!(kept = 1, pruned = pruned.len(), "snapshot complete");
            Ok(())
        }
        Command::Snapshots { into } => {
            let directory = into.unwrap_or_else(|| recovery::backups_in(state_root));
            // The realm is read from the database rather than guessed from the
            // directory, so a shared backup directory lists only this realm's.
            let store = kontor_store::SqliteStore::open(&recovery::database_in(state_root))
                .map_err(|source| recovery::RecoveryError::Store { source })?;
            for snapshot in kontor_store::backup::list_snapshots(&directory, store.realm_id())
                .map_err(recovery::RecoveryError::from)?
            {
                println!(
                    "{}\t{}\t{} bytes",
                    snapshot.manifest.created_at,
                    snapshot.snapshot.display(),
                    snapshot.manifest.byte_length
                );
            }
            Ok(())
        }
        Command::Restore { snapshot } => {
            let plan = recovery::restore(state_root, &snapshot, now)?;
            println!("{}", plan.restored.display());
            Ok(())
        }
        Command::Export { out } => {
            let export = recovery::export(state_root, now)?;
            let bytes = export
                .canonical_bytes()
                .map_err(recovery::RecoveryError::from)?;
            match out {
                Some(path) => {
                    std::fs::write(&path, bytes).map_err(|source| recovery::RecoveryError::Io {
                        action: "written",
                        source,
                    })?;
                    println!("{}", path.display());
                }
                None => {
                    use std::io::Write;
                    std::io::stdout().write_all(&bytes).map_err(|source| {
                        recovery::RecoveryError::Io {
                            action: "written",
                            source,
                        }
                    })?;
                }
            }
            Ok(())
        }
        Command::Import { from, project } => {
            let project = ProjectId::parse(&project).map_err(|source| {
                recovery::RecoveryError::Backup(kontor_store::backup::BackupError::Domain(source))
            })?;
            let report = recovery::import(state_root, &from, project, now)?;
            println!("{}", report.import_id);
            Ok(())
        }
        Command::RotateCredentials => {
            recovery::rotate_credentials(state_root)?;
            Ok(())
        }
        Command::InstallJiraCredential { alias } => install_jira_credential(
            state_root,
            &alias,
            std::io::stdin().lock(),
            |scope, secret| kontor_jira::install_credentials(scope, &alias, secret),
        ),
    }
}

fn install_jira_credential(
    state_root: &Path,
    alias: &str,
    reader: impl std::io::Read,
    install: impl FnOnce(
        &kontor_jira::JiraCredentialScope,
        secrecy::SecretString,
    ) -> Result<(), kontor_jira::CredentialInstallError>,
) -> Result<(), OperatorError> {
    if !state_root.is_absolute() {
        return Err(OperatorError::CredentialScope);
    }
    let root = state_root
        .canonicalize()
        .map_err(|_| OperatorError::CredentialScope)?;
    if !root.is_dir() {
        return Err(OperatorError::CredentialScope);
    }
    let _lock = kontor_daemon::lock::StateRootLock::acquire(&root)
        .map_err(|_| OperatorError::CredentialLock)?;
    // Read-only validation refuses missing/legacy/invalid databases without
    // initializing, migrating, or replacing any realm. Read stdin only last.
    let realm = kontor_store::SqliteStore::read_existing_realm(&recovery::database_in(&root))
        .map_err(|_| OperatorError::CredentialScope)?;
    let connectors = kontor_jira::JiraConnectors::read(&root, realm.realm_id)?;
    if !connectors.references_alias(alias) {
        return Err(OperatorError::CredentialScope);
    }
    let scope = kontor_jira::JiraCredentialScope::at(&root, realm.realm_id)?;
    let secret = kontor_jira::read_credential_document(reader)?;
    install(&scope, secret)?;
    Ok(())
}

/// Keep the API live while the startup barrier is being settled.
///
/// Reconciliation is allowed to wait on a runtime, but that wait must not hide
/// the health, identity, snapshots and event feed an operator needs to diagnose
/// it. The router already refuses scheduling while the barrier is pending or
/// failed; polling both futures here makes that existing contract reachable over
/// the loopback socket from the moment it is bound.
async fn serve_while_reconciling<Server, Reconciliation, OnSettled, Output>(
    server: Server,
    reconciliation: Reconciliation,
    mut on_settled: OnSettled,
) -> Output
where
    Server: std::future::Future<Output = Output>,
    Reconciliation: std::future::Future<Output = BarrierState>,
    OnSettled: FnMut(BarrierState),
{
    tokio::pin!(server);
    tokio::pin!(reconciliation);
    let mut reconciliation_pending = true;
    loop {
        tokio::select! {
            output = &mut server => return output,
            outcome = &mut reconciliation, if reconciliation_pending => {
                reconciliation_pending = false;
                on_settled(outcome);
            }
        }
    }
}

/// Serve one Realm until the process is asked to stop.
async fn serve(state_root: PathBuf, port: u16, origins: Vec<String>) -> std::process::ExitCode {
    let mut config = DaemonConfig::at(state_root).with_port(port);
    if !origins.is_empty() {
        config.allowed_origins = origins;
    }
    // The fleet comes from the state root, so the shipped daemon's session routes
    // are backed by the adapters this Realm is configured with. A Realm with no
    // `runtimes.json` composes an empty fleet and says so below rather than
    // pretending to have one.
    let daemon = match Daemon::start_configured(config) {
        Ok(daemon) => daemon,
        Err(error) => {
            error!(detail = %error, "kontor could not start");
            return std::process::ExitCode::FAILURE;
        }
    };
    let families: Vec<String> = daemon
        .state()
        .runtimes()
        .families()
        .map(ToString::to_string)
        .collect();
    if families.is_empty() {
        info!(
            realm_id = %daemon.realm_id(),
            settings = %runtimes::path_in(&daemon.config().state_root).display(),
            "no runtime is configured; session routes will answer as unconfigured"
        );
    } else {
        info!(realm_id = %daemon.realm_id(), ?families, "runtime fleet composed");
    }

    // Quota observation starts before the socket does. A daemon that has just
    // come up is exactly when its quota rows are most likely to be stale —
    // anything that happened while it was down happened unobserved — and the
    // poller stops itself when the same shutdown signal the streams watch fires.
    tokio::spawn(usage::poll_until_stopped(
        daemon.usage_poller(),
        daemon.state(),
    ));
    tokio::spawn(jira_sync::poll_until_stopped(
        daemon.jira_reconciler(),
        daemon.state(),
    ));
    // Publication checks run only where an operator installed the GitHub App;
    // without the document the gateway is absent and nothing is posted.
    if let Some(gateway) = daemon.github_publication() {
        tokio::spawn(kontor_daemon::github_publication::poll_until_stopped(
            gateway,
            daemon.jira_reconciler(),
            daemon.state(),
        ));
    }
    let _succession_supervisor = daemon.spawn_succession_supervisor(daemon.jira_reconciler());
    let _completion_scanner = daemon.spawn_completion_scanner(COMPLETION_SCAN_INTERVAL);
    let _admission_reconciler = daemon.spawn_admission_reconciler(COMPLETION_SCAN_INTERVAL);
    let _consultation_releaser = daemon.spawn_consultation_releaser();

    let bind: SocketAddr = daemon.config().bind;
    let listener = match tokio::net::TcpListener::bind(bind).await {
        Ok(listener) => listener,
        Err(error) => {
            error!(address = %bind, detail = %error, "the loopback socket could not be bound");
            daemon.shutdown();
            return std::process::ExitCode::FAILURE;
        }
    };
    // Recorded from the listener's own address, not from the configured one: a
    // daemon started with `--port 0` asked the operating system to choose, and the
    // configured value is a port no caller can reach. A failure to record is a
    // warning rather than a stop — the file is a convenience for local callers, and
    // the lock is what proves ownership of the state root.
    match listener.local_addr() {
        Ok(bound) => match endpoint::publish(&daemon.config().state_root, bound) {
            Ok(path) => {
                info!(realm_id = %daemon.realm_id(), endpoint = %path.display(), "loopback endpoint recorded")
            }
            Err(error) => warn!(
                realm_id = %daemon.realm_id(),
                detail = %error,
                "the loopback endpoint could not be recorded; local callers must pass --base-url"
            ),
        },
        Err(error) => warn!(detail = %error, "the bound address could not be read back"),
    }

    info!(realm_id = %daemon.realm_id(), address = %bind, "kontor is serving");

    let signals = daemon.state().signals().clone();
    let served = axum::serve(listener, daemon.router())
        .with_graceful_shutdown(async move {
            // Ctrl-C ends the process; the same signal ends every open stream, so a
            // subscriber's last delivered position is one it really reached.
            if tokio::signal::ctrl_c().await.is_err() {
                return;
            }
            signals.stop();
        })
        .into_future();
    let realm_id = daemon.realm_id();
    let outcome = {
        let running = serve_while_reconciling(served, daemon.reconcile(), move |outcome| {
            if outcome != BarrierState::Open {
                // Serving with the barrier shut is deliberate: health, identity,
                // snapshots and the event feed still answer, and they are exactly
                // what an operator needs to see *why* scheduling is shut.
                error!(
                    realm_id = %realm_id,
                    "startup reconciliation did not complete; scheduling stays shut"
                );
            }
        });
        tokio::pin!(running);

        loop {
            tokio::select! {
                result = &mut running => break result,
                // A rotation while serving is the only way to invalidate every issued
                // token without a restart, which is what makes it useful: a leaked
                // token is refused from the next request onwards and the runs this
                // Realm is supervising never notice.
                () = rotation_requested() => match daemon.rotate_credentials() {
                    Ok(()) => {}
                    Err(error) => error!(
                        realm_id = %daemon.realm_id(),
                        category = "credentials",
                        detail = %error,
                        "credentials could not be rotated; the previous set stays in force"
                    ),
                },
            }
        }
    };

    daemon.shutdown();
    match outcome {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            error!(detail = %error, "the loopback server stopped with an error");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Resolve when the operator asks for a credential rotation.
///
/// `SIGHUP` is the conventional "re-read your configuration" signal and it is
/// the one an operator already has for a process they cannot send an
/// authenticated request to — which is exactly the situation a leaked token
/// creates. On platforms without it the future never resolves, and rotation is
/// the stopped-realm command.
#[cfg(unix)]
async fn rotation_requested() {
    use tokio::signal::unix::{SignalKind, signal};
    match signal(SignalKind::hangup()) {
        Ok(mut hangup) => {
            hangup.recv().await;
        }
        // No handler could be installed, so no rotation can be requested this
        // way. Never resolving is the honest answer: resolving immediately would
        // rotate the credentials of a Realm nobody asked to rotate.
        Err(_) => std::future::pending::<()>().await,
    }
}

#[cfg(not(unix))]
async fn rotation_requested() {
    std::future::pending::<()>().await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn jira_credential_command_accepts_only_a_non_secret_alias() {
        let args = Arguments::try_parse_from([
            "kontor-daemon",
            "--state-root",
            "/tmp/synthetic-realm",
            "install-jira-credential",
            "--alias",
            "work",
        ])
        .unwrap();
        assert!(
            matches!(args.command, Some(Command::InstallJiraCredential { alias }) if alias == "work")
        );
        assert!(
            Arguments::try_parse_from([
                "kontor-daemon",
                "install-jira-credential",
                "--alias",
                "work",
                "--api-token",
                "synthetic-canary",
            ])
            .is_err()
        );
    }

    fn configured_credential_root() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let store = kontor_store::SqliteStore::open(&recovery::database_in(root.path())).unwrap();
        drop(store);
        std::fs::write(root.path().join("jira.json"), serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "projects": [{"project_id": ProjectId::generate(), "endpoint": "https://example.atlassian.net", "project_key": "ASMA", "credential_alias": "work"}]
        })).unwrap()).unwrap();
        root
    }

    #[test]
    fn unknown_roots_aliases_and_relative_paths_refuse_before_stdin_or_effects() {
        struct MustNotRead;
        impl std::io::Read for MustNotRead {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                panic!("preflight must precede stdin");
            }
        }
        let empty = tempfile::tempdir().unwrap();
        let configured = configured_credential_root();
        // A real initialized relative root distinguishes the absolute-path
        // guard from an incidental missing-directory error.
        let cwd = std::env::current_dir().unwrap();
        let mut relative = PathBuf::new();
        for _ in cwd.components().skip(1) {
            relative.push("..");
        }
        relative.push(configured.path().strip_prefix("/").unwrap());
        assert_eq!(
            relative.canonicalize().unwrap(),
            configured.path().canonicalize().unwrap()
        );
        for (root, alias) in [
            (relative.as_path(), "work"),
            (empty.path(), "work"),
            (configured.path(), "unknown"),
        ] {
            assert!(
                install_jira_credential(root, alias, MustNotRead, |_, _| panic!(
                    "no credential effects"
                ))
                .is_err()
            );
        }
        assert!(!recovery::database_in(empty.path()).exists());
        std::fs::write(configured.path().join("jira.json"), b"{invalid}").unwrap();
        assert!(
            install_jira_credential(configured.path(), "work", MustNotRead, |_, _| panic!(
                "no credential effects"
            ))
            .is_err()
        );
    }

    #[test]
    fn a_running_realm_refuses_credential_installation_before_stdin_or_effects() {
        let root = tempfile::tempdir().unwrap();
        let _held = kontor_daemon::lock::StateRootLock::acquire(root.path()).unwrap();
        struct MustNotRead;
        impl std::io::Read for MustNotRead {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                panic!("locked realm must refuse before reading credentials");
            }
        }
        let result = install_jira_credential(root.path(), "work", MustNotRead, |_, _| {
            panic!("locked realm must refuse before credential effects");
        });
        assert!(matches!(result, Err(OperatorError::CredentialLock)));
    }

    #[test]
    fn a_copied_realm_and_duplicate_alias_cannot_update_a_locked_roots_entry() {
        use kontor_accounts::{KeychainBackend, KeychainFailure, KeychainTarget, KeychainWriter};
        use secrecy::{ExposeSecret, SecretString};
        #[derive(Default)]
        struct Fake(Mutex<std::collections::BTreeMap<(String, String), SecretString>>);
        impl KeychainBackend for Fake {
            fn secret(&self, t: &KeychainTarget) -> Result<SecretString, KeychainFailure> {
                self.0
                    .lock()
                    .unwrap()
                    .get(&(t.service().to_owned(), t.account().to_owned()))
                    .cloned()
                    .ok_or(KeychainFailure::NotFound)
            }
        }
        impl KeychainWriter for Fake {
            fn set_secret(
                &self,
                t: &KeychainTarget,
                s: &SecretString,
            ) -> Result<(), KeychainFailure> {
                self.0
                    .lock()
                    .unwrap()
                    .insert((t.service().to_owned(), t.account().to_owned()), s.clone());
                Ok(())
            }
            fn delete_secret(&self, t: &KeychainTarget) -> Result<(), KeychainFailure> {
                self.0
                    .lock()
                    .unwrap()
                    .remove(&(t.service().to_owned(), t.account().to_owned()));
                Ok(())
            }
        }
        let a = configured_credential_root();
        let b = tempfile::tempdir().unwrap();
        std::fs::copy(
            recovery::database_in(a.path()),
            recovery::database_in(b.path()),
        )
        .unwrap();
        std::fs::copy(a.path().join("jira.json"), b.path().join("jira.json")).unwrap();
        let fake = Fake::default();
        let old = br#"{"email":"old@example.test","api_token":"old-synthetic-token"}"#;
        install_jira_credential(a.path(), "work", &old[..], |scope, secret| {
            kontor_jira::install_credentials_with(scope, "work", secret, &fake)
        })
        .unwrap();
        let (old_key, old_value) = {
            let v = fake.0.lock().unwrap();
            let (k, v) = v.iter().next().unwrap();
            (k.clone(), v.expose_secret().to_owned())
        };
        let _running = kontor_daemon::lock::StateRootLock::acquire(a.path()).unwrap();
        let new = br#"{"email":"new@example.test","api_token":"new-synthetic-token"}"#;
        install_jira_credential(b.path(), "work", &new[..], |scope, secret| {
            kontor_jira::install_credentials_with(scope, "work", secret, &fake)
        })
        .unwrap();
        let values = fake.0.lock().unwrap();
        assert_eq!(values.len(), 2);
        assert_eq!(values.get(&old_key).unwrap().expose_secret(), old_value);
    }

    #[test]
    fn stopped_realm_installation_retains_its_lock_and_refuses_bad_input() {
        let root = configured_credential_root();
        let valid = br#"{"email":"operator@example.test","api_token":"synthetic-canary"}"#;
        install_jira_credential(root.path(), "work", &valid[..], |_, _| {
            assert!(kontor_daemon::lock::StateRootLock::acquire(root.path()).is_err());
            Ok(())
        })
        .unwrap();
        assert!(kontor_daemon::lock::StateRootLock::acquire(root.path()).is_ok());
        let result =
            install_jira_credential(root.path(), "work", &b"synthetic-canary"[..], |_, _| {
                panic!("malformed credentials cannot reach the installer");
            });
        assert!(result.is_err());
        assert!(!format!("{:?}", result.unwrap_err()).contains("synthetic-canary"));
    }

    #[tokio::test]
    async fn the_server_is_polled_while_startup_reconciliation_is_pending() {
        let server_polled = Arc::new(AtomicBool::new(false));
        let reconciliation_reported = Arc::new(AtomicBool::new(false));

        let observed_server = Arc::clone(&server_polled);
        let observed_reconciliation = Arc::clone(&reconciliation_reported);
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            serve_while_reconciling(
                async move {
                    observed_server.store(true, Ordering::SeqCst);
                    "server stopped"
                },
                std::future::pending::<BarrierState>(),
                move |_| observed_reconciliation.store(true, Ordering::SeqCst),
            ),
        )
        .await
        .expect("the server is available without waiting for reconciliation");

        assert_eq!(outcome, "server stopped");
        assert!(server_polled.load(Ordering::SeqCst));
        assert!(!reconciliation_reported.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn failed_reconciliation_leaves_the_server_running() {
        let stop = Arc::new(tokio::sync::Notify::new());
        let observed = Arc::new(Mutex::new(None));

        let server_stop = Arc::clone(&stop);
        let callback_stop = Arc::clone(&stop);
        let callback_observed = Arc::clone(&observed);
        let outcome = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            serve_while_reconciling(
                async move {
                    server_stop.notified().await;
                    "server stopped"
                },
                std::future::ready(BarrierState::Failed),
                move |state| {
                    *callback_observed.lock().expect("the observation lock") = Some(state);
                    callback_stop.notify_one();
                },
            ),
        )
        .await
        .expect("failed reconciliation leaves the server available");

        assert_eq!(outcome, "server stopped");
        assert_eq!(
            *observed.lock().expect("the observation lock"),
            Some(BarrierState::Failed)
        );
    }
}
