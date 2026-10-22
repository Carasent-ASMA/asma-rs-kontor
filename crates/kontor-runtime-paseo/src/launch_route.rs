//! Private launch bindings; policy and public provenance keep canonical identities.

use std::collections::BTreeMap;

use kontor_core::id::ContentHash;
use kontor_core::launch_route::ResolvedRoute;
use kontor_core::spec::{ModelRef, ModelRung, ProviderRef};
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};

pub(crate) fn validate(canonical: &ModelRung, route: Option<&ResolvedRoute>) -> RuntimeResult<()> {
    if let Some(route) = route {
        route.validate()?;
        if route.canonical_rung() != *canonical
            || route.route_harness != "paseo"
            || kontor_core::provider::normalize_provider(&route.route_provider)
                != Some(route.route_provider.as_str())
        {
            return Err(RuntimeError::LaunchNotAdmitted {
                rule: "host_route_binding_mismatch",
            });
        }
    }
    Ok(())
}

pub(crate) fn family<'a>(canonical: &'a ModelRung, route: Option<&'a ResolvedRoute>) -> &'a str {
    route.map_or(canonical.provider.0.as_str(), |route| {
        route.route_provider.as_str()
    })
}

pub(crate) fn native_rung(canonical: &ModelRung, route: Option<&ResolvedRoute>) -> ModelRung {
    route.map_or_else(
        || canonical.clone(),
        |route| ModelRung {
            provider: ProviderRef(route.native_alias.clone()),
            model: ModelRef(route.route_model_id.clone()),
            effort: route.canonical_effort,
        },
    )
}

pub(crate) fn family_rung(canonical: &ModelRung, route: Option<&ResolvedRoute>) -> ModelRung {
    let mut rung = native_rung(canonical, route);
    rung.provider = ProviderRef(family(canonical, route).to_owned());
    rung
}

pub(crate) fn digest(route: &ResolvedRoute) -> RuntimeResult<ContentHash> {
    serde_json::to_vec(route)
        .map(|bytes| ContentHash::of(&bytes))
        .map_err(|_| RuntimeError::LaunchNotAdmitted {
            rule: "host_route_snapshot_invalid",
        })
}

pub(crate) fn bind_labels(
    labels: &mut BTreeMap<String, String>,
    route: Option<&ResolvedRoute>,
) -> RuntimeResult<()> {
    if let Some(route) = route {
        labels.insert(
            crate::wire::label::LAUNCH_INTENT.to_owned(),
            digest(route)?.to_string(),
        );
    }
    Ok(())
}

pub(crate) fn verify_agent(
    agent: &crate::wire::PaseoAgent,
    route: Option<&ResolvedRoute>,
) -> RuntimeResult<()> {
    if let Some(route) = route {
        validate(&route.canonical_rung(), Some(route))?;
        if agent.provider != route.native_alias
            || agent.model != route.route_model_id
            || route.canonical_effort.is_some_and(|effort| {
                agent.effective_thinking_option_id.as_deref() != Some(effort.as_str())
            })
            || agent.label(crate::wire::label::LAUNCH_INTENT) != Some(digest(route)?.as_str())
        {
            return Err(RuntimeError::CorrelationFailed);
        }
    }
    Ok(())
}

pub(crate) fn bind_rpc(request: &mut crate::client::PaseoRpc, route: Option<&ResolvedRoute>) {
    if let Some(route) = route {
        request.message["config"]["provider"] = serde_json::json!(route.native_alias);
        request.message["config"]["model"] = serde_json::json!(route.route_model_id);
    }
}
