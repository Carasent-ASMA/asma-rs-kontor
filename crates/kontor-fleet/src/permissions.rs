//! Exact behavioral consultation exceptions; never a containment claim.
use super::*;

const FIELD: &str = "consultation_permissions must name an exact declared account, symbolic model, supported effort and plan mode on a permission_eligible model in schema_version 2, without duplicates";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PermissionSpec {
    account: String,
    model: String,
    // The key is required, including an explicit null for a model with no effort.
    #[serde(deserialize_with = "Option::<EffortLevel>::deserialize")]
    effort: Option<EffortLevel>,
    mode: String,
}

/// A projection of one validated fleet policy, with its exact content identity.
/// Fields are private: callers cannot manufacture an exception from a route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsultationPermissions {
    hash: ContentHash,
    routes: Vec<ModelRung>,
}

impl ConsultationPermissions {
    /// Whether this exact policy hash explicitly admits this exact route.
    #[must_use]
    pub fn permits(&self, policy_hash: &ContentHash, rung: &ModelRung) -> bool {
        self.hash == *policy_hash && self.routes.contains(rung)
    }
}

pub(super) fn validate(document: &FleetDocument, schema: Schema) -> Result<(), FleetError> {
    if !document.consultation_permissions.is_empty() && schema != Schema::V2 {
        return Err(invalid(FIELD));
    }
    let mut seen = BTreeSet::new();
    for permission in &document.consultation_permissions {
        let model = document
            .models
            .get(&permission.model)
            .ok_or(invalid(FIELD))?;
        let domain = document.domains.get(&model.domain).ok_or(invalid(FIELD))?;
        let matches = document
            .models
            .values()
            .filter(|candidate| {
                candidate.id == model.id
                    && document.domains[&candidate.domain]
                        .accounts
                        .contains(&permission.account)
            })
            .count();
        if permission.mode != "plan"
            || matches != 1
            || !model.permission_eligible
            || !matches!(
                normalize_provider(&permission.account),
                Some("cursor" | "opencode")
            )
            || normalize_provider(&permission.account) != Some(domain.provider.as_str())
            || !domain.accounts.contains(&permission.account)
            || permission.account.contains(['*', '?', '[', ']'])
            || model.id.contains(['*', '?', '[', ']'])
            || model.vendor == "unknown"
            || match permission.effort {
                Some(effort) => !model.efforts.iter().any(|value| value == effort.as_str()),
                None => !model.efforts.is_empty(),
            }
            || !seen.insert((
                &permission.account,
                &model.id,
                permission.effort.map(EffortLevel::as_str),
            ))
        {
            return Err(invalid(FIELD));
        }
    }
    Ok(())
}

pub(super) fn resolve(hash: &ContentHash, document: &FleetDocument) -> ConsultationPermissions {
    ConsultationPermissions {
        hash: hash.clone(),
        routes: document
            .consultation_permissions
            .iter()
            .map(|permission| ModelRung {
                provider: ProviderRef(permission.account.clone()),
                model: ModelRef(document.models[&permission.model].id.clone()),
                effort: permission.effort,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const POLICY: &str =
        include_str!("../../../tests/fixtures/config/consultation-permissions.yml");

    fn rung(account: &str, model: &str, effort: Option<EffortLevel>) -> ModelRung {
        ModelRung {
            provider: ProviderRef(account.into()),
            model: ModelRef(model.into()),
            effort,
        }
    }

    #[test]
    fn consultation_permission_requires_exact_route_and_policy_hash() {
        let policy = FleetSnapshot::parse_policy(POLICY).unwrap();
        let permissions = policy.consultation_permissions();
        let selected = rung("cursor", "review-model", Some(EffortLevel::Xhigh));
        assert!(permissions.permits(policy.hash(), &selected));
        assert!(!permissions.permits(&ContentHash::of(b"another policy"), &selected));
        for other in [
            rung("cursor-secondary", "review-model", Some(EffortLevel::Xhigh)),
            rung("cursor", "other-model", Some(EffortLevel::Xhigh)),
            rung("cursor", "review-model", Some(EffortLevel::High)),
            rung("cursor", "review-model", None),
        ] {
            assert!(!permissions.permits(policy.hash(), &other));
        }
        assert!(permissions.permits(
            policy.hash(),
            &rung("opencode", "example/review", Some(EffortLevel::Max))
        ));
        assert!(permissions.permits(
            policy.hash(),
            &rung("opencode-secondary", "example/default", None)
        ));
        let omitted = POLICY.split("consultation_permissions:").next().unwrap();
        let omitted = FleetSnapshot::parse_policy(omitted).unwrap();
        assert!(
            !omitted
                .consultation_permissions()
                .permits(omitted.hash(), &selected)
        );
    }

    #[test]
    fn consultation_permission_refuses_unknown_wildcard_partial_duplicate_or_unproven_fields() {
        for (from, to) in [
            ("account: cursor,", "account: absent,"),
            ("account: cursor,", "account: '*',"),
            ("model: cursor-review,", "model: absent,"),
            ("model: cursor-review,", "model: '*',"),
            ("effort: xhigh,", "effort: ultra,"),
            ("effort: xhigh,", "effort: '*',"),
            ("effort: xhigh,", "effort: null,"),
            ("effort: xhigh,", ""),
            ("mode: plan", "mode: auto"),
            ("mode: plan", "mode: '*'"),
            ("mode: plan", ""),
            ("vendor: example", "vendor: unknown"),
            ("schema_version: 2", "schema_version: 1"),
        ] {
            let altered = POLICY.replacen(from, to, 1);
            assert!(
                FleetSnapshot::parse_policy(&altered).is_err(),
                "{from} -> {to}"
            );
        }
        for document in [
            POLICY.replace("cursor-secondary", "cursor-*").replacen(
                "account: cursor,",
                "account: cursor-*,",
                1,
            ),
            POLICY.replace("id: review-model", "id: review-*"),
        ] {
            assert!(FleetSnapshot::parse_policy(&document).is_err());
        }
        let duplicate = format!(
            "{POLICY}  - {{account: cursor, model: cursor-review, effort: xhigh, mode: plan}}\n"
        );
        assert!(FleetSnapshot::parse_policy(&duplicate).is_err());
    }

    #[test]
    fn consultation_permission_bytes_are_bound_to_publication_and_activation() {
        let policy = FleetSnapshot::parse_policy(POLICY).unwrap();
        assert!(FleetSnapshot::activated(policy.hash(), 2, POLICY).is_ok());
        let edited = POLICY.replacen("effort: xhigh, mode:", "effort: high, mode:", 1);
        assert!(FleetSnapshot::published(policy.hash(), &edited).is_err());
        let edited = FleetSnapshot::parse_policy(&edited).unwrap();
        assert_ne!(policy.hash(), edited.hash());
        assert!(!edited.consultation_permissions().permits(
            policy.hash(),
            &rung("cursor", "review-model", Some(EffortLevel::High))
        ));
    }
}

#[cfg(test)]
mod eligibility_tests {
    use super::*;
    const POLICY: &str =
        include_str!("../../../tests/fixtures/config/consultation-permissions.yml");

    #[test]
    fn permission_eligible_is_explicit_required_and_hash_bound() {
        let policy = FleetSnapshot::parse_policy(POLICY).unwrap();
        let route = ModelRung {
            provider: ProviderRef("cursor".into()),
            model: ModelRef("review-model".into()),
            effort: Some(EffortLevel::Xhigh),
        };
        assert!(
            policy
                .consultation_permissions()
                .permits(policy.hash(), &route)
        );
        for text in [
            POLICY.replacen("permission_eligible: true", "permission_eligible: false", 1),
            POLICY.replace("    permission_eligible: true\n", ""),
        ] {
            assert!(matches!(
                FleetSnapshot::parse_policy(&text),
                Err(FleetError::Invalid { rule: FIELD })
            ));
        }
        let plain = POLICY.split("consultation_permissions:").next().unwrap();
        let eligible = FleetSnapshot::parse_policy(plain).unwrap();
        let ineligible = plain.replace("    permission_eligible: true\n", "");
        let old = FleetSnapshot::parse_policy(&ineligible).unwrap();
        assert_ne!(eligible.hash(), old.hash());
        assert!(FleetSnapshot::published(eligible.hash(), &ineligible).is_err());
        assert!(
            FleetSnapshot::parse(&ineligible.replace("schema_version: 2", "schema_version: 1"))
                .is_ok()
        );
        let automatic = POLICY
            .replace("id: review-model", "id: auto-smart")
            .replacen("permission_eligible: true", "permission_eligible: false", 1);
        assert!(FleetSnapshot::parse_policy(&automatic).is_err());
    }
}
