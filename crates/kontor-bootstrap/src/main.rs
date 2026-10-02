//! `kontor-bootstrap` — install one coherent artifact generation and patch the
//! supported MCP client entries, against explicit injected roots.
//!
//! Every root is a required argument: there is no default home, no service
//! action, and no host operation in this unit. All roots, client selections,
//! specifications and the receipt destination are validated before the first
//! artifact or client effect. The program emits exactly one redacted JSON
//! document per run.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use kontor_bootstrap::client::claude::UnavailableClaudeBoundary;
use kontor_bootstrap::confinement::Dir;
use kontor_bootstrap::{
    AdapterResult, ArtifactManifest, BootstrapReceipt, ClaudeAdapter, ClientAdapter, ClientHome,
    ClientId, ClientReport, FileClientAdapter, Installer, NoFaults, ObservedHash, OwnershipStore,
    ServerSpec,
};

type Failure = (&'static str, String);

/// Install and read back the Kontor artifact set and client entries.
#[derive(Debug, Parser)]
#[command(name = "kontor-bootstrap", version, about)]
struct Arguments {
    /// The action to perform.
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Install or update the artifact generation and the present client entries.
    Install {
        /// The injected install root.
        #[arg(long)]
        install_root: PathBuf,
        /// The directory holding the four release artifacts.
        #[arg(long)]
        source_dir: PathBuf,
        /// The release manifest.
        #[arg(long)]
        manifest: PathBuf,
        /// The injected client home root.
        #[arg(long)]
        home_root: PathBuf,
        /// The injected state root the MCP entry addresses.
        #[arg(long)]
        state_root: PathBuf,
        /// Restrict to named clients; defaults to all five.
        #[arg(long = "client")]
        clients: Vec<String>,
    },
    /// Read the artifact and client state without writing anything.
    Readback {
        /// The injected install root.
        #[arg(long)]
        install_root: PathBuf,
        /// The release manifest.
        #[arg(long)]
        manifest: PathBuf,
        /// The injected client home root.
        #[arg(long)]
        home_root: PathBuf,
        /// The injected state root the MCP entry addresses.
        #[arg(long)]
        state_root: PathBuf,
        /// Restrict to named clients; defaults to all five.
        #[arg(long = "client")]
        clients: Vec<String>,
    },
    /// Replace one refused client entry whose full observed digest is cited.
    Repair {
        /// The injected client home root.
        #[arg(long)]
        home_root: PathBuf,
        /// The injected state root.
        #[arg(long)]
        state_root: PathBuf,
        /// The install root holding the active `current/kontor-mcp`.
        #[arg(long)]
        install_root: PathBuf,
        /// Which client to repair.
        #[arg(long)]
        client: String,
        /// The full observed digest the operator saw.
        #[arg(long)]
        expected_hash: String,
    },
    /// Finish or unwind an interrupted artifact transaction.
    Recover {
        /// The injected install root.
        #[arg(long)]
        install_root: PathBuf,
        /// The release manifest the interrupted transaction targeted.
        #[arg(long)]
        manifest: PathBuf,
    },
}

fn main() -> ExitCode {
    let arguments = Arguments::parse();
    match run(arguments) {
        Ok(document) => {
            println!("{document}");
            ExitCode::SUCCESS
        }
        Err((code, detail)) => {
            let envelope = serde_json::json!({
                "error": {"code": code, "detail": detail}
            });
            println!("{envelope}");
            ExitCode::from(1)
        }
    }
}

fn run(arguments: Arguments) -> Result<String, Failure> {
    match arguments.command {
        Command::Install {
            install_root,
            source_dir,
            manifest,
            home_root,
            state_root,
            clients,
        } => install(
            &install_root,
            &source_dir,
            &manifest,
            &home_root,
            &state_root,
            &clients,
        ),
        Command::Readback {
            install_root,
            manifest,
            home_root,
            state_root,
            clients,
        } => readback(&install_root, &manifest, &home_root, &state_root, &clients),
        Command::Repair {
            home_root,
            state_root,
            install_root,
            client,
            expected_hash,
        } => repair(
            &home_root,
            &state_root,
            &install_root,
            &client,
            &expected_hash,
        ),
        Command::Recover {
            install_root,
            manifest,
        } => recover(&install_root, &manifest),
    }
}

/// Validate one injected root before any effect.
fn validate_root(path: &Path, code: &'static str) -> Result<(), Failure> {
    if !path.is_absolute() {
        return Err((code, "the root must be an absolute path".to_owned()));
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err((code, "the root may not be a symlink".to_owned()))
        }
        Ok(metadata) if !metadata.is_dir() => Err((code, "the root is not a directory".to_owned())),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err((code, "the root cannot be inspected".to_owned())),
    }
}

fn client_list(requested: &[String]) -> Result<Vec<ClientId>, Failure> {
    if requested.is_empty() {
        return Ok(ClientId::ALL.to_vec());
    }
    requested
        .iter()
        .map(|name| {
            ClientId::parse(name)
                .ok_or_else(|| ("unknown_client", format!("unknown client name: {name}")))
        })
        .collect()
}

fn server_spec(installer: &Installer, state_root: &Path) -> ServerSpec {
    ServerSpec {
        program: installer.active_mcp_path(),
        args: vec![
            "--state-root".to_owned(),
            state_root.to_string_lossy().into_owned(),
            "--credential-tier".to_owned(),
            "admin".to_owned(),
        ],
    }
}

fn adapter_for(
    client: ClientId,
    home_root: &Path,
    ownership: &OwnershipStore,
) -> Result<Box<dyn ClientAdapter>, Failure> {
    match client {
        ClientId::ClaudeCode => Ok(Box::new(ClaudeAdapter::new(
            Box::new(UnavailableClaudeBoundary),
            ownership.clone(),
        ))),
        other => {
            let home = ClientHome::at(home_root.to_path_buf()).map_err(client_error)?;
            FileClientAdapter::new(other, &home, ownership.clone())
                .map(|adapter| Box::new(adapter) as Box<dyn ClientAdapter>)
                .map_err(client_error)
        }
    }
}

fn client_error(error: kontor_bootstrap::AdapterError) -> Failure {
    ("client_adapter_failed", error.to_string())
}

fn install(
    install_root: &Path,
    source_dir: &Path,
    manifest_path: &Path,
    home_root: &Path,
    state_root: &Path,
    requested: &[String],
) -> Result<String, Failure> {
    // Validation before any effect, in one place.
    let clients = client_list(requested)?;
    let manifest = ArtifactManifest::read(manifest_path)
        .map_err(|error| ("manifest_invalid", error.to_string()))?;
    validate_root(install_root, "install_root_invalid")?;
    validate_root(home_root, "home_root_invalid")?;
    validate_root(state_root, "state_root_invalid")?;
    let source =
        Dir::open_root(source_dir).map_err(|error| ("source_invalid", error.to_string()))?;
    let sources = manifest
        .verify_sources_in(&source)
        .map_err(|error| ("source_invalid", error.to_string()))?;
    let installer = Installer::at(install_root.to_path_buf())
        .map_err(|error| ("install_root_invalid", error.to_string()))?;
    let spec = server_spec(&installer, state_root);
    spec.validate().map_err(client_error)?;
    let ownership = OwnershipStore::at(state_root.to_path_buf())
        .map_err(|error| ("state_root_invalid", error.to_string()))?;
    // Construct every adapter (a pure validation) before the artifact effect.
    let mut adapters = Vec::with_capacity(clients.len());
    for client in &clients {
        adapters.push((*client, adapter_for(*client, home_root, &ownership)?));
    }

    let outcome = installer
        .install(&manifest, &sources, &source, &mut NoFaults)
        .map_err(|error| ("artifact_install_failed", error.to_string()))?;

    let mut receipt = BootstrapReceipt::from_install(&manifest, outcome);
    for (client, adapter) in &mut adapters {
        let result = adapter
            .install(&spec, &mut NoFaults)
            .map_err(client_error)?;
        receipt.push_client(ClientReport::Applied {
            client: *client,
            result,
        });
    }
    receipt
        .write_to(state_root)
        .map_err(|error| ("receipt_failed", error.to_string()))?;
    receipt
        .to_json()
        .map_err(|error| ("receipt_failed", error.to_string()))
}

fn readback(
    install_root: &Path,
    manifest_path: &Path,
    home_root: &Path,
    state_root: &Path,
    requested: &[String],
) -> Result<String, Failure> {
    let clients = client_list(requested)?;
    let manifest = ArtifactManifest::read(manifest_path)
        .map_err(|error| ("manifest_invalid", error.to_string()))?;
    validate_root(install_root, "install_root_invalid")?;
    validate_root(home_root, "home_root_invalid")?;
    validate_root(state_root, "state_root_invalid")?;
    let installer = Installer::at(install_root.to_path_buf())
        .map_err(|error| ("install_root_invalid", error.to_string()))?;
    let spec = server_spec(&installer, state_root);
    spec.validate().map_err(client_error)?;
    let ownership = OwnershipStore::at(state_root.to_path_buf())
        .map_err(|error| ("state_root_invalid", error.to_string()))?;
    let mut adapters = Vec::with_capacity(clients.len());
    for client in &clients {
        adapters.push((*client, adapter_for(*client, home_root, &ownership)?));
    }

    let statuses = installer
        .status(&manifest)
        .map_err(|error| ("artifact_readback_failed", error.to_string()))?;
    let mut receipt = BootstrapReceipt::from_status(&manifest, &statuses);
    for (client, adapter) in &mut adapters {
        let state = adapter.inspect(&spec).map_err(client_error)?;
        receipt.push_client(ClientReport::Observed {
            client: *client,
            state,
        });
    }
    receipt
        .to_json()
        .map_err(|error| ("receipt_failed", error.to_string()))
}

fn repair(
    home_root: &Path,
    state_root: &Path,
    install_root: &Path,
    client: &str,
    expected_hash: &str,
) -> Result<String, Failure> {
    let client = ClientId::parse(client)
        .ok_or_else(|| ("unknown_client", format!("unknown client name: {client}")))?;
    let expected = ObservedHash::parse(expected_hash).ok_or_else(|| {
        (
            "invalid_hash",
            "the expected hash must be 64 lowercase hex".to_owned(),
        )
    })?;
    validate_root(install_root, "install_root_invalid")?;
    validate_root(home_root, "home_root_invalid")?;
    validate_root(state_root, "state_root_invalid")?;
    let installer = Installer::at(install_root.to_path_buf())
        .map_err(|error| ("install_root_invalid", error.to_string()))?;
    let spec = server_spec(&installer, state_root);
    spec.validate().map_err(client_error)?;
    let ownership = OwnershipStore::at(state_root.to_path_buf())
        .map_err(|error| ("state_root_invalid", error.to_string()))?;
    let mut adapter = adapter_for(client, home_root, &ownership)?;
    let result = adapter
        .repair(&spec, &expected, &mut NoFaults)
        .map_err(client_error)?;
    let report = serde_json::json!({
        "schema_version": 1,
        "client": client,
        "result": result,
    });
    serde_json::to_string_pretty(&report).map_err(|error| ("receipt_failed", error.to_string()))
}

fn recover(install_root: &Path, manifest_path: &Path) -> Result<String, Failure> {
    let manifest = ArtifactManifest::read(manifest_path)
        .map_err(|error| ("manifest_invalid", error.to_string()))?;
    validate_root(install_root, "install_root_invalid")?;
    let installer = Installer::at(install_root.to_path_buf())
        .map_err(|error| ("install_root_invalid", error.to_string()))?;
    let outcome = installer
        .recover(&mut NoFaults)
        .map_err(|error| ("artifact_recovery_failed", error.to_string()))?;
    let statuses = installer
        .status(&manifest)
        .map_err(|error| ("artifact_readback_failed", error.to_string()))?;
    let receipt = BootstrapReceipt::from_status(&manifest, &statuses);
    let document = serde_json::json!({
        "schema_version": 1,
        "recovery": recovery_label(outcome),
        "receipt": receipt,
    });
    serde_json::to_string_pretty(&document).map_err(|error| ("receipt_failed", error.to_string()))
}

fn recovery_label(outcome: kontor_bootstrap::RecoveryOutcome) -> &'static str {
    use kontor_bootstrap::RecoveryOutcome;
    match outcome {
        RecoveryOutcome::Nothing => "nothing",
        RecoveryOutcome::Completed => "completed",
        RecoveryOutcome::RolledBack => "rolled_back",
        RecoveryOutcome::PreservedConflict => "preserved_conflict",
    }
}

#[allow(dead_code)]
fn _assert_result(_: AdapterResult) {}
