//! The `kontor` executable.
//!
//! Everything reusable is in the library, so this file is only the two things a
//! binary owns: parsing the command line and turning an exit class into an exit
//! code.
//!
//! `--version` still does what KON-MVP-02 promised — one line, immediate exit, no
//! socket and no child process — because clap answers it before any of this runs.

use clap::Parser;
use kontor_cli::args::Cli;
use kontor_cli::commands;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // A parse failure is clap's own message on standard error and its own exit
    // code; `Cli::parse` handles both, and a CLI syntax error is exit 2 there.
    let cli = Cli::parse();
    std::process::ExitCode::from(commands::run(&cli).await.code())
}
