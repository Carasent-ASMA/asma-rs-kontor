//! Maintain startup policy only inside this realm's owned provider homes.
//!
//! Codebase memory is an optional accelerator. Its connection pool or startup
//! failure must never prevent an otherwise healthy Codex seat from starting.
//! A Codex consultation's scoped bearer reaches the app-server process as
//! `KONTOR_AUTH`; Codex forwards ambient variables to stdio MCP children only
//! when the server explicitly names them in `env_vars`. Reconcile that name
//! without ever placing the bearer value in configuration.

use std::io::{self, Write};
use std::path::Path;

const CONSULTATION_AUTH_ENV: &str = "KONTOR_AUTH";

fn reconciled_provider_config(source: &str) -> io::Result<Option<String>> {
    let mut document = source
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid provider TOML"))?;
    let mut changed = false;
    let Some(servers) = document.get_mut("mcp_servers") else {
        return Ok(None);
    };
    let servers = servers
        .as_table_like_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid MCP servers table"))?;
    if let Some(server) = servers.get_mut("codebase-memory-mcp") {
        let server = server.as_table_like_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid memory server table")
        })?;
        if server.get("required").and_then(toml_edit::Item::as_bool) == Some(true) {
            server.insert("required", toml_edit::value(false));
            changed = true;
        }
    }
    if let Some(server) = servers.get_mut("kontor") {
        let server = server.as_table_like_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid Kontor server table")
        })?;
        match server.get_mut("env_vars") {
            None => {
                let mut names = toml_edit::Array::new();
                names.push(CONSULTATION_AUTH_ENV);
                server.insert("env_vars", toml_edit::value(names));
                changed = true;
            }
            Some(item) => {
                let names = item.as_array_mut().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid Kontor server env_vars")
                })?;
                if !names
                    .iter()
                    .any(|name| name.as_str() == Some(CONSULTATION_AUTH_ENV))
                {
                    names.push(CONSULTATION_AUTH_ENV);
                    changed = true;
                }
            }
        }
    }
    Ok(changed.then(|| document.to_string()))
}

/// Reconcile registered Codex homes, preserving comments and all other servers.
/// Symlinked homes/configs are not realm-owned and are left alone.
pub fn reconcile(state_root: &Path) -> io::Result<usize> {
    let root = state_root.join("provider-homes");
    if std::fs::symlink_metadata(&root).is_ok_and(|metadata| metadata.is_symlink()) {
        return Err(io::Error::other(
            "provider homes must be owned by the realm",
        ));
    }
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut changed = 0;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !(name == "codex" || name.starts_with("codex-")) || !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path().join("config.toml");
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => continue,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let original = std::fs::read_to_string(&path)?;
        let Some(updated) = reconciled_provider_config(&original)? else {
            continue;
        };
        let temporary = path.with_extension(format!("kontor-{}.tmp", uuid::Uuid::now_v7()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.set_permissions(metadata.permissions())?;
            file.write_all(updated.as_bytes())?;
            file.sync_all()?;
            if std::fs::read_to_string(&path)? != original {
                return Err(io::Error::other(
                    "provider config changed during reconciliation",
                ));
            }
            std::fs::rename(&temporary, &path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result?;
        changed += 1;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_provider_policy_changes_once_and_preserves_other_values() {
        let input = "# operator comment\nmodel = 'example'\n[mcp_servers.codebase-memory-mcp]\ncommand = '/bin/memory'\nrequired = true\n[mcp_servers.kontor]\nrequired = true\n";
        let updated = reconciled_provider_config(input).unwrap().unwrap();
        assert!(updated.starts_with("# operator comment\nmodel = 'example'"));
        let parsed = updated.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(
            parsed["mcp_servers"]["codebase-memory-mcp"]["required"].as_bool(),
            Some(false)
        );
        assert_eq!(
            parsed["mcp_servers"]["kontor"]["required"].as_bool(),
            Some(true)
        );
        assert_eq!(
            parsed["mcp_servers"]["kontor"]["env_vars"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(toml_edit::Value::as_str)
                .collect::<Vec<_>>(),
            vec![CONSULTATION_AUTH_ENV]
        );
        assert!(reconciled_provider_config(&updated).unwrap().is_none());
        assert!(
            reconciled_provider_config("model = 'example'\n")
                .unwrap()
                .is_none()
        );
        assert!(reconciled_provider_config("not valid = [").is_err());
    }

    #[test]
    fn an_existing_kontor_environment_allowlist_is_extended_without_replacement() {
        let input = "[mcp_servers.kontor]\nenv_vars = ['EXISTING']\n";
        let updated = reconciled_provider_config(input).unwrap().unwrap();
        let parsed = updated.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(
            parsed["mcp_servers"]["kontor"]["env_vars"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(toml_edit::Value::as_str)
                .collect::<Vec<_>>(),
            vec!["EXISTING", CONSULTATION_AUTH_ENV]
        );
        assert!(reconciled_provider_config(&updated).unwrap().is_none());
        assert!(reconciled_provider_config("[mcp_servers.kontor]\nenv_vars = false\n").is_err());
    }

    #[test]
    fn reconciliation_is_limited_to_owned_codex_configs() {
        let root = tempfile::tempdir().unwrap();
        for alias in ["codex-personal", "claude-work"] {
            let home = root.path().join("provider-homes").join(alias);
            std::fs::create_dir_all(&home).unwrap();
            std::fs::write(
                home.join("config.toml"),
                "[mcp_servers.codebase-memory-mcp]\nrequired = true\n",
            )
            .unwrap();
        }
        assert_eq!(reconcile(root.path()).unwrap(), 1);
        assert_eq!(reconcile(root.path()).unwrap(), 0);
        assert!(
            std::fs::read_to_string(root.path().join("provider-homes/claude-work/config.toml"))
                .unwrap()
                .contains("true")
        );
    }
    #[cfg(unix)]
    #[test]
    fn symlinked_homes_and_configs_are_never_rewritten() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source = "[mcp_servers.codebase-memory-mcp]\nrequired = true\n";
        std::fs::write(outside.path().join("config.toml"), source).unwrap();
        let homes = root.path().join("provider-homes");
        std::fs::create_dir_all(homes.join("codex-config-link")).unwrap();
        symlink(outside.path(), homes.join("codex-home-link")).unwrap();
        symlink(
            outside.path().join("config.toml"),
            homes.join("codex-config-link/config.toml"),
        )
        .unwrap();
        assert_eq!(reconcile(root.path()).unwrap(), 0);
        assert_eq!(
            std::fs::read_to_string(outside.path().join("config.toml")).unwrap(),
            source
        );
    }
}
