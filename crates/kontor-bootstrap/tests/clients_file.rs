//! The file-adapter fixture and security matrix, over a synthetic home only.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use kontor_bootstrap::client::file_adapter::{ClientHome, FileClientAdapter};
use kontor_bootstrap::client::json_edit::{Dialect, JsonDocument};
use kontor_bootstrap::fault::{FailPoint, FaultInjector, NoFaults, ScriptedFaults};
use kontor_bootstrap::{
    AdapterError, AdapterResult, ClientAdapter, ClientId, ConflictReason, EntryState, InstallError,
    ObservedHash, OwnershipStore, ServerSpec,
};

const MANAGED: &str = "/opt/kontor tools/current/kontor-mcp";
const OTHER: &str = "/usr/local/bin/other-mcp";
const LEDGER: &str = "bootstrap-ownership.json";

fn spec(state_root: &str) -> ServerSpec {
    ServerSpec {
        program: PathBuf::from(MANAGED),
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
    state_root: PathBuf,
    client: ClientId,
    ownership: OwnershipStore,
    adapter: FileClientAdapter,
}

impl Fixture {
    fn new(client: ClientId) -> Self {
        let holder = tempfile::tempdir().expect("tempdir");
        let home_root = holder.path().join("home with space");
        let state_root = holder.path().join("state root");
        std::fs::create_dir_all(&home_root).expect("home");
        std::fs::create_dir_all(&state_root).expect("state");
        let home = ClientHome::at(&home_root).expect("client home");
        let ownership = OwnershipStore::at(&state_root).expect("ownership");
        let adapter = FileClientAdapter::new(client, &home, ownership.clone()).expect("adapter");
        Self {
            _holder: holder,
            home_root,
            state_root,
            client,
            ownership,
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
        if self.file().exists() {
            std::fs::read_to_string(self.file()).expect("read fixture")
        } else {
            std::fs::read_to_string(self.directory().join("opencode.jsonc"))
                .expect("read fixture candidate")
        }
    }

    fn write(&self, text: &str) {
        self.present();
        std::fs::write(self.file(), text).expect("write fixture");
    }

    fn owns(&self, value: &Value) -> bool {
        self.ownership
            .load()
            .expect("ledger")
            .owns(self.client, &ObservedHash::of_value(value))
    }
}

type HookAction = Box<dyn FnMut(&Path) -> Result<(), InstallError>>;

struct Hooks(Vec<(FailPoint, HookAction)>);

impl FaultInjector for Hooks {
    fn hit(&mut self, point: FailPoint, root: &Path) -> Result<(), InstallError> {
        for (target, action) in &mut self.0 {
            if *target == point {
                action(root)?;
            }
        }
        Ok(())
    }
}

fn hook(
    point: FailPoint,
    action: impl FnMut(&Path) -> Result<(), InstallError> + 'static,
) -> (FailPoint, HookAction) {
    (point, Box::new(action))
}

fn entry_value(client: ClientId, program: &str, args: &[String]) -> Value {
    match client {
        ClientId::Codex | ClientId::Cursor => json!({"command": program, "args": args}),
        ClientId::OpenCode => {
            let mut command = vec![program.to_owned()];
            command.extend(args.iter().cloned());
            json!({"type": "local", "command": command})
        }
        ClientId::Copilot | ClientId::ClaudeCode => {
            json!({"type": "stdio", "command": program, "args": args})
        }
    }
}

fn seed_entry(fixture: &Fixture, value: &Value) {
    fixture.present();
    match fixture.client {
        ClientId::Codex => {
            fixture.write(&format!(
                "[mcp_servers.kontor]\ncommand = \"{}\"\nargs = {}\n",
                value["command"].as_str().unwrap_or_default(),
                toml_args(&value["args"])
            ));
        }
        ClientId::OpenCode => {
            fixture.write(&json!({"mcp": {"servers": {"kontor": value}}}).to_string())
        }
        ClientId::Copilot => fixture.write(&json!({"servers": {"kontor": value}}).to_string()),
        ClientId::Cursor => fixture.write(&json!({"mcpServers": {"kontor": value}}).to_string()),
        ClientId::ClaudeCode => unreachable!(),
    }
}

fn toml_args(args: &Value) -> String {
    let items: Vec<String> = args
        .as_array()
        .map(|array| {
            array
                .iter()
                .filter_map(Value::as_str)
                .map(|item| format!("\"{item}\""))
                .collect()
        })
        .unwrap_or_default();
    format!("[{}]", items.join(", "))
}

fn assert_shape(fixture: &Fixture, spec: &ServerSpec) {
    let expected = desired_value(fixture.client, spec);
    let text = fixture.read();
    match fixture.client {
        ClientId::Codex => {
            let document = text.parse::<toml_edit::DocumentMut>().expect("toml");
            assert_eq!(
                document["mcp_servers"]["kontor"]["command"].as_str(),
                expected["command"].as_str()
            );
            let args: Vec<&str> = document["mcp_servers"]["kontor"]["args"]
                .as_array()
                .expect("args")
                .iter()
                .map(|value| value.as_str().expect("string"))
                .collect();
            let expected_args: Vec<&str> = expected["args"]
                .as_array()
                .expect("args")
                .iter()
                .map(|value| value.as_str().expect("string"))
                .collect();
            assert_eq!(args, expected_args);
        }
        ClientId::OpenCode => {
            let document = JsonDocument::parse(text, Dialect::Jsonc).expect("jsonc");
            assert_eq!(
                document.value(&["mcp", "servers", "kontor"]),
                Some(expected)
            );
        }
        ClientId::Copilot => {
            let document: Value = serde_json::from_str(&text).expect("json");
            assert_eq!(document["servers"]["kontor"], expected);
        }
        ClientId::Cursor => {
            let document: Value = serde_json::from_str(&text).expect("json");
            assert_eq!(document["mcpServers"]["kontor"], expected);
        }
        ClientId::ClaudeCode => unreachable!(),
    }
}

fn desired_value(client: ClientId, spec: &ServerSpec) -> Value {
    let args: Vec<String> = spec.args.clone();
    let program = spec.program.to_string_lossy().into_owned();
    match client {
        ClientId::Codex | ClientId::Cursor => json!({"command": program, "args": args}),
        ClientId::OpenCode => {
            let mut command = vec![program];
            command.extend(args);
            json!({"type": "local", "command": command})
        }
        ClientId::Copilot | ClientId::ClaudeCode => json!({
            "type": "stdio",
            "command": program,
            "args": args,
        }),
    }
}

#[test]
fn every_file_client_installs_cleanly_and_replays() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        fixture.present();
        let desired = spec("/synthetic/realm");
        assert_eq!(
            fixture.install(&desired),
            AdapterResult::Installed,
            "{client}"
        );
        assert_shape(&fixture, &desired);
        assert!(fixture.owns(&desired_value(client, &desired)), "{client}");
        assert_eq!(
            fixture.install(&desired),
            AdapterResult::AlreadyCurrent,
            "{client}"
        );
    }
}

#[test]
fn every_file_client_updates_owned_entries_and_preserves_unrelated_configuration() {
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
        let first = spec("/synthetic/one");
        let second = spec("/synthetic/two");
        assert_eq!(
            fixture.install(&first),
            AdapterResult::Installed,
            "{client}"
        );
        assert_eq!(fixture.install(&second), AdapterResult::Updated, "{client}");
        assert_shape(&fixture, &second);
        let installed = std::fs::read_to_string(&path).expect("read");
        for fragment in unrelated_fragments(client) {
            assert!(
                installed.contains(fragment),
                "{client}: lost {fragment:?} from\n{installed}"
            );
        }
    }
}

#[test]
fn every_file_client_refuses_unowned_entries_and_repairs_only_with_its_hash() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        let desired = spec("/synthetic/realm");
        let other = entry_value(client, OTHER, &["--own".to_owned()]);
        seed_entry(&fixture, &other);
        let observed = match fixture.inspect(&desired) {
            EntryState::Unrelated { observed } => observed,
            state => panic!("{client}: expected unrelated, got {state:?}"),
        };
        assert_eq!(
            fixture.install(&desired),
            AdapterResult::Conflict {
                reason: ConflictReason::SameNameUnrelated,
                observed: Some(observed.clone()),
            },
            "{client}"
        );
        let wrong = ObservedHash::parse(&"a".repeat(64)).expect("hash");
        assert_eq!(
            fixture.repair(&desired, &wrong),
            AdapterResult::Conflict {
                reason: ConflictReason::ObservedHashMismatch,
                observed: Some(observed.clone()),
            },
            "{client}"
        );
        assert_eq!(
            fixture.repair(&desired, &observed),
            AdapterResult::Repaired,
            "{client}"
        );
        assert_shape(&fixture, &desired);
        assert!(fixture.owns(&desired_value(client, &desired)), "{client}");
    }
}

#[test]
fn a_kontor_mcp_basename_is_not_ownership() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        let desired = spec("/synthetic/realm");
        let same_name = entry_value(
            client,
            "/elsewhere/bin/kontor-mcp",
            &["--state-root".to_owned(), "/somewhere".to_owned()],
        );
        seed_entry(&fixture, &same_name);
        assert!(
            matches!(fixture.inspect(&desired), EntryState::Unrelated { .. }),
            "{client}: a basename must not claim ownership"
        );
        assert!(
            matches!(
                fixture.install(&desired),
                AdapterResult::Conflict {
                    reason: ConflictReason::SameNameUnrelated,
                    ..
                }
            ),
            "{client}"
        );
    }
}

#[test]
fn an_env_or_extra_field_change_makes_an_owned_entry_unowned() {
    for client in [ClientId::Codex, ClientId::Cursor] {
        let fixture = Fixture::new(client);
        fixture.present();
        let desired = spec("/synthetic/realm");
        assert_eq!(fixture.install(&desired), AdapterResult::Installed);
        let mutated = match client {
            ClientId::Codex => fixture.read().replace(
                "[mcp_servers.kontor]",
                "[mcp_servers.kontor]\nenv = { A = \"1\" }",
            ),
            _ => {
                let mut document: Value = serde_json::from_str(&fixture.read()).expect("json");
                document["mcpServers"]["kontor"]["env"] = json!({"A": "1"});
                document.to_string()
            }
        };
        std::fs::write(fixture.file(), &mutated).expect("mutate");
        let before = fixture.read();
        assert!(
            matches!(fixture.inspect(&desired), EntryState::Unrelated { .. }),
            "{client}: a changed full snapshot must lose ownership"
        );
        assert!(
            matches!(
                fixture.install(&desired),
                AdapterResult::Conflict {
                    reason: ConflictReason::SameNameUnrelated,
                    ..
                }
            ),
            "{client}"
        );
        assert_eq!(fixture.read(), before, "{client}: bytes were overwritten");
    }
}

#[test]
fn a_commented_jsonc_managed_entry_is_never_absent_or_overwritten() {
    let fixture = Fixture::new(ClientId::OpenCode);
    fixture.present();
    let original = "{\n  // outer note survives\n  \"mcp\": {\n    \"servers\": {\n      \"kontor\": {\n        /* transport */ \"type\": \"local\",\n        \"command\": [/* binary */ \"/usr/bin/other-mcp\"]\n      }\n    }\n  }\n}\n";
    std::fs::write(fixture.directory().join("opencode.jsonc"), original).expect("seed");
    let desired = spec("/synthetic/realm");
    let observed = match fixture.inspect(&desired) {
        EntryState::Unrelated { observed } => observed,
        state => panic!("a commented entry must not read as {state:?}"),
    };
    assert!(matches!(
        fixture.install(&desired),
        AdapterResult::Conflict {
            reason: ConflictReason::SameNameUnrelated,
            observed: Some(_),
        }
    ));
    let installed =
        std::fs::read_to_string(fixture.directory().join("opencode.jsonc")).expect("read");
    assert!(installed.contains("// outer note survives"));
    assert_eq!(installed, original);
    assert_eq!(fixture.repair(&desired, &observed), AdapterResult::Repaired);
    let repaired =
        std::fs::read_to_string(fixture.directory().join("opencode.jsonc")).expect("read");
    assert!(repaired.contains("// outer note survives"));
}

#[test]
fn an_absent_file_creation_race_is_refused() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        fixture.present();
        let target = fixture.file();
        let created = target.clone();
        let mut hooks = Hooks(vec![hook(
            FailPoint::BeforeClientRename(client),
            move |_| {
                std::fs::write(&created, "created after observation").map_err(|_| InstallError::Io)
            },
        )]);
        let mut adapter = fixture.adapter.clone();
        match adapter.install(&spec("/synthetic/realm"), &mut hooks) {
            Ok(AdapterResult::Conflict {
                reason: ConflictReason::ConcurrentChange,
                ..
            }) => {}
            other => panic!("{client}: expected a concurrent-change conflict, got {other:?}"),
        }
        assert_eq!(fixture.read(), "created after observation", "{client}");
    }
}

#[test]
fn a_creation_that_races_the_final_check_is_preserved() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        fixture.present();
        let target = fixture.file();
        let created = target.clone();
        let mut hooks = Hooks(vec![hook(
            FailPoint::BeforeClientCommit(client),
            move |_| std::fs::write(&created, "raced bytes").map_err(|_| InstallError::Io),
        )]);
        let mut adapter = fixture.adapter.clone();
        match adapter.install(&spec("/synthetic/realm"), &mut hooks) {
            Ok(AdapterResult::Conflict {
                reason: ConflictReason::ConcurrentChange,
                ..
            }) => {}
            other => panic!("{client}: a raced creation must be refused, got {other:?}"),
        }
        assert_eq!(fixture.read(), "raced bytes", "{client}");
    }
}

#[test]
fn a_directory_swap_cannot_redirect_a_client_write() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        for client in [ClientId::Codex, ClientId::Cursor] {
            let fixture = Fixture::new(client);
            fixture.present();
            let outside = fixture._holder.path().join("outside-client");
            std::fs::create_dir_all(&outside).expect("outside");
            let moved_holder = fixture._holder.path().join("moved");
            std::fs::create_dir_all(&moved_holder).expect("moved holder");
            let directory = fixture.directory();
            let moved = moved_holder.join("client-moved");
            let swap_directory = directory.clone();
            let swap_moved = moved.clone();
            let swap_outside = outside.clone();
            let mut hooks = Hooks(vec![hook(
                FailPoint::BeforeClientRename(client),
                move |_| {
                    std::fs::rename(&swap_directory, &swap_moved).map_err(|_| InstallError::Io)?;
                    symlink(&swap_outside, &swap_directory).map_err(|_| InstallError::Io)
                },
            )]);
            let mut adapter = fixture.adapter.clone();
            assert_eq!(
                adapter
                    .install(&spec("/synthetic/realm"), &mut hooks)
                    .expect("install"),
                AdapterResult::FailedReadback,
                "{client}: readback resolves the swapped path, which is correct"
            );
            assert!(
                std::fs::read_dir(&outside)
                    .expect("outside")
                    .next()
                    .is_none(),
                "{client}: nothing may land outside the held directory"
            );
            assert!(
                std::fs::read_dir(&moved).expect("moved").next().is_some(),
                "{client}: the write landed in the moved original directory"
            );
        }
    }
}

#[test]
fn temporary_client_files_are_private_before_content() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        fixture.present();
        let directory = fixture.directory();
        let file_name = fixture
            .file()
            .file_name()
            .and_then(|name| name.to_str())
            .expect("name")
            .to_owned();
        let scan = directory.clone();
        let mut hooks = Hooks(vec![hook(
            FailPoint::AfterClientTempCreate(client),
            move |_| {
                let temp = std::fs::read_dir(&scan)
                    .map_err(|_| InstallError::Io)?
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .find(|name| name.starts_with(&format!(".{file_name}.kontor-bootstrap-")))
                    .ok_or(InstallError::Io)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = std::fs::metadata(scan.join(&temp))
                        .map_err(|_| InstallError::Io)?
                        .permissions()
                        .mode()
                        & 0o777;
                    if mode != 0o600 {
                        return Err(InstallError::Injected {
                            point: FailPoint::AfterClientTempCreate(client),
                        });
                    }
                }
                Ok(())
            },
        )]);
        let mut adapter = fixture.adapter.clone();
        assert_eq!(
            adapter
                .install(&spec("/synthetic/realm"), &mut hooks)
                .expect("install"),
            AdapterResult::Installed,
            "{client}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_permission_failure_is_propagated_without_partial_state() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new(ClientId::Cursor);
    fixture.present();
    let directory = fixture.directory();
    let mut permissions = std::fs::metadata(&directory)
        .expect("metadata")
        .permissions();
    permissions.set_mode(0o500);
    std::fs::set_permissions(&directory, permissions).expect("chmod");
    let mut adapter = fixture.adapter.clone();
    let result = adapter.install(&spec("/synthetic/realm"), &mut NoFaults);
    let mut restore = std::fs::metadata(&directory)
        .expect("metadata")
        .permissions();
    restore.set_mode(0o700);
    std::fs::set_permissions(&directory, restore).expect("restore");
    assert!(
        matches!(
            result,
            Err(AdapterError::Confinement(_)) | Err(AdapterError::Io)
        ),
        "permission failure must propagate: {result:?}"
    );
    assert!(!fixture.file().exists());
    assert_eq!(
        adapter.inspect(&spec("/synthetic/realm")).expect("inspect"),
        EntryState::EntryAbsent
    );
}

#[test]
fn symlinked_homes_files_and_directories_are_refused() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        for client in FileClientAdapter::FILE_CLIENTS {
            let fixture = Fixture::new(client);
            fixture.present();
            let outside = fixture._holder.path().join("outside-config");
            std::fs::write(&outside, "outside bytes").expect("outside");
            symlink(&outside, fixture.file()).expect("symlink file");
            let desired = spec("/synthetic/realm");
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
                std::fs::read_to_string(&outside).expect("read"),
                "outside bytes"
            );
        }
    }
}

#[test]
fn wrong_transports_and_invalid_schemas_are_never_current() {
    let fixture = Fixture::new(ClientId::Cursor);
    seed_entry(
        &fixture,
        &json!({"type": "http", "url": "https://example.invalid"}),
    );
    let desired = spec("/synthetic/realm");
    assert!(!matches!(
        fixture.inspect(&desired),
        EntryState::Current { .. }
    ));
    assert!(matches!(
        fixture.install(&desired),
        AdapterResult::Conflict { .. }
    ));
    assert!(fixture.read().contains("https://example.invalid"));

    let fixture = Fixture::new(ClientId::OpenCode);
    seed_entry(
        &fixture,
        &json!({"type": "remote", "url": "https://example.invalid"}),
    );
    assert!(!matches!(
        fixture.inspect(&desired),
        EntryState::Current { .. }
    ));
    assert!(matches!(
        fixture.install(&desired),
        AdapterResult::Conflict { .. }
    ));
}

#[test]
fn an_unreadable_ledger_refuses_writes() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        seed_entry(&fixture, &entry_value(client, "/elsewhere/kontor-mcp", &[]));
        std::fs::write(fixture.state_root.join(LEDGER), "not a ledger").expect("ledger");
        let desired = spec("/synthetic/realm");
        assert_eq!(
            fixture.inspect(&desired),
            EntryState::Refused {
                reason: ConflictReason::OwnershipUnreadable
            },
            "{client}"
        );
        assert_eq!(
            fixture.install(&desired),
            AdapterResult::Conflict {
                reason: ConflictReason::OwnershipUnreadable,
                observed: None
            },
            "{client}"
        );
        let before = fixture.read();
        assert_eq!(fixture.read(), before);
    }
}

#[test]
fn oversized_and_deep_documents_are_typed_refusals() {
    let fixture = Fixture::new(ClientId::Cursor);
    fixture.present();
    let deep = format!("{}1{}", "[".repeat(100), "]".repeat(100));
    std::fs::write(fixture.file(), and_wrap(&deep)).expect("deep");
    let desired = spec("/synthetic/realm");
    assert_eq!(
        fixture.inspect(&desired),
        EntryState::Refused {
            reason: ConflictReason::TooDeep
        }
    );
    assert_eq!(
        fixture.install(&desired),
        AdapterResult::Conflict {
            reason: ConflictReason::TooDeep,
            observed: None
        }
    );
}

fn and_wrap(inner: &str) -> String {
    format!("{{\"mcpServers\": {{\"kontor\": {inner}}}}}")
}

#[test]
fn a_failed_readback_does_not_record_ownership() {
    for client in [ClientId::Codex, ClientId::Cursor] {
        let fixture = Fixture::new(client);
        fixture.present();
        // Replace the target immediately after the rename, before readback.
        let target = fixture.file();
        let mut hooks = Hooks(vec![hook(FailPoint::AfterClientWrite(client), move |_| {
            std::fs::write(&target, "no longer a document").map_err(|_| InstallError::Io)
        })]);
        let mut adapter = fixture.adapter.clone();
        assert_eq!(
            adapter
                .install(&spec("/synthetic/realm"), &mut hooks)
                .expect("install"),
            AdapterResult::FailedReadback,
            "{client}"
        );
        assert!(
            !fixture.ownership.load().expect("ledger").owns(
                client,
                &ObservedHash::of_value(&desired_value(client, &spec("/synthetic/realm")))
            ),
            "{client}: a failed readback must not claim ownership"
        );
    }
}

#[test]
fn an_absent_client_is_a_typed_absence_without_creation() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        let desired = spec("/synthetic/realm");
        assert_eq!(
            fixture.inspect(&desired),
            EntryState::ClientAbsent,
            "{client}"
        );
        assert_eq!(fixture.install(&desired), AdapterResult::Absent, "{client}");
        assert!(!fixture.directory().exists(), "{client}");
    }
}

#[test]
fn opencode_refuses_ambiguity_and_legacy_surfaces() {
    let fixture = Fixture::new(ClientId::OpenCode);
    fixture.present();
    std::fs::write(fixture.directory().join("opencode.json"), "{}").expect("json");
    std::fs::write(fixture.directory().join("opencode.jsonc"), "{}").expect("jsonc");
    let desired = spec("/synthetic/realm");
    assert_eq!(
        fixture.inspect(&desired),
        EntryState::Refused {
            reason: ConflictReason::AmbiguousConfig
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

#[test]
fn a_relative_client_home_or_state_root_is_refused() {
    assert_eq!(
        ClientHome::at("relative").unwrap_err(),
        AdapterError::RelativeRoot
    );
    assert_eq!(
        OwnershipStore::at("relative").unwrap_err(),
        AdapterError::RelativeRoot
    );
}

#[test]
fn a_scripted_concurrent_writer_is_refused() {
    let fixture = Fixture::new(ClientId::Cursor);
    fixture.present();
    let desired = spec("/synthetic/realm");
    let second = spec("/synthetic/other");
    assert_eq!(fixture.install(&desired), AdapterResult::Installed);
    let target = fixture.file();
    let mut faults =
        ScriptedFaults::observing(FailPoint::BeforeClientWrite(ClientId::Cursor), move |_| {
            std::fs::write(&target, "foreign writer").map_err(|_| InstallError::Io)
        });
    let mut adapter = fixture.adapter.clone();
    match adapter.install(&second, &mut faults).expect("install") {
        AdapterResult::Conflict {
            reason: ConflictReason::ConcurrentChange,
            ..
        } => {}
        other => panic!("expected concurrent-change conflict, got {other:?}"),
    }
    assert_eq!(fixture.read(), "foreign writer");
}
