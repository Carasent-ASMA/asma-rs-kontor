# KON-OP-03 — refusal envelope inventory

The complaint: a session-message refusal came back as
`"the request was refused by a domain rule"` and nothing else. The code was
right and the envelope was honest, but five different domain variants shared one
sentence, so an operator could not tell a malformed field from a state machine
saying no.

## What changed

`ApiErrorBody` gains three additive fields. The `code` and `rule` fields are
untouched in name and type, and no code moved.

| Field | Content | Why it is safe |
| --- | --- | --- |
| `subject` | The type, field or state machine that refused | Always a `&'static str` written in this workspace — a type or operation name, never a stored value |
| `at` | Structural path of the offending node | A path (`slots[2].role`), never what was in it |
| `action` | One line of corrective advice | Static text; every code has a floor via `ApiErrorCode::default_action` |

## Every `DomainError` variant, mapped deliberately

| Variant | Code (unchanged) | `rule` | Extra |
| --- | --- | --- | --- |
| `Invalid` | `invalid_request` | a value did not satisfy the invariant of its type | `subject` |
| `InvalidAt` | `invalid_request` | a value inside the document did not satisfy its invariant | `subject`, `at` |
| `IllegalTransition` | `invalid_request` | the aggregate does not accept this transition from the state it is in | `subject`; advice differs when `from == to` |
| `MissingEvidence` | `invalid_request` | the operation requires evidence that has not been recorded | `subject` |
| `SensitiveMaterial` | `invalid_request` | the document carries credential, token or unredacted personal material | `at` only — the value is never echoed |
| `Terminal` | `revision_conflict` | the aggregate is terminal and immutable | `subject` |
| `RevisionConflict` | `revision_conflict` | the aggregate moved since the caller read it | `subject`, `current_revision` |
| `MissingAuthority` | `forbidden` | the acting authority is not sufficient for this operation | `subject` |
| `RealmMismatch` | `realm_mismatch` | the value belongs to another realm | — |

The five previously collapsed variants are the first five rows.

## Catch-alls found

| Site | Disposition |
| --- | --- |
| `ApiError::from_domain` | **Fixed.** Was one arm for five variants. The remaining `_` arm exists only because `DomainError` is `#[non_exhaustive]`; it now says *"a domain rule refused the request and this build cannot classify it"* and advises reading the daemon log and upgrading |
| `ApiError::from_repository` | **Fixed.** Its `_` arm said "the control-plane store refused the operation" with nothing logged. It now says classification is unavailable, logs the detail at `warn`, and advises where to look |
| `ApiError::from_runtime` | **Fixed.** Already logged the detail; the answer now says classification is unavailable rather than implying a mapped refusal |
| `RepositoryError::Conflict` | **Justified.** One sentence for every uniqueness and immutability rule in the store is deliberate — a client could otherwise enumerate them. The subject and rule are logged |
| `RepositoryError::Backend` | **Justified.** "The store could not answer" is the whole truth; the detail is a backend message and does not belong on the wire |
| Route-level refusals in `applications.rs` | **Justified.** Audited: every repeated string names its aggregate and situation (`no such project exists in this realm`, `the epic moved since the caller read it`, `the Completion service is not composed in this build`). No generic catch-all remains at route level |

## The sensitive-text boundary bug

`has_marker` matched a credential prefix anywhere in the string. `sk-` therefore
matched inside `ta`**sk-**`scoped` and `ri`**sk-**`free`, so an ordinary
hyphenated English sentence long enough to clear the 24-character tail bound was
refused as an OpenAI key. Confirmed against the brief that triggered it: two
hits, both mid-word, both `sk-`.

A credential prefix is the start of a token. The match must now begin where the
preceding character is not alphanumeric — a rule rather than a whitelist, so
`=`, `:`, `"`, `/`, `-`, `_` and whitespace all still open a token, which is
where credentials actually appear.

Regression tests run both directions:

- **allow** — five prose phrases embedding `sk-`, `akia` and `asia` mid-word,
  through `reject_sensitive_text` and `BoundedText`;
- **reject** — the same markers at a real boundary, opened by nothing,
  whitespace, `=`, a quote, a path separator and `:`, each asserted to be
  `SensitiveMaterial` and each asserted not to be echoed.

### Residual, explicitly justified

`("basic-", 20)` and `("bearer-", 20)` are separator variants of the HTTP auth
schemes, added so a token written `Basic-<base64>` is caught. They match at a
genuine token boundary, so the boundary fix does not affect them — and
`basic-hygiene` in prose is still refused. Left as-is: removing them narrows
*detection*, which is a different decision from fixing a *boundary*, and this
brief asked for the second. Flagged here rather than changed quietly.

## Tests

`crates/kontor-api/tests/error_envelope.rs` — five tests:

- no variant this build knows returns the unclassified sentence, every one
  carries an action, and no two variants are reported identically;
- the codes a client branches on are unchanged, asserted per variant;
- `subject` and `at` are populated and no credential-shaped text appears in the
  serialized body;
- "already in that state" and "cannot go there" advise differently;
- every `ApiErrorCode` has a non-trivial default action.

`crates/kontor-core/tests/spec_validation.rs` —
`a_credential_prefix_must_begin_at_a_token_boundary`.
