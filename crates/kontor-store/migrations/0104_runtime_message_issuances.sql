-- Schema v104. Durable ledger of the client message ids Kontor itself issued.
--
-- Observing a current turn has to answer two different questions: *which* turn
-- is current, and whether the message naming it is unambiguous. The second was
-- answered by reading the session's canonical content from its origin and
-- counting occurrences of the id. That read costs a request per page of
-- transcript, so proving uniqueness is what made observation unbounded — and a
-- seat with a long transcript could not be observed at all, which is how two
-- scope runs ended up unsettleable while their sessions read perfectly well.
--
-- Uniqueness is not really a fact about the transcript, though. It is a fact
-- about what Kontor sent. Every client message id this realm puts into a
-- session is minted here, so recording the issuance makes "was this id issued
-- exactly once, to exactly this binding?" a primary-key lookup instead of a
-- scan, and the transcript read shrinks to the bounded tail window that proves
-- the occurrence and its terminal response.
--
-- `message_id` is the primary key rather than a column of some wider key: that
-- *is* the exactly-once guarantee. One id, one issuance, one binding. A second
-- issuance of the same id to a different session is refused by the database
-- rather than detected afterwards, and a replay under the same idempotency key
-- recognises its own row instead of writing a second one.
--
-- Nothing is backfilled. A message issued before this table existed has no row
-- and never will, and observation refuses it with an action rather than
-- guessing: an operator sends one new correlation message, which is cheap, and
-- the alternative would be trusting a caller's claim about content the realm
-- cannot vouch for.

CREATE TABLE runtime_message_issuances (
    message_id         TEXT NOT NULL PRIMARY KEY
                            CHECK (length(message_id) = 36
                                   AND message_id NOT GLOB '*[^0-9a-f-]*'),
    runtime_kind       TEXT NOT NULL CHECK (length(runtime_kind) BETWEEN 1 AND 128),
    host               TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 512),
    runtime_binding_id TEXT NOT NULL
                            CHECK (length(runtime_binding_id) = 36
                                   AND runtime_binding_id NOT GLOB '*[^0-9a-f-]*'),
    native_session_id  TEXT NOT NULL CHECK (length(native_session_id) BETWEEN 1 AND 256),
    idempotency_key    TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    -- Which path issued it. Both are Kontor writing into a session: an operator
    -- send, and the follow-up a settled turn derives for the next role slot.
    provenance         TEXT NOT NULL
                            CHECK (provenance IN ('session_message_send', 'handoff_dispatch')),
    issued_at          TEXT NOT NULL
                            CHECK (issued_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

-- The lookup observation makes: this binding's issuance of this exact id.
CREATE INDEX runtime_message_issuances_by_binding
    ON runtime_message_issuances (runtime_binding_id, message_id);

PRAGMA user_version = 104;
