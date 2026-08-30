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

use kontor_core::id::{ContentHash, reject_sensitive_text};

/// The most text a transient refusal may carry.
///
/// A provider refusal is a sentence, not a transcript. This bound is what stops
/// an adapter turning "the turn ended oddly" into an unbounded copy of the
/// session, and it is enforced here rather than trusted to each caller.
pub const MAX_REFUSAL_CHARS: usize = 2_000;

/// Text a runtime appears to have ended a turn with.
///
/// Construct with [`TransientRefusal::parse`]. There is no other constructor,
/// and no way to recover the text except [`TransientRefusal::as_str`].
#[derive(Clone, PartialEq, Eq)]
pub struct TransientRefusal {
    text: String,
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
    pub fn parse(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        let bounded: String = if trimmed.chars().count() > MAX_REFUSAL_CHARS {
            let skip = trimmed.chars().count() - MAX_REFUSAL_CHARS;
            trimmed.chars().skip(skip).collect()
        } else {
            trimmed.to_owned()
        };
        // The path is structural and the candidate is never echoed, so a canary
        // in the text cannot reach a log through the error either.
        reject_sensitive_text("runtime.transient_refusal", &bounded).ok()?;
        Some(Self { text: bounded })
    }

    /// The bounded text, for classification only.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// A digest of the bounded text.
    ///
    /// This is the only part of a refusal that may be persisted: it lets an
    /// operator prove two observations carried the same sentence without the
    /// store ever holding one.
    #[must_use]
    pub fn digest(&self) -> ContentHash {
        ContentHash::of(self.text.as_bytes())
    }
}

/// Redacted on purpose: the length and digest are diagnosable, the text is not.
impl fmt::Debug for TransientRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransientRefusal")
            .field("chars", &self.text.chars().count())
            .field("digest", &self.digest().as_str())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_blank_text_is_not_a_refusal() {
        assert!(TransientRefusal::parse("").is_none());
        assert!(TransientRefusal::parse("   \n\t ").is_none());
    }

    #[test]
    fn a_sentence_is_carried_verbatim_for_classification() {
        let refusal = TransientRefusal::parse("  You've hit your usage limit.  ")
            .expect("a non-sensitive sentence");
        assert_eq!(refusal.as_str(), "You've hit your usage limit.");
    }

    #[test]
    fn sensitive_material_is_refused_rather_than_carried() {
        assert!(TransientRefusal::parse("authorization: Bearer sk-abcdefghijklmnop").is_none());
    }

    #[test]
    fn over_long_text_keeps_its_tail_where_a_refusal_lands() {
        let padding = "a".repeat(MAX_REFUSAL_CHARS);
        let refusal = TransientRefusal::parse(&format!("{padding} usage limit reached"))
            .expect("a bounded refusal");
        assert_eq!(refusal.as_str().chars().count(), MAX_REFUSAL_CHARS);
        assert!(refusal.as_str().ends_with("usage limit reached"));
    }

    #[test]
    fn debug_redacts_the_text_it_carries() {
        let refusal = TransientRefusal::parse("You've hit your usage limit.").expect("a refusal");
        let rendered = format!("{refusal:?}");
        assert!(!rendered.contains("usage limit"));
        assert!(rendered.contains("chars"));
        assert!(rendered.contains("digest"));
    }

    #[test]
    fn the_same_sentence_digests_the_same_way() {
        let first = TransientRefusal::parse("You've hit your usage limit.").expect("a refusal");
        let second = TransientRefusal::parse("You've hit your usage limit.").expect("a refusal");
        assert_eq!(first.digest(), second.digest());
    }
}
