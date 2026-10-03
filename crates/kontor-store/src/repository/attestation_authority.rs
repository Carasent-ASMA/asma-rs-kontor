//! Public issuer-key ledger on the existing SQLite single writer.
//!
//! Every mutation is one BEGIN IMMEDIATE unit; schema triggers advance its
//! head only after the corresponding key mutation exists. Returned metadata
//! is detached and grants no current authority, possession or native effect.

use kontor_core::DomainError;
use kontor_core::id::{ContentHash, ExternalId};
use kontor_core::repository::{
    AttestationAuthorityProjection, AttestationAuthorityRepository, AttestationAuthorityScope,
    RegisterAttestationKey, RepositoryError, RepositoryResult, StoredAttestationKey,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::{SqliteStore, backend, conflict};

const SUBJECT: &str = "attestation key authority";
type KeyRow = (String, String, Vec<u8>, String, i64, i64, i64, Option<i64>);

fn validate_id(id: &ExternalId) -> RepositoryResult<()> {
    if id.as_str().len() > 256 {
        return Err(DomainError::invalid(SUBJECT, "identifier exceeds 256 UTF-8 bytes").into());
    }
    Ok(())
}

fn sqlite_integer(value: u64) -> RepositoryResult<i64> {
    i64::try_from(value)
        .map_err(|_| DomainError::invalid(SUBJECT, "integer exceeds SQLite range").into())
}

fn validate_scope(scope: &AttestationAuthorityScope) -> RepositoryResult<()> {
    validate_id(&scope.application)
}

fn validate_expected(expected: Option<u64>) -> RepositoryResult<()> {
    if let Some(revision) = expected {
        sqlite_integer(revision)?;
        if revision == 0 {
            return Err(DomainError::invalid(SUBJECT, "head revision must be positive").into());
        }
    }
    Ok(())
}

fn head_in(
    connection: &Connection,
    scope: &AttestationAuthorityScope,
) -> RepositoryResult<Option<u64>> {
    let revision: Option<i64> = connection.query_row(
        "SELECT revision FROM attestation_authority_heads WHERE project_id = ?1 AND application = ?2",
        params![scope.project_id.to_string(), scope.application.as_str()], |row| row.get(0),
    ).optional().map_err(backend)?;
    revision
        .map(|revision| {
            u64::try_from(revision).map_err(|_| conflict(SUBJECT, "invalid stored head revision"))
        })
        .transpose()
}

fn next_revision(head: Option<u64>) -> RepositoryResult<i64> {
    head.unwrap_or(0)
        .checked_add(1)
        .filter(|revision| *revision <= i64::MAX as u64)
        .map(|revision| revision as i64)
        .ok_or_else(|| conflict(SUBJECT, "head revision overflow"))
}

fn require_head(actual: Option<u64>, expected: Option<u64>) -> RepositoryResult<()> {
    if actual != expected {
        return Err(conflict(SUBJECT, "expected head revision does not match"));
    }
    Ok(())
}

fn key_from_row(row: KeyRow) -> RepositoryResult<StoredAttestationKey> {
    let (issuer, key_id, der, digest, start, end, registered, revoked) = row;
    let digest = ContentHash::parse(&digest)?;
    if digest != ContentHash::of(&der) {
        return Err(conflict(SUBJECT, "stored material digest mismatch"));
    }
    let unsigned =
        |value| u64::try_from(value).map_err(|_| conflict(SUBJECT, "invalid stored integer"));
    Ok(StoredAttestationKey {
        issuer: ExternalId::parse(&issuer)?,
        key_id: ExternalId::parse(&key_id)?,
        public_key_der: der,
        material_digest: digest,
        not_before: unsigned(start)?,
        expires_at: unsigned(end)?,
        registered_revision: unsigned(registered)?,
        revoked_revision: revoked.map(unsigned).transpose()?,
    })
}

fn projection_in(
    connection: &Connection,
    scope: &AttestationAuthorityScope,
    issuer: &ExternalId,
    key_id: &ExternalId,
) -> RepositoryResult<Option<AttestationAuthorityProjection>> {
    let row: Option<(i64, Option<KeyRow>)> = connection
        .query_row(
            "SELECT h.revision, k.issuer, k.key_id, k.public_key_der, k.material_digest,
                k.not_before, k.expires_at, k.registered_revision, k.revoked_revision
           FROM attestation_authority_heads h LEFT JOIN attestation_authority_keys k
             ON k.project_id = h.project_id AND k.application = h.application
            AND k.issuer = ?3 AND k.key_id = ?4
          WHERE h.project_id = ?1 AND h.application = ?2",
            params![
                scope.project_id.to_string(),
                scope.application.as_str(),
                issuer.as_str(),
                key_id.as_str()
            ],
            |row| {
                let issuer: Option<String> = row.get(1)?;
                let key = if let Some(issuer) = issuer {
                    Some((
                        issuer,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                } else {
                    None
                };
                Ok((row.get(0)?, key))
            },
        )
        .optional()
        .map_err(backend)?;
    row.map(|(revision, key)| {
        Ok(AttestationAuthorityProjection {
            scope: scope.clone(),
            head_revision: u64::try_from(revision)
                .map_err(|_| conflict(SUBJECT, "invalid stored head revision"))?,
            selected_key: key.map(key_from_row).transpose()?,
        })
    })
    .transpose()
}

impl SqliteStore {
    fn require_attestation_scope(
        &self,
        transaction: &Transaction<'_>,
        scope: &AttestationAuthorityScope,
    ) -> RepositoryResult<()> {
        if scope.realm_id != self.realm_id() {
            return Err(RepositoryError::NotFound { subject: SUBJECT });
        }
        let exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
                [scope.project_id.to_string()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if !exists {
            return Err(RepositoryError::NotFound { subject: SUBJECT });
        }
        Ok(())
    }
}

impl AttestationAuthorityRepository for SqliteStore {
    fn register_attestation_key(
        &self,
        request: &RegisterAttestationKey,
    ) -> RepositoryResult<AttestationAuthorityProjection> {
        validate_scope(&request.scope)?;
        validate_id(&request.issuer)?;
        validate_id(&request.key_id)?;
        validate_expected(request.expected_head_revision)?;
        if request.public_key_der.is_empty()
            || request.public_key_der.len() > 2048
            || request.not_before >= request.expires_at
        {
            return Err(DomainError::invalid(
                SUBJECT,
                "invalid public material or validity interval",
            )
            .into());
        }
        let start = sqlite_integer(request.not_before)?;
        let end = sqlite_integer(request.expires_at)?;
        let transaction = self.begin()?;
        self.require_attestation_scope(&transaction, &request.scope)?;
        let head = head_in(&transaction, &request.scope)?;
        require_head(head, request.expected_head_revision)?;
        let next = next_revision(head)?;
        if projection_in(
            &transaction,
            &request.scope,
            &request.issuer,
            &request.key_id,
        )?
        .is_some_and(|projection| projection.selected_key.is_some())
        {
            return Err(conflict(
                SUBJECT,
                "issuer/key identity is already registered",
            ));
        }
        transaction
            .execute(
                "INSERT INTO attestation_authority_keys
             (project_id, application, issuer, key_id, public_key_der, material_digest,
              not_before, expires_at, registered_revision, revoked_revision)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL)",
                params![
                    request.scope.project_id.to_string(),
                    request.scope.application.as_str(),
                    request.issuer.as_str(),
                    request.key_id.as_str(),
                    &request.public_key_der,
                    ContentHash::of(&request.public_key_der).as_str(),
                    start,
                    end,
                    next
                ],
            )
            .map_err(backend)?;
        let result = projection_in(
            &transaction,
            &request.scope,
            &request.issuer,
            &request.key_id,
        )?
        .ok_or_else(|| conflict(SUBJECT, "registered key projection is absent"))?;
        transaction.commit().map_err(backend)?;
        Ok(result)
    }

    fn revoke_attestation_key(
        &self,
        scope: &AttestationAuthorityScope,
        issuer: &ExternalId,
        key_id: &ExternalId,
        expected_head_revision: u64,
    ) -> RepositoryResult<AttestationAuthorityProjection> {
        validate_scope(scope)?;
        validate_id(issuer)?;
        validate_id(key_id)?;
        validate_expected(Some(expected_head_revision))?;
        let transaction = self.begin()?;
        self.require_attestation_scope(&transaction, scope)?;
        let head = head_in(&transaction, scope)?;
        require_head(head, Some(expected_head_revision))?;
        let current = projection_in(&transaction, scope, issuer, key_id)?
            .ok_or(RepositoryError::NotFound { subject: SUBJECT })?;
        let key = current
            .selected_key
            .as_ref()
            .ok_or(RepositoryError::NotFound { subject: SUBJECT })?;
        if key.revoked_revision.is_none() {
            let next = next_revision(head)?;
            transaction
                .execute(
                    "UPDATE attestation_authority_keys SET revoked_revision = ?5
                   WHERE project_id = ?1 AND application = ?2 AND issuer = ?3 AND key_id = ?4
                     AND revoked_revision IS NULL",
                    params![
                        scope.project_id.to_string(),
                        scope.application.as_str(),
                        issuer.as_str(),
                        key_id.as_str(),
                        next
                    ],
                )
                .map_err(backend)?;
        }
        let result = projection_in(&transaction, scope, issuer, key_id)?
            .ok_or_else(|| conflict(SUBJECT, "revoked key projection is absent"))?;
        transaction.commit().map_err(backend)?;
        Ok(result)
    }

    fn read_attestation_key_authority(
        &self,
        scope: &AttestationAuthorityScope,
        issuer: &ExternalId,
        key_id: &ExternalId,
    ) -> RepositoryResult<Option<AttestationAuthorityProjection>> {
        validate_scope(scope)?;
        validate_id(issuer)?;
        validate_id(key_id)?;
        if scope.realm_id != self.realm_id() {
            return Ok(None);
        }
        projection_in(&self.connection, scope, issuer, key_id)
    }
}
