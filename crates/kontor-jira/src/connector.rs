use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use kontor_accounts::{KeychainBackend, KeychainTarget, SystemKeychain};
use kontor_core::id::{
    CanonicalDocument, ContentHash, ExternalId, ExternalName, ProjectId, Timestamp,
};
use kontor_core::ticket::OwnershipAction;
use reqwest::{Client, Method, StatusCode, Url};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::jira::{
    FieldWrite, JiraExchange, JiraOperation, JiraOutcome, JiraRequest, JiraResponse,
    WireAssignment, WireConfirmation, WireEffects, WireFieldValue, WireObservation, WireTransition,
};
use crate::{JiraError, UnavailableReason, WireTimestamp};

const CONFIG_SCHEMA: u32 = 1;
const CONFIG_FILE: &str = "jira.json";
const KEYCHAIN_SERVICE: &str = "kontor-jira";
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraIssueReadback {
    pub issue_key: ExternalId,
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
            keychain,
            client,
        })
    }

    fn credentials(&self) -> Result<JiraCredentials, JiraError> {
        let target = KeychainTarget::new(KEYCHAIN_SERVICE, self.credential_alias.clone());
        let secret: SecretString = self.keychain.secret(&target).map_err(|_| {
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
        let credentials = self.credentials()?;
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
        if !response.status().is_success() {
            return Err(transport("Jira returned a non-success status"));
        }
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
        let observation = WireObservation {
            status_id,
            status_name,
            status_category,
            issue_type,
            assignee_account_id,
            assignee_display,
            update_token,
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
            let offered = before.live_transitions.iter().any(|candidate| {
                candidate.transition_id == transition.transition_id
                    && candidate.to_status_id == transition.to_status_id
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
        if let Some(transition) = request.transition.as_ref()
            && after.observation.status_id != transition.to_status_id
        {
            return Err(transport("Jira readback did not confirm the transition"));
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
                    return Err(JiraError::Conflict {
                        operation: "materialize",
                        kind: kontor_core::ticket::StatusConflictKind::IncompatibleHumanMove,
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
        let mut fields = Map::from_iter([
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
        if text_at(&value, &["fields", "project", "key"])? != self.project_key.as_str()
            || optional_external_at(&value, &["fields", "parent", "key"])? != plan.parent_key
            || (strict_content
                && (observed_summary != plan.summary
                    || observed_description != adf(&plan.description)))
        {
            return Err(JiraError::Conflict {
                operation: "materialize",
                kind: kontor_core::ticket::StatusConflictKind::IncompatibleHumanMove,
            });
        }
        let issue_type_matches = match plan.kind {
            JiraIssueKind::Epic => {
                value_at(&value, &["fields", "issuetype", "hierarchyLevel"]).and_then(Value::as_i64)
                    == Some(1)
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
        if !issue_type_matches || !marker_matches {
            return Err(JiraError::Conflict {
                operation: "materialize",
                kind: kontor_core::ticket::StatusConflictKind::IncompatibleHumanMove,
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
                "description": plan.description,
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
        Ok(JiraIssueReadback {
            issue_key: key.clone(),
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
        "content": text.lines().map(|line| json!({
            "type": "paragraph",
            "content": if line.is_empty() { Vec::<Value>::new() } else { vec![json!({"type": "text", "text": line})] }
        })).collect::<Vec<_>>()
    })
}

fn adf_text(value: &Value) -> String {
    let mut text = Vec::new();
    collect_text(value, &mut text);
    text.join("\n")
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
