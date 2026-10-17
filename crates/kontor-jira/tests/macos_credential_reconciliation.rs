//! ASMA-8015 exact-target credential reconciliation seam: test-only, macOS-only.
//!
//! The Tier-A case `an_owned_previous_scoped_entry_is_restored_byte_for_byte`
//! failed `RollbackUnproven` and retained one synthetic scoped keychain entry
//! whose value is UNKNOWN. This target pins the retained-root identity, binds
//! the failure's immutable receipts, refuses on any mismatch before touching
//! the backend, and then performs exactly one value-free pre-read plus (only
//! when the target is present) exactly one exact-target delete with a single
//! NotFound readback.
//!
//! # Guarantees
//!
//! * No enumeration, listing, wildcard, prefix, legacy or service-wide
//!   operation: every backend call addresses one exact `KeychainTarget`.
//! * The pinned retained root and the immutable evidence directory are excluded
//!   from the output directory lexically and canonically, before any receipt is
//!   created. A missing or unresolvable pinned root, or an overlap that cannot
//!   be disproved, refuses with zero file creation and zero backend calls.
//! * The validated output directory is acquired component-wise by
//!   `openat(parent_fd, name, O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC|O_RDONLY)`,
//!   retaining every component name together with its parent and child
//!   `(device, inode)` identities. Directory type, private mode and
//!   current-euid ownership are `fstat`-checked, the checked final identity is
//!   recorded, and identity-chain containment against the evidence directory
//!   and pinned retained root is required before any creation. At the immediate
//!   pre-create boundary every retained root-to-leaf parent/name edge is
//!   revalidated with a fresh fd-relative, non-following open whose observed
//!   child identity must equal the retained child identity, and the directory
//!   facts and containment are checked again; any mismatch or observation error
//!   refuses before PREPARED or backend calls. This proves the observed
//!   ancestry at that boundary only: retained handles do not freeze later
//!   namespace changes, so no stronger concurrency guarantee is claimed. The
//!   unique receipt leaf is then created **relative to the retained handle**
//!   (`O_CREAT|O_EXCL|O_WRONLY|O_NOFOLLOW|O_CLOEXEC`, mode 600) and never by
//!   reopening the output pathname; after creation the same held file
//!   descriptor carries both records.
//! * No reconciliation receipt is ever automatically unlinked: every created
//!   artifact is preserved as evidence, and a pathname replacement after the
//!   handle is open is reported as bounded `PathUnknown`.
//! * Directory and file identity/ownership observation is fail-closed: a
//!   metadata error is a static refusal before binding/backend, and an
//!   unobservable held-file identity is `IdentityUnknown`, which can never
//!   finalize or succeed.
//! * Presence is observed value-free: one exact-target, attributes-only
//!   `SecItemCopyMatching` with an explicit authentication-UI-fail answers
//!   present / absent / denied / unavailable without materializing secret
//!   bytes. No variant of the presence answer can carry a value, and
//!   reconciliation never calls `KeychainBackend::secret`; the proven
//!   production resolution path stays byte-identical and is the only reader.
//! * Both receipt records carry only non-secret identities, anchors, hashes,
//!   static reasons, factual effect fields, attempted-call counts and
//!   timestamps. Receipt records use schema
//!   `asma-8015-reconciliation-receipt/2`; no durable `/1` receipt was ever
//!   emitted (the `/1` schema existed only inside in-test `TempDir` fixtures).
//! * The disposable retained root is only canonicalized for identity binding;
//!   it is never modified, disposed or written into.
//! * No retry and at most one delete; every uncertain backend outcome stops.
//!   A delete error is unproven, not a refusal: `SystemKeychain::Unavailable`
//!   can occur after the delete command was spawned or timed out, so the
//!   factual `delete_outcome` is serialized as `unproven`.
//!
//! # Outcome taxonomy
//!
//! `Deleted`, `AlreadyAbsent`, `Refused(static reason)`,
//! `Uncertain(static reason)`, combined with a receipt state of `NotCreated`,
//! `PreparedFailed`, `Finalized`, `FinalFailed`, `PathUnknown` or
//! `IdentityUnknown`. Only a `Deleted` or `AlreadyAbsent` outcome with a
//! `Finalized` receipt is a successful terminal verdict; everything else fails
//! with a static reason after the durable evidence that could be written has
//! been written.
//!
//! # Receipt format and limits
//!
//! The receipt is a line-oriented JSON log: a `prepared` record written before
//! binding (pinned anchors plus static state UNKNOWN) and a `final` record
//! appended after the backend attempt, both through the one held descriptor.
//! If the supplied directory path is renamed or replaced after the handle is
//! open, the FINAL record still lands in the same inode, but the report is
//! `PathUnknown` and nothing is deleted: the residual discoverability
//! limitation is stated, not papered over.
//!
//! # Input divergence and disposal authority
//!
//! The frozen Tier-A guidance says to reconcile with a fresh synthetic
//! document. This seam intentionally accepts no synthetic input: it is an
//! exact-target delete-only reconciliation, and the original immutable
//! artifacts and guidance are preserved untouched. A reconcile-with-fresh-
//! document flow, if ever wanted, is a separate design. The retained root is
//! never modified, disposed, cleaned up or written into here; any disposal
//! remains a separate, separately authorized act.
//!
//! # Ignored execution
//!
//! The single `#[ignore]`d case runs the real value-free observation and, only
//! after a present pre-observation, one exact-target delete. A deliberate run
//! needs an independently supplied private receipt directory:
//!
//! ```text
//! KONTOR_ASMA8015_RECONCILE_RECEIPT_DIR=/private/path \
//!   cargo test -p kontor-jira --test macos_credential_reconciliation -- --ignored --exact \
//!   the_retained_synthetic_entry_is_reconciled_by_exact_target_delete
//! ```
//!
//! Every normal (non-ignored) test below is deterministic, uses only `TempDir`
//! state and a scripted fake keychain, and never touches the real Keychain.

#![cfg(target_os = "macos")]

use std::collections::{BTreeMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kontor_accounts::{
    KeychainBackend, KeychainFailure, KeychainTarget, KeychainWriter, SystemKeychain,
};
use kontor_core::id::{ContentHash, RealmId};
use rustix::fs::{CWD, Mode, OFlags, fstat, lstat, openat};
use rustix::io::Errno;
use rustix::process::geteuid;
use secrecy::SecretString;

/// The value-free answer to "does this exact entry exist". No variant can
/// carry or materialize secret bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemPresence {
    Present,
    Absent,
    Denied,
    Unavailable,
    TimedOut,
    Unsupported,
}

/// The value-free presence port. A backend that cannot answer without reading
/// the secret returns `Unsupported`; reconciliation never calls `secret`.
trait PresenceProbe {
    fn presence(&self, target: &KeychainTarget) -> ItemPresence;
}

/// The only `unsafe_code` exception in this workspace: one private, macOS-only
/// module inside the reconciliation integration test. Every unsafe operation
/// carries a `// SAFETY:` note. The query retrieves non-encrypted attributes
/// only and the returned dictionary is adopted and dropped unread.
#[allow(unsafe_code)]
mod value_free_observation {
    use std::ptr;
    use std::sync::mpsc;
    use std::time::Duration;

    use core_foundation::base::{CFType, OSStatus, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef, CFMutableDictionary};
    use core_foundation::number::CFNumber;
    use core_foundation::string::{CFString, CFStringRef};
    use kontor_accounts::{KeychainTarget, SystemKeychain};
    use security_framework_sys::base::{errSecAuthFailed, errSecItemNotFound, errSecSuccess};
    use security_framework_sys::item::{
        kSecAttrAccount, kSecAttrService, kSecAttrSynchronizable, kSecClass,
        kSecClassGenericPassword, kSecMatchCaseInsensitive, kSecMatchLimit, kSecMatchSearchList,
        kSecReturnAttributes, kSecReturnData, kSecReturnRef, kSecUseAuthenticationUI,
        kSecUseAuthenticationUISkip,
    };
    use security_framework_sys::keychain_item::SecItemCopyMatching;

    use super::{ItemPresence, PresenceProbe};

    /// The five-second bound on one read-only observation, mirroring the
    /// bounded `/usr/bin/security` helper's deadline.
    const OBSERVE_DEADLINE: Duration = Duration::from_secs(5);
    /// The SDK's `errSecInteractionNotAllowed` (`SecBase.h`).
    const ERR_SEC_INTERACTION_NOT_ALLOWED: OSStatus = -25308;
    /// The SDK's `errSecNotAvailable` (`SecBase.h`).
    const ERR_SEC_NOT_AVAILABLE: OSStatus = -25291;

    // `kSecUseAuthenticationUIFail` is documented in the installed SDK
    // (`SecItem.h`, macOS 10.11+) but is not declared by
    // `security-framework-sys 2.17.0`, so it is declared locally and cited.
    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        static kSecUseAuthenticationUIFail: CFStringRef;
    }

    /// Build the exact six-key attributes-only query for one exact target:
    /// class = generic password, exact service, exact account, attributes
    /// only, an explicit one-match limit, and authentication-UI-fail.
    /// `kSecReturnData`, `kSecReturnRef`, `kSecMatchSearchList`,
    /// `kSecUseDataProtectionKeychain`, `kSecMatchCaseInsensitive`,
    /// `kSecAttrSynchronizable` and `kSecUseAuthenticationUISkip` are never
    /// set.
    pub(super) fn presence_query(
        service: &str,
        account: &str,
    ) -> CFMutableDictionary<CFString, CFType> {
        // SAFETY: every wrapped symbol is an immutable process-lifetime
        // `CFStringRef` exported by the linked Security.framework. Wrapping
        // under the get rule CFRetains the borrowed static exactly once and
        // the wrapper balances that retained reference on Drop; the original
        // borrowed static is never released. The dictionary is created with
        // the kCFType key/value callbacks, so it retains every inserted entry.
        let class = unsafe { CFString::wrap_under_get_rule(kSecClass) };
        let generic = unsafe { CFString::wrap_under_get_rule(kSecClassGenericPassword) };
        let service_key = unsafe { CFString::wrap_under_get_rule(kSecAttrService) };
        let account_key = unsafe { CFString::wrap_under_get_rule(kSecAttrAccount) };
        let attributes_key = unsafe { CFString::wrap_under_get_rule(kSecReturnAttributes) };
        let limit_key = unsafe { CFString::wrap_under_get_rule(kSecMatchLimit) };
        let authentication_key = unsafe { CFString::wrap_under_get_rule(kSecUseAuthenticationUI) };
        // SAFETY: `kSecUseAuthenticationUIFail` is the immutable
        // process-lifetime static declared above; the get-rule wrapper retains
        // it and balances that retained reference on Drop.
        let authentication_fail =
            unsafe { CFString::wrap_under_get_rule(kSecUseAuthenticationUIFail) };

        CFMutableDictionary::from_CFType_pairs(&[
            (class, generic.as_CFType()),
            (service_key, CFString::from(service).as_CFType()),
            (account_key, CFString::from(account).as_CFType()),
            (attributes_key, CFBoolean::true_value().as_CFType()),
            (limit_key, CFNumber::from(1_i32).as_CFType()),
            (authentication_key, authentication_fail.as_CFType()),
        ])
    }

    /// One attributes-only, exact-target observation. The returned dictionary
    /// is adopted once under the create rule and dropped without inspecting
    /// its contents; a documented success with a missing or unusable result is
    /// uncertainty, never `Present`.
    pub(super) fn observe_exact_target(target: &KeychainTarget) -> ItemPresence {
        let query = presence_query(target.service(), target.account());
        let mut result: core_foundation::base::CFTypeRef = ptr::null();
        // SAFETY: `query` is an owned six-key dictionary alive for the call
        // and `result` is a null-initialized out-pointer; on success the
        // framework fills it with exactly one +1 created reference. The
        // framework neither retains nor releases the query on our behalf.
        let status = unsafe {
            SecItemCopyMatching(query.as_concrete_TypeRef() as CFDictionaryRef, &mut result)
        };
        if status != errSecSuccess {
            return map_status(status);
        }
        if result.is_null() {
            return ItemPresence::Unavailable;
        }
        // SAFETY: on documented success the out-pointer holds exactly one +1
        // created reference. It is adopted under the create rule exactly once
        // and dropped once, without reading or retaining its contents.
        let owned = unsafe { CFType::wrap_under_create_rule(result) };
        let usable = owned.instance_of::<CFDictionary>();
        drop(owned);
        if usable {
            ItemPresence::Present
        } else {
            ItemPresence::Unavailable
        }
    }

    /// Map one raw Security.framework status to the value-free answer. An
    /// unrecognized status is `Unavailable`, never `Absent`.
    pub(super) fn map_status(status: OSStatus) -> ItemPresence {
        match status {
            s if s == errSecSuccess => ItemPresence::Present,
            s if s == errSecItemNotFound => ItemPresence::Absent,
            s if s == errSecAuthFailed => ItemPresence::Denied,
            s if s == ERR_SEC_INTERACTION_NOT_ALLOWED || s == ERR_SEC_NOT_AVAILABLE => {
                ItemPresence::Unavailable
            }
            _ => ItemPresence::Unavailable,
        }
    }

    /// Bound one read-only observation to `deadline`. A timed-out worker is
    /// detached and owns only its inputs; it can never delete or write, and
    /// its late result is discarded. A panicked worker is `Unavailable`.
    pub(super) fn observe_bounded<F>(probe: F, deadline: Duration) -> ItemPresence
    where
        F: FnOnce() -> ItemPresence + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel();
        let _worker = std::thread::spawn(move || {
            let _ = sender.send(probe());
        });
        match receiver.recv_timeout(deadline) {
            Ok(presence) => presence,
            Err(mpsc::RecvTimeoutError::Timeout) => ItemPresence::TimedOut,
            Err(mpsc::RecvTimeoutError::Disconnected) => ItemPresence::Unavailable,
        }
    }

    /// A flat, secret-free description of the constructed query for the
    /// metadata-construction tests. It enumerates the actual dictionary and
    /// never calls `SecItemCopyMatching`.
    pub(super) fn describe_query(service: &str, account: &str) -> Vec<(String, String)> {
        let query = presence_query(service, account);
        // SAFETY: every symbol below is an immutable process-lifetime
        // `CFStringRef` from the linked Security.framework; each get-rule
        // wrapper balances its retained reference on Drop and never releases
        // the original borrowed static.
        let known_keys: Vec<(&str, CFString)> = vec![
            ("kSecClass", unsafe {
                CFString::wrap_under_get_rule(kSecClass)
            }),
            ("kSecAttrService", unsafe {
                CFString::wrap_under_get_rule(kSecAttrService)
            }),
            ("kSecAttrAccount", unsafe {
                CFString::wrap_under_get_rule(kSecAttrAccount)
            }),
            ("kSecReturnAttributes", unsafe {
                CFString::wrap_under_get_rule(kSecReturnAttributes)
            }),
            ("kSecMatchLimit", unsafe {
                CFString::wrap_under_get_rule(kSecMatchLimit)
            }),
            ("kSecUseAuthenticationUI", unsafe {
                CFString::wrap_under_get_rule(kSecUseAuthenticationUI)
            }),
            ("kSecReturnData", unsafe {
                CFString::wrap_under_get_rule(kSecReturnData)
            }),
            ("kSecReturnRef", unsafe {
                CFString::wrap_under_get_rule(kSecReturnRef)
            }),
            ("kSecMatchSearchList", unsafe {
                CFString::wrap_under_get_rule(kSecMatchSearchList)
            }),
            ("kSecMatchCaseInsensitive", unsafe {
                CFString::wrap_under_get_rule(kSecMatchCaseInsensitive)
            }),
            ("kSecAttrSynchronizable", unsafe {
                CFString::wrap_under_get_rule(kSecAttrSynchronizable)
            }),
            ("kSecUseAuthenticationUISkip", unsafe {
                CFString::wrap_under_get_rule(kSecUseAuthenticationUISkip)
            }),
        ];
        // SAFETY: both constants are immutable process-lifetime CFStrings;
        // each get-rule wrapper balances its retained reference on Drop.
        let known_values: Vec<(&str, CFString)> = vec![
            ("kSecClassGenericPassword", unsafe {
                CFString::wrap_under_get_rule(kSecClassGenericPassword)
            }),
            ("kSecUseAuthenticationUIFail", unsafe {
                CFString::wrap_under_get_rule(kSecUseAuthenticationUIFail)
            }),
        ];

        let (keys, values) = query.get_keys_and_values();
        let mut described = Vec::with_capacity(keys.len());
        for (key_ptr, value_ptr) in keys.into_iter().zip(values) {
            // SAFETY: both pointers are owned by the live `query` dictionary;
            // each get-rule wrapper retains the entry while it is described
            // and balances that reference on Drop. The wrappers never release
            // the dictionary's own references.
            let key = unsafe { CFType::wrap_under_get_rule(key_ptr) };
            let value = unsafe { CFType::wrap_under_get_rule(value_ptr) };
            let key_name = known_keys
                .iter()
                .find(|(_, known)| key == known.as_CFType())
                .map_or("unknown-key", |(name, _)| *name);
            let value_name = known_values
                .iter()
                .find(|(_, known)| value == known.as_CFType())
                .map_or_else(|| describe_value(&value), |(name, _)| (*name).to_owned());
            described.push((key_name.to_owned(), value_name));
        }
        described
    }

    fn describe_value(value: &CFType) -> String {
        if let Some(text) = value.downcast::<CFString>() {
            return format!("string:{text}");
        }
        if let Some(flag) = value.downcast::<CFBoolean>() {
            return format!("bool:{}", bool::from(flag));
        }
        if let Some(number) = value.downcast::<CFNumber>() {
            return match number.to_i64() {
                Some(number) => format!("number:{number}"),
                None => "number:non-integer".to_owned(),
            };
        }
        "other".to_owned()
    }

    impl PresenceProbe for SystemKeychain {
        fn presence(&self, target: &KeychainTarget) -> ItemPresence {
            let target = target.clone();
            observe_bounded(move || observe_exact_target(&target), OBSERVE_DEADLINE)
        }
    }
}

/// The immutable receipt manifest inside the frozen run directory.
const RECEIPTS_MANIFEST: &str = "receipts.sha256";
/// The run manifest carrying the failed run's reconciliation context.
const RUN_MANIFEST: &str = "run-manifest.txt";
/// The failed case result carrying outcome, exit and retained root.
const CASE_RESULT: &str = "case1.result.txt";
/// The only file this seam writes, created relative to the held directory.
const RECEIPT_FILE: &str = "reconciliation-receipt.json";
/// The immutable ownership fence the receipts must carry.
const OWNERSHIP_FENCE: &str = "PROVEN_AFTER_ABSENCE_FENCE";
/// The immutable failed outcome the receipts must carry.
const REQUIRED_OUTCOME: &str = "rollback_unproven";
/// The immutable failed exit code the receipts must carry.
const REQUIRED_EXIT: &str = "101";
/// The service namespace the retained target must derive from.
const SERVICE_PREFIX: &str = "kontor-jira";
/// Upper bound for the receipt manifest itself.
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
/// Upper bound for one listed receipt member.
const MAX_MEMBER_BYTES: usize = 4 * 1024 * 1024;
/// Upper bound for the number of listed receipt members.
const MAX_MEMBERS: usize = 64;
/// Upper bound for one receipt record line.
const MAX_RECORD_BYTES: usize = 8 * 1024;
/// Upper bound for the whole receipt (both records).
const MAX_RECEIPT_BYTES: usize = 16 * 1024;

/// Frozen non-secret inputs for one reconciliation run.
struct Inputs<'a> {
    run_dir: &'a Path,
    expected_receipts_sha256: &'a str,
    expected_source_blob: &'a str,
    output_dir: &'a Path,
    expected_retained_root: &'a Path,
}

/// One retained component edge: the component name and the parent/child
/// directory identities it bound at acquisition time.
struct DirectoryEdge {
    name: OsString,
    parent: Rc<OwnedFd>,
    child: Rc<OwnedFd>,
}

/// A component-wise, non-following absolute-path acquisition that retains every
/// component name plus its parent and child identities.
struct DirectoryChain {
    root: Rc<OwnedFd>,
    edges: Vec<DirectoryEdge>,
}

impl DirectoryChain {
    fn handle(&self) -> &OwnedFd {
        self.edges
            .last()
            .map_or(self.root.as_ref(), |edge| edge.child.as_ref())
    }
}

/// The validated output directory, held open by identity for the whole run.
struct BoundDirectory {
    chain: DirectoryChain,
    canonical_path: PathBuf,
    owner_uid: u32,
}

/// Upper bound on the path components one acquisition walks.
const MAX_PATH_COMPONENTS: usize = 64;

fn stat_identity(stat: &rustix::fs::Stat) -> (u64, u64) {
    (u64::from(stat.st_dev as u32), stat.st_ino)
}

fn handle_identity(handle: &OwnedFd) -> Result<(u64, u64), &'static str> {
    fstat(handle)
        .map(|stat| stat_identity(&stat))
        .map_err(|_| "output_dir_metadata_error")
}

/// Classify a refused component without following the final component.
fn classify_component(path: &Path) -> &'static str {
    match lstat(path) {
        Ok(stat) if (stat.st_mode & 0o170000) == 0o120000 => "output_dir_is_symlink",
        Ok(_) => "output_dir_not_directory",
        Err(_) => "output_dir_metadata_error",
    }
}

fn open_root() -> Result<OwnedFd, &'static str> {
    openat(
        CWD,
        "/",
        OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::RDONLY,
        Mode::empty(),
    )
    .map_err(|_| "output_dir_metadata_error")
}

/// Acquire `path` component-wise relative to the previous directory handle,
/// refusing symlink or non-directory components, and retain the whole chain so
/// the opened identity is the only directory the receipt can be created in.
fn walk_absolute(path: &Path) -> Result<DirectoryChain, &'static str> {
    if !path.is_absolute() {
        return Err("output_dir_unresolvable");
    }
    let mut components: Vec<&OsStr> = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                components.push(name);
                if components.len() > MAX_PATH_COMPONENTS {
                    return Err("output_dir_unresolvable");
                }
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err("output_dir_unresolvable");
            }
        }
    }
    let root = Rc::new(open_root()?);
    let mut current = Rc::clone(&root);
    let mut edges = Vec::new();
    let mut accumulated = PathBuf::from("/");
    for name in components {
        accumulated.push(name);
        match openat(
            current.as_ref(),
            name,
            OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::RDONLY,
            Mode::empty(),
        ) {
            Ok(child) => {
                let child = Rc::new(child);
                edges.push(DirectoryEdge {
                    name: name.to_os_string(),
                    parent: Rc::clone(&current),
                    child: Rc::clone(&child),
                });
                current = child;
            }
            Err(error) if error == Errno::NOENT => return Err("output_dir_missing"),
            Err(error) if error == Errno::NOTDIR || error == Errno::LOOP => {
                return Err(classify_component(&accumulated));
            }
            Err(_) => return Err("output_dir_metadata_error"),
        }
    }
    Ok(DirectoryChain { root, edges })
}

fn chain_identities(chain: &DirectoryChain) -> Result<Vec<(u64, u64)>, &'static str> {
    let mut identities = Vec::with_capacity(chain.edges.len() + 1);
    identities.push(handle_identity(chain.root.as_ref())?);
    for edge in &chain.edges {
        identities.push(handle_identity(edge.child.as_ref())?);
    }
    Ok(identities)
}

/// Identity-chain containment for the retained output chain against the
/// evidence directory and the pinned retained root, recomputed from fresh
/// non-following walks of both anchors.
fn containment_reasons(
    inputs: &Inputs<'_>,
    output_chain: &DirectoryChain,
) -> Result<(), &'static str> {
    let canonical_run = inputs
        .run_dir
        .canonicalize()
        .map_err(|_| "receipts_dir_unresolvable")?;
    let canonical_pinned_root = inputs
        .expected_retained_root
        .canonicalize()
        .map_err(|_| "retained_root_unresolvable")?;
    let evidence_chain = walk_absolute(&canonical_run).map_err(|_| "receipts_dir_unresolvable")?;
    let root_chain =
        walk_absolute(&canonical_pinned_root).map_err(|_| "retained_root_unresolvable")?;
    let output_identity = handle_identity(output_chain.handle())?;
    let output_identities = chain_identities(output_chain)?;
    let evidence_identities =
        chain_identities(&evidence_chain).map_err(|_| "receipts_dir_unresolvable")?;
    let root_identities =
        chain_identities(&root_chain).map_err(|_| "retained_root_unresolvable")?;
    let evidence_identity = *evidence_identities
        .last()
        .ok_or("receipts_dir_unresolvable")?;
    let root_identity = *root_identities.last().ok_or("retained_root_unresolvable")?;
    if evidence_identities.contains(&output_identity)
        || output_identities.contains(&evidence_identity)
    {
        return Err("output_dir_overlaps_evidence");
    }
    if root_identities.contains(&output_identity) || output_identities.contains(&root_identity) {
        return Err("output_dir_overlaps_retained_root");
    }
    Ok(())
}

/// The immediate pre-create boundary: revalidate every retained root-to-leaf
/// parent/name edge with a fresh fd-relative, non-following acquisition whose
/// observed child identity must equal the retained child identity, then
/// re-observe the directory facts and re-check evidence/retained-root identity
/// containment. Any mismatch or observation error refuses before PREPARED or
/// backend calls. This proves the observed ancestry at this boundary only;
/// retained handles do not freeze later namespace changes.
fn revalidate_directory(
    inputs: &Inputs<'_>,
    directory: &BoundDirectory,
) -> Result<(), &'static str> {
    for edge in &directory.chain.edges {
        let observed = match openat(
            edge.parent.as_ref(),
            edge.name.as_os_str(),
            OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::RDONLY,
            Mode::empty(),
        ) {
            Ok(child) => child,
            Err(error)
                if error == Errno::NOENT || error == Errno::NOTDIR || error == Errno::LOOP =>
            {
                return Err("output_dir_replaced");
            }
            Err(_) => return Err("output_dir_metadata_error"),
        };
        if handle_identity(&observed)? != handle_identity(edge.child.as_ref())? {
            return Err("output_dir_replaced");
        }
    }
    let facts = observe_directory(directory.chain.handle())?;
    if !facts.is_directory {
        return Err("output_dir_not_directory");
    }
    if !facts.private {
        return Err("output_dir_not_private");
    }
    if facts.owner_uid != geteuid().as_raw() {
        return Err("output_dir_not_owned");
    }
    containment_reasons(inputs, &directory.chain)
}

/// The observed facts of the held output directory.
#[derive(Clone, Copy)]
struct DirectoryFacts {
    device: u64,
    inode: u64,
    owner_uid: u32,
    is_directory: bool,
    private: bool,
}

/// Observes the held directory; injectable for deterministic tests.
type DirObserver<'a> = &'a dyn Fn(&OwnedFd) -> Result<DirectoryFacts, &'static str>;

/// Creates the receipt sink for one run; injectable for deterministic tests.
type SinkFactory<'a> = &'a dyn Fn(&BoundDirectory) -> Result<Box<dyn ReceiptSink>, &'static str>;

/// The non-secret identities proven by the immutable receipts.
struct Bound {
    run_dir: PathBuf,
    realm: String,
    service: String,
    account: String,
    retained_root: PathBuf,
    root_hash: String,
}

/// The four permitted reconciliation outcomes, with static reasons only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Deleted,
    AlreadyAbsent,
    Refused(&'static str),
    Uncertain(&'static str),
}

impl Outcome {
    fn label(self) -> &'static str {
        match self {
            Self::Deleted => "deleted",
            Self::AlreadyAbsent => "already_absent",
            Self::Refused(_) => "refused",
            Self::Uncertain(_) => "uncertain",
        }
    }

    fn reason(self) -> Option<&'static str> {
        match self {
            Self::Refused(reason) | Self::Uncertain(reason) => Some(reason),
            Self::Deleted | Self::AlreadyAbsent => None,
        }
    }
}

/// The durability and discoverability state of the private receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiptState {
    /// No receipt file was created (a refusal before preparation).
    NotCreated,
    /// The receipt file was created but the PREPARED write/sync failed.
    PreparedFailed,
    /// PREPARED and FINAL were both written through the held descriptor and the
    /// pathname still resolves to that inode.
    Finalized,
    /// PREPARED is durable but the FINAL append or sync failed.
    FinalFailed,
    /// Both records are durable in the held inode, but the pathname no longer
    /// resolves to it (renamed or replaced); nothing was deleted.
    PathUnknown,
    /// The held file's identity could not be observed, so finalization can
    /// never be claimed; nothing was deleted.
    IdentityUnknown,
}

/// Exact attempted backend call counts for one run: value-free observations
/// (counted before the call, so a timeout is counted) and the single delete.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Counts {
    observations: u32,
    deletes: u32,
}

/// One reconciliation attempt: outcome, counts, binding, receipt path and state.
struct ReconcileReport {
    outcome: Outcome,
    counts: Counts,
    bound: Option<Bound>,
    receipt_path: Option<PathBuf>,
    receipt_state: ReceiptState,
    started_unix_seconds: u64,
    finished_unix_seconds: u64,
}

/// The append-only receipt sink. The real implementation holds one open file
/// descriptor; tests can wrap it to inject preparation, finalization, owner or
/// identity failures.
trait ReceiptSink {
    fn append_prepared(&mut self, record: &str) -> Result<(), &'static str>;
    fn append_final(&mut self, record: &str) -> Result<(), &'static str>;
    /// The held receipt fd's owning uid; a metadata error is a refusal.
    fn owner_uid(&self) -> Result<u32, &'static str>;
    /// The held receipt fd's `(device, inode)`; a metadata error is uncertain.
    fn file_identity(&self) -> Result<(u64, u64), &'static str>;
}

/// The production sink: one mode-600 leaf created relative to the held
/// directory and never reopened by pathname.
struct FileReceiptSink {
    file: File,
    written: usize,
}

impl FileReceiptSink {
    fn create(directory: &BoundDirectory) -> Result<Box<dyn ReceiptSink>, &'static str> {
        let file = create_receipt_leaf(directory)?;
        Ok(Box::new(Self { file, written: 0 }))
    }

    fn append(&mut self, record: &str, reason: &'static str) -> Result<(), &'static str> {
        let line = format!("{record}\n");
        if line.len() > MAX_RECORD_BYTES || self.written + line.len() > MAX_RECEIPT_BYTES {
            return Err("receipt_too_large");
        }
        self.file.write_all(line.as_bytes()).map_err(|_| reason)?;
        self.file.sync_all().map_err(|_| reason)?;
        self.written += line.len();
        Ok(())
    }
}

impl ReceiptSink for FileReceiptSink {
    fn append_prepared(&mut self, record: &str) -> Result<(), &'static str> {
        self.append(record, "receipt_prepared_failed")
    }

    fn append_final(&mut self, record: &str) -> Result<(), &'static str> {
        self.append(record, "receipt_final_failed")
    }

    fn owner_uid(&self) -> Result<u32, &'static str> {
        fstat(self.file.as_fd())
            .map(|stat| stat.st_uid)
            .map_err(|_| "receipt_owner_metadata_error")
    }

    fn file_identity(&self) -> Result<(u64, u64), &'static str> {
        fstat(self.file.as_fd())
            .map(|stat| (u64::from(stat.st_dev as u32), stat.st_ino))
            .map_err(|_| "receipt_identity_unavailable")
    }
}

/// Wraps a real sink so the FINAL append always fails after PREPARED is durable.
struct FailFinalSink {
    inner: Box<dyn ReceiptSink>,
}

impl ReceiptSink for FailFinalSink {
    fn append_prepared(&mut self, record: &str) -> Result<(), &'static str> {
        self.inner.append_prepared(record)
    }

    fn append_final(&mut self, _record: &str) -> Result<(), &'static str> {
        Err("receipt_final_failed")
    }

    fn owner_uid(&self) -> Result<u32, &'static str> {
        self.inner.owner_uid()
    }

    fn file_identity(&self) -> Result<(u64, u64), &'static str> {
        self.inner.file_identity()
    }
}

/// Wraps a real sink so the PREPARED append always fails.
struct FailPreparedSink {
    inner: Box<dyn ReceiptSink>,
}

impl ReceiptSink for FailPreparedSink {
    fn append_prepared(&mut self, _record: &str) -> Result<(), &'static str> {
        Err("receipt_prepared_failed")
    }

    fn append_final(&mut self, record: &str) -> Result<(), &'static str> {
        self.inner.append_final(record)
    }

    fn owner_uid(&self) -> Result<u32, &'static str> {
        self.inner.owner_uid()
    }

    fn file_identity(&self) -> Result<(u64, u64), &'static str> {
        self.inner.file_identity()
    }
}

/// Wraps a real sink so owner metadata fails closed.
struct FailOwnerSink {
    inner: Box<dyn ReceiptSink>,
}

impl ReceiptSink for FailOwnerSink {
    fn append_prepared(&mut self, record: &str) -> Result<(), &'static str> {
        self.inner.append_prepared(record)
    }

    fn append_final(&mut self, record: &str) -> Result<(), &'static str> {
        self.inner.append_final(record)
    }

    fn owner_uid(&self) -> Result<u32, &'static str> {
        Err("receipt_owner_metadata_error")
    }

    fn file_identity(&self) -> Result<(u64, u64), &'static str> {
        self.inner.file_identity()
    }
}

/// Wraps a real sink so a directory/file owner mismatch is observable.
struct MismatchedOwnerSink {
    inner: Box<dyn ReceiptSink>,
    uid: u32,
}

impl ReceiptSink for MismatchedOwnerSink {
    fn append_prepared(&mut self, record: &str) -> Result<(), &'static str> {
        self.inner.append_prepared(record)
    }

    fn append_final(&mut self, record: &str) -> Result<(), &'static str> {
        self.inner.append_final(record)
    }

    fn owner_uid(&self) -> Result<u32, &'static str> {
        Ok(self.uid)
    }

    fn file_identity(&self) -> Result<(u64, u64), &'static str> {
        self.inner.file_identity()
    }
}

/// Wraps a real sink so the held file identity cannot be observed.
struct LostIdentitySink {
    inner: Box<dyn ReceiptSink>,
}

impl ReceiptSink for LostIdentitySink {
    fn append_prepared(&mut self, record: &str) -> Result<(), &'static str> {
        self.inner.append_prepared(record)
    }

    fn append_final(&mut self, record: &str) -> Result<(), &'static str> {
        self.inner.append_final(record)
    }

    fn owner_uid(&self) -> Result<u32, &'static str> {
        self.inner.owner_uid()
    }

    fn file_identity(&self) -> Result<(u64, u64), &'static str> {
        Err("receipt_identity_unavailable")
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn refused(reason: &'static str, started_unix_seconds: u64) -> ReconcileReport {
    ReconcileReport {
        outcome: Outcome::Refused(reason),
        counts: Counts::default(),
        bound: None,
        receipt_path: None,
        receipt_state: ReceiptState::NotCreated,
        started_unix_seconds,
        finished_unix_seconds: unix_now(),
    }
}

/// A lowercase hexadecimal SHA-256 digest: exactly 64 characters.
fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A Git object id in its lowercase hexadecimal form: exactly 40 characters.
fn is_git_oid(text: &str) -> bool {
    text.len() == 40
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A member name must be one plain file name: no separators, no traversal, no
/// leading dot and only a small ASCII alphabet.
fn is_safe_member_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, &'static str> {
    let mut reader = File::open(path)
        .map_err(|_| "receipt_file_unreadable")?
        .take(limit as u64 + 1);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|_| "receipt_file_unreadable")?;
    if bytes.len() > limit {
        return Err("receipt_file_too_large");
    }
    Ok(bytes)
}

/// Parse `<sha256>  <member>` lines, rejecting malformed lines, traversal,
/// duplicates and oversized manifests.
fn parse_member_manifest(text: &str) -> Result<BTreeMap<String, String>, &'static str> {
    let mut members: BTreeMap<String, String> = BTreeMap::new();
    for line in text.lines() {
        if line.is_empty() {
            return Err("receipts_manifest_malformed");
        }
        let mut fields = line.split_whitespace();
        let hash = fields.next().ok_or("receipts_manifest_malformed")?;
        let name = fields.next().ok_or("receipts_manifest_malformed")?;
        if fields.next().is_some() || !is_sha256(hash) {
            return Err("receipts_manifest_malformed");
        }
        if !is_safe_member_name(name) {
            return Err("receipts_member_traversal");
        }
        if members.len() >= MAX_MEMBERS {
            return Err("receipts_manifest_too_large");
        }
        if members.insert(name.to_owned(), hash.to_owned()).is_some() {
            return Err("receipts_member_duplicate");
        }
    }
    if members.is_empty() {
        return Err("receipts_manifest_empty");
    }
    Ok(members)
}

/// Parse the colon-form run manifest: one optional leading title line, then
/// exactly `key: value` lines, unique keys, no empty lines.
fn parse_colon_manifest(text: &str) -> Result<BTreeMap<String, String>, &'static str> {
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            return Err("manifest_malformed");
        }
        if index == 0 && !line.contains(':') {
            continue;
        }
        let (key, value) = line.split_once(':').ok_or("manifest_malformed")?;
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            return Err("manifest_malformed");
        }
        if fields.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err("manifest_duplicate_key");
        }
    }
    Ok(fields)
}

/// Parse the equals-form case result: every line is `key=value`, unique keys.
fn parse_equals_manifest(text: &str) -> Result<BTreeMap<String, String>, &'static str> {
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    for line in text.lines() {
        if line.is_empty() {
            return Err("result_malformed");
        }
        let (key, value) = line.split_once('=').ok_or("result_malformed")?;
        if key.is_empty() || value.is_empty() {
            return Err("result_malformed");
        }
        if fields.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err("result_duplicate_key");
        }
    }
    Ok(fields)
}

fn required<'a>(
    fields: &'a BTreeMap<String, String>,
    key: &str,
    reason: &'static str,
) -> Result<&'a str, &'static str> {
    fields.get(key).map(String::as_str).ok_or(reason)
}

/// Strictly bind the immutable receipts. This reads only; it is called after
/// the PREPARED record is durable and before any backend call. The parsed
/// retained root must equal the independently pinned identity.
fn bind(inputs: &Inputs<'_>) -> Result<Bound, &'static str> {
    let manifest_bytes = read_bounded(&inputs.run_dir.join(RECEIPTS_MANIFEST), MAX_MANIFEST_BYTES)?;
    if ContentHash::of(&manifest_bytes).as_str() != inputs.expected_receipts_sha256 {
        return Err("receipts_sha256_mismatch");
    }
    let manifest_text =
        std::str::from_utf8(&manifest_bytes).map_err(|_| "receipts_manifest_not_text")?;
    let members = parse_member_manifest(manifest_text)?;
    if !members.contains_key(RUN_MANIFEST) || !members.contains_key(CASE_RESULT) {
        return Err("receipts_required_member_missing");
    }
    let mut run_manifest_bytes: Option<Vec<u8>> = None;
    let mut case_result_bytes: Option<Vec<u8>> = None;
    for (name, expected_hash) in &members {
        let bytes = read_bounded(&inputs.run_dir.join(name), MAX_MEMBER_BYTES)?;
        if ContentHash::of(&bytes).as_str() != expected_hash {
            return Err("receipts_member_hash_mismatch");
        }
        match name.as_str() {
            RUN_MANIFEST => run_manifest_bytes = Some(bytes),
            CASE_RESULT => case_result_bytes = Some(bytes),
            _ => {}
        }
    }
    let run_manifest_text = std::str::from_utf8(
        run_manifest_bytes
            .as_deref()
            .ok_or("receipts_required_member_missing")?,
    )
    .map_err(|_| "manifest_not_text")?;
    let run_manifest = parse_colon_manifest(run_manifest_text)?;
    let case_result_text = std::str::from_utf8(
        case_result_bytes
            .as_deref()
            .ok_or("receipts_required_member_missing")?,
    )
    .map_err(|_| "result_not_text")?;
    let case_result = parse_equals_manifest(case_result_text)?;

    if required(&case_result, "harness_outcome", "result_outcome_missing")? != REQUIRED_OUTCOME {
        return Err("result_outcome_mismatch");
    }
    if required(&case_result, "exit", "result_exit_missing")? != REQUIRED_EXIT {
        return Err("result_exit_mismatch");
    }
    let manifest_blob = required(&run_manifest, "source blob", "manifest_source_blob_missing")?;
    if manifest_blob != inputs.expected_source_blob {
        return Err("manifest_source_blob_mismatch");
    }
    if required(
        &run_manifest,
        "reconciliation_ownership",
        "manifest_ownership_missing",
    )? != OWNERSHIP_FENCE
    {
        return Err("manifest_ownership_mismatch");
    }
    let realm_text = required(
        &run_manifest,
        "reconciliation_realm",
        "manifest_realm_missing",
    )?;
    let realm = RealmId::parse(realm_text).map_err(|_| "manifest_realm_invalid")?;
    if realm.to_string() != *realm_text {
        return Err("manifest_realm_invalid");
    }
    let retained_text = required(&run_manifest, "retained_root", "manifest_root_missing")?;
    let retained_path = Path::new(retained_text);
    if retained_path != inputs.expected_retained_root {
        return Err("manifest_retained_root_mismatch");
    }
    if !retained_path.is_absolute() {
        return Err("manifest_root_not_absolute");
    }
    let canonical_root = retained_path
        .canonicalize()
        .map_err(|_| "manifest_root_unresolvable")?;
    let canonical_pinned = inputs
        .expected_retained_root
        .canonicalize()
        .map_err(|_| "retained_root_unresolvable")?;
    if canonical_root != canonical_pinned {
        return Err("manifest_retained_root_mismatch");
    }
    let root_hash = ContentHash::of(canonical_root.as_os_str().as_encoded_bytes());
    let derived_service = format!("{SERVICE_PREFIX}:{realm}:{root_hash}");
    let manifest_service = required(
        &run_manifest,
        "reconciliation_service",
        "manifest_service_missing",
    )?;
    if derived_service != *manifest_service {
        return Err("manifest_service_mismatch");
    }
    let account = required(
        &run_manifest,
        "reconciliation_account",
        "manifest_account_missing",
    )?;
    if account.is_empty() || account.len() > 128 || account.chars().any(char::is_control) {
        return Err("manifest_account_invalid");
    }
    if required(
        &case_result,
        "retained_root",
        "result_retained_root_missing",
    )? != retained_text
    {
        return Err("result_retained_root_mismatch");
    }
    Ok(Bound {
        run_dir: inputs.run_dir.to_path_buf(),
        realm: realm.to_string(),
        service: manifest_service.to_owned(),
        account: account.to_owned(),
        retained_root: canonical_root,
        root_hash: root_hash.to_string(),
    })
}

/// Component-based same/ancestor/descendant containment (lexical on the given
/// paths, canonical when both sides have been canonicalized).
fn overlaps(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

/// Validate the two expected anchors before anything is created.
fn validate_inputs(inputs: &Inputs<'_>) -> Result<(), &'static str> {
    if !is_sha256(inputs.expected_receipts_sha256) || !is_git_oid(inputs.expected_source_blob) {
        return Err("expected_hashes_malformed");
    }
    Ok(())
}

/// Observe the held directory by `fstat`; any metadata error refuses.
fn observe_directory(handle: &OwnedFd) -> Result<DirectoryFacts, &'static str> {
    let stat = fstat(handle).map_err(|_| "output_dir_metadata_error")?;
    Ok(DirectoryFacts {
        device: u64::from(stat.st_dev as u32),
        inode: stat.st_ino,
        owner_uid: stat.st_uid,
        is_directory: (stat.st_mode & 0o170000) == 0o040000,
        private: (stat.st_mode & 0o077) == 0,
    })
}

/// Create the unique receipt leaf relative to the held directory identity:
/// `O_CREAT|O_EXCL|O_WRONLY|O_NOFOLLOW|O_CLOEXEC`, owner-only mode.
fn create_receipt_leaf(directory: &BoundDirectory) -> Result<File, &'static str> {
    match openat(
        directory.chain.handle(),
        RECEIPT_FILE,
        OFlags::CREATE | OFlags::EXCL | OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    ) {
        Ok(leaf) => Ok(File::from(leaf)),
        Err(error) if error == Errno::EXIST => Err("receipt_exists"),
        Err(error) if error == Errno::LOOP => Err("receipt_path_symlink"),
        Err(_) => Err("receipt_create_failed"),
    }
}

/// Pin the retained root, exclude it and the evidence directory, open and
/// validate the output directory, and refuse on any uncertainty before any
/// receipt exists.
/// Pin the retained root, exclude it and the evidence directory, bind the
/// containment decision to one checked identity, acquire the canonical path
/// component-wise without following any component, and refuse on any
/// uncertainty before any receipt exists. `before_open` runs after the
/// containment decision and before the non-following acquisition, so tests can
/// exercise substitution in that exact interval.
fn open_output_directory(
    inputs: &Inputs<'_>,
    observe: DirObserver<'_>,
    before_open: &dyn Fn(&Path),
) -> Result<BoundDirectory, &'static str> {
    let canonical_run = inputs
        .run_dir
        .canonicalize()
        .map_err(|_| "receipts_dir_unresolvable")?;
    let canonical_pinned_root = inputs
        .expected_retained_root
        .canonicalize()
        .map_err(|_| "retained_root_unresolvable")?;
    if overlaps(inputs.output_dir, inputs.run_dir) {
        return Err("output_dir_overlaps_evidence");
    }
    if overlaps(inputs.output_dir, inputs.expected_retained_root) {
        return Err("output_dir_overlaps_retained_root");
    }
    // Final-component shape, without following it; ancestors are covered by
    // the canonical exclusion and the component-wise acquisition below.
    match fs::symlink_metadata(inputs.output_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err("output_dir_is_symlink");
        }
        Ok(metadata) if !metadata.is_dir() => return Err("output_dir_not_directory"),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err("output_dir_missing");
        }
        Err(_) => return Err("output_dir_metadata_error"),
    }
    let canonical_output =
        inputs
            .output_dir
            .canonicalize()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => "output_dir_missing",
                std::io::ErrorKind::PermissionDenied => "output_dir_metadata_error",
                _ => "output_dir_unresolvable",
            })?;
    if overlaps(&canonical_output, &canonical_run) {
        return Err("output_dir_overlaps_evidence");
    }
    if overlaps(&canonical_output, &canonical_pinned_root) {
        return Err("output_dir_overlaps_retained_root");
    }
    // The identity the containment decision was made about.
    let checked = lstat(&canonical_output).map_err(|_| "output_dir_metadata_error")?;
    let checked_identity = stat_identity(&checked);
    before_open(&canonical_output);
    // Acquire and retain the whole non-following component chain, with every
    // component name and parent/child identity.
    let output_chain = walk_absolute(&canonical_output)?;
    let facts = observe(output_chain.handle())?;
    if !facts.is_directory {
        return Err("output_dir_not_directory");
    }
    if !facts.private {
        return Err("output_dir_not_private");
    }
    if facts.owner_uid != geteuid().as_raw() {
        return Err("output_dir_not_owned");
    }
    if (facts.device, facts.inode) != checked_identity {
        return Err("output_dir_replaced");
    }
    // Containment by identity against the evidence directory and the pinned
    // retained root, using the retained chains rather than pathname strings.
    containment_reasons(inputs, &output_chain)?;
    Ok(BoundDirectory {
        chain: output_chain,
        canonical_path: canonical_output,
        owner_uid: facts.owner_uid,
    })
}

/// Render the PREPARED record: pinned anchors and static state UNKNOWN only.
fn render_prepared(inputs: &Inputs<'_>, created_unix_seconds: u64) -> String {
    serde_json::json!({
        "schema": "asma-8015-reconciliation-receipt/2",
        "record": "prepared",
        "run_dir": inputs.run_dir.to_string_lossy(),
        "receipts_sha256": inputs.expected_receipts_sha256,
        "source_blob": inputs.expected_source_blob,
        "retained_root": inputs.expected_retained_root.to_string_lossy(),
        "state": "UNKNOWN",
        "created_unix_seconds": created_unix_seconds,
    })
    .to_string()
}

/// The factual effect fields derived from the outcome and the actual counts:
/// `(pre_observation, delete_attempted, delete_outcome, post_observation)`.
/// Raw OSStatus values are never persisted; only these static labels are.
fn effect_facts(
    outcome: Outcome,
    counts: Counts,
) -> (&'static str, bool, &'static str, &'static str) {
    let delete_attempted = counts.deletes > 0;
    match outcome {
        Outcome::AlreadyAbsent => (
            "not_found",
            delete_attempted,
            "not_attempted",
            "not_attempted",
        ),
        Outcome::Deleted => ("present", delete_attempted, "succeeded", "not_found"),
        Outcome::Uncertain("delete_failed") => {
            ("present", delete_attempted, "unproven", "not_attempted")
        }
        Outcome::Uncertain("unexpected_presence") => {
            ("present", delete_attempted, "succeeded", "present")
        }
        Outcome::Uncertain("pre_observation_denied") => {
            ("denied", delete_attempted, "not_attempted", "not_attempted")
        }
        Outcome::Uncertain("pre_observation_unavailable") => (
            "unavailable",
            delete_attempted,
            "not_attempted",
            "not_attempted",
        ),
        Outcome::Uncertain("pre_observation_timeout") => (
            "timed_out",
            delete_attempted,
            "not_attempted",
            "not_attempted",
        ),
        Outcome::Uncertain("pre_observation_unsupported") => (
            "unsupported",
            delete_attempted,
            "not_attempted",
            "not_attempted",
        ),
        Outcome::Uncertain("post_observation_denied") => {
            ("present", delete_attempted, "succeeded", "denied")
        }
        Outcome::Uncertain("post_observation_unavailable") => {
            ("present", delete_attempted, "succeeded", "unavailable")
        }
        Outcome::Uncertain("post_observation_timeout") => {
            ("present", delete_attempted, "succeeded", "timed_out")
        }
        Outcome::Uncertain("post_observation_unsupported") => {
            ("present", delete_attempted, "succeeded", "unsupported")
        }
        Outcome::Uncertain(_) => ("unknown", delete_attempted, "unknown", "unknown"),
        Outcome::Refused(_) => (
            "not_attempted",
            delete_attempted,
            "not_attempted",
            "not_attempted",
        ),
    }
}

/// A static, truthful terminal state label derived from the keychain outcome.
fn terminal_state(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Deleted => "scoped_entry_deleted",
        Outcome::AlreadyAbsent => "scoped_entry_absent",
        Outcome::Refused(_) => "no_backend_effect",
        Outcome::Uncertain(_) => "keychain_state_unknown",
    }
}

/// Render the FINAL record from the outcome, counts, bound identities and
/// timestamps. Fields are derived factually; there is no unconditional claim.
fn render_final(
    inputs: &Inputs<'_>,
    bound: Option<&Bound>,
    outcome: Outcome,
    counts: Counts,
    started_unix_seconds: u64,
    finished_unix_seconds: u64,
) -> String {
    let (pre_observation, delete_attempted, delete_outcome, post_observation) =
        effect_facts(outcome, counts);
    let mut document = serde_json::json!({
        "schema": "asma-8015-reconciliation-receipt/2",
        "record": "final",
        "run_dir": inputs.run_dir.to_string_lossy(),
        "receipts_sha256": inputs.expected_receipts_sha256,
        "source_blob": inputs.expected_source_blob,
        "outcome": outcome.label(),
        "reason": outcome.reason(),
        "terminal_state": terminal_state(outcome),
        "observations": counts.observations,
        "deletes": counts.deletes,
        "pre_observation": pre_observation,
        "delete_attempted": delete_attempted,
        "delete_outcome": delete_outcome,
        "post_observation": post_observation,
        "started_unix_seconds": started_unix_seconds,
        "finished_unix_seconds": finished_unix_seconds,
    });
    if let Some(bound) = bound {
        document["realm"] = serde_json::json!(&bound.realm);
        document["service"] = serde_json::json!(&bound.service);
        document["account"] = serde_json::json!(&bound.account);
        document["retained_root"] = serde_json::json!(bound.retained_root.to_string_lossy());
        document["root_hash"] = serde_json::json!(&bound.root_hash);
        document["ownership"] =
            serde_json::json!("PROVEN_AFTER_ABSENCE_FENCE (immutable frozen receipt)");
    }
    document.to_string()
}

fn complete(mut report: ReconcileReport) -> ReconcileReport {
    report.finished_unix_seconds = unix_now();
    report
}

/// Does the pathname still resolve to the exact inode the sink holds open?
/// An unobservable held identity is an error, never a match.
fn path_matches_sink(sink: &dyn ReceiptSink, path: &Path) -> Result<bool, &'static str> {
    let (device, inode) = sink.file_identity()?;
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.dev() == device && metadata.ino() == inode),
        Err(_) => Ok(false),
    }
}

/// Open, validate, bind, and only then touch the backend.
///
/// The output directory is held open by identity; the PREPARED record is
/// created and synced relative to that handle before binding; the FINAL record
/// is appended through the same held descriptor. Nothing is ever unlinked.
fn reconcile_with_seams<K: KeychainBackend + KeychainWriter + PresenceProbe>(
    inputs: &Inputs<'_>,
    keychain: &K,
    observe_dir: DirObserver<'_>,
    before_open: &dyn Fn(&Path),
    create_sink: SinkFactory<'_>,
    before_create: &dyn Fn(&Path),
    after_prepare: &dyn Fn(&Path),
) -> ReconcileReport {
    let started = unix_now();
    if let Err(reason) = validate_inputs(inputs) {
        return complete(refused(reason, started));
    }
    let bound_directory = match open_output_directory(inputs, observe_dir, before_open) {
        Ok(bound_directory) => bound_directory,
        Err(reason) => return complete(refused(reason, started)),
    };
    before_create(&bound_directory.canonical_path);
    if let Err(reason) = revalidate_directory(inputs, &bound_directory) {
        return complete(refused(reason, started));
    }
    let receipt_path = bound_directory.canonical_path.join(RECEIPT_FILE);
    let mut sink = match create_sink(&bound_directory) {
        Ok(sink) => sink,
        Err(reason) => return complete(refused(reason, started)),
    };
    let prepared = render_prepared(inputs, started);
    if let Err(reason) = sink.append_prepared(&prepared) {
        let mut report = refused(reason, started);
        report.receipt_path = Some(receipt_path);
        report.receipt_state = ReceiptState::PreparedFailed;
        return complete(report);
    }
    after_prepare(&receipt_path);

    let file_uid = match sink.owner_uid() {
        Ok(uid) => uid,
        Err(reason) => {
            let counts = Counts::default();
            let receipt_state = finalize(
                sink.as_mut(),
                inputs,
                None,
                Outcome::Refused(reason),
                counts,
                started,
                &receipt_path,
            );
            return complete(ReconcileReport {
                outcome: Outcome::Refused(reason),
                counts,
                bound: None,
                receipt_path: Some(receipt_path),
                receipt_state,
                started_unix_seconds: started,
                finished_unix_seconds: started,
            });
        }
    };
    if file_uid != bound_directory.owner_uid {
        let reason = "output_dir_owner_mismatch";
        let counts = Counts::default();
        let receipt_state = finalize(
            sink.as_mut(),
            inputs,
            None,
            Outcome::Refused(reason),
            counts,
            started,
            &receipt_path,
        );
        return complete(ReconcileReport {
            outcome: Outcome::Refused(reason),
            counts,
            bound: None,
            receipt_path: Some(receipt_path),
            receipt_state,
            started_unix_seconds: started,
            finished_unix_seconds: started,
        });
    }

    let bound = match bind(inputs) {
        Ok(bound) => bound,
        Err(reason) => {
            let counts = Counts::default();
            let receipt_state = finalize(
                sink.as_mut(),
                inputs,
                None,
                Outcome::Refused(reason),
                counts,
                started,
                &receipt_path,
            );
            return complete(ReconcileReport {
                outcome: Outcome::Refused(reason),
                counts,
                bound: None,
                receipt_path: Some(receipt_path),
                receipt_state,
                started_unix_seconds: started,
                finished_unix_seconds: started,
            });
        }
    };
    let target = KeychainTarget::new(bound.service.clone(), bound.account.clone());
    let mut counts = Counts::default();
    counts.observations += 1;
    let outcome = match keychain.presence(&target) {
        ItemPresence::Present => {
            counts.deletes += 1;
            match keychain.delete_secret(&target) {
                Err(_) => Outcome::Uncertain("delete_failed"),
                Ok(()) => {
                    counts.observations += 1;
                    match keychain.presence(&target) {
                        ItemPresence::Absent => Outcome::Deleted,
                        ItemPresence::Present => Outcome::Uncertain("unexpected_presence"),
                        ItemPresence::Denied => Outcome::Uncertain("post_observation_denied"),
                        ItemPresence::Unavailable => {
                            Outcome::Uncertain("post_observation_unavailable")
                        }
                        ItemPresence::TimedOut => Outcome::Uncertain("post_observation_timeout"),
                        ItemPresence::Unsupported => {
                            Outcome::Uncertain("post_observation_unsupported")
                        }
                    }
                }
            }
        }
        ItemPresence::Absent => Outcome::AlreadyAbsent,
        ItemPresence::Denied => Outcome::Uncertain("pre_observation_denied"),
        ItemPresence::Unavailable => Outcome::Uncertain("pre_observation_unavailable"),
        ItemPresence::TimedOut => Outcome::Uncertain("pre_observation_timeout"),
        ItemPresence::Unsupported => Outcome::Uncertain("pre_observation_unsupported"),
    };
    let receipt_state = finalize(
        sink.as_mut(),
        inputs,
        Some(&bound),
        outcome,
        counts,
        started,
        &receipt_path,
    );
    if receipt_state == ReceiptState::FinalFailed {
        eprintln!(
            "asma-8015 reconciliation: prepared record retained; final record NOT durable; outcome={} reason={}",
            outcome.label(),
            outcome.reason().unwrap_or("none")
        );
    }
    complete(ReconcileReport {
        outcome,
        counts,
        bound: Some(bound),
        receipt_path: Some(receipt_path),
        receipt_state,
        started_unix_seconds: started,
        finished_unix_seconds: started,
    })
}

fn finalize(
    sink: &mut dyn ReceiptSink,
    inputs: &Inputs<'_>,
    bound: Option<&Bound>,
    outcome: Outcome,
    counts: Counts,
    started_unix_seconds: u64,
    receipt_path: &Path,
) -> ReceiptState {
    let record = render_final(
        inputs,
        bound,
        outcome,
        counts,
        started_unix_seconds,
        unix_now(),
    );
    match sink.append_final(&record) {
        Ok(()) => match path_matches_sink(sink, receipt_path) {
            Ok(true) => ReceiptState::Finalized,
            Ok(false) => ReceiptState::PathUnknown,
            Err(_) => ReceiptState::IdentityUnknown,
        },
        Err(_) => ReceiptState::FinalFailed,
    }
}

/// The terminal verdict: only `Deleted` or `AlreadyAbsent` with a `Finalized`
/// receipt is success; everything else is a static failure reason.
fn terminal_verdict(report: &ReconcileReport) -> Result<(), &'static str> {
    if report.receipt_state == ReceiptState::FinalFailed {
        return Err("final_persistence_failed");
    }
    if report.receipt_state == ReceiptState::IdentityUnknown {
        return Err("receipt_identity_unknown");
    }
    if report.receipt_state == ReceiptState::PathUnknown {
        return Err("receipt_path_unknown");
    }
    match report.outcome {
        Outcome::Deleted | Outcome::AlreadyAbsent => match report.receipt_state {
            ReceiptState::Finalized => Ok(()),
            _ => Err("receipt_not_finalized"),
        },
        Outcome::Refused(reason) => Err(reason),
        Outcome::Uncertain(reason) => Err(reason),
    }
}

fn noop_hook(_: &Path) {}

fn reconcile_retained_entry<K: KeychainBackend + KeychainWriter + PresenceProbe>(
    inputs: &Inputs<'_>,
    keychain: &K,
) -> ReconcileReport {
    reconcile_with_seams(
        inputs,
        keychain,
        &observe_directory,
        &noop_hook,
        &FileReceiptSink::create,
        &noop_hook,
        &noop_hook,
    )
}

/// The frozen receipt directory of the failed Tier-A run.
const TIER_A_RUN_DIR: &str = "/private/tmp/asma-8015-tier-a-20261003T073137Z";
/// The frozen receipt manifest hash recorded by that run.
const TIER_A_RECEIPTS_SHA256: &str =
    "d6d8cc5aa4775e0f841a254ac8575f8442df2934360dcc7278683ee9462cd0cc";
/// The frozen failed-case source blob (a 40-character Git object id).
const TIER_A_SOURCE_BLOB: &str = "cd1826d11f2336fb628fe122de5f03af09a47cf7";
/// The frozen retained disposable root recorded by that run (non-secret).
const TIER_A_RETAINED_ROOT: &str = "/var/folders/t3/0tbx772d571_57t01yb9twx00000gn/T/.tmp0QsiLj";
/// The non-secret path input naming the private receipt directory.
const RECONCILE_OUTPUT_ENV: &str = "KONTOR_ASMA8015_RECONCILE_RECEIPT_DIR";

/// Real-backend reconciliation of the one retained synthetic scoped entry.
///
/// It verifies the pinned retained root and immutable receipts, observes the
/// exact target value-free (attributes-only, no secret read), deletes it once
/// through `SystemKeychain` only after a `Present` observation, persists the
/// private receipt and prints a bounded non-secret outcome. Only a finalized
/// `Deleted` or `AlreadyAbsent` returns success; every refusal, uncertainty,
/// unknown path or identity, or final-persistence failure panics with a static
/// reason after the durable evidence is written. It never disposes or writes
/// into the retained root.
#[test]
#[ignore = "macOS platform harness: deletes the one retained synthetic keychain entry through SystemKeychain; run deliberately with an authorized private receipt dir"]
fn the_retained_synthetic_entry_is_reconciled_by_exact_target_delete() {
    let Some(output_dir) = std::env::var_os(RECONCILE_OUTPUT_ENV) else {
        panic!("set `{RECONCILE_OUTPUT_ENV}` to an existing private receipt directory");
    };
    let output_dir = PathBuf::from(output_dir);
    let inputs = Inputs {
        run_dir: Path::new(TIER_A_RUN_DIR),
        expected_receipts_sha256: TIER_A_RECEIPTS_SHA256,
        expected_source_blob: TIER_A_SOURCE_BLOB,
        output_dir: &output_dir,
        expected_retained_root: Path::new(TIER_A_RETAINED_ROOT),
    };
    let report = reconcile_retained_entry(&inputs, &SystemKeychain);
    match terminal_verdict(&report) {
        Ok(()) => println!(
            "outcome={} receipt={:?} observations={} deletes={}",
            report.outcome.label(),
            report.receipt_state,
            report.counts.observations,
            report.counts.deletes
        ),
        Err(reason) => {
            eprintln!("asma-8015 reconciliation failed: {reason}");
            panic!("{reason}");
        }
    }
}

// ---------------------------------------------------------------------------
// Deterministic fake tests: no Keychain, TempDir state only.
// ---------------------------------------------------------------------------

/// Frozen non-secret canaries for fake backend reads.
const FAKE_CANARY_PRE: &str = "synthetic-pre-read-canary";
/// A second canary for the unexpected post-read presence.
const FAKE_CANARY_POST: &str = "synthetic-post-read-canary";

/// Scripted read responses for the fake backend.
#[derive(Clone, Copy)]
enum ScriptedRead {
    NotFound,
    Value(&'static str),
    Denied,
    Unavailable,
}

/// Scripted value-free presence responses.
#[derive(Clone, Copy)]
enum ScriptedPresence {
    Present,
    Absent,
    Denied,
    Unavailable,
    TimedOut,
    Unsupported,
}

/// A scripted backend that records every addressed target and call kind. The
/// `presences` queue scripts value-free observations; the `reads` queue is the
/// poison/spy the reconciliation path must never consume.
struct FakeKeychain {
    reads: Mutex<VecDeque<ScriptedRead>>,
    presences: Mutex<VecDeque<ScriptedPresence>>,
    delete_result: Result<(), KeychainFailure>,
    calls: Mutex<Vec<(&'static str, String, String)>>,
}

impl FakeKeychain {
    fn new(presences: Vec<ScriptedPresence>, delete_result: Result<(), KeychainFailure>) -> Self {
        Self {
            reads: Mutex::new(VecDeque::new()),
            presences: Mutex::new(presences.into()),
            delete_result,
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Poison the value-read queue: reconciliation must never consume it.
    fn with_reads(mut self, reads: Vec<ScriptedRead>) -> Self {
        self.reads = Mutex::new(reads.into());
        self
    }

    fn call_count(&self, kind: &str) -> usize {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .filter(|(recorded, _, _)| *recorded == kind)
            .count()
    }

    fn total_calls(&self) -> usize {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    fn targets(&self) -> Vec<(String, String)> {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|(_, service, account)| (service.clone(), account.clone()))
            .collect()
    }
}

impl PresenceProbe for FakeKeychain {
    fn presence(&self, target: &KeychainTarget) -> ItemPresence {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((
                "presence",
                target.service().to_owned(),
                target.account().to_owned(),
            ));
        match self
            .presences
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .unwrap_or(ScriptedPresence::Unsupported)
        {
            ScriptedPresence::Present => ItemPresence::Present,
            ScriptedPresence::Absent => ItemPresence::Absent,
            ScriptedPresence::Denied => ItemPresence::Denied,
            ScriptedPresence::Unavailable => ItemPresence::Unavailable,
            ScriptedPresence::TimedOut => ItemPresence::TimedOut,
            ScriptedPresence::Unsupported => ItemPresence::Unsupported,
        }
    }
}

impl KeychainBackend for FakeKeychain {
    fn secret(&self, target: &KeychainTarget) -> Result<SecretString, KeychainFailure> {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((
                "secret",
                target.service().to_owned(),
                target.account().to_owned(),
            ));
        match self
            .reads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .unwrap_or(ScriptedRead::Unavailable)
        {
            ScriptedRead::NotFound => Err(KeychainFailure::NotFound),
            ScriptedRead::Value(value) => Ok(SecretString::from(value)),
            ScriptedRead::Denied => Err(KeychainFailure::Denied),
            ScriptedRead::Unavailable => Err(KeychainFailure::Unavailable),
        }
    }
}

impl KeychainWriter for FakeKeychain {
    fn set_secret(
        &self,
        target: &KeychainTarget,
        _secret: &SecretString,
    ) -> Result<(), KeychainFailure> {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((
                "set",
                target.service().to_owned(),
                target.account().to_owned(),
            ));
        Err(KeychainFailure::Unavailable)
    }

    fn delete_secret(&self, target: &KeychainTarget) -> Result<(), KeychainFailure> {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((
                "delete",
                target.service().to_owned(),
                target.account().to_owned(),
            ));
        self.delete_result
    }
}

/// A receipt tamper used to prove one specific binding refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tamper {
    None,
    ReceiptsSha,
    MemberHash,
    MalformedLine,
    Traversal,
    DuplicateMember,
    MissingRequiredMember,
    DuplicateResultKey,
    SourceBlob,
    Outcome,
    Ownership,
    Service,
    Root,
    Account,
    Realm,
    ResultRoot,
}

/// A complete fake receipt fixture deriving the exact target identity.
struct Fixture {
    _run_dir: tempfile::TempDir,
    _root_dir: tempfile::TempDir,
    _other_root_dir: tempfile::TempDir,
    _output_dir: tempfile::TempDir,
    run_dir: PathBuf,
    root_dir: PathBuf,
    output_dir: PathBuf,
    expected_receipts_sha256: String,
    expected_source_blob: String,
    service: String,
    account: String,
}

impl Fixture {
    fn inputs(&self) -> Inputs<'_> {
        Inputs {
            run_dir: &self.run_dir,
            expected_receipts_sha256: &self.expected_receipts_sha256,
            expected_source_blob: &self.expected_source_blob,
            output_dir: &self.output_dir,
            expected_retained_root: &self.root_dir,
        }
    }

    fn inputs_with_output<'a>(&'a self, output_dir: &'a Path) -> Inputs<'a> {
        Inputs {
            run_dir: &self.run_dir,
            expected_receipts_sha256: &self.expected_receipts_sha256,
            expected_source_blob: &self.expected_source_blob,
            output_dir,
            expected_retained_root: &self.root_dir,
        }
    }

    fn receipt_path(&self) -> PathBuf {
        self.output_dir.join(RECEIPT_FILE)
    }
}

/// A `TempDir` forced to mode 700 (the crate default follows the process umask).
fn private_tempdir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a private temp directory");
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))
        .expect("the temp directory is made private");
    dir
}

/// Build a held-directory binding for a real private directory.
fn test_bound_directory(path: &Path) -> Result<BoundDirectory, &'static str> {
    let canonical = path.canonicalize().map_err(|_| "output_dir_unresolvable")?;
    let chain = walk_absolute(&canonical)?;
    let facts = observe_directory(chain.handle())?;
    Ok(BoundDirectory {
        chain,
        canonical_path: canonical,
        owner_uid: facts.owner_uid,
    })
}

fn fixture(tamper: Tamper) -> Fixture {
    let run_dir = private_tempdir();
    let root_dir = private_tempdir();
    let other_root_dir = private_tempdir();
    let output_dir = private_tempdir();
    let realm = RealmId::generate();
    let root_hash = ContentHash::of(
        root_dir
            .path()
            .canonicalize()
            .expect("the fake root canonicalizes")
            .as_os_str()
            .as_encoded_bytes(),
    );
    let service = format!("{SERVICE_PREFIX}:{realm}:{root_hash}");
    let account = format!("asma-8015-harness-{realm}");
    let source_blob = TIER_A_SOURCE_BLOB.to_owned();
    let other_blob = "d".repeat(40);
    let root_text = root_dir.path().display().to_string();

    let mut manifest = format!(
        "ASMA-8015 synthetic reconciliation fixture\n\
         source blob: {source_blob}\n\
         reconciliation_realm: {realm}\n\
         reconciliation_service: {service}\n\
         reconciliation_account: {account}\n\
         retained_root: {root_text}\n\
         reconciliation_ownership: {OWNERSHIP_FENCE}\n\
         case1_harness_outcome: {REQUIRED_OUTCOME} (retained root, no retry, no delete)\n"
    );
    let mut result = format!(
        "case=an_owned_previous_scoped_entry_is_restored_byte_for_byte\n\
         harness_outcome={REQUIRED_OUTCOME}\n\
         exit={REQUIRED_EXIT}\n\
         retained_root={root_text}\n"
    );

    match tamper {
        Tamper::SourceBlob => manifest = manifest.replace(&source_blob, &other_blob),
        Tamper::Outcome => {
            result = result.replace(
                &format!("harness_outcome={REQUIRED_OUTCOME}"),
                "harness_outcome=rolled_back",
            );
        }
        Tamper::Ownership => manifest = manifest.replace(OWNERSHIP_FENCE, "UNPROVEN"),
        Tamper::Service => {
            let replacement = format!("{SERVICE_PREFIX}:{realm}:{}", "e".repeat(64));
            manifest = manifest.replace(&service, &replacement);
        }
        Tamper::Root => {
            manifest = manifest.replace(&root_text, "/private/tmp/asma-8015-missing-root");
        }
        Tamper::Account => manifest = manifest.replace(&account, &"x".repeat(200)),
        Tamper::Realm => manifest = manifest.replace(&realm.to_string(), "not-a-realm"),
        Tamper::ResultRoot => {
            result = result.replace(&root_text, &other_root_dir.path().display().to_string());
        }
        Tamper::DuplicateResultKey => {
            result.push_str(&format!("harness_outcome={REQUIRED_OUTCOME}\n"));
        }
        Tamper::None
        | Tamper::ReceiptsSha
        | Tamper::MemberHash
        | Tamper::MalformedLine
        | Tamper::Traversal
        | Tamper::DuplicateMember
        | Tamper::MissingRequiredMember => {}
    }

    fs::write(run_dir.path().join(RUN_MANIFEST), manifest.as_bytes())
        .expect("the fake run manifest is written");
    fs::write(run_dir.path().join(CASE_RESULT), result.as_bytes())
        .expect("the fake case result is written");

    let manifest_hash = ContentHash::of(manifest.as_bytes());
    let result_hash = ContentHash::of(result.as_bytes());
    let mut receipts = format!("{result_hash}  {CASE_RESULT}\n{manifest_hash}  {RUN_MANIFEST}\n");
    match tamper {
        Tamper::MemberHash => {
            receipts = receipts.replace(&result_hash.to_string(), &"0".repeat(64));
        }
        Tamper::MalformedLine => receipts.push_str("this is not a manifest line\n"),
        Tamper::Traversal => receipts.push_str(&format!("{}  ../escape\n", "a".repeat(64))),
        Tamper::DuplicateMember => {
            receipts.push_str(&format!("{manifest_hash}  {RUN_MANIFEST}\n"));
        }
        Tamper::MissingRequiredMember => {
            receipts = receipts
                .lines()
                .filter(|line| !line.ends_with(RUN_MANIFEST))
                .map(|line| format!("{line}\n"))
                .collect();
        }
        _ => {}
    }
    fs::write(run_dir.path().join(RECEIPTS_MANIFEST), receipts.as_bytes())
        .expect("the fake receipts manifest is written");
    let mut expected_receipts_sha256 = ContentHash::of(receipts.as_bytes()).to_string();
    if tamper == Tamper::ReceiptsSha {
        let mut characters: Vec<char> = expected_receipts_sha256.chars().collect();
        characters[0] = if characters[0] == '0' { '1' } else { '0' };
        expected_receipts_sha256 = characters.into_iter().collect();
    }

    let run_path = run_dir.path().to_path_buf();
    let root_path = root_dir.path().to_path_buf();
    let output_path = output_dir.path().to_path_buf();
    Fixture {
        _run_dir: run_dir,
        _root_dir: root_dir,
        _other_root_dir: other_root_dir,
        _output_dir: output_dir,
        run_dir: run_path,
        root_dir: root_path,
        output_dir: output_path,
        expected_receipts_sha256,
        expected_source_blob: source_blob,
        service,
        account,
    }
}

fn assert_exact_single_target(fake: &FakeKeychain, fixture: &Fixture) {
    let targets = fake.targets();
    assert!(!targets.is_empty(), "the backend must have been reached");
    for (service, account) in targets {
        assert_eq!(
            service, fixture.service,
            "only the exact service is addressed"
        );
        assert_eq!(
            account, fixture.account,
            "only the exact account is addressed"
        );
    }
}

fn directory_contains(root: &Path, needle: &str) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if directory_contains(&path, needle) {
                return true;
            }
        } else if let Ok(bytes) = fs::read(&path)
            && String::from_utf8_lossy(&bytes).contains(needle)
        {
            return true;
        }
    }
    false
}

fn read_records(path: &Path) -> Vec<serde_json::Value> {
    let text = fs::read_to_string(path).expect("the receipt is readable text");
    text.lines()
        .map(|line| serde_json::from_str(line).expect("each receipt record is JSON"))
        .collect()
}

fn assert_prepared_record(record: &serde_json::Value, fixture: &Fixture) {
    assert_eq!(record["record"], "prepared");
    assert_eq!(record["state"], "UNKNOWN");
    assert_eq!(record["receipts_sha256"], fixture.expected_receipts_sha256);
    assert_eq!(record["source_blob"], fixture.expected_source_blob);
}

/// Expected factual fields of the FINAL record.
struct ExpectedFinal {
    outcome: &'static str,
    reason: Option<&'static str>,
    observations: u64,
    deletes: u64,
    pre_observation: &'static str,
    delete_attempted: bool,
    delete_outcome: &'static str,
    post_observation: &'static str,
    terminal_state: &'static str,
}

fn assert_final_record(record: &serde_json::Value, expected: &ExpectedFinal) {
    assert_eq!(record["record"], "final");
    assert_eq!(record["outcome"], expected.outcome);
    assert_eq!(record["reason"].as_str(), expected.reason);
    assert_eq!(record["observations"], expected.observations);
    assert_eq!(record["deletes"], expected.deletes);
    assert_eq!(record["pre_observation"], expected.pre_observation);
    assert_eq!(record["delete_attempted"], expected.delete_attempted);
    assert_eq!(record["delete_outcome"], expected.delete_outcome);
    assert_eq!(record["post_observation"], expected.post_observation);
    assert_eq!(record["terminal_state"], expected.terminal_state);
}

fn assert_records(path: &Path, fixture: &Fixture, expected: &ExpectedFinal) {
    let records = read_records(path);
    assert_eq!(records.len(), 2, "prepared and final records are present");
    assert_prepared_record(&records[0], fixture);
    assert_final_record(&records[1], expected);
}

fn synthetic_report(
    outcome: Outcome,
    counts: Counts,
    receipt_state: ReceiptState,
) -> ReconcileReport {
    ReconcileReport {
        outcome,
        counts,
        bound: None,
        receipt_path: None,
        receipt_state,
        started_unix_seconds: 0,
        finished_unix_seconds: 0,
    }
}

#[test]
fn a_valid_receipt_binds_and_an_absent_target_needs_no_delete() {
    let fixture = fixture(Tamper::None);
    let fake = FakeKeychain::new(vec![ScriptedPresence::Absent], Ok(()));
    let report = reconcile_retained_entry(&fixture.inputs(), &fake);
    assert_eq!(report.outcome, Outcome::AlreadyAbsent);
    assert_eq!(report.receipt_state, ReceiptState::Finalized);
    assert_eq!(
        report.counts,
        Counts {
            observations: 1,
            deletes: 0
        }
    );
    assert_eq!(fake.call_count("secret"), 0, "no value read");
    assert_eq!(fake.call_count("presence"), 1);
    assert_eq!(fake.call_count("delete"), 0);
    assert_eq!(fake.call_count("set"), 0);
    assert_exact_single_target(&fake, &fixture);
    assert!(terminal_verdict(&report).is_ok());
    let bound = report
        .bound
        .as_ref()
        .expect("bound identities are retained");
    assert_eq!(bound.run_dir, fixture.run_dir);
    assert_eq!(bound.service, fixture.service);
    assert_eq!(bound.account, fixture.account);
    assert!(!bound.realm.is_empty());
    assert_eq!(bound.root_hash.len(), 64);
    assert!(bound.retained_root.exists());
    assert!(report.finished_unix_seconds >= report.started_unix_seconds);
    assert_records(
        &fixture.receipt_path(),
        &fixture,
        &ExpectedFinal {
            outcome: "already_absent",
            reason: None,
            observations: 1,
            deletes: 0,
            pre_observation: "not_found",
            delete_attempted: false,
            delete_outcome: "not_attempted",
            post_observation: "not_attempted",
            terminal_state: "scoped_entry_absent",
        },
    );
}

#[test]
fn a_present_target_is_deleted_and_absence_is_verified() {
    let fixture = fixture(Tamper::None);
    let fake = FakeKeychain::new(
        vec![ScriptedPresence::Present, ScriptedPresence::Absent],
        Ok(()),
    );
    let report = reconcile_retained_entry(&fixture.inputs(), &fake);
    assert_eq!(report.outcome, Outcome::Deleted);
    assert_eq!(report.receipt_state, ReceiptState::Finalized);
    assert_eq!(
        report.counts,
        Counts {
            observations: 2,
            deletes: 1
        }
    );
    assert_eq!(fake.call_count("secret"), 0, "no value read");
    assert_eq!(fake.call_count("presence"), 2);
    assert_eq!(fake.call_count("delete"), 1);
    assert_exact_single_target(&fake, &fixture);
    assert!(terminal_verdict(&report).is_ok());
    assert_records(
        &fixture.receipt_path(),
        &fixture,
        &ExpectedFinal {
            outcome: "deleted",
            reason: None,
            observations: 2,
            deletes: 1,
            pre_observation: "present",
            delete_attempted: true,
            delete_outcome: "succeeded",
            post_observation: "not_found",
            terminal_state: "scoped_entry_deleted",
        },
    );
}

#[test]
fn a_denied_or_unavailable_pre_observation_is_uncertain_without_delete() {
    for (scripted, reason, pre_observation) in [
        (ScriptedPresence::Denied, "pre_observation_denied", "denied"),
        (
            ScriptedPresence::Unavailable,
            "pre_observation_unavailable",
            "unavailable",
        ),
    ] {
        let fixture = fixture(Tamper::None);
        let fake = FakeKeychain::new(vec![scripted], Ok(()));
        let report = reconcile_retained_entry(&fixture.inputs(), &fake);
        assert_eq!(report.outcome, Outcome::Uncertain(reason));
        assert_eq!(report.receipt_state, ReceiptState::Finalized);
        assert_eq!(
            report.counts,
            Counts {
                observations: 1,
                deletes: 0
            }
        );
        assert_eq!(fake.call_count("secret"), 0, "no value read");
        assert_eq!(fake.call_count("presence"), 1);
        assert_eq!(fake.call_count("delete"), 0);
        assert_exact_single_target(&fake, &fixture);
        assert_eq!(terminal_verdict(&report), Err(reason));
        assert_records(
            &fixture.receipt_path(),
            &fixture,
            &ExpectedFinal {
                outcome: "uncertain",
                reason: Some(reason),
                observations: 1,
                deletes: 0,
                pre_observation,
                delete_attempted: false,
                delete_outcome: "not_attempted",
                post_observation: "not_attempted",
                terminal_state: "keychain_state_unknown",
            },
        );
    }
}

#[test]
fn a_timed_out_or_unsupported_pre_observation_is_uncertain_without_delete() {
    for (presences, reason, pre_observation) in [
        (
            vec![ScriptedPresence::TimedOut],
            "pre_observation_timeout",
            "timed_out",
        ),
        (
            vec![ScriptedPresence::Unsupported],
            "pre_observation_unsupported",
            "unsupported",
        ),
        (Vec::new(), "pre_observation_unsupported", "unsupported"),
    ] {
        let fixture = fixture(Tamper::None);
        let fake = FakeKeychain::new(presences, Ok(()));
        let report = reconcile_retained_entry(&fixture.inputs(), &fake);
        assert_eq!(report.outcome, Outcome::Uncertain(reason));
        assert_eq!(report.receipt_state, ReceiptState::Finalized);
        assert_eq!(
            report.counts,
            Counts {
                observations: 1,
                deletes: 0
            }
        );
        assert_eq!(fake.call_count("secret"), 0, "no value read");
        assert_eq!(fake.call_count("presence"), 1);
        assert_eq!(fake.call_count("delete"), 0);
        assert_eq!(terminal_verdict(&report), Err(reason));
        assert_records(
            &fixture.receipt_path(),
            &fixture,
            &ExpectedFinal {
                outcome: "uncertain",
                reason: Some(reason),
                observations: 1,
                deletes: 0,
                pre_observation,
                delete_attempted: false,
                delete_outcome: "not_attempted",
                post_observation: "not_attempted",
                terminal_state: "keychain_state_unknown",
            },
        );
    }
}

#[test]
fn a_delete_error_is_uncertain_without_retry() {
    let fixture = fixture(Tamper::None);
    let fake = FakeKeychain::new(
        vec![ScriptedPresence::Present],
        Err(KeychainFailure::Denied),
    );
    let report = reconcile_retained_entry(&fixture.inputs(), &fake);
    assert_eq!(report.outcome, Outcome::Uncertain("delete_failed"));
    assert_eq!(report.receipt_state, ReceiptState::Finalized);
    assert_eq!(
        report.counts,
        Counts {
            observations: 1,
            deletes: 1
        }
    );
    assert_eq!(fake.call_count("secret"), 0, "no value read");
    assert_eq!(fake.call_count("presence"), 1);
    assert_eq!(fake.call_count("delete"), 1, "no delete retry");
    assert_eq!(fake.total_calls(), 2, "no call follows a delete error");
    assert_exact_single_target(&fake, &fixture);
    assert_eq!(terminal_verdict(&report), Err("delete_failed"));
    assert_records(
        &fixture.receipt_path(),
        &fixture,
        &ExpectedFinal {
            outcome: "uncertain",
            reason: Some("delete_failed"),
            observations: 1,
            deletes: 1,
            pre_observation: "present",
            delete_attempted: true,
            delete_outcome: "unproven",
            post_observation: "not_attempted",
            terminal_state: "keychain_state_unknown",
        },
    );
}

#[test]
fn an_unexpected_post_observation_presence_is_uncertain_and_never_materialized() {
    let fixture = fixture(Tamper::None);
    let fake = FakeKeychain::new(
        vec![ScriptedPresence::Present, ScriptedPresence::Present],
        Ok(()),
    )
    .with_reads(vec![
        ScriptedRead::Value(FAKE_CANARY_PRE),
        ScriptedRead::Value(FAKE_CANARY_POST),
    ]);
    let report = reconcile_retained_entry(&fixture.inputs(), &fake);
    assert_eq!(report.outcome, Outcome::Uncertain("unexpected_presence"));
    assert_eq!(report.receipt_state, ReceiptState::Finalized);
    assert_eq!(
        report.counts,
        Counts {
            observations: 2,
            deletes: 1
        }
    );
    assert_eq!(fake.call_count("secret"), 0, "no value read");
    assert_eq!(fake.call_count("presence"), 2);
    assert_eq!(fake.call_count("delete"), 1);
    assert_exact_single_target(&fake, &fixture);
    assert_eq!(terminal_verdict(&report), Err("unexpected_presence"));
    assert_records(
        &fixture.receipt_path(),
        &fixture,
        &ExpectedFinal {
            outcome: "uncertain",
            reason: Some("unexpected_presence"),
            observations: 2,
            deletes: 1,
            pre_observation: "present",
            delete_attempted: true,
            delete_outcome: "succeeded",
            post_observation: "present",
            terminal_state: "keychain_state_unknown",
        },
    );
    assert!(!directory_contains(&fixture.output_dir, FAKE_CANARY_PRE));
    assert!(!directory_contains(&fixture.output_dir, FAKE_CANARY_POST));
}

#[test]
fn a_post_observation_short_of_absence_is_uncertain() {
    for (presences, reason, post_observation) in [
        (
            vec![ScriptedPresence::Present, ScriptedPresence::Denied],
            "post_observation_denied",
            "denied",
        ),
        (
            vec![ScriptedPresence::Present, ScriptedPresence::Unavailable],
            "post_observation_unavailable",
            "unavailable",
        ),
        (
            vec![ScriptedPresence::Present, ScriptedPresence::TimedOut],
            "post_observation_timeout",
            "timed_out",
        ),
        (
            vec![ScriptedPresence::Present, ScriptedPresence::Unsupported],
            "post_observation_unsupported",
            "unsupported",
        ),
        (
            vec![ScriptedPresence::Present],
            "post_observation_unsupported",
            "unsupported",
        ),
    ] {
        let fixture = fixture(Tamper::None);
        let fake = FakeKeychain::new(presences, Ok(()));
        let report = reconcile_retained_entry(&fixture.inputs(), &fake);
        assert_eq!(report.outcome, Outcome::Uncertain(reason));
        assert_eq!(report.receipt_state, ReceiptState::Finalized);
        assert_eq!(
            report.counts,
            Counts {
                observations: 2,
                deletes: 1
            }
        );
        assert_eq!(fake.call_count("secret"), 0, "no value read");
        assert_eq!(fake.call_count("presence"), 2);
        assert_eq!(fake.call_count("delete"), 1, "no delete retry");
        assert_exact_single_target(&fake, &fixture);
        assert_eq!(terminal_verdict(&report), Err(reason));
        assert_records(
            &fixture.receipt_path(),
            &fixture,
            &ExpectedFinal {
                outcome: "uncertain",
                reason: Some(reason),
                observations: 2,
                deletes: 1,
                pre_observation: "present",
                delete_attempted: true,
                delete_outcome: "succeeded",
                post_observation,
                terminal_state: "keychain_state_unknown",
            },
        );
    }
}

#[test]
fn presence_query_is_exact_and_limited_to_attributes() {
    let mut pairs = value_free_observation::describe_query("exact-service", "exact-account");
    assert_eq!(pairs.len(), 6, "exactly six query keys");
    pairs.sort();
    assert_eq!(
        pairs,
        vec![
            (
                "kSecAttrAccount".to_owned(),
                "string:exact-account".to_owned(),
            ),
            (
                "kSecAttrService".to_owned(),
                "string:exact-service".to_owned(),
            ),
            (
                "kSecClass".to_owned(),
                "kSecClassGenericPassword".to_owned(),
            ),
            ("kSecMatchLimit".to_owned(), "number:1".to_owned()),
            ("kSecReturnAttributes".to_owned(), "bool:true".to_owned()),
            (
                "kSecUseAuthenticationUI".to_owned(),
                "kSecUseAuthenticationUIFail".to_owned(),
            ),
        ]
    );
    let forbidden = [
        "kSecReturnData",
        "kSecReturnRef",
        "kSecMatchSearchList",
        "kSecUseDataProtectionKeychain",
        "kSecMatchCaseInsensitive",
        "kSecAttrSynchronizable",
        "kSecUseAuthenticationUISkip",
    ];
    for name in forbidden {
        assert!(
            !pairs.iter().any(|(key, _)| key.as_str() == name),
            "the forbidden key {name} must be absent"
        );
    }
}

#[test]
fn status_mapping_is_exhaustive() {
    assert_eq!(value_free_observation::map_status(0), ItemPresence::Present);
    assert_eq!(
        value_free_observation::map_status(-25300),
        ItemPresence::Absent
    );
    assert_eq!(
        value_free_observation::map_status(-25293),
        ItemPresence::Denied
    );
    assert_eq!(
        value_free_observation::map_status(-25308),
        ItemPresence::Unavailable
    );
    assert_eq!(
        value_free_observation::map_status(-25291),
        ItemPresence::Unavailable
    );
    assert_eq!(
        value_free_observation::map_status(12_345),
        ItemPresence::Unavailable,
        "an unrecognized status is never Absent"
    );
}

#[test]
fn bounded_observation_never_blocks_and_never_invents_presence() {
    assert_eq!(
        value_free_observation::observe_bounded(
            || ItemPresence::Present,
            Duration::from_millis(100),
        ),
        ItemPresence::Present
    );
    assert_eq!(
        value_free_observation::observe_bounded(
            || {
                std::thread::sleep(Duration::from_millis(300));
                ItemPresence::Present
            },
            Duration::from_millis(50),
        ),
        ItemPresence::TimedOut,
        "a late read-only result is discarded, never an answer"
    );
    assert_eq!(
        value_free_observation::observe_bounded(
            || panic!("the observation worker panicked"),
            Duration::from_millis(500),
        ),
        ItemPresence::Unavailable,
        "a panicked worker is uncertainty"
    );
}

#[test]
fn a_value_read_regression_would_consume_the_poison_canary() {
    for poison in [
        ScriptedRead::NotFound,
        ScriptedRead::Value(FAKE_CANARY_PRE),
        ScriptedRead::Denied,
        ScriptedRead::Unavailable,
    ] {
        let fixture = fixture(Tamper::None);
        let fake =
            FakeKeychain::new(vec![ScriptedPresence::Absent], Ok(())).with_reads(vec![poison]);
        let report = reconcile_retained_entry(&fixture.inputs(), &fake);
        assert_eq!(report.outcome, Outcome::AlreadyAbsent);
        assert_eq!(
            report.counts,
            Counts {
                observations: 1,
                deletes: 0
            }
        );
        assert_eq!(
            fake.call_count("secret"),
            0,
            "reconciliation never reads the secret"
        );
        assert_eq!(fake.call_count("presence"), 1);
        assert_eq!(fake.call_count("delete"), 0);
        assert_eq!(fake.call_count("set"), 0);
        assert!(!directory_contains(&fixture.output_dir, FAKE_CANARY_PRE));
    }
}

#[test]
fn every_receipt_mismatch_refuses_before_any_backend_call() {
    let cases = [
        (Tamper::ReceiptsSha, "receipts_sha256_mismatch"),
        (Tamper::MemberHash, "receipts_member_hash_mismatch"),
        (Tamper::MalformedLine, "receipts_manifest_malformed"),
        (Tamper::Traversal, "receipts_member_traversal"),
        (Tamper::DuplicateMember, "receipts_member_duplicate"),
        (
            Tamper::MissingRequiredMember,
            "receipts_required_member_missing",
        ),
        (Tamper::DuplicateResultKey, "result_duplicate_key"),
        (Tamper::SourceBlob, "manifest_source_blob_mismatch"),
        (Tamper::Outcome, "result_outcome_mismatch"),
        (Tamper::Ownership, "manifest_ownership_mismatch"),
        (Tamper::Service, "manifest_service_mismatch"),
        (Tamper::Root, "manifest_retained_root_mismatch"),
        (Tamper::Account, "manifest_account_invalid"),
        (Tamper::Realm, "manifest_realm_invalid"),
        (Tamper::ResultRoot, "result_retained_root_mismatch"),
    ];
    for (tamper, expected_reason) in cases {
        let fixture = fixture(tamper);
        let fake = FakeKeychain::new(Vec::new(), Ok(()));
        let report = reconcile_retained_entry(&fixture.inputs(), &fake);
        assert_eq!(
            report.outcome,
            Outcome::Refused(expected_reason),
            "tamper {tamper:?}"
        );
        assert_eq!(report.counts, Counts::default(), "tamper {tamper:?}");
        assert_eq!(
            report.receipt_state,
            ReceiptState::Finalized,
            "tamper {tamper:?}"
        );
        assert_eq!(fake.total_calls(), 0, "tamper {tamper:?}");
        assert_eq!(
            terminal_verdict(&report),
            Err(expected_reason),
            "tamper {tamper:?}"
        );
        let records = read_records(&fixture.receipt_path());
        assert_eq!(records.len(), 2, "tamper {tamper:?}");
        assert_prepared_record(&records[0], &fixture);
        assert_eq!(records[1]["record"], "final", "tamper {tamper:?}");
        assert_eq!(records[1]["outcome"], "refused", "tamper {tamper:?}");
        assert_eq!(
            records[1]["reason"].as_str(),
            Some(expected_reason),
            "tamper {tamper:?}"
        );
        assert_eq!(records[1]["observations"], 0, "tamper {tamper:?}");
        assert_eq!(records[1]["deletes"], 0, "tamper {tamper:?}");
    }
}

#[test]
fn malformed_expected_anchors_refuse_before_any_backend_call() {
    let fixture = fixture(Tamper::None);
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let mut long_receipts = fixture.expected_receipts_sha256.clone();
    long_receipts.push('0');
    let mut non_hex_blob = "z".to_owned();
    non_hex_blob.push_str(&"a".repeat(39));
    let short_blob = "a".repeat(39);
    let cases = [
        (long_receipts.as_str(), TIER_A_SOURCE_BLOB),
        (
            fixture.expected_receipts_sha256.as_str(),
            non_hex_blob.as_str(),
        ),
        (
            fixture.expected_receipts_sha256.as_str(),
            short_blob.as_str(),
        ),
    ];
    for (receipts, blob) in cases {
        let inputs = Inputs {
            run_dir: &fixture.run_dir,
            expected_receipts_sha256: receipts,
            expected_source_blob: blob,
            output_dir: &fixture.output_dir,
            expected_retained_root: &fixture.root_dir,
        };
        let report = reconcile_retained_entry(&inputs, &fake);
        assert_eq!(
            report.outcome,
            Outcome::Refused("expected_hashes_malformed")
        );
        assert_eq!(report.receipt_state, ReceiptState::NotCreated);
        assert_eq!(report.counts, Counts::default());
        assert_eq!(fake.total_calls(), 0);
        assert!(!fixture.receipt_path().exists());
    }
}

#[test]
fn a_missing_pinned_retained_root_refuses_before_any_write() {
    let fixture = fixture(Tamper::None);
    let missing = fixture.output_dir.join("missing-root");
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let inputs = Inputs {
        run_dir: &fixture.run_dir,
        expected_receipts_sha256: &fixture.expected_receipts_sha256,
        expected_source_blob: &fixture.expected_source_blob,
        output_dir: &fixture.output_dir,
        expected_retained_root: &missing,
    };
    let report = reconcile_retained_entry(&inputs, &fake);
    assert_eq!(
        report.outcome,
        Outcome::Refused("retained_root_unresolvable")
    );
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(report.counts, Counts::default());
    assert_eq!(fake.total_calls(), 0);
    assert!(!fixture.receipt_path().exists());
}

fn expect_refused(fixture: &Fixture, output_dir: &Path, reason: &'static str) {
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let report = reconcile_retained_entry(&fixture.inputs_with_output(output_dir), &fake);
    assert_eq!(report.outcome, Outcome::Refused(reason));
    assert_eq!(report.counts, Counts::default());
    assert_eq!(fake.total_calls(), 0);
    assert_eq!(terminal_verdict(&report), Err(reason));
}

#[test]
fn output_directories_overlapping_the_pinned_retained_root_create_no_artifact() {
    let fixture = fixture(Tamper::None);
    expect_refused(
        &fixture,
        &fixture.root_dir,
        "output_dir_overlaps_retained_root",
    );
    assert!(!fixture.root_dir.join(RECEIPT_FILE).exists());

    let inside = fixture.root_dir.join("inside");
    fs::create_dir(&inside).expect("a directory");
    fs::set_permissions(&inside, fs::Permissions::from_mode(0o700)).expect("mode");
    expect_refused(&fixture, &inside, "output_dir_overlaps_retained_root");
    assert!(!inside.join(RECEIPT_FILE).exists());

    let container = private_tempdir();
    let nested_root = container.path().join("nested-root");
    fs::create_dir(&nested_root).expect("a directory");
    fs::set_permissions(&nested_root, fs::Permissions::from_mode(0o700)).expect("mode");
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let inputs = Inputs {
        run_dir: &fixture.run_dir,
        expected_receipts_sha256: &fixture.expected_receipts_sha256,
        expected_source_blob: &fixture.expected_source_blob,
        output_dir: container.path(),
        expected_retained_root: &nested_root,
    };
    let report = reconcile_retained_entry(&inputs, &fake);
    assert_eq!(
        report.outcome,
        Outcome::Refused("output_dir_overlaps_retained_root")
    );
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(fake.total_calls(), 0);
    assert!(!container.path().join(RECEIPT_FILE).exists());
}

#[test]
fn receipt_output_boundaries_refuse_before_any_backend_call() {
    let fixture = fixture(Tamper::None);

    let non_private = fixture.output_dir.join("non-private");
    fs::create_dir(&non_private).expect("a directory");
    fs::set_permissions(&non_private, fs::Permissions::from_mode(0o755)).expect("mode");
    expect_refused(&fixture, &non_private, "output_dir_not_private");
    assert!(!non_private.join(RECEIPT_FILE).exists());

    let real_private = fixture.output_dir.join("real-private");
    fs::create_dir(&real_private).expect("a directory");
    fs::set_permissions(&real_private, fs::Permissions::from_mode(0o700)).expect("mode");
    let link = fixture.output_dir.join("symlinked-output");
    symlink(&real_private, &link).expect("a symlink");
    expect_refused(&fixture, &link, "output_dir_is_symlink");
    assert!(!real_private.join(RECEIPT_FILE).exists());

    let nested = fixture.run_dir.join("nested");
    fs::create_dir(&nested).expect("a directory");
    fs::set_permissions(&nested, fs::Permissions::from_mode(0o700)).expect("mode");
    expect_refused(&fixture, &nested, "output_dir_overlaps_evidence");
    assert!(!nested.join(RECEIPT_FILE).exists());
    expect_refused(&fixture, &fixture.run_dir, "output_dir_overlaps_evidence");
    assert!(!fixture.receipt_path().exists());

    let existing = fixture.output_dir.join(RECEIPT_FILE);
    fs::write(&existing, b"existing").expect("an existing receipt");
    expect_refused(&fixture, &fixture.output_dir, "receipt_exists");
    assert_eq!(
        fs::read(&existing).expect("the planted leaf is unreadable"),
        b"existing",
        "a planted leaf is never touched"
    );
    fs::remove_file(&existing).expect("the probe receipt is removed");

    let receipt_link_target = fixture.output_dir.join("receipt-link-target");
    fs::write(&receipt_link_target, b"target").expect("a target");
    let receipt_link = fixture.output_dir.join(RECEIPT_FILE);
    symlink(&receipt_link_target, &receipt_link).expect("a receipt symlink");
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let report = reconcile_retained_entry(&fixture.inputs(), &fake);
    assert!(
        matches!(
            report.outcome,
            Outcome::Refused("receipt_exists") | Outcome::Refused("receipt_path_symlink")
        ),
        "a planted symlink leaf is refused: {:?}",
        report.outcome
    );
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(fake.total_calls(), 0);
    assert_eq!(
        fs::read(&receipt_link_target).expect("the target is readable"),
        b"target",
        "the symlink target is never written"
    );
    fs::remove_file(&receipt_link).expect("the receipt symlink is removed");
    fs::remove_file(&receipt_link_target).expect("the target is removed");

    // Unwritable directory: opening the directory succeeds, the leaf create
    // fails with zero backend calls.
    let unwritable = fixture.output_dir.join("unwritable");
    fs::create_dir(&unwritable).expect("a directory");
    fs::set_permissions(&unwritable, fs::Permissions::from_mode(0o500)).expect("mode");
    let probe = unwritable.join("probe");
    if fs::File::create(&probe).is_ok() {
        // Running privileged: the write probe would not fail, so skip.
        let _ = fs::remove_file(&probe);
        fs::set_permissions(&unwritable, fs::Permissions::from_mode(0o700)).expect("mode");
    } else {
        expect_refused(&fixture, &unwritable, "receipt_create_failed");
        assert!(!unwritable.join(RECEIPT_FILE).exists());
        fs::set_permissions(&unwritable, fs::Permissions::from_mode(0o700)).expect("mode");
    }

    // Metadata error (unreadable parent) refuses; it never means absence.
    let blocked = fixture.output_dir.join("blocked");
    let blocked_child = blocked.join("child");
    fs::create_dir_all(&blocked_child).expect("a blocked tree");
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).expect("mode");
    let probe = blocked.join("probe");
    if fs::File::create(&probe).is_ok() {
        // Running privileged: metadata access is not denied, so skip.
        let _ = fs::remove_file(&probe);
    } else {
        let fake = FakeKeychain::new(Vec::new(), Ok(()));
        let report = reconcile_retained_entry(&fixture.inputs_with_output(&blocked_child), &fake);
        assert_eq!(
            report.outcome,
            Outcome::Refused("output_dir_metadata_error")
        );
        assert_eq!(report.receipt_state, ReceiptState::NotCreated);
        assert_eq!(fake.total_calls(), 0);
    }
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).expect("mode");
}

#[test]
fn a_symlinked_output_directory_is_refused_by_nofollow() {
    let fixture = fixture(Tamper::None);
    let real = private_tempdir();
    let link_parent = private_tempdir();
    let link = link_parent.path().join("link");
    symlink(real.path(), &link).expect("a symlink");
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let report = reconcile_retained_entry(&fixture.inputs_with_output(&link), &fake);
    assert_eq!(report.outcome, Outcome::Refused("output_dir_is_symlink"));
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(fake.total_calls(), 0);
    assert!(!real.path().join(RECEIPT_FILE).exists());
}

#[test]
fn output_directory_ancestor_symlink_substitution_before_open_is_refused() {
    let fixture = fixture(Tamper::None);
    let base = fixture.output_dir.join("base");
    let out = base.join("output");
    fs::create_dir(&base).expect("a base directory");
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).expect("mode");
    fs::create_dir(&out).expect("the output directory");
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700)).expect("mode");
    let trap = fixture.root_dir.join("output");
    fs::create_dir(&trap).expect("a trap directory inside the retained root");
    fs::set_permissions(&trap, fs::Permissions::from_mode(0o700)).expect("mode");
    let moved = fixture.output_dir.join("moved-base");
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let hook = |_: &Path| {
        fs::rename(&base, &moved).expect("the ancestor is renamed away");
        symlink(&fixture.root_dir, &base).expect("the ancestor becomes a symlink");
    };
    let report = reconcile_with_seams(
        &fixture.inputs_with_output(&out),
        &fake,
        &observe_directory,
        &hook,
        &FileReceiptSink::create,
        &noop_hook,
        &noop_hook,
    );
    assert_eq!(report.outcome, Outcome::Refused("output_dir_is_symlink"));
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(report.counts, Counts::default());
    assert_eq!(fake.total_calls(), 0);
    assert!(
        !trap.join(RECEIPT_FILE).exists(),
        "no receipt inside the retained root"
    );
    assert!(
        !moved.join("output").join(RECEIPT_FILE).exists(),
        "no receipt in the renamed ancestor"
    );
}

#[test]
fn output_directory_ancestor_real_substitution_before_open_is_refused() {
    let fixture = fixture(Tamper::None);
    let base = fixture.output_dir.join("base");
    let out = base.join("output");
    fs::create_dir(&base).expect("a base directory");
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).expect("mode");
    fs::create_dir(&out).expect("the output directory");
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700)).expect("mode");
    let moved = fixture.output_dir.join("moved-base");
    let substitute_out = fixture.output_dir.join("base").join("output");
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let hook = |_: &Path| {
        fs::rename(&base, &moved).expect("the ancestor is renamed away");
        let substitute_base = fixture.output_dir.join("base");
        fs::create_dir(&substitute_base).expect("a substitute ancestor exists");
        fs::set_permissions(&substitute_base, fs::Permissions::from_mode(0o700)).expect("mode");
        let substitute_out = substitute_base.join("output");
        fs::create_dir(&substitute_out).expect("a substitute output exists");
        fs::set_permissions(&substitute_out, fs::Permissions::from_mode(0o700)).expect("mode");
    };
    let report = reconcile_with_seams(
        &fixture.inputs_with_output(&out),
        &fake,
        &observe_directory,
        &hook,
        &FileReceiptSink::create,
        &noop_hook,
        &noop_hook,
    );
    assert_eq!(report.outcome, Outcome::Refused("output_dir_replaced"));
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(report.counts, Counts::default());
    assert_eq!(fake.total_calls(), 0);
    assert!(!substitute_out.join(RECEIPT_FILE).exists());
    assert!(!moved.join("output").join(RECEIPT_FILE).exists());
}

#[test]
fn an_ancestor_alias_at_the_create_boundary_is_refused() {
    let fixture = fixture(Tamper::None);
    let base = fixture.output_dir.join("base");
    let out = base.join("output");
    fs::create_dir(&base).expect("a base directory");
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).expect("mode");
    fs::create_dir(&out).expect("the output directory");
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700)).expect("mode");
    let moved = fixture.root_dir.join("nested-base");
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let hook = |_: &Path| {
        fs::rename(&base, &moved).expect("the ancestor is moved under the retained root");
        symlink(&moved, &base).expect("an alias preserves the old pathname");
    };
    let report = reconcile_with_seams(
        &fixture.inputs_with_output(&out),
        &fake,
        &observe_directory,
        &noop_hook,
        &FileReceiptSink::create,
        &hook,
        &noop_hook,
    );
    assert_eq!(report.outcome, Outcome::Refused("output_dir_replaced"));
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(report.counts, Counts::default());
    assert_eq!(fake.total_calls(), 0);
    assert!(
        !moved.join("output").join(RECEIPT_FILE).exists(),
        "no receipt beneath the retained root"
    );
}

#[test]
fn an_ancestor_real_substitution_at_the_create_boundary_is_refused() {
    let fixture = fixture(Tamper::None);
    let base = fixture.output_dir.join("base");
    let out = base.join("output");
    fs::create_dir(&base).expect("a base directory");
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).expect("mode");
    fs::create_dir(&out).expect("the output directory");
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700)).expect("mode");
    let moved = fixture.output_dir.join("moved-base");
    let substitute_out = fixture.output_dir.join("base").join("output");
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let hook = |_: &Path| {
        fs::rename(&base, &moved).expect("the ancestor is renamed away");
        let substitute_base = fixture.output_dir.join("base");
        fs::create_dir(&substitute_base).expect("a substitute ancestor exists");
        fs::set_permissions(&substitute_base, fs::Permissions::from_mode(0o700)).expect("mode");
        let substitute_out = substitute_base.join("output");
        fs::create_dir(&substitute_out).expect("a substitute output exists");
        fs::set_permissions(&substitute_out, fs::Permissions::from_mode(0o700)).expect("mode");
    };
    let report = reconcile_with_seams(
        &fixture.inputs_with_output(&out),
        &fake,
        &observe_directory,
        &noop_hook,
        &FileReceiptSink::create,
        &hook,
        &noop_hook,
    );
    assert_eq!(report.outcome, Outcome::Refused("output_dir_replaced"));
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(report.counts, Counts::default());
    assert_eq!(fake.total_calls(), 0);
    assert!(!substitute_out.join(RECEIPT_FILE).exists());
    assert!(!moved.join("output").join(RECEIPT_FILE).exists());
}

#[test]
fn changed_directory_facts_at_the_create_boundary_are_refused() {
    let fixture = fixture(Tamper::None);
    let out = fixture.output_dir.join("out");
    fs::create_dir(&out).expect("a directory");
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700)).expect("mode");
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let hook = |_: &Path| {
        fs::set_permissions(&out, fs::Permissions::from_mode(0o755)).expect("mode");
    };
    let report = reconcile_with_seams(
        &fixture.inputs_with_output(&out),
        &fake,
        &observe_directory,
        &noop_hook,
        &FileReceiptSink::create,
        &hook,
        &noop_hook,
    );
    assert_eq!(report.outcome, Outcome::Refused("output_dir_not_private"));
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(report.counts, Counts::default());
    assert_eq!(fake.total_calls(), 0);
    assert!(!out.join(RECEIPT_FILE).exists());
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700)).expect("restore");
}

/// Restores a fixture directory to mode 700 on every exit path, including
/// unwinding assertions.
struct FixturePermissions {
    path: PathBuf,
}

impl FixturePermissions {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    fn deny(&self) {
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o000))
            .expect("the fixture directory is denied");
    }

    fn allow(&self) {
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o700))
            .expect("the fixture directory is restored");
    }
}

impl Drop for FixturePermissions {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o700));
    }
}

#[test]
fn boundary_edge_metadata_error_is_refused() {
    let fixture = fixture(Tamper::None);
    let base = fixture.output_dir.join("base");
    let out = base.join("output");
    fs::create_dir(&base).expect("a base directory");
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).expect("mode");
    fs::create_dir(&out).expect("the output directory");
    fs::set_permissions(&out, fs::Permissions::from_mode(0o700)).expect("mode");
    let permissions = FixturePermissions::new(&base);
    permissions.deny();
    if fs::read_dir(&out).is_ok() {
        // The current privilege/capability traverses the denied directory, so
        // the intended metadata-observation failure cannot be produced here.
        // `permissions` restores mode 700 on drop; the skip is honest.
        return;
    }
    permissions.allow();
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let hook = |_: &Path| {
        fs::set_permissions(&base, fs::Permissions::from_mode(0o000)).expect("mode");
    };
    let report = reconcile_with_seams(
        &fixture.inputs_with_output(&out),
        &fake,
        &observe_directory,
        &noop_hook,
        &FileReceiptSink::create,
        &hook,
        &noop_hook,
    );
    assert_eq!(
        report.outcome,
        Outcome::Refused("output_dir_metadata_error")
    );
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(report.counts, Counts::default());
    assert_eq!(fake.total_calls(), 0);
    assert!(!out.join(RECEIPT_FILE).exists());
}

#[test]
fn output_directory_replaced_before_receipt_creation_is_refused_without_artifact() {
    let fixture = fixture(Tamper::None);
    let held_parent = private_tempdir();
    let held = held_parent.path().join("held-output");
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let hook = |_: &Path| {
        fs::rename(&fixture.output_dir, &held).expect("the validated directory is renamed");
        fs::create_dir(&fixture.output_dir).expect("a substitute directory exists");
        fs::set_permissions(&fixture.output_dir, fs::Permissions::from_mode(0o700)).expect("mode");
    };
    let report = reconcile_with_seams(
        &fixture.inputs(),
        &fake,
        &observe_directory,
        &noop_hook,
        &FileReceiptSink::create,
        &hook,
        &noop_hook,
    );
    assert_eq!(report.outcome, Outcome::Refused("output_dir_replaced"));
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(report.counts, Counts::default());
    assert_eq!(fake.total_calls(), 0);
    assert!(
        !fixture.receipt_path().exists(),
        "no receipt in the substitute target"
    );
    assert!(
        !held.join(RECEIPT_FILE).exists(),
        "creation never ran after the replacement"
    );
}

#[test]
fn output_directory_replaced_with_a_symlink_before_receipt_creation_is_refused() {
    let fixture = fixture(Tamper::None);
    let substitute = private_tempdir();
    let held_parent = private_tempdir();
    let held = held_parent.path().join("held-output");
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let hook = |_: &Path| {
        fs::rename(&fixture.output_dir, &held).expect("the validated directory is renamed");
        symlink(substitute.path(), &fixture.output_dir).expect("a substitute symlink");
    };
    let report = reconcile_with_seams(
        &fixture.inputs(),
        &fake,
        &observe_directory,
        &noop_hook,
        &FileReceiptSink::create,
        &hook,
        &noop_hook,
    );
    assert_eq!(report.outcome, Outcome::Refused("output_dir_replaced"));
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(fake.total_calls(), 0);
    assert!(!held.join(RECEIPT_FILE).exists());
    assert!(!substitute.path().join(RECEIPT_FILE).exists());
}

#[test]
fn a_held_directory_anchors_leaf_creation_across_path_replacement() {
    let parent = private_tempdir();
    let original = parent.path().join("original");
    fs::create_dir(&original).expect("a directory");
    fs::set_permissions(&original, fs::Permissions::from_mode(0o700)).expect("mode");
    let bound = test_bound_directory(&original).expect("a bound directory");
    let moved = parent.path().join("moved");
    fs::rename(&original, &moved).expect("the held directory is renamed");
    fs::create_dir(&original).expect("a substitute directory exists");
    fs::set_permissions(&original, fs::Permissions::from_mode(0o700)).expect("mode");
    let mut leaf = create_receipt_leaf(&bound).expect("the leaf is created through the held fd");
    leaf.write_all(b"{\"probe\":true}\n")
        .expect("the leaf writes");
    leaf.sync_all().expect("the leaf syncs");
    assert!(
        moved.join(RECEIPT_FILE).exists(),
        "the leaf is anchored to the held directory identity"
    );
    assert!(
        !original.join(RECEIPT_FILE).exists(),
        "no leaf appears in the replacement path"
    );
}

#[test]
fn directory_observation_failure_refuses_before_any_write() {
    let fixture = fixture(Tamper::None);
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let observer: DirObserver = &|_fd: &OwnedFd| -> Result<DirectoryFacts, &'static str> {
        Err("output_dir_metadata_error")
    };
    let report = reconcile_with_seams(
        &fixture.inputs(),
        &fake,
        observer,
        &noop_hook,
        &FileReceiptSink::create,
        &noop_hook,
        &noop_hook,
    );
    assert_eq!(
        report.outcome,
        Outcome::Refused("output_dir_metadata_error")
    );
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(report.counts, Counts::default());
    assert_eq!(fake.total_calls(), 0);
    assert!(!fixture.receipt_path().exists());
}

#[test]
fn directory_identity_mismatch_refuses_before_any_write() {
    let fixture = fixture(Tamper::None);
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let observer: DirObserver = &|fd: &OwnedFd| -> Result<DirectoryFacts, &'static str> {
        let mut facts = observe_directory(fd)?;
        facts.device = facts.device.wrapping_add(1);
        Ok(facts)
    };
    let report = reconcile_with_seams(
        &fixture.inputs(),
        &fake,
        observer,
        &noop_hook,
        &FileReceiptSink::create,
        &noop_hook,
        &noop_hook,
    );
    assert_eq!(report.outcome, Outcome::Refused("output_dir_replaced"));
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(fake.total_calls(), 0);
    assert!(!fixture.receipt_path().exists());
}

#[test]
fn directory_owner_mismatch_refuses_before_any_write() {
    let fixture = fixture(Tamper::None);
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let observer: DirObserver = &|fd: &OwnedFd| -> Result<DirectoryFacts, &'static str> {
        let mut facts = observe_directory(fd)?;
        facts.owner_uid = facts.owner_uid.wrapping_add(1);
        Ok(facts)
    };
    let report = reconcile_with_seams(
        &fixture.inputs(),
        &fake,
        observer,
        &noop_hook,
        &FileReceiptSink::create,
        &noop_hook,
        &noop_hook,
    );
    assert_eq!(report.outcome, Outcome::Refused("output_dir_not_owned"));
    assert_eq!(report.receipt_state, ReceiptState::NotCreated);
    assert_eq!(fake.total_calls(), 0);
    assert!(!fixture.receipt_path().exists());
}

#[test]
fn prepared_write_failure_preserves_the_created_receipt_and_refuses() {
    let fixture = fixture(Tamper::None);
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let factory = |bound: &BoundDirectory| -> Result<Box<dyn ReceiptSink>, &'static str> {
        Ok(Box::new(FailPreparedSink {
            inner: FileReceiptSink::create(bound)?,
        }))
    };
    let report = reconcile_with_seams(
        &fixture.inputs(),
        &fake,
        &observe_directory,
        &noop_hook,
        &factory,
        &noop_hook,
        &noop_hook,
    );
    assert_eq!(report.outcome, Outcome::Refused("receipt_prepared_failed"));
    assert_eq!(report.receipt_state, ReceiptState::PreparedFailed);
    assert_eq!(report.counts, Counts::default());
    let created = report
        .receipt_path
        .as_ref()
        .expect("the created artifact path is reported");
    assert!(created.ends_with(RECEIPT_FILE));
    assert!(
        created.exists(),
        "the created artifact is preserved, never unlinked"
    );
    assert_eq!(
        fake.total_calls(),
        0,
        "no backend call before a durable PREPARED"
    );
    assert_eq!(terminal_verdict(&report), Err("receipt_prepared_failed"));
}

#[test]
fn finalization_failure_preserves_prepared_and_reports_final_failed() {
    let fixture = fixture(Tamper::None);
    let fake = FakeKeychain::new(vec![ScriptedPresence::Absent], Ok(()));
    let factory = |bound: &BoundDirectory| -> Result<Box<dyn ReceiptSink>, &'static str> {
        Ok(Box::new(FailFinalSink {
            inner: FileReceiptSink::create(bound)?,
        }))
    };
    let report = reconcile_with_seams(
        &fixture.inputs(),
        &fake,
        &observe_directory,
        &noop_hook,
        &factory,
        &noop_hook,
        &noop_hook,
    );
    assert_eq!(report.outcome, Outcome::AlreadyAbsent);
    assert_eq!(report.receipt_state, ReceiptState::FinalFailed);
    assert_eq!(
        report.counts,
        Counts {
            observations: 1,
            deletes: 0
        }
    );
    assert_eq!(fake.call_count("secret"), 0, "no value read");
    assert_eq!(fake.call_count("presence"), 1);
    assert_eq!(terminal_verdict(&report), Err("final_persistence_failed"));
    let records = read_records(&fixture.receipt_path());
    assert_eq!(records.len(), 1, "only the prepared record is durable");
    assert_prepared_record(&records[0], &fixture);
}

#[test]
fn a_replaced_output_directory_reports_unknown_path_without_deleting() {
    let fixture = fixture(Tamper::None);
    let moved_parent = private_tempdir();
    let moved = moved_parent.path().join("moved-output");
    let fake = FakeKeychain::new(vec![ScriptedPresence::Absent], Ok(()));
    let hook = |_: &Path| {
        fs::rename(&fixture.output_dir, &moved).expect("the output directory is renamed");
    };
    let report = reconcile_with_seams(
        &fixture.inputs(),
        &fake,
        &observe_directory,
        &noop_hook,
        &FileReceiptSink::create,
        &noop_hook,
        &hook,
    );
    assert_eq!(report.outcome, Outcome::AlreadyAbsent);
    assert_eq!(report.receipt_state, ReceiptState::PathUnknown);
    assert_eq!(terminal_verdict(&report), Err("receipt_path_unknown"));
    assert!(!fixture.output_dir.exists(), "the original path is gone");
    let moved_receipt = moved.join(RECEIPT_FILE);
    assert!(
        moved_receipt.exists(),
        "the FINAL record lands in the held inode and is not deleted"
    );
    assert_records(
        &moved_receipt,
        &fixture,
        &ExpectedFinal {
            outcome: "already_absent",
            reason: None,
            observations: 1,
            deletes: 0,
            pre_observation: "not_found",
            delete_attempted: false,
            delete_outcome: "not_attempted",
            post_observation: "not_attempted",
            terminal_state: "scoped_entry_absent",
        },
    );
}

#[test]
fn owner_metadata_error_refuses_and_preserves_prepared() {
    let fixture = fixture(Tamper::None);
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let factory = |bound: &BoundDirectory| -> Result<Box<dyn ReceiptSink>, &'static str> {
        Ok(Box::new(FailOwnerSink {
            inner: FileReceiptSink::create(bound)?,
        }))
    };
    let report = reconcile_with_seams(
        &fixture.inputs(),
        &fake,
        &observe_directory,
        &noop_hook,
        &factory,
        &noop_hook,
        &noop_hook,
    );
    assert_eq!(
        report.outcome,
        Outcome::Refused("receipt_owner_metadata_error")
    );
    assert_eq!(report.receipt_state, ReceiptState::Finalized);
    assert_eq!(report.counts, Counts::default());
    assert_eq!(fake.total_calls(), 0);
    assert_eq!(
        terminal_verdict(&report),
        Err("receipt_owner_metadata_error")
    );
    let records = read_records(&fixture.receipt_path());
    assert_eq!(
        records.len(),
        2,
        "prepared and refusing final are preserved"
    );
    assert_prepared_record(&records[0], &fixture);
    assert_eq!(records[1]["outcome"], "refused");
    assert_eq!(
        records[1]["reason"].as_str(),
        Some("receipt_owner_metadata_error")
    );
}

#[test]
fn owner_mismatch_refuses_and_preserves_prepared() {
    let fixture = fixture(Tamper::None);
    let directory_uid = fs::metadata(&fixture.output_dir)
        .expect("the output directory metadata")
        .uid();
    let other_uid = if directory_uid == u32::MAX {
        0
    } else {
        directory_uid + 1
    };
    let fake = FakeKeychain::new(Vec::new(), Ok(()));
    let factory = move |bound: &BoundDirectory| -> Result<Box<dyn ReceiptSink>, &'static str> {
        Ok(Box::new(MismatchedOwnerSink {
            inner: FileReceiptSink::create(bound)?,
            uid: other_uid,
        }))
    };
    let report = reconcile_with_seams(
        &fixture.inputs(),
        &fake,
        &observe_directory,
        &noop_hook,
        &factory,
        &noop_hook,
        &noop_hook,
    );
    assert_eq!(
        report.outcome,
        Outcome::Refused("output_dir_owner_mismatch")
    );
    assert_eq!(report.receipt_state, ReceiptState::Finalized);
    assert_eq!(report.counts, Counts::default());
    assert_eq!(fake.total_calls(), 0);
    let records = read_records(&fixture.receipt_path());
    assert_eq!(
        records.len(),
        2,
        "prepared and refusing final are preserved"
    );
    assert_prepared_record(&records[0], &fixture);
    assert_eq!(records[1]["outcome"], "refused");
    assert_eq!(
        records[1]["reason"].as_str(),
        Some("output_dir_owner_mismatch")
    );
}

#[test]
fn lost_receipt_identity_cannot_finalize() {
    let fixture = fixture(Tamper::None);
    let fake = FakeKeychain::new(vec![ScriptedPresence::Absent], Ok(()));
    let factory = |bound: &BoundDirectory| -> Result<Box<dyn ReceiptSink>, &'static str> {
        Ok(Box::new(LostIdentitySink {
            inner: FileReceiptSink::create(bound)?,
        }))
    };
    let report = reconcile_with_seams(
        &fixture.inputs(),
        &fake,
        &observe_directory,
        &noop_hook,
        &factory,
        &noop_hook,
        &noop_hook,
    );
    assert_eq!(report.outcome, Outcome::AlreadyAbsent);
    assert_eq!(report.receipt_state, ReceiptState::IdentityUnknown);
    assert_eq!(terminal_verdict(&report), Err("receipt_identity_unknown"));
    let records = read_records(&fixture.receipt_path());
    assert_eq!(records.len(), 2, "both records are still durable");
    assert_prepared_record(&records[0], &fixture);
}

#[test]
fn prepared_and_final_keep_the_same_fd_identity() {
    let dir = private_tempdir();
    let bound = test_bound_directory(dir.path()).expect("a bound directory");
    let mut sink = FileReceiptSink::create(&bound).expect("the receipt sink is created");
    let before = sink.file_identity().expect("the identity is observable");
    sink.append_prepared("{\"record\":\"prepared\"}")
        .expect("prepared appends");
    sink.append_final("{\"record\":\"final\"}")
        .expect("final appends");
    let after = sink.file_identity().expect("the identity is observable");
    assert_eq!(before, after, "one held descriptor for both records");
    let metadata = fs::metadata(bound.canonical_path.join(RECEIPT_FILE))
        .expect("the receipt metadata is readable");
    assert_eq!((metadata.dev(), metadata.ino()), before);
}

#[test]
fn the_real_receipt_sink_reports_its_owner_uid_and_identity() {
    let dir = private_tempdir();
    let bound = test_bound_directory(dir.path()).expect("a bound directory");
    let sink = FileReceiptSink::create(&bound).expect("the receipt sink is created");
    let uid = sink.owner_uid().expect("owner metadata is readable");
    let directory_uid = fs::metadata(dir.path())
        .expect("the directory metadata")
        .uid();
    assert_eq!(uid, directory_uid);
    let identity = sink.file_identity().expect("identity is observable");
    let metadata = fs::metadata(dir.path().join(RECEIPT_FILE)).expect("the receipt metadata");
    assert_eq!((metadata.dev(), metadata.ino()), identity);
}

#[test]
fn terminal_classification_rejects_unsuccessful_outcomes() {
    let counts = Counts {
        observations: 1,
        deletes: 0,
    };
    assert!(
        terminal_verdict(&synthetic_report(
            Outcome::Deleted,
            counts,
            ReceiptState::Finalized
        ))
        .is_ok()
    );
    assert!(
        terminal_verdict(&synthetic_report(
            Outcome::AlreadyAbsent,
            counts,
            ReceiptState::Finalized
        ))
        .is_ok()
    );
    assert_eq!(
        terminal_verdict(&synthetic_report(
            Outcome::Uncertain("delete_failed"),
            counts,
            ReceiptState::Finalized
        )),
        Err("delete_failed")
    );
    assert_eq!(
        terminal_verdict(&synthetic_report(
            Outcome::Refused("receipt_exists"),
            counts,
            ReceiptState::Finalized
        )),
        Err("receipt_exists")
    );
    assert_eq!(
        terminal_verdict(&synthetic_report(
            Outcome::Deleted,
            counts,
            ReceiptState::FinalFailed
        )),
        Err("final_persistence_failed")
    );
    assert_eq!(
        terminal_verdict(&synthetic_report(
            Outcome::Deleted,
            counts,
            ReceiptState::PathUnknown
        )),
        Err("receipt_path_unknown")
    );
    assert_eq!(
        terminal_verdict(&synthetic_report(
            Outcome::Deleted,
            counts,
            ReceiptState::IdentityUnknown
        )),
        Err("receipt_identity_unknown")
    );
    assert_eq!(
        terminal_verdict(&synthetic_report(
            Outcome::AlreadyAbsent,
            counts,
            ReceiptState::NotCreated
        )),
        Err("receipt_not_finalized")
    );
}

#[test]
fn a_written_receipt_is_private_bounded_and_secret_free() {
    let fixture = fixture(Tamper::None);
    let fake = FakeKeychain::new(
        vec![ScriptedPresence::Present, ScriptedPresence::Absent],
        Ok(()),
    )
    .with_reads(vec![ScriptedRead::Value(FAKE_CANARY_PRE)]);
    let report = reconcile_retained_entry(&fixture.inputs(), &fake);
    assert_eq!(report.outcome, Outcome::Deleted);
    assert_eq!(report.receipt_state, ReceiptState::Finalized);
    assert_eq!(fake.call_count("secret"), 0, "no value read");
    let path = fixture.receipt_path();
    let mode = fs::metadata(&path)
        .expect("receipt metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "the receipt is private");
    let body = fs::read_to_string(&path).expect("the receipt reads back");
    assert!(body.len() <= MAX_RECEIPT_BYTES);
    assert!(body.contains(&fixture.service));
    assert!(body.contains(&fixture.account));
    assert!(body.contains("deleted"));
    assert!(!body.contains(FAKE_CANARY_PRE));
    assert!(!directory_contains(&fixture.output_dir, FAKE_CANARY_PRE));
    // A second run must refuse the existing receipt and never call the backend.
    let second = FakeKeychain::new(Vec::new(), Ok(()));
    let refused = reconcile_retained_entry(&fixture.inputs(), &second);
    assert_eq!(refused.outcome, Outcome::Refused("receipt_exists"));
    assert_eq!(refused.receipt_state, ReceiptState::NotCreated);
    assert_eq!(second.total_calls(), 0);
    assert_eq!(fs::read_to_string(&path).expect("unchanged"), body);
}
