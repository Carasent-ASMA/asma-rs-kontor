# Test fixtures (data only)

This directory holds test data for the Kontor workspace (KON-MVP-18 pilot
fixtures and contract fixtures).

It is deliberately **not** a Cargo workspace member: data lives here, test
crates live in `../contract` and `../e2e`. Do not add a `Cargo.toml` or source
files to this directory; fixtures must not be compiled.
