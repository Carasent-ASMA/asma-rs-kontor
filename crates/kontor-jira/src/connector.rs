use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use kontor_accounts::{KeychainBackend, KeychainTarget, SystemKeychain};
use kontor_core::id::{
    BoundedText, CanonicalDocument, ContentHash, ExternalId, ExternalName, ProjectId, Timestamp,
};
use kontor_core::ticket::{ObservedBody, OwnershipAction};
use reqwest::{Client, Method, StatusCode, Url};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::jira::{
    FieldWrite, JiraExchange, JiraOperation, JiraOutcome, JiraRequest, JiraResponse,
    WireAssignment, WireConfirmation, WireEffects, WireFieldValue, WireObservation, WireTransition,
};
use crate::{JiraError, MaterializationConflict, UnavailableReason, WireTimestamp};

const CONFIG_SCHEMA: u32 = 1;
const CONFIG_FILE: &str = "jira.json";
const KEYCHAIN_SERVICE: &str = "kontor-jira";
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CREDENTIAL_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CREATE_FIELDS: usize = 64;
const MAX_CREATE_FIELDS_BYTES: usize = 64 * 1024;
const RESERVED_CREATE_FIELDS: [&str; 6] = [
    "project",
    "issuetype",
    "summary",
    "description",
    "labels",
    "parent",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JiraConfig {
    pub schema_version: u32,
    pub projects: Vec<JiraProjectConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JiraProjectConfig {
    pub project_id: ProjectId,
    pub endpoint: String,
    pub project_key: ExternalId,
    pub credential_alias: String,
    #[serde(default)]
    pub create_fields: JiraCreateFields,
}

/// Operator-owned Jira fields required when Kontor creates an issue.
///
/// Kontor still owns the structural fields in [`RESERVED_CREATE_FIELDS`]. A
/// project may configure only additional fields required by its Jira screens,
/// independently for epic and task creation.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JiraCreateFields {
    #[serde(default)]
    pub epic: BTreeMap<String, Value>,
    #[serde(default)]
    pub task: BTreeMap<String, Value>,
}

/// The configured connector set, keyed only by Kontor project identity.
#[derive(Clone, Default)]
pub struct JiraConnectors {
    projects: BTreeMap<ProjectId, JiraConnector>,
}

impl std::fmt::Debug for JiraConnectors {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JiraConnectors")
            .field("project_ids", &self.projects.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl JiraConnectors {
    /// Read strict operator configuration. A missing file means Jira is not
    /// configured; a malformed file refuses daemon startup.
    pub fn read(state_root: &Path) -> Result<Self, JiraError> {
        Self::read_with_keychain(state_root, Arc::new(SystemKeychain))
    }

    pub fn read_with_keychain(
        state_root: &Path,
        keychain: Arc<dyn KeychainBackend>,
    ) -> Result<Self, JiraError> {
        let path = state_root.join(CONFIG_FILE);
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(_) => {
                return Err(JiraError::unavailable(
                    "configuration",
                    UnavailableReason::Configuration,
                    "jira.json could not be read",
                ));
            }
        };
        let config: JiraConfig = serde_json::from_slice(&bytes).map_err(|_| {
            JiraError::unavailable(
                "configuration",
                UnavailableReason::Configuration,
                "jira.json is not a strict supported document",
            )
        })?;
        if config.schema_version != CONFIG_SCHEMA {
            return Err(JiraError::unavailable(
                "configuration",
                UnavailableReason::Configuration,
                "jira.json declares an unsupported schema version",
            ));
        }
        let mut projects = BTreeMap::new();
        for project in config.projects {
            let id = project.project_id;
            let connector = JiraConnector::new(project, Arc::clone(&keychain))?;
            if projects.insert(id, connector).is_some() {
                return Err(JiraError::unavailable(
                    "configuration",
                    UnavailableReason::Configuration,
                    "jira.json configures the same project more than once",
                ));
            }
        }
        Ok(Self { projects })
    }

    #[must_use]
    pub fn for_project(&self, project_id: ProjectId) -> Option<&JiraConnector> {
        self.projects.get(&project_id)
    }
}

#[derive(Clone)]
pub struct JiraConnector {
    endpoint: Url,
    project_key: ExternalId,
    credential_alias: String,
    create_fields: JiraCreateFields,
    keychain: Arc<dyn KeychainBackend>,
    client: Client,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JiraIssueKind {
    Epic,
    Task,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraIssuePlan {
    pub kind: JiraIssueKind,
    pub requested_key: Option<ExternalId>,
    pub marker: ExternalId,
    /// Whether an explicit link must prove the original create marker and
    /// exact Kontor-authored content. Ordinary operator links do not claim
    /// authority over summary or description; in-place recovery does.
    pub require_marker: bool,
    pub summary: String,
    pub description: String,
    pub parent_key: Option<ExternalId>,
}

/// One issue's identity as Jira currently reports it.
///
/// The key is whatever Jira answers with *now*, which is not necessarily the
/// key that was asked for; the id is the same for the life of the issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraIssueIdentity {
    /// The canonical key Jira reports today.
    pub issue_key: ExternalId,
    /// The immutable REST issue id.
    pub issue_id: ExternalId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraIssueReadback {
    pub issue_key: ExternalId,
    /// Jira's immutable REST top-level issue id, observed in the same readback
    /// that proved the key.
    ///
    /// The canonical key moves with the issue whenever it changes project; this
    /// does not. It is the only observed value that tells a key change on one
    /// issue apart from a rebind onto a different one, which the readback hash
    /// cannot do because the key is itself part of the hashed document.
    pub issue_id: ExternalId,
    pub readback_hash: ContentHash,
}

impl std::fmt::Debug for JiraConnector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JiraConnector")
            .field("origin", &self.endpoint.origin().ascii_serialization())
            .field("project_key", &self.project_key)
            .field("credential", &"<redacted>")
            .finish()
    }
}

impl JiraConnector {
    fn new(
        config: JiraProjectConfig,
        keychain: Arc<dyn KeychainBackend>,
    ) -> Result<Self, JiraError> {
        if config.credential_alias.trim().is_empty() || config.credential_alias.len() > 128 {
            return Err(configuration("credential_alias is empty or oversized"));
        }
        let mut endpoint = Url::parse(&config.endpoint)
            .map_err(|_| configuration("endpoint is not an absolute URL"))?;
        let loopback = endpoint
            .host_str()
            .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1"));
        if endpoint.scheme() != "https" && !(endpoint.scheme() == "http" && loopback) {
            return Err(configuration(
                "endpoint must use HTTPS (or explicit loopback HTTP)",
            ));
        }
        if !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(configuration(
                "endpoint may not contain credentials, query, or fragment",
            ));
        }
        validate_create_fields(&config.create_fields)?;
        endpoint.set_path("/");
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|_| configuration("the Jira HTTP client could not be built"))?;
        Ok(Self {
            endpoint,
            project_key: config.project_key,
            credential_alias: config.credential_alias,
            create_fields: config.create_fields,
            keychain,
            client,
        })
    }

    async fn credentials(&self) -> Result<JiraCredentials, JiraError> {
        let keychain = Arc::clone(&self.keychain);
        let alias = self.credential_alias.clone();
        let secret = tokio::time::timeout(
            CREDENTIAL_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let target = KeychainTarget::new(KEYCHAIN_SERVICE, alias);
                keychain.secret(&target)
            }),
        )
        .await
        .map_err(|_| {
            JiraError::unavailable(
                "credential",
                UnavailableReason::Credential,
                "the keychain credential read exceeded its bound",
            )
        })?
        .map_err(|_| {
            JiraError::unavailable(
                "credential",
                UnavailableReason::Credential,
                "the keychain credential read was cancelled",
            )
        })?
        .map_err(|_| {
            JiraError::unavailable(
                "credential",
                UnavailableReason::Credential,
                "the configured keychain credential could not be resolved",
            )
        })?;
        serde_json::from_str(secret.expose_secret()).map_err(|_| {
            JiraError::unavailable(
                "credential",
                UnavailableReason::Credential,
                "the keychain credential is not the supported document",
            )
        })
    }

    fn url(&self, path: &str) -> Result<Url, JiraError> {
        let url = self.endpoint.join(path).map_err(|_| {
            JiraError::unavailable(
                "transport",
                UnavailableReason::Configuration,
                "a Jira request path could not be formed",
            )
        })?;
        if url.origin() != self.endpoint.origin() {
            return Err(JiraError::refused(
                "transport",
                "a Jira request may not leave the configured origin",
            ));
        }
        Ok(url)
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, JiraError> {
        let credentials = self.credentials().await?;
        let mut request = self
            .client
            .request(method, self.url(path)?)
            .basic_auth(credentials.email, Some(credentials.api_token))
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .map_err(|_| transport("request failed"))?;
        if response.status().is_redirection() {
            return Err(JiraError::refused(
                "transport",
                "Jira redirects are not followed",
            ));
        }
        if response.status() == StatusCode::NO_CONTENT {
            return Ok(Value::Null);
        }
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(oversized());
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| transport("response body failed"))?;
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(oversized());
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(non_success(status, &bytes));
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|_| {
            JiraError::unavailable(
                "transport",
                UnavailableReason::MalformedResponse,
                "Jira returned a malformed JSON response",
            )
        })
    }

    async fn live(&self, issue_key: &ExternalId) -> Result<LiveIssue, JiraError> {
        let encoded =
            url::form_urlencoded::byte_serialize(issue_key.as_str().as_bytes()).collect::<String>();
        let issue = self
            .request(
                Method::GET,
                &format!("rest/api/3/issue/{encoded}?fields=*all"),
                None,
            )
            .await?;
        let project = text_at(&issue, &["fields", "project", "key"])?;
        if project != self.project_key.as_str() {
            return Err(JiraError::refused(
                "observe",
                "the issue belongs to another configured Jira project",
            ));
        }
        let transitions = self
            .request(
                Method::GET,
                &format!("rest/api/3/issue/{encoded}/transitions?expand=transitions.fields"),
                None,
            )
            .await?;
        let principal = self.request(Method::GET, "rest/api/3/myself", None).await?;
        let status_id = external_at(&issue, &["fields", "status", "id"])?;
        let status_name = name_at(&issue, &["fields", "status", "name"])?;
        let status_category = name_at(&issue, &["fields", "status", "statusCategory", "name"])?;
        let issue_type = name_at(&issue, &["fields", "issuetype", "name"])?;
        let assignee_account_id =
            optional_external_at(&issue, &["fields", "assignee", "accountId"])?;
        let assignee_display = optional_name_at(&issue, &["fields", "assignee", "displayName"])?;
        let update_token = optional_external_at(&issue, &["fields", "updated"])?;
        let observation_hash = CanonicalDocument::from_serializable(&json!({
            "schema_version": 1,
            "key": issue_key.as_str(),
            "project": project,
            "fields": issue.get("fields").cloned().unwrap_or(Value::Null),
        }))?
        .hash()
        .clone();
        let description = observed_body(issue.pointer("/fields/description"))?;
        let observation = WireObservation {
            status_id,
            status_name,
            status_category,
            issue_type,
            assignee_account_id,
            assignee_display,
            update_token,
            description,
            observation_hash,
        };
        let live_transitions = transitions
            .get("transitions")
            .and_then(Value::as_array)
            .ok_or_else(malformed)?
            .iter()
            .map(|transition| {
                Ok(WireTransition {
                    transition_id: external_at(transition, &["id"])?,
                    to_status_id: external_at(transition, &["to", "id"])?,
                    to_status_name: name_at(transition, &["to", "name"])?,
                    to_status_category: optional_name_at(
                        transition,
                        &["to", "statusCategory", "name"],
                    )?,
                })
            })
            .collect::<Result<Vec<_>, JiraError>>()?;
        let principal_account_id = Some(external_at(&principal, &["accountId"])?);
        let fields = issue
            .get("fields")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(malformed)?;
        Ok(LiveIssue {
            observation,
            live_transitions,
            principal_account_id,
            fields,
        })
    }

    fn validate_expected(request: &JiraRequest, live: &LiveIssue) -> Result<(), JiraError> {
        let Some(expected) = request.expected.as_ref() else {
            return Ok(());
        };
        if expected.status_id != live.observation.status_id
            || expected.assignee_account_id != live.observation.assignee_account_id
            || expected.update_token != live.observation.update_token
            || expected
                .observation_hash
                .as_ref()
                .is_some_and(|hash| hash != &live.observation.observation_hash)
        {
            return Err(JiraError::Conflict {
                operation: "apply",
                kind: kontor_core::ticket::StatusConflictKind::IncompatibleHumanMove,
            });
        }
        Ok(())
    }

    async fn apply_effects(
        &self,
        request: &JiraRequest,
        before: &LiveIssue,
    ) -> Result<WireEffects, JiraError> {
        let encoded = url::form_urlencoded::byte_serialize(request.issue_key.as_str().as_bytes())
            .collect::<String>();
        let mut fields = Map::new();
        for write in &request.field_writes {
            fields.insert(write.field_id.as_str().to_owned(), encode_field(write));
        }
        if !fields.is_empty() {
            self.request(
                Method::PUT,
                &format!("rest/api/3/issue/{encoded}"),
                Some(&json!({"fields": fields})),
            )
            .await?;
        }
        let assignment = match request.ownership_action {
            OwnershipAction::ReassignToPrincipal => {
                let account_id = before.principal_account_id.clone().ok_or_else(malformed)?;
                self.request(
                    Method::PUT,
                    &format!("rest/api/3/issue/{encoded}/assignee"),
                    Some(&json!({"accountId": account_id.as_str()})),
                )
                .await?;
                Some(WireAssignment {
                    action: OwnershipAction::ReassignToPrincipal,
                    account_id: Some(account_id),
                })
            }
            OwnershipAction::Preserve => None,
            _ => {
                return Err(JiraError::refused(
                    "apply",
                    "the native connector does not clear or invent an assignee",
                ));
            }
        };
        if let Some(transition) = request.transition.as_ref() {
            let destination = request.destination.as_ref().ok_or_else(|| {
                JiraError::refused("apply", "a transition requires its exact destination")
            })?;
            let offered = before.live_transitions.iter().any(|candidate| {
                candidate.transition_id == transition.transition_id
                    && candidate.to_status_id == destination.status_id
                    && candidate.to_status_name == destination.status_name
                    && transition.to_status_id == destination.status_id
            });
            if !offered {
                return Err(JiraError::Conflict {
                    operation: "apply",
                    kind: kontor_core::ticket::StatusConflictKind::IncompatibleHumanMove,
                });
            }
            self.request(
                Method::POST,
                &format!("rest/api/3/issue/{encoded}/transitions"),
                Some(&json!({"transition": {"id": transition.transition_id.as_str()}})),
            )
            .await?;
        }
        Ok(WireEffects {
            field_ids: request
                .field_writes
                .iter()
                .map(|write| write.field_id.clone())
                .collect(),
            assignment,
            transition: request.transition.clone(),
        })
    }

    fn confirm(
        request: &JiraRequest,
        before: &LiveIssue,
        after: &LiveIssue,
    ) -> Result<(), JiraError> {
        for write in &request.field_writes {
            if !field_matches(write, after.fields.get(write.field_id.as_str())) {
                return Err(transport("Jira readback did not confirm an owned field"));
            }
        }
        if request.ownership_action == OwnershipAction::ReassignToPrincipal
            && after.observation.assignee_account_id != before.principal_account_id
        {
            return Err(transport("Jira readback did not confirm the assignee"));
        }
        if request.transition.is_some() {
            let destination = request.destination.as_ref().ok_or_else(|| {
                JiraError::refused("apply", "a transition requires its exact destination")
            })?;
            if after.observation.status_id != destination.status_id
                || after.observation.status_name != destination.status_name
            {
                return Err(transport("Jira readback did not confirm the transition"));
            }
        }
        Ok(())
    }

    /// Read all inbound Jira comments. Pagination is resolved inside the
    /// connector so a caller cannot accidentally report a partial mirror.
    pub async fn comments(&self, issue_key: &ExternalId) -> Result<Vec<JiraComment>, JiraError> {
        let encoded =
            url::form_urlencoded::byte_serialize(issue_key.as_str().as_bytes()).collect::<String>();
        let mut start = 0_u64;
        let mut comments = Vec::new();
        loop {
            let page = self
                .request(
                    Method::GET,
                    &format!(
                        "rest/api/3/issue/{encoded}/comment?startAt={start}&maxResults=100&orderBy=created"
                    ),
                    None,
                )
                .await?;
            let values = page
                .get("comments")
                .and_then(Value::as_array)
                .ok_or_else(malformed)?;
            for value in values {
                comments.push(JiraComment::from_value(value)?);
            }
            let total = page
                .get("total")
                .and_then(Value::as_u64)
                .ok_or_else(malformed)?;
            start = start.saturating_add(values.len() as u64);
            if start >= total || values.is_empty() {
                break;
            }
        }
        Ok(comments)
    }

    /// Link or create one issue, then accept it only after exact readback.
    pub async fn materialize(&self, plan: &JiraIssuePlan) -> Result<JiraIssueReadback, JiraError> {
        let key = if let Some(key) = plan.requested_key.clone() {
            key
        } else {
            let matches = self.find_marker(&plan.marker).await?;
            match matches.as_slice() {
                [] => self.create_issue(plan).await?,
                [key] => key.clone(),
                _ => {
                    return Err(JiraError::MaterializationConflict {
                        kind: MaterializationConflict::AmbiguousMarker,
                    });
                }
            }
        };
        self.readback_issue(&key, plan).await
    }

    async fn find_marker(&self, marker: &ExternalId) -> Result<Vec<ExternalId>, JiraError> {
        let jql = format!(
            "project = {} AND labels = \"{}\"",
            self.project_key.as_str(),
            marker.as_str()
        );
        let encoded = url::form_urlencoded::byte_serialize(jql.as_bytes()).collect::<String>();
        let value = self
            .request(
                Method::GET,
                &format!("rest/api/3/search/jql?jql={encoded}&fields=key&maxResults=3"),
                None,
            )
            .await?;
        value
            .get("issues")
            .and_then(Value::as_array)
            .ok_or_else(malformed)?
            .iter()
            .map(|issue| external_at(issue, &["key"]))
            .collect()
    }

    async fn create_issue(&self, plan: &JiraIssuePlan) -> Result<ExternalId, JiraError> {
        let issue_type = self.issue_type(plan.kind).await?;
        let configured = match plan.kind {
            JiraIssueKind::Epic => &self.create_fields.epic,
            JiraIssueKind::Task => &self.create_fields.task,
        };
        let mut fields = Map::from_iter(
            configured
                .iter()
                .map(|(field, value)| (field.clone(), value.clone())),
        );
        fields.extend([
            (
                "project".to_owned(),
                json!({"key": self.project_key.as_str()}),
            ),
            ("issuetype".to_owned(), json!({"id": issue_type.as_str()})),
            ("summary".to_owned(), json!(plan.summary)),
            ("description".to_owned(), adf(&plan.description)),
            ("labels".to_owned(), json!([plan.marker.as_str()])),
        ]);
        if let Some(parent) = plan.parent_key.as_ref() {
            fields.insert("parent".to_owned(), json!({"key": parent.as_str()}));
        }
        let created = self
            .request(
                Method::POST,
                "rest/api/3/issue",
                Some(&json!({"fields": fields})),
            )
            .await?;
        external_at(&created, &["key"])
    }

    async fn issue_type(&self, kind: JiraIssueKind) -> Result<ExternalId, JiraError> {
        let project = url::form_urlencoded::byte_serialize(self.project_key.as_str().as_bytes())
            .collect::<String>();
        let value = self
            .request(
                Method::GET,
                &format!("rest/api/3/issue/createmeta/{project}/issuetypes?maxResults=100"),
                None,
            )
            .await?;
        let candidates = value
            .get("issueTypes")
            .or_else(|| value.get("values"))
            .and_then(Value::as_array)
            .ok_or_else(malformed)?;
        let matched = candidates
            .iter()
            .filter(|candidate| match kind {
                JiraIssueKind::Epic => {
                    candidate.get("hierarchyLevel").and_then(Value::as_i64) == Some(1)
                }
                JiraIssueKind::Task => candidate
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| normalize(name) == "task"),
            })
            .collect::<Vec<_>>();
        if matched.len() != 1 {
            return Err(JiraError::refused(
                "materialize",
                "the project does not expose one unambiguous required issue type",
            ));
        }
        external_at(matched[0], &["id"])
    }

    /// Read one issue's current canonical key and immutable id, nothing else.
    ///
    /// Jira resolves a superseded key to the issue that now owns it, so asking
    /// by the key Kontor last confirmed is how a rename is discovered at all.
    /// Deliberately lighter than [`Self::materialize`]: this makes no claim
    /// about summary, description, parent or type, because a rename is a change
    /// of name and asserting unrelated content here would refuse renames for
    /// reasons that have nothing to do with identity.
    ///
    /// # Errors
    /// Transport failures, and [`JiraError`] when the response carries no
    /// usable top-level `id` or `key`.
    pub async fn observe_identity(&self, key: &ExternalId) -> Result<JiraIssueIdentity, JiraError> {
        let encoded =
            url::form_urlencoded::byte_serialize(key.as_str().as_bytes()).collect::<String>();
        let value = self
            .request(
                Method::GET,
                &format!("rest/api/3/issue/{encoded}?fields=project"),
                None,
            )
            .await?;
        if text_at(&value, &["fields", "project", "key"])? != self.project_key.as_str() {
            return Err(JiraError::MaterializationConflict {
                kind: MaterializationConflict::ProjectMismatch,
            });
        }
        Ok(JiraIssueIdentity {
            issue_key: external_at(&value, &["key"])?,
            issue_id: external_at(&value, &["id"])?,
        })
    }

    async fn readback_issue(
        &self,
        key: &ExternalId,
        plan: &JiraIssuePlan,
    ) -> Result<JiraIssueReadback, JiraError> {
        let encoded =
            url::form_urlencoded::byte_serialize(key.as_str().as_bytes()).collect::<String>();
        let value = self
            .request(
                Method::GET,
                &format!(
                    "rest/api/3/issue/{encoded}?fields=project,issuetype,parent,summary,description,labels"
                ),
                None,
            )
            .await?;
        let observed_summary = text_at(&value, &["fields", "summary"])?;
        let observed_description = value_at(&value, &["fields", "description"])
            .cloned()
            .unwrap_or(Value::Null);
        let explicit_link = plan.requested_key.is_some();
        let strict_content = !explicit_link || plan.require_marker;
        // A body is held to the plan's exact text only while Kontor confirms an
        // issue it just created. There, any difference means it wrote to or
        // found the wrong issue.
        //
        // Recovering a previously planned Create by explicit key is a different
        // question. Identity is already proven by the marker label checked
        // below, and the body has had a life of its own since creation: the five
        // placeholder epics were repaired by hand precisely because Kontor's
        // generated body was wrong. Holding those to the generated text reported
        // an authored description as an incompatible human move, which is what
        // made the repair unrepeatable.
        //
        // So recovery asks the weaker, honest question — is there a body at all?
        // A richer body is the one the reader wants and is preserved. An absent
        // or empty one is refused, because it is neither an authored repair nor
        // the marker Kontor wrote, and losing a body is not a recovery.
        let body_refused = if explicit_link {
            // Recovering an issue that already existed: there must be a body,
            // and whatever it now says is the reader's to own.
            plan.require_marker
                && !observed_body(Some(&observed_description))?.is_some_and(|body| !body.is_empty())
        } else {
            // Confirming an issue Kontor just created or just found by marker:
            // the body must be exactly the one it wrote.
            observed_description != adf(&plan.description)
        };
        let mismatch = if text_at(&value, &["fields", "project", "key"])?
            != self.project_key.as_str()
        {
            Some(MaterializationConflict::ProjectMismatch)
        } else if optional_external_at(&value, &["fields", "parent", "key"])? != plan.parent_key {
            Some(MaterializationConflict::ParentMismatch)
        } else if strict_content && observed_summary != plan.summary {
            Some(MaterializationConflict::SummaryMismatch)
        } else if body_refused {
            Some(MaterializationConflict::DescriptionMismatch)
        } else {
            None
        };
        if let Some(kind) = mismatch {
            return Err(JiraError::MaterializationConflict { kind });
        }
        let issue_type_matches = match plan.kind {
            JiraIssueKind::Epic => {
                value_at(&value, &["fields", "issuetype", "hierarchyLevel"]).and_then(Value::as_i64)
                    == Some(1)
            }
            // Kontor's task is a hierarchy role, not a claim that a linked
            // legacy Jira issue has the literal type name `Task`. Jira's
            // standard work-item types (for example User Story and Tech tasks)
            // all live at hierarchy level zero and may be linked without being
            // rewritten. A newly created item, or an explicit-key recovery of
            // a previously planned Create, remains strict because Kontor itself
            // chose Jira's Task type for that durable creation intent.
            JiraIssueKind::Task if explicit_link && !plan.require_marker => {
                value_at(&value, &["fields", "issuetype", "hierarchyLevel"]).and_then(Value::as_i64)
                    == Some(0)
                    && value_at(&value, &["fields", "issuetype", "subtask"])
                        .and_then(Value::as_bool)
                        != Some(true)
            }
            JiraIssueKind::Task => text_at(&value, &["fields", "issuetype", "name"])
                .is_ok_and(|name| normalize(name) == "task"),
        };
        let marker_matches = (explicit_link && !plan.require_marker)
            || value_at(&value, &["fields", "labels"])
                .and_then(Value::as_array)
                .is_some_and(|labels| {
                    labels
                        .iter()
                        .any(|label| label.as_str() == Some(plan.marker.as_str()))
                });
        if !issue_type_matches {
            return Err(JiraError::MaterializationConflict {
                kind: MaterializationConflict::IssueTypeMismatch,
            });
        }
        if !marker_matches {
            return Err(JiraError::MaterializationConflict {
                kind: MaterializationConflict::MissingMarker,
            });
        }
        let readback = if explicit_link && plan.require_marker {
            json!({
                "schema_version": 1,
                "mode": "recover",
                "key": key.as_str(),
                "project": self.project_key.as_str(),
                "kind": match plan.kind { JiraIssueKind::Epic => "epic", JiraIssueKind::Task => "task" },
                "parent": plan.parent_key.as_ref().map(ExternalId::as_str),
                "summary": plan.summary,
                // The observed body, not the planned one: this evidence must say
                // what the reader has, and after a body repair those differ.
                "description": observed_description,
                "marker": plan.marker.as_str(),
            })
        } else if explicit_link {
            json!({
                "schema_version": 1,
                "mode": "link",
                "key": key.as_str(),
                "project": self.project_key.as_str(),
                "kind": match plan.kind { JiraIssueKind::Epic => "epic", JiraIssueKind::Task => "task" },
                "parent": plan.parent_key.as_ref().map(ExternalId::as_str),
                "summary": observed_summary,
                "description": observed_description,
            })
        } else {
            json!({
                "schema_version": 1,
                "key": key.as_str(),
                "project": self.project_key.as_str(),
                "kind": match plan.kind { JiraIssueKind::Epic => "epic", JiraIssueKind::Task => "task" },
                "parent": plan.parent_key.as_ref().map(ExternalId::as_str),
                "summary": plan.summary,
                "description": plan.description,
                "marker": plan.marker.as_str(),
            })
        };
        let readback_hash = CanonicalDocument::from_serializable(&readback)?
            .hash()
            .clone();
        // Read after every conflict check, so a response that is going to be
        // refused is still refused for its own exact reason. Jira returns the
        // top-level id on every issue GET regardless of the `fields` filter, so
        // a response without one is malformed rather than merely unsupported,
        // and an accepted readback never carries an unproven identity.
        let issue_id = external_at(&value, &["id"])?;
        Ok(JiraIssueReadback {
            issue_key: key.clone(),
            issue_id,
            readback_hash,
        })
    }
}

#[async_trait]
impl JiraExchange for JiraConnector {
    async fn execute(
        &self,
        _operation: &'static str,
        request: &JiraRequest,
    ) -> Result<JiraResponse, JiraError> {
        let requested_at = WireTimestamp::new(Timestamp::now());
        let before = self.live(&request.issue_key).await?;
        Self::validate_expected(request, &before)?;
        let (effective_operation, outcome, effects, confirmation) = match request.operation {
            JiraOperation::Observe | JiraOperation::Refetch => (
                request.operation,
                JiraOutcome::Observed,
                WireEffects::default(),
                None,
            ),
            JiraOperation::DryRun => (
                JiraOperation::DryRun,
                if request.field_writes.is_empty()
                    && request.transition.is_none()
                    && request.ownership_action == OwnershipAction::Preserve
                {
                    JiraOutcome::NoOp
                } else {
                    JiraOutcome::Planned
                },
                planned_effects(request, &before)?,
                None,
            ),
            JiraOperation::Apply if request.authorized_apply => {
                let effects = self.apply_effects(request, &before).await?;
                let after = self.live(&request.issue_key).await?;
                Self::confirm(request, &before, &after)?;
                (
                    JiraOperation::Apply,
                    if effects == WireEffects::default() {
                        JiraOutcome::NoOp
                    } else {
                        JiraOutcome::Applied
                    },
                    effects,
                    Some(WireConfirmation {
                        observation: after.observation,
                        confirmed_at: WireTimestamp::new(Timestamp::now()),
                    }),
                )
            }
            JiraOperation::Apply => (
                JiraOperation::DryRun,
                JiraOutcome::Planned,
                planned_effects(request, &before)?,
                None,
            ),
        };
        Ok(JiraResponse {
            schema_version: request.schema_version,
            operation: request.operation,
            effective_operation,
            issue_key: request.issue_key.clone(),
            idempotency_key: request.idempotency_key.clone(),
            intent_hash: request.intent_hash.clone(),
            requested_at,
            completed_at: WireTimestamp::new(Timestamp::now()),
            outcome,
            observation: Some(before.observation),
            principal_account_id: before.principal_account_id,
            live_transitions: before.live_transitions,
            effects,
            confirmation,
            conflict: None,
            unavailable: None,
            notes: Vec::new(),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JiraCredentials {
    email: String,
    api_token: String,
}

struct LiveIssue {
    observation: WireObservation,
    live_transitions: Vec<WireTransition>,
    principal_account_id: Option<ExternalId>,
    fields: Map<String, Value>,
}

fn validate_create_fields(fields: &JiraCreateFields) -> Result<(), JiraError> {
    for configured in [&fields.epic, &fields.task] {
        if configured.len() > MAX_CREATE_FIELDS
            || serde_json::to_vec(configured)
                .map_or(true, |encoded| encoded.len() > MAX_CREATE_FIELDS_BYTES)
        {
            return Err(configuration(
                "configured Jira create fields exceed the supported bound",
            ));
        }
        for field in configured.keys() {
            if field.is_empty()
                || field.len() > 64
                || !field
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(configuration(
                    "a configured Jira create field id is invalid",
                ));
            }
            if RESERVED_CREATE_FIELDS.contains(&field.as_str()) {
                return Err(configuration(
                    "configured Jira create fields may not override Kontor-owned fields",
                ));
            }
        }
    }
    Ok(())
}

fn non_success(status: StatusCode, bytes: &[u8]) -> JiraError {
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return JiraError::unavailable(
            "transport",
            UnavailableReason::Credential,
            "Jira rejected the configured credential or its project permission",
        );
    }
    if status == StatusCode::BAD_REQUEST {
        let field_ids = serde_json::from_slice::<Value>(bytes)
            .ok()
            .and_then(|value| value.get("errors").and_then(Value::as_object).cloned())
            .map(|errors| {
                errors
                    .into_iter()
                    .map(|(field, _)| field)
                    .filter(|field| {
                        !field.is_empty()
                            && field.len() <= 64
                            && field
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                    })
                    .take(MAX_CREATE_FIELDS)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let detail = if field_ids.is_empty() {
            "Jira rejected the request because its input is invalid".to_owned()
        } else {
            format!(
                "Jira rejected invalid or missing fields: {}",
                field_ids.join(", ")
            )
        };
        return JiraError::unavailable("transport", UnavailableReason::SchemaMismatch, detail);
    }
    JiraError::unavailable(
        "transport",
        UnavailableReason::Transport,
        format!("Jira returned non-success status {}", status.as_u16()),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraComment {
    pub external_comment_id: ExternalId,
    pub body: String,
    pub body_hash: ContentHash,
    pub author_account_id: ExternalId,
    pub author_display: Option<ExternalName>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl JiraComment {
    fn from_value(value: &Value) -> Result<Self, JiraError> {
        let body_value = value.get("body").cloned().ok_or_else(malformed)?;
        let body = adf_text(&body_value);
        let body_hash = ContentHash::of(body.as_bytes());
        Ok(Self {
            external_comment_id: external_at(value, &["id"])?,
            body,
            body_hash,
            author_account_id: external_at(value, &["author", "accountId"])?,
            author_display: optional_name_at(value, &["author", "displayName"])?,
            created_at: timestamp_at(value, &["created"])?,
            updated_at: timestamp_at(value, &["updated"])?,
        })
    }
}

fn planned_effects(request: &JiraRequest, before: &LiveIssue) -> Result<WireEffects, JiraError> {
    let assignment = if request.ownership_action == OwnershipAction::ReassignToPrincipal {
        Some(WireAssignment {
            action: OwnershipAction::ReassignToPrincipal,
            account_id: before.principal_account_id.clone(),
        })
    } else {
        None
    };
    Ok(WireEffects {
        field_ids: request
            .field_writes
            .iter()
            .map(|write| write.field_id.clone())
            .collect(),
        assignment,
        transition: request.transition.clone(),
    })
}

fn encode_field(write: &FieldWrite) -> Value {
    match &write.value {
        WireFieldValue::Text { text }
            if write.encoding == kontor_core::ticket::FieldEncoding::StructuredDocument =>
        {
            adf(text.as_str())
        }
        WireFieldValue::Text { text } => Value::String(text.as_str().to_owned()),
        WireFieldValue::Select { option_id } => json!({"id": option_id.as_str()}),
        WireFieldValue::MultiSelect { option_ids } => Value::Array(
            option_ids
                .iter()
                .map(|id| json!({"id": id.as_str()}))
                .collect(),
        ),
        WireFieldValue::Number { value } => json!(value),
        WireFieldValue::Date { value } => json!(value.as_str()),
        WireFieldValue::Labels { values } => {
            Value::Array(values.iter().map(|value| json!(value.as_str())).collect())
        }
    }
}

fn field_matches(write: &FieldWrite, actual: Option<&Value>) -> bool {
    let Some(actual) = actual else { return false };
    match &write.value {
        WireFieldValue::Select { option_id } => actual
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == option_id.as_str()),
        WireFieldValue::MultiSelect { option_ids } => {
            let expected: BTreeSet<&str> = option_ids.iter().map(ExternalId::as_str).collect();
            let found: BTreeSet<&str> = actual
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|value| value.get("id").and_then(Value::as_str))
                .collect();
            found == expected
        }
        _ => actual == &encode_field(write),
    }
}

fn adf(text: &str) -> Value {
    json!({
        "type": "doc",
        "version": 1,
        "content": text.lines().map(|line| {
            let inline = adf_line(line);
            // A blank line is a paragraph with no children, and Jira stores that
            // as `{"type": "paragraph"}` — it drops an empty `content` array.
            // Sending the empty array means the confirming readback compares a
            // document Jira will never return, so an otherwise successful write
            // is reported as unconfirmed. Kontor only ever wrote single-line
            // bodies before ASMA-8123, which is why this never showed.
            if inline.is_empty() {
                json!({"type": "paragraph"})
            } else {
                json!({"type": "paragraph", "content": inline})
            }
        }).collect::<Vec<_>>()
    })
}

/// Split one line into ADF text nodes, marking bare URLs as links.
///
/// A plan reference has to be *clickable* for a Jira reader — the epic content
/// contract asks for a link, and a bare URL sitting in a paragraph is not one.
/// Marking them here rather than accepting a richer input format keeps the
/// authored body plain text everywhere else: the caller writes prose, and the
/// only structure Kontor infers is the one it can infer unambiguously.
fn adf_line(line: &str) -> Vec<Value> {
    let mut nodes = Vec::new();
    let mut cursor = 0usize;
    while cursor < line.len() {
        let Some(offset) = ["https://", "http://"]
            .iter()
            .filter_map(|scheme| line[cursor..].find(scheme))
            .min()
        else {
            break;
        };
        let start = cursor.saturating_add(offset);
        let end = line[start..]
            .find(char::is_whitespace)
            .map_or(line.len(), |length| start.saturating_add(length));
        // Sentence punctuation that happens to follow a URL is not part of it.
        let url = line[start..end].trim_end_matches(['.', ',', ')', ';', ':', '!', '?']);
        if url.ends_with("//") {
            // A bare scheme is not a link. Skip it rather than emit an empty one.
            cursor = end;
            continue;
        }
        if start > cursor {
            nodes.push(json!({"type": "text", "text": &line[cursor..start]}));
        }
        nodes.push(json!({
            "type": "text",
            "text": url,
            "marks": [{"type": "link", "attrs": {"href": url}}]
        }));
        cursor = start.saturating_add(url.len());
    }
    if cursor < line.len() {
        nodes.push(json!({"type": "text", "text": &line[cursor..]}));
    }
    nodes
}

/// Read one observed issue body into comparable evidence.
///
/// `None` for the whole field means the connector never reported it, so Kontor
/// has no evidence either way. A JSON `null` is different: Jira says the issue
/// exists and its body is empty, which is evidence, and is reported as a
/// present-but-empty body rather than as missing evidence.
///
/// # Errors
/// Returns [`JiraError`] when the body cannot be canonicalized or its rendered
/// text exceeds the bounded-text limit.
fn observed_body(value: Option<&Value>) -> Result<Option<ObservedBody>, JiraError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(Some(ObservedBody {
            present: false,
            content_hash: CanonicalDocument::from_serializable(&json!({
                "schema_version": 1,
                "body": Value::Null,
            }))?
            .hash()
            .clone(),
            plain_text: BoundedText::parse("")?,
        }));
    }
    let content_hash = CanonicalDocument::from_serializable(&json!({
        "schema_version": 1,
        "body": value,
    }))?
    .hash()
    .clone();
    let rendered = adf_text(value);
    let plain_text = BoundedText::parse(&rendered).or_else(|_| {
        // An oversized body is still observable evidence: keep a bounded prefix
        // so a refusal stays readable, and let the exact hash carry identity.
        let mut truncated: String = rendered.chars().take(4000).collect();
        truncated.push_str("\n[truncated]");
        BoundedText::parse(&truncated)
    })?;
    Ok(Some(ObservedBody {
        present: true,
        content_hash,
        plain_text,
    }))
}

/// The digest an observed body would carry if it held exactly `text`.
///
/// Computed by rendering `text` to the same document a write sends and reading
/// it back through [`observed_body`], so an intended body and an observed body
/// are comparable by construction rather than by two hand-kept formulas that
/// could drift apart. A drift there would silently report every repair as
/// unconverged and invite a rewrite loop.
///
/// # Errors
/// Returns [`JiraError`] when the rendered text cannot be canonicalized or
/// exceeds the bounded-text limit.
pub fn description_hash(text: &str) -> Result<ContentHash, JiraError> {
    let document = adf(text);
    let observed = observed_body(Some(&document))?
        .ok_or_else(|| JiraError::refused("description", "a rendered body is always observable"))?;
    Ok(observed.content_hash)
}

/// Render one external body to the plain text a human would read.
///
/// Inline nodes are joined *within* their block and blocks are separated by a
/// newline. Joining every text node with a newline instead — which is what this
/// did before ASMA-8123 — turns one sentence into as many lines as it has bold
/// runs and links, so the evidence a refusal quotes reads as fragments rather
/// than prose. The whole reason this rendering exists is to be readable without
/// fetching the issue again.
fn adf_text(value: &Value) -> String {
    let mut blocks = Vec::new();
    collect_blocks(value, &mut blocks);
    blocks.join("\n")
}

/// Whether an ADF node is a block, and therefore starts its own line.
fn is_block(kind: &str) -> bool {
    matches!(
        kind,
        "paragraph"
            | "heading"
            | "blockquote"
            | "codeBlock"
            | "listItem"
            | "bulletList"
            | "orderedList"
            | "panel"
            | "rule"
            | "tableRow"
            | "tableCell"
            | "tableHeader"
    )
}

fn collect_blocks(value: &Value, output: &mut Vec<String>) {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let has_block_child = value
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|content| {
            content.iter().any(|child| {
                child
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(is_block)
            })
        });
    if is_block(kind) && !has_block_child {
        let mut inline = Vec::new();
        collect_text(value, &mut inline);
        output.push(inline.concat());
        return;
    }
    if let Some(content) = value.get("content").and_then(Value::as_array) {
        for child in content {
            collect_blocks(child, output);
        }
        return;
    }
    // A bare inline node with no block around it is still readable text.
    let mut inline = Vec::new();
    collect_text(value, &mut inline);
    if !inline.is_empty() {
        output.push(inline.concat());
    }
}

fn collect_text(value: &Value, output: &mut Vec<String>) {
    if value.get("type").and_then(Value::as_str) == Some("text") {
        if let Some(text) = value.get("text").and_then(Value::as_str) {
            output.push(text.to_owned());
        }
        return;
    }
    if let Some(content) = value.get("content").and_then(Value::as_array) {
        for child in content {
            collect_text(child, output);
        }
    }
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn value_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
}

fn text_at<'a>(value: &'a Value, path: &[&str]) -> Result<&'a str, JiraError> {
    value_at(value, path)
        .and_then(Value::as_str)
        .ok_or_else(malformed)
}

fn external_at(value: &Value, path: &[&str]) -> Result<ExternalId, JiraError> {
    ExternalId::parse(text_at(value, path)?).map_err(JiraError::from)
}

fn optional_external_at(value: &Value, path: &[&str]) -> Result<Option<ExternalId>, JiraError> {
    value_at(value, path)
        .and_then(Value::as_str)
        .map(ExternalId::parse)
        .transpose()
        .map_err(JiraError::from)
}

fn name_at(value: &Value, path: &[&str]) -> Result<ExternalName, JiraError> {
    ExternalName::parse(text_at(value, path)?).map_err(JiraError::from)
}

fn optional_name_at(value: &Value, path: &[&str]) -> Result<Option<ExternalName>, JiraError> {
    value_at(value, path)
        .and_then(Value::as_str)
        .map(ExternalName::parse)
        .transpose()
        .map_err(JiraError::from)
}

fn timestamp_at(value: &Value, path: &[&str]) -> Result<Timestamp, JiraError> {
    let value = text_at(value, path)?;
    value.parse().map_err(|_| malformed())
}

fn malformed() -> JiraError {
    JiraError::unavailable(
        "transport",
        UnavailableReason::MalformedResponse,
        "Jira returned an incomplete response",
    )
}

fn configuration(detail: &'static str) -> JiraError {
    JiraError::unavailable("configuration", UnavailableReason::Configuration, detail)
}

fn transport(detail: &'static str) -> JiraError {
    JiraError::unavailable("transport", UnavailableReason::Transport, detail)
}

fn oversized() -> JiraError {
    JiraError::unavailable(
        "transport",
        UnavailableReason::OversizedOutput,
        "Jira returned an oversized response",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_line_is_written_the_way_jira_stores_it() {
        // Jira drops an empty `content` array. Sending one means the confirming
        // readback compares against a document Jira will never return, so a
        // successful multi-paragraph write reports as unconfirmed — which is
        // exactly what happened to the first ASMA-8123 repair run.
        let document = adf("First.\n\nSecond.");
        assert_eq!(
            document["content"][1],
            json!({"type": "paragraph"}),
            "an empty paragraph carries no content key"
        );
        assert_eq!(document["content"][0]["content"][0]["text"], "First.");
        // The rendering still shows the blank line it was written as.
        assert_eq!(adf_text(&document), "First.\n\nSecond.");
    }

    #[test]
    fn a_plan_reference_is_written_as_a_clickable_link() {
        // The epic content contract asks for a link a Jira reader can follow.
        let document = adf(
            "Detailed plan\nPublication plan — https://github.com/Carasent-ASMA/asma-modules/blob/master/plan/x.md",
        );
        let paragraph = &document["content"][1]["content"];
        assert_eq!(paragraph[0]["text"], "Publication plan — ");
        assert!(
            paragraph[0].get("marks").is_none(),
            "prose around a URL is not itself a link"
        );
        assert_eq!(
            paragraph[1]["marks"][0]["attrs"]["href"],
            "https://github.com/Carasent-ASMA/asma-modules/blob/master/plan/x.md"
        );
        // And the rendering still reads as the one line it was written as.
        assert_eq!(
            adf_text(&document),
            "Detailed plan\nPublication plan — https://github.com/Carasent-ASMA/asma-modules/blob/master/plan/x.md"
        );
    }

    #[test]
    fn sentence_punctuation_after_a_url_stays_out_of_the_link() {
        let document = adf("See https://example.com/a. Then stop.");
        let paragraph = &document["content"][0]["content"];
        assert_eq!(
            paragraph[1]["marks"][0]["attrs"]["href"],
            "https://example.com/a"
        );
        assert_eq!(paragraph[2]["text"], ". Then stop.");
        assert_eq!(adf_text(&document), "See https://example.com/a. Then stop.");
    }

    #[test]
    fn an_inline_run_renders_as_one_line_not_one_line_per_node() {
        // The shape every hand-authored Jira body has: bold runs and links
        // inside one paragraph. Rendering each node on its own line is what made
        // the observed evidence read as fragments.
        let observed = observed_body(Some(&json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [
                    {"type": "text", "text": "Branches carrying no "},
                    {"type": "text", "text": "ASMA-<number>", "marks": [{"type": "strong"}]},
                    {"type": "text", "text": " key are refused."}
                ]
            }]
        })))
        .expect("read")
        .expect("body");
        assert_eq!(
            observed.plain_text.as_str(),
            "Branches carrying no ASMA-<number> key are refused."
        );
    }

    #[test]
    fn a_missing_description_field_is_missing_evidence() {
        // The connector did not report the field at all. That is not the same
        // as an empty body, and must not be reported as one.
        assert!(observed_body(None).expect("read").is_none());
    }

    #[test]
    fn a_null_description_is_evidence_of_an_empty_body() {
        let observed = observed_body(Some(&Value::Null))
            .expect("read")
            .expect("body");
        assert!(!observed.present);
        assert!(observed.is_empty());
        assert!(!observed.is_placeholder_only());
    }

    #[test]
    fn an_adf_body_is_rendered_and_hashed() {
        let document = adf("## Goal\nShip the thing.");
        let observed = observed_body(Some(&document)).expect("read").expect("body");
        assert!(observed.present);
        assert!(!observed.is_empty());
        assert_eq!(observed.plain_text.as_str(), "## Goal\nShip the thing.");
        // The hash is over the exact document, so an identical rendering from a
        // different structure is still a different body.
        let other = observed_body(Some(&json!({
            "type": "doc",
            "version": 1,
            "content": [{"type": "paragraph", "content": [
                {"type": "text", "text": "## Goal"},
                {"type": "text", "text": "Ship the thing."}
            ]}]
        })))
        .expect("read")
        .expect("body");
        assert_ne!(observed.content_hash, other.content_hash);
    }

    #[test]
    fn the_creation_marker_survives_the_adf_round_trip() {
        // This is the exact body Kontor writes at create time, and the exact
        // body observed on the five placeholder epics.
        let marker =
            "Kontor epic 01a0721b-ea30-7fe3-88a5-4d33ca613414: Publication identity enforcement";
        let observed = observed_body(Some(&adf(marker)))
            .expect("read")
            .expect("body");
        assert_eq!(observed.plain_text.as_str(), marker);
        assert!(
            observed.is_placeholder_only(),
            "the connector must observe the creation marker as a placeholder"
        );
    }

    #[test]
    fn jira_validation_failure_names_only_safe_field_ids() {
        let error = non_success(
            StatusCode::BAD_REQUEST,
            br#"{"errors":{"customfield_10251":"secret operator-facing detail"}}"#,
        );
        let rendered = error.to_string();
        assert!(rendered.contains("customfield_10251"));
        assert!(!rendered.contains("secret operator-facing detail"));
        assert!(matches!(
            error,
            JiraError::Unavailable {
                reason: UnavailableReason::SchemaMismatch,
                ..
            }
        ));
    }
}
