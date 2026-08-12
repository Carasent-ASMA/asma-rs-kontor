//! The MCP server: one Realm, one authority, one tool list.
//!
//! # One authority for the whole process
//!
//! A [`KontorMcp`] is built with exactly one [`CallerTier`] and holds exactly one
//! credential for it. There is no per-call authority, no escalation operand and no
//! way for a tool to ask for a different secret — a tool is handed a client that
//! already is what it is. Running two authorities means running two servers, which
//! is the honest way to say it: the credential a process holds is a fact about the
//! process.
//!
//! [`KontorMcp::list_tools`] therefore lists only what this server can actually
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
//! process (TST-001): a test drives [`serve`] over the *other* half of a duplex
//! pair, which is the same code path the binary takes minus the two pumps.

use std::borrow::Cow;
use std::future::Future;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData,
    Implementation, InitializeResult, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
    Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ServerHandler, ServiceExt};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

use crate::capability::Gate;
use crate::client::{CallerTier, RealmClient};
use crate::tools::ToolSpec;
use crate::{Failure, execute};

/// How many bytes the stdio bridge buffers in each direction.
///
/// One MCP frame is a single JSON line; 256 KiB is far more than a tool result of
/// this contract produces, and the duplex applies backpressure rather than
/// truncating if one ever exceeds it.
const BRIDGE_BUFFER: usize = 256 * 1024;

/// The capability-gated tool server for one Realm.
pub struct KontorMcp {
    client: Arc<RealmClient>,
    gate: Gate,
}

impl std::fmt::Debug for KontorMcp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KontorMcp")
            .field("authority", &self.gate.configured())
            .finish_non_exhaustive()
    }
}

impl KontorMcp {
    /// A server that acts at exactly `tier` and no higher.
    ///
    /// The tier comes from the client, not from a separate argument: they cannot
    /// disagree, because there is only one of them.
    #[must_use]
    pub fn new(client: RealmClient) -> Self {
        let gate = Gate::new(client.tier());
        Self {
            client: Arc::new(client),
            gate,
        }
    }

    /// The authority this server was configured with.
    #[must_use]
    pub const fn authority(&self) -> CallerTier {
        self.gate.configured()
    }

    /// Every operation this server can perform, in name order.
    #[must_use]
    pub fn served(&self) -> Vec<ToolSpec> {
        crate::tools::catalogue()
            .into_iter()
            .filter(|tool| self.gate.configured().at_least(tool.tier))
            .collect()
    }

    /// The MCP declaration of one operation.
    ///
    /// Built through `Tool::new` rather than as a struct literal: the model types
    /// are `#[non_exhaustive]`, and going through the constructor means a later rmcp
    /// generation that adds a field does not silently leave it at whatever this
    /// build assumed.
    fn declare(tool: &ToolSpec) -> Tool {
        Tool::new(
            Cow::Borrowed(tool.name),
            Cow::Borrowed(tool.description),
            Arc::new(tool.input_schema()),
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
            self.gate.configured()
        ));
        let mut info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build());
        info.server_info = implementation;
        info.instructions = Some(instructions(self.gate.configured()));
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
        let client = Arc::clone(&self.client);
        let gate = self.gate;
        async move {
            let arguments = request.arguments.unwrap_or_default();
            match execute(&client, gate, &request.name, &arguments).await {
                // A tool result rather than a protocol error: a refusal is an
                // *answer* about this realm, and a caller that can read the code
                // and the rule can decide what to do. A JSON-RPC error would strip
                // both down to a message.
                Err(failure) => Ok(refusal(&failure, client.expected_realm().as_deref()).into()),
                Ok(envelope) => Ok(answer(&envelope).into()),
            }
        }
    }
}

/// Render one successful answer as a tool result.
fn answer(envelope: &crate::Envelope) -> CallToolResult {
    let document = serde_json::to_string_pretty(envelope).unwrap_or_else(|_| "{}".to_owned());
    CallToolResult::success(vec![ContentBlock::text(document)])
}

/// Render one refusal as a tool result, carrying the document unchanged.
fn refusal(failure: &Failure, realm_id: Option<&str>) -> CallToolResult {
    let document =
        serde_json::to_string_pretty(&failure.body(realm_id)).unwrap_or_else(|_| "{}".to_owned());
    CallToolResult::error(vec![ContentBlock::text(document)])
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
         not served here. Two rules govern every call. First, a write names the revision a read \
         returned: read the aggregate, then write with its revision, and a `revision_conflict` \
         means read it again. Second, recording a command is not the command having happened — the \
         answer is a durable receipt, and the run or task must be read again to see what the \
         runtime reported. Pass `dry_run` to any write to see the request it would make without \
         making it, and repeat an `idempotency_key` to replay a receipt instead of recording a \
         second command."
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
///
/// # Panics
/// Never. The pumps end when their stream ends, and a send on a closed channel is
/// the ordinary way a client disconnects.
pub async fn serve_stdio(server: KontorMcp) -> Result<(), Box<dyn std::error::Error>> {
    let (mine, theirs) = tokio::io::duplex(BRIDGE_BUFFER);
    let (mut from_protocol, mut to_protocol) = tokio::io::split(mine);
    let (inbound_sender, mut inbound) = tokio::sync::mpsc::channel::<Vec<u8>>(16);
    let (outbound_sender, mut outbound) = tokio::sync::mpsc::channel::<Vec<u8>>(16);

    // Standard input, read on a blocking thread. `tokio`'s async stdin is behind
    // the `io-std` feature this workspace does not enable, and a blocking read on a
    // dedicated thread is what that feature does underneath anyway.
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
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
        use std::io::Write;
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
        use tokio::io::AsyncReadExt;
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
    use super::*;

    /// A server over a recording fake, for the checks that only read declarations.
    fn server(tier: CallerTier) -> KontorMcp {
        KontorMcp::new(RealmClient::new(Box::new(crate::fake::FakeTransport::new(
            tier,
        ))))
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
            observer.get_tool("run_launch").is_none(),
            "a tool above this authority is not declared"
        );

        let admin = server(CallerTier::Admin);
        assert_eq!(
            admin.served().len(),
            crate::tools::catalogue().len(),
            "an admin server serves the whole catalogue"
        );
        assert!(admin.get_tool("account_list").is_some());
    }

    #[test]
    fn a_declaration_carries_the_schema_the_validator_uses() {
        let admin = server(CallerTier::Admin);
        let declared = admin
            .get_tool("gate_verdict")
            .expect("the gate_verdict tool is declared");
        let spec = crate::tools::find("gate_verdict").expect("the gate_verdict spec");
        assert_eq!(
            *declared.input_schema,
            spec.input_schema(),
            "the advertised schema and the enforced one are the same object"
        );
    }

    #[test]
    fn the_instructions_name_the_authority_and_the_two_rules_callers_get_wrong() {
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
    }
}
