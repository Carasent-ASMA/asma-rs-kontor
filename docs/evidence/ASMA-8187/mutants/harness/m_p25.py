import pathlib
P = pathlib.Path("crates/kontor-store/src/repository.rs")
s = P.read_text()
block = """        if let Some(bound) = bound {
            transaction.rollback().map_err(backend)?;
            return if bound == receipt_id.to_string() {
                Ok(Applied::Unchanged)
            } else {
                Err(RepositoryError::Conflict {
                    subject: "core team route succession",
                    rule: "the succession is already bound to another receipt",
                })
            };
        }"""
assert s.count(block) == 1
s = s.replace(block, "", 1)
anchor = "        // A foreign key proves the receipt exists; it says nothing about whose"
assert s.count(anchor) == 1
P.write_text(s.replace(anchor, "        // MUTANT M-P25: an identical binding answers before anything is proved.\n" + block + "\n" + anchor, 1))
