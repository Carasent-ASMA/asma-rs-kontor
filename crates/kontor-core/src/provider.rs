//! Provider normalization identifies a family; the host supplies the harness and account.

// The boolean preserves the existing fleet-domain subset of runtime families.
const PROVIDERS: [(&str, bool); 8] = [
    ("claude", true),
    ("codex", true),
    ("cursor", true),
    ("opencode", true),
    ("copilot", false),
    ("pi", false),
    ("omp", false),
    ("glm-acp-agent", false),
];

/// Resolve a syntactically valid known provider/account id to its unique family.
/// An account suffix alone supplies no catalog, placement or permission grant.
#[must_use]
pub fn normalize_provider(provider: &str) -> Option<&'static str> {
    if !provider
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_lowercase)
        || !provider
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return None;
    }
    let mut matches = PROVIDERS.iter().filter(|(family, _)| {
        provider == *family
            || provider
                .strip_prefix(family)
                .and_then(|suffix| suffix.strip_prefix('-'))
                .is_some_and(|suffix| !suffix.is_empty())
    });
    let (family, _) = matches.next()?;
    matches.next().is_none().then_some(*family)
}

/// The known family, or the unchanged unknown id for legacy refusal diagnostics.
#[must_use]
pub fn provider_family(provider: &str) -> &str {
    normalize_provider(provider).unwrap_or(provider)
}

/// Whether a canonical provider is supported as a fleet domain.
#[must_use]
pub fn fleet_domain_provider(provider: &str) -> bool {
    PROVIDERS
        .iter()
        .any(|(family, fleet)| *family == provider && *fleet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_is_shared_and_unknown_or_malformed_ids_refuse() {
        for (family, _) in PROVIDERS {
            assert_eq!(normalize_provider(family), Some(family));
            assert_eq!(
                normalize_provider(&format!("{family}-secondary")),
                Some(family)
            );
        }
        for unknown in [
            "",
            "cursor-",
            "cursorfoo",
            "Cursor-secondary",
            "cursor:secondary",
            "unknown-secondary",
            "deepseek-harness",
        ] {
            assert_eq!(normalize_provider(unknown), None, "{unknown}");
            assert_eq!(provider_family(unknown), unknown);
        }
        assert!(!fleet_domain_provider("copilot"));
        assert!(!fleet_domain_provider("cursor-secondary"));
        assert!(!fleet_domain_provider("glm-acp-agent"));
    }
}
