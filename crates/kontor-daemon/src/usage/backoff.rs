//! Negative-only, restart-safe usage endpoint retry deadlines. Never quota evidence.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use kontor_core::repository::AccountProfile;
use serde::{Deserialize, Serialize};

const FILE: &str = "provider-usage-backoff.json";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProbeKey {
    account_profile_id: String,
    credential_alias: String,
    provider_family: String,
}

impl ProbeKey {
    pub(super) fn new(profile: &AccountProfile, provider: &str) -> Self {
        Self {
            account_profile_id: profile.id.to_string(),
            credential_alias: profile.credential_ref.alias.as_str().to_owned(),
            provider_family: crate::applications::provider_family(provider).to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetryRecord {
    key: ProbeKey,
    /// Derived only from an actual HTTP429 or the injected non-secret reporter seam.
    retry_at_unix_millis: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    records: Vec<RetryRecord>,
}

pub(super) struct ProbeBackoffs {
    path: PathBuf,
    records: Mutex<BTreeMap<ProbeKey, RetryRecord>>,
    in_flight: Mutex<BTreeMap<ProbeKey, Arc<tokio::sync::Mutex<()>>>>,
}

impl std::fmt::Debug for ProbeBackoffs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProbeBackoffs")
            .finish_non_exhaustive()
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

impl ProbeBackoffs {
    pub(super) fn load(root: &Path) -> Self {
        let path = root.join(FILE);
        let records = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Journal>(&bytes).ok())
            .filter(|journal| journal.schema_version == 1)
            .map(|journal| {
                journal
                    .records
                    .into_iter()
                    .filter(|record| record.retry_at_unix_millis > now_millis())
                    .map(|record| (record.key.clone(), record))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            path,
            records: Mutex::new(records),
            in_flight: Mutex::new(BTreeMap::new()),
        }
    }

    pub(super) fn latch(&self, key: &ProbeKey) -> Arc<tokio::sync::Mutex<()>> {
        Arc::clone(
            self.in_flight
                .lock()
                .expect("usage probe map lock")
                .entry(key.clone())
                .or_default(),
        )
    }

    pub(super) fn remaining(&self, key: &ProbeKey) -> Option<u64> {
        self.records
            .lock()
            .expect("usage backoff lock")
            .get(key)
            .map(|record| record.retry_at_unix_millis.saturating_sub(now_millis()))
            .filter(|remaining| *remaining > 0)
            .map(|millis| millis.div_ceil(1000))
    }

    pub(super) fn record(&self, key: ProbeKey, seconds: u64) -> std::io::Result<()> {
        let mut records = self.records.lock().expect("usage backoff lock");
        let now = now_millis();
        records.retain(|_, record| record.retry_at_unix_millis > now);
        records.insert(
            key.clone(),
            RetryRecord {
                key,
                retry_at_unix_millis: now.saturating_add(seconds.max(1).saturating_mul(1000)),
            },
        );
        let document = Journal {
            schema_version: 1,
            records: records.values().cloned().collect(),
        };
        let bytes = serde_json::to_vec(&document)?;
        // One Realm process owns this cache. A failed write retains the in-memory
        // refusal and is logged; it never manufactures a successful observation.
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(temporary, &self.path)
    }
}
