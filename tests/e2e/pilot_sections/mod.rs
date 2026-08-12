//! The pilot's sections, one module per part of the KON-MVP-18 brief.
//!
//! Each section takes the shared [`kontor_tests_e2e::Bundle`] and answers the
//! criteria it owns. Sections never assert their way out of the driver: a
//! refuted claim is recorded as a failing case so the run still produces a
//! bundle, and `pilot` fails at the end on the count. The only `assert!`s here
//! are on driver invariants — a fixture that will not parse, a helper misused —
//! because those are bugs in the proof rather than findings about the tree.

pub(crate) mod domain;
pub(crate) mod fixture;
pub(crate) mod gates;
pub(crate) mod project;
pub(crate) mod runtime;
pub(crate) mod scheduling;
pub(crate) mod session;
pub(crate) mod ui;
