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

/// The one `kontor-bootstrap` temporary entry under a client directory.
fn find_temp(directory: &Path) -> Result<std::fs::DirEntry, InstallError> {
    std::fs::read_dir(directory)
        .map_err(|_| InstallError::Io)?
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .contains("kontor-bootstrap")
        })
        .ok_or(InstallError::Io)
}

/// The one `kontor-bootstrap` temporary basename under a directory, with
/// explicit enumeration and error handling: directory-entry iteration errors
/// are propagated, and zero or more than one match is an error. Used where an
/// oracle must retain the exact displaced basename instead of rediscovering a
/// matching name after the operation.
fn single_temp_name(directory: &Path) -> Result<String, InstallError> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(directory).map_err(|_| InstallError::Io)? {
        let entry = entry.map_err(|_| InstallError::Io)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.contains("kontor-bootstrap") {
            found.push(name);
        }
    }
    match found.len() {
        1 => Ok(found.pop().expect("one entry")),
        _ => Err(InstallError::Io),
    }
}

/// How many `kontor-bootstrap` temporary names remain under a directory.
fn temp_leftovers(directory: &Path) -> usize {
    std::fs::read_dir(directory)
        .expect("directory")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .contains("kontor-bootstrap")
        })
        .count()
}

#[cfg(unix)]
fn inode_of(path: &Path) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path).expect("metadata");
    (metadata.dev(), metadata.ino())
}

/// A cross-platform capture slot for a temporary's identity; only unix builds
/// compare it, because only there is the identity an inode.
#[cfg(unix)]
fn temp_identity(path: &Path) -> (u64, u64) {
    inode_of(path)
}

#[cfg(not(unix))]
fn temp_identity(_path: &Path) -> (u64, u64) {
    (0, 0)
}

/// The exact whole-file bytes a clean two-step install produces, from an
/// independent fixture with the same inputs.
fn two_step_rendered(client: ClientId) -> Vec<u8> {
    let reference = Fixture::new(client);
    reference.present();
    assert_eq!(
        reference.install(&spec("/synthetic/one")),
        AdapterResult::Installed
    );
    assert_eq!(
        reference.install(&spec("/synthetic/two")),
        AdapterResult::Updated
    );
    std::fs::read(reference.file()).expect("reference bytes")
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
        ClientId::OpenCode => fixture.write(&json!({"mcp": {"kontor": value}}).to_string()),
        ClientId::Copilot => fixture.write(&json!({"mcpServers": {"kontor": value}}).to_string()),
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
            assert_eq!(document.value(&["mcp", "kontor"]), Some(expected));
        }
        ClientId::Copilot => {
            let document: Value = serde_json::from_str(&text).expect("json");
            assert_eq!(document["mcpServers"]["kontor"], expected);
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
        // A successful absent-target install cleans its own pre-exchange
        // temporary by retained identity: no temporary name survives.
        assert_eq!(
            temp_leftovers(&fixture.directory()),
            0,
            "{client}: the own pre-exchange temporary is cleaned up"
        );
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
    let original = "{\n  // outer note survives\n  \"mcp\": {\n    \"kontor\": {\n      /* transport */ \"type\": \"local\",\n      \"command\": [/* binary */ \"/usr/bin/other-mcp\"]\n    }\n  }\n}\n";
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
fn an_opencode_flat_same_name_entry_is_a_conflict_and_never_overwritten() {
    let fixture = Fixture::new(ClientId::OpenCode);
    fixture.present();
    let user_entry = json!({
        "type": "local",
        "command": [
            "/user/own/kontor-mcp",
            "--state-root",
            "/user-realm",
            "--credential-tier",
            "operator"
        ],
        "enabled": true
    });
    let original = json!({"mcp": {"kontor": user_entry}}).to_string();
    fixture.write(&original);
    let desired = spec("/synthetic/realm");
    let observed = match fixture.inspect(&desired) {
        EntryState::Unrelated { observed } => observed,
        state => panic!("a flat user entry must be unrelated, got {state:?}"),
    };
    assert_eq!(observed, ObservedHash::of_value(&user_entry));
    assert_eq!(
        fixture.install(&desired),
        AdapterResult::Conflict {
            reason: ConflictReason::SameNameUnrelated,
            observed: Some(observed),
        }
    );
    assert_eq!(fixture.read(), original, "the user entry is untouched");
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
fn a_foreign_client_temporary_name_is_never_removed() {
    for client in [ClientId::Codex, ClientId::Cursor] {
        let fixture = Fixture::new(client);
        fixture.present();
        let mut hooks = Hooks(vec![hook(
            FailPoint::BeforeClientTempCreate(client),
            move |path| std::fs::write(path, b"foreign temp").map_err(|_| InstallError::Io),
        )]);
        let mut adapter = fixture.adapter.clone();
        match adapter.install(&spec("/synthetic/realm"), &mut hooks) {
            Ok(AdapterResult::Conflict {
                reason: ConflictReason::ConcurrentChange,
                ..
            }) => {}
            other => panic!("{client}: a foreign temporary must be preserved, got {other:?}"),
        }
        let foreign = std::fs::read_dir(fixture.directory())
            .expect("directory")
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("kontor-bootstrap")
            })
            .expect("the foreign temporary is preserved");
        assert_eq!(
            std::fs::read_to_string(foreign.path()).expect("read"),
            "foreign temp",
            "{client}"
        );
        assert!(!fixture.file().exists(), "{client}: no target was written");
    }
}

#[test]
fn a_replaced_temporary_is_never_removed_or_promoted() {
    for client in [ClientId::Codex, ClientId::Cursor] {
        let fixture = Fixture::new(client);
        fixture.present();
        let directory = fixture.directory();
        let mut hooks = Hooks(vec![hook(
            FailPoint::AfterClientTempWrite(client),
            move |_| {
                let temp = std::fs::read_dir(&directory)
                    .map_err(|_| InstallError::Io)?
                    .filter_map(Result::ok)
                    .find(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .contains("kontor-bootstrap")
                    })
                    .ok_or(InstallError::Io)?;
                std::fs::remove_file(temp.path()).map_err(|_| InstallError::Io)?;
                std::fs::write(temp.path(), b"replaced temp bytes").map_err(|_| InstallError::Io)
            },
        )]);
        let mut adapter = fixture.adapter.clone();
        match adapter.install(&spec("/synthetic/realm"), &mut hooks) {
            Ok(AdapterResult::Conflict {
                reason: ConflictReason::ConcurrentChange,
                ..
            }) => {}
            other => panic!("{client}: a replaced temporary must be refused, got {other:?}"),
        }
        let foreign = std::fs::read_dir(fixture.directory())
            .expect("directory")
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("kontor-bootstrap")
            })
            .expect("the replaced temporary is preserved");
        assert_eq!(
            std::fs::read_to_string(foreign.path()).expect("read"),
            "replaced temp bytes",
            "{client}"
        );
        assert!(!fixture.file().exists(), "{client}: no target was written");
    }
}

#[test]
fn an_existing_entry_raced_at_the_final_boundary_is_never_replaced() {
    let fixture = Fixture::new(ClientId::Cursor);
    fixture.present();
    let first = spec("/synthetic/one");
    let second = spec("/synthetic/two");
    assert_eq!(fixture.install(&first), AdapterResult::Installed);
    let expected_rendered = two_step_rendered(ClientId::Cursor);
    let target = fixture.file();
    let raced_target = target.clone();
    let target_after_exchange = target.clone();
    let commit_directory = fixture.directory();
    let scan_directory = fixture.directory();
    let raced_bytes = b"raced at the final boundary";
    let raced_inode: std::rc::Rc<std::cell::RefCell<Option<(u64, u64)>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let rendered_inode: std::rc::Rc<std::cell::RefCell<Option<(u64, u64)>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let displaced_name: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_raced_inode = std::rc::Rc::clone(&raced_inode);
    let hook_rendered_inode = std::rc::Rc::clone(&rendered_inode);
    let hook_displaced_name = std::rc::Rc::clone(&displaced_name);
    let mut hooks = Hooks(vec![
        hook(FailPoint::BeforeClientCommit(ClientId::Cursor), move |_| {
            // Retain the actual transaction temporary basename at the
            // intervention boundary, with explicit enumeration and error
            // handling: no post-return rediscovery of a matching name.
            *hook_displaced_name.borrow_mut() = Some(single_temp_name(&commit_directory)?);
            std::fs::write(&raced_target, raced_bytes).map_err(|_| InstallError::Io)?;
            #[cfg(unix)]
            {
                *hook_raced_inode.borrow_mut() = Some(inode_of(&raced_target));
            }
            Ok(())
        }),
        hook(
            FailPoint::AfterClientExchange(ClientId::Cursor),
            move |_| {
                #[cfg(unix)]
                {
                    *hook_rendered_inode.borrow_mut() = Some(inode_of(&target_after_exchange));
                }
                Ok(())
            },
        ),
    ]);
    let mut adapter = fixture.adapter.clone();
    match adapter.install(&second, &mut hooks) {
        Err(AdapterError::PreserveBoth) => {}
        other => panic!("a final-boundary race must be refused, got {other:?}"),
    }
    // Exact evidence, no bytes-anywhere scan: the target carries the exact
    // rendered bytes and the exact rendered inode captured after the exchange
    // (there is no blind exchange-back), and the raced inode/bytes are
    // preserved at the exact displaced basename retained at intervention.
    assert_eq!(
        std::fs::read(fixture.file()).expect("target bytes"),
        expected_rendered,
        "the target keeps the exact rendered bytes"
    );
    #[cfg(unix)]
    assert_eq!(
        inode_of(&fixture.file()),
        rendered_inode.borrow().expect("captured rendered inode"),
        "the target keeps the exact rendered inode"
    );
    let name = displaced_name
        .borrow()
        .clone()
        .expect("captured displaced basename");
    assert_eq!(
        single_temp_name(&scan_directory).expect("exactly one displaced name remains"),
        name,
        "the only remaining temporary name is the exact retained displaced basename"
    );
    let displaced = scan_directory.join(&name);
    let metadata =
        std::fs::symlink_metadata(&displaced).expect("the exact displaced basename is preserved");
    assert!(
        !metadata.file_type().is_symlink(),
        "the preserved displaced entry is a regular file"
    );
    assert_eq!(
        std::fs::read(&displaced).expect("raced bytes"),
        raced_bytes,
        "the raced bytes are preserved exactly at the retained displaced name"
    );
    #[cfg(unix)]
    assert_eq!(
        inode_of(&displaced),
        raced_inode.borrow().expect("captured raced inode"),
        "the raced inode is preserved exactly at the retained displaced name"
    );

    // A replay must not report the client as current while the displaced
    // bytes still sit in the temporary: the reconciliation stays visible.
    assert_eq!(
        fixture.inspect(&second),
        EntryState::Refused {
            reason: ConflictReason::UnreconciledTemporary,
        }
    );
    assert_eq!(
        fixture.install(&second),
        AdapterResult::Conflict {
            reason: ConflictReason::UnreconciledTemporary,
            observed: None,
        }
    );
    assert_eq!(
        std::fs::read(fixture.file()).expect("target bytes"),
        expected_rendered,
        "the replay writes nothing"
    );
}

#[test]
fn a_leftover_temporary_from_an_interrupted_write_does_not_block_a_replay() {
    let fixture = Fixture::new(ClientId::Cursor);
    fixture.present();
    let residue = fixture
        .directory()
        .join(".mcp.json.kontor-bootstrap-123-456.tmp");
    std::fs::write(&residue, b"interrupted write residue").expect("residue");
    let desired = spec("/synthetic/realm");
    assert_eq!(fixture.install(&desired), AdapterResult::Installed);
    assert_eq!(fixture.install(&desired), AdapterResult::AlreadyCurrent);
    assert_eq!(
        std::fs::read(&residue).expect("residue"),
        b"interrupted write residue",
        "an untracked temporary is preserved, not removed"
    );
}

#[test]
fn an_entry_with_null_and_boolean_extras_is_not_refused() {
    for client in [ClientId::Cursor, ClientId::Copilot] {
        let fixture = Fixture::new(client);
        let key = "mcpServers";
        let seed = format!(
            "{{\"{key}\":{{\"kontor\":{{\"type\":\"stdio\",\"command\":\"/usr/bin/other-mcp\",\"args\":[],\"extra\":null,\"nested\":{{\"none\":null}},\"flag\":true,\"list\":[1,null,2]}}}}}}"
        );
        fixture.write(&seed);
        // C3: the valid literal extras keep present semantics and the refused
        // path leaves the whole file byte-for-byte unchanged.
        let before = fixture.read();
        let desired = spec("/synthetic/realm");
        let state = fixture.inspect(&desired);
        assert!(
            !matches!(state, EntryState::Refused { .. })
                && !matches!(state, EntryState::EntryAbsent),
            "{client}: null/boolean extras keep present semantics, got {state:?}"
        );
        assert!(
            matches!(fixture.install(&desired), AdapterResult::Conflict { .. }),
            "{client}"
        );
        assert!(fixture.read().contains("\"extra\":null"), "{client}");
        assert_eq!(
            fixture.read(),
            before,
            "{client}: the whole file is preserved exactly"
        );
    }
}

#[test]
fn a_post_exchange_displaced_mutation_preserves_both_names_exactly() {
    let fixture = Fixture::new(ClientId::Cursor);
    fixture.present();
    assert_eq!(
        fixture.install(&spec("/synthetic/one")),
        AdapterResult::Installed
    );
    let expected_rendered = two_step_rendered(ClientId::Cursor);
    let hook_directory = fixture.directory();
    let scan_directory = fixture.directory();
    let target = fixture.file();
    let captured_name: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let captured_inode: std::rc::Rc<std::cell::RefCell<Option<(u64, u64)>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let captured_target_inode: std::rc::Rc<std::cell::RefCell<Option<(u64, u64)>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_name = std::rc::Rc::clone(&captured_name);
    let hook_inode = std::rc::Rc::clone(&captured_inode);
    let hook_target_inode = std::rc::Rc::clone(&captured_target_inode);
    let mut hooks = Hooks(vec![hook(
        FailPoint::AfterClientExchange(ClientId::Cursor),
        move |_| {
            // The first post-exchange snapshots are complete; this intervention is
            // therefore a post-first-observation displaced-side mutation, never a
            // pre-exchange substitute.
            let temp = find_temp(&hook_directory)?;
            std::fs::remove_file(temp.path()).map_err(|_| InstallError::Io)?;
            std::fs::write(temp.path(), b"post exchange foreign").map_err(|_| InstallError::Io)?;
            *hook_name.borrow_mut() = Some(temp.file_name().to_string_lossy().into_owned());
            #[cfg(unix)]
            {
                *hook_inode.borrow_mut() = Some(inode_of(&temp.path()));
                *hook_target_inode.borrow_mut() = Some(inode_of(&target));
            }
            Ok(())
        },
    )]);
    let mut adapter = fixture.adapter.clone();
    match adapter.install(&spec("/synthetic/two"), &mut hooks) {
        Err(AdapterError::PreserveBoth) => {}
        other => panic!("a post-exchange mutation must PreserveBoth, got {other:?}"),
    }
    // Exact target operands: the swapped-in rendered inode and its complete
    // bytes are unchanged (no exchange-back), and the exact displaced
    // name/inode/bytes are preserved (no cleanup). There is no
    // bytes-anywhere tolerance here: one exact name and one exact inode.
    let target_bytes = std::fs::read(fixture.file()).expect("target bytes");
    assert_eq!(
        target_bytes, expected_rendered,
        "the target keeps the exact rendered bytes"
    );
    #[cfg(unix)]
    assert_eq!(
        inode_of(&fixture.file()),
        captured_target_inode
            .borrow()
            .expect("captured target inode"),
        "the target keeps the exact rendered inode"
    );
    let foreign = find_temp(&scan_directory).expect("the displaced name is preserved");
    assert_eq!(
        foreign.file_name().to_string_lossy(),
        captured_name.borrow().as_deref().expect("captured name"),
        "the preserved bytes stay at the exact displaced name"
    );
    assert_eq!(
        std::fs::read(foreign.path()).expect("read"),
        b"post exchange foreign"
    );
    #[cfg(unix)]
    assert_eq!(
        inode_of(&foreign.path()),
        captured_inode.borrow().expect("captured inode"),
        "the preserved exact inode is the mutated displaced side"
    );
}

#[test]
fn a_rollback_restores_the_original_and_cleans_the_corrupted_temporary() {
    let fixture = Fixture::new(ClientId::Cursor);
    fixture.present();
    let first = spec("/synthetic/one");
    assert_eq!(fixture.install(&first), AdapterResult::Installed);
    let original = std::fs::read(fixture.file()).expect("original bytes");
    #[cfg(unix)]
    let original_inode = inode_of(&fixture.file());
    let exchange_directory = fixture.directory();
    let scan_directory = fixture.directory();
    let captured_name: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_name = std::rc::Rc::clone(&captured_name);
    let mut hooks = Hooks(vec![hook(
        FailPoint::BeforeClientExchange(ClientId::Cursor),
        move |_| {
            let temp = find_temp(&exchange_directory)?;
            // Deliberately truncate and then write the synthetic tamper, so
            // the complete corrupted bytes are exactly this literal (the
            // prefix-only overwrite left an unpredictable suffix). Same inode,
            // changed content: both sides remain fully observed and stable, so
            // the explicit rollback path restores the original; the restored
            // temporary equals the retained first post-exchange snapshot and
            // is cleaned by its retained identity.
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(temp.path())
                .map_err(|_| InstallError::Io)?;
            file.write_all(b"corrupted owned temporary")
                .map_err(|_| InstallError::Io)?;
            *hook_name.borrow_mut() = Some(temp.file_name().to_string_lossy().into_owned());
            Ok(())
        },
    )]);
    let mut adapter = fixture.adapter.clone();
    match adapter.install(&spec("/synthetic/two"), &mut hooks) {
        Ok(AdapterResult::Conflict {
            reason: ConflictReason::ConcurrentChange,
            ..
        }) => {}
        other => panic!("an explicit rollback is a conflict, got {other:?}"),
    }
    // The exchange-back restored the original identity and complete bytes at
    // the target; the restored temporary matched the retained first
    // post-exchange snapshot exactly, so the proved-owned rollback cleanup
    // unlinked precisely the transaction-created temporary by retained
    // identity.
    assert_eq!(
        std::fs::read(fixture.file()).expect("target bytes"),
        original,
        "the exchange-back restores the original bytes exactly"
    );
    #[cfg(unix)]
    assert_eq!(
        inode_of(&fixture.file()),
        original_inode,
        "the exchange-back restores the exact original inode"
    );
    let name = captured_name.borrow().clone().expect("captured temporary");
    let error = std::fs::symlink_metadata(scan_directory.join(&name))
        .expect_err("the proved-owned corrupted temporary is cleaned up");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(
        temp_leftovers(&scan_directory),
        0,
        "no temporary name survives the proved-owned rollback cleanup"
    );
}

#[test]
fn a_rollback_cleanup_preserves_a_foreign_replacement_and_restores_the_original() {
    let fixture = Fixture::new(ClientId::Cursor);
    fixture.present();
    let first = spec("/synthetic/one");
    assert_eq!(fixture.install(&first), AdapterResult::Installed);
    let original = std::fs::read(fixture.file()).expect("original bytes");
    #[cfg(unix)]
    let original_inode = inode_of(&fixture.file());
    let exchange_directory = fixture.directory();
    let rollback_directory = fixture.directory();
    let scan_directory = fixture.directory();
    let captured_name: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let captured_inode: std::rc::Rc<std::cell::RefCell<Option<(u64, u64)>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_name = std::rc::Rc::clone(&captured_name);
    let hook_inode = std::rc::Rc::clone(&captured_inode);
    let mut hooks = Hooks(vec![
        hook(
            FailPoint::BeforeClientExchange(ClientId::Cursor),
            move |_| {
                let temp = find_temp(&exchange_directory)?;
                // Deliberately truncate and write the synthetic tamper so the
                // corrupted content is exact. Same inode, changed content: the
                // exchange displaces an inode we created whose bytes are not the
                // rendered document, so the rollback path is eligible.
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(temp.path())
                    .map_err(|_| InstallError::Io)?;
                file.write_all(b"corrupted owned temporary")
                    .map_err(|_| InstallError::Io)?;
                Ok(())
            },
        ),
        hook(
            FailPoint::AfterClientRollback(ClientId::Cursor),
            move |_| {
                let temp = find_temp(&rollback_directory)?;
                // The restored-state verification has already passed (target
                // original; temporary equal to its retained first snapshot), so
                // this branch takes the proved-owned cleanup. The hook replaces
                // the temporary after that verification and before the unlink:
                // the cleanup is identity-bound to the retained temporary, so
                // this exact foreign inode must survive instead of being unlinked
                // by name.
                std::fs::remove_file(temp.path()).map_err(|_| InstallError::Io)?;
                std::fs::write(temp.path(), b"rollback foreign replacement")
                    .map_err(|_| InstallError::Io)?;
                *hook_name.borrow_mut() = Some(temp.file_name().to_string_lossy().into_owned());
                #[cfg(unix)]
                {
                    *hook_inode.borrow_mut() = Some(inode_of(&temp.path()));
                }
                Ok(())
            },
        ),
    ]);
    let mut adapter = fixture.adapter.clone();
    match adapter.install(&spec("/synthetic/two"), &mut hooks) {
        Ok(AdapterResult::Conflict {
            reason: ConflictReason::ConcurrentChange,
            ..
        }) => {}
        other => panic!("a rollback with a foreign replacement is a conflict, got {other:?}"),
    }
    assert_eq!(
        std::fs::read(fixture.file()).expect("target bytes"),
        original,
        "the exchange-back restores the original bytes exactly"
    );
    #[cfg(unix)]
    assert_eq!(
        inode_of(&fixture.file()),
        original_inode,
        "the exchange-back restores the exact original inode"
    );
    let preserved = find_temp(&scan_directory)
        .expect("the foreign replacement is preserved, not unlinked by name");
    assert_eq!(
        preserved.file_name().to_string_lossy(),
        captured_name.borrow().as_deref().expect("captured name")
    );
    assert_eq!(
        std::fs::read(preserved.path()).expect("read"),
        b"rollback foreign replacement"
    );
    #[cfg(unix)]
    assert_eq!(
        inode_of(&preserved.path()),
        captured_inode.borrow().expect("captured inode"),
        "the preserved exact foreign inode survives the identity-bound cleanup"
    );
}

#[test]
fn a_rollback_observation_error_preserves_both_names() {
    let fixture = Fixture::new(ClientId::Cursor);
    fixture.present();
    let first = spec("/synthetic/one");
    assert_eq!(fixture.install(&first), AdapterResult::Installed);
    let original = std::fs::read(fixture.file()).expect("original bytes");
    #[cfg(unix)]
    let original_inode = inode_of(&fixture.file());
    let target = fixture.file();
    let exchange_directory = fixture.directory();
    let scan_directory = fixture.directory();
    let target_for_hook = target.clone();
    let injected_inode: std::rc::Rc<std::cell::RefCell<Option<(u64, u64)>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let displaced_name: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_injected_inode = std::rc::Rc::clone(&injected_inode);
    let hook_displaced_name = std::rc::Rc::clone(&displaced_name);
    let mut hooks = Hooks(vec![hook(
        FailPoint::AfterClientExchange(ClientId::Cursor),
        move |_| {
            // Retain the actual displaced basename at the intervention
            // boundary, with explicit enumeration and error handling.
            *hook_displaced_name.borrow_mut() = Some(single_temp_name(&exchange_directory)?);
            // A kind observation error on the fresh target snapshot: the
            // target becomes a directory, so the fresh content read is not a
            // successful observation and the rollback eligibility is false.
            // This is an observed error, never an unobserved final race.
            std::fs::remove_file(&target_for_hook).map_err(|_| InstallError::Io)?;
            std::fs::create_dir(&target_for_hook).map_err(|_| InstallError::Io)?;
            #[cfg(unix)]
            {
                *hook_injected_inode.borrow_mut() = Some(inode_of(&target_for_hook));
            }
            Ok(())
        },
    )]);
    let mut adapter = fixture.adapter.clone();
    match adapter.install(&spec("/synthetic/two"), &mut hooks) {
        Err(AdapterError::PreserveBoth) => {}
        other => panic!("an observed rollback error preserves both names, got {other:?}"),
    }
    // Neither name is unlinked or exchanged back: the exact injected directory
    // identity is preserved at the target and the untouched original inode and
    // bytes remain at the exact displaced basename retained at intervention.
    assert!(
        target.is_dir(),
        "the injected directory is preserved at the target"
    );
    #[cfg(unix)]
    assert_eq!(
        inode_of(&target),
        injected_inode.borrow().expect("captured injected inode"),
        "the exact injected directory inode is preserved"
    );
    let name = displaced_name
        .borrow()
        .clone()
        .expect("captured displaced basename");
    assert_eq!(
        single_temp_name(&scan_directory).expect("exactly one displaced name remains"),
        name,
        "the only remaining temporary name is the exact retained displaced basename"
    );
    let displaced = scan_directory.join(&name);
    let metadata =
        std::fs::symlink_metadata(&displaced).expect("the exact displaced basename is preserved");
    assert!(
        !metadata.file_type().is_symlink(),
        "the preserved displaced entry is a regular file"
    );
    assert_eq!(
        std::fs::read(&displaced).expect("displaced bytes"),
        original,
        "the displaced original is preserved exactly"
    );
    #[cfg(unix)]
    assert_eq!(
        inode_of(&displaced),
        original_inode,
        "the displaced original keeps its exact inode"
    );
}

#[test]
fn an_absent_commit_foreign_replacement_keeps_the_exact_temporary_name() {
    let fixture = Fixture::new(ClientId::Cursor);
    fixture.present();
    let hook_directory = fixture.directory();
    let scan_directory = fixture.directory();
    let captured_name: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let captured_inode: std::rc::Rc<std::cell::RefCell<Option<(u64, u64)>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_name = std::rc::Rc::clone(&captured_name);
    let hook_inode = std::rc::Rc::clone(&captured_inode);
    let mut hooks = Hooks(vec![hook(
        FailPoint::BeforeClientCommit(ClientId::Cursor),
        move |_| {
            let temp = find_temp(&hook_directory)?;
            std::fs::remove_file(temp.path()).map_err(|_| InstallError::Io)?;
            std::fs::write(temp.path(), b"absent commit foreign").map_err(|_| InstallError::Io)?;
            *hook_name.borrow_mut() = Some(temp.file_name().to_string_lossy().into_owned());
            #[cfg(unix)]
            {
                *hook_inode.borrow_mut() = Some(inode_of(&temp.path()));
            }
            Ok(())
        },
    )]);
    let mut adapter = fixture.adapter.clone();
    match adapter.install(&spec("/synthetic/realm"), &mut hooks) {
        Ok(AdapterResult::Conflict {
            reason: ConflictReason::ConcurrentChange,
            ..
        }) => {}
        other => panic!("an absent-commit foreign replacement is refused, got {other:?}"),
    }
    assert!(
        !fixture.file().exists(),
        "the foreign temporary is never promoted to the absent target"
    );
    let foreign = find_temp(&scan_directory).expect("the foreign replacement is preserved");
    assert_eq!(
        foreign.file_name().to_string_lossy(),
        captured_name.borrow().as_deref().expect("captured name")
    );
    assert_eq!(
        std::fs::read(foreign.path()).expect("read"),
        b"absent commit foreign"
    );
    #[cfg(unix)]
    assert_eq!(
        inode_of(&foreign.path()),
        captured_inode.borrow().expect("captured inode")
    );
}

/// Test-only capture cell for one displaced temporary's exact basename and
/// device/inode identity.
type DisplacedCapture = std::rc::Rc<std::cell::RefCell<Option<(String, (u64, u64))>>>;

#[test]
fn a_verified_commit_cleanup_unlinks_only_the_displaced_original() {
    let fixture = Fixture::new(ClientId::Cursor);
    fixture.present();
    assert_eq!(
        fixture.install(&spec("/synthetic/one")),
        AdapterResult::Installed
    );
    let expected_rendered = two_step_rendered(ClientId::Cursor);
    #[cfg(unix)]
    let original_inode = inode_of(&fixture.file());
    let exchange_directory = fixture.directory();
    let scan_directory = fixture.directory();
    let displaced: DisplacedCapture = std::rc::Rc::new(std::cell::RefCell::new(None));
    let hook_displaced = std::rc::Rc::clone(&displaced);
    let mut hooks = Hooks(vec![hook(
        FailPoint::AfterClientExchange(ClientId::Cursor),
        move |_| {
            let temp = find_temp(&exchange_directory)?;
            *hook_displaced.borrow_mut() = Some((
                temp.file_name().to_string_lossy().into_owned(),
                temp_identity(&temp.path()),
            ));
            Ok(())
        },
    )]);
    let mut adapter = fixture.adapter.clone();
    assert_eq!(
        adapter
            .install(&spec("/synthetic/two"), &mut hooks)
            .expect("install"),
        AdapterResult::Updated
    );
    // Successful commit cleanup: the target holds the exact rendered bytes in
    // a new inode, and the displaced original name/inode is gone by exact
    // NotFound, with no other temporary left behind.
    assert_eq!(
        std::fs::read(fixture.file()).expect("target bytes"),
        expected_rendered
    );
    #[cfg(unix)]
    assert_ne!(
        inode_of(&fixture.file()),
        original_inode,
        "the target is the rendered inode, not the displaced original"
    );
    let captured = displaced.borrow();
    let (name, _) = captured.as_ref().expect("captured displaced name");
    let error = std::fs::symlink_metadata(scan_directory.join(name))
        .expect_err("the displaced original is cleaned up");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(temp_leftovers(&scan_directory), 0);
}

#[test]
fn a_changed_displaced_side_preserves_both_names() {
    let fixture = Fixture::new(ClientId::Cursor);
    fixture.present();
    let first = spec("/synthetic/one");
    assert_eq!(fixture.install(&first), AdapterResult::Installed);
    let directory = fixture.directory();
    let mut hooks = Hooks(vec![hook(
        FailPoint::BeforeClientExchange(ClientId::Cursor),
        move |_| {
            let temp = std::fs::read_dir(&directory)
                .map_err(|_| InstallError::Io)?
                .filter_map(Result::ok)
                .find(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .contains("kontor-bootstrap")
                })
                .ok_or(InstallError::Io)?;
            std::fs::remove_file(temp.path()).map_err(|_| InstallError::Io)?;
            std::fs::write(temp.path(), b"changed displaced side").map_err(|_| InstallError::Io)
        },
    )]);
    let mut adapter = fixture.adapter.clone();
    match adapter.install(&spec("/synthetic/two"), &mut hooks) {
        Err(AdapterError::PreserveBoth) => {}
        other => panic!("a changed displaced side must be refused, got {other:?}"),
    }
    // Both names are preserved: no blind exchange-back, no cleanup.
    let mut changed = false;
    let mut original = false;
    for entry in std::fs::read_dir(fixture.directory())
        .expect("directory")
        .filter_map(Result::ok)
    {
        if let Ok(text) = std::fs::read_to_string(entry.path()) {
            if text == "changed displaced side" {
                changed = true;
            }
            if text.contains("/synthetic/one") {
                original = true;
            }
        }
    }
    assert!(changed, "the changed displaced bytes are preserved");
    assert!(original, "the original bytes are preserved");
}

#[test]
fn a_present_entry_with_an_unrepresentable_number_is_refused_never_absent() {
    for client in [ClientId::Cursor, ClientId::Copilot] {
        let fixture = Fixture::new(client);
        let desired = spec("/synthetic/realm");
        let key = "mcpServers";
        // Raw text: a huge integer is intentionally outside i64/u64.
        let seed = format!(
            "{{\"{key}\":{{\"kontor\":{{\"type\":\"stdio\",\"command\":\"/usr/bin/other-mcp\",\"args\":[],\"out_of_range\":1e9999,\"huge_integer\":123456789012345678901234567890}}}}}}"
        );
        fixture.write(&seed);
        assert_eq!(
            fixture.inspect(&desired),
            EntryState::Refused {
                reason: ConflictReason::Unparsable
            },
            "{client}: a present value must never read as absent"
        );
        let before = fixture.read();
        assert!(
            matches!(
                fixture.install(&desired),
                AdapterResult::Conflict {
                    reason: ConflictReason::Unparsable,
                    ..
                }
            ),
            "{client}"
        );
        assert_eq!(fixture.read(), before, "{client}: bytes were modified");
    }
}

#[test]
fn a_swapped_client_temporary_is_never_promoted() {
    for client in [ClientId::Codex, ClientId::Cursor] {
        let fixture = Fixture::new(client);
        fixture.present();
        let directory = fixture.directory();
        let mut hooks = Hooks(vec![hook(
            FailPoint::BeforeClientCommit(client),
            move |_| {
                let temp = std::fs::read_dir(&directory)
                    .map_err(|_| InstallError::Io)?
                    .filter_map(Result::ok)
                    .find(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .contains("kontor-bootstrap")
                    })
                    .ok_or(InstallError::Io)?;
                std::fs::remove_file(temp.path()).map_err(|_| InstallError::Io)?;
                std::fs::write(temp.path(), b"foreign swapped temp").map_err(|_| InstallError::Io)
            },
        )]);
        let mut adapter = fixture.adapter.clone();
        match adapter.install(&spec("/synthetic/realm"), &mut hooks) {
            Ok(AdapterResult::Conflict {
                reason: ConflictReason::ConcurrentChange,
                ..
            }) => {}
            other => panic!("{client}: a swapped temporary must never be promoted, got {other:?}"),
        }
        let foreign = std::fs::read_dir(fixture.directory())
            .expect("directory")
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("kontor-bootstrap")
            })
            .expect("the foreign temporary is preserved");
        assert_eq!(
            std::fs::read_to_string(foreign.path()).expect("read"),
            "foreign swapped temp",
            "{client}"
        );
        assert!(!fixture.file().exists(), "{client}: no target was written");
    }
}

#[test]
fn a_different_inode_with_same_rendered_bytes_is_never_promoted() {
    for client in [ClientId::Codex, ClientId::Cursor] {
        let fixture = Fixture::new(client);
        fixture.present();
        let directory = fixture.directory();
        let mut hooks = Hooks(vec![hook(
            FailPoint::BeforeClientCommit(client),
            move |_| {
                let temp = std::fs::read_dir(&directory)
                    .map_err(|_| InstallError::Io)?
                    .filter_map(Result::ok)
                    .find(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .contains("kontor-bootstrap")
                    })
                    .ok_or(InstallError::Io)?;
                let bytes = std::fs::read(temp.path()).map_err(|_| InstallError::Io)?;
                std::fs::remove_file(temp.path()).map_err(|_| InstallError::Io)?;
                std::fs::write(temp.path(), bytes).map_err(|_| InstallError::Io)
            },
        )]);
        let mut adapter = fixture.adapter.clone();
        match adapter.install(&spec("/synthetic/realm"), &mut hooks) {
            Ok(AdapterResult::Conflict {
                reason: ConflictReason::ConcurrentChange,
                ..
            }) => {}
            other => panic!("{client}: a different inode is never promoted, got {other:?}"),
        }
        assert!(!fixture.file().exists(), "{client}");
    }
}

#[test]
fn independent_injected_error_cleanup_removes_only_its_own_temporary() {
    for client in [ClientId::Codex, ClientId::Cursor] {
        let fixture = Fixture::new(client);
        fixture.present();
        let mut hooks = Hooks(vec![hook(
            FailPoint::AfterClientTempCreate(client),
            move |_| {
                Err(InstallError::Injected {
                    point: FailPoint::AfterClientTempCreate(client),
                })
            },
        )]);
        let mut adapter = fixture.adapter.clone();
        assert!(
            adapter
                .install(&spec("/synthetic/realm"), &mut hooks)
                .is_err(),
            "{client}: the injected error propagates"
        );
        let leftovers = std::fs::read_dir(fixture.directory())
            .expect("directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("kontor-bootstrap")
            })
            .count();
        assert_eq!(leftovers, 0, "{client}: only our own temporary is removed");
        assert!(!fixture.file().exists(), "{client}");
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
fn a_replay_after_an_interrupted_write_completes_ownership() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        fixture.present();
        let first = spec("/synthetic/one");
        let second = spec("/synthetic/two");
        assert_eq!(
            fixture.install(&first),
            AdapterResult::Installed,
            "{client}"
        );
        let mut faults = ScriptedFaults::failing_at(FailPoint::AfterClientWrite(client));
        let mut adapter = fixture.adapter.clone();
        assert_eq!(
            adapter.install(&second, &mut faults),
            Err(AdapterError::Injected),
            "{client}: the interruption lands after the configuration write"
        );
        assert!(
            !fixture.owns(&desired_value(client, &second)),
            "{client}: the ledger update was lost"
        );
        // Replay verifies the landed snapshot and completes ownership.
        assert_eq!(
            fixture.install(&second),
            AdapterResult::AlreadyCurrent,
            "{client}"
        );
        assert!(
            fixture.owns(&desired_value(client, &second)),
            "{client}: the replay completes ownership"
        );
        // A later update proceeds instead of reporting SameNameUnrelated.
        assert_eq!(fixture.install(&first), AdapterResult::Updated, "{client}");
    }
}

#[test]
fn consecutive_interruptions_keep_the_landed_entry_owned() {
    for client in FileClientAdapter::FILE_CLIENTS {
        let fixture = Fixture::new(client);
        fixture.present();
        let first = spec("/synthetic/one");
        let second = spec("/synthetic/two");
        let third = spec("/synthetic/three");
        assert_eq!(
            fixture.install(&first),
            AdapterResult::Installed,
            "{client}"
        );
        let mut interrupted = ScriptedFaults::failing_at(FailPoint::AfterClientWrite(client));
        let mut adapter = fixture.adapter.clone();
        assert_eq!(
            adapter.install(&second, &mut interrupted),
            Err(AdapterError::Injected),
            "{client}: the first write lands but its record is pending"
        );
        // The second update is interrupted before its exchange: the landed
        // second snapshot stays active and must keep its provenance.
        let mut interrupted = ScriptedFaults::failing_at(FailPoint::BeforeClientExchange(client));
        let mut adapter = fixture.adapter.clone();
        assert_eq!(
            adapter.install(&third, &mut interrupted),
            Err(AdapterError::Injected),
            "{client}: the replacement is interrupted before the exchange"
        );
        assert_eq!(
            fixture.install(&third),
            AdapterResult::Updated,
            "{client}: the earlier landed entry is still owned"
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

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
#[test]
fn unsupported_targets_refuse_directory_operations_without_effects() {
    use kontor_bootstrap::confinement::ConfinementError;
    let fixture = Fixture::new(ClientId::Cursor);
    let desired = spec("/synthetic/realm");
    let mut adapter = fixture.adapter.clone();
    assert_eq!(
        adapter.inspect(&desired),
        Err(AdapterError::Confinement(
            ConfinementError::UnsupportedPlatform
        ))
    );
    assert_eq!(
        adapter.install(&desired, &mut NoFaults),
        Err(AdapterError::Confinement(
            ConfinementError::UnsupportedPlatform
        ))
    );
    assert!(
        !fixture.directory().exists(),
        "no client directory may be created on an unsupported target"
    );
    assert!(!fixture.file().exists());
}
