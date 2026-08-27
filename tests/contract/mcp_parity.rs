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
//! 148 mapped operations and exactly two allowlisted ones. The canary is not a
//! claim that 148 is forever — it is what makes a later change to the daemon's
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

use kontor_mcp::{ArgType, CLI_ONLY, NON_AGENT_ROUTES, Place, REGISTRY, ToolSpec};

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
    if let Some(one_of) = schema.get("oneOf").and_then(serde_json::Value::as_array)
        && let [nullable, value] = one_of.as_slice()
    {
        let value = if nullable.get("type").and_then(serde_json::Value::as_str) == Some("null") {
            value
        } else if value.get("type").and_then(serde_json::Value::as_str) == Some("null") {
            nullable
        } else {
            schema
        };
        if !std::ptr::eq(value, schema) {
            return resolve(document, value);
        }
    }
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

/// Every declared nested object matches the DTO field it stands for.
///
/// `ArgType::Object` exists so a caller can read a nested shape instead of
/// guessing it — which is worth having only while the declaration is *true*. A
/// schema that has drifted from its DTO is worse than the bare `object` it
/// replaced: the caller has no reason to doubt it, so a wrong field name is
/// discovered from a refusal anyway, having first been advertised as correct.
///
/// This is stated over the whole registry rather than over one tool, so a later
/// `ArgType::Object` gets the same guarantee without anyone remembering to ask
/// for it.
#[test]
fn every_declared_nested_object_matches_the_contracts_own_dto() {
    let document = contract();
    let mut checked = 0_usize;
    for tool in REGISTRY {
        let route = route_of(tool);
        let Some(schema) = document
            .pointer(&format!(
                "/paths/{}/{}/requestBody/content/application~1json/schema",
                tool.path.replace('/', "~1"),
                route.0.to_lowercase()
            ))
            .map(|schema| resolve(&document, schema))
        else {
            continue;
        };

        for arg in tool.args_in(Place::Body) {
            let (fields, is_array) = match arg.ty {
                ArgType::Object(fields) => (fields, false),
                ArgType::ObjectArray(fields) => (fields, true),
                _ => continue,
            };
            let property = schema
                .pointer(&format!("/properties/{}", arg.name))
                .unwrap_or_else(|| panic!("{}'s {} is a property of its DTO", tool.name, arg.name));
            let resolved = if is_array {
                resolve(
                    &document,
                    property.get("items").unwrap_or_else(|| {
                        panic!("{}'s {} declares array items", tool.name, arg.name)
                    }),
                )
            } else {
                resolve(&document, property)
            };

            let contracted: BTreeSet<String> = resolved
                .get("properties")
                .and_then(serde_json::Value::as_object)
                .map(|properties| properties.keys().cloned().collect())
                .unwrap_or_default();
            let declared: BTreeSet<String> =
                fields.iter().map(|field| field.name.to_owned()).collect();
            assert_eq!(
                declared, contracted,
                "{}'s {} declares a shape its DTO does not have",
                tool.name, arg.name
            );

            let contracted_required: BTreeSet<String> = resolved
                .get("required")
                .and_then(serde_json::Value::as_array)
                .map(|required| {
                    required
                        .iter()
                        .filter_map(|name| name.as_str().map(ToOwned::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let declared_required: BTreeSet<String> = fields
                .iter()
                .filter(|field| field.required)
                .map(|field| field.name.to_owned())
                .collect();
            assert_eq!(
                declared_required, contracted_required,
                "{}'s {} disagrees with its DTO about which fields are required",
                tool.name, arg.name
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "no nested object was checked, so this test proved nothing"
    );
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
    // Not "148 forever": this is what makes a later contract change fail here, so a
    // new operation gets a deliberate tool or a recorded deferral instead of
    // slipping past unreviewed.
    assert_eq!(
        REGISTRY.len(),
        148,
        "the mapped-operation count changed; map the new operation or record a deferral"
    );
    // Not every mapped operation is an advertised one. `CLI_ONLY` is subtracted
    // from `tools/list` and nowhere else, so this second number is what a seat's
    // context is actually charged for — and it has to move deliberately too.
    assert_eq!(
        REGISTRY.len() - CLI_ONLY.len(),
        147,
        "the advertised tool count changed; a tool held off the listing is a budget decision"
    );
    assert_eq!(
        NON_AGENT_ROUTES.len(),
        2,
        "the allowlist changed; an omission must be reviewed, not added"
    );
    assert_eq!(
        documented().len(),
        149,
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
        ("kontor_projects_list", CallerTier::Observer),
        ("kontor_project_get", CallerTier::Observer),
        ("kontor_project_ensure", CallerTier::Admin),
        ("kontor_account_profile_ensure", CallerTier::Admin),
        // Retiring an account is admin for the same reason recording quota is:
        // disabling every profile but one routes the whole realm onto it.
        ("kontor_account_profile_amend", CallerTier::Admin),
        ("kontor_epic_apply", CallerTier::Admin),
        ("kontor_epic_preview", CallerTier::Admin),
        ("kontor_execution_arm", CallerTier::Admin),
        ("kontor_execution_disarm", CallerTier::Admin),
        ("kontor_profile_select", CallerTier::Admin),
        ("kontor_team_select", CallerTier::Admin),
        ("kontor_account_select", CallerTier::Admin),
        ("kontor_scheduler_plan", CallerTier::Operator),
        ("kontor_scheduler_start", CallerTier::Operator),
        ("kontor_scheduler_resume", CallerTier::Operator),
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
        ("kontor_topology_seat_message_send", CallerTier::Operator),
        ("kontor_session_permission_respond", CallerTier::Operator),
        // KON-15 route additions: the five new surface groups.
        ("kontor_work_profile_get", CallerTier::Observer),
        ("kontor_work_profile_validate", CallerTier::Observer),
        ("kontor_trigger_get", CallerTier::Observer),
        ("kontor_intake_submit", CallerTier::Operator),
        ("kontor_intake_receipt_get", CallerTier::Observer),
        ("kontor_connector_field_specs_list", CallerTier::Observer),
        ("kontor_connector_workflow_specs_list", CallerTier::Observer),
        ("kontor_connector_workflow_spec_install", CallerTier::Admin),
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
        ("kontor_subject_authority_get", CallerTier::Observer),
        ("kontor_subject_authority_attest", CallerTier::Admin),
        ("kontor_memory_cutover_switch", CallerTier::Admin),
        ("kontor_backlog_import_preview", CallerTier::Admin),
        ("kontor_backlog_import_apply", CallerTier::Admin),
        ("kontor_backlog_cutover_switch", CallerTier::Admin),
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
        // KON-OP-03: naming a semantic scope is operator work — it is asking
        // Kontor to place the sessions the work already implies. Moving an
        // epic's pinned specification is not: it changes what kinds may exist
        // for work already running, so both halves of the upgrade are admin.
        ("kontor_topology_inspect", CallerTier::Observer),
        ("kontor_topology_drift", CallerTier::Operator),
        ("kontor_topology_ensure", CallerTier::Operator),
        ("kontor_topology_materialize", CallerTier::Operator),
        ("kontor_topology_retire", CallerTier::Operator),
        ("kontor_topology_archive", CallerTier::Operator),
        (
            "kontor_project_topology_selection_preview",
            CallerTier::Admin,
        ),
        ("kontor_project_topology_selection_apply", CallerTier::Admin),
        ("kontor_jira_materialization_preview", CallerTier::Admin),
        ("kontor_jira_materialization_apply", CallerTier::Admin),
        ("kontor_topology_upgrade_preview", CallerTier::Admin),
        ("kontor_topology_upgrade_apply", CallerTier::Admin),
        ("kontor_native_names_preview", CallerTier::Admin),
        ("kontor_native_names_apply", CallerTier::Admin),
        // KON-OP-03: the ceilings a realm admits work under are configuration,
        // so reading and changing them is admin. Collecting evidence, judging
        // an account and attending or releasing one exact seat are operator
        // work; the derived picture itself is a read anyone may take.
        ("kontor_capacity_config_get", CallerTier::Admin),
        ("kontor_capacity_config_preview", CallerTier::Admin),
        ("kontor_capacity_config_apply", CallerTier::Admin),
        ("kontor_capacity_get", CallerTier::Observer),
        ("kontor_capacity_refresh", CallerTier::Operator),
        ("kontor_capacity_observation_get", CallerTier::Observer),
        ("kontor_capacity_override", CallerTier::Operator),
        ("kontor_seat_attention", CallerTier::Operator),
        ("kontor_seat_retire", CallerTier::Operator),
        // KON-OP-03 successor contracts. Configuration — who the Core Team is,
        // what an Advisor, Committee or Completion profile says, which roster
        // an epic pins — is admin. Running a consultation, opening or promoting
        // Quick work and moving a completion are operator acts. The catalogs
        // themselves are reads.
        ("kontor_core_team_get", CallerTier::Observer),
        ("kontor_core_team_preview", CallerTier::Admin),
        ("kontor_core_team_apply", CallerTier::Admin),
        ("kontor_core_team_materialize", CallerTier::Operator),
        ("kontor_core_team_route_preview", CallerTier::Admin),
        ("kontor_core_team_route_apply", CallerTier::Admin),
        ("kontor_seat_claim_preview", CallerTier::Admin),
        ("kontor_seat_claim_apply", CallerTier::Admin),
        ("kontor_quick_roles_list", CallerTier::Observer),
        ("kontor_quick_session_ensure", CallerTier::Operator),
        ("kontor_promotion_preview", CallerTier::Operator),
        ("kontor_promotion_apply", CallerTier::Operator),
        ("kontor_roster_upgrade_preview", CallerTier::Admin),
        ("kontor_roster_upgrade_apply", CallerTier::Admin),
        ("kontor_advisor_profiles_list", CallerTier::Observer),
        ("kontor_advisor_profile_preview", CallerTier::Admin),
        ("kontor_advisor_profile_apply", CallerTier::Admin),
        ("kontor_advisor_run_invoke", CallerTier::Operator),
        ("kontor_advisor_run_settle", CallerTier::Operator),
        ("kontor_advisor_run_get", CallerTier::Observer),
        ("kontor_committee_templates_list", CallerTier::Observer),
        ("kontor_committee_template_preview", CallerTier::Admin),
        ("kontor_committee_template_apply", CallerTier::Admin),
        ("kontor_committee_run_invoke", CallerTier::Operator),
        ("kontor_consultation_seat_recover", CallerTier::Admin),
        // Rerouting a native-less seat replaces the active generation and
        // provider route under immutable lineage. That authority-changing
        // compare-and-swap is Admin-only on both the daemon and MCP surfaces.
        (
            "kontor_committee_seat_reroute_unmaterialized",
            CallerTier::Admin,
        ),
        ("kontor_committee_findings_record", CallerTier::Operator),
        ("kontor_committee_run_get", CallerTier::Observer),
        ("kontor_committee_run_settle", CallerTier::Operator),
        ("kontor_completion_profiles_list", CallerTier::Observer),
        ("kontor_completion_profile_preview", CallerTier::Admin),
        ("kontor_completion_profile_apply", CallerTier::Admin),
        ("kontor_completion_get", CallerTier::Observer),
        ("kontor_completion_advance", CallerTier::Operator),
        ("kontor_completion_remediate", CallerTier::Operator),
        // Repairing a container's visible title is admin: what it corrects is a
        // rendering decision the control plane made, and the operation derives the
        // title rather than accepting one.
        ("kontor_container_retitle_preview", CallerTier::Admin),
        ("kontor_container_retitle_apply", CallerTier::Admin),
        // Runtime-owned correlation labels are immutable placement evidence.
        // Repair is exact-id/generation fenced and therefore Admin-only.
        ("kontor_session_labels_reconcile", CallerTier::Admin),
        // Publishing a trigger may declare a bounded auto-arm, which is the
        // capability to start work with no human in the loop. That is an
        // authority grant, so the daemon requires admin on the route.
        ("kontor_trigger_publish", CallerTier::Admin),
        // Reading which providers are out of quota is observation. Recording it
        // is admin: the state decides which rung a launch lands on, so a caller
        // who can write it can route every seat in the realm onto a provider of
        // their choosing by declaring the others exhausted.
        ("kontor_provider_quota_states_list", CallerTier::Observer),
        ("kontor_provider_quota_record", CallerTier::Admin),
        ("kontor_provider_quota_probe", CallerTier::Operator),
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
