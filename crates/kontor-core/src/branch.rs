//! Deterministic publication branches and the managed worktrees that carry them.
//!
//! The ASMA CLI derives every working branch from a confirmed tracker issue:
//! `asma git checkout -b --from ASMA-3432` yields `<type>/ASMA-3432-<slug>`.
//! Kontor prepares Git worktrees for the same repositories, and before this
//! module it turned whatever path a caller declared under `.worktrees/` into a
//! branch name — `cat-11`, `kon-op-22`, `consultation-<uuid>` — with no tracker
//! identity at all. This module is the one grammar both producers share:
//!
//! ```text
//! <type>/<PROJECT>-<number>-<slug>      ordinary work, e.g. feat/ASMA-8101-publication-identity
//! releases/v<major>.<minor>.<patch>     hotpatch release lines, the only keyless form
//! ```
//!
//! The grammar is generic over the tracker project. *Which* key a branch must
//! carry is decided by comparing it against the confirmed binding Kontor holds
//! for the epic or task ([`BranchName::ensure_bound_to`]); nothing here assumes
//! one deployment's project key, and a key is never inferred from a display code.
//!
//! Every refusal is a stable [`BranchRefusal`] code, so runtime adapters, the
//! API and the CLI report the same reason without re-deriving the rule, and no
//! refusal echoes the branch text that failed.

use std::fmt;

use crate::DomainError;
use crate::id::ExternalId;

/// The directory below a project root that holds Kontor-managed task checkouts.
pub const MANAGED_WORKTREES_DIR: &str = ".worktrees";

/// The most characters a derived slug keeps. Mirrors the ASMA CLI.
pub const SLUG_MAX_LEN: usize = 100;

/// The slug used when a title has no letters or digits at all. Mirrors the ASMA CLI.
pub const SLUG_FALLBACK: &str = "work-item";

closed_enum! {
    /// The conventional-commit type a publication branch is filed under.
    ///
    /// `releases` doubles as the prefix of the keyless hotpatch form
    /// `releases/v<major>.<minor>.<patch>`.
    BranchType, "BranchType" {
        /// New behaviour.
        Feat => "feat",
        /// A defect fix.
        Fix => "fix",
        /// Documentation only.
        Docs => "docs",
        /// Maintenance that changes no behaviour.
        Chore => "chore",
        /// Restructuring that changes no behaviour.
        Refactor => "refactor",
        /// Tests only.
        Test => "test",
        /// Performance work.
        Perf => "perf",
        /// Build system or dependencies.
        Build => "build",
        /// Continuous-integration configuration.
        Ci => "ci",
        /// A release or hotpatch line.
        Releases => "releases",
    }
}

closed_enum! {
    /// Why a branch was refused. The spelling is the stable reason code the ASMA
    /// CLI reports for the same condition.
    BranchRefusal, "BranchRefusal" {
        /// No `<type>/` prefix at all, e.g. `cat-11`.
        ShapeInvalid => "branch_shape_invalid",
        /// A prefix outside the closed type vocabulary, e.g. `land/…`.
        TypeUnknown => "branch_type_unknown",
        /// Nothing after the type reads as a tracker key, e.g. `fix/restore-rule`.
        KeyMissing => "branch_key_missing",
        /// A key-like segment that is lowercase, zero-padded or otherwise not
        /// canonical, e.g. `docs/asma-8050-closeout` or `fix/KON-OP-22-x`.
        KeyNotCanonical => "branch_key_not_canonical",
        /// The slug after the key is empty or not lowercase kebab-case.
        SlugInvalid => "branch_slug_invalid",
        /// The branch carries a key the confirmed binding does not.
        BindingMismatch => "branch_binding_mismatch",
        /// No confirmed tracker key exists to derive or verify a branch against.
        BindingUnconfirmed => "binding_unconfirmed",
    }
}

impl BranchRefusal {
    /// The rule that was violated, prefixed with the stable code and never
    /// echoing the branch that failed.
    #[must_use]
    pub const fn rule(self) -> &'static str {
        match self {
            Self::ShapeInvalid => {
                "branch_shape_invalid: a publication branch is `<type>/<PROJECT>-<number>-<slug>` or `releases/v<major>.<minor>.<patch>`"
            }
            Self::TypeUnknown => {
                "branch_type_unknown: the branch type must be one of feat, fix, docs, chore, refactor, test, perf, build, ci or releases"
            }
            Self::KeyMissing => {
                "branch_key_missing: the branch carries no tracker key after its type"
            }
            Self::KeyNotCanonical => {
                "branch_key_not_canonical: the tracker key must be an uppercase project key, a hyphen and a positive number without leading zeroes"
            }
            Self::SlugInvalid => {
                "branch_slug_invalid: the slug after the tracker key must be non-empty lowercase words joined by single hyphens"
            }
            Self::BindingMismatch => {
                "branch_binding_mismatch: the branch key is not the confirmed tracker key of the epic or task it serves"
            }
            Self::BindingUnconfirmed => {
                "binding_unconfirmed: no confirmed tracker key exists to derive or verify a branch for this work"
            }
        }
    }
}

impl From<BranchRefusal> for DomainError {
    fn from(refusal: BranchRefusal) -> Self {
        Self::invalid("BranchName", refusal.rule())
    }
}

/// A canonical tracker issue key: `<PROJECT>-<number>`.
///
/// The project key is one uppercase ASCII letter followed by uppercase letters
/// or digits; the number is a positive decimal without leading zeroes. Case is
/// never repaired: `asma-8050` is a refusal, not a spelling of `ASMA-8050`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackerKey(String);

impl TrackerKey {
    /// Parse an exact canonical key.
    ///
    /// # Errors
    /// [`BranchRefusal::KeyMissing`] when the text does not start like a key,
    /// [`BranchRefusal::KeyNotCanonical`] when it does but is not exactly one.
    pub fn parse(text: &str) -> Result<Self, BranchRefusal> {
        let key_len = scan_key(text)?;
        if key_len != text.len() {
            return Err(BranchRefusal::KeyNotCanonical);
        }
        Ok(Self(text.to_owned()))
    }

    /// Read a confirmed external issue key as a tracker key.
    ///
    /// # Errors
    /// The same refusals as [`Self::parse`]; an opaque external id that is not a
    /// canonical key (an internal UUID stood in as a key, say) is refused.
    pub fn from_external(key: &ExternalId) -> Result<Self, BranchRefusal> {
        Self::parse(key.as_str())
    }

    /// The exact key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TrackerKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Derive the slug the ASMA CLI would derive from a title.
///
/// Lowercase; every run of anything other than an ASCII letter or digit becomes
/// one hyphen; leading and trailing hyphens are dropped; the result is cut at
/// [`SLUG_MAX_LEN`] characters without leaving a dangling hyphen; a title with
/// no letters or digits at all becomes [`SLUG_FALLBACK`].
#[must_use]
pub fn slugify(title: &str) -> String {
    let mut slug = String::with_capacity(title.len());
    let mut pending_hyphen = false;
    for character in title.to_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            if pending_hyphen && !slug.is_empty() {
                slug.push('-');
            }
            pending_hyphen = false;
            slug.push(character);
        } else {
            pending_hyphen = true;
        }
    }
    if slug.len() > SLUG_MAX_LEN {
        slug.truncate(SLUG_MAX_LEN);
    }
    let trimmed = slug.trim_end_matches('-');
    if trimmed.is_empty() {
        SLUG_FALLBACK.to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// A publication branch that satisfies the shared grammar.
///
/// Construct one by [`BranchName::derive`] from a confirmed key, or by
/// [`BranchName::parse`] from text somebody else chose. Either way the value
/// cannot exist unless it is canonical.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BranchName {
    text: String,
    branch_type: BranchType,
    key: Option<TrackerKey>,
}

impl BranchName {
    /// The branch the ASMA CLI would create for this key and title.
    #[must_use]
    pub fn derive(branch_type: BranchType, key: &TrackerKey, title: &str) -> Self {
        let slug = slugify(title);
        Self {
            text: format!("{branch_type}/{key}-{slug}"),
            branch_type,
            key: Some(key.clone()),
        }
    }

    /// Parse a branch somebody declared.
    ///
    /// # Errors
    /// A [`BranchRefusal`] naming the first rule the text breaks.
    pub fn parse(text: &str) -> Result<Self, BranchRefusal> {
        let (prefix, rest) = text.split_once('/').ok_or(BranchRefusal::ShapeInvalid)?;
        let branch_type = BranchType::parse(prefix).map_err(|_| BranchRefusal::TypeUnknown)?;
        if branch_type == BranchType::Releases && is_release_version(rest) {
            return Ok(Self {
                text: text.to_owned(),
                branch_type,
                key: None,
            });
        }
        let key_len = scan_key(rest)?;
        let slug = rest[key_len..]
            .strip_prefix('-')
            .ok_or(BranchRefusal::SlugInvalid)?;
        if !is_canonical_slug(slug) {
            return Err(BranchRefusal::SlugInvalid);
        }
        Ok(Self {
            text: text.to_owned(),
            branch_type,
            key: Some(TrackerKey(rest[..key_len].to_owned())),
        })
    }

    /// The exact branch text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The type prefix.
    #[must_use]
    pub const fn branch_type(&self) -> BranchType {
        self.branch_type
    }

    /// The tracker key, absent only for the keyless `releases/v…` form.
    #[must_use]
    pub const fn key(&self) -> Option<&TrackerKey> {
        self.key.as_ref()
    }

    /// Prove this branch carries one of the keys the confirmed binding holds.
    ///
    /// The epic key and, where one exists, the task key are both acceptable:
    /// a mini-project branches from its epic by default and from a task only
    /// under an explicit per-task decision, and both are confirmed identities.
    ///
    /// # Errors
    /// [`BranchRefusal::BindingUnconfirmed`] when `confirmed` is empty, and
    /// [`BranchRefusal::BindingMismatch`] when the branch is keyless or carries
    /// a key that is not in `confirmed`.
    pub fn ensure_bound_to<'a>(
        &self,
        confirmed: impl IntoIterator<Item = &'a TrackerKey>,
    ) -> Result<(), BranchRefusal> {
        let mut any = false;
        for key in confirmed {
            any = true;
            if self.key.as_ref() == Some(key) {
                return Ok(());
            }
        }
        if any {
            Err(BranchRefusal::BindingMismatch)
        } else {
            Err(BranchRefusal::BindingUnconfirmed)
        }
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// The canonical checkout of `branch` below `project_root`:
/// `<project_root>/.worktrees/<branch>`.
#[must_use]
pub fn managed_worktree_path(project_root: &str, branch: &BranchName) -> String {
    format!(
        "{}/{MANAGED_WORKTREES_DIR}/{branch}",
        project_root.trim_end_matches('/')
    )
}

/// The branch a managed worktree path encodes, or `None` when the path is not
/// below `<project_root>/.worktrees/` and therefore implies no branch at all.
#[must_use]
pub fn managed_branch_text<'a>(project_root: &str, worktree: &'a str) -> Option<&'a str> {
    let encoded = worktree
        .strip_prefix(project_root.trim_end_matches('/'))?
        .strip_prefix('/')?
        .strip_prefix(MANAGED_WORKTREES_DIR)?
        .strip_prefix('/')?
        .trim_end_matches('/');
    (!encoded.is_empty()).then_some(encoded)
}

/// The ASMA CLI catalog-worktree slug and module directory encoded by a
/// managed two-segment path, or `None` when the path has another shape.
///
/// `asma worktree add ASMA-8114 --mod _tools/asma-rs-kontor` deliberately
/// creates `<project>/.worktrees/asma-8114/asma-rs-kontor`. Unlike a
/// Kontor-created checkout, that path does not encode its Git branch. Callers
/// must first try [`BranchName::parse`] over [`managed_branch_text`], then use
/// this shape only when the slug is bound to the task's confirmed tracker key
/// and the runtime proves the checkout's actual branch.
#[must_use]
pub fn managed_catalog_worktree_parts<'a>(
    project_root: &str,
    worktree: &'a str,
) -> Option<(&'a str, &'a str)> {
    let encoded = managed_branch_text(project_root, worktree)?;
    let (slug, module) = encoded.split_once('/')?;
    if slug.is_empty() || module.is_empty() || module.contains('/') {
        return None;
    }
    Some((slug, module))
}

/// The length of the canonical key `text` starts with.
///
/// `KeyMissing` when the text does not even begin like a key; `KeyNotCanonical`
/// when it begins like one (including a lowercase spelling) but is not one.
fn scan_key(text: &str) -> Result<usize, BranchRefusal> {
    let bytes = text.as_bytes();
    let Some(first) = bytes.first() else {
        return Err(BranchRefusal::KeyMissing);
    };
    if !first.is_ascii_uppercase() {
        return Err(if looks_like_a_lowercase_key(text) {
            BranchRefusal::KeyNotCanonical
        } else {
            BranchRefusal::KeyMissing
        });
    }
    let project_len = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        .count();
    let number = text[project_len..]
        .strip_prefix('-')
        .ok_or(BranchRefusal::KeyNotCanonical)?;
    let number_len = number.bytes().take_while(u8::is_ascii_digit).count();
    if number_len == 0 || number.starts_with('0') {
        return Err(BranchRefusal::KeyNotCanonical);
    }
    Ok(project_len + 1 + number_len)
}

/// Whether `text` would scan as a canonical key if its project were uppercase —
/// the `asma-8050` spelling that must be reported as non-canonical, not as absent.
fn looks_like_a_lowercase_key(text: &str) -> bool {
    let bytes = text.as_bytes();
    if !bytes.first().is_some_and(u8::is_ascii_lowercase) {
        return false;
    }
    let project_len = bytes
        .iter()
        .take_while(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        .count();
    let Some(number) = text[project_len..].strip_prefix('-') else {
        return false;
    };
    let number_len = number.bytes().take_while(u8::is_ascii_digit).count();
    number_len > 0
        && !number.starts_with('0')
        && matches!(number.as_bytes().get(number_len), None | Some(b'-'))
}

fn is_canonical_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

/// `v<major>.<minor>.<patch>` with decimal components.
fn is_release_version(text: &str) -> bool {
    let Some(version) = text.strip_prefix('v') else {
        return false;
    };
    let mut parts = version.split('.');
    let numeric = |part: Option<&str>| {
        part.is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    };
    numeric(parts.next())
        && numeric(parts.next())
        && numeric(parts.next())
        && parts.next().is_none()
}
