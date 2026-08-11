//! The one continuity and idempotency policy shared by every adapter.
//!
//! Session content is only useful if it is *exactly once*. Three rules give
//! that, and they live here so a real adapter and the scripted fake cannot
//! disagree about them:
//!
//! 1. **History anchors live.** A page-by-page read validates monotonicity and
//!    page continuity and ends at a position; live delivery starts strictly
//!    after that position.
//! 2. **A suspect stream stops.** An epoch change or a forward sequence gap
//!    breaks the timeline permanently and demands a refetch. Continuing would
//!    hand the control plane a hole it cannot see.
//! 3. **An identifier is the effect.** A retried message or permission response
//!    replays its original result instead of committing a second effect.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use kontor_core::id::{CanonicalDocument, ContentHash, ExternalId, RuntimeBindingId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::adapter::{PermissionAck, RuntimeError, RuntimeResult};
use crate::request::{MessageId, PermissionDecision};

/// A kind of thing that happens inside a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEventKind {
    /// Content produced by, or delivered to, the session.
    Message,
    /// The session invoked a tool.
    ToolCall,
    /// The session is waiting for a permission decision.
    PermissionRequest,
    /// A permission request was answered.
    PermissionResolved,
    /// The session reported a lifecycle change.
    StateChange,
    /// Diagnostic output.
    Log,
}

/// What one event is *about*, when it is about something addressable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventSubject {
    /// Nothing addressable.
    None,
    /// The runtime's own permission request id.
    Permission(ExternalId),
    /// A Kontor-generated message id. A native id never appears here.
    Message(MessageId),
}

/// A position in one session's content, inside one epoch.
///
/// Sequences are 1-based and contiguous inside an epoch. An epoch change means
/// the runtime cannot promise the old numbering any more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TimelinePosition {
    /// The numbering generation of the session's content.
    pub epoch: u64,
    /// The position inside that epoch.
    pub sequence: u64,
}

impl TimelinePosition {
    /// The anchor that precedes every event of `epoch`.
    #[must_use]
    pub const fn start_of(epoch: u64) -> Self {
        Self { epoch, sequence: 0 }
    }
}

impl fmt::Display for TimelinePosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.epoch, self.sequence)
    }
}

/// One event of session content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEvent {
    /// What kind of thing happened.
    pub kind: SessionEventKind,
    /// Where it sits in the session's content.
    pub position: TimelinePosition,
    /// What it is about, when that is addressable.
    pub subject: EventSubject,
    /// The runtime's own event id, when it provides one.
    pub native_event_id: Option<ExternalId>,
    /// When the runtime emitted it.
    pub emitted_at: Timestamp,
    /// The canonical payload.
    pub payload: CanonicalDocument,
}

impl SessionEvent {
    /// The digest that decides whether a repeated position is a replay or a
    /// contradiction.
    #[must_use]
    pub const fn digest(&self) -> &ContentHash {
        self.payload.hash()
    }
}

/// Why a timeline can no longer be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimelineBreak {
    /// The runtime renumbered its content.
    EpochChanged,
    /// One or more events between the last accepted position and this one were
    /// never delivered.
    SequenceGap,
    /// The same position arrived twice with different content.
    ConflictingDuplicate,
}

impl TimelineBreak {
    /// The stable spelling used in errors and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EpochChanged => "the runtime renumbered its content",
            Self::SequenceGap => "events are missing before this one",
            Self::ConflictingDuplicate => "the same position arrived with different content",
        }
    }
}

impl fmt::Display for TimelineBreak {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What accepting one event did to the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineStep {
    /// The event was the next one; the position advanced.
    Accepted,
    /// The event was a replay of something already seen; nothing advanced.
    DuplicateIgnored,
}

/// The continuity guard both history and live delivery run through.
///
/// Once broken it stays broken: every later call returns the same refusal, so a
/// caller cannot keep draining a stream it has already been told to refetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineGuard {
    start: TimelinePosition,
    position: TimelinePosition,
    /// Every digest this guard has accepted, kept for its whole lifetime.
    ///
    /// Retaining only the newest digest would make the guard unable to tell a
    /// redelivery of an older position from a *rewrite* of it: everything
    /// behind the cursor would look benign. The map is bounded by the events
    /// one reader or one subscription validates, which is the same content it
    /// is already handing to its caller.
    digests: BTreeMap<u64, ContentHash>,
    broken: Option<TimelineBreak>,
}

impl TimelineGuard {
    /// A guard that accepts the event strictly after `position`.
    #[must_use]
    pub fn starting_after(position: TimelinePosition) -> Self {
        Self {
            start: position,
            position,
            digests: BTreeMap::new(),
            broken: None,
        }
    }

    /// The last position this guard accepted.
    #[must_use]
    pub const fn position(&self) -> TimelinePosition {
        self.position
    }

    /// Whether the timeline has been refused and must be refetched.
    #[must_use]
    pub const fn is_broken(&self) -> bool {
        self.broken.is_some()
    }

    /// Validate one event against the timeline.
    ///
    /// # Errors
    /// Returns [`RuntimeError::TimelineRefetchRequired`] on an epoch change, a
    /// forward gap, a contradictory duplicate at *any* position this guard has
    /// accepted, and on every call after any of those.
    pub fn accept(&mut self, event: &SessionEvent) -> RuntimeResult<TimelineStep> {
        if let Some(reason) = self.broken {
            return Err(RuntimeError::TimelineRefetchRequired { reason });
        }
        if event.position.epoch != self.position.epoch {
            return Err(self.break_with(TimelineBreak::EpochChanged));
        }
        if event.position.sequence <= self.start.sequence {
            // Behind where this guard began. Whatever belongs at that position
            // was validated before this guard existed, so it has no digest to
            // judge against and no way to advance trusted state either. Drop
            // it rather than pretend to an opinion.
            return Ok(TimelineStep::DuplicateIgnored);
        }
        if event.position.sequence <= self.position.sequence {
            // A position this guard already accepted: identical content is a
            // redelivery, different content means the runtime contradicted
            // itself — and it means that just as much three events back as it
            // does at the cursor.
            return match self.digests.get(&event.position.sequence) {
                Some(seen) if seen == event.digest() => Ok(TimelineStep::DuplicateIgnored),
                _ => Err(self.break_with(TimelineBreak::ConflictingDuplicate)),
            };
        }
        if event.position.sequence > self.position.sequence + 1 {
            return Err(self.break_with(TimelineBreak::SequenceGap));
        }
        self.position = event.position;
        self.digests
            .insert(event.position.sequence, event.digest().clone());
        Ok(TimelineStep::Accepted)
    }

    fn break_with(&mut self, reason: TimelineBreak) -> RuntimeError {
        self.broken = Some(reason);
        RuntimeError::TimelineRefetchRequired { reason }
    }
}

/// An opaque continuation token for one binding's history.
///
/// It is opaque to the caller but *bound* to a binding: a cursor from another
/// session, or one that has been edited, is refused rather than silently
/// treated as "start from the beginning".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HistoryCursor(String);

impl HistoryCursor {
    /// Issue a cursor for `position` inside `binding_id`.
    #[must_use]
    pub fn issue(binding_id: RuntimeBindingId, position: TimelinePosition) -> Self {
        Self(format!(
            "{binding_id}:{}:{}",
            position.epoch, position.sequence
        ))
    }

    /// Build a cursor from stored or transported text.
    #[must_use]
    pub fn from_text(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The opaque text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Resolve the cursor against the binding it must belong to.
    ///
    /// # Errors
    /// Returns [`RuntimeError::InvalidCursor`] for a malformed cursor and for a
    /// cursor issued for another binding.
    pub fn resolve(&self, binding_id: RuntimeBindingId) -> RuntimeResult<TimelinePosition> {
        let mut parts = self.0.rsplitn(3, ':');
        let sequence = parts.next().unwrap_or_default();
        let epoch = parts.next().unwrap_or_default();
        let owner = parts.next().unwrap_or_default();
        if owner != binding_id.to_string() {
            return Err(RuntimeError::InvalidCursor {
                rule: "was issued for another session",
            });
        }
        let (Ok(epoch), Ok(sequence)) = (epoch.parse::<u64>(), sequence.parse::<u64>()) else {
            return Err(RuntimeError::InvalidCursor {
                rule: "is not a position this runtime issued",
            });
        };
        Ok(TimelinePosition { epoch, sequence })
    }
}

/// One page of a session's recorded content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPage {
    /// The epoch every item on the page belongs to.
    pub epoch: u64,
    /// The items, in ascending sequence order.
    pub items: Vec<SessionEvent>,
    /// Where to continue, or `None` when the history is exhausted.
    pub next: Option<HistoryCursor>,
    /// The last position this page covers. An empty page keeps the anchor it
    /// started from.
    pub end: TimelinePosition,
}

/// A page-by-page history read that leaves a trustworthy live anchor behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryReader {
    binding_id: RuntimeBindingId,
    guard: TimelineGuard,
}

impl HistoryReader {
    /// Start reading `binding_id` from the beginning of `epoch`.
    #[must_use]
    pub fn start(binding_id: RuntimeBindingId, epoch: u64) -> Self {
        Self {
            binding_id,
            guard: TimelineGuard::starting_after(TimelinePosition::start_of(epoch)),
        }
    }

    /// Resume reading `binding_id` strictly after a position an earlier page
    /// ended at.
    ///
    /// A reader that could only start at the beginning of an epoch cannot
    /// validate a page fetched with a continuation cursor: the page's first item
    /// is legitimately far past sequence 0, and a from-the-start guard would call
    /// that a gap. The position comes from resolving the caller's own cursor
    /// against this binding, so a resumed reader still cannot be pointed at a
    /// position no runtime issued.
    #[must_use]
    pub fn resuming(binding_id: RuntimeBindingId, after: TimelinePosition) -> Self {
        Self {
            binding_id,
            guard: TimelineGuard::starting_after(after),
        }
    }

    /// Validate one page, drop anything already covered, and advance the
    /// anchor.
    ///
    /// The page is taken by `&mut` so a redelivered item cannot survive
    /// validation: on success `page.items` holds exactly the events that moved
    /// the timeline forward, in ascending order. Validating a page while
    /// leaving a duplicate in it would hand the caller content it was just told
    /// to ignore — "exactly once" has to be true of what comes out, not only of
    /// what the guard counted.
    ///
    /// # Errors
    /// * [`RuntimeError::TimelineRefetchRequired`] — the page changed epoch,
    ///   skipped a sequence, or contradicted a position it already delivered.
    /// * [`RuntimeError::InvalidCursor`] — the continuation cursor belongs to
    ///   another session or does not match where the page ended.
    pub fn accept_page(&mut self, page: &mut HistoryPage) -> RuntimeResult<()> {
        if page.epoch != self.guard.position().epoch {
            return Err(self.guard.break_with(TimelineBreak::EpochChanged));
        }
        let mut accepted = Vec::with_capacity(page.items.len());
        for item in &page.items {
            if self.guard.accept(item)? == TimelineStep::Accepted {
                accepted.push(item.clone());
            }
        }
        page.items = accepted;
        if let Some(cursor) = &page.next {
            let resolved = cursor.resolve(self.binding_id)?;
            if resolved != self.guard.position() {
                return Err(RuntimeError::InvalidCursor {
                    rule: "does not continue where the page ended",
                });
            }
        }
        Ok(())
    }

    /// The position live delivery must start strictly after.
    #[must_use]
    pub const fn anchor(&self) -> TimelinePosition {
        self.guard.position()
    }
}

/// Whether a caller-supplied identifier is a new effect or a replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission<T> {
    /// Nothing was recorded under this identifier yet.
    New,
    /// The identifier already committed this exact effect.
    Replay(T),
}

/// The idempotency ledger for everything a caller pushes into a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageLedger<T> {
    entries: BTreeMap<MessageId, LedgerEntry<T>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LedgerEntry<T> {
    body_hash: ContentHash,
    result: T,
}

impl<T> Default for MessageLedger<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
}

impl<T: Clone> MessageLedger<T> {
    /// An empty ledger.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many distinct effects this ledger has committed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the ledger has committed anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Decide whether an identifier may commit a new effect.
    ///
    /// # Errors
    /// Returns [`RuntimeError::DuplicateMessage`] when the same identifier
    /// arrives with different content, which is a caller bug rather than a
    /// retry.
    pub fn admit(
        &mut self,
        message_id: &MessageId,
        body_hash: &ContentHash,
    ) -> RuntimeResult<Admission<T>> {
        match self.entries.get(message_id) {
            None => Ok(Admission::New),
            Some(entry) if &entry.body_hash == body_hash => {
                Ok(Admission::Replay(entry.result.clone()))
            }
            Some(_) => Err(RuntimeError::DuplicateMessage {
                rule: "was already used for different content",
            }),
        }
    }

    /// Record the effect an identifier committed.
    pub fn record(&mut self, message_id: MessageId, body_hash: ContentHash, result: T) {
        self.entries
            .insert(message_id, LedgerEntry { body_hash, result });
    }
}

/// The permission requests one session has raised and resolved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionLedger {
    pending: BTreeMap<ExternalId, RuntimeBindingId>,
    resolved: BTreeMap<ExternalId, PermissionAck>,
}

impl PermissionLedger {
    /// An empty ledger.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `binding_id`'s session is waiting on `permission_id`.
    ///
    /// The first session to raise a request owns it. Re-opening never transfers
    /// ownership, so re-reading a session's content cannot quietly hand another
    /// session the right to answer.
    pub fn open(&mut self, binding_id: RuntimeBindingId, permission_id: ExternalId) {
        if self.resolved.contains_key(&permission_id) {
            return;
        }
        self.pending.entry(permission_id).or_insert(binding_id);
    }

    /// Every request still waiting for an answer.
    #[must_use]
    pub fn pending(&self) -> BTreeSet<ExternalId> {
        self.pending.keys().cloned().collect()
    }

    /// Decide whether an answer may be applied.
    ///
    /// # Errors
    /// Returns [`RuntimeError::PermissionConflict`] for an unknown request, a
    /// request raised by another session, a second answer under a different
    /// response id, and the same response id carrying a different answer.
    pub fn classify(
        &self,
        binding_id: RuntimeBindingId,
        permission_id: &ExternalId,
        response_id: MessageId,
        decision: PermissionDecision,
    ) -> RuntimeResult<Admission<PermissionAck>> {
        if let Some(existing) = self.resolved.get(permission_id) {
            if existing.binding_id != binding_id {
                return Err(RuntimeError::PermissionConflict {
                    rule: "belongs to another session",
                });
            }
            if existing.response_id != response_id {
                return Err(RuntimeError::PermissionConflict {
                    rule: "was already resolved by a different response",
                });
            }
            if existing.decision != decision {
                return Err(RuntimeError::PermissionConflict {
                    rule: "was already resolved with a different answer",
                });
            }
            return Ok(Admission::Replay(existing.clone()));
        }
        match self.pending.get(permission_id) {
            None => Err(RuntimeError::PermissionConflict {
                rule: "is unknown to this runtime",
            }),
            Some(owner) if *owner != binding_id => Err(RuntimeError::PermissionConflict {
                rule: "belongs to another session",
            }),
            Some(_) => Ok(Admission::New),
        }
    }

    /// Record the answer an identifier committed.
    pub fn record(&mut self, permission_id: ExternalId, acknowledgement: PermissionAck) {
        self.pending.remove(&permission_id);
        self.resolved.insert(permission_id, acknowledgement);
    }
}

/// Reduce a run of session content into the permission requests still open.
///
/// Content survives paging and reconnection, so a permission raised in history
/// is still pending after a live subscription takes over.
#[must_use]
pub fn pending_permissions<'a>(
    events: impl IntoIterator<Item = &'a SessionEvent>,
) -> BTreeSet<ExternalId> {
    let mut open = BTreeSet::new();
    for event in events {
        let EventSubject::Permission(id) = &event.subject else {
            continue;
        };
        match event.kind {
            SessionEventKind::PermissionRequest => {
                open.insert(id.clone());
            }
            SessionEventKind::PermissionResolved => {
                open.remove(id);
            }
            _ => {}
        }
    }
    open
}

/// A live subscription that validates continuity over every event and delivers
/// only the kinds the caller selected.
///
/// Filtering happens *after* validation on purpose: if selection could hide
/// events, a caller's own filter would look exactly like a runtime dropping
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSubscription {
    kinds: BTreeSet<SessionEventKind>,
    guard: TimelineGuard,
    queued: VecDeque<SessionEvent>,
    closed_without_terminal: bool,
}

impl LiveSubscription {
    /// A subscription starting strictly after `strict_after`.
    #[must_use]
    pub fn new(
        kinds: BTreeSet<SessionEventKind>,
        strict_after: TimelinePosition,
        events: impl IntoIterator<Item = SessionEvent>,
        closed_without_terminal: bool,
    ) -> Self {
        Self {
            kinds,
            guard: TimelineGuard::starting_after(strict_after),
            queued: events.into_iter().collect(),
            closed_without_terminal,
        }
    }

    /// The next selected event, a refusal, or `None` when the stream ends.
    ///
    /// # Errors
    /// Returns [`RuntimeError::TimelineRefetchRequired`] once the stream breaks,
    /// and on every later call.
    pub fn next_event(&mut self) -> Option<RuntimeResult<SessionEvent>> {
        while let Some(event) = self.queued.pop_front() {
            match self.guard.accept(&event) {
                Err(error) => return Some(Err(error)),
                Ok(TimelineStep::DuplicateIgnored) => {}
                Ok(TimelineStep::Accepted) => {
                    if self.kinds.contains(&event.kind) {
                        return Some(Ok(event));
                    }
                }
            }
        }
        None
    }

    /// The last position the subscription validated.
    #[must_use]
    pub const fn position(&self) -> TimelinePosition {
        self.guard.position()
    }

    /// Whether the stream ended without the session reaching a terminal state.
    ///
    /// A closed stream is a fact about the channel. It is never a completion.
    #[must_use]
    pub const fn closed_without_terminal(&self) -> bool {
        self.closed_without_terminal
    }
}

#[cfg(test)]
mod tests {
    use kontor_core::id::parse_utc_timestamp;

    use super::*;

    fn event(sequence: u64, body: &str) -> SessionEvent {
        SessionEvent {
            kind: SessionEventKind::Message,
            position: TimelinePosition { epoch: 1, sequence },
            subject: EventSubject::None,
            native_event_id: None,
            emitted_at: parse_utc_timestamp("2026-08-10T09:00:00Z").expect("canonical UTC"),
            payload: CanonicalDocument::from_serializable(&serde_json::json!({
                "schema_version": 1,
                "sequence": sequence,
                "body": body,
            }))
            .expect("canonical payload"),
        }
    }

    #[test]
    fn a_gap_breaks_the_timeline_permanently() {
        let mut guard = TimelineGuard::starting_after(TimelinePosition::start_of(1));
        assert_eq!(
            guard.accept(&event(1, "a")).expect("first event"),
            TimelineStep::Accepted
        );
        assert!(guard.accept(&event(3, "c")).is_err());
        // The next contiguous event must not resurrect a suspect stream.
        assert!(guard.accept(&event(2, "b")).is_err());
        assert!(guard.is_broken());
    }

    #[test]
    fn an_exact_replay_is_dropped_and_a_contradiction_is_refused() {
        let mut guard = TimelineGuard::starting_after(TimelinePosition::start_of(1));
        guard.accept(&event(1, "a")).expect("first event");
        assert_eq!(
            guard.accept(&event(1, "a")).expect("replay"),
            TimelineStep::DuplicateIgnored
        );
        assert_eq!(guard.position().sequence, 1);
        assert!(guard.accept(&event(1, "different")).is_err());
    }

    #[test]
    fn a_rewrite_of_an_older_position_is_refused_too() {
        let mut guard = TimelineGuard::starting_after(TimelinePosition::start_of(1));
        for (sequence, body) in [(1, "a"), (2, "b"), (3, "c")] {
            guard.accept(&event(sequence, body)).expect("in order");
        }
        // Three positions back, redelivered unchanged: benign.
        assert_eq!(
            guard.accept(&event(1, "a")).expect("an old redelivery"),
            TimelineStep::DuplicateIgnored
        );
        assert_eq!(guard.position().sequence, 3);
        // The same position with other content is the runtime contradicting
        // itself, however far behind the cursor it happens.
        assert_eq!(
            guard
                .accept(&event(1, "rewritten"))
                .expect_err("history behind the cursor was rewritten"),
            RuntimeError::TimelineRefetchRequired {
                reason: TimelineBreak::ConflictingDuplicate
            }
        );
        assert!(guard.is_broken());
    }

    #[test]
    fn a_position_from_before_the_guard_started_is_dropped_not_judged() {
        // A live subscription starts after a validated history anchor. It holds
        // no digest for what history covered, so a redelivery from there is
        // dropped rather than called a contradiction.
        let mut guard = TimelineGuard::starting_after(TimelinePosition {
            epoch: 1,
            sequence: 2,
        });
        guard.accept(&event(3, "c")).expect("the next event");
        assert_eq!(
            guard
                .accept(&event(2, "whatever history had"))
                .expect("behind the anchor"),
            TimelineStep::DuplicateIgnored
        );
        assert_eq!(guard.position().sequence, 3);
        assert!(!guard.is_broken());
    }

    #[test]
    fn a_validated_page_carries_no_duplicate_out() {
        let binding_id = RuntimeBindingId::generate();
        let mut reader = HistoryReader::start(binding_id, 1);
        let mut page = HistoryPage {
            epoch: 1,
            items: vec![event(1, "a"), event(2, "b"), event(2, "b")],
            next: None,
            end: TimelinePosition {
                epoch: 1,
                sequence: 2,
            },
        };
        reader.accept_page(&mut page).expect("the page validates");
        assert_eq!(
            page.items
                .iter()
                .map(|it| it.position.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2],
            "the redelivered item is gone from the page, not merely uncounted"
        );
        assert_eq!(reader.anchor().sequence, 2);

        // A page that contradicts a position it already delivered is refused.
        let mut contradicting = HistoryPage {
            epoch: 1,
            items: vec![event(3, "c"), event(2, "rewritten")],
            next: None,
            end: TimelinePosition {
                epoch: 1,
                sequence: 3,
            },
        };
        assert_eq!(
            reader
                .accept_page(&mut contradicting)
                .expect_err("the page rewrites an item it already returned"),
            RuntimeError::TimelineRefetchRequired {
                reason: TimelineBreak::ConflictingDuplicate
            }
        );
    }

    #[test]
    fn a_cursor_from_another_binding_is_refused() {
        let mine = RuntimeBindingId::generate();
        let theirs = RuntimeBindingId::generate();
        let cursor = HistoryCursor::issue(
            mine,
            TimelinePosition {
                epoch: 1,
                sequence: 4,
            },
        );
        assert_eq!(
            cursor.resolve(mine).expect("my own cursor resolves"),
            TimelinePosition {
                epoch: 1,
                sequence: 4
            }
        );
        assert!(cursor.resolve(theirs).is_err());
    }
}
