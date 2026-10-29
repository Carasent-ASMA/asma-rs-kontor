//! The headless, read-only Paseo census.
//!
//! # What it reads
//!
//! One gated session connection, its own persistent `clientId`, and four read
//! operations: the pushed server identity, `project.list.request`,
//! `fetch_workspaces_request` with no project filter, and
//! `fetch_agents_request` with an empty label filter and
//! `includeArchived: true`. Nothing here creates, prompts, stops, archives or
//! otherwise mutates a session, and no operation in this module has a mutating
//! shape.
//!
//! # Why the workspace walk is global
//!
//! The governance adapter discovers sessions through configured projects, so
//! a session outside every governed project is invisible to it. Paseo's
//! directory accepts a host-wide workspace page, and each workspace carries
//! its project metadata; the first page also carries projects with no active
//! workspace. The census therefore sees the whole host without borrowing the
//! governance adapter's project bindings or its connection identity.
//!
//! # Completeness
//!
//! The walk follows bounded pages and refuses a repeated or missing cursor
//! instead of returning a truncated list as if it were complete. A runtime
//! whose server identity changes, or whose connection is replaced, during the
//! walk is an incomplete observation: pages read across two sockets do not
//! describe one directory.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use kontor_catalog::native::{
    MAX_NATIVE_PAGES, NATIVE_PAGE_LIMIT, NativeCensus, NativeCensusSource, NativeCheckout,
    NativePageCounts, NativeProject, NativeSession, NativeWorkspace,
};
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};
use secrecy::SecretString;

use crate::client::{PaseoLiveTransport, PaseoRpc, PaseoTransport};
use crate::wire::{
    PASEO_WS_PROTOCOL_VERSION, PaseoAgent, PaseoAgentEntry, PaseoAgentPage, PaseoDirectoryProject,
    PaseoProject, PaseoProjectList, PaseoServerInfo, PaseoWorkspace, PaseoWorkspacePage,
};

/// The settings one census connection needs.
///
/// `host_target` is the complete `--host` argument and is held as a
/// [`SecretString`] so a debug rendering cannot print it. The census itself
/// never runs the CLI; the value exists because the shared transport requires
/// the same construction contract as the governance adapter.
pub struct PaseoCensusConfig {
    /// Stable runtime instance identity.
    pub runtime_instance: String,
    /// The Paseo executable the shared transport would dispatch.
    pub executable: String,
    /// The credential-bearing host target.
    pub host_target: SecretString,
    /// The session WebSocket endpoint.
    pub endpoint: String,
    /// The persistent client id, distinct from the governance adapter's.
    pub client_id: String,
    /// The per-request deadline.
    pub timeout_seconds: u64,
    /// The daemon's maximum page size.
    pub page_limit: u32,
    /// The bounded page walk.
    pub max_pages: usize,
}

impl std::fmt::Debug for PaseoCensusConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PaseoCensusConfig")
            .field("runtime_instance", &self.runtime_instance)
            .field("endpoint", &self.endpoint)
            .field("client_id", &self.client_id)
            .field("host_target", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl PaseoCensusConfig {
    /// Build the qualified defaults for one runtime.
    #[must_use]
    pub fn new(
        runtime_instance: impl Into<String>,
        executable: impl Into<String>,
        host_target: SecretString,
        endpoint: impl Into<String>,
        client_id: impl Into<String>,
        timeout_seconds: u64,
    ) -> Self {
        Self {
            runtime_instance: runtime_instance.into(),
            executable: executable.into(),
            host_target,
            endpoint: endpoint.into(),
            client_id: client_id.into(),
            timeout_seconds,
            page_limit: NATIVE_PAGE_LIMIT,
            max_pages: MAX_NATIVE_PAGES,
        }
    }
}

/// One read-only census over one Paseo runtime instance.
pub struct PaseoCensus {
    runtime_instance: String,
    client_id: String,
    page_limit: u32,
    max_pages: usize,
    transport: Box<dyn PaseoTransport>,
}

impl std::fmt::Debug for PaseoCensus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PaseoCensus")
            .field("runtime_instance", &self.runtime_instance)
            .field("client_id", &self.client_id)
            .field("transport", &self.transport)
            .finish()
    }
}

impl PaseoCensus {
    /// Build a live census with its own connection identity.
    ///
    /// # Errors
    /// The shared transport refuses an empty executable, host, endpoint or
    /// client id.
    pub fn new(config: PaseoCensusConfig) -> RuntimeResult<Self> {
        let transport = PaseoLiveTransport::new(
            &config.executable,
            config.host_target,
            &config.endpoint,
            &config.client_id,
            config.timeout_seconds,
        )?;
        Ok(Self::with_transport(
            config.runtime_instance,
            config.client_id,
            config.page_limit,
            config.max_pages,
            Box::new(transport),
        ))
    }

    /// Build one census around an already composed transport.
    ///
    /// This is the test seam: the recorded Paseo fixture answers the same
    /// correlated frames as the live socket, and the contract suite inspects
    /// the exact read messages the census sent.
    #[must_use]
    pub fn with_transport(
        runtime_instance: impl Into<String>,
        client_id: impl Into<String>,
        page_limit: u32,
        max_pages: usize,
        transport: Box<dyn PaseoTransport>,
    ) -> Self {
        Self {
            runtime_instance: runtime_instance.into(),
            client_id: client_id.into(),
            page_limit: page_limit.clamp(1, NATIVE_PAGE_LIMIT),
            max_pages: max_pages.clamp(1, MAX_NATIVE_PAGES),
            transport,
        }
    }

    /// Observe one census, complete or partial.
    ///
    /// A transport failure is represented as an incomplete census with an
    /// error, so the catalog can record the runtime's coverage honestly rather
    /// than losing the distinction between "not collected" and "empty".
    pub async fn census(&self) -> NativeCensus {
        let mut census = NativeCensus {
            runtime_instance: self.runtime_instance.clone(),
            client_id: self.client_id.clone(),
            server_id: None,
            server_version: None,
            protocol_version: u32::try_from(PASEO_WS_PROTOCOL_VERSION).unwrap_or(1),
            permissions: None,
            features: BTreeMap::new(),
            include_archived: false,
            complete: false,
            error: None,
            diagnostics: Vec::new(),
            pages: NativePageCounts::default(),
            projects: Vec::new(),
            workspaces: Vec::new(),
            sessions: Vec::new(),
        };
        // A census is a fresh inventory. Discard any socket left from the
        // previous attempt: after a sleep or a daemon restart it can look open
        // while every request times out.
        if let Err(error) = self.transport.reset_connection().await {
            census.error = Some(format!(
                "the census connection could not be reset ({error})"
            ));
            return census;
        }
        if let Err(error) = self.walk(&mut census).await {
            census.error = Some(error);
            // The next attempt must start on a fresh gated connection too.
            let _ = self.transport.reset_connection().await;
            return census;
        }
        census.complete = true;
        census.include_archived = true;
        census
    }

    async fn walk(&self, census: &mut NativeCensus) -> Result<(), String> {
        let identity = self
            .transport
            .server_identity()
            .await
            .map_err(|error| format!("the runtime did not announce itself ({error})"))?;
        self.absorb_identity(census, &identity)?;
        let epoch = self.transport.connection_epoch();
        let projects = self
            .read_projects()
            .await
            .map_err(|error| format!("the project directory could not be read ({error})"))?;
        census.pages.project_pages = 1;
        let (workspaces, empty_projects) = self
            .read_workspaces(census)
            .await
            .map_err(|error| format!("the workspace directory could not be read ({error})"))?;
        let entries = self
            .read_sessions(census)
            .await
            .map_err(|error| format!("the session directory could not be read ({error})"))?;

        // One walk must describe one daemon boot. A reconnect in the middle is
        // not a directory; it is two directories with a cursor between them.
        let final_identity = self
            .transport
            .server_identity()
            .await
            .map_err(|error| format!("the runtime identity could not be re-read ({error})"))?;
        if final_identity.server_id != identity.server_id {
            return Err(
                "the runtime server identity changed during the walk; the census was discarded"
                    .to_owned(),
            );
        }
        if self.transport.connection_epoch() != epoch {
            return Err(
                "the runtime connection was replaced during the walk; the census was discarded"
                    .to_owned(),
            );
        }

        let (projects, workspaces, sessions) = assemble(
            census.runtime_instance.as_str(),
            projects,
            empty_projects,
            workspaces,
            entries,
        );
        census.projects = projects;
        census.workspaces = workspaces;
        census.sessions = sessions;
        Ok(())
    }

    fn absorb_identity(
        &self,
        census: &mut NativeCensus,
        identity: &PaseoServerInfo,
    ) -> Result<(), String> {
        if !identity.is_supported_baseline() {
            return Err(format!(
                "the runtime version {:?} is below the supported baseline",
                identity.version
            ));
        }
        if !identity.permits_workspace_reads() {
            return Err(
                "the runtime connection does not grant `workspace.read`; no directory was read"
                    .to_owned(),
            );
        }
        census.server_id = Some(identity.server_id.clone());
        census.server_version = identity.version.clone();
        census.permissions = identity
            .permissions
            .as_ref()
            .map(|permissions| permissions.iter().cloned().collect());
        census.features = identity.features.clone();
        Ok(())
    }

    async fn read_projects(&self) -> RuntimeResult<Vec<PaseoProject>> {
        let request = PaseoRpc::project_list(self.next_request_id("project"));
        let frame = self.transport.request(&request).await?;
        let listed: PaseoProjectList = frame.resolve(&request, "PaseoProjectList")?;
        if listed.error.is_some() {
            return Err(RuntimeError::Transport {
                rule: "the project directory reports a native error",
            });
        }
        Ok(listed.projects)
    }

    async fn read_workspaces(
        &self,
        census: &mut NativeCensus,
    ) -> RuntimeResult<(Vec<PaseoWorkspace>, Vec<PaseoProject>)> {
        let mut workspaces = Vec::new();
        let mut empty_projects = Vec::new();
        let mut cursor: Option<String> = None;
        let mut cursors = BTreeSet::new();
        for _ in 0..self.max_pages {
            let request = PaseoRpc::workspace_census_page(
                self.next_request_id("workspace"),
                self.page_limit,
                cursor.as_deref(),
            );
            let frame = self.transport.request(&request).await?;
            let page: PaseoWorkspacePage = frame.resolve(&request, "PaseoWorkspacePage")?;
            census.pages.workspace_pages += 1;
            if page.error.is_some() {
                return Err(RuntimeError::Transport {
                    rule: "the workspace directory reports a native error",
                });
            }
            let next = if page.page_info.has_more {
                let Some(next) = page.page_info.next() else {
                    return Err(RuntimeError::Transport {
                        rule: "the workspace directory omitted its required continuation cursor",
                    });
                };
                if !cursors.insert(next.to_owned()) {
                    return Err(RuntimeError::Transport {
                        rule: "the workspace directory repeated a continuation cursor",
                    });
                }
                Some(next.to_owned())
            } else {
                None
            };
            empty_projects.extend(page.empty_projects);
            workspaces.extend(page.entries);
            let Some(next) = next else {
                return Ok((workspaces, empty_projects));
            };
            cursor = Some(next);
        }
        Err(RuntimeError::Transport {
            rule: "the workspace directory did not end within the bounded page walk",
        })
    }

    async fn read_sessions(
        &self,
        census: &mut NativeCensus,
    ) -> RuntimeResult<Vec<PaseoAgentEntry>> {
        let mut entries = Vec::new();
        let mut cursor: Option<String> = None;
        let mut cursors = BTreeSet::new();
        let labels = BTreeMap::new();
        for _ in 0..self.max_pages {
            let request = PaseoRpc::agent_list(
                self.next_request_id("session"),
                &labels,
                true,
                self.page_limit,
                cursor.as_deref(),
            );
            let frame = self.transport.request(&request).await?;
            let page: PaseoAgentPage = frame.resolve(&request, "PaseoAgentPage")?;
            census.pages.session_pages += 1;
            if page.error.is_some() {
                return Err(RuntimeError::Transport {
                    rule: "the session directory reports a native error",
                });
            }
            let next = if page.page_info.has_more {
                let Some(next) = page.page_info.next() else {
                    return Err(RuntimeError::Transport {
                        rule: "the session directory omitted its required continuation cursor",
                    });
                };
                if !cursors.insert(next.to_owned()) {
                    return Err(RuntimeError::Transport {
                        rule: "the session directory repeated a continuation cursor",
                    });
                }
                Some(next.to_owned())
            } else {
                None
            };
            entries.extend(page.entries);
            let Some(next) = next else {
                return Ok(entries);
            };
            cursor = Some(next);
        }
        Err(RuntimeError::Transport {
            rule: "the session directory did not end within the bounded page walk",
        })
    }

    fn next_request_id(&self, kind: &str) -> String {
        format!("census-{kind}-{}", uuid::Uuid::now_v7())
    }
}

#[async_trait]
impl NativeCensusSource for PaseoCensus {
    fn runtime_instance(&self) -> &str {
        &self.runtime_instance
    }

    fn client_id(&self) -> &str {
        &self.client_id
    }

    async fn observe(&self) -> NativeCensus {
        self.census().await
    }
}

/// Assemble the census rows, keeping identity keyed by native id.
fn assemble(
    runtime_instance: &str,
    projects: Vec<PaseoProject>,
    empty_projects: Vec<PaseoProject>,
    workspaces: Vec<PaseoWorkspace>,
    entries: Vec<PaseoAgentEntry>,
) -> (Vec<NativeProject>, Vec<NativeWorkspace>, Vec<NativeSession>) {
    let mut project_map: BTreeMap<String, NativeProject> = BTreeMap::new();
    let mut workspace_map: BTreeMap<String, NativeWorkspace> = BTreeMap::new();
    let mut session_map: BTreeMap<String, NativeSession> = BTreeMap::new();

    for project in projects.into_iter().chain(empty_projects) {
        observe_project(&mut project_map, runtime_instance, &project);
    }
    for workspace in workspaces {
        observe_project_from_workspace(&mut project_map, runtime_instance, &workspace);
        workspace_map.insert(
            workspace.id.clone(),
            NativeWorkspace {
                runtime_instance: runtime_instance.to_owned(),
                workspace_id: workspace.id,
                project_id: workspace.project_id,
                name: workspace.name,
                title: workspace.title,
                directory: workspace.workspace_directory,
                kind: workspace_kind(&workspace.workspace_kind),
                archiving_at: workspace.archiving_at,
                worktree_slug: workspace.worktree_slug,
                project_display_name: workspace.project_display_name,
                project_custom_name: workspace.project_custom_name,
                project_root_path: workspace.project_root_path,
                project_kind: workspace.project_kind,
            },
        );
    }

    let workspace_projects: BTreeMap<String, String> = workspace_map
        .values()
        .map(|workspace| (workspace.workspace_id.clone(), workspace.project_id.clone()))
        .collect();
    for entry in entries {
        let PaseoAgentEntry { agent, project } = entry;
        let session = session_from_entry(
            runtime_instance,
            &workspace_projects,
            &mut project_map,
            &agent,
            project.as_ref(),
        );
        // Identity is the native id. A duplicate page row from a session that
        // changed mid-walk replaces display state but never creates a second
        // identity.
        session_map.insert(session.session_id.clone(), session);
    }

    let mut projects: Vec<NativeProject> = project_map.into_values().collect();
    projects.sort_by(|left, right| left.project_id.cmp(&right.project_id));
    let mut workspaces: Vec<NativeWorkspace> = workspace_map.into_values().collect();
    workspaces.sort_by(|left, right| {
        left.project_id
            .cmp(&right.project_id)
            .then_with(|| left.workspace_id.cmp(&right.workspace_id))
    });
    let mut sessions: Vec<NativeSession> = session_map.into_values().collect();
    sessions.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    (projects, workspaces, sessions)
}

fn observe_project(
    projects: &mut BTreeMap<String, NativeProject>,
    runtime_instance: &str,
    project: &PaseoProject,
) {
    projects.insert(
        project.id.clone(),
        NativeProject {
            runtime_instance: runtime_instance.to_owned(),
            project_id: project.id.clone(),
            display_name: project.display_name.clone(),
            custom_name: project.custom_name.clone(),
            project_key: project.project_key.clone(),
            root_path: project.root_path.clone(),
            project_kind: project.project_kind.clone(),
        },
    );
}

fn observe_project_from_workspace(
    projects: &mut BTreeMap<String, NativeProject>,
    runtime_instance: &str,
    workspace: &PaseoWorkspace,
) {
    let Some(display_name) = workspace.project_display_name.as_deref() else {
        return;
    };
    let entry = projects
        .entry(workspace.project_id.clone())
        .or_insert_with(|| NativeProject {
            runtime_instance: runtime_instance.to_owned(),
            project_id: workspace.project_id.clone(),
            display_name: display_name.to_owned(),
            custom_name: None,
            project_key: None,
            root_path: workspace.project_root_path.clone().unwrap_or_default(),
            project_kind: None,
        });
    if entry.display_name.is_empty() {
        entry.display_name = display_name.to_owned();
    }
    if entry.root_path.is_empty()
        && let Some(root_path) = &workspace.project_root_path
    {
        entry.root_path = root_path.clone();
    }
    if entry.custom_name.is_none() {
        entry.custom_name = workspace.project_custom_name.clone();
    }
    if entry.project_kind.is_none() {
        entry.project_kind = workspace.project_kind.clone();
    }
}

fn session_from_entry(
    runtime_instance: &str,
    workspace_projects: &BTreeMap<String, String>,
    projects: &mut BTreeMap<String, NativeProject>,
    agent: &PaseoAgent,
    placement: Option<&PaseoDirectoryProject>,
) -> NativeSession {
    let workspace_id = agent.workspace_id.clone();
    let project_id = workspace_id
        .as_deref()
        .and_then(|id| workspace_projects.get(id).cloned())
        .or_else(|| placement.map(|project| project.project_key.clone()));
    let project_name = placement.map(|project| project.project_name.clone());
    let workspace_name = placement.and_then(|project| project.workspace_name.clone());
    if let (Some(project_id), Some(project_name)) = (&project_id, &project_name) {
        let entry = projects
            .entry(project_id.clone())
            .or_insert_with(|| NativeProject {
                runtime_instance: runtime_instance.to_owned(),
                project_id: project_id.clone(),
                display_name: project_name.clone(),
                custom_name: None,
                project_key: Some(project_id.clone()),
                root_path: String::new(),
                project_kind: None,
            });
        if entry.display_name.is_empty() {
            entry.display_name = project_name.clone();
        }
    }
    let checkout = placement
        .and_then(|project| project.checkout.as_ref())
        .map(|checkout| NativeCheckout {
            cwd: checkout.cwd.clone(),
            branch: checkout.current_branch.clone(),
            worktree_root: checkout.worktree_root.clone(),
            is_git: checkout.is_git,
            is_paseo_owned_worktree: checkout.is_paseo_owned_worktree,
        });
    NativeSession {
        runtime_instance: runtime_instance.to_owned(),
        session_id: agent.id.clone(),
        workspace_id,
        project_id,
        project_name,
        workspace_name,
        title: agent.title.clone(),
        provider: agent.provider.clone(),
        model: (!agent.model.is_empty()).then(|| agent.model.clone()),
        status: agent_status(agent),
        archived_at: agent.archived_at.clone(),
        attention_reason: agent.attention_reason.clone(),
        cwd: agent.cwd.clone(),
        created_at: agent.created_at.clone(),
        updated_at: agent.updated_at.clone(),
        last_user_message_at: agent.last_user_message_at.clone(),
        parent_session_id: agent.parent_agent_id().map(str::to_owned),
        checkout,
    }
}

fn workspace_kind(kind: &crate::wire::PaseoWorkspaceKind) -> String {
    match kind {
        crate::wire::PaseoWorkspaceKind::Worktree => "worktree",
        crate::wire::PaseoWorkspaceKind::Checkout => "checkout",
        crate::wire::PaseoWorkspaceKind::LocalCheckout => "local_checkout",
        crate::wire::PaseoWorkspaceKind::Directory => "directory",
        crate::wire::PaseoWorkspaceKind::Other => "other",
    }
    .to_owned()
}

fn agent_status(agent: &PaseoAgent) -> String {
    match agent.status {
        crate::wire::PaseoAgentStatus::Initializing => "initializing",
        crate::wire::PaseoAgentStatus::Idle => "idle",
        crate::wire::PaseoAgentStatus::Running => "running",
        crate::wire::PaseoAgentStatus::Error => "error",
        crate::wire::PaseoAgentStatus::Closed => "closed",
        crate::wire::PaseoAgentStatus::Unknown => "unknown",
    }
    .to_owned()
}
