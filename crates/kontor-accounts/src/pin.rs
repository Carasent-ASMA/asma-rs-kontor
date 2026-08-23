//! The governed account pin: which provider aliases select which account.
//!
//! # The blocker this exists to lift
//!
//! Account-before-rung resolution is only worth building if Kontor can actually
//! address one coding account rather than another. It could not. The two Codex
//! logins live as separate `CODEX_HOME` directories, but the surface Kontor
//! drives — `paseo agent run` — takes `--workspace --cwd --provider --model
//! --mode --thinking --title --label` and nothing else, and Paseo's own
//! `create_agent` exposes no account parameter. So "try the work account, then
//! the personal one" was unreachable, and a realm's single account profile
//! served every provider.
//!
//! The lever was already in that flag list. If a deployment registers **one
//! Paseo provider alias per coding account** — `codex-work` for one login,
//! `codex-personal` for the other — then `--provider` *is* the account
//! selector, and the pin is attested by the readback Paseo already performs:
//! the adapter compares the provider the agent reports against the provider
//! that was requested and fails correlation if they differ.
//!
//! The `<built-in>-<account>` alias shape is not invented here. The Paseo client
//! already normalizes it: `built_in_provider` strips the account suffix so a
//! permission mode, a fallback route and every other family-level rule keep
//! resolving against `codex`, while the full alias stays the thing that selects
//! and attests the account.
//!
//! # Why it is declared and not inferred
//!
//! Kontor cannot tell, by looking, whether two provider aliases are two accounts
//! or two spellings of one. Guessing wrong in the permissive direction is the
//! expensive mistake: it would report a per-run account guarantee the runtime
//! does not make, and every launch receipt written under it would be attesting
//! to something unverified. So the mapping is deployment configuration, the
//! runtime declares `account_env` only when it has it, and an account naming no
//! provider is simply not walked for one.

use std::collections::BTreeSet;

use kontor_core::repository::AccountProfile;
use kontor_core::{DomainError, DomainResult};
use kontor_scheduler::headroom::EligibleAccount;

/// The key the routing document declares selectable provider aliases under.
///
/// It lives in `routing` — the profile's existing non-secret routing metadata —
/// rather than in a new column, because that is precisely what that document is
/// for and it is already immutable for the life of the profile. A pin that could
/// be edited under a running seat would not be a pin.
pub const SELECTABLE_PROVIDERS_KEY: &str = "selectable_providers";

/// The provider aliases this account may be launched under.
///
/// An empty set is a valid, meaningful answer: this deployment cannot address
/// this account per-provider, so no rung walk will select it. That is the same
/// refusal `account_env: false` makes at dispatch, taken early enough that
/// nothing is queued which dispatch would then have to throw away.
///
/// # Errors
/// Returns [`DomainError`] when the key is present but is not an array of
/// non-empty strings. A malformed pin is refused rather than read as absent: the
/// two answers differ by exactly whether this account is routable, and silently
/// choosing the safer one hides a typo that an operator needs to see.
pub fn selectable_providers(profile: &AccountProfile) -> DomainResult<BTreeSet<String>> {
    let document: serde_json::Value = profile.routing.deserialize()?;
    let Some(declared) = document.get(SELECTABLE_PROVIDERS_KEY) else {
        return Ok(BTreeSet::new());
    };
    let entries = declared.as_array().ok_or_else(|| {
        DomainError::invalid(
            "AccountProfile.routing",
            "selectable_providers must be an array of provider aliases",
        )
    })?;
    entries
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .map(str::trim)
                .filter(|alias| !alias.is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    DomainError::invalid(
                        "AccountProfile.routing",
                        "every selectable provider alias must be a non-empty string",
                    )
                })
        })
        .collect()
}

/// Validate a caller-declared pin before it is frozen into a routing document.
///
/// The constraints are exactly the ones [`selectable_providers`] enforces on
/// the stored document, applied at the door instead: an alias must be a
/// non-empty string once trimmed. Returning the normalized set — trimmed,
/// deduplicated, ordered — is what lets the caller freeze one canonical
/// spelling of the pin rather than whichever whitespace the request carried.
///
/// # Errors
/// Returns [`DomainError`] when any declared alias is empty after trimming.
pub fn declared_selectable_providers(aliases: &[String]) -> DomainResult<BTreeSet<String>> {
    aliases
        .iter()
        .map(|alias| {
            let alias = alias.trim();
            if alias.is_empty() {
                return Err(DomainError::invalid(
                    "AccountProfile.routing",
                    "every selectable provider alias must be a non-empty string",
                ));
            }
            Ok(alias.to_owned())
        })
        .collect()
}

/// The providers a *pinned* run may still be walked under.
///
/// A pin used to be considered under every rung provider, because a pinned run
/// is not moving between accounts and a family rung carries no account of its
/// own. Alias rungs change what that latitude means: admitting a declared
/// account onto another account's alias would launch one account while
/// claiming the other, and the readback — which verifies the provider alone —
/// could not tell. So a pin that declares aliases is walked only under them,
/// while an undeclared pin keeps the full latitude it always had.
#[must_use]
pub fn pinned_selectable_providers(
    pin: kontor_core::id::AccountProfileId,
    accounts: &[EligibleAccount],
    every_rung_provider: BTreeSet<String>,
) -> BTreeSet<String> {
    accounts
        .iter()
        .find(|account| account.account_profile_id == pin)
        .filter(|account| !account.selectable_providers.is_empty())
        .map(|account| account.selectable_providers.clone())
        .unwrap_or(every_rung_provider)
}

/// The accounts a launch may be resolved across, in the shape the walk takes.
///
/// Disabled profiles are dropped here rather than filtered later. `enabled:
/// false` is the operator's permanent removal of an account, and an account that
/// cannot be launched is not a candidate whose quota is worth consulting.
///
/// # Errors
/// Returns [`DomainError`] for any profile whose routing document declares a
/// malformed pin, and when two enabled profiles declare the same alias. One
/// alias naming two accounts is refused rather than resolved by ordering: the
/// alias is the only thing that selects and attests the account at launch, so
/// an ambiguous one would let a receipt claim an account the launch may not
/// have used.
pub fn eligible_accounts(profiles: &[AccountProfile]) -> DomainResult<Vec<EligibleAccount>> {
    let mut claimed: BTreeSet<String> = BTreeSet::new();
    profiles
        .iter()
        .filter(|profile| profile.enabled)
        .map(|profile| {
            let selectable = selectable_providers(profile)?;
            for alias in &selectable {
                if !claimed.insert(alias.clone()) {
                    return Err(DomainError::invalid(
                        "AccountProfile.routing",
                        "two enabled account profiles declare the same provider alias",
                    ));
                }
            }
            Ok(EligibleAccount {
                account_profile_id: profile.id,
                selectable_providers: selectable,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use kontor_core::id::{
        AccountProfileId, AggregateRevision, CanonicalDocument, CredentialAlias, ExternalName,
        ProjectId, RuntimeKindKey, Timestamp,
    };
    use kontor_core::repository::{CredentialReference, CredentialReferenceKind};

    use super::*;

    /// Every canonical document declares its schema generation, so a fixture
    /// that omitted it would be refused before it reached the parser.
    fn document(mut body: serde_json::Value) -> CanonicalDocument {
        body.as_object_mut()
            .expect("a JSON object")
            .insert("schema_version".to_owned(), serde_json::json!(1));
        CanonicalDocument::from_serializable(&body).expect("a canonical document")
    }

    fn profile(routing: serde_json::Value, enabled: bool) -> AccountProfile {
        AccountProfile {
            id: AccountProfileId::generate(),
            project_id: ProjectId::parse("01890000-0000-7000-8000-00000000f001")
                .expect("a valid project id"),
            label: ExternalName::parse("Igor · Local Paseo").expect("a valid label"),
            external_account_id: None,
            harness: RuntimeKindKey::parse("paseo.agent").expect("a valid runtime kind"),
            credential_ref: CredentialReference {
                kind: CredentialReferenceKind::Keychain,
                alias: CredentialAlias::parse("approved-alias").expect("an approved alias"),
            },
            environment: document(serde_json::json!({})),
            routing: document(routing),
            capability: document(serde_json::json!({})),
            provider_identity: None,
            enabled,
            revision: AggregateRevision::INITIAL,
            created_at: Timestamp::from_second(1).expect("an instant"),
            updated_at: Timestamp::from_second(1).expect("an instant"),
        }
    }

    #[test]
    fn one_alias_per_login_is_what_makes_two_codex_accounts_addressable() {
        let profile = profile(
            serde_json::json!({ "selectable_providers": ["codex-work", "codex-personal"] }),
            true,
        );
        assert_eq!(
            selectable_providers(&profile).expect("a well-formed pin"),
            ["codex-personal".to_owned(), "codex-work".to_owned()]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn a_profile_declaring_nothing_is_simply_not_routable_per_provider() {
        let profile = profile(serde_json::json!({}), true);
        assert!(
            selectable_providers(&profile)
                .expect("absence is not an error")
                .is_empty()
        );
    }

    #[test]
    fn a_malformed_pin_is_refused_rather_than_read_as_absent() {
        for routing in [
            serde_json::json!({ "selectable_providers": "codex" }),
            serde_json::json!({ "selectable_providers": ["codex", ""] }),
            serde_json::json!({ "selectable_providers": ["codex", 7] }),
            serde_json::json!({ "selectable_providers": ["codex", "   "] }),
        ] {
            assert!(
                selectable_providers(&profile(routing.clone(), true)).is_err(),
                "{routing} must not be read as an empty pin"
            );
        }
    }

    #[test]
    fn one_alias_naming_two_enabled_accounts_is_refused_not_ordered_away() {
        let profiles = [
            profile(
                serde_json::json!({ "selectable_providers": ["codex-work"] }),
                true,
            ),
            profile(
                serde_json::json!({ "selectable_providers": ["codex-work"] }),
                true,
            ),
        ];
        assert!(
            eligible_accounts(&profiles).is_err(),
            "an alias that could mean either account must not resolve to one of them"
        );
    }

    #[test]
    fn a_disabled_profile_may_share_an_alias_because_it_is_never_walked() {
        let profiles = [
            profile(
                serde_json::json!({ "selectable_providers": ["codex-work"] }),
                false,
            ),
            profile(
                serde_json::json!({ "selectable_providers": ["codex-work"] }),
                true,
            ),
        ];
        let eligible = eligible_accounts(&profiles).expect("only one enabled claimant");
        assert_eq!(eligible.len(), 1);
    }

    #[test]
    fn a_declared_pin_is_normalized_and_a_blank_alias_is_refused() {
        let declared =
            declared_selectable_providers(&[" codex-work ".to_owned(), "codex-work".to_owned()])
                .expect("a well-formed declaration");
        assert_eq!(
            declared,
            ["codex-work".to_owned()]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
        assert!(declared_selectable_providers(&["   ".to_owned()]).is_err());
    }

    #[test]
    fn a_declared_pin_is_walked_only_under_its_own_aliases() {
        let declared = profile(
            serde_json::json!({ "selectable_providers": ["codex-work"] }),
            true,
        );
        let accounts = eligible_accounts(std::slice::from_ref(&declared)).expect("one candidate");
        let every_rung: BTreeSet<String> = ["codex-work".to_owned(), "codex-personal".to_owned()]
            .into_iter()
            .collect();
        assert_eq!(
            pinned_selectable_providers(declared.id, &accounts, every_rung.clone()),
            ["codex-work".to_owned()]
                .into_iter()
                .collect::<BTreeSet<_>>(),
            "a foreign alias rung would launch one account while claiming another"
        );
        // An undeclared pin keeps the latitude it always had: any rung.
        assert_eq!(
            pinned_selectable_providers(
                AccountProfileId::generate(),
                &accounts,
                every_rung.clone()
            ),
            every_rung
        );
    }

    #[test]
    fn a_disabled_profile_is_not_a_candidate_at_all() {
        let profiles = [
            profile(
                serde_json::json!({ "selectable_providers": ["codex"] }),
                false,
            ),
            profile(
                serde_json::json!({ "selectable_providers": ["claude"] }),
                true,
            ),
        ];
        let eligible = eligible_accounts(&profiles).expect("well-formed pins");
        assert_eq!(eligible.len(), 1);
        assert_eq!(
            eligible[0].selectable_providers,
            ["claude".to_owned()].into_iter().collect::<BTreeSet<_>>()
        );
    }
}
