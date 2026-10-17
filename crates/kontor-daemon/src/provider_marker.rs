//! The supported correction for one runtime setting's `unavailable_providers`.
//!
//! `runtimes.json` is operator-owned configuration, and its
//! `unavailable_providers` list is a marker an outage leaves behind: the daemon
//! composes the set once, at start, so a marker whose outage has lifted keeps
//! refusing every launch on that provider until a human edits the file. This
//! module is that edit, performed under the public preview/apply contract
//! instead of by hand.
//!
//! # Why the document is not round-tripped through the settings model
//!
//! [`crate::runtimes`] parses the exact structure the fleet composes from, and
//! it deliberately ignores fields a later generation adds. Serializing that
//! model back would therefore *drop* every unknown member, and reordering the
//! members of a file an operator wrote is an edit nobody asked for. So the
//! correction preserves the document as an ordered tree — every member keeps
//! its position and every unknown field survives — and rewrites only the one
//! marker array. The typed parse still runs first, so a document this build
//! cannot compose is refused before anything is rewritten.
//!
//! # What the caller gets
//!
//! [`plan`] is effect-free: it answers the exact before/after sets, the host
//! key the marker belongs to, and — only when the marker is present — the
//! rewritten bytes. [`write`] commits those bytes atomically, refusing if the
//! file changed since the plan was computed, so two writers cannot silently
//! overwrite each other. Neither function touches a runtime, a seat or the
//! store: the whole correction is one member of one configuration file.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::Path;

use kontor_core::id::ExternalName;
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

/// Why one provider-marker correction could not be computed or committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkerError {
    /// The settings file could not be read, or a write failed.
    Unreadable,
    /// The settings file is present but is not a fleet description this build
    /// writes.
    Malformed,
    /// The settings file is a symlink or not a regular file, so rewriting it in
    /// place would replace something this realm does not own.
    NotRewritable,
    /// More than one setting declares the addressed runtime family.
    DuplicateFamily,
    /// No configured setting declares the addressed runtime family.
    UnknownFamily,
    /// The file changed between the read a plan was computed from and the write.
    Changed,
}

/// What one correction would do, computed from exact document bytes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MarkerPlan {
    /// The non-secret host key the addressed setting is bound under.
    pub host_key: ExternalName,
    /// The setting's unavailable providers, as the document stands.
    pub before: BTreeSet<String>,
    /// The same set with the named provider removed.
    pub after: BTreeSet<String>,
    /// The rewritten document, present only when the marker was there to remove.
    pub updated: Option<Vec<u8>>,
}

/// Read the settings file, distinguishing an absent fleet from an unreadable one.
pub(crate) fn read(path: &Path) -> Result<Option<Vec<u8>>, MarkerError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(MarkerError::Unreadable),
    }
}

/// Compute what removing `provider` from `runtime_kind`'s marker would do.
///
/// A provider the setting does not mark is not an error: the plan simply has no
/// update, which is the answer an idempotent correction owes a replay.
pub(crate) fn plan(
    bytes: &[u8],
    runtime_kind: &str,
    provider: &str,
) -> Result<MarkerPlan, MarkerError> {
    let settings = crate::runtimes::parse(bytes).map_err(|_| MarkerError::Malformed)?;
    let mut matching = settings
        .runtimes
        .iter()
        .filter(|setting| setting.family() == runtime_kind);
    let setting = matching.next().ok_or(MarkerError::UnknownFamily)?;
    if matching.next().is_some() {
        return Err(MarkerError::DuplicateFamily);
    }
    // A new runtime variant must decide, at compile time, whether it carries an
    // availability marker rather than being silently skipped here: this local
    // binding becomes refutable the moment a second variant exists.
    let crate::runtimes::RuntimeSetting::Paseo(setting) = setting;
    let host_key = ExternalName::parse(&setting.host_key).map_err(|_| MarkerError::Malformed)?;
    let before = setting.unavailable_providers.clone();
    if !before.contains(provider) {
        return Ok(MarkerPlan {
            host_key,
            after: before.clone(),
            before,
            updated: None,
        });
    }
    let document: Document = serde_json::from_slice(bytes).map_err(|_| MarkerError::Malformed)?;
    let edited = remove_marker(&document, runtime_kind, provider)?;
    let updated = serde_json::to_vec_pretty(&edited).map_err(|_| MarkerError::Malformed)?;
    let mut after = before.clone();
    after.remove(provider);
    Ok(MarkerPlan {
        host_key,
        before,
        after,
        updated: Some(updated),
    })
}

/// Commit rewritten bytes, refusing a file that changed after the read.
///
/// The temporary file takes the original's permissions, so a `0600` settings
/// file stays `0600`; the rename is the commit, and it only lands if the file
/// still holds the exact bytes the plan was computed from.
pub(crate) fn write(path: &Path, expected: &[u8], updated: &[u8]) -> Result<(), MarkerError> {
    /// Keep "the file moved" distinct from "the write failed": they are a
    /// caller's conflict and an operator's disk, respectively.
    enum Commit {
        Changed,
        Io(std::io::Error),
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| MarkerError::Unreadable)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(MarkerError::NotRewritable);
    }
    let temporary = path.with_extension(format!("kontor-{}.tmp", uuid::Uuid::now_v7()));
    let result: Result<(), Commit> = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(Commit::Io)?;
        file.set_permissions(metadata.permissions())
            .map_err(Commit::Io)?;
        file.write_all(updated).map_err(Commit::Io)?;
        file.sync_all().map_err(Commit::Io)?;
        if std::fs::read(path).map_err(Commit::Io)? != expected {
            return Err(Commit::Changed);
        }
        std::fs::rename(&temporary, path).map_err(Commit::Io)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    match result {
        Ok(()) => Ok(()),
        Err(Commit::Changed) => Err(MarkerError::Changed),
        Err(Commit::Io(error)) => {
            tracing::warn!(
                error = %error,
                "a runtime settings correction could not be written"
            );
            Err(MarkerError::Unreadable)
        }
    }
}

/// Remove one provider from one runtime object's `unavailable_providers`.
///
/// The object is addressed by its own `runtime_kind`, and the marker array is
/// the only thing replaced. Every other member — known or unknown, whatever
/// order the operator wrote them in — is carried through untouched.
fn remove_marker(
    document: &Document,
    runtime_kind: &str,
    provider: &str,
) -> Result<Document, MarkerError> {
    let mut edited = document.clone();
    let Document::Object(top) = &mut edited else {
        return Err(MarkerError::Malformed);
    };
    let runtimes = top
        .iter_mut()
        .find(|(key, _)| key == "runtimes")
        .and_then(|(_, value)| match value {
            Document::Array(elements) => Some(elements),
            _ => None,
        })
        .ok_or(MarkerError::Malformed)?;
    let matching = runtimes
        .iter()
        .filter(|element| member_str(element, "runtime_kind") == Some(runtime_kind))
        .count();
    if matching > 1 {
        return Err(MarkerError::DuplicateFamily);
    }
    let Some(Document::Object(members)) = runtimes
        .iter_mut()
        .find(|element| member_str(element, "runtime_kind") == Some(runtime_kind))
    else {
        return Err(MarkerError::UnknownFamily);
    };
    let Some((_, Document::Array(providers))) = members
        .iter_mut()
        .find(|(key, _)| key == "unavailable_providers")
    else {
        return Err(MarkerError::Malformed);
    };
    let position = providers
        .iter()
        .position(|value| value.as_str() == Some(provider))
        .ok_or(MarkerError::Malformed)?;
    providers.remove(position);
    Ok(edited)
}

/// One member value of an object, by key.
fn member_str<'a>(value: &'a Document, key: &str) -> Option<&'a str> {
    match value {
        Document::Object(members) => members
            .iter()
            .find(|(name, _)| name == key)
            .and_then(|(_, value)| value.as_str()),
        _ => None,
    }
}

/// An order-preserving JSON tree.
///
/// `serde_json::Value` collapses objects into a `BTreeMap`, so a round-trip
/// would silently alphabetize a file the operator wrote. This type keeps every
/// member in document order — including duplicates, which the settings parser
/// resolves by last-wins and this editor must not quietly drop.
#[derive(Debug, Clone, PartialEq)]
enum Document {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<Document>),
    Object(Vec<(String, Document)>),
}

impl Document {
    fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for Document {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct DocumentVisitor;

        impl<'de> Visitor<'de> for DocumentVisitor {
            type Value = Document;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("any JSON value")
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(Document::Bool(value))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(Document::Number(value.into()))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(Document::Number(value.into()))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                serde_json::Number::from_f64(value)
                    .map(Document::Number)
                    .ok_or_else(|| E::custom("a JSON number that cannot be represented"))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
                Ok(Document::String(value.to_owned()))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(Document::String(value))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(Document::Null)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(Document::Null)
            }

            fn visit_some<D: Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                Document::deserialize(deserializer)
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element()? {
                    values.push(value);
                }
                Ok(Document::Array(values))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut members = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    members.push((key, map.next_value()?));
                }
                Ok(Document::Object(members))
            }
        }

        deserializer.deserialize_any(DocumentVisitor)
    }
}

impl Serialize for Document {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Number(value) => value.serialize(serializer),
            Self::String(value) => serializer.serialize_str(value),
            Self::Array(values) => {
                let mut sequence = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    sequence.serialize_element(value)?;
                }
                sequence.end()
            }
            Self::Object(members) => {
                let mut map = serializer.serialize_map(Some(members.len()))?;
                for (key, value) in members {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fleet document carrying a secret the correction must never touch.
    fn document() -> &'static [u8] {
        br#"{
  "schema_version": 5,
  "runtimes": [
    {
      "family": "paseo",
      "runtime_kind": "paseo.agent",
      "host_key": "paseo-host",
      "mini_project_id": "01890000-0000-7000-8000-000000000001",
      "jira_epic_key": "ASMA-1",
      "mini_project_short_title": "Epic",
      "plan_item_key": "KON-MVP-15",
      "jira_issue_key": "ASMA-7759",
      "ticket_short_code": "KON-15",
      "seat_display_roles": { "implement": { "role": "Implement" } },
      "project_root_cwd": "/w/epic",
      "canonical_worktree_cwd": "/w/task",
      "orchestrator_agent_id": "agent-1",
      "max_concurrent_sessions": 2,
      "unavailable_providers": ["claude", "codex"],
      "executable": "paseo",
      "host_target": "https://user:hunter2@paseo.example",
      "endpoint": "ws://127.0.0.1:6767/ws",
      "client_id": "kontor-mini-1",
      "timeout_seconds": 30
    },
    {
      "family": "paseo",
      "runtime_kind": "paseo.second",
      "host_key": "second-host",
      "mini_project_id": "01890000-0000-7000-8000-000000000002",
      "jira_epic_key": "ASMA-2",
      "mini_project_short_title": "Second",
      "plan_item_key": "KON-MVP-16",
      "jira_issue_key": "ASMA-7760",
      "ticket_short_code": "KON-16",
      "seat_display_roles": { "implement": { "role": "Implement" } },
      "project_root_cwd": "/w/second",
      "canonical_worktree_cwd": "/w/second-task",
      "orchestrator_agent_id": "agent-2",
      "max_concurrent_sessions": 1,
      "unavailable_providers": ["claude"],
      "executable": "paseo",
      "host_target": "https://other:hunter2@second.example",
      "endpoint": "ws://127.0.0.1:6768/ws",
      "client_id": "kontor-mini-2",
      "timeout_seconds": 30
    }
  ]
}"#
    }

    fn value(bytes: &[u8]) -> serde_json::Value {
        serde_json::from_slice(bytes).expect("the edited document is JSON")
    }

    #[test]
    fn a_plan_reports_the_exact_before_and_after_sets() {
        let plan = plan(document(), "paseo.agent", "claude").expect("the marker is removable");
        assert_eq!(plan.host_key.as_str(), "paseo-host");
        assert_eq!(
            plan.before.iter().collect::<Vec<_>>(),
            vec!["claude", "codex"],
            "before is the document's own set"
        );
        assert_eq!(plan.after.iter().collect::<Vec<_>>(), vec!["codex"]);
        assert!(plan.updated.is_some());
    }

    #[test]
    fn an_absent_marker_is_a_plan_with_no_update() {
        let plan = plan(document(), "paseo.agent", "cursor").expect("absence is not a failure");
        assert_eq!(plan.before, plan.after);
        assert!(plan.updated.is_none(), "nothing is rewritten");
    }

    #[test]
    fn the_correction_preserves_every_other_member_including_unknown_ones() {
        let input = br#"{
  "schema_version": 5,
  "future_top_level": { "unknown": [1, 2, {"deep": null}] },
  "runtimes": [
    {
      "family": "paseo",
      "runtime_kind": "paseo.agent",
      "host_key": "paseo-host",
      "future_member": "kept verbatim",
      "mini_project_id": "01890000-0000-7000-8000-000000000001",
      "jira_epic_key": "ASMA-1",
      "mini_project_short_title": "Epic",
      "plan_item_key": "KON-MVP-15",
      "jira_issue_key": "ASMA-7759",
      "ticket_short_code": "KON-15",
      "seat_display_roles": { "implement": { "role": "Implement" } },
      "project_root_cwd": "/w/epic",
      "canonical_worktree_cwd": "/w/task",
      "orchestrator_agent_id": "agent-1",
      "max_concurrent_sessions": 2,
      "unavailable_providers": [
        "claude",
        "codex"
      ],
      "executable": "paseo",
      "host_target": "https://user:hunter2@paseo.example",
      "endpoint": "ws://127.0.0.1:6767/ws",
      "client_id": "kontor-mini-1",
      "timeout_seconds": 30
    }
  ]
}"#;
        let plan = plan(input, "paseo.agent", "claude").expect("removable");
        let updated = plan.updated.expect("the marker is present");
        let parsed = value(&updated);
        assert_eq!(
            parsed["future_top_level"],
            serde_json::json!({ "unknown": [1, 2, { "deep": null }] }),
            "an unknown top-level member survives"
        );
        assert_eq!(
            parsed["runtimes"][0]["future_member"], "kept verbatim",
            "an unknown setting member survives"
        );
        assert_eq!(
            parsed["runtimes"][0]["host_target"], "https://user:hunter2@paseo.example",
            "the one sensitive field is carried through untouched"
        );
        assert_eq!(
            parsed["runtimes"][0]["unavailable_providers"],
            serde_json::json!(["codex"])
        );
        // Member order is preserved: the editor is not a formatter.
        let text = String::from_utf8(updated).expect("utf-8");
        let host = text.find("\"host_key\"").expect("host_key survives");
        let future = text
            .find("\"future_member\"")
            .expect("future member survives");
        assert!(
            host < future,
            "the member order is the document's, not an alphabetized rewrite"
        );
    }

    #[test]
    fn removing_the_last_marker_leaves_an_empty_array() {
        let plan = plan(document(), "paseo.second", "claude").expect("removable");
        let updated = plan.updated.expect("the marker is present");
        assert_eq!(
            value(&updated)["runtimes"][1]["unavailable_providers"],
            serde_json::json!([])
        );
    }

    #[test]
    fn removing_one_setting_never_touches_another() {
        let plan = plan(document(), "paseo.agent", "claude").expect("removable");
        let updated = plan.updated.expect("the marker is present");
        assert_eq!(
            value(&updated)["runtimes"][1]["unavailable_providers"],
            serde_json::json!(["claude"]),
            "the second setting's marker is untouched"
        );
    }

    #[test]
    fn an_absent_setting_is_a_typed_refusal() {
        assert_eq!(
            plan(document(), "paseo.absent", "claude"),
            Err(MarkerError::UnknownFamily)
        );
    }

    #[test]
    fn a_duplicate_setting_is_refused_so_the_edit_is_never_ambiguous() {
        let duplicated = String::from_utf8(document().to_vec())
            .expect("utf-8")
            .replace("\"paseo.second\"", "\"paseo.agent\"");
        assert_eq!(
            plan(duplicated.as_bytes(), "paseo.agent", "claude"),
            Err(MarkerError::DuplicateFamily)
        );
    }

    #[test]
    fn a_document_this_build_cannot_compose_is_never_rewritten() {
        assert_eq!(
            plan(
                b"{\"schema_version\": 5, \"runtimes\": \"not a list\"}",
                "paseo.agent",
                "claude"
            ),
            Err(MarkerError::Malformed)
        );
        assert_eq!(
            plan(
                b"{\"schema_version\": 5, \"runtimes\": [{\"family\": \"ao\"}]}",
                "ao",
                "claude"
            ),
            Err(MarkerError::Malformed),
            "a deferred family is refused before any edit"
        );
    }

    #[test]
    fn a_changed_file_is_never_overwritten() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("runtimes.json");
        std::fs::write(&path, document()).expect("the fixture is installed");
        let first = plan(document(), "paseo.agent", "claude").expect("removable");
        let updated = first.updated.expect("a marker to remove");
        std::fs::write(&path, b"{}").expect("a competing writer");
        assert_eq!(
            write(&path, document(), &updated),
            Err(MarkerError::Changed),
            "the write refuses a file that moved after the read"
        );
        assert_eq!(
            std::fs::read(&path).expect("the competing write stands"),
            b"{}"
        );
    }

    #[test]
    fn the_write_commits_the_plan_and_keeps_the_files_permissions() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("runtimes.json");
        std::fs::write(&path, document()).expect("the fixture is installed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("a private settings file");
        }
        let first = plan(document(), "paseo.agent", "claude").expect("removable");
        let updated = first.updated.expect("a marker to remove");
        write(&path, document(), &updated).expect("the correction commits");
        let written = std::fs::read(&path).expect("the file is readable");
        assert_eq!(
            value(&written)["runtimes"][0]["unavailable_providers"],
            serde_json::json!(["codex"])
        );
        let second =
            plan(&written, "paseo.agent", "codex").expect("the second marker is removable");
        assert!(
            second.updated.is_some(),
            "the committed document is composable"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("the file")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "the correction did not widen the file");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_settings_file_is_never_replaced() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().expect("a temporary directory");
        let outside = tempfile::tempdir().expect("an outside directory");
        std::fs::write(outside.path().join("runtimes.json"), document()).expect("the target");
        symlink(
            outside.path().join("runtimes.json"),
            directory.path().join("runtimes.json"),
        )
        .expect("the symlink");
        assert_eq!(
            write(&directory.path().join("runtimes.json"), document(), b"{}"),
            Err(MarkerError::NotRewritable)
        );
        assert_eq!(
            std::fs::read(outside.path().join("runtimes.json")).expect("the target stands"),
            document()
        );
    }
}
