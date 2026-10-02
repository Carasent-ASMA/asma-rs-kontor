//! The file-adapter fixture matrix: every supported client, every required
//! interruption and preservation case, over a synthetic home only.

use std::path::{Path, PathBuf};

use kontor_bootstrap::client::file_adapter::{ClientHome, FileClientAdapter};
use kontor_bootstrap::fault::ScriptedFaults;
use kontor_bootstrap::{
    AdapterResult, ClientAdapter, ClientId, ConflictReason, EntryState, FailPoint, NoFaults,
    ObservedHash, ServerSpec,
};

const MANAGED: &str = "/opt/kontor tools/kontor-mcp";
const OTHER: &str = "/usr/local/bin/other-mcp";

fn spec(program: &str, state_root: &str) -> ServerSpec {
    ServerSpec {
        program: PathBuf::from(program),
        args: vec![
            "--state-root".to_owned(),
            state_root.to_owned(),
            "--credential-tier".to_owned(),
            "admin".to_owned(),
        ],
    }
}

struct Fixture {
    _holder: tempfile::TempDir,
    home_root: PathBuf,
    client: ClientId,
    adapter: FileClientAdapter,
}

impl Fixture {
    fn new(client: ClientId) -> Self {
        let holder = tempfile::tempdir().expect("tempdir");
        let home_root = holder.path().join("home with space");
        std::fs::create_dir_all(&home_root).expect("home");
        let home = ClientHome::at(&home_root).expect("client home");
        let adapter = FileClientAdapter::new(client, &home).expect("adapter");
        Self {
            _holder: holder,
            home_root,
            client,
            adapter,
        }
    }

    fn directory(&self) -> PathBuf {
        self.home_root.join(match self.client {
            ClientId::Codex => ".codex",
            ClientId::OpenCode => ".config/opencode",
            ClientId::Copilot => ".copilot",
            ClientId::Cursor => ".cursor",
            ClientId::ClaudeCode => unreachable!(),
        })
    }

    fn file(&self) -> PathBuf {
        match self.client {
            ClientId::Codex => self.directory().join("config.toml"),
            ClientId::OpenCode => self.directory().join("opencode.json"),
            ClientId::Copilot => self.directory().join("mcp-config.json"),
            ClientId::Cursor => self.directory().join("mcp.json"),
            ClientId::ClaudeCode => unreachable!(),
        }
    }

    fn present(&self) {
        std::fs::create_dir_all(self.directory()).expect("client directory");
    }

    fn inspect(&self, spec: &ServerSpec) -> EntryState {
        let mut adapter = self.adapter.clone();
        adapter.inspect(spec).expect("inspect")
    }

    fn install(&self, spec: &ServerSpec) -> AdapterResult {
        let mut adapter = self.adapter.clone();
        adapter.install(spec, &mut NoFaults).expect("install")
    }

    fn repair(&self, spec: &ServerSpec, expected: &ObservedHash) -> AdapterResult {
        let mut adapter = self.adapter.clone();
        adapter
            .repair(spec, expected, &mut NoFaults)
            .expect("repair")
    }

    fn read(&self) -> String {
        let primary = self.file();
        if primary.exists() {
            std::fs::read_to_string(primary).expect("read fixture")
        } else {
            std::fs::read_to_string(self.directory().join("opencode.jsonc"))
                .expect("read fixture candidate")
        }
    }
}

fn shape_entry(fixture: &Fixture, program: &str, args: &[String]) -> serde_json::Value {
    match fixture.client {
        ClientId::Codex => serde_json::json!({"command": program, "args": args}),
        ClientId::OpenCode => {
            let mut command = vec![program.to_owned()];
            command.extend(args.iter().cloned());
            serde_json::json!({"type": "local", "command": command})
        }
        ClientId::Copilot => {
            serde_json::json!({"type": "stdio", "command": program, "args": args})
        }
        ClientId::Cursor => serde_json::json!({"command": program, "args": args}),
        ClientId::ClaudeCode => unreachable!(),
    }
}

fn assert_entry_shape(fixture: &Fixture, spec: &ServerSpec) {
    let text = fixture.read();
    match fixture.client {
        ClientId::Codex => {
            let document = text.parse::<toml_edit::DocumentMut>().expect("valid TOML");
            let entry = &document["mcp_servers"]["kontor"];
            assert_eq!(
                entry["command"].as_str(),
                Some(spec.program.to_str().unwrap())
            );
            let args: Vec<&str> = entry["args"]
                .as_array()
                .expect("args")
                .iter()
                .map(|value| value.as_str().expect("string"))
                .collect();
            assert_eq!(
                args,
                spec.args.iter().map(String::as_str).collect::<Vec<_>>()
            );
        }
        ClientId::OpenCode => {
            let document = kontor_bootstrap::client::json_edit::JsonDocument::parse(
                text,
                kontor_bootstrap::client::json_edit::Dialect::Jsonc,
            )
            .expect("valid JSONC");
            assert_eq!(
                document
                    .value(&["mcp", "servers", "kontor"])
                    .expect("entry"),
                shape_entry(fixture, spec.program.to_str().unwrap(), &spec.args)
            );
        }
        ClientId::Copilot | ClientId::Cursor => {
            let document: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
            let value = match fixture.client {
                ClientId::Copilot => &document["servers"]["kontor"],
                ClientId::Cursor => &document["mcpServers"]["kontor"],
                _ => unreachable!(),
            };
            assert_eq!(
                *value,
                shape_entry(fixture, spec.program.to_str().unwrap(), &spec.args)
            );
        }
        ClientId::ClaudeCode => unreachable!(),
    }
}

fn clean_install(client: ClientId) {
    let fixture = Fixture::new(client);
    fixture.present();
    let desired = spec(MANAGED, "/synthetic/realm");
    assert_eq!(fixture.install(&desired), AdapterResult::Installed);
    assert_entry_shape(&fixture, &desired);
    assert_eq!(
        fixture.inspect(&desired),
        EntryState::Current {
            observed: ObservedHash::of_entry(MANAGED, &desired.args)
        }
    );
}

#[test]
fn every_file_client_installs_cleanly() {
    for client in FileClientAdapter::FILE_CLIENTS {
        clean_install(client);
    }
}

#[test]
fn every_file_client_updates_and_replays() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        fixture.present();
        let first = spec(MANAGED, "/synthetic/one");
        let second = spec(MANAGED, "/synthetic/two");
        assert_eq!(fixture.install(&first), AdapterResult::Installed);
        let after_first = fixture.read();
        assert_eq!(fixture.install(&second), AdapterResult::Updated);
        assert_entry_shape(&fixture, &second);
        let after_update = fixture.read();
        assert_eq!(fixture.install(&second), AdapterResult::AlreadyCurrent);
        assert_eq!(fixture.read(), after_update, "replay is write-free");
        assert_ne!(after_first, after_update);
    }
}

#[test]
fn every_file_client_refuses_a_same_name_unrelated_entry_and_repairs_only_with_its_hash() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        fixture.present();
        let desired = spec(MANAGED, "/synthetic/realm");
        seed_unrelated(&fixture);
        let observed = match fixture.inspect(&desired) {
            EntryState::Unrelated { observed } => observed,
            other => panic!("{client}: expected unrelated, got {other:?}"),
        };
        assert_eq!(
            fixture.install(&desired),
            AdapterResult::Conflict {
                reason: ConflictReason::SameNameUnrelated,
                observed: Some(observed.clone()),
            }
        );
        assert_eq!(
            fixture.repair(
                &desired,
                &ObservedHash::parse(&"a".repeat(64)).expect("hash")
            ),
            AdapterResult::Conflict {
                reason: ConflictReason::ObservedHashMismatch,
                observed: Some(observed.clone()),
            }
        );
        assert_eq!(fixture.repair(&desired, &observed), AdapterResult::Repaired);
        assert_entry_shape(&fixture, &desired);
    }
}

#[test]
fn every_file_client_reports_an_absent_client_without_creating_one() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        let desired = spec(MANAGED, "/synthetic/realm");
        assert_eq!(fixture.inspect(&desired), EntryState::ClientAbsent);
        assert_eq!(fixture.install(&desired), AdapterResult::Absent);
        assert!(!fixture.directory().exists());
        assert_eq!(
            fixture.repair(
                &desired,
                &ObservedHash::parse(&"b".repeat(64)).expect("hash")
            ),
            AdapterResult::Absent
        );
    }
}

#[test]
fn every_file_client_fails_readback_when_the_write_vanishes() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        fixture.present();
        let desired = spec(MANAGED, "/synthetic/realm");
        let mut faults = ScriptedFaults::observing(FailPoint::AfterClientWrite(client), |path| {
            std::fs::write(path, "no longer a document")
                .map_err(|_| kontor_bootstrap::InstallError::Io)
        });
        let mut adapter = fixture.adapter.clone();
        assert_eq!(
            adapter.install(&desired, &mut faults).expect("install"),
            AdapterResult::FailedReadback
        );
    }
}

#[test]
fn every_file_client_preserves_unrelated_configuration() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        fixture.present();
        let original = rich_config(client);
        let path = if client == ClientId::OpenCode {
            fixture.directory().join("opencode.jsonc")
        } else {
            fixture.file()
        };
        std::fs::write(&path, &original).expect("seed");
        let desired = spec(MANAGED, "/synthetic/realm");
        assert_eq!(fixture.install(&desired), AdapterResult::Installed);
        let installed = std::fs::read_to_string(&path).expect("read installed");
        for fragment in unrelated_fragments(client) {
            assert!(
                installed.contains(fragment),
                "{client}: lost {fragment:?} from\n{installed}"
            );
        }
        assert_entry_shape(&fixture, &desired);
        // The original document still parses as the same unrelated structure.
        assert_unrelated_intact(client, &installed);
    }
}

#[cfg(unix)]
#[test]
fn every_file_client_refuses_a_symlinked_configuration_file() {
    use std::os::unix::fs::symlink;
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        fixture.present();
        let outside = fixture._holder.path().join("outside-config");
        std::fs::write(&outside, "outside bytes").expect("outside");
        symlink(&outside, fixture.file()).expect("symlink");
        let desired = spec(MANAGED, "/synthetic/realm");
        assert_eq!(
            fixture.inspect(&desired),
            EntryState::Refused {
                reason: ConflictReason::Symlink
            }
        );
        assert_eq!(
            fixture.install(&desired),
            AdapterResult::Conflict {
                reason: ConflictReason::Symlink,
                observed: None
            }
        );
        assert_eq!(
            std::fs::read_to_string(&outside).expect("read outside"),
            "outside bytes"
        );
    }
}

#[test]
fn every_file_client_refuses_a_concurrent_writer() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        fixture.present();
        let first = spec(MANAGED, "/synthetic/one");
        let second = spec(MANAGED, "/synthetic/two");
        assert_eq!(fixture.install(&first), AdapterResult::Installed);
        let mut faults = ScriptedFaults::observing(FailPoint::BeforeClientWrite(client), |path| {
            std::fs::write(path, "foreign writer").map_err(|_| kontor_bootstrap::InstallError::Io)
        });
        let mut adapter = fixture.adapter.clone();
        match adapter.install(&second, &mut faults).expect("install") {
            AdapterResult::Conflict {
                reason: ConflictReason::ConcurrentChange,
                observed: Some(_),
            } => {}
            other => panic!("{client}: expected a concurrent-change conflict, got {other:?}"),
        }
        assert_eq!(fixture.read(), "foreign writer");
    }
}

#[test]
fn opencode_refuses_an_ambiguous_or_legacy_surface() {
    let fixture = Fixture::new(ClientId::OpenCode);
    fixture.present();
    std::fs::write(fixture.directory().join("opencode.json"), "{}").expect("json");
    std::fs::write(fixture.directory().join("opencode.jsonc"), "{}").expect("jsonc");
    let desired = spec(MANAGED, "/synthetic/realm");
    assert_eq!(
        fixture.inspect(&desired),
        EntryState::Refused {
            reason: ConflictReason::AmbiguousConfig
        }
    );
    assert_eq!(
        fixture.install(&desired),
        AdapterResult::Conflict {
            reason: ConflictReason::AmbiguousConfig,
            observed: None
        }
    );

    std::fs::remove_file(fixture.directory().join("opencode.jsonc")).expect("remove");
    std::fs::write(
        fixture.directory().join("opencode.json"),
        r#"{"mcpServers": {"kontor": {"command": "old"}}}"#,
    )
    .expect("legacy");
    assert_eq!(
        fixture.inspect(&desired),
        EntryState::Refused {
            reason: ConflictReason::LegacySchema
        }
    );

    std::fs::write(fixture.directory().join("opencode.json"), r#"{"mcp": []}"#)
        .expect("legacy mcp");
    assert_eq!(
        fixture.inspect(&desired),
        EntryState::Refused {
            reason: ConflictReason::LegacySchema
        }
    );
}

#[test]
fn opencode_installs_into_a_jsonc_document_with_comments() {
    let fixture = Fixture::new(ClientId::OpenCode);
    fixture.present();
    let original = "{\n  // operator note\n  \"theme\": \"dark\",\n}\n";
    let path = fixture.directory().join("opencode.jsonc");
    std::fs::write(&path, original).expect("seed");
    let desired = spec(MANAGED, "/synthetic/realm");
    assert_eq!(fixture.install(&desired), AdapterResult::Installed);
    let installed = std::fs::read_to_string(&path).expect("read jsonc");
    assert!(installed.contains("// operator note"));
    assert!(installed.contains("\"theme\": \"dark\""));
    let parsed = kontor_bootstrap::client::json_edit::JsonDocument::parse(
        installed.clone(),
        kontor_bootstrap::client::json_edit::Dialect::Jsonc,
    )
    .expect("parses as JSONC");
    assert_eq!(parsed.value(&["theme"]), Some(serde_json::json!("dark")));
    assert_eq!(
        parsed
            .value(&["mcp", "servers", "kontor", "command"])
            .expect("command")[0],
        serde_json::json!(MANAGED)
    );
    // The sibling `.json` candidate was never created.
    assert!(!fixture.directory().join("opencode.json").exists());
}

fn seed_unrelated(fixture: &Fixture) {
    fixture.present();
    let text = match fixture.client {
        ClientId::Codex => {
            format!("[mcp_servers.kontor]\ncommand = \"{OTHER}\"\nargs = [\"--own\"]\n")
        }
        ClientId::OpenCode => serde_json::json!({
            "mcp": {"servers": {"kontor": shape_entry(fixture, OTHER, &["--own".to_owned()])}}
        })
        .to_string(),
        ClientId::Copilot => serde_json::json!({
            "servers": {"kontor": shape_entry(fixture, OTHER, &["--own".to_owned()])}
        })
        .to_string(),
        ClientId::Cursor => serde_json::json!({
            "mcpServers": {"kontor": shape_entry(fixture, OTHER, &["--own".to_owned()])}
        })
        .to_string(),
        ClientId::ClaudeCode => unreachable!(),
    };
    std::fs::write(fixture.file(), text).expect("seed unrelated");
}

fn rich_config(client: ClientId) -> String {
    match client {
        ClientId::Codex => "# operator comment\nmodel = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"/usr/bin/other\"\nargs = [\"a\", \"b\"]\n\n[other_section]\nkeep = true\n".to_owned(),
        ClientId::OpenCode => "{\n  // operator note\n  \"theme\": \"dark\",\n  \"mcp\": {\n    \"other\": { \"type\": \"remote\", \"url\": \"https://example.invalid\" }\n  },\n  \"keep\": [1, 2, 3],\n}\n".to_owned(),
        ClientId::Copilot => "{\n  \"servers\": {\n    \"other\": { \"type\": \"stdio\", \"command\": \"/usr/bin/other\", \"args\": [\"a\"] }\n  },\n  \"unrelated\": true\n}\n".to_owned(),
        ClientId::Cursor => "{\n  \"mcpServers\": {\n    \"other\": { \"command\": \"/usr/bin/other\", \"args\": [\"a\"] }\n  },\n  \"unrelated\": 7\n}\n".to_owned(),
        ClientId::ClaudeCode => unreachable!(),
    }
}

fn unrelated_fragments(client: ClientId) -> Vec<&'static str> {
    match client {
        ClientId::Codex => vec![
            "# operator comment",
            "model = \"gpt-5\"",
            "[mcp_servers.other]",
            "command = \"/usr/bin/other\"",
            "[other_section]",
            "keep = true",
        ],
        ClientId::OpenCode => vec![
            "// operator note",
            "\"theme\": \"dark\"",
            "\"type\": \"remote\"",
            "https://example.invalid",
            "\"keep\": [1, 2, 3]",
        ],
        ClientId::Copilot => vec![
            "\"/usr/bin/other\"",
            "\"unrelated\": true",
            "\"type\": \"stdio\"",
        ],
        ClientId::Cursor => vec!["\"/usr/bin/other\"", "\"unrelated\": 7"],
        ClientId::ClaudeCode => Vec::new(),
    }
}

fn assert_unrelated_intact(client: ClientId, installed: &str) {
    match client {
        ClientId::Codex => {
            let document = installed.parse::<toml_edit::DocumentMut>().expect("toml");
            assert_eq!(document["model"].as_str(), Some("gpt-5"));
            assert_eq!(document["other_section"]["keep"].as_bool(), Some(true));
            assert_eq!(
                document["mcp_servers"]["other"]["command"].as_str(),
                Some("/usr/bin/other")
            );
        }
        ClientId::OpenCode => {
            let document = kontor_bootstrap::client::json_edit::JsonDocument::parse(
                installed.to_owned(),
                kontor_bootstrap::client::json_edit::Dialect::Jsonc,
            )
            .expect("jsonc");
            assert_eq!(document.value(&["theme"]), Some(serde_json::json!("dark")));
            assert_eq!(
                document.value(&["mcp", "other", "url"]),
                Some(serde_json::json!("https://example.invalid"))
            );
            assert_eq!(
                document.value(&["keep"]),
                Some(serde_json::json!([1, 2, 3]))
            );
        }
        ClientId::Copilot => {
            let document: serde_json::Value = serde_json::from_str(installed).expect("json");
            assert_eq!(document["unrelated"], true);
            assert_eq!(document["servers"]["other"]["command"], "/usr/bin/other");
        }
        ClientId::Cursor => {
            let document: serde_json::Value = serde_json::from_str(installed).expect("json");
            assert_eq!(document["unrelated"], 7);
            assert_eq!(document["mcpServers"]["other"]["command"], "/usr/bin/other");
        }
        ClientId::ClaudeCode => {}
    }
}

#[test]
fn a_relative_client_home_is_refused() {
    assert!(ClientHome::at(Path::new("relative")).is_err());
}

#[cfg(unix)]
#[test]
fn every_file_client_refuses_a_symlinked_client_directory() {
    use std::os::unix::fs::symlink;
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        let outside = fixture._holder.path().join("outside-directory");
        std::fs::create_dir_all(&outside).expect("outside dir");
        std::fs::write(outside.join("config.toml"), "outside = true").expect("outside file");
        let directory = fixture.directory();
        if let Some(parent) = directory.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        symlink(&outside, &directory).expect("symlink directory");
        let desired = spec(MANAGED, "/synthetic/realm");
        assert_eq!(
            fixture.inspect(&desired),
            EntryState::Refused {
                reason: ConflictReason::Symlink
            },
            "{client}"
        );
        assert_eq!(
            fixture.install(&desired),
            AdapterResult::Conflict {
                reason: ConflictReason::Symlink,
                observed: None
            },
            "{client}"
        );
        assert_eq!(
            std::fs::read_to_string(outside.join("config.toml")).expect("read"),
            "outside = true"
        );
    }
}
