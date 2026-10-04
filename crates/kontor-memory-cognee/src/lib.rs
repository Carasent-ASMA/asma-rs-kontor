//! Synthetic-qualified Cognee REST boundary. Provider bytes never become authority.
//!
//! The CHUNKS wire shape is pinned in the evidence for ASMA-8158. Unknown shapes
//! fail closed to lexical recall; a future upstream release needs qualification.

use std::time::Duration;

use kontor_core::id::{ContentHash, reject_sensitive_text};
use kontor_core::memory::{
    DegradedReason, MAX_CANDIDATES, MemoryCandidate, ProjectionEntry, RecallIntent,
};
use kontor_store::memory::{
    ProjectionPreview, ProjectionQualification, ProjectionSnapshot, RecalledMemory, SemanticRecall,
};
use reqwest::{RequestBuilder, multipart};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bound every response before JSON decoding, including chunk text and errors.
pub const MAX_RESPONSE_BYTES: usize = 262_144;

/// No credential values belong in the persisted configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Explicit opt-in; an absent document performs no network work.
    pub enabled: bool,
    /// Self-hosted endpoint without userinfo, query or fragment.
    pub endpoint: Option<String>,
    /// Deployment alias, resolved by an authorized composition boundary.
    pub credential_alias: Option<String>,
    /// Version-one snapshot names are fixed to `kontor` by the store contract.
    pub dataset_prefix: String,
    /// Total semantic deadline, leaving at least 500 ms for lexical completion.
    pub timeout_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: None,
            credential_alias: None,
            dataset_prefix: "kontor".into(),
            timeout_ms: 1500,
        }
    }
}

/// Redacted static refusal; never carries upstream bodies or credential values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error(pub DegradedReason);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cognee_{}", self.0.as_str())
    }
}
impl std::error::Error for Error {}

impl Config {
    /// Read a bounded optional document. This never resolves a credential alias.
    pub fn read(path: &std::path::Path) -> Result<Option<Self>, Error> {
        use std::io::Read;
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(Error(DegradedReason::Unavailable)),
        };
        let mut bytes = Vec::new();
        file.take(8193)
            .read_to_end(&mut bytes)
            .map_err(|_| Error(DegradedReason::Unavailable))?;
        if bytes.len() > 8192 {
            return Err(Error(DegradedReason::Malformed));
        }
        let config: Self =
            serde_json::from_slice(&bytes).map_err(|_| Error(DegradedReason::Malformed))?;
        config.validate()?;
        Ok(Some(config))
    }

    /// Validate without contacting a provider or resolving credentials.
    pub fn validate(&self) -> Result<(), Error> {
        let invalid = || Error(DegradedReason::Malformed);
        if self.dataset_prefix != "kontor" || !(1..=1500).contains(&self.timeout_ms) {
            return Err(invalid());
        }
        if let Some(alias) = &self.credential_alias
            && (alias.is_empty()
                || alias.len() > 128
                || reject_sensitive_text("cognee.credential_alias", alias).is_err())
        {
            return Err(invalid());
        }
        if let Some(endpoint) = &self.endpoint {
            let url = url::Url::parse(endpoint).map_err(|_| invalid())?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || reject_sensitive_text("cognee.endpoint", endpoint).is_err()
            {
                return Err(invalid());
            }
            if url.scheme() == "http"
                && !matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "localhost"))
            {
                return Err(invalid());
            }
        } else if self.enabled {
            return Err(invalid());
        }
        Ok(())
    }
}

/// Has no logging, implicit retries, redirects, proxy inheritance or secret lookup.
#[derive(Clone)]
pub struct Client {
    config: Config,
    http: reqwest::Client,
    credential: Option<SecretString>,
}
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CogneeClient")
            .field("enabled", &self.config.enabled)
            .finish_non_exhaustive()
    }
}

struct Search {
    candidates: Vec<MemoryCandidate>,
}

impl Client {
    /// Compose an explicitly supplied credential; never reads Keychain or env.
    pub fn new(config: Config, credential: Option<SecretString>) -> Result<Self, Error> {
        config.validate()?;
        if config.enabled && config.credential_alias.is_some() != credential.is_some() {
            return Err(Error(DegradedReason::Unavailable));
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|_| Error(DegradedReason::Unavailable))?;
        Ok(Self {
            config,
            http,
            credential,
        })
    }

    fn request(&self, path: &str) -> Result<RequestBuilder, Error> {
        if !self.config.enabled {
            return Err(Error(DegradedReason::Absent));
        }
        let endpoint = self
            .config
            .endpoint
            .as_deref()
            .ok_or(Error(DegradedReason::Absent))?;
        let request = self
            .http
            .post(format!("{}{path}", endpoint.trim_end_matches('/')));
        Ok(match &self.credential {
            Some(secret) => request.bearer_auth(secret.expose_secret()),
            None => request,
        })
    }

    async fn json(&self, request: RequestBuilder) -> Result<Value, Error> {
        let mut response = request.send().await.map_err(transport_error)?;
        if !response.status().is_success() {
            return Err(Error(DegradedReason::Unavailable));
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
        {
            return Err(Error(DegradedReason::Malformed));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(Error(DegradedReason::Malformed));
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error(DegradedReason::Malformed))
    }

    async fn chunks(&self, snapshot: &ProjectionSnapshot, query: &str) -> Result<Search, Error> {
        let request = self.request("/api/v1/search")?.json(&serde_json::json!({
            "query": query, "search_type": "CHUNKS", "datasets": [snapshot.dataset], "top_k": MAX_CANDIDATES,
        }));
        let response = self.json(request).await?;
        parse_chunks(&response)
    }

    /// Network finishes before `freeze` enters the authoritative transaction.
    /// Only identifiers and finite scores reach it. The returned block must come
    /// from that transaction, never from the text used to locate candidates.
    pub async fn recall<E>(
        &self,
        snapshot: &ProjectionSnapshot,
        intent: &RecallIntent,
        freeze: impl FnOnce(&SemanticRecall) -> Result<RecalledMemory, E>,
    ) -> Result<RecalledMemory, E> {
        let search = async {
            let query = intent
                .canonical()
                .map_err(|_| Error(DegradedReason::Malformed))?;
            self.chunks(snapshot, query.json()).await
        };
        let semantic =
            match tokio::time::timeout(Duration::from_millis(self.config.timeout_ms), search).await
            {
                Ok(Ok(search)) => SemanticRecall::Candidates {
                    projection_digest: snapshot.digest.clone(),
                    candidates: search.candidates,
                },
                Ok(Err(error)) => SemanticRecall::Degraded(error.0),
                Err(_) => SemanticRecall::Degraded(DegradedReason::Timeout),
            };
        freeze(&semantic)
    }

    /// Add -> blocking cognify -> CHUNKS canary, without activating a pointer.
    /// The caller stages a store-produced coherent snapshot before this call.
    pub async fn qualify(
        &self,
        preview: &ProjectionPreview,
    ) -> Result<ProjectionQualification, Error> {
        let operation = async {
            validate_preview(preview)?;
            let mut form =
                multipart::Form::new().text("datasetName", preview.snapshot.dataset.clone());
            for (index, entry) in preview.entries.iter().enumerate() {
                let text =
                    serde_json::to_string(entry).map_err(|_| Error(DegradedReason::Malformed))?;
                let part = multipart::Part::text(text)
                    .file_name(format!("experience-{index}.txt"))
                    .mime_str("text/plain")
                    .map_err(|_| Error(DegradedReason::Malformed))?;
                form = form.part("data", part);
            }
            self.json(self.request("/api/v1/add")?.multipart(form))
                .await?;
            let result = self
                .json(self.request("/api/v1/cognify")?.json(&serde_json::json!({
                    "datasets": [preview.snapshot.dataset], "run_in_background": false,
                })))
                .await?;
            let runs = result.as_object().ok_or(Error(DegradedReason::Malformed))?;
            if runs.is_empty()
                || runs
                    .values()
                    .any(|run| run["status"] != "PipelineRunCompleted")
            {
                return Err(Error(DegradedReason::Unavailable));
            }
            let first = preview
                .entries
                .first()
                .ok_or(Error(DegradedReason::Empty))?;
            let search = self.chunks(&preview.snapshot, &first.lesson).await?;
            if search.candidates.is_empty()
                || search.candidates.iter().any(|candidate| {
                    !preview.snapshot.identities.iter().any(|id| {
                        id.project_id == candidate.project_id
                            && id.item_id == candidate.item_id
                            && id.revision_id == candidate.revision_id
                            && id.content_hash == candidate.content_hash
                    })
                })
                || !search
                    .candidates
                    .iter()
                    .any(|candidate| candidate.revision_id == first.identity.revision_id)
            {
                return Err(Error(DegradedReason::NoEligibleCandidates));
            }
            Ok(ProjectionQualification {
                digest: preview.snapshot.digest.clone(),
                added: true,
                cognified: true,
                canary_passed: true,
            })
        };
        tokio::time::timeout(Duration::from_millis(self.config.timeout_ms), operation)
            .await
            .map_err(|_| Error(DegradedReason::Timeout))?
    }
}

fn transport_error(error: reqwest::Error) -> Error {
    Error(if error.is_timeout() {
        DegradedReason::Timeout
    } else {
        DegradedReason::Unavailable
    })
}

fn validate_preview(preview: &ProjectionPreview) -> Result<(), Error> {
    if preview.entries.is_empty() {
        return Err(Error(DegradedReason::Empty));
    }
    let s = &preview.snapshot;
    let digest = ContentHash::of(
        &serde_json::to_vec(&(s.project_id, s.memory_cursor, &s.dataset, &preview.entries))
            .map_err(|_| Error(DegradedReason::Malformed))?,
    );
    if s.dataset != format!("kontor_{}_{}", s.project_id, s.memory_cursor)
        || s.digest != digest
        || s.identities
            != preview
                .entries
                .iter()
                .map(|entry| entry.identity.clone())
                .collect::<Vec<_>>()
        || preview
            .entries
            .iter()
            .any(|entry| entry.identity.project_id != s.project_id)
    {
        return Err(Error(DegradedReason::Malformed));
    }
    Ok(())
}

fn parse_chunks(response: &Value) -> Result<Search, Error> {
    let malformed = || Error(DegradedReason::Malformed);
    let datasets = response.as_array().ok_or_else(malformed)?;
    if datasets.len() > MAX_CANDIDATES {
        return Err(malformed());
    }
    let mut candidates = Vec::new();
    for dataset in datasets {
        let chunks = dataset
            .get("search_result")
            .and_then(Value::as_array)
            .ok_or_else(malformed)?;
        if candidates.len() + chunks.len() > MAX_CANDIDATES {
            return Err(malformed());
        }
        for chunk in chunks {
            // CHUNKS returns backend distance (smaller is better). Negate once
            // so the authoritative store's descending score order is correct.
            let score = -chunk
                .get("score")
                .and_then(Value::as_f64)
                .filter(|score| score.is_finite())
                .ok_or_else(malformed)?;
            let text = chunk
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(malformed)?;
            let entry: ProjectionEntry = serde_json::from_str(text).map_err(|_| malformed())?;
            let candidate = MemoryCandidate {
                project_id: entry.identity.project_id,
                item_id: entry.identity.item_id,
                revision_id: entry.identity.revision_id,
                content_hash: entry.identity.content_hash,
                score,
            };
            candidate.validate().map_err(|_| malformed())?;
            candidates.push(candidate);
        }
    }
    Ok(Search { candidates })
}
