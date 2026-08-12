//! The `kontor-daemon` executable: one Realm, one loopback socket.
//!
//! Everything reusable lives in the library, so this file is only the three things
//! a binary owns — argument parsing, binding the socket, and the signal that ends
//! the process.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;
use kontor_api::state::BarrierState;
use kontor_daemon::{DEFAULT_PORT, Daemon, DaemonConfig, endpoint, runtimes};
use tracing::{error, info, warn};

/// Serve one Kontor realm on loopback.
#[derive(Debug, Parser)]
#[command(name = "kontor-daemon", version, about)]
struct Arguments {
    /// The state root holding this realm's database, lock and credentials.
    #[arg(long)]
    state_root: PathBuf,
    /// The loopback port to bind.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,
    /// A browser origin to answer, repeatable. Defaults to the desktop shell's.
    #[arg(long = "origin")]
    origins: Vec<String>,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let arguments = Arguments::parse();

    let mut config = DaemonConfig::at(arguments.state_root).with_port(arguments.port);
    if !arguments.origins.is_empty() {
        config.allowed_origins = arguments.origins;
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

    if daemon.reconcile().await != BarrierState::Open {
        // Serving with the barrier shut is deliberate: health, identity, snapshots
        // and the event feed all still answer, and they are exactly what an
        // operator needs to see *why* scheduling is shut.
        error!(
            realm_id = %daemon.realm_id(),
            "startup reconciliation did not complete; scheduling stays shut"
        );
    }

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
        .await;
    daemon.shutdown();
    match served {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            error!(detail = %error, "the loopback server stopped with an error");
            std::process::ExitCode::FAILURE
        }
    }
}
