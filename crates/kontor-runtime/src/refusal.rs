//! A transient, provider-neutral diagnostic for text a runtime refused with.
//!
//! # Why this is not [`kontor_api::error::NativeRuntimeRefusal`]
//!
//! That type is a **closed, non-secret command-refusal envelope**: it says why
//! *Kontor's own request* was turned down, in a vocabulary Kontor defines, and
//! its invariant is that arbitrary runtime text never enters it. Widening it to
//! carry a vendor's sentence would delete that invariant for every caller of the
//! API, to serve one classifier.
//!
//! This type is the opposite in every dimension that matters, and deliberately
//! so:
//!
//! * it lives in the runtime layer, not the API layer, and no API response can
//!   name it;
//! * it is **not serializable** — there is no `Serialize`, no `Deserialize`, and
//!   no `Display`, so it cannot be written into canonical evidence, a stored
//!   payload, an event or a response by accident;
//! * its [`fmt::Debug`] is redacted, so a structure that merely *contains* one —
//!   [`crate::observation::ControlPlaneObservation`] does — cannot leak it
//!   through a log line or a panic message;
//! * it is bounded on construction and refused outright when the core
//!   sensitive-material rule matches.
//!
//! It exists only long enough to be classified. Nothing downstream keeps it: the
//! daemon reads it, derives a structured classification and a digest, and drops
//! the value.

use std::fmt;

use kontor_core::id::{AgentRunId, ContentHash, reject_sensitive_text};

use crate::timeline::TimelinePosition;

/// The most text a transient refusal may carry.
///
/// A provider refusal is a sentence, not a transcript. This bound is what stops
/// an adapter turning "the turn ended oddly" into an unbounded copy of the
/// session, and it is enforced here rather than trusted to each caller.
pub const MAX_REFUSAL_CHARS: usize = 2_000;

/// The largest candidate this type will even look at.
///
/// The sensitive-material scan runs over the whole candidate before it is
/// bounded, so the candidate itself has to be bounded first. Anything past this
/// is refused rather than scanned: the Paseo probe caps what it collects far
/// below this, and a caller offering more is not describing a turn's last
/// words.
pub const MAX_CANDIDATE_BYTES: usize = 64 * 1024;

/// Where a candidate refusal came from, exactly.
///
/// Persisted as part of the digest so a succession can prove which item, on
/// which run, authorized it. "Some prose that contained these words" is not
/// provenance: the canonical range, the native type and the owning run are.
/// Carrying the run and generation is what stops evidence being transplanted
/// onto a sibling seat or a previous binding generation, and carrying the full
/// range is what stops a collapsed multi-sequence entry being mistaken for a
/// single one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusalProvenance {
    /// The run whose session carried the item.
    pub agent_run_id: AgentRunId,
    /// The immutable binding generation that run was on.
    pub binding_generation: u64,
    /// Canonical position of the item's first sequence.
    pub position: TimelinePosition,
    /// The item's last native sequence. Equal to `position.sequence` unless the
    /// runtime collapsed a range into one entry.
    pub sequence_end: u64,
    /// Exactly which native sequences the entry covers, canonicalized: sorted,
    /// deduplicated, `(start, end)` inclusive.
    pub source_sequences: Vec<(u64, u64)>,
    /// The runtime's own item type, as it spelled it.
    pub item_type: String,
}

impl RefusalProvenance {
    /// The canonical rendering that enters the digest.
    fn canonical(&self) -> String {
        let ranges: Vec<String> = self
            .source_sequences
            .iter()
            .map(|(start, end)| format!("{start}-{end}"))
            .collect();
        format!(
            "v2\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.agent_run_id,
            self.binding_generation,
            self.position.epoch,
            self.position.sequence,
            self.sequence_end,
            ranges.join(","),
            self.item_type,
        )
    }
}

/// Text a runtime appears to have ended a turn with, and the item it came from.
///
/// Construct with [`TransientRefusal::parse`]. There is no other constructor,
/// and no way to recover the text except [`TransientRefusal::as_str`].
#[derive(Clone, PartialEq, Eq)]
pub struct TransientRefusal {
    text: String,
    provenance: RefusalProvenance,
}

impl TransientRefusal {
    /// Bound one candidate text and refuse anything sensitive.
    ///
    /// Returns `None` for text that is empty after trimming, or that the core
    /// sensitive-material rule rejects. Over-long input is **truncated to its
    /// tail** rather than refused: the newest bytes are the ones that can
    /// explain why a turn ended, and a refusal that arrived after a long answer
    /// would otherwise be cut off precisely when it matters.
    #[must_use]
    pub fn parse(text: &str, provenance: RefusalProvenance) -> Option<Self> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        // An input larger than any plausible refusal is refused outright rather
        // than scanned. This is what keeps the whole-candidate scan below
        // bounded: the adapter already caps what it collects, and a caller that
        // hands over more than this is not describing a turn's last words.
        if trimmed.len() > MAX_CANDIDATE_BYTES {
            return None;
        }
        // Scan the *whole* candidate, before any truncation.
        //
        // Truncating first was a hole: the tail is kept, so a candidate whose
        // sensitive marker sat in the discarded head — `authorization: Bearer`
        // followed by two thousand characters — passed the scan while the
        // secret itself survived into the bounded value and its digest. The
        // marker and the material it guards are not required to be adjacent, so
        // the only sound order is scan-then-bound.
        //
        // The path is structural and the candidate is never echoed, so a canary
        // in the text cannot reach a log through the error either.
        reject_sensitive_text("runtime.transient_refusal", trimmed).ok()?;
        let bounded: String = if trimmed.chars().count() > MAX_REFUSAL_CHARS {
            let skip = trimmed.chars().count() - MAX_REFUSAL_CHARS;
            trimmed.chars().skip(skip).collect()
        } else {
            trimmed.to_owned()
        };
        Some(Self {
            text: bounded,
            provenance,
        })
    }

    /// The bounded text, for classification only.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Which exact item this text came from.
    #[must_use]
    pub const fn provenance(&self) -> &RefusalProvenance {
        &self.provenance
    }

    /// A digest of the bounded text **and the item it came from**.
    ///
    /// This is the only part of a refusal that may be persisted. Covering the
    /// canonical position and native type as well as the prose is what lets a
    /// succession prove the quota row came from the predecessor's own terminal
    /// response — a digest of prose alone would be satisfied by the same
    /// sentence appearing anywhere, in any turn, on any seat.
    #[must_use]
    pub fn digest(&self) -> ContentHash {
        ContentHash::of(format!("{}\n{}", self.provenance.canonical(), self.text).as_bytes())
    }
}

/// Redacted on purpose: the length and digest are diagnosable, the text is not.
impl fmt::Debug for TransientRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransientRefusal")
            .field("chars", &self.text.chars().count())
            .field("run", &self.provenance.agent_run_id)
            .field("generation", &self.provenance.binding_generation)
            .field("position", &self.provenance.position)
            .field("sequence_end", &self.provenance.sequence_end)
            .field("item_type", &self.provenance.item_type)
            .field("digest", &self.digest().as_str())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn where_from() -> RefusalProvenance {
        RefusalProvenance {
            agent_run_id: AgentRunId::parse("01a0306f-9398-7a51-a612-8c2b58251d58").expect("a canonical run id"),
            binding_generation: 1,
            position: TimelinePosition {
                epoch: 1,
                sequence: 7,
            },
            sequence_end: 7,
            source_sequences: vec![(7, 7)],
            item_type: "assistant_message".to_owned(),
        }
    }

    #[test]
    fn empty_and_blank_text_is_not_a_refusal() {
        assert!(TransientRefusal::parse("", where_from()).is_none());
        assert!(TransientRefusal::parse("   \n\t ", where_from()).is_none());
    }

    #[test]
    fn a_sentence_is_carried_verbatim_for_classification() {
        let refusal = TransientRefusal::parse("  You've hit your usage limit.  ", where_from())
            .expect("a non-sensitive sentence");
        assert_eq!(refusal.as_str(), "You've hit your usage limit.");
    }

    #[test]
    fn sensitive_material_is_refused_rather_than_carried() {
        assert!(TransientRefusal::parse("authorization: Bearer sk-abcdefghijklmnop", where_from()).is_none());
    }

    #[test]
    fn over_long_text_keeps_its_tail_where_a_refusal_lands() {
        let padding = "a".repeat(MAX_REFUSAL_CHARS);
        let refusal = TransientRefusal::parse(&format!("{padding} usage limit reached"), where_from())
            .expect("a bounded refusal");
        assert_eq!(refusal.as_str().chars().count(), MAX_REFUSAL_CHARS);
        assert!(refusal.as_str().ends_with("usage limit reached"));
    }

    /// The head-boundary hole: a sensitive marker in the *discarded* head with
    /// its secret in the *retained* tail. Truncating before scanning kept the
    /// secret and reported the value clean.
    #[test]
    fn a_sensitive_head_cannot_smuggle_its_secret_through_the_retained_tail() {
        let filler = "a".repeat(MAX_REFUSAL_CHARS);
        let candidate = format!("authorization: Bearer {filler} sk-livesecretvalue00");
        assert!(
            TransientRefusal::parse(&candidate, where_from()).is_none(),
            "the whole candidate is scanned, not just the part that survives",
        );
    }

    #[test]
    fn a_sensitive_tail_is_still_refused() {
        let filler = "a".repeat(MAX_REFUSAL_CHARS);
        let candidate = format!("{filler} authorization: Bearer sk-livesecretvalue00");
        assert!(TransientRefusal::parse(&candidate, where_from()).is_none());
    }

    #[test]
    fn an_absurdly_large_candidate_is_refused_rather_than_scanned() {
        let huge = "a".repeat(MAX_CANDIDATE_BYTES + 1);
        assert!(TransientRefusal::parse(&huge, where_from()).is_none());
    }

    #[test]
    fn debug_redacts_the_text_it_carries() {
        let refusal = TransientRefusal::parse("You've hit your usage limit.", where_from()).expect("a refusal");
        let rendered = format!("{refusal:?}");
        assert!(!rendered.contains("usage limit"));
        assert!(rendered.contains("chars"));
        assert!(rendered.contains("digest"));
    }

    #[test]
    fn the_same_sentence_digests_the_same_way() {
        let first = TransientRefusal::parse("You've hit your usage limit.", where_from()).expect("a refusal");
        let second = TransientRefusal::parse("You've hit your usage limit.", where_from()).expect("a refusal");
        assert_eq!(first.digest(), second.digest());
    }
}
