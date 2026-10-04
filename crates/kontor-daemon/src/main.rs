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

    /// A reader that records whether anybody tried to read the credential.
    ///
    /// Not a panicking reader: a panic would prove the read did not *complete*,
    /// and the contract under test is that it is never attempted at all. The
    /// body matters for the control, which has to get *past* the schema gate and
    /// then fail on the document itself.
    struct WatchfulReader {
        seen: Arc<AtomicBool>,
        body: std::io::Cursor<Vec<u8>>,
    }

    impl WatchfulReader {
        fn new(seen: &Arc<AtomicBool>, body: &[u8]) -> Self {
            Self {
                seen: Arc::clone(seen),
                body: std::io::Cursor::new(body.to_vec()),
            }
        }
    }

    impl std::io::Read for WatchfulReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.seen.store(true, Ordering::SeqCst);
            std::io::Read::read(&mut self.body, buffer)
        }
    }

    /// The frozen-claim trigger exactly as `0120` defined it.
    ///
    /// Sliced out of that migration rather than transcribed, so a fixture that
    /// claims to reproduce the 120/121 era cannot drift from the migration that
    /// actually created it.
    fn frozen_claim_trigger_as_of_0120() -> String {
        const MIGRATION: &str =
            include_str!("../../kontor-store/migrations/0120_core_team_route_successions.sql");
        let start = MIGRATION
            .find("CREATE TRIGGER core_team_route_succession_claim_is_frozen")
            .expect("0120 defines the frozen-claim trigger");
        let end = MIGRATION[start..]
            .find("\nEND;")
            .map(|offset| start + offset + "\nEND;".len())
            .expect("the trigger body terminates");
        MIGRATION[start..end].to_owned()
    }

    /// Everything about one state root that must survive a refused install.
    fn realm_fingerprint(database: &std::path::Path) -> (Vec<u8>, i64, Vec<String>) {
        let bytes = std::fs::read(database).expect("the database reads");
        let connection = rusqlite::Connection::open(database).expect("the database opens");
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("the version reads");
        let mut statement = connection
            .prepare("SELECT type || ':' || name FROM sqlite_master ORDER BY type, name")
            .expect("the catalogue prepares");
        let objects: Vec<String> = statement
            .query_map([], |row| row.get(0))
            .expect("the catalogue reads")
            .collect::<Result<_, _>>()
            .expect("the catalogue rows read");
        (bytes, version, objects)
    }

    /// Reconstruct the schema as it stood at `version`.
    ///
    /// Stamping `user_version` over a current database is not a stopped Realm;
    /// it is a current Realm wearing an old number, and a fixture built that way
    /// cannot show that the refusal met the schema those versions actually had.
    /// The later 0123/0124 memory tables are removed first. Then `0122` is
    /// undone properly — its unique receipt index dropped and its
    /// replacement trigger swapped back for the one `0120` wrote — and then each
    /// later table is removed in turn.
    ///
    /// `0120` adds `core_team_route_successions`, `0121` adds
    /// `imported_record_evidence`, and `0122` adds the index and replaces the
    /// trigger. At 119 the `0120` table is gone and takes its trigger with it.
    fn realm_stopped_at(root: &std::path::Path, version: i64) -> std::path::PathBuf {
        let database = root.join("kontor.sqlite3");
        drop(kontor_store::SqliteStore::open(&database).expect("the realm initializes"));
        let connection = rusqlite::Connection::open(&database).expect("the database opens");

        connection
            .execute_batch(
                "DROP TABLE memory_projection_rebuild_results;
                 DROP TABLE memory_projection_rebuild_keys;
                 DROP TABLE memory_recall_keys;
                 DROP TABLE memory_recall_metadata;
                 DROP TABLE memory_experience_proposals;
                 DROP TABLE memory_projection_active;
                 DROP TABLE memory_projection_snapshots;
                 DROP TABLE memory_experience_eligibility;",
            )
            .expect("the later memory tables are removed");

        // Undo 0122: none of 119, 120 or 121 ever had it.
        connection
            .execute_batch(
                "DROP INDEX ux_core_team_route_succession_receipt;
                 DROP TRIGGER core_team_route_succession_claim_is_frozen;",
            )
            .expect("the 0122 artefacts are removed");
        if version >= 120 {
            connection
                .execute_batch(&frozen_claim_trigger_as_of_0120())
                .expect("the 0120-era trigger is restored");
        }
        if version < 121 {
            connection
                .execute_batch("DROP TABLE imported_record_evidence;")
                .expect("the 0121 table is removed");
        }
        if version < 120 {
            connection
                .execute_batch("DROP TABLE core_team_route_successions;")
                .expect("the 0120 table is removed");
        }
        connection
            .pragma_update(None, "user_version", version)
            .expect("the version is stamped");
        drop(connection);
        database
    }

    /// The schema text of one named object, as the database holds it.
    fn stored_object_sql(database: &std::path::Path, name: &str) -> Option<String> {
        let connection = rusqlite::Connection::open(database).expect("the database opens");
        connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = ?1",
                [name],
                |row| row.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten()
    }

    /// The 120 and 121 fixtures are the schema those versions had.
    ///
    /// Asserted separately from the refusal, because a fixture that is merely a
    /// renumbered 122 database would let the refusal test pass while proving
    /// nothing about the schemas it claims to cover.
    #[test]
    fn the_stopped_fixtures_reproduce_their_own_schema_era() {
        for version in [120_i64, 121] {
            let root = tempfile::tempdir().expect("a state root");
            let canonical = root.path().canonicalize().expect("an absolute root");
            let database = realm_stopped_at(&canonical, version);

            assert!(
                stored_object_sql(&database, "ux_core_team_route_succession_receipt").is_none(),
                "the 0122 unique receipt index exists in a fixture stopped at {version}"
            );
            let trigger =
                stored_object_sql(&database, "core_team_route_succession_claim_is_frozen")
                    .unwrap_or_else(|| panic!("the 0120-era trigger is missing at {version}"));
            assert_eq!(
                trigger.trim(),
                frozen_claim_trigger_as_of_0120()
                    .trim()
                    .trim_end_matches(';')
                    .trim(),
                "the frozen-claim trigger at {version} is not the one 0120 wrote"
            );
            assert!(
                !trigger.contains("receipted_at IS NOT NEW.receipted_at"),
                "the 0122 trigger clause survives in a fixture stopped at {version}"
            );
            let evidence = stored_object_sql(&database, "imported_record_evidence");
            if version < 121 {
                assert!(evidence.is_none(), "the 0121 table exists at {version}");
            } else {
                assert!(evidence.is_some(), "the 0121 table is missing at {version}");
            }
        }
    }

    /// Every schema this lane passes through is refused, and refused early.
    ///
    /// ASMA-8187 takes the binary from 119 to 122, so an operator can hold a
    /// Realm stopped at 119, 120 or 121 and meet a 122 binary. The exact-version
    /// contract refuses all three — that is decided and correct — and what this
    /// test pins is the *shape* of the refusal: it happens before the credential
    /// is read and before anything is installed, and it leaves the Realm exactly
    /// as it found it. A refusal that read the secret first would have handled
    /// it needlessly; one that migrated would upgrade a Realm its operator had
    /// deliberately stopped (ASMA-8187 / ASMA-8015).
    #[test]
    fn a_realm_stopped_before_the_current_schema_refuses_without_reading_or_installing() {
        for version in [119_i64, 120, 121] {
            let root = tempfile::tempdir().expect("a state root");
            let canonical = root.path().canonicalize().expect("an absolute root");
            let database = realm_stopped_at(&canonical, version);
            let before = realm_fingerprint(&database);

            let read_attempted = Arc::new(AtomicBool::new(false));
            let installed = Arc::new(AtomicBool::new(false));
            let outcome = install_jira_credential(
                &canonical,
                "any-configured-alias",
                WatchfulReader::new(&read_attempted, b""),
                |_scope, _secret| {
                    installed.store(true, Ordering::SeqCst);
                    Ok(())
                },
            );

            assert!(
                matches!(outcome, Err(OperatorError::CredentialScope)),
                "a realm stopped at {version} was not refused as out of scope: {outcome:?}"
            );
            assert!(
                !read_attempted.load(Ordering::SeqCst),
                "the credential was read before the realm at {version} was refused"
            );
            assert!(
                !installed.load(Ordering::SeqCst),
                "an installation was attempted against a realm stopped at {version}"
            );

            let after = realm_fingerprint(&database);
            assert_eq!(after.0, before.0, "the refusal changed bytes at {version}");
            assert_eq!(
                after.1, before.1,
                "the refusal migrated a realm stopped at {version}"
            );
            assert_eq!(
                after.2, before.2,
                "the refusal added or removed schema objects at {version}"
            );
        }
    }

    /// A current Realm gets past the schema gate and fails further in.
    ///
    /// Without this the stopped-schema test above would pass just as well
    /// against an implementation that refused *every* Realm at the schema
    /// boundary — the assertions there are all about what does not happen, and
    /// "nothing happened" is exactly what a blanket refusal produces.
    ///
    /// So this control is arranged to get further: the alias is configured, the
    /// Realm is current, and the input is malformed. That forces a distinct
    /// later failure — `OperatorError::Jira`, raised by the document parser —
    /// and proves the reader was reached, which a refusal at the schema gate
    /// could never do. The installer must still not run: malformed input is
    /// refused before anything is written (ASMA-8187 / ASMA-8015).
    #[test]
    fn a_current_realm_reaches_the_credential_reader_and_fails_after_the_gate() {
        let root = configured_credential_root();
        let database = recovery::database_in(root.path());
        let before = realm_fingerprint(&database);

        let read_attempted = Arc::new(AtomicBool::new(false));
        let installed = Arc::new(AtomicBool::new(false));
        let outcome = install_jira_credential(
            root.path(),
            "work",
            WatchfulReader::new(&read_attempted, b"synthetic-canary"),
            |_scope, _secret| {
                installed.store(true, Ordering::SeqCst);
                Ok(())
            },
        );

        // Past the gate: the failure is the document's, not the schema's.
        assert!(
            matches!(outcome, Err(OperatorError::Jira(_))),
            "a current realm did not reach the credential reader: {outcome:?}"
        );
        assert!(
            read_attempted.load(Ordering::SeqCst),
            "the credential reader was never reached on a current realm, so the \
             stopped-schema assertions prove nothing"
        );
        assert!(
            !installed.load(Ordering::SeqCst),
            "malformed input reached the installer"
        );
        // The refused document is not echoed into the error.
        let message = format!("{:?}", outcome.unwrap_err());
        assert!(!message.contains("synthetic-canary"));

        let after = realm_fingerprint(&database);
        assert_eq!(
            after.1, before.1,
            "a current realm was migrated by a refusal"
        );
        assert_eq!(after.2, before.2);
    }
}
