//! The control-plane event log: append, deduplicate, gap-detect and replay.
//!
//! One cursor space, allocated by SQLite inside the writing transaction, is the
//! spine of the whole control plane. Everything else in this module exists to
//! keep three questions apart that look alike and are not:
//!
//! * *Did we miss a local position?* — a paging question, answered by cursors.
//! * *Did the runtime skip one of its own control facts?* — a
//!   [`types::ControlGap`], answered by native sequences.
//! * *Is a transcript incomplete?* — a
//!   [`types::ContentGapOutcome::TimelineRefetchRequired`], answered by content
//!   epochs, which changes no state at all.
//!
//! Collapsing any of the three into another is how a control plane starts
//! inventing certainty it does not have.

pub(crate) mod append;
pub(crate) mod replay;
pub(crate) mod types;
