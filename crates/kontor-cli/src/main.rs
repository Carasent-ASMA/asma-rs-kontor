//! `kontor` — the Kontor control plane command-line interface.
//!
//! Scaffold placeholder created by KON-MVP-02. KON-MVP-16 implements the full
//! command surface; until then this binary only supports the deterministic
//! `--version` flag (one line, immediate exit, no listeners, no child
//! processes).

use clap::Parser;

/// Kontor control plane command-line interface.
#[derive(Parser)]
#[command(name = "kontor", version, about, long_about = None)]
struct Cli {}

fn main() {
    Cli::parse();
}
