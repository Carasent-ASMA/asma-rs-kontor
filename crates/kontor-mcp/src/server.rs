//! The MCP server: one Realm, one authority, one tool list.
//!
//! # One authority for the whole process
//!
//! A [`KontorMcp`] is built with exactly one [`CallerTier`] and holds exactly one
//! credential for it. There is no per-call authority, no escalation operand and no
//! way for a tool to ask for a different secret — a tool is handed a dispatcher
//! that already is what it is. Running two authorities means running two servers,
//! which is the honest way to say it: the credential a process holds is a fact
//! about the process.
//!
//! [`KontorMcp::served`] therefore lists only what this server can actually
//! perform. A caller that names a higher-tier tool anyway is still refused by the
//! gate — the two are not redundant. Listing everything and refusing later would
//! invite a language model to keep trying a tool that will never work; refusing
//! *only* by omission would let a caller conclude the tool does not exist when the
//! real answer is that this process was started at the wrong authority.
//!
//! # The stdio bridge, and why it is not `rmcp::transport::io::stdio`
//!
//! `rmcp`'s own stdio helper needs the `transport-io` feature, which needs
//! `tokio`'s `io-std`. Neither is enabled in this workspace's pinned dependency
//! set, and the root manifest is owned by KON-MVP-02 (CON-007) — a ticket does not
//! edit it. So [`serve_stdio`] bridges the process's blocking standard streams onto
//! an in-memory duplex pair and hands `rmcp` that instead. The protocol, the
//! framing and the handshake are all still `rmcp`'s; only the two pipes are ours.
//!
//! The bridge is also what makes the protocol testable without a socket or a child
//! process: a test drives [`serve`] over the *other* half of a duplex pair, which
//! is the same code path the binary takes minus the two pumps.

use std::borrow::Cow;
use std::future::Future;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData,
    Implementation, InitializeResult, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
    Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt as _};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt as _};

use crate::client::CallerTier;
use crate::dispatch::{Dispatcher, Envelope, Failure};
use crate::registry::ToolSpec;

/// How many bytes the stdio bridge buffers in each direction.
///
/// One MCP frame is a single JSON line; 256 KiB is far more than a tool result of
/// this contract produces, and the duplex applies backpressure rather than
/// truncating if one ever exceeds it.
const BRIDGE_BUFFER: usize = 256 * 1024;

/// The capability-gated tool server for one Realm.
pub struct KontorMcp {
    dispatcher: Arc<Dispatcher>,
}

impl std::fmt::Debug for KontorMcp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KontorMcp")
            .field("authority", &self.dispatcher.tier())
            .finish_non_exhaustive()
    }
}

impl KontorMcp {
    /// A server that acts at exactly the dispatcher's tier and no higher.
    #[must_use]
    pub fn new(dispatcher: Dispatcher) -> Self {
        Self {
            dispatcher: Arc::new(dispatcher),
        }
    }

    /// The authority this server was configured with.
    #[must_use]
    pub fn authority(&self) -> CallerTier {
        self.dispatcher.tier()
    }

    /// Every operation this server can perform.
    #[must_use]
    pub fn served(&self) -> Vec<&'static ToolSpec> {
        self.dispatcher.tools().collect()
    }

    /// The MCP declaration of one operation.
    ///
    /// Built through `Tool::new` rather than as a struct literal: the model types
    /// are `#[non_exhaustive]`, so going through the constructor means a later rmcp
    /// generation that adds a field does not silently leave it at whatever this
    /// build assumed.
    fn declare(tool: &&'static ToolSpec) -> Tool {
        Tool::new(
            Cow::Borrowed(tool.name),
            Cow::Borrowed(tool.about),
            Arc::new(match tool.input_schema() {
                serde_json::Value::Object(schema) => schema,
                // `input_schema` builds an object literal; this arm cannot be taken
                // and an empty schema is the harmless reading if it ever were.
                _ => serde_json::Map::new(),
            }),
        )
    }
}

impl ServerHandler for KontorMcp {
    fn get_info(&self) -> InitializeResult {
        let mut implementation = Implementation::from_build_env();
        implementation.name = "kontor".to_owned();
        implementation.title = Some("Kontor control plane".to_owned());
        implementation.description = Some(format!(
            "The loopback tool surface of one Kontor realm, served at {} authority.",
            self.dispatcher.tier()
        ));
        let mut info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build());
        info.server_info = implementation;
        info.instructions = Some(instructions(self.dispatcher.tier()));
        info
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        let listed =
            ListToolsResult::with_all_items(self.served().iter().map(Self::declare).collect());
        std::future::ready(Ok(listed))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.served()
            .iter()
            .find(|tool| tool.name == name)
            .map(Self::declare)
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResponse, ErrorData>> + Send + '_ {
        let dispatcher = Arc::clone(&self.dispatcher);
        async move {
            let arguments = serde_json::Value::Object(request.arguments.unwrap_or_default());
            match dispatcher.call(&request.name, &arguments).await {
                // A refusal is a tool *result* rather than a protocol error: it is
                // an answer about this realm, and a caller that can read the code
                // and the body can decide what to do. A JSON-RPC error would strip
                // both down to a message.
                Err(failure) => Ok(refused(&request.name, &failure).into()),
                Ok(envelope) => Ok(answer(&envelope).into()),
            }
        }
    }
}

/// Render one answer as a tool result, carrying the daemon's document unchanged.
///
/// A non-2xx status is marked as an error so a client sees it as one, and the body
/// underneath is still exactly what the daemon sent — the receipt, the revision or
/// the refusal code a caller is owed is not summarized away.
fn answer(envelope: &Envelope) -> CallToolResult {
    let document = serde_json::to_string_pretty(envelope).unwrap_or_else(|_| "{}".to_owned());
    let block = vec![ContentBlock::text(document)];
    if envelope.is_success() {
        CallToolResult::success(block)
    } else {
        CallToolResult::error(block)
    }
}

/// Render one local refusal — nothing was dispatched — as a tool result.
fn refused(tool: &str, failure: &Failure) -> CallToolResult {
    let document = serde_json::json!({
        "tool": tool,
        "code": failure.code(),
        "rule": failure.to_string(),
        "dispatched": false,
    });
    CallToolResult::error(vec![ContentBlock::text(
        serde_json::to_string_pretty(&document).unwrap_or_else(|_| "{}".to_owned()),
    )])
}

/// What a caller is told about this server before it calls anything.
///
/// It names the authority, because the most confusing failure mode for a caller is
/// a tool that is missing for a reason it cannot see. It also names the two facts
/// that are easiest to get wrong about this control plane: an acknowledgement is
/// not a completion, and a write needs the revision a read returned.
fn instructions(tier: CallerTier) -> String {
    format!(
        "This server acts on one Kontor realm at {tier} authority; tools above that authority are \
         not served here. Three rules govern every call. First, a write names the revision a read \
         returned: read the aggregate, then write with its revision, and a `revision_conflict` \
         means read it again. Second, recording a command is not the command having happened — the \
         answer is a durable receipt, and the run or task must be read again to see what the \
         runtime reported. Third, every write takes an `idempotency_key` you choose; repeating one \
         returns the original receipt rather than recording a second command, so a retry is safe \
         and is never performed for you. Streamed reads return a bounded batch from one response: \
         to continue, call again with the cursor the last frame carried."
    )
}

/// Serve the MCP protocol over one already-connected byte stream.
///
/// This is the whole server, and the transport is a parameter — which is what lets
/// a test drive the real handshake and the real dispatch over an in-memory pair
/// without binding a socket or spawning a process.
///
/// # Errors
/// Returns the initialization failure when the peer does not complete the MCP
/// handshake, and the service error when the connection ends badly.
pub async fn serve<S>(server: KontorMcp, stream: S) -> Result<(), Box<dyn std::error::Error>>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let running = server.serve(stream).await?;
    running.waiting().await?;
    Ok(())
}

/// Serve the MCP protocol over this process's standard input and output.
///
/// # Errors
/// As [`serve`].
pub async fn serve_stdio(server: KontorMcp) -> Result<(), Box<dyn std::error::Error>> {
    let (mine, theirs) = tokio::io::duplex(BRIDGE_BUFFER);
    let (mut from_protocol, mut to_protocol) = tokio::io::split(mine);
    let (inbound_sender, mut inbound) = tokio::sync::mpsc::channel::<Vec<u8>>(16);
    let (outbound_sender, mut outbound) = tokio::sync::mpsc::channel::<Vec<u8>>(16);

    // Standard input, read on a blocking thread. `tokio`'s async stdin is behind
    // the `io-std` feature this workspace does not enable, and a blocking read on a
    // dedicated thread is what that feature does underneath anyway.
    tokio::task::spawn_blocking(move || {
        use std::io::Read as _;
        let mut stdin = std::io::stdin().lock();
        let mut buffer = [0u8; 8192];
        loop {
            match stdin.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if inbound_sender
                        .blocking_send(buffer[..read].to_vec())
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
    tokio::task::spawn_blocking(move || {
        use std::io::Write as _;
        let mut stdout = std::io::stdout().lock();
        while let Some(chunk) = outbound.blocking_recv() {
            if stdout.write_all(&chunk).is_err() || stdout.flush().is_err() {
                break;
            }
        }
    });

    // Pump the two directions between the channels and the duplex half `rmcp` is
    // not holding.
    tokio::spawn(async move {
        while let Some(chunk) = inbound.recv().await {
            if to_protocol.write_all(&chunk).await.is_err() {
                break;
            }
        }
        // Closing this half is how the protocol learns the client hung up.
        drop(to_protocol);
    });
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt as _;
        let mut buffer = [0u8; 8192];
        loop {
            match from_protocol.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if outbound_sender.send(buffer[..read].to_vec()).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    serve(server, theirs).await
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::capability::Denied;
    use crate::fake::RecordingTransport;
    use crate::registry::{REGISTRY, ServeProfile};

    fn server(tier: CallerTier) -> KontorMcp {
        KontorMcp::new(Dispatcher::new(Box::new(RecordingTransport::new(tier))))
    }

    fn profiled(tier: CallerTier, profile: &str) -> KontorMcp {
        KontorMcp::new(
            Dispatcher::new(Box::new(RecordingTransport::new(tier)))
                .with_profile(ServeProfile::find(profile).expect("a declared profile")),
        )
    }

    /// TEST-001: the worker profile at operator authority serves exactly the
    /// profile's eighteen tools — no more, no fewer.
    #[test]
    fn the_worker_profile_at_operator_serves_exactly_the_profile() {
        let served: BTreeSet<&str> = profiled(CallerTier::Operator, "worker")
            .served()
            .iter()
            .map(|tool| tool.name)
            .collect();
        let declared: BTreeSet<&str> = ServeProfile::find("worker")
            .expect("the worker profile")
            .tools
            .iter()
            .copied()
            .collect();
        assert_eq!(
            served, declared,
            "the served list is exactly the profile ∩ operator, which is the whole profile"
        );
        assert_eq!(served.len(), 18, "worker v2 is eighteen tools");
    }

    /// TEST-002: a tool the tier reaches but the profile excludes is refused at
    /// call time with a distinct error, and nothing is dispatched. A narrowed
    /// list with open calls would be a defect (REQ-003).
    #[tokio::test]
    async fn a_tool_excluded_by_the_profile_is_refused_at_call_time() {
        let recorder = std::sync::Arc::new(RecordingTransport::new(CallerTier::Operator));
        let dispatcher = Dispatcher::new(Box::new(std::sync::Arc::clone(&recorder)))
            .with_profile(ServeProfile::find("worker").expect("the worker profile"));
        let failure = dispatcher
            .call("kontor_topology_ensure", &serde_json::json!({}))
            .await
            .expect_err("an operator tool outside the profile is refused");
        assert!(
            matches!(
                failure,
                Failure::Denied(Denied::ProfileExcluded { profile, .. }) if profile == "worker"
            ),
            "the refusal is the profile's own, not an authority or schema one: {failure}"
        );
        assert_eq!(recorder.count(), 0, "nothing reached the wire");
    }

    /// TEST-003: a profile intersects the tier and never widens it. At observer
    /// authority the worker profile serves only its read tools.
    #[test]
    fn a_profile_never_widens_what_the_tier_allows() {
        let served: BTreeSet<&str> = profiled(CallerTier::Observer, "worker")
            .served()
            .iter()
            .map(|tool| tool.name)
            .collect();
        let observer_reads: BTreeSet<&str> = ServeProfile::find("worker")
            .expect("the worker profile")
            .tools
            .iter()
            .copied()
            .filter(|name| {
                ToolSpec::find(name).expect("a registry tool").tier == CallerTier::Observer
            })
            .collect();
        assert_eq!(
            served, observer_reads,
            "an observer server under the worker profile serves profile ∩ observer only"
        );
        assert_eq!(served.len(), 10, "the worker profile holds ten reads");
        assert!(
            !served.contains("kontor_ticket_claim"),
            "a profile entry above the tier is not served"
        );
    }

    #[test]
    fn a_server_lists_only_what_its_authority_can_perform() {
        let observer = server(CallerTier::Observer);
        assert!(
            observer
                .served()
                .iter()
                .all(|tool| tool.tier == CallerTier::Observer),
            "an observer server must not advertise a tool it would refuse"
        );
        assert!(
            observer.get_tool("kontor_epic_apply").is_none(),
            "a tool above this authority is not declared"
        );
        assert!(observer.get_tool("kontor_realm_get").is_some());

        let admin = server(CallerTier::Admin);
        assert_eq!(
            admin.served().len(),
            REGISTRY.len(),
            "an admin server serves the whole vocabulary"
        );
    }

    #[test]
    fn a_declaration_carries_the_same_schema_the_dispatch_path_enforces() {
        let admin = server(CallerTier::Admin);
        let declared = admin
            .get_tool("kontor_gate_record")
            .expect("the gate tool is declared");
        let spec = ToolSpec::find("kontor_gate_record").expect("the gate spec");
        assert_eq!(
            serde_json::Value::Object((*declared.input_schema).clone()),
            spec.input_schema(),
            "the advertised schema and the enforced one are the same object"
        );
    }

    #[test]
    fn every_declared_schema_is_closed() {
        let admin = server(CallerTier::Admin);
        for tool in admin.served() {
            assert_eq!(
                tool.input_schema().get("additionalProperties"),
                Some(&serde_json::Value::Bool(false)),
                "{} advertises a schema a caller could smuggle a property past",
                tool.name
            );
        }
    }

    #[test]
    fn the_instructions_name_the_authority_and_the_rules_callers_get_wrong() {
        let text = instructions(CallerTier::Operator);
        assert!(text.contains("operator"), "the authority is stated");
        assert!(
            text.contains("revision"),
            "a write needs the revision a read returned"
        );
        assert!(
            text.contains("receipt"),
            "an acknowledgement is not a completion"
        );
        assert!(
            text.contains("idempotency_key"),
            "the caller chooses the key, and this server never invents one"
        );
    }

    #[tokio::test]
    async fn a_refusal_says_that_nothing_was_dispatched() {
        let transport = Box::new(RecordingTransport::new(CallerTier::Observer));
        let dispatcher = Dispatcher::new(transport);
        let failure = dispatcher
            .call("kontor_epic_apply", &serde_json::json!({}))
            .await
            .expect_err("an observer may not apply an epic");
        let rendered = refused("kontor_epic_apply", &failure);
        let text = format!("{rendered:?}");
        assert!(text.contains("forbidden"), "the code is the daemon's own");
        assert!(
            text.contains("\\\"dispatched\\\": false") || text.contains("dispatched"),
            "a caller is told the write was never attempted"
        );
    }
}
