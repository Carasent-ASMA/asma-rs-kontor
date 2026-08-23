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
    DEFAULT_PORT, Daemon, DaemonConfig, endpoint, logging, recovery, runtimes, usage,
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
fn run(state_root: &Path, command: Command) -> Result<(), recovery::RecoveryError> {
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
            for snapshot in kontor_store::backup::list_snapshots(&directory, store.realm_id())? {
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
            let bytes = export.canonical_bytes()?;
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
        Command::RotateCredentials => recovery::rotate_credentials(state_root),
    }
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
        usage::UsagePoller::discover(&daemon.config().state_root),
        daemon.state(),
    ));

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
