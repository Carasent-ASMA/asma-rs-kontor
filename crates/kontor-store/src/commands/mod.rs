//! Command intent, durable claiming, and recovery after a crash.
//!
//! The rule the whole module exists for: **a process that restarts must never
//! discover a launch by guessing.** Between recording an intent and hearing a
//! native confirmation there are eight places a daemon can die, and at seven of
//! them a command may already have taken effect. So every step writes what the
//! next one will need *before* it needs it — the correlation before the native
//! call, the transition before the acknowledgement — and recovery reads that
//! record rather than the clock, the lease or the fact that the process is young.
//!
//! [`receipts::CommandRecovery`] is where that shows up: exactly one of its
//! variants authorizes a launch, and it is the one that proves nothing was ever
//! sent.

pub(crate) mod intent;
pub(crate) mod receipts;
