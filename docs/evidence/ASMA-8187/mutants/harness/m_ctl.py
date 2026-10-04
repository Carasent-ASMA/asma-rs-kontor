import pathlib
P = pathlib.Path("crates/kontor-daemon/src/main.rs")
s = P.read_text()
old = """    let realm = kontor_store::SqliteStore::read_existing_realm(&recovery::database_in(&root))
        .map_err(|_| OperatorError::CredentialScope)?;"""
new = """    // MUTANT M-CTL: refuse every Realm at the schema boundary.
    if true {
        return Err(OperatorError::CredentialScope);
    }
    let realm = kontor_store::SqliteStore::read_existing_realm(&recovery::database_in(&root))
        .map_err(|_| OperatorError::CredentialScope)?;"""
assert s.count(old) == 1
P.write_text(s.replace(old, new, 1))
