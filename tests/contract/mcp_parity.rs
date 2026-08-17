//! API-to-MCP parity: the generated contract and the tool registry describe the
//! same surface, or this fails.
//!
//! # The invariant, and the canary
//!
//! The durable rule is **completeness**: every Lead-applicable public operation
//! has exactly one tool, every tool targets exactly one operation that exists, and
//! every omission is explicitly allowlisted with a reason. That rule survives the
//! contract growing.
//!
//! On top of it sits a **snapshot canary**: at this base the contract has exactly
//! 29 mapped operations and exactly two allowlisted ones. The canary is not a
//! claim that 29 is forever — it is what makes a later change to the daemon's
//! surface *fail here* rather than pass silently, so somebody has to decide
//! whether the new operation gets a tool or a recorded deferral.
//!
//! # Why the oracle is built from `document()`
//!
//! Because a second handwritten list of routes is exactly the drift this test
//! exists to catch. The left-hand side is the contract utoipa generates from the
//! handlers and DTOs; the right-hand side is `kontor_mcp::REGISTRY`. Neither can be
//! edited to agree with the other without the edit being visible.

use std::collections::{BTreeMap, BTreeSet};

use kontor_mcp::{ArgType, NON_AGENT_ROUTES, Place, REGISTRY, ToolSpec};

/// One documented operation, read out of the generated contract.
#[derive(Debug)]
struct Documented {
    method: String,
    path: String,
    /// `(name, location, required)` for every declared parameter.
    parameters: Vec<(String, String, bool)>,
    /// The top-level properties of the request body, and which are required.
    body: Option<(BTreeSet<String>, BTreeSet<String>)>,
}

/// The generated contract, as any consumer of `/v1/openapi.json` sees it.
fn contract() -> serde_json::Value {
    serde_json::to_value(kontor_api::openapi::document()).expect("the contract serializes")
}

/// Resolve a `$ref` into the components it names.
fn resolve<'a>(
    document: &'a serde_json::Value,
    schema: &'a serde_json::Value,
) -> &'a serde_json::Value {
    let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) else {
        return schema;
    };
    let name = reference
        .rsplit('/')
        .next()
        .expect("a reference names a component");
    document
        .pointer(&format!("/components/schemas/{name}"))
        .unwrap_or_else(|| panic!("{reference} is defined in the contract"))
}

/// Every `/v1` operation the contract declares.
fn documented() -> Vec<Documented> {
    let document = contract();
    let paths = document
        .get("paths")
        .and_then(serde_json::Value::as_object)
        .expect("the contract declares paths")
        .clone();

    let mut operations = Vec::new();
    for (path, item) in paths {
        let item = item.as_object().expect("a path item is an object");
        for (method, operation) in item {
            if !matches!(
                method.as_str(),
                "get" | "post" | "put" | "delete" | "patch" | "head" | "options" | "trace"
            ) {
                continue;
            }
            let parameters = operation
                .get("parameters")
                .and_then(serde_json::Value::as_array)
                .map(|declared| {
                    declared
                        .iter()
                        .map(|parameter| {
                            (
                                parameter["name"].as_str().unwrap_or_default().to_owned(),
                                parameter["in"].as_str().unwrap_or_default().to_owned(),
                                parameter
                                    .get("required")
                                    .and_then(serde_json::Value::as_bool)
                                    .unwrap_or(false),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();

            let body = operation
                .pointer("/requestBody/content/application~1json/schema")
                .map(|schema| {
                    let resolved = resolve(&document, schema);
                    let properties = resolved
                        .get("properties")
                        .and_then(serde_json::Value::as_object)
                        .map(|properties| properties.keys().cloned().collect())
                        .unwrap_or_default();
                    let required = resolved
                        .get("required")
                        .and_then(serde_json::Value::as_array)
                        .map(|required| {
                            required
                                .iter()
                                .filter_map(|name| name.as_str().map(ToOwned::to_owned))
                                .collect()
                        })
                        .unwrap_or_default();
                    (properties, required)
                });

            operations.push(Documented {
                method: method.to_uppercase(),
                path: path.clone(),
                parameters,
                body,
            });
        }
    }
    operations
}

/// The registry's spelling of one tool's operation.
fn route_of(tool: &ToolSpec) -> (String, String) {
    (tool.method.as_str().to_owned(), tool.path.to_owned())
}

#[test]
fn every_documented_operation_is_mapped_once_or_allowlisted_once() {
    let documented = documented();
    let mapped: BTreeMap<(String, String), &'static str> = REGISTRY
        .iter()
        .map(|tool| (route_of(tool), tool.name))
        .collect();
    let allowlisted: BTreeSet<(String, String)> = NON_AGENT_ROUTES
        .iter()
        .map(|route| (route.method.as_str().to_owned(), route.path.to_owned()))
        .collect();

    let mut unmapped = Vec::new();
    for operation in &documented {
        let key = (operation.method.clone(), operation.path.clone());
        let is_mapped = mapped.contains_key(&key);
        let is_allowlisted = allowlisted.contains(&key);
        assert!(
            !(is_mapped && is_allowlisted),
            "{} {} is both mapped and allowlisted",
            operation.method,
            operation.path
        );
        if !is_mapped && !is_allowlisted {
            unmapped.push(key);
        }
    }
    assert!(
        unmapped.is_empty(),
        "these public operations have neither a tool nor an allowlist entry, so nobody decided \
         whether a Lead may reach them: {unmapped:#?}"
    );
}

#[test]
fn every_tool_targets_an_operation_that_exists() {
    let existing: BTreeSet<(String, String)> = documented()
        .into_iter()
        .map(|operation| (operation.method, operation.path))
        .collect();
    for tool in REGISTRY {
        let route = route_of(tool);
        assert!(
            existing.contains(&route),
            "{} targets {} {}, which the contract does not declare",
            tool.name,
            route.0,
            route.1
        );
        assert!(
            tool.path.starts_with("/v1/"),
            "{} targets a route outside /v1",
            tool.name
        );
    }
}

#[test]
fn the_allowlist_holds_only_real_routes_and_the_contract_document_itself() {
    let existing: BTreeSet<(String, String)> = documented()
        .into_iter()
        .map(|operation| (operation.method, operation.path))
        .collect();

    let mut absent = BTreeSet::new();
    for route in NON_AGENT_ROUTES {
        assert!(
            !route.reason.is_empty(),
            "{} is omitted with no reason",
            route.path
        );
        let key = (route.method.as_str().to_owned(), route.path.to_owned());
        if !existing.contains(&key) {
            absent.insert(route.path);
        }
    }
    // The document cannot describe its own endpoint, so that one route is served
    // and undocumented by construction. Every *other* allowlist entry must name a
    // route the contract really has, or it is stale and stopped being reviewed.
    assert_eq!(
        absent,
        BTreeSet::from(["/v1/openapi.json"]),
        "an allowlist entry names a route the contract does not declare"
    );
}

#[test]
fn every_tool_declares_the_same_parameters_the_contract_does() {
    let documented = documented();
    for tool in REGISTRY {
        let route = route_of(tool);
        let operation = documented
            .iter()
            .find(|candidate| (candidate.method.clone(), candidate.path.clone()) == route)
            .unwrap_or_else(|| panic!("{} targets a declared operation", tool.name));

        for (place, location) in [(Place::Path, "path"), (Place::Query, "query")] {
            let declared: BTreeSet<_> =
                tool.args_in(place).map(|arg| arg.name.to_owned()).collect();
            let contracted: BTreeSet<_> = operation
                .parameters
                .iter()
                .filter(|(_, at, _)| at == location)
                .map(|(name, _, _)| name.clone())
                .collect();
            assert_eq!(
                declared, contracted,
                "{}'s {location} parameters disagree with the contract",
                tool.name
            );
        }

        // The one header this surface takes is the idempotency key, and it is
        // present exactly when the operation is a write.
        let header_names: BTreeSet<_> = operation
            .parameters
            .iter()
            .filter(|(_, at, _)| at == "header")
            .map(|(name, _, _)| name.clone())
            .collect();
        let takes_key = header_names.contains("Idempotency-Key");
        assert_eq!(
            tool.is_write(),
            takes_key,
            "{} is {} in the registry and {} in the contract",
            tool.name,
            if tool.is_write() { "a write" } else { "a read" },
            if takes_key {
                "committed under a key"
            } else {
                "not"
            }
        );
        assert_eq!(
            tool.args_in(Place::Header).count(),
            usize::from(takes_key),
            "{} declares the wrong number of headers",
            tool.name
        );
    }
}

#[test]
fn every_tool_declares_the_same_body_properties_the_contract_does() {
    let documented = documented();
    for tool in REGISTRY {
        let route = route_of(tool);
        let operation = documented
            .iter()
            .find(|candidate| (candidate.method.clone(), candidate.path.clone()) == route)
            .unwrap_or_else(|| panic!("{} targets a declared operation", tool.name));

        let declared: BTreeSet<_> = tool
            .args_in(Place::Body)
            .map(|arg| arg.name.to_owned())
            .collect();
        let required: BTreeSet<_> = tool
            .args_in(Place::Body)
            .filter(|arg| arg.required)
            .map(|arg| arg.name.to_owned())
            .collect();

        match &operation.body {
            None => assert!(
                declared.is_empty(),
                "{} declares body properties for an operation that takes no body",
                tool.name
            ),
            Some((properties, contracted_required)) => {
                assert_eq!(
                    declared, *properties,
                    "{}'s body properties disagree with its request DTO",
                    tool.name
                );
                assert_eq!(
                    required, *contracted_required,
                    "{}'s required body properties disagree with its request DTO",
                    tool.name
                );
            }
        }
    }
}

#[test]
fn every_tool_schema_is_closed_and_types_its_properties() {
    for tool in REGISTRY {
        let schema = tool.input_schema();
        assert_eq!(
            schema["additionalProperties"],
            serde_json::Value::Bool(false),
            "{} advertises a schema a caller could smuggle a property past",
            tool.name
        );
        let properties = schema["properties"]
            .as_object()
            .expect("a schema declares properties");
        assert_eq!(
            properties.len(),
            tool.args.len(),
            "{}'s schema and argument list disagree",
            tool.name
        );
        for arg in tool.args {
            let property = &properties[arg.name];
            assert_eq!(
                property["type"].as_str(),
                Some(arg.ty.json_type()),
                "{}'s {} is typed inconsistently",
                tool.name,
                arg.name
            );
            assert!(
                property["description"].is_string(),
                "{}'s {} has no description",
                tool.name,
                arg.name
            );
            if let ArgType::Enum(allowed) = arg.ty {
                let advertised: Vec<_> = property["enum"]
                    .as_array()
                    .expect("an enum property advertises its values")
                    .iter()
                    .map(|value| value.as_str().unwrap_or_default())
                    .collect();
                assert_eq!(
                    advertised, allowed,
                    "{}'s {} enum drifted",
                    tool.name, arg.name
                );
            }
        }
    }
}

/// The closed value set one tool argument declares.
fn declared_enum(tool_name: &str, argument: &str) -> &'static [&'static str] {
    let tool = ToolSpec::find(tool_name).expect("a declared tool");
    let declared = tool
        .args
        .iter()
        .find(|arg| arg.name == argument)
        .map(|arg| arg.ty)
        .unwrap_or_else(|| panic!("{tool_name} has no {argument} argument"));
    match declared {
        ArgType::Enum(allowed) => allowed,
        _ => panic!("{tool_name}'s {argument} is not declared as a closed set"),
    }
}

#[test]
fn the_lifecycle_actions_match_the_contracts_own_enum() {
    // The contract publishes this one as a real enum, so the comparison is against
    // the generated document and a rename in the DTO fails here.
    let document = contract();
    let contracted: Vec<_> = document
        .pointer("/components/schemas/LifecycleAction/enum")
        .and_then(serde_json::Value::as_array)
        .expect("LifecycleAction publishes its values")
        .iter()
        .map(|value| value.as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        declared_enum("kontor_lifecycle_transition", "action"),
        &contracted[..],
        "the lifecycle actions a tool offers are not the ones the daemon accepts"
    );
}

#[test]
fn the_permission_decisions_match_the_runtimes_own_spelling() {
    // `PermissionRequestBody.decision` is declared to the contract as a plain
    // string, so the generated document carries no value set to compare against.
    // The authority is then the runtime enum the daemon deserializes into, and
    // this asserts against *that* rather than skipping — a test that quietly
    // compared nothing would be the worst of the three options.
    let contracted: Vec<String> = [
        kontor_runtime::request::PermissionDecision::Allow,
        kontor_runtime::request::PermissionDecision::Deny,
    ]
    .iter()
    .map(|decision| {
        serde_json::to_value(decision)
            .expect("a decision serializes")
            .as_str()
            .expect("as a string")
            .to_owned()
    })
    .collect();
    assert_eq!(
        declared_enum("kontor_session_permission_respond", "decision"),
        &contracted[..],
        "the decisions a tool offers are not the ones the runtime deserializes"
    );
}

#[test]
fn the_snapshot_canary_holds_at_this_base() {
    // Not "29 forever": this is what makes a later contract change fail here, so a
    // new operation gets a deliberate tool or a recorded deferral instead of
    // slipping past unreviewed.
    assert_eq!(
        REGISTRY.len(),
        71,
        "the mapped-operation count changed; map the new operation or record a deferral"
    );
    assert_eq!(
        NON_AGENT_ROUTES.len(),
        2,
        "the allowlist changed; an omission must be reviewed, not added"
    );
    assert_eq!(
        documented().len(),
        72,
        "the contract's operation count changed; parity must be re-decided"
    );
}

#[test]
fn the_local_documents_and_default_port_match_the_daemons() {
    // `kontor-mcp` restates these because it may not link `kontor-daemon` — that
    // crate reaches the store and every runtime adapter. Restating is safe only
    // because this test compares the two spellings.
    assert_eq!(
        kontor_mcp::client::CREDENTIAL_FILE,
        kontor_daemon::credentials::CREDENTIAL_FILE,
        "the credential file the client reads is not the one the daemon writes"
    );
    assert_eq!(
        kontor_mcp::client::DEFAULT_PORT,
        kontor_daemon::DEFAULT_PORT,
        "a client that defaults to the wrong port cannot reach a default realm"
    );
}

#[test]
fn the_tier_of_every_tool_is_the_one_the_daemon_requires() {
    // The registry's tiers are not an independent policy: they mirror the
    // `caller.require(...)` on the same route. Where they are *stricter* that is
    // deliberate and named; where they are looser it is a defect, because MCP would
    // send a request the daemon is about to refuse.
    use kontor_mcp::CallerTier;
    let expected: BTreeMap<&str, CallerTier> = BTreeMap::from([
        ("kontor_realm_get", CallerTier::Observer),
        ("kontor_run_get", CallerTier::Observer),
        ("kontor_task_get", CallerTier::Observer),
        ("kontor_events_list", CallerTier::Observer),
        ("kontor_work_profiles_list", CallerTier::Observer),
        ("kontor_team_templates_list", CallerTier::Observer),
        ("kontor_runtime_capabilities_list", CallerTier::Observer),
        ("kontor_account_profiles_list", CallerTier::Observer),
        ("kontor_epic_get", CallerTier::Observer),
        ("kontor_session_timeline_get", CallerTier::Observer),
        ("kontor_session_stream_read", CallerTier::Observer),
        ("kontor_project_ensure", CallerTier::Admin),
        ("kontor_account_profile_ensure", CallerTier::Admin),
        ("kontor_epic_apply", CallerTier::Admin),
        ("kontor_execution_arm", CallerTier::Admin),
        ("kontor_execution_disarm", CallerTier::Admin),
        ("kontor_profile_select", CallerTier::Admin),
        ("kontor_team_select", CallerTier::Admin),
        ("kontor_account_select", CallerTier::Admin),
        ("kontor_scheduler_plan", CallerTier::Operator),
        ("kontor_scheduler_start", CallerTier::Operator),
        ("kontor_lifecycle_transition", CallerTier::Operator),
        ("kontor_context_resolve", CallerTier::Operator),
        ("kontor_gate_record", CallerTier::Operator),
        ("kontor_runtime_settle", CallerTier::Operator),
        // Abandoning an unbound run drives the same seat-shaped aggregate that
        // settlement does, so it sits at the same tier — no wider, because the
        // daemon refuses it outright once the seat holds a session.
        ("kontor_runtime_abandon", CallerTier::Operator),
        ("kontor_ticket_reconcile_plan", CallerTier::Operator),
        ("kontor_ticket_reconcile_apply", CallerTier::Operator),
        ("kontor_session_message_send", CallerTier::Operator),
        ("kontor_session_permission_respond", CallerTier::Operator),
        // KON-15 route additions: the five new surface groups.
        ("kontor_work_profile_get", CallerTier::Observer),
        ("kontor_work_profile_validate", CallerTier::Observer),
        ("kontor_trigger_get", CallerTier::Observer),
        ("kontor_intake_submit", CallerTier::Operator),
        ("kontor_intake_receipt_get", CallerTier::Observer),
        ("kontor_connector_field_specs_list", CallerTier::Observer),
        ("kontor_connector_workflow_specs_list", CallerTier::Observer),
        ("kontor_ticket_conflicts_list", CallerTier::Observer),
        ("kontor_ticket_conflict_resolve", CallerTier::Operator),
        ("kontor_ticket_comments_pull", CallerTier::Operator),
        ("kontor_ticket_comments_list", CallerTier::Observer),
        ("kontor_ticket_claim", CallerTier::Operator),
        // KON-15 round 2: registering a catalogue widens what every later apply
        // in this realm may freeze onto a task, so it is an admin act; listing
        // what the realm can resolve from is a read.
        // A bounded role turn is Kontor's own decision about its own work, so it
        // is an operator act like every other seat-driving one.
        ("kontor_turn_settle", CallerTier::Operator),
        ("kontor_late_handoff_attest", CallerTier::Admin),
        ("kontor_seat_replace", CallerTier::Admin),
        ("kontor_profile_packs_list", CallerTier::Observer),
        ("kontor_profile_pack_register", CallerTier::Admin),
        // KON-24: the context-window preview reads and changes nothing; the
        // explicit compaction drives a session and is an operator act.
        ("kontor_context_policy_preview", CallerTier::Observer),
        ("kontor_session_compact", CallerTier::Operator),
        // KON-16: excusing a declared slot discharges an obligation the frozen
        // template imposed, which is the same kind of act as waiving a gate — so
        // the daemon requires admin on the route and the registry says so too.
        ("kontor_role_slot_waive", CallerTier::Admin),
        ("kontor_memory_search", CallerTier::Observer),
        ("kontor_memory_history", CallerTier::Observer),
        ("kontor_memory_propose", CallerTier::Operator),
        ("kontor_memory_approve", CallerTier::Admin),
        ("kontor_memory_tombstone", CallerTier::Admin),
        ("kontor_memory_purge", CallerTier::Admin),
        ("kontor_memory_ingest_preview", CallerTier::Admin),
        ("kontor_memory_ingest_apply", CallerTier::Admin),
        ("kontor_memory_cutover_freeze", CallerTier::Admin),
        ("kontor_memory_cutover_switch", CallerTier::Admin),
        // KON-25: the Realm catalogue and Teams projection are reads; saving a
        // draft and publishing its next immutable revision are operator acts.
        ("kontor_model_catalog_get", CallerTier::Observer),
        ("kontor_teams_get", CallerTier::Observer),
        ("kontor_team_draft_save", CallerTier::Operator),
        ("kontor_team_publish", CallerTier::Operator),
        // KON-OP-03: publishing or reading a topology specification decides what
        // kinds may ever exist in this project, so the whole specification family
        // is admin — including the two that persist nothing, because a builder
        // that hands back a complete candidate is still describing the shape of
        // the project's sessions. The catalog and code help are the opposite:
        // they are the server's own dictionary, and a client that cannot read
        // them has to keep a private one, which is the failure they exist to
        // prevent.
        ("kontor_topology_spec_draft", CallerTier::Admin),
        ("kontor_topology_spec_validate", CallerTier::Admin),
        ("kontor_topology_spec_publish", CallerTier::Admin),
        ("kontor_topology_spec_get", CallerTier::Admin),
        ("kontor_role_catalog_get", CallerTier::Observer),
        ("kontor_role_get", CallerTier::Observer),
        ("kontor_code_help_get", CallerTier::Observer),
    ]);
    for tool in REGISTRY {
        assert_eq!(
            Some(&tool.tier),
            expected.get(tool.name),
            "{}'s authority is not the one the daemon requires on its route",
            tool.name
        );
    }
    assert_eq!(expected.len(), REGISTRY.len());
}
