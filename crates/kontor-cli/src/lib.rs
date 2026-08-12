//! `kontor` — the command-line interface to one Kontor Realm.
//!
//! # What it can reach
//!
//! One loopback `/v1` contract, with one tier secret read from one Realm's `0600`
//! credential file. It does not open the control-plane database, does not run the
//! scheduler, does not instantiate a runtime adapter and does not execute `asma`.
//! Every one of those is a decision the daemon owns, and the crate graph is the
//! enforcement: below this crate there is only `kontor-mcp` — the narrow client and
//! the operation catalogue — and `kontor-core`, which is pure domain vocabulary.
//!
//! # The output contract
//!
//! Exactly one JSON value on standard output, diagnostics on standard error, and an
//! exit code that is a *class* a script can branch on:
//!
//! | Code | Meaning |
//! | --- | --- |
//! | 0 | it worked, including an idempotent replay and a valid dry run |
//! | 1 | something answered, but not this contract |
//! | 2 | the command line, or this machine's configuration |
//! | 3 | the Realm would not authenticate or authorize the caller |
//! | 4 | the caller's state is stale: read again, then retry |
//! | 5 | a dependency is not ready; retry unchanged |
//! | 6 | it does not exist, or this runtime cannot do it |
//!
//! A success carries `{schema_version, realm_id, command, data, receipt}` and nests
//! the daemon's own documents inside `data` or `receipt` **unchanged**. A failure
//! prints the daemon's own `ApiErrorBody`, also unchanged — this crate reads `code`
//! out of it to pick an exit class and rewrites nothing, because the revision a
//! caller is owed lives in that body and a renamed code would be a second contract.
//!
//! # Where the surface is defined
//!
//! Not here. `kontor_mcp::tools` is the single catalogue of operations, their
//! authorities and their operands, and [`args`] only maps subcommands onto it. The
//! same catalogue is what `kontor mcp` serves, so the CLI and the tool surface
//! cannot disagree about what exists.

pub mod args;
pub mod client;
pub mod commands;
pub mod output;
