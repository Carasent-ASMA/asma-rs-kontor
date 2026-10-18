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
use kontor_bootstrap::confinement::{
    ConfinementError, Dir, effective_user, is_private_directory, normalized_root_path,
};
use kontor_bootstrap::{
    AdapterResult, ArtifactManifest, BootstrapReceipt, ClaudeAdapter, ClientAdapter, ClientFailure,
    ClientHome, ClientId, ClientReport, FileClientAdapter, InstallError, Installer, NoFaults,
    ObservedHash, OwnershipStore, ServerSpec,
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

/// Which injected root is being validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootScope {
    /// The install root, which holds the executables every client entry names.
    Install,
    /// The state root, which holds the ownership ledger and credentials.
    State,
    /// The client home root.
    Home,
}

/// Validate one injected root before any effect.
fn validate_root(path: &Path, code: &'static str, scope: RootScope) -> Result<(), Failure> {
    let path = normalized_root_path(path);
    if !path.is_absolute() {
        return Err((code, "the root must be an absolute path".to_owned()));
    }
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err((code, "the root may not be a symlink".to_owned()))
        }
        Ok(metadata) if !metadata.is_dir() => Err((code, "the root is not a directory".to_owned())),
        Ok(_) => match scope {
            RootScope::Install | RootScope::State => {
                validate_private_root(&path, code, scope == RootScope::Install)
            }
            RootScope::Home => Ok(()),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err((code, "the root cannot be inspected".to_owned())),
    }
}

/// Refuse a root this process does not own or that another local user can
/// write. The check runs on the descriptor opened here. A missing root can be
/// created by another local user before the writer opens it, so the artifact
/// installer, the ownership store and the receipt writer repeat the same
/// owner-and-mode check on the descriptor they actually use.
fn validate_private_root(
    path: &Path,
    code: &'static str,
    with_releases: bool,
) -> Result<(), Failure> {
    let dir =
        Dir::open_root(path).map_err(|_| (code, "the root cannot be inspected".to_owned()))?;
    require_private_directory(&dir, code)?;
    if with_releases {
        match dir.open_child("releases") {
            Ok(releases) => require_private_directory(&releases, code)?,
            Err(ConfinementError::Missing) => {}
            Err(_) => {
                return Err((
                    code,
                    "the releases directory cannot be inspected".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn require_private_directory(dir: &Dir, code: &'static str) -> Result<(), Failure> {
    let (owner, mode) = dir
        .owner_and_mode()
        .map_err(|_| (code, "the root cannot be inspected".to_owned()))?;
    if owner != effective_user() {
        return Err((
            code,
            "the root must be owned by the effective user".to_owned(),
        ));
    }
    if mode & 0o022 != 0 {
        return Err((
            code,
            "the root must not be writable by group or other users".to_owned(),
        ));
    }
    if !is_private_directory(dir)
        .map_err(|_| (code, "the root cannot be inspected".to_owned()))?
    {
        return Err((
            code,
            "the root must not be writable by another local user".to_owned(),
        ));
    }
    Ok(())
}

fn client_list(requested: &[String]) -> Result<Vec<ClientId>, Failure> {
    if requested.is_empty() {
        return Ok(ClientId::ALL.to_vec());
    }
    requested
        .iter()
        .map(|name| {
            ClientId::parse(name).ok_or((
                "unknown_client",
                "the requested client is not one of the five supported clients".to_owned(),
            ))
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
    validate_root(install_root, "install_root_invalid", RootScope::Install)?;
    validate_root(home_root, "home_root_invalid", RootScope::Home)?;
    validate_root(state_root, "state_root_invalid", RootScope::State)?;
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
        .map_err(|error| match error {
            InstallError::InsecureRoot => ("install_root_invalid", error.to_string()),
            _ => ("artifact_install_failed", error.to_string()),
        })?;

    let mut receipt = BootstrapReceipt::from_install(&manifest, outcome);
    let mut first_failure = None;
    for (client, adapter) in &mut adapters {
        match adapter.install(&spec, &mut NoFaults) {
            Ok(result) => receipt.push_client(ClientReport::Applied {
                client: *client,
                result,
            }),
            Err(error) => {
                let reason = ClientFailure::from(error);
                if first_failure.is_none() {
                    first_failure = Some(reason);
                }
                receipt.push_client(ClientReport::Failed {
                    client: *client,
                    reason,
                });
            }
        }
    }
    receipt
        .write_to(state_root)
        .map_err(|error| ("receipt_failed", error.to_string()))?;
    let document = receipt
        .to_json()
        .map_err(|error| ("receipt_failed", error.to_string()))?;
    if let Some(reason) = first_failure {
        return Err(("client_adapter_failed", reason.as_str().to_owned()));
    }
    Ok(document)
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
    validate_root(install_root, "install_root_invalid", RootScope::Install)?;
    validate_root(home_root, "home_root_invalid", RootScope::Home)?;
    validate_root(state_root, "state_root_invalid", RootScope::State)?;
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
    let client = ClientId::parse(client).ok_or((
        "unknown_client",
        "the requested client is not one of the five supported clients".to_owned(),
    ))?;
    let expected = ObservedHash::parse(expected_hash).ok_or_else(|| {
        (
            "invalid_hash",
            "the expected hash must be 64 lowercase hex".to_owned(),
        )
    })?;
    validate_root(install_root, "install_root_invalid", RootScope::Install)?;
    validate_root(home_root, "home_root_invalid", RootScope::Home)?;
    validate_root(state_root, "state_root_invalid", RootScope::State)?;
    let installer = Installer::at(install_root.to_path_buf())
        .map_err(|error| ("install_root_invalid", error.to_string()))?;
    installer
        .verify_active_for_execution()
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
    validate_root(install_root, "install_root_invalid", RootScope::Install)?;
    let installer = Installer::at(install_root.to_path_buf())
        .map_err(|error| ("install_root_invalid", error.to_string()))?;
    let outcome = installer
        .recover(&mut NoFaults)
        .map_err(|error| match error {
            InstallError::InsecureRoot => ("install_root_invalid", error.to_string()),
            _ => ("artifact_recovery_failed", error.to_string()),
        })?;
    let statuses = installer
        .status(&manifest)
        .map_err(|error| ("artifact_readback_failed", error.to_string()))?;
    let receipt = BootstrapReceipt::from_status(&manifest, &statuses);
    // The receipt's validated `to_json` is the only serialization path.
    let receipt_value: serde_json::Value = serde_json::from_str(
        &receipt
            .to_json()
            .map_err(|error| ("receipt_failed", error.to_string()))?,
    )
    .map_err(|error| ("receipt_failed", error.to_string()))?;
    let document = serde_json::json!({
        "schema_version": 1,
        "recovery": recovery_label(outcome),
        "receipt": receipt_value,
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
